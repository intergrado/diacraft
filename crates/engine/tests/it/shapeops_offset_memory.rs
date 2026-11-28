//! Offset and Fit Curve in an address space that is bounded (SHAPEOPS-CAPS): a crafted drawing is finished or refused in words, and the process is not aborted by an
//! allocation it cannot make. Each scenario runs in a child of this test binary under `ulimit -v 3000000`, as `shapeops_trim_memory.rs` and `vsdx_memory.rs` do, since a
//! test that is killed for its memory is no test result.
//!
//! The path library keeps a sweep's memory in step with the crossings of the paths it is given (a star of many vertices), and a command that makes thousands of shapes holds
//! each one's rows. These scenarios are the largest inputs a command takes of each kind, and the ones that are refused.

use std::process::Command;

use serde_json::json;

use crate::shapeops::{Build, ids, line, rect, session_with};

/// The address space the child may take, in KiB: the engine's own tests of memory take 3,000,000.
const LIMIT_KIB: u64 = 3_000_000;

const SCENARIO: &str = "DIACRAFT_SHAPEOPS_SCENARIO";

/// The peak of the address space of this process, in KiB.
fn vm_peak_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status.lines().find(|l| l.starts_with("VmPeak")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// A closed star of `n` vertices, each joined to the one `step` places on, as one shape.
fn star(n: u32, step: u32) -> diacraft_doc::Shape {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from((step * i) % n) / f64::from(n);
            (5.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin())
        })
        .collect();
    Build::new(1, 10.0, 10.0).pin(5.0, 5.0).section(&points, true, false).build()
}

/// The scenarios the child runs: each prints a line that starts with `RESULT`.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("a scenario");
    let (command, shapes, params) = match scenario.as_str() {
        // The most copies a command makes, from 5,000 lines in 50 shapes of 100 sections.
        "lines10000" => {
            let shapes: Vec<_> = (0..50_u32)
                .map(|k| {
                    let mut b = Build::new(1 + k, 1.0, 100.0).pin(f64::from(k) * 10.0 + 0.5, 50.0);
                    for j in 0..100_u32 {
                        b = b.section(&[(0.0, f64::from(j)), (1.0, f64::from(j))], false, true);
                    }
                    b.build()
                })
                .collect();
            ("shape.offset", shapes, json!({"ids": (1..=50_u32).collect::<Vec<_>>(), "distance": 0.1}))
        }
        // Many shapes of one line each that lie across one another's heights.
        "lines4096" => {
            let shapes: Vec<_> = (0..4_096_u32).map(|i| line(i + 1, (f64::from(i) * 0.1, 0.0), (f64::from(i) * 0.1, 0.05)).build()).collect();
            ("shape.offset", shapes, json!({"ids": (1..=4_096_u32).collect::<Vec<_>>(), "distance": 0.01}))
        }
        // Stars that cross themselves in very many places.
        "star501" => ("shape.offset", vec![star(501, 250)], json!({"ids": [1], "distance": 0.25})),
        "star2001" => ("shape.offset", vec![star(2_001, 1_000)], json!({"ids": [1], "distance": 0.25})),
        // A path of 25 million crossings: 5,000 horizontal passes and then 5,000 vertical ones, in one open path.
        "serpentine25m" => {
            let n = 5_000_u32;
            let mut points: Vec<(f64, f64)> = Vec::new();
            for i in 0..n {
                let y = f64::from(i) * 0.01;
                points.push(if i % 2 == 0 { (0.0, y) } else { (50.0, y) });
                points.push(if i % 2 == 0 { (50.0, y) } else { (0.0, y) });
            }
            for j in 0..n {
                let x = f64::from(j) * 0.01;
                points.push(if j % 2 == 0 { (x, 0.0) } else { (x, 50.0) });
                points.push(if j % 2 == 0 { (x, 50.0) } else { (x, 0.0) });
            }
            (
                "shape.offset",
                vec![Build::new(1, 50.0, 50.0).pin(25.0, 25.0).section(&points, false, true).build()],
                json!({"ids": [1], "distance": 0.001}),
            )
        }
        // A closed polygon of many vertices.
        "polygon50000" => {
            let n = 50_000_u32;
            let points: Vec<(f64, f64)> = (0..n)
                .map(|i| {
                    let a = f64::from(i) * std::f64::consts::TAU / f64::from(n);
                    (10.0 + 10.0 * a.cos(), 10.0 + 10.0 * a.sin())
                })
                .collect();
            (
                "shape.offset",
                vec![Build::new(1, 20.0, 20.0).pin(10.0, 10.0).section(&points, true, false).build()],
                json!({"ids": [1], "distance": 0.5}),
            )
        }
        // Fit Curve of a sine wave of 100,000 rows, and of the circles flag on a circle of 100,000 points.
        "sine100000" => {
            let points: Vec<(f64, f64)> = (0..100_000_u32).map(|i| (f64::from(i) * 0.001, 1.0 + (f64::from(i) * 0.01).sin())).collect();
            (
                "shape.fit_curve",
                vec![Build::new(1, 100.0, 2.0).pin(50.0, 1.0).section(&points, false, true).build()],
                json!({"ids": [1], "tolerance": 0.01, "flags": 7}),
            )
        }
        "circle100000" => {
            let n = 100_000_u32;
            let points: Vec<(f64, f64)> = (0..n)
                .map(|i| {
                    let a = f64::from(i) * std::f64::consts::TAU / f64::from(n);
                    (10.0 + 10.0 * a.cos(), 10.0 + 10.0 * a.sin())
                })
                .collect();
            (
                "shape.fit_curve",
                vec![Build::new(1, 20.0, 20.0).pin(10.0, 10.0).section(&points, true, false).build()],
                json!({"ids": [1], "tolerance": 0.0001, "flags": 2}),
            )
        }
        // 4,096 circles of 36 lines each.
        "circles4096" => {
            let shapes: Vec<_> = (0..4_096_u32)
                .map(|i| {
                    let (cx, cy) = (f64::from(i % 64) * 3.0, f64::from(i / 64) * 3.0);
                    let points: Vec<(f64, f64)> = (0..36)
                        .map(|k| {
                            let a = f64::from(k) * 10.0_f64.to_radians();
                            (1.0 + a.cos(), 1.0 + a.sin())
                        })
                        .collect();
                    Build::new(i + 1, 2.0, 2.0).pin(cx + 1.0, cy + 1.0).section(&points, true, false).build()
                })
                .collect();
            ("shape.fit_curve", shapes, json!({"ids": (1..=4_096_u32).collect::<Vec<_>>(), "tolerance": 0.02, "flags": 2}))
        }
        "squares4096" => {
            let shapes: Vec<_> = (0..4_096_u32).map(|i| rect(i + 1, (f64::from(i) * 3.0, 0.0, f64::from(i) * 3.0 + 2.0, 2.0)).build()).collect();
            ("shape.offset", shapes, json!({"ids": (1..=4_096_u32).collect::<Vec<_>>(), "distance": 0.25}))
        }
        other => panic!("no scenario {other}"),
    };
    let mut s = session_with(shapes);
    let started = std::time::Instant::now();
    let result = s.execute(command, &params);
    let took = started.elapsed();
    let what = match &result {
        Ok(done) => format!("ok made={} changed={}", ids(done, "ids").len(), ids(done, "changed").len()),
        Err(error) => format!("refused {error}"),
    };
    println!("\nRESULT {what} took={took:?} VmPeak={}KiB", vm_peak_kib());
}

/// Runs the child's `scenario` under `ulimit -v`: whether it ended well, and what it printed.
fn run_child(scenario: &str) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {LIMIT_KIB} && exec \"$0\" --exact shapeops_offset_memory::child --ignored --nocapture --test-threads=1");
    let out =
        Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).env("DIACRAFT_NO_PREFS", "1").output().expect("run /bin/sh");
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
    eprintln!("{scenario}: {line}");
    line
}

/// The child ended well, with the command made or refused: either is an answer, and neither is an abort.
fn assert_ends_well_either_way(scenario: &str) -> String {
    let (ended_well, stdout) = run_child(scenario);
    assert!(ended_well, "the child `{scenario}` aborted under `ulimit -v {LIMIT_KIB}`: {stdout}");
    let line = result_line(&stdout);
    assert!(line.starts_with("RESULT ok") || line.starts_with("RESULT refused"), "{scenario}: {line}");
    assert!(peak(&line) < LIMIT_KIB, "{scenario}: the address space reached {} KiB of the {LIMIT_KIB}: {line}", peak(&line));
    eprintln!("{scenario}: {line}");
    line
}

#[test]
fn the_most_copies_a_command_makes_are_made_in_a_bounded_address_space() {
    assert_ends_well("lines10000", "ok made=10000");
}

#[test]
fn four_thousand_ninety_six_shapes_of_one_line_each_are_offset_in_a_bounded_address_space() {
    assert_ends_well("lines4096", "ok made=8192");
}

#[test]
fn four_thousand_ninety_six_rectangles_are_offset_in_a_bounded_address_space() {
    assert_ends_well("squares4096", "ok made=8192");
}

#[test]
fn stars_and_combs_that_cross_in_very_many_places_end_in_a_bounded_address_space() {
    for scenario in ["star501", "star2001", "serpentine25m"] {
        assert_ends_well_either_way(scenario);
    }
}

#[test]
fn a_polygon_of_50000_vertices_is_offset_or_refused_in_a_bounded_address_space() {
    assert_ends_well_either_way("polygon50000");
}

#[test]
fn fit_curve_of_long_polylines_and_of_many_circles_ends_in_a_bounded_address_space() {
    for scenario in ["sine100000", "circle100000"] {
        assert_ends_well_either_way(scenario);
    }
    assert_ends_well("circles4096", "ok made=0 changed=4096");
}
