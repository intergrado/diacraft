//! What the history charges for the step an edit leaves: a text of many parts, and the sheet of a page.
//!
//! The history is held to the bytes its steps are **charged** (`odgfile::hold::unshared_bytes`). A charge that is far under what a step holds lets the process run
//! out of memory with the history saying it holds little: a text of 100,000 one-letter runs was priced at about a byte a part, and each edit of it leaves a vector of
//! 100,000 parts of 48 bytes, so that 560 edits held 2.7 GB under a history that said 48 MiB. The child test runs the review's input under `ulimit -v 3000000`.

use std::mem::size_of;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Text, TextPart, Value};
#[cfg(target_os = "linux")]
use diacraft_engine::History;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, open_drawing};
use crate::odg_file::four;
use crate::run;

/// The first page of a drawing read from a `.odg`: the reader numbers pages from 0.
const PAGE: u32 = 0;

/// A rectangle dropped on the page: its `ID`.
fn a_shape(s: &mut Session) -> u32 {
    let made = run(s, "shape.drop", json!({ "master": "rectangle", "page": PAGE, "x": 5.0, "y": 5.0 }));
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

/// The text of shape `id` set to `parts`, by an edit of the session.
fn set_parts(s: &mut Session, id: u32, parts: Vec<TextPart>) {
    s.edit("a text of many parts", move |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let at = page.shapes.iter().position(|x| x.id == id).unwrap();
        Arc::make_mut(page.shapes.get_mut(at).unwrap()).text = Some(Arc::new(Text { parts, ..Text::default() }));
        Ok(())
    })
    .unwrap();
}

/// `n` runs of one letter each, one next to the other: what the reader makes of a `<Text>` of `n` CDATA sections.
fn adjacent(n: usize) -> Vec<TextPart> {
    (0..n).map(|_| TextPart::Chars(Arc::from("a"))).collect()
}

#[test]
fn an_edit_of_a_text_of_a_hundred_thousand_parts_is_charged_the_vector_it_leaves_behind() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = a_shape(&mut s);
    set_parts(&mut s, id, adjacent(100_000));
    let before = s.history().unwrap().undo_bytes();
    run(&mut s, "shape.set_text", json!({ "id": id, "text": "b", "start": 0, "end": 1, "page": PAGE }));
    let charged = s.history().unwrap().undo_bytes() - before;
    // The step keeps the old text: a vector of 100,000 parts of `size_of::<TextPart>()` bytes, and each run behind its `Arc`.
    let vector = 100_000 * size_of::<TextPart>() as u64;
    assert!(charged >= vector, "the step was charged {charged} bytes and holds a vector of {vector}");
    assert!(charged < 4 * vector, "{charged} bytes for a vector of {vector}: the price is an estimate, not a multiple");
}

#[test]
fn an_edit_of_a_page_whose_sheet_is_full_is_charged_the_sheet_it_leaves_behind() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    // 4,000 of the 4,096 cells a sheet may hold.
    s.edit("a full page sheet", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut page.sheet);
        for k in 0..4_000 {
            sheet.cells.insert(CellName::new(&format!("User{k}")), Cell::constant(Value::from_text("1")));
        }
        Ok(())
    })
    .unwrap();
    let before = s.history().unwrap().undo_bytes();
    run(&mut s, "page.size", json!({ "preset": "a4", "page": PAGE }));
    let charged = s.history().unwrap().undo_bytes() - before;
    // The edit writes the size, which copies the sheet's map of cells: the old copy is the step's.
    let cells = 4_000 * size_of::<Cell>() as u64;
    assert!(charged >= cells, "the step was charged {charged} bytes and holds a map of 4,000 cells ({cells} bytes of cells)");
}

// ---- the review's input, under `ulimit -v 3000000` -----------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by the test below under `ulimit -v`; alone it makes the edits in this process"]
fn child_text_parts() {
    let edits: usize = std::env::var("TEXT_EDITS").ok().and_then(|n| n.parse().ok()).unwrap_or(700);
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = a_shape(&mut s);
    set_parts(&mut s, id, adjacent(100_000));
    let started = std::time::Instant::now();
    for k in 0..edits {
        let text = if k % 2 == 0 { "b" } else { "a" };
        s.execute("shape.set_text", &json!({ "id": id, "text": text, "start": 0, "end": 1, "page": PAGE }))
            .unwrap_or_else(|e| panic!("edit {k} of {edits} was refused: {e}"));
    }
    let history = s.history().unwrap();
    let (held, steps, cut) = (history.undo_bytes(), history.undo_len(), history.undo_was_cut());
    // The history holds what it says: within its budget, with steps let go for the bytes, and every step is a whole vector of parts.
    assert!(held <= 2 * History::DEFAULT_BYTES, "the history holds {held} bytes past its budget of {}", History::DEFAULT_BYTES);
    assert!(cut && steps < edits, "{steps} steps of {edits} edits, cut: {cut}");
    println!(
        "\nRESULT text_parts edits={edits} steps_kept={steps} bytes_held={held} ms={} VmHWM={}KiB VmPeak={}KiB",
        started.elapsed().as_millis(),
        status_kib("VmHWM"),
        status_kib("VmPeak")
    );
}

/// **The review's input.** 100,000 adjacent one-letter runs in one text (a 9 KB file makes them), then 700 range edits of one character, under 3 GB of address
/// space. Priced at a byte a part the history kept every step and the process aborted at about 560 edits; priced at what a part is it keeps the steps its budget
/// holds, and the 700 end.
#[cfg(target_os = "linux")]
#[test]
fn seven_hundred_range_edits_of_a_text_of_a_hundred_thousand_adjacent_parts_end_within_the_budget_of_the_history_under_ulimit_v() {
    use std::process::Command;
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = "ulimit -v 3000000 && exec \"$0\" --exact history_bytes::child_text_parts --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    assert!(out.status.success(), "the child aborted, or its assertion failed: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    let line = stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line");
    eprintln!("{line}");
    assert!(line.contains("edits=700"), "{line}");
}
