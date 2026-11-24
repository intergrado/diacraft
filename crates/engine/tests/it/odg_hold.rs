//! What the engine holds beside the drawing an `.odg` save is given, and what flattening costs across saves.
//! The cases that claim a memory bound run in a **child process under `ulimit -v 3000000`**, as `odg_memory.rs` does: a process that is out of memory
//! aborts, and a child that aborts fails the parent. Each ends as a save or as a refusal in words, never as an abort.
//!
//! The inputs are the review's: eight groups of 20,000 rectangles and 175,000 loose ones with four rounds of `shape.format` on the groups (32 commands; the save
//! aborted with "memory allocation of 1160 bytes failed"), a group of 175,000 members whose members were written six times (aborted), and a save, an undo and a small
//! edit four times over a group of 175,000 (the pages the index holds alone: aborted); a file of 4,000 rectangles with 3,990 deleted and of 8,000 with 7,990
//! deleted (refused for "more than 0 bytes"); and 45,000 new rectangles saved and 10,000 more (refused: every save flattened every shape ever flattened).

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::odg_build::{Dir, Drawing};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, save};
use crate::run;

const SCENARIO: &str = "ODG_ENGINE_HOLD_SCENARIO";

fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

fn rect(k: usize) -> String {
    format!("<draw:rect draw:name=\"s{k}\" svg:x=\"{}.{}cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"/>", k % 997, k % 89 + 1)
}

/// A page of `n` rectangles.
fn rects_page(n: usize) -> Vec<u8> {
    Drawing::new().page(&(0..n).map(rect).collect::<String>()).bytes()
}

/// `groups` groups of `each` rectangles, then `top` rectangles at the top level.
fn grouped_many(groups: usize, each: usize, top: usize) -> Vec<u8> {
    let mut body = String::new();
    for g in 0..groups {
        body.push_str(&format!("<draw:g draw:name=\"G{g}\">"));
        for k in g * each..(g + 1) * each {
            body.push_str(&rect(k));
        }
        body.push_str("</draw:g>");
    }
    for k in groups * each..groups * each + top {
        body.push_str(&rect(k));
    }
    Drawing::new().page(&body).bytes()
}

/// Every member of the first group written, as a command that edits what a group holds does.
fn edit_members(s: &mut Session, round: usize) {
    s.edit("Test edit", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| !x.children.is_empty()).unwrap();
        let group = Arc::make_mut(page.shapes.get_mut(at).unwrap());
        for k in 0..group.children.len() {
            let member = Arc::make_mut(group.children.get_mut(k).unwrap());
            let sheet = Arc::make_mut(&mut member.sheet);
            let value = format!("{}", 1.0 + round as f64 * 0.01);
            sheet
                .cells
                .insert(diacraft_doc::CellName::new("PinX"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&value)).with_unit("CM"));
        }
        Ok(())
    })
    .unwrap();
}

/// What a save ended as: `saved`, or a refusal and its first words.
fn how(result: &Result<serde_json::Value, EngineError>) -> String {
    match result {
        Ok(v) => format!("saved(edited={})", v["edited"]),
        Err(e) => {
            let text = e.to_string();
            if text.contains("of memory") || text.contains("past a limit") || text.contains("more memory than a save may") {
                format!("refused(memory, {})", text.chars().take(160).collect::<String>())
            } else {
                format!("refused({text})")
            }
        }
    }
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    let edits: usize = std::env::var("ODG_EDITS").ok().and_then(|n| n.parse().ok()).unwrap_or(1);
    let dir = Dir::new();
    let open = |bytes: &[u8]| {
        let path = dir.write("a.odg", bytes);
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": path })).unwrap_or_else(|e| panic!("the file did not open: {e}"));
        s
    };
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let result = match scenario.as_str() {
        // Only commands: each group of 20,000 recoloured with `shape.format`, one command for each group, `edits` rounds.
        "format_groups" => {
            let bytes = grouped_many(8, 20_000, 175_000);
            let mut s = open(&bytes);
            drop(bytes);
            let groups: Vec<u32> =
                s.document().unwrap().pages.get(0).unwrap().shapes.iter().filter(|x| !x.children.is_empty()).map(|x| x.id).collect();
            for round in 0..edits {
                for g in &groups {
                    let colour = format!("#{:02x}00{:02x}", 40 + round * 30, 200 - round * 20);
                    s.execute("shape.format", &json!({ "ids": [*g], "fill": colour })).unwrap();
                }
            }
            s.execute("file.save", &json!({}))
        }
        // One group of 175,000 members, the members written `edits` times, and 175,000 loose rectangles.
        "group" => {
            let one = |k: usize| rect(k);
            let inner: String = (0..175_000).map(one).collect();
            let outer: String = (175_000..350_000).map(one).collect();
            let bytes = Drawing::new().page(&format!("<draw:g draw:name=\"G\">{inner}</draw:g>{outer}")).bytes();
            let mut s = open(&bytes);
            drop(bytes);
            for round in 0..edits {
                edit_members(&mut s, round);
            }
            s.execute("file.save", &json!({}))
        }
        // The members written and saved (the index holds the saved pages), the save undone and a small edit made (it drops the redo steps), and then the members
        // written `edits` times more: the pages of the save are held by the index alone, and the review's input (four of them aborted).
        "saved_pages" => {
            let inner: String = (0..175_000).map(rect).collect();
            let outer: String = (175_000..350_000).map(rect).collect();
            let bytes = Drawing::new().page(&format!("<draw:g draw:name=\"G\">{inner}</draw:g>{outer}")).bytes();
            let mut s = open(&bytes);
            drop(bytes);
            edit_members(&mut s, 0);
            let first = s.execute("file.save", &json!({}));
            assert!(first.is_ok() || how(&first).starts_with("refused(memory"), "{}", how(&first));
            s.execute("edit.undo", &json!({})).unwrap();
            let end = s.document().unwrap().pages.get(0).unwrap().shapes.iter().next_back().unwrap().id;
            s.execute("shape.move", &json!({ "ids": [end], "dx": 0.1, "dy": 0.0 })).unwrap();
            for round in 1..=edits {
                edit_members(&mut s, round);
            }
            s.execute("file.save", &json!({}))
        }
        other => panic!("no scenario {other}"),
    };
    println!("\nRESULT {scenario} edits={edits} class={} VmHWM={}KiB VmPeak={}KiB", how(&result), status_kib("VmHWM"), status_kib("VmPeak"));
    // A save is made, or refused for memory in words: never any other refusal.
    assert!(result.is_ok() || how(&result).starts_with("refused(memory"), "{}", how(&result));
}

/// Runs the child's `scenario` under `ulimit -v 3000000` and gives what it printed.
fn run_child(scenario: &str, edits: usize) -> String {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact odg_hold::child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .env(SCENARIO, scenario)
        .env("ODG_EDITS", edits.to_string())
        .output()
        .expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    assert!(
        out.status.success(),
        "the child `{scenario}` ({edits} edits) aborted, or its assertion failed: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    let line = stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned();
    eprintln!("{line}");
    line
}

/// **Group members are counted**: eight groups of 20,000 and 175,000 loose rectangles, `shape.format` on each group four rounds over, the review's
/// 32 commands. The save aborted at three rounds' margin; it is a save or a refusal in words.
#[test]
fn a_save_after_four_rounds_of_formatting_eight_groups_of_20000_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    let line = run_child("format_groups", 4);
    assert!(line.contains("class=saved") || line.contains("class=refused(memory"), "{line}");
}

/// **The members of one group are counted**: a group of 175,000 members written six times (four peaked at exactly the limit, six aborted).
#[test]
fn a_save_after_six_edits_of_every_member_of_a_group_of_175000_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    let line = run_child("group", 6);
    assert!(line.contains("class=refused(memory"), "six whole writes of 175,000 members do not fit beside the drawing: {line}");
}

/// **What only the index holds is counted**: the members of a group of 175,000 written and saved, the save undone and a small edit made, then the members
/// written four times more (the pages of the save are held by the index alone once the redo steps are dropped).
#[test]
fn a_save_an_undo_a_small_edit_and_four_more_writes_of_a_group_of_175000_ends_as_a_save_or_a_refusal_and_does_not_abort() {
    run_child("saved_pages", 4);
}

/// **A mass delete does not make the save impossible**: 4,000 rectangles with 3,990 deleted in one command, and 8,000 with 7,990 deleted in two. Each is
/// saved, and the file read again is the drawing. The history is priced by what the shapes held, and the refusal that said "more than 0 bytes" never comes.
#[test]
fn a_file_of_rectangles_with_nearly_all_deleted_saves_and_a_refusal_never_names_a_limit_of_nothing() {
    for (n, keep) in [(4_000usize, 10usize), (8_000, 10)] {
        let dir = Dir::new();
        let path = dir.write("a.odg", &rects_page(n));
        let mut s = Session::new();
        run(&mut s, "file.open", json!({ "path": path }));
        let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().skip(keep).map(|x| x.id).collect();
        for part in ids.chunks(4_000) {
            run(&mut s, "shape.delete", json!({ "ids": part }));
        }
        assert_eq!(s.document().unwrap().pages.get(0).unwrap().shapes.len(), keep, "{n}");
        let result = s.execute("file.save", &json!({}));
        let saved = result.unwrap_or_else(|e| panic!("{n} rectangles with {} deleted: the save was refused: {e}", n - keep));
        assert_eq!(saved["edited"], true, "{n}");
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// **A save does not flatten what an earlier save flattened**: 35,000 new rectangles saved, then 10,000 more, and both saves are made (the second was
/// refused with "save after adding fewer", advice that could not work: every save flattened every shape ever flattened, and the budget held the sum). The shapes
/// the first save flattened are given again, at a step each, and the file read again holds all 45,000. **It was 45,000 and then 10,000 until a later change** (
/// the `.odg` model at 64 bytes an item, where it was 54), whose gate refuses the second save of those (more than the 2,766 MiB a save may hold).
#[test]
fn rectangles_added_after_a_save_of_35000_are_saved_too_and_the_first_are_given_again_and_not_flattened_again() {
    let dir = Dir::new();
    let path = dir.write("a.odg", &rects_page(3));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let add = |s: &mut Session, count: usize| {
        let made = run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        s.edit("Test edit", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let template = page.shapes.iter().find(|x| x.id == id).unwrap().clone();
            for k in 1..count {
                let mut shape = (*template).clone();
                shape.id = id + u32::try_from(k).unwrap();
                page.shapes.push(Arc::new(shape));
            }
            Ok(())
        })
        .unwrap();
    };
    add(&mut s, 35_000);
    let first = save(&mut s);
    assert_eq!(first["edited"], true);
    add(&mut s, 10_000);
    let started = std::time::Instant::now();
    let second = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("10,000 more after 35,000: the save was refused: {e}"));
    let took = started.elapsed();
    assert_eq!(second["edited"], true);
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 3 + 35_000 + 10_000, "all the shapes are in the file");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    eprintln!("45,000 shapes saved in two saves, the second in {took:?}");
}

/// **What the save costs follows the drawing and the history once each** (the cost of the walk of what the engine holds): a save after 1,000 one-shape edits, of a
/// drawing four times as big, costs about four times, never the drawing again for each step of the history. Timed back to back, so that a loaded machine slows both.
#[test]
fn a_save_after_a_thousand_one_shape_edits_costs_the_drawing_and_the_history_once_each() {
    let time = |n: usize| {
        let dir = Dir::new();
        let path = dir.write("a.odg", &rects_page(n));
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": path })).unwrap();
        let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().step_by(n / 1_000).take(1_000).map(|x| x.id).collect();
        for id in ids {
            // A shape written as a command that writes one does (the commands themselves cost more than the save at this size, and are not what is timed).
            s.edit("Test edit", |doc| {
                let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
                let at = page.shapes.iter().position(|x| x.id == id).unwrap();
                let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
                let pin = Arc::make_mut(&mut shape.sheet).cells.get("PinX").and_then(|c| c.value.as_ref()?.number()).unwrap();
                let moved = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(&format!("{}", pin + 0.01))).with_unit("IN");
                Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new("PinX"), moved);
                Ok(())
            })
            .unwrap();
        }
        let started = std::time::Instant::now();
        let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("{n} shapes, 1,000 edits: refused: {e}"));
        assert_eq!(saved["edited"], true);
        started.elapsed()
    };
    let (small, large) = (time(20_000), time(80_000));
    eprintln!("1,000 edits then a save: 20,000 shapes {small:?}, 80,000 shapes {large:?}");
    assert!(large < small * 10 + std::time::Duration::from_secs(2), "four times the drawing, about four times the time: {small:?} then {large:?}");
}
