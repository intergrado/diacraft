//! What the `.odg` model writer costs in memory, in a child process under an address-space limit (`ulimit -v 3000000`), as `docfile_memory.rs` runs its own (the plan's
//! done-criterion D3). A process that is out of memory *aborts*, and a child that aborts fails the parent: a save that cannot have the memory must end as an error of a class, never as an
//! abort.
//!
//! * **Parts the model holds** (`Basic/`, objects, fonts, any package entry): the writer holds them in the package it makes, in the read of it, and in the package the splice makes. A
//!   model whose parts, by the lengths they state, are past what a save may hold is refused before anything is made; one under it is written.
//! * **A settings tree or kept fields past the cap of a small part** are refused by a bound that walks no more nodes than the cap, before a tree is made of them.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Document, Format, Part, PartRole, RawAttrs, RawElement, RawNode};
use diacraft_odg::{WriteOptions, WriteWork, write_counted};

use crate::model_write::blank;

const SCENARIO: &str = "ODG_MODEL_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// `megabytes` MiB of noise (a generator of its own, so that the bytes do not deflate): the bytes of a part that a ratio of a thousand would not refuse.
fn noise(megabytes: usize, seed: u64) -> Arc<[u8]> {
    let mut state = seed | 1;
    let mut bytes = vec![0u8; megabytes << 20];
    for chunk in bytes.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let word = state.to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
    Arc::from(bytes)
}

/// A drawing with `count` parts of `megabytes` MiB of noise each.
fn with_parts(count: usize, megabytes: usize) -> Document {
    let mut doc = blank(1);
    for k in 0..count {
        doc.parts.push(Arc::new(Part {
            format: Format::Odf,
            name: Arc::from(format!("Fonts/noise{k}.bin")),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Font,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: noise(megabytes, 0x9E37_79B9_7F4A_7C15 ^ k as u64),
        }));
    }
    doc
}

fn save(doc: &Document) -> (String, String, WriteWork, Option<Vec<u8>>) {
    let mut work = WriteWork::default();
    let saved = write_counted(doc, None, &WriteOptions::default(), &mut work);
    let class = saved.as_ref().err().map_or("saved".to_owned(), |e| e.class().to_owned());
    let words = saved.as_ref().err().map(ToString::to_string).unwrap_or_default();
    (class, words, work, saved.ok().map(|w| w.bytes))
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        "four_parts_of_200mb" => {
            // 800 MB of parts: four times that is past the 2.9 GB a save may hold, so it is refused from the lengths the parts state, before a byte is made.
            let doc = with_parts(4, 200);
            let (class, words, work, _) = save(&doc);
            println!("\nRESULT {scenario} class={class} copied={} VmHWM={}KiB {words}", work.bytes_copied, status_kib("VmHWM"));
            assert_eq!(class, "write_limit", "{scenario}: {words}");
            assert!(words.contains("bytes of memory a save holds"), "{scenario}: {words}");
            assert_eq!(work.bytes_copied, 0, "{scenario}: nothing was made");
        }
        "three_parts_of_190mb" => {
            // 570 MB of parts is what the splice's own gate refuses (it counts what its read and its package hold beside the model); the plan's gate counts the same copies, so the refusal is
            // the plan's, before a byte is made.
            let doc = with_parts(3, 190);
            let (class, words, work, _) = save(&doc);
            println!("\nRESULT {scenario} class={class} copied={} VmHWM={}KiB {words}", work.bytes_copied, status_kib("VmHWM"));
            assert_eq!(class, "write_limit", "{scenario}: {words}");
            assert!(words.contains("bytes of memory a save holds"), "{scenario}: {words}");
            assert_eq!(work.bytes_copied, 0, "{scenario}: nothing was made");
        }
        "two_parts_of_200mb" => {
            // 400 MB of parts: the most of this kind the gates let through (five copies of it and the largest part are 2.2 GB), and written under 3 GB of address space.
            let doc = with_parts(2, 200);
            let (class, words, _, made) = save(&doc);
            println!("\nRESULT {scenario} class={class} VmHWM={}KiB {words}", status_kib("VmHWM"));
            assert_eq!(class, "saved", "{scenario}: {words}");
            drop(doc);
            if let Some(made) = made {
                crate::write_core::assert_reads_back(&scenario, &made);
            }
        }
        "two_parts_of_100mb" => {
            let doc = with_parts(2, 100);
            let (class, words, _, made) = save(&doc);
            println!("\nRESULT {scenario} class={class} VmHWM={}KiB {words}", status_kib("VmHWM"));
            assert_eq!(class, "saved", "{scenario}: {words}");
            drop(doc);
            if let Some(made) = made {
                crate::write_core::assert_reads_back(&scenario, &made);
            }
        }
        "a_settings_tree_of_a_million_nodes" => {
            let mut doc = blank(1);
            let texts: Vec<RawNode> = (0..1_000_000).map(|_| RawNode::Text(Arc::from("0123456789"))).collect();
            doc.ext.make_mut().odf.settings = Some(Arc::new(RawElement {
                name: Arc::from("office:document-settings"),
                attrs: RawAttrs::from_pairs([("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0")]),
                children: vec![RawNode::Element(RawElement { name: Arc::from("office:settings"), attrs: RawAttrs::new(), children: texts })],
            }));
            let before = status_kib("VmHWM");
            let (class, words, work, _) = save(&doc);
            let after = status_kib("VmHWM");
            println!("\nRESULT {scenario} class={class} copied={} VmHWM={before}..{after}KiB {words}", work.bytes_copied);
            assert_eq!(class, "write_unsupported", "{scenario}: {words}");
            assert!(after < before + 60_000, "{scenario}: a tree of ten million bytes was made ({before} to {after} KiB)");
        }
        "a_page_of_three_thousand_kept_mebibytes" => {
            // A page keeps three thousand children of a mebibyte each, one text shared by all (the model holds a mebibyte). Each fits the cap of the document's
            // data alone; a save that did not take each from what is left made a tree of three gigabytes of them and aborted. It is refused in words once they pass the cap.
            let doc = crate::model_kept::page_keeping(3000);
            let before = status_kib("VmHWM");
            let (class, words, work, _) = save(&doc);
            let after = status_kib("VmHWM");
            println!("\nRESULT {scenario} class={class} copied={} VmHWM={before}..{after}KiB {words}", work.bytes_copied);
            assert_eq!(class, "write_unsupported", "{scenario}: {words}");
            assert!(words.contains("larger than the reader reads of a part"), "{scenario}: {words}");
            assert!(after < before + 200_000, "{scenario}: the children were copied ({before} to {after} KiB)");
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact model_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

#[test]
fn parts_past_what_a_save_may_hold_are_refused_in_words_before_anything_is_made_and_do_not_abort() {
    let (ended_well, stdout) = run_child("four_parts_of_200mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn parts_the_splices_own_gate_refuses_are_refused_by_the_plans_before_anything_is_made() {
    let (ended_well, stdout) = run_child("three_parts_of_190mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn two_parts_of_200mb_are_written_under_the_address_limit() {
    let (ended_well, stdout) = run_child("two_parts_of_200mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn two_parts_of_100mb_are_written_under_the_address_limit() {
    let (ended_well, stdout) = run_child("two_parts_of_100mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_settings_tree_of_a_million_nodes_is_refused_by_the_bound_and_no_tree_is_made() {
    let (ended_well, stdout) = run_child("a_settings_tree_of_a_million_nodes", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_page_of_three_thousand_kept_mebibytes_is_refused_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("a_page_of_three_thousand_kept_mebibytes", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}
