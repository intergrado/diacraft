//! What a retext of a paragraph that holds 1,300,000 zero-width elements, of 600,000 paragraphs that each hold one, and of tens of thousands of fields whose
//! characters change costs in memory, each case in a child process under an
//! address-space limit (`ulimit -v 3000000`), as `fodg_memory.rs` runs its children. A process that is out of memory *aborts*, and a child that aborts is a failure of
//! the parent: a save that cannot have the memory must end as an error of a class, never as an abort. The tokens of a paragraph are held to the memory a save may hold
//! for as long as the paragraph is planned, and given back (the counted test at the end).

#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_doc::TextPart;
use diacraft_odg::{Limits, OriginalPackage, SaveNote, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, open};
use crate::write_runs::{chars, set_parts, set_row};

const SCENARIO: &str = "ODG_ZW_MEMORY_SCENARIO";

const STYLES: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// A custom shape holding `inner` as its text.
fn drawing(inner: &str) -> Drawing {
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{inner}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    );
    Drawing::a4().auto(STYLES).page(&body)
}

/// One paragraph of `n` words each followed by a bookmark, its last word bold: written whole, every bookmark put back. (Each word, bookmark and name is an item of
/// the reader's tree: 1,300,000 is the most the reader's cap of 4,000,000 items admits.)
fn marks(n: usize) -> Open {
    let inner: String = (0..n).map(|k| format!("w<text:bookmark text:name=\"B{k}\"/>")).collect();
    let mut o = open(&drawing(&format!("<text:p>{inner}end</text:p>")));
    set_row(&mut o, "Character", 1, &[("Style", 1.0)]);
    set_parts(&mut o, vec![chars(&"w".repeat(n)), TextPart::Cp(1), chars("end")]);
    o
}

/// `n` paragraphs that each hold a bookmark, taken out but the first: every bookmark goes to the end of the one paragraph kept.
fn paragraphs(n: usize) -> Open {
    let inner: String = (0..=n).map(|k| format!("<text:p>w<text:bookmark text:name=\"B{k}\"/></text:p>")).collect();
    let mut o = open(&drawing(&inner));
    set_parts(&mut o, vec![chars("w")]);
    o
}

/// One paragraph of `n` fields (`x` and a title field showing `t`), whose characters are all changed to `u`: each field is written as plain text, and the save says
/// so once for the kind of field, and not for each.
fn fields(n: usize) -> Open {
    let inner: String = (0..n).map(|_| "x<text:title>t</text:title>").collect();
    let mut o = open(&drawing(&format!("<text:p>{inner}z</text:p>")));
    set_row(&mut o, "Character", 1, &[("Style", 1.0)]);
    set_parts(&mut o, vec![chars(&"xu".repeat(n)), TextPart::Cp(1), chars("z")]);
    o
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let o = match scenario.as_str() {
        "marks_1_3m" => marks(1_300_000),
        "paragraphs_taken_out_600k" => paragraphs(600_000),
        "fields_changed_50k" => fields(50_000),
        other => panic!("no scenario {other}"),
    };
    println!("\nREAD VmHWM={}KiB", status_kib("VmHWM"));
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let class = saved.as_ref().err().map_or("saved", |e| e.class());
    let notes = saved.as_ref().map_or(0, |w| w.notes.iter().filter(|n| matches!(n, SaveNote::FieldWritten { .. })).count());
    println!(
        "\nRESULT {scenario} class={class} zero_steps={} tokenized={} copied={} notes={notes} gate={} room={} VmHWM={}KiB",
        work.zero_steps,
        work.tokenized_bytes,
        work.bytes_copied,
        work.estimated_memory,
        work.memory_room,
        status_kib("VmHWM")
    );
    // Saved, or refused as a limit: never anything else, and never an abort.
    assert!(["saved", "write_limit"].contains(&class), "{class}");
    // A save that was made gives a file the reader reads, with the model let go.
    if let Ok(made) = saved {
        drop(o);
        crate::write_core::assert_reads_back(&scenario, &made.bytes);
    }
    if scenario == "fields_changed_50k" && class == "saved" {
        // One note for the kind of field this shape changed, and not 50,000.
        assert_eq!(notes, 1, "{notes} notes");
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact write_zero_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

#[test]
fn a_retext_of_a_paragraph_of_1300000_bookmarks_ends_as_a_save_or_a_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("marks_1_3m", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_retext_that_takes_out_600000_paragraphs_that_each_hold_a_bookmark_ends_as_a_save_or_a_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("paragraphs_taken_out_600k", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_retext_that_changes_fifty_thousand_fields_ends_as_a_save_or_a_limit_with_one_note_and_does_not_abort() {
    let (ended_well, stdout) = run_child("fields_changed_50k", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// A save of `o` under `memory` bytes: its outcome (whether it edited, or the class and words of the refusal) and what it counted.
fn save_under(o: &Open, memory: u64) -> (Result<bool, (&'static str, String)>, WriteWork) {
    let mut work = WriteWork::default();
    let limits = Limits { max_write_memory: memory, ..Limits::default() };
    let saved = write_counted(
        &o.doc,
        Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { limits, ..WriteOptions::default() },
        &mut work,
    );
    (saved.map(|w| w.edited).map_err(|e| (e.class(), e.to_string())), work)
}

/// The tokens of a paragraph (a token for each piece of its content, and four bytes for each character room of a text) are held to the memory a
/// save may hold while the paragraph is planned, and given back when it is: with room for what the retext keeps and a kilobyte besides, they do not fit and the save is
/// refused as a limit, in the words of the retext's memory; with room for them it saves, and the room that is left is what was beside what the retext keeps.
#[test]
fn the_tokens_of_a_paragraph_are_held_to_the_memory_of_a_save_while_it_is_planned_and_given_back() {
    let o = marks(20_000);
    let whole = Limits::default().max_write_memory;
    let (first, work) = save_under(&o, whole);
    assert_eq!(first, Ok(true));
    // What the gate holds for the model, and what the retext keeps for good (the elements it lists, 96 bytes each): the room the gate left, less what is left of it.
    let gate = work.estimated_memory;
    let kept = whole - gate - work.memory_room;
    assert!(kept >= 20_000 * 96, "{kept} bytes kept for 20,000 elements");
    // No room for the tokens: one kilobyte beside what is kept.
    let (tight, _) = save_under(&o, gate + kept + 1_000);
    match &tight {
        Err((class, why)) => assert!(*class == "write_limit" && why.contains("memory for the paragraphs of a retext"), "{class}: {why}"),
        Ok(edited) => panic!("a paragraph whose tokens do not fit was saved (edited {edited})"),
    }
    // Room for them, 64 MiB, and the paragraph's 40,000 tokens and 20,000 texts are a few megabytes of it: it saves, and all of it is given back.
    let spare = 64 * 1024 * 1024;
    let (roomy, counted) = save_under(&o, gate + kept + spare);
    assert_eq!(roomy, Ok(true));
    assert_eq!(counted.memory_room, spare, "the room the tokens held is given back");
}

/// The notes of fields written as plain text are one for each kind of field in a shape, however many changed: 3,000 title fields and 3,000
/// author fields whose characters all change are two notes, in the order the kinds were first met, and a second save is no edit.
#[test]
fn the_notes_of_fields_written_as_plain_text_are_one_for_each_kind_of_field() {
    let n = 3_000;
    let inner: String = (0..n).map(|k| if k % 2 == 0 { "x<text:title>t</text:title>" } else { "y<text:author-name>a</text:author-name>" }).collect();
    let mut o = open(&drawing(&format!("<text:p>{inner}z</text:p>")));
    set_row(&mut o, "Character", 1, &[("Style", 1.0)]);
    let changed: String = (0..n).map(|k| if k % 2 == 0 { "xu" } else { "yb" }).collect();
    set_parts(&mut o, vec![chars(&changed), TextPart::Cp(1), chars("z")]);
    let mut work = WriteWork::default();
    let saved =
        write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work).expect("saved");
    let kinds: Vec<&str> = saved
        .notes
        .iter()
        .filter_map(|n| match n {
            SaveNote::FieldWritten { element, .. } => Some(*element),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["title", "author-name"], "one note for each kind, in the order met");
    let again = write_counted(
        &o.doc,
        Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }),
        &WriteOptions::default(),
        &mut WriteWork::default(),
    )
    .expect("a second save");
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}
