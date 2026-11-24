//! What the `.odg` model writer does with the drawings the engine makes, by class (`diacraft_odg::write_with(doc, None, ..)`), so that start
//! from numbers. Each case is a new drawing with one construct in it, made by the engine's own commands and handed to the writer **as the model holds it** (the copy a splice save
//! flattens is not the model writer's input). The outcome of a case is one of: `written`; `written, said: <class>` (the save wrote it and a note says what a new shape held that was
//! not written); `refused: <words>`.
//!
//! The suite checks only that no case ends as a fault of the writer (a self-check, a mismatch or a panic): what is refused is the list and -3 close, and it is printed by
//! the ignored `measure` test (`cargo test -p diacraft-engine --test it -- --ignored --nocapture odg_model_measure::measure`).

use std::collections::BTreeMap;

use diacraft_engine::Session;
use diacraft_odg::{WriteError, WriteOptions, write_with};
use serde_json::json;

use crate::run;

/// A construct: a name for the table, and what makes it in a new drawing.
type Case = (&'static str, Box<dyn Fn(&mut Session)>);

/// The `ID` of the page at `place`.
fn page(s: &Session, place: usize) -> u32 {
    s.document().expect("a drawing").pages.get(place).expect("a page").id
}

fn rectangle(s: &mut Session, x: f64, y: f64) -> serde_json::Value {
    let page = page(s, 0);
    run(s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": x, "y": y }))
}

/// The constructs, in the order of the table.
fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = vec![
        ("an empty drawing", Box::new(|_| {})),
        ("a rectangle", Box::new(|s| drop(rectangle(s, 4.0, 4.0)))),
        (
            "a rectangle with a fill",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.format", json!({ "ids": [r["id"]], "fill": "#123456" }));
            }),
        ),
        (
            "a rectangle with a heavy line",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.format", json!({ "ids": [r["id"]], "weight": 3.0 }));
            }),
        ),
        (
            "a rectangle with a text",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.set_text", json!({ "id": r["id"], "text": "Hello" }));
            }),
        ),
        (
            "a rectangle with a text of two lines",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.set_text", json!({ "id": r["id"], "text": "One\nTwo" }));
            }),
        ),
        (
            "a turned rectangle",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.rotate", json!({ "ids": [r["id"]], "angle": 30 }));
            }),
        ),
        (
            "a flipped rectangle",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.flip", json!({ "ids": [r["id"]], "axis": "horizontal" }));
            }),
        ),
        (
            "a resized rectangle",
            Box::new(|s| {
                let r = rectangle(s, 4.0, 4.0);
                run(s, "shape.resize", json!({ "id": r["id"], "x": 3.0, "y": 3.0, "width": 2.5, "height": 1.0 }));
            }),
        ),
        (
            "a text box",
            Box::new(|s| {
                let page = page(s, 0);
                drop(run(s, "text.box", json!({ "page": page, "x": 4.0, "y": 4.0, "text": "Hi" })));
            }),
        ),
        (
            "two rectangles and a straight connector",
            Box::new(|s| {
                let (a, b) = (rectangle(s, 3.0, 2.0), rectangle(s, 8.0, 2.0));
                run(s, "connector.add", json!({ "from": { "id": a["id"] }, "to": { "id": b["id"] } }));
            }),
        ),
        (
            "two rectangles and a curved connector",
            Box::new(|s| {
                let (a, b) = (rectangle(s, 3.0, 2.0), rectangle(s, 8.0, 4.0));
                let c = run(s, "connector.add", json!({ "from": { "id": a["id"] }, "to": { "id": b["id"] } }));
                run(s, "connector.style", json!({ "style": "curved", "ids": [c["id"]] }));
            }),
        ),
        (
            "two rectangles grouped",
            Box::new(|s| {
                let (a, b) = (rectangle(s, 3.0, 2.0), rectangle(s, 8.0, 2.0));
                run(s, "shape.group", json!({ "ids": [a["id"], b["id"]] }));
            }),
        ),
        (
            "a second page with a rectangle",
            Box::new(|s| {
                run(s, "page.add", json!({ "name": "Second" }));
                let page = page(s, 1);
                drop(run(s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 4.0, "y": 4.0 })));
            }),
        ),
        (
            "a page of another size",
            Box::new(|s| {
                run(s, "page.size", json!({ "width": 8.27, "height": 11.69 }));
                drop(rectangle(s, 4.0, 4.0));
            }),
        ),
    ];
    // Every master of every built-in stencil, dropped alone.
    for master in diacraft_engine::stencil::all_masters() {
        let id = master.id;
        cases.push((
            id,
            Box::new(move |s| {
                let page = page(s, 0);
                drop(run(s, "shape.drop", json!({ "master": id, "page": page, "x": 4.0, "y": 4.0 })));
            }),
        ));
    }
    cases
}

/// What the writer does with the drawing a case makes: the outcome, in words that group alike.
fn outcome(case: &Case) -> (String, Option<WriteError>) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    (case.1)(&mut s);
    let doc = s.document().expect("a drawing");
    match write_with(doc, None, &WriteOptions::default()) {
        Ok(written) => {
            let said: Vec<&'static str> = written.notes.iter().map(diacraft_odg::SaveNote::class).collect();
            let mut said = said;
            said.sort_unstable();
            said.dedup();
            (if said.is_empty() { "written".to_owned() } else { format!("written, said: {}", said.join(", ")) }, None)
        }
        Err(e) => (format!("refused: {}", e.to_string().chars().take(110).collect::<String>()), Some(e)),
    }
}

/// No case ends as a fault of the writer, and the plain cases are written: the writer from nothing takes what the engine makes.
#[test]
fn no_drawing_the_engine_makes_ends_as_a_fault_of_the_model_writer() {
    let mut faults = Vec::new();
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    for case in cases() {
        let (outcome, error) = outcome(&case);
        *tally.entry(outcome.clone()).or_insert(0) += 1;
        if let Some(e) = error
            && matches!(e, WriteError::SelfCheck(_) | WriteError::Mismatch(_) | WriteError::Package(_) | WriteError::Original(_))
        {
            faults.push(format!("{}: {outcome}", case.0));
        }
        if matches!(case.0, "an empty drawing" | "a rectangle" | "a rectangle with a fill") {
            assert!(outcome.starts_with("written"), "{}: {outcome}", case.0);
        }
    }
    assert!(faults.is_empty(), "faults of the writer: {faults:#?}");
    assert!(tally.values().sum::<usize>() > 50, "the masters of the built-in stencils are among the cases: {tally:?}");
}

/// **What the engine does with each drawing** (the engine's own Save As `.odg`, which hands the writer the model with the engine's shapes flattened as a save into a file flattens
/// its new ones): a case is written and read again, and the file is the same picture; or it is refused in words, with nothing written. The outcome of a case is its words.
fn engine_outcome(case: &Case) -> String {
    let dir = crate::native_common::Dir::new();
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    (case.1)(&mut s);
    match s.execute("file.save_as", &json!({ "path": dir.path("x.odg") })) {
        Err(e) => format!("refused: {}", e.to_string().chars().take(110).collect::<String>()),
        Ok(saved) => {
            let mut file = Session::new();
            run(&mut file, "file.open", json!({ "path": dir.path("x.odg") }));
            let apart = crate::native_odg::pixels_apart(s.document().expect("a drawing"), file.document().expect("a drawing"));
            let first: String = saved["warnings"][0].as_str().unwrap_or("").chars().take(60).collect();
            format!(
                "written{}{}",
                if first.is_empty() { String::new() } else { format!(", said: {first}") },
                if apart > 0 { ", NOT the same picture" } else { "" }
            )
        }
    }
}

/// **Every drawing the engine makes is saved as an `.odg` by the engine and reads back as the same picture**, or is refused in words with nothing written: no case is refused for a reason that
/// is a fault, and the plain ones are written (the 65 cases of this file: a rectangle in eight forms, a text box, connectors, a group, a second page, a page of another size, and every master of
/// every built-in stencil).
#[test]
fn every_drawing_the_engine_makes_is_saved_as_an_odg_by_the_engine_and_reads_back_as_the_same_picture() {
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut apart: Vec<&str> = Vec::new();
    for case in cases() {
        let outcome = engine_outcome(&case);
        if outcome.contains("NOT the same picture") {
            apart.push(case.0);
        }
        if matches!(case.0, "an empty drawing" | "a rectangle" | "a rectangle with a fill") {
            assert!(outcome.starts_with("written"), "{}: {outcome}", case.0);
        }
        *tally.entry(outcome).or_insert(0) += 1;
    }
    assert!(tally.values().sum::<usize>() > 50, "the masters of the built-in stencils are among the cases: {tally:?}");
    assert!(apart.is_empty(), "drawings that do not read back as the picture they were: {apart:?}; tally {tally:#?}");
}

/// Writes what the engine's Save As makes of each case into the folder `DIACRAFT_LO_DIR` names (`case_NN.odg`), for the black-box check of another program's reader: an `#[ignore]`d helper
/// of the report's LibreOffice run, and no test.
#[test]
#[ignore = "writes files for a black-box check by hand: DIACRAFT_LO_DIR names the folder"]
fn write_the_engines_drawings_for_the_black_box_check() {
    let Ok(folder) = std::env::var("DIACRAFT_LO_DIR") else { return };
    for (n, case) in cases().into_iter().enumerate() {
        let mut s = Session::new();
        run(&mut s, "file.new", json!({}));
        (case.1)(&mut s);
        run(&mut s, "file.save_as", json!({ "path": format!("{folder}/case_{n:02}.odg") }));
    }
}

/// Prints the engine's table: every case, and the count of each outcome (`cargo test -p diacraft-engine --test it -- --ignored --nocapture measure_the_engine`).
#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure_the_engine() {
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    for case in cases() {
        let outcome = engine_outcome(&case);
        if !outcome.starts_with("written") || outcome.contains("NOT") {
            println!("CASE {:<45} {outcome}", case.0);
        }
        *tally.entry(outcome).or_insert(0) += 1;
        total += 1;
    }
    println!("TOTAL {total} cases");
    for (outcome, n) in &tally {
        println!("TALLY {n:>4}  {outcome}");
    }
}

/// Prints the table: every case, and the count of each outcome, for the report of the task and for the start.
#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure() {
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    for case in cases() {
        let (outcome, _) = outcome(&case);
        if !case.0.contains('.') && case.0.contains(' ') {
            println!("CASE {:<45} {outcome}", case.0);
        }
        *tally.entry(outcome).or_insert(0) += 1;
        total += 1;
    }
    println!("TOTAL {total} cases");
    for (outcome, n) in &tally {
        println!("TALLY {n:>4}  {outcome}");
    }
}
