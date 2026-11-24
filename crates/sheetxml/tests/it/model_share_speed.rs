//! Item 1: **a decode that shares costs the model nothing in speed.** Found that the `.vsdx` reader's table of what it had met (a boxed stream and a
//! `Vec` for each unit) spread the model it built between small allocations of its own, so that a walk of the model, and with it every edit and calculation after a read, was slower
//! than after a read that shared nothing. The decoder of `.diacraft` (`model::share`) has a table of the same shape. A page of 12,000 shapes that are all different is decoded with the
//! table (the open's own budget) and without it ([`Budget::without_sharing`]), each in a process of its own, three times, and the model walked in an order that is not the order it was
//! read in, as a calculation that follows references reads it. **Measured on beast (2026-11-21, load 12 to 16): 994 ticks without the table and 998 with it** (the `.vsdx` reader's old table
//! was 1,254 against 872), so the walk of the cells of a model the decoder built is not slower for its table; what an edit costs after a `.diacraft` open is the engine test `native_speed`'s.
//! (found that the edits were slower after all, by 1.3 to 1.5 times, and why: the table's small allocations were let go at the end of the decode and left holes all over the
//! model that the small objects of the edits were made in, which this walk, that makes none, does not pay for. The table is in arrays and chunks now.)
//! Timed, so it is `#[ignore]`d: `cargo test -p diacraft-sheetxml --test it -- --ignored distinct_shapes_of_a_diacraft_page`.
use diacraft_container::xml::parse_with;
use diacraft_sheetxml::model::{Budget, MemBlobs, URN, decode_page};

use super::model_util::native_limits;

const D: &str = "diacraft";

/// The CPU time the process has used, in hundredths of a second.
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    fields.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + fields.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// A shape of its own on every line (no two are equal, so nothing is shared and the table only grows): six cells, three of them formulas, and a path of five rows of two cells.
fn distinct_shape(i: u32) -> String {
    let rows: String = (1..=5)
        .map(|r| {
            format!(
                "<{D}:Row IX=\"{r}\" T=\"LineTo\"><{D}:Cell N=\"X\" V=\"{r}\" F=\"Width*{r}\"/><{D}:Cell N=\"Y\" V=\"{i}\" F=\"Height*1\"/></{D}:Row>"
            )
        })
        .collect();
    format!(
        "<{D}:shape id=\"{id}\"><{D}:sheet><{D}:Cell N=\"PinX\" V=\"{i}.5\"/><{D}:Cell N=\"PinY\" V=\"2\"/><{D}:Cell N=\"Width\" V=\"1\"/><{D}:Cell N=\"Height\" V=\"0.75\"/><{D}:Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><{D}:Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><{D}:Section N=\"Geometry\" IX=\"0\">{rows}</{D}:Section></{D}:sheet></{D}:shape>",
        id = i + 1
    )
}

/// Decodes `SHARE_SPEED_N` distinct shapes with the table (`SHARE_SPEED_SHARING=1`) or without, and walks every cell of the model two hundred times, in an order that is not the order
/// it was read in: the walk's CPU time, which is what a calculation over the model costs.
#[test]
#[ignore = "run by the test below in a process of its own"]
fn walk_of_a_decoded_page_child() {
    let n: u32 = std::env::var("SHARE_SPEED_N").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sharing = std::env::var("SHARE_SPEED_SHARING").is_ok_and(|v| v == "1");
    let shapes: String = (0..n).map(distinct_shape).collect();
    let el = parse_with(format!("<{D}:page xmlns:{D}=\"{URN}\" id=\"1\">{shapes}</{D}:page>").as_bytes(), &native_limits()).unwrap();
    drop(shapes);
    let huge = 1u64 << 40;
    let budget = if sharing { Budget::new(huge, huge, huge) } else { Budget::new(huge, huge, huge).without_sharing() };
    let page = decode_page(&el, &MemBlobs::new(), &mut { budget }, &mut Vec::new()).unwrap();
    drop(el);
    let start = cpu_ticks();
    let mut seen = 0usize;
    let touch = |name: &str, cell: &diacraft_doc::Cell, seen: &mut usize| {
        *seen = seen.wrapping_add(name.len()).wrapping_add(usize::from(cell.value.is_some()));
        if let diacraft_doc::FormulaSlot::Local(formula) = &cell.formula {
            *seen = seen.wrapping_add(formula.node_count());
        }
    };
    let count = page.shapes.len().max(1);
    for pass in 0..200usize {
        for k in 0..page.shapes.len() {
            let Some(shape) = page.shapes.get((k * 7_919 + pass) % count) else { continue };
            for (name, cell) in &shape.sheet.cells {
                touch(name.as_str(), cell, &mut seen);
            }
            for section in &shape.sheet.sections {
                for row in section.rows.iter() {
                    for (name, cell) in &row.cells {
                        touch(name.as_str(), cell, &mut seen);
                    }
                }
            }
        }
    }
    println!("WALK sharing={sharing} n={n} ticks={} seen={seen}", cpu_ticks() - start);
}

/// **A decode that shares costs the model nothing in speed**: a page of distinct shapes, decoded with a table that finds nothing equal, is walked as fast as the same page decoded
/// without one (the least of three runs of each, in a process of its own, within a quarter).
#[test]
#[ignore = "a timing: it decodes and walks 12,000 shapes six times, two hundred passes each"]
fn distinct_shapes_of_a_diacraft_page_are_walked_as_fast_after_a_decode_that_shares_as_after_one_that_does_not() {
    let least = |sharing: &str| -> u64 {
        (0..3)
            .map(|_| {
                let out = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "model_share_speed::walk_of_a_decoded_page_child", "--ignored", "--nocapture", "--test-threads=1"])
                    .env("SHARE_SPEED_N", "12000")
                    .env("SHARE_SPEED_SHARING", sharing)
                    .output()
                    .unwrap();
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                assert!(out.status.success(), "{text}");
                text.split("ticks=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).expect(&text)
            })
            .min()
            .unwrap()
    };
    let (alone, shared) = (least("0"), least("1"));
    println!("WALK least of three: {alone} ticks without the table, {shared} with it");
    assert!(shared * 100 <= alone * 125 + 100, "a model decoded with the table is walked in {shared} ticks, the same decode without it in {alone}");
}
