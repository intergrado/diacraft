//! What a scene holds, counted: each price pinned and the sum of a scene's parts, an ignored measurement of what a scene really holds, and a scene at its caps built in a child process under
//! `ulimit -v 3000000`, which refuses nothing and aborts nothing (the lesson).

use std::sync::Arc;

use diacraft_doc::{Document, Shape};

use super::build::{boxed, glue_both, line, ours, page, placeable};
use super::plenty;
use crate::draw::route_scene::price::{Parts, price};
use crate::draw::route_scene::{MAX_SCENE_LINES, MAX_SCENE_SHAPES, Scene};

/// Each price on its own, and the sum: the numbers are measured (the ignored `measure_what_a_scene_holds`), and a change of one is a change of these.
#[test]
fn each_price_is_pinned_and_they_add_up() {
    let one = |f: &dyn Fn(&mut Parts)| {
        let mut parts = Parts::default();
        f(&mut parts);
        price(&parts)
    };
    assert_eq!(price(&Parts::default()), 0, "an empty scene holds nothing");
    let prices = [
        ("a top-level shape", one(&|p| p.tops = 1)),
        ("a member of a group", one(&|p| p.members = 1)),
        ("a candidate", one(&|p| p.candidates = 1)),
        ("an obstacle of the router", one(&|p| p.obstacles = 1)),
        ("a connector", one(&|p| p.lines = 1)),
        ("a leg the scene keeps", one(&|p| p.legs = 1)),
        ("a wire of the router", one(&|p| p.wires = 1)),
        ("a leg of a wire", one(&|p| p.wire_legs = 1)),
        ("an end's glue", one(&|p| p.glue_from = 1)),
        ("a glued shape", one(&|p| p.glue_to = 1)),
    ];
    let want = [
        ("a top-level shape", 128),
        ("a member of a group", 8),
        ("a candidate", 256),
        ("an obstacle of the router", 336),
        ("a connector", 128),
        ("a leg the scene keeps", 48),
        ("a wire of the router", 96),
        ("a leg of a wire", 112),
        ("an end's glue", 64),
        ("a glued shape", 40),
    ];
    assert_eq!(prices.to_vec(), want.to_vec());
    let all = Parts { tops: 2, members: 3, candidates: 5, obstacles: 7, lines: 11, legs: 13, wires: 17, wire_legs: 19, glue_from: 23, glue_to: 29 };
    let sum: u64 = 2 * 128 + 3 * 8 + 5 * 256 + 7 * 336 + 11 * 128 + 13 * 48 + 17 * 96 + 19 * 112 + 23 * 64 + 29 * 40;
    assert_eq!(price(&all), sum, "each is counted at its own price and they add");
    let huge = Parts { tops: u64::MAX, candidates: u64::MAX, ..Parts::default() };
    assert_eq!(price(&huge), u64::MAX, "a count past the range saturates and does not wrap");
}

/// What a scene is charged is the counts of what it keeps: a page of boxes and connectors, then one shape more.
#[test]
fn a_scene_is_charged_by_what_it_keeps() {
    let mut shapes =
        vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), placeable(2, (6.0, 0.0, 1.0, 1.0)), boxed(3, (9.0, 0.0, 1.0, 1.0)).cell("ObjType", "4")];
    shapes.push(ours(line(10, &[(1.0, 0.5), (3.0, 0.5), (3.0, 2.0), (6.0, 2.0)])));
    let doc = page(shapes, glue_both(10, 1, 2));
    let scene = Scene::read(&doc, 1, &mut plenty());
    let parts = scene.parts();
    assert_eq!(
        parts,
        Parts { tops: 4, members: 0, candidates: 3, obstacles: 2, lines: 1, legs: 3, wires: 1, wire_legs: 3, glue_from: 1, glue_to: 2 },
        "four shapes, three of them two-dimensional, two obstacles, one connector of three legs, and its glue"
    );
    assert_eq!(scene.held_bytes(), price(&parts));
    let empty = Scene::new(1);
    assert_eq!(empty.held_bytes(), 0, "a scene that has read nothing holds nothing");
    let bigger = Scene::read(&page((1..=50).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect(), Vec::new()), 1, &mut plenty());
    assert!(bigger.held_bytes() > scene.held_bytes(), "more shapes, more bytes");
}

/// The process's anonymous resident memory in bytes (`RssAnon`): what the heap holds, which is what the charge is for.
fn anon_bytes() -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines().find_map(|l| l.strip_prefix("RssAnon:")).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0)
        * 1024
}

/// A document of `boxes` placeable boxes and `lines` connectors that all hold the sheet of one (two rows, a route of one leg), so that the document is the shapes and not the rows.
fn crowd(boxes: u32, lines: u32) -> Document {
    crowd_of(boxes, lines, "1", true)
}

/// [`crowd`] with the `ObjType` the boxes hold (4 makes them candidates and no obstacle) and whether the connectors are ours (a plain line is no wire).
fn crowd_of(boxes: u32, lines: u32, objtype: &str, wired: bool) -> Document {
    let mut shapes: Vec<_> =
        (1..=boxes).map(|k| boxed(k, (f64::from((k - 1) % 100) * 3.0, f64::from((k - 1) / 100) * 3.0, 1.0, 1.0)).cell("ObjType", objtype)).collect();
    let route = line(boxes + 1, &[(0.0, -5.0), (4.0, -5.0)]);
    shapes.push(if wired { ours(route) } else { route });
    let mut doc = page(shapes, Vec::new());
    let first = doc.page(1).unwrap().shapes.iter().find(|s| s.id == boxes + 1).unwrap().clone();
    let page = crate::draw::page_mut(&mut doc, 1).unwrap();
    for k in 1..lines {
        page.shapes.push(Arc::new(Shape { id: boxes + 1 + k, ..Shape::clone(&first) }));
    }
    doc
}

/// A measurement, not a test: what a scene of a page of `SCENE_MEASURE_BOXES` boxes and `SCENE_MEASURE_LINES` connectors holds by the process's resident memory, against what it is charged, over
/// scenes made one after another in one process (the first leaves out the heap's own growth). Run with `--ignored --nocapture`; the prices of `price.rs` were set from it.
#[test]
#[ignore = "a measurement, not a test: SCENE_MEASURE_BOXES=<n> SCENE_MEASURE_LINES=<n> prints bytes a scene holds and is charged; run with --ignored --nocapture"]
fn measure_what_a_scene_holds() {
    let var = |name: &str, default: u32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    let (boxes, lines) = (var("SCENE_MEASURE_BOXES", 10_000), var("SCENE_MEASURE_LINES", 10_000));
    // SCENE_MEASURE_OBJTYPE=4 measures candidates that are no obstacle, SCENE_MEASURE_PLAIN=1 connectors that are plain lines (no wire).
    let objtype = std::env::var("SCENE_MEASURE_OBJTYPE").unwrap_or_else(|_| "1".to_owned());
    let doc = crowd_of(boxes, lines, &objtype, std::env::var("SCENE_MEASURE_PLAIN").is_err());
    let mut kept = Vec::new();
    let (mut real, mut counted) = (0u64, 0u64);
    for k in 0..5 {
        let before = anon_bytes();
        let scene = Scene::read(&doc, 1, &mut plenty());
        let after = anon_bytes();
        if k > 0 {
            real += after.saturating_sub(before);
            counted += 1;
        }
        eprintln!("MEASURE scene {k}: parts {:?}", scene.parts());
        kept.push(scene);
    }
    let charged = kept.last().map_or(0, Scene::held_bytes);
    eprintln!(
        "MEASURE boxes={boxes} lines={lines}: a scene holds {:.1} MiB by RssAnon and is charged {:.1} MiB ({:.2} charged for each held)",
        real as f64 / counted as f64 / 1_048_576.0,
        charged as f64 / 1_048_576.0,
        charged as f64 / (real as f64 / counted as f64).max(1.0)
    );
}

/// **The charge is over what a scene holds**, for the dearest page the caps allow of each kind. Run by hand (what a process holds depends on its allocator); see the measurement above.
#[test]
#[ignore = "resident-memory measurement; allocator-dependent"]
fn a_scene_is_charged_more_than_it_holds_in_memory() {
    for (boxes, lines, objtype, wired) in
        [(10_000, 0, "1", true), (10_000, 0, "4", true), (0, 20_000, "1", true), (0, 20_000, "1", false), (10_000, 10_000, "1", true)]
    {
        let doc = crowd_of(boxes, lines, objtype, wired);
        let mut kept = Vec::new();
        let (mut real, mut counted) = (0u64, 0u64);
        for k in 0..5 {
            let before = anon_bytes();
            let scene = Scene::read(&doc, 1, &mut plenty());
            let after = anon_bytes();
            if k > 0 {
                real += after.saturating_sub(before);
                counted += 1;
            }
            kept.push(scene);
        }
        if real == 0 {
            return;
        }
        let held = real as f64 / counted as f64;
        let charged = kept.last().map_or(0, Scene::held_bytes) as f64;
        assert!(
            held <= charged,
            "{boxes} boxes of {objtype} and {lines} connectors (wired: {wired}): a scene holds {:.1} MiB and is charged {:.1} MiB",
            held / 1_048_576.0,
            charged / 1_048_576.0
        );
    }
}

const SCENARIO: &str = "ROUTE_SCENE_MEMORY_SCENARIO";

/// What the child does: a scene at one of its caps, put right again after an edit.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn route_scene_memory_child() {
    match std::env::var(SCENARIO).unwrap_or_default().as_str() {
        // The shapes cap: 10,000 two-dimensional shapes, and 100 routes of 4,000 rows that share one sheet: the rows cap.
        "caps" => {
            let mut doc = crowd(MAX_SCENE_SHAPES as u32, 1);
            let long: Vec<(f64, f64)> = (0..4_000).map(|k| (f64::from(k) * 0.001, -10.0)).collect();
            let seed = super::build::made(ours(line(900_000, &long)));
            let page = crate::draw::page_mut(&mut doc, 1).unwrap();
            for k in 0..100 {
                page.shapes.push(Arc::new(Shape { id: 900_000 + k, ..Shape::clone(&seed) }));
            }
            let mut scene = Scene::read(&doc, 1, &mut plenty());
            println!("SCENE caps: held {} bytes, parts {:?}, cut {:?}", scene.held_bytes(), scene.parts(), scene.cut());
            assert!(scene.is_complete() && scene.wires().len() >= 100);
            assert!(scene.held_bytes() < 256 * 1024 * 1024, "a scene at its caps is charged {} bytes", scene.held_bytes());
            let mut next = doc.clone();
            super::build::shift(&mut next, 5, 0.0, 0.5);
            scene.put_right(&next, 1, &mut plenty());
            assert!(scene.is_complete());
        }
        // The connectors cap: 100,000 connectors that share one sheet, and a few boxes.
        "lines" => {
            let doc = crowd(100, MAX_SCENE_LINES as u32);
            let scene = Scene::read(&doc, 1, &mut plenty());
            println!("SCENE lines: held {} bytes, parts {:?}, cut {:?}", scene.held_bytes(), scene.parts(), scene.cut());
            assert!(scene.is_complete() && scene.wires().len() == MAX_SCENE_LINES);
            assert!(scene.held_bytes() < 256 * 1024 * 1024, "a scene at its caps is charged {} bytes", scene.held_bytes());
        }
        other => panic!("no scenario {other}"),
    }
    println!("scene: no abort");
}

#[cfg(target_os = "linux")]
fn run(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script =
        "ulimit -v 3000000 && exec \"$0\" --exact draw::route_scene::tests::memory::route_scene_memory_child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("scene: no abort"), "{scenario}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("SCENE")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn a_scene_at_the_shape_and_row_caps_is_built_and_put_right_under_a_limit_on_address_space() {
    run("caps");
}

#[test]
#[cfg(target_os = "linux")]
fn a_scene_at_the_cap_on_connectors_is_built_under_a_limit_on_address_space() {
    run("lines");
}
