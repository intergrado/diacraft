//! A refusal is not an abort: each case runs in a child process under `ulimit -v 3000000` (an address space of about 3 GB, which an abort for memory would pass), and ends as a
//! read or as a refusal in words. A process that is out of memory aborts, and a child that aborts fails the parent. The pattern is `crates/engine/tests/it/odg_memory.rs`.
//!
//! The parents run the children one at a time and are in the `memory-heavy` group of `.config/nextest.toml`.
#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_doc::Page;
use diacraft_native::{Limits, MAX_MEMORY, Previous, State, WriteOptions, read, write};
use diacraft_sheetxml::model::stamp_of_bytes;

use crate::util::*;

const SCENARIO: &str = "NATIVE_MEMORY_SCENARIO";

/// The classes a refusal of a read or a write that stays within the process may have.
const REFUSALS: &[&str] =
    &["too_large", "model_too_complex", "part_too_large", "package_too_large", "xml_too_complex", "zip_ratio_exceeded", "out_of_memory"];

/// How a read or a write ended, for the `RESULT` line: `ok`, or `refused <class> <words>`.
fn how<T>(result: &Result<T, diacraft_native::NativeError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(e) => format!("refused {} {}", e.class(), e.to_string().replace(['\n', '\r'], " ")),
    }
}

/// The peak resident size of this process so far, in kibibytes (`VmHWM`), and the reset of it: the peak of a read is what the read added to what the process held when it began.
fn peak_kib() -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    text.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()).expect("a VmHWM line")
}

/// The kibibytes of address space this process holds now (`VmSize`: what `ulimit -v` bounds).
fn vm_size_kib() -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    text.lines()
        .find_map(|l| l.strip_prefix("VmSize:"))
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("a VmSize line")
}

/// The environment variable by which a parent makes its child treat the write to `/proc/self/clear_refs` as denied (a sandbox with `/proc` mounted read-only does that), so that
/// the path of a test that is skipped is itself run.
const DENY_CLEAR_REFS: &str = "NATIVE_MEMORY_DENY_CLEAR_REFS";

/// Resets the peak, or says why it cannot: where the write to `/proc/self/clear_refs` is denied, no peak can be measured and the test that needs one is **skipped, with this
/// reason on its output**, and not failed.
fn reset_peak() -> Result<(), String> {
    if std::env::var_os(DENY_CLEAR_REFS).is_some() {
        return Err("the write to /proc/self/clear_refs is denied (asked for by the parent)".to_owned());
    }
    std::fs::write("/proc/self/clear_refs", "5")
        .map_err(|e| format!("the peak of a process is reset by writing 5 to /proc/self/clear_refs, and that write is denied here ({e})"))
}

/// Bytes that deflate (a repeated block, and not the signature of a picture, which a writer stores).
fn plain(seed: u64, size: usize) -> std::sync::Arc<[u8]> {
    let mut bytes = patterned(seed, size).to_vec();
    bytes[0] = b'z';
    std::sync::Arc::from(bytes)
}

/// A part of an unknown role, of `bytes`, as the model holds one a newer version wrote.
fn extra_part(name: &str, bytes: std::sync::Arc<[u8]>) -> std::sync::Arc<diacraft_doc::Part> {
    std::sync::Arc::new(diacraft_doc::Part {
        format: diacraft_doc::Format::Native,
        name: std::sync::Arc::from(name),
        media_type: std::sync::Arc::from("application/octet-stream"),
        role: diacraft_doc::PartRole::Other,
        manifest: diacraft_doc::RawAttrs::from_pairs([("role", "annotation")]),
        rels: std::sync::Arc::new(Vec::new()),
        bytes,
    })
}

/// A page whose element carries `count` attributes of `len` bytes each, of a repeated pattern (so that it deflates far below the ratio limit).
fn heavy_page(count: usize, len: usize) -> Vec<u8> {
    let pattern: String = patterned(5, 4096).iter().map(|b| (b'a' + b % 26) as char).collect();
    let value: String = pattern.chars().cycle().take(len).collect();
    let attrs: String = (0..count).map(|i| format!(" a{i}=\"{value}\"")).collect();
    part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"P1\"{attrs}/>"))
}

fn package_with_page(page: Vec<u8>) -> Vec<u8> {
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page);
    package(&items)
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    match scenario.as_str() {
        // Two pictures of 150 MiB: the blobs near their pool. Written, and read again: each ends as a package or a refusal in words.
        "blobs_near_the_pool" => {
            let (a, b) = (patterned(1, 150 << 20), patterned(2, 150 << 20));
            let doc = drawing_of(vec![page_showing(1, &[&a, &b])]);
            let written = write(&doc, &State::default(), None, &WriteOptions::default());
            let mut line = how(&written);
            if let Ok(w) = written {
                drop(doc);
                line = format!("{line} then {}", how(&read(&w.bytes, &Limits::default())));
            }
            println!("RESULT {line}");
        }
        // Records that state blobs of 900 MiB in all, where the entries are a few bytes: refused by the sum, before a blob is fetched.
        "records_past_the_pool" => {
            let stamp = stamp_of_bytes(&PICTURE).text();
            let list: String = (0..3)
                .map(|i| {
                    format!("<diacraft:blob id=\"{i}\" path=\"blobs/0.png\" media-type=\"image/png\" size=\"{}\" stamp=\"{stamp}\"/>", 300 << 20)
                })
                .collect();
            let mut items = vector_5();
            let document = String::from_utf8(entry(&package(&items), "document.xml"))
                .unwrap()
                .replace("<diacraft:blob path=\"blobs/0.png\" media-type=\"image/png\" size=\"12\" stamp=\"71df7ba063b3b4da\"/>", &list);
            replace(&mut items, "document.xml", document.into_bytes());
            println!("RESULT {}", how(&read(&package(&items), &Limits::default())));
        }
        // A model part of 120 attributes of a million bytes: a part just under the cap, and then one over it.
        "attributes_of_a_million_bytes" => {
            let under = package_with_page(heavy_page(120, 1_000_000));
            let mut line = how(&read(&under, &Limits::default()));
            drop(under);
            let over = package_with_page(heavy_page(140, 1_000_000));
            line = format!("{line} then {}", how(&read(&over, &Limits::default())));
            println!("RESULT {line}");
        }
        // A file larger than any package may be, as a slice of zeros: refused before it is read.
        "file_over_the_cap" => {
            let bytes = vec![0u8; 600 << 20];
            println!("RESULT {}", how(&read(&bytes, &Limits::default())));
        }
        // A save beside a process that holds nearly all of the memory: refused for memory, in words, and not made.
        "save_beside_a_full_process" => {
            let a = patterned(3, 100 << 20);
            let doc = drawing_of(vec![page_showing(1, &[&a])]);
            let beside = Limits::beside(MAX_MEMORY - (50 << 20));
            println!("RESULT {}", how(&write(&doc, &State::default(), None, &WriteOptions { limits: beside, ..WriteOptions::default() })));
        }
        // A read beside the same process: the pools shrink, and a drawing with a picture of 100 MiB is refused for memory.
        "read_beside_a_full_process" => {
            let a = patterned(3, 100 << 20);
            let bytes = write_doc(&drawing_of(vec![page_showing(1, &[&a]), Page::new(2, "x")]));
            drop(a);
            let beside = Limits::beside(MAX_MEMORY - (150 << 20));
            println!("RESULT {}", how(&read(&bytes, &beside)));
        }
        // A read of a picture of 120 MiB: the file, and the one copy of the picture. The peak is the file and the picture (and what a process has), not the picture twice.
        "blob_read_peak" => {
            let bytes = {
                let picture = patterned(1, 120 << 20);
                write_doc(&drawing_of(vec![page_showing(1, &[&picture])]))
            };
            if let Err(why) = reset_peak() {
                println!("RESULT skipped {why}");
                return;
            }
            let result = read(&bytes, &Limits::default());
            println!("RESULT {} peak={}", how(&result), peak_kib());
        }
        // A read of a part of an unknown role of 120 MiB, which deflates to a few hundred kilobytes: the one copy of the part, held by the model.
        "extra_read_peak" => {
            let bytes = {
                let mut doc = drawing_of(vec![Page::new(1, "One")]);
                doc.parts = diacraft_doc::SharedVec::from_vec(vec![extra_part("notes/big.bin", plain(2, 120 << 20))]);
                write_doc(&doc)
            };
            if let Err(why) = reset_peak() {
                println!("RESULT skipped {why}");
                return;
            }
            let result = read(&bytes, &Limits::default());
            println!("RESULT {} peak={}", how(&result), peak_kib());
        }
        // A manifest of 40,000 rows, each with a path of a thousand bytes, that name nothing (blob rows may): the plan the read makes of it is not a second copy.
        "manifest_rows_peak" => {
            let bytes = {
                let rows: Vec<(String, String, String)> = (0..40_000)
                    .map(|i| (format!("b/{i:06}{}", "x".repeat(990)), "blob".to_owned(), String::new()))
                    .chain([
                        ("document.xml".to_owned(), "document".to_owned(), String::new()),
                        ("pages/page1.xml".to_owned(), "page".to_owned(), String::new()),
                        ("state.xml".to_owned(), "state".to_owned(), String::new()),
                    ])
                    .collect();
                let borrowed: Vec<(&str, &str, &str)> = rows.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
                let mut items = vector_1();
                replace(&mut items, "manifest.xml", manifest("1.0", "drawing", &borrowed));
                package(&items)
            };
            if let Err(why) = reset_peak() {
                println!("RESULT skipped {why}");
                return;
            }
            let result = read(&bytes, &Limits::default());
            let notes = result.as_ref().map(|(_, o)| o.notes.len()).unwrap_or(0);
            println!("RESULT {} notes={notes} peak={}", how(&result), peak_kib());
        }
        // What the process holds before it does anything: the parent sets the room of the next child from it.
        "vm_size" => println!("RESULT {}", vm_size_kib()),
        // A package read from standard input in an address space that cannot hold its blob: the read ends as a refusal in words, and the process does not abort.
        "blob_in_a_short_address_space" => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).expect("the package on standard input");
            eprintln!("the child holds {} KiB of address space and reads a package of {} bytes", vm_size_kib(), bytes.len());
            println!("RESULT {}", how(&read(&bytes, &Limits::default())));
        }
        // A package of another tool's zipping whose preview states 1 KB and inflates to 100 MiB, read from standard input and saved again: the preview is left out, and the save
        // does not make the allocation of what the entry inflates to.
        "save_after_a_preview_that_inflates_past_its_size" => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).expect("the package on standard input");
            let (doc, opened) = read(&bytes, &Limits::default()).expect("the package reads");
            if let Err(why) = reset_peak() {
                println!("RESULT skipped {why}");
                return;
            }
            let written = write(&doc, &opened.state, Some(&Previous { bytes: &bytes, decoded: &opened.decoded }), &WriteOptions::default());
            let kept = written.as_ref().map(|w| names(&w.bytes).contains(&"preview.png".to_owned())).unwrap_or(false);
            println!("RESULT {} preview={kept} peak={}", how(&written), peak_kib());
        }
        other => panic!("no scenario {other}"),
    }
}

/// The address space the children are held to, in kibibytes (`ulimit -v`): about 3 GB, which an abort for memory would pass.
const ADDRESS_SPACE_KIB: u64 = 3_000_000;

/// What a child needs besides its scenario: the address space it is held to, bytes for its standard input, and the environment variables to set.
struct Run<'a> {
    limit_kib: u64,
    stdin: &'a [u8],
    env: &'a [(&'a str, &'a str)],
}

/// Runs the child's `scenario` under `ulimit -v 3000000`: whether it ended well, and the line it printed.
fn run_child(scenario: &str) -> (bool, String) {
    run_child_with(scenario, &Run { limit_kib: ADDRESS_SPACE_KIB, stdin: &[], env: &[] })
}

/// [`run_child`] with the address space, the input and the environment `run` says.
fn run_child_with(scenario: &str, run: &Run<'_>) -> (bool, String) {
    use std::io::Write;
    use std::process::Stdio;
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {} && exec \"$0\" --exact memory::child --ignored --nocapture --test-threads=1", run.limit_kib);
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (name, value) in run.env {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("run /bin/sh");
    // The input is written from another thread, so that a child that ends before it has read it all cannot leave the parent blocked on a full pipe.
    let mut input = child.stdin.take().expect("a piped standard input");
    let bytes = run.stdin.to_vec();
    let writer = std::thread::spawn(move || {
        // A child that has ended closes the pipe: that is the child's answer to read, not an error of the writer.
        let _ = input.write_all(&bytes);
    });
    let out = child.wait_with_output().expect("wait for the child");
    writer.join().expect("the writer of the input");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    let line = stdout.lines().find_map(|l| l.split_once("RESULT ").map(|(_, rest)| rest)).unwrap_or("").to_owned();
    (out.status.success(), line)
}

/// Every `refused <class>` in `line` is a class of a refusal that stays within the process, and has words after it.
fn assert_refusals_in_words(line: &str) {
    for part in line.split(" then ") {
        if let Some(rest) = part.strip_prefix("refused ") {
            let (class, words) = rest.split_once(' ').unwrap_or((rest, ""));
            assert!(REFUSALS.contains(&class), "a class that is not a limit: {line}");
            assert!(words.len() > 10, "words that say what is large: {line}");
        } else {
            assert!(part.starts_with("ok"), "{line}");
        }
    }
}

#[test]
fn blobs_near_the_pool_end_as_a_read_or_a_refusal_in_words_under_ulimit_v() {
    let (ok, line) = run_child("blobs_near_the_pool");
    assert!(ok, "the child aborted or failed");
    assert_refusals_in_words(&line);
}

#[test]
fn records_that_state_blobs_past_the_pool_are_refused_before_a_blob_is_fetched_under_ulimit_v() {
    let (ok, line) = run_child("records_past_the_pool");
    assert!(ok, "the child aborted or failed");
    assert!(line.starts_with("refused model_too_complex"), "{line}");
    assert!(line.contains("pictures and parts"), "{line}");
}

#[test]
fn a_model_part_of_attributes_of_a_million_bytes_ends_as_a_read_or_a_refusal_in_words_under_ulimit_v() {
    let (ok, line) = run_child("attributes_of_a_million_bytes");
    assert!(ok, "the child aborted or failed");
    assert_refusals_in_words(&line);
    // The part over the cap of an XML part (128 MiB) is refused for it.
    let (_, over) = line.split_once(" then ").expect("both reads");
    assert!(over.starts_with("refused part_too_large"), "{line}");
}

#[test]
fn a_file_over_the_cap_is_refused_before_it_is_read_under_ulimit_v() {
    let (ok, line) = run_child("file_over_the_cap");
    assert!(ok, "the child aborted or failed");
    assert!(line.starts_with("refused too_large"), "{line}");
}

#[test]
fn a_save_beside_a_process_that_holds_the_memory_is_refused_in_words_under_ulimit_v() {
    let (ok, line) = run_child("save_beside_a_full_process");
    assert!(ok, "the child aborted or failed");
    assert!(line.starts_with("refused too_large") && line.contains("memory"), "{line}");
}

#[test]
fn a_read_beside_a_process_that_holds_the_memory_is_refused_in_words_under_ulimit_v() {
    let (ok, line) = run_child("read_beside_a_full_process");
    assert!(ok, "the child aborted or failed");
    assert_refusals_in_words(&line);
    assert!(line.starts_with("refused"), "a drawing of a picture of 100 MiB does not open beside a process that holds all but 150 MiB: {line}");
}

/// The peak a child reported, in kibibytes, or `None` when the child could not measure one here (`skipped`, with its reason, which is printed so that the output says why the
/// test checked nothing): the caller returns, and the test is skipped and not failed.
fn reported_peak(line: &str) -> Option<u64> {
    if let Some(why) = line.strip_prefix("skipped ") {
        eprintln!("SKIPPED, no peak can be measured on this machine: {why}");
        return None;
    }
    assert!(line.starts_with("ok"), "{line}");
    Some(line.split("peak=").nth(1).and_then(|n| n.trim().parse().ok()).unwrap_or_else(|| panic!("no peak in {line:?}")))
}

#[test]
fn a_picture_is_held_once_while_it_is_read_under_ulimit_v() {
    // M1: the entry is inflated into the allocation the model holds it in. The file (120 MB, stored) and the picture (120 MB) and what a process has: under 300 MB, where a
    // vector that was converted held the picture twice for the copy.
    let (ok, line) = run_child("blob_read_peak");
    assert!(ok, "the child aborted or failed");
    let Some(peak) = reported_peak(&line) else { return };
    eprintln!("a picture of 120 MiB: peak {peak} KiB");
    assert!(peak < 300_000, "the picture was held twice while it was read: {peak} KiB");
}

#[test]
fn a_part_of_an_unknown_role_is_held_once_while_it_is_read_under_ulimit_v() {
    // M1: the same for a package part kept as bytes. The file is small (the part deflates), so the peak is the part and what a process has: under 200 MB.
    let (ok, line) = run_child("extra_read_peak");
    assert!(ok, "the child aborted or failed");
    let Some(peak) = reported_peak(&line) else { return };
    eprintln!("a part of 120 MiB: {line}");
    assert!(peak < 200_000, "the part was held twice while it was read: {peak} KiB");
}

#[test]
fn the_plan_of_a_manifest_of_forty_thousand_rows_holds_no_second_copy_of_them_under_ulimit_v() {
    // M3: the rows are indexed by their place, and not cloned (a path of a thousand bytes, 40,000 times, is 40 MB each time it is copied).
    let (ok, line) = run_child("manifest_rows_peak");
    assert!(ok, "the child aborted or failed");
    let Some(peak) = reported_peak(&line) else { return };
    eprintln!("40,000 rows: peak {peak} KiB");
    assert!(peak < MANIFEST_PEAK_KIB, "the rows were copied: {peak} KiB");
}

/// The peak a read of the manifest of 40,000 rows may reach (KiB): the code that cloned the rows measured 142,500 here and this code 104,300.
const MANIFEST_PEAK_KIB: u64 = 123_000;

#[test]
fn a_blob_that_the_address_space_cannot_hold_is_refused_in_words_and_the_process_does_not_abort() {
    // A picture of 250 MiB that deflates to about a megabyte: the file is small, and the entry states a size the pool allows. The address space of the child is what it holds
    // when it starts and 150 MiB more (about 350,000 KiB on the machine this was written on), so that the allocation for the picture cannot be had. The reader made it with
    // `repeat_n(..).collect()`, which cannot fail: the process aborted. It is reserved with `try_reserve_exact` now, and the read ends as `out_of_memory`, in words.
    let (ok, line) = run_child("vm_size");
    assert!(ok, "the child that measures the address space failed");
    let held: u64 = line.trim().parse().unwrap_or_else(|_| panic!("a number of kibibytes, got {line:?}"));
    let bytes = {
        let picture = plain(7, 250 << 20);
        write_doc(&drawing_of(vec![page_showing(1, &[&picture])]))
    };
    assert!(bytes.len() < 4 << 20, "the picture deflates: the package is {} bytes", bytes.len());
    let run = Run { limit_kib: held + (150 << 10), stdin: &bytes, env: &[] };
    let (ok, line) = run_child_with("blob_in_a_short_address_space", &run);
    assert!(ok, "the child aborted for memory where it should have refused");
    assert!(line.starts_with("refused out_of_memory "), "{line}");
    assert_refusals_in_words(&line);
    // An entry that states a size its bytes cannot reach (300 MiB from 12 bytes) is refused by the ratio when the archive is opened, before any allocation for it is asked of the
    // allocator: in the same address space the allocation would abort, and the read ends as `zip_ratio_exceeded`.
    let mut items = vector_5();
    let at = items.iter().position(|i| i.name == b"blobs/0.png").expect("vector 5 has a blob");
    items[at] = crate::zipbuild::Item::stored("blobs/0.png", &PICTURE).claiming_size(300 << 20);
    let lying = package(&items);
    let (ok, line) = run_child_with("blob_in_a_short_address_space", &Run { limit_kib: held + (150 << 10), stdin: &lying, env: &[] });
    assert!(ok, "the child aborted for memory where the ratio refuses the entry");
    assert!(line.starts_with("refused zip_ratio_exceeded "), "{line}");
}

#[test]
fn a_denied_reset_of_the_peak_skips_the_measure_and_does_not_fail_the_test() {
    // The three peak tests need the write to `/proc/self/clear_refs`. Where it is denied they were failed by an `expect`: the child now says `skipped` with the reason, and the
    // parent prints it and returns. The parent asks the child to treat the write as denied, so that this path is run on every machine.
    for scenario in ["blob_read_peak", "extra_read_peak", "manifest_rows_peak"] {
        let run = Run { limit_kib: ADDRESS_SPACE_KIB, stdin: &[], env: &[(DENY_CLEAR_REFS, "1")] };
        let (ok, line) = run_child_with(scenario, &run);
        assert!(ok, "the child `{scenario}` failed where the write is denied");
        assert!(line.starts_with("skipped "), "{scenario}: {line}");
        assert!(line.contains("clear_refs") && line.len() > 30, "the reason is in words: {line}");
        assert_eq!(reported_peak(&line), None, "a skipped child has no peak");
    }
}

#[test]
fn a_preview_that_inflates_past_the_size_its_entry_states_is_not_inflated_whole_by_a_save() {
    // The save of a drawing that another tool zipped writes the preview again, from the previous package. The meter is charged the size the entry states (1 KB here), and the
    // entry inflates to 100 MiB: it was read into a buffer that grew to what the container's limits allow, which no charge covered. It is read into a buffer of the stated size
    // now, and the entry that holds more is left out.
    let ours = {
        let mut doc = drawing_of(vec![Page::new(1, "One"), Page::new(2, "Two")]);
        doc.title = std::sync::Arc::from("Two pages");
        write(&doc, &State::default(), None, &WriteOptions { preview: Some(preview_png(b"a picture")), ..WriteOptions::default() })
            .expect("written")
            .bytes
    };
    let mut lying = preview_png(b"");
    lying.resize(100 << 20, 0);
    let items: Vec<crate::zipbuild::Item> = entries(&ours)
        .into_iter()
        .map(|(name, content)| match name.as_str() {
            "mimetype" => crate::zipbuild::Item::stored(&name, &content),
            "preview.png" => crate::zipbuild::Item::deflated(&name, &lying).claiming_size(1024),
            _ => crate::zipbuild::Item::deflated(&name, &content),
        })
        .collect();
    drop(lying);
    let bytes = package(&items);
    assert!(bytes.len() < 4 << 20, "the entry deflates: the package is {} bytes", bytes.len());
    let run = Run { limit_kib: ADDRESS_SPACE_KIB, stdin: &bytes, env: &[] };
    let (ok, line) = run_child_with("save_after_a_preview_that_inflates_past_its_size", &run);
    assert!(ok, "the child aborted or failed");
    let Some(peak) = reported_peak(&line) else { return };
    eprintln!("a preview that states 1 KB and inflates to 100 MiB: {line}");
    assert!(line.contains("preview=false"), "an entry that holds more than it states is left out: {line}");
    assert!(peak < SAVE_PEAK_KIB, "the save allocated for what the entry inflates to, which no charge covered: {peak} KiB");
}

/// The peak a save of a small drawing may reach beside a preview that inflates to 100 MiB (KiB): the code that inflated whatever the entry held measured 111,600 here and this code 10,700.
const SAVE_PEAK_KIB: u64 = 60_000;

/// A preview picture: a PNG signature and `tag`.
fn preview_png(tag: &[u8]) -> Vec<u8> {
    [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], tag].concat()
}
