//! A black-box check of the line ends against LibreOffice. Not part of the suite: both tests are ignored, and run
//! by hand with `ODG_LE_DIR` set to an empty folder.
//!
//! 1. `lo_ends_write` writes `in/all.odg` (every end 1 to 45 at every size class 0 to 6, on two line weights, one line each, the begin at the class and the end at
//!    the class counted from the other side) and `in/look.odg` (all 45 ends, large enough to see).
//! 2. LibreOffice converts them as a black box: `soffice -env:UserInstallation=file://<folder>/profile --headless --convert-to odg --outdir out in/all.odg`, and
//!    `--convert-to pdf` for `in/look.odg` and for `in/all.odg`; only a missing output file is a failure.
//! 3. `lo_ends_read` reads `out/all.odg` with the reader and counts the numbers and the size classes that come back right.

use std::path::PathBuf;

use diacraft_odg::{Limits, read};

use crate::write_core::{num_of, open, real};
use crate::write_markers::{arrowed, base};
use crate::write_new::push_shape;

fn folder() -> PathBuf {
    PathBuf::from(std::env::var("ODG_LE_DIR").expect("set ODG_LE_DIR to an empty folder"))
}

/// The weights of the matrix, in points: 0.75 and 3, or those `ODG_LE_WEIGHTS` lists, comma-separated.
fn weights() -> Vec<f64> {
    match std::env::var("ODG_LE_WEIGHTS") {
        Ok(list) => list.split(',').map(|w| w.trim().parse().expect("a weight in points")).collect(),
        Err(_) => vec![0.75, 3.0],
    }
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_LE_DIR set"]
fn lo_ends_write() {
    let dir = folder().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    // The matrix.
    let mut o = open(&base());
    let first = o.index.next_id(0).unwrap();
    let mut count = 0u32;
    for n in 1..=45u32 {
        for class in 0..7u32 {
            for weight in weights() {
                let shape = arrowed(first + count, Some((f64::from(n), f64::from(class))), Some((f64::from(n), f64::from(6 - class))), weight);
                push_shape(&mut o.doc, shape);
                count += 1;
            }
        }
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    std::fs::write(dir.join("all.odg"), &saved.bytes).unwrap();
    // The ends to look at: all 45, at class 5 on a 3 pt line, in three columns of fifteen lines (the first line of a column is number 1, 16 and 31).
    let mut o = open(&base());
    let first = o.index.next_id(0).unwrap();
    for k in 0..45usize {
        let (column, row) = (k / 15, k % 15);
        let (x, y) = (1.5 + 7.0 * column as f64, 1.5 + 1.9 * row as f64);
        let mut c = crate::write_new::connector(first + k as u32, (x, y, x + 5.0, y));
        crate::write_markers::set(&mut c, "LineWeight", 3.0 / 72.0);
        crate::write_markers::set(&mut c, "EndArrow", (k + 1) as f64);
        crate::write_markers::set(&mut c, "EndArrowSize", 5.0);
        push_shape(&mut o.doc, c);
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    std::fs::write(dir.join("look.odg"), &saved.bytes).unwrap();
    println!("wrote {count} lines in all.odg and 45 in look.odg");
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_LE_DIR set"]
fn lo_ends_read() {
    let dir = folder();
    let bytes = std::fs::read(dir.join("out").join("all.odg")).expect("out/all.odg: run LibreOffice first");
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    let shapes = &doc.pages.get(0).unwrap().shapes;
    let (mut total, mut number_ok, mut class_ok, mut both_ok) = (0u32, 0u32, 0u32, 0u32);
    let mut wrong: Vec<String> = Vec::new();
    let mut k = 0usize;
    // The shapes of the page, in the order written (the first of the file is the page's own).
    let drawn: Vec<_> = shapes.iter().filter(|s| s.sheet.cell("EndArrow").is_some() || s.sheet.cell("BeginArrow").is_some()).collect();
    println!("{} shapes with an end of {} expected", drawn.len(), 45 * 7 * weights().len());
    for n in 1..=45u32 {
        for class in 0..7u32 {
            for weight in weights() {
                let Some(s) = drawn.get(k) else { break };
                k += 1;
                for (arrow, size, want_class) in [("BeginArrow", "BeginArrowSize", class), ("EndArrow", "EndArrowSize", 6 - class)] {
                    total += 1;
                    let (got_n, got_c) = (num_of(s, arrow), num_of(s, size));
                    let (n_ok, c_ok) = (got_n == f64::from(n), got_c == f64::from(want_class));
                    number_ok += u32::from(n_ok);
                    class_ok += u32::from(c_ok);
                    both_ok += u32::from(n_ok && c_ok);
                    if !(n_ok && c_ok) && wrong.len() < 40 {
                        wrong.push(format!("{arrow} {n} class {want_class} weight {weight}: read {got_n} class {got_c}"));
                    }
                }
            }
        }
    }
    println!("ends read back: {total}; numbers right {number_ok}; classes right {class_ok}; both right {both_ok}");
    println!("kept widths: {}, kept markers: {}", report.kept("marker_width"), report.kept("marker"));
    for line in &wrong {
        println!("  {line}");
    }
}

/// A timing probe for the review, not part of the suite (the suite counts work and never asserts time): the worst drawing the markers allow, every one of the 3,475
/// there are, saved beside a yardstick (the same number of lines with no line end) in the same window, then read back.
#[test]
#[ignore = "a timing probe, run by hand with --no-capture"]
fn lo_ends_time_the_worst_drawing_beside_its_yardstick() {
    use std::time::Instant;
    let unbanded = [2u32, 4, 5, 6, 8, 10, 11, 13, 39, 42];
    let build = |with_ends: bool| {
        let mut o = open(&base());
        let first = o.index.next_id(0).unwrap();
        let mut made = 0u32;
        for n in 1..=45u32 {
            let weights: Vec<f64> =
                if unbanded.contains(&n) { vec![0.75] } else { (1..=99).map(|k| 2.52 * f64::from(k) / f64::from(100 - k)).collect() };
            for weight in weights {
                let shape = if with_ends {
                    arrowed(first + made, None, Some((f64::from(n), 2.0)), weight)
                } else {
                    let mut c = crate::write_new::connector(first + made, (1.0, 10.0, 8.0, 12.0));
                    crate::write_markers::set(&mut c, "LineWeight", weight / 72.0);
                    c
                };
                push_shape(&mut o.doc, shape);
                made += 1;
            }
        }
        o
    };
    for round in 0..3 {
        let plain = build(false);
        let ends = build(true);
        let t = Instant::now();
        let saved_plain = real(&plain.doc, &plain.bytes, &plain.index).unwrap();
        let yardstick = t.elapsed();
        let t = Instant::now();
        let saved = real(&ends.doc, &ends.bytes, &ends.index).unwrap();
        let worst = t.elapsed();
        let t = Instant::now();
        let back = read(&saved.bytes, &Limits::default()).unwrap();
        let reading = t.elapsed();
        println!(
            "round {round}: 3,475 lines with no end saved in {yardstick:?} ({} bytes); with every marker in {worst:?} ({} bytes, {} steps of art); read back in {reading:?} ({} shapes)",
            saved_plain.bytes.len(),
            saved.bytes.len(),
            saved.work.marker_art_steps,
            back.0.pages.get(0).unwrap().shapes.len()
        );
    }
}
