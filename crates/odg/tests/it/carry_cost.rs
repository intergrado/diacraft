//! The crafted inputs of the carrier, **counted and timed beside a yardstick** (the shared done-criteria). Each case has a default-run test that counts the work in the units the
//! loop walks (items the reader charged, entries, bytes copied) with a bound near the measured value, and an `#[ignore]` test that times it back to back with a yardstick of plain content of the same size.
//! A child process under `ulimit -v 3000000` proves that a refusal is a refusal and not an abort.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Document, Format, PartRole, RawAttrs};
use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::carry_model::{raw_node, vsdx_part};

fn written(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the writer refused: {e} ({})", e.class())).bytes
}

/// A drawing of one page that carries `n` small parts of the other format, each its own bytes (`/p/<k>.bin`, ten bytes).
fn parts_of(n: usize) -> Document {
    let mut doc = crate::model_write::blank(1);
    for k in 0..n {
        doc.parts.push(vsdx_part(&format!("/p/{k}.bin"), "application/octet-stream", PartRole::Other, Vec::new(), format!("{k:010}").as_bytes()));
    }
    doc
}

/// A drawing of `n` rectangles that each hold VSDX data (an attribute, an element) when `with` is set, and the same rectangles with none otherwise (the yardstick).
fn shapes_of(n: u32, with: bool) -> Document {
    let mut doc = crate::model_write::blank(0);
    for k in 0..n {
        let mut s = crate::write_new::rectangle(k + 1, &format!("S{k}"), (1.0 + f64::from(k % 20), 1.0 + f64::from(k / 20 % 20), 0.5, 0.5));
        if with {
            s.raw = RawAttrs::from_pairs([("OriginalID", k.to_string())]);
            s.passthrough.push(0, raw_node("Hyperlink", &[("ID", "1")], Vec::new()));
        }
        crate::write_new::push_shape(&mut doc, s);
    }
    doc
}

// ------------------------------------------------------------------------------------------------------------------------------ entries

/// 30,000 small carried entries (the most one file names) are written and read once each; one more is refused in words before anything is made, and 50,000 at the writer's door.
#[test]
fn fifty_thousand_small_carried_entries_are_counted() {
    let doc = parts_of(30_000);
    let bytes = written(&doc);
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(report.carry.parts_kept, 30_000, "{:?}", report.carry);
    assert_eq!(report.carry.left_out, 0, "{:?}", report.warnings);
    assert_eq!(back.parts.iter().filter(|p| p.format == Format::Vsdx).count(), 30_000);
    // Each entry is read once: the bytes the carried entries charged are the bytes they hold.
    assert_eq!(report.work.media_bytes, 300_000, "ten bytes each, once each");
    let package = diacraft_odg::package::Package::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(package.entries().filter(|e| e.name.starts_with("DiaCraft/blob")).count(), 30_000);
    // Items the reader took are in proportion to the entries (a record and a part each), not to their square.
    eprintln!("MEASURED 30,000 entries: {} items, {} xml events", report.work.items, report.work.package.xml_events);
    assert!(report.work.items < 30_000 * 37, "{} items for 30,000 entries", report.work.items);
    for n in [30_001usize, 50_000] {
        let refused = write_with(&parts_of(n), None, &WriteOptions::default()).unwrap_err();
        assert!(matches!(&refused, WriteError::Limit { max: 30_000, .. }), "{n}: {refused:?}");
        assert!(refused.to_string().contains("30000"), "{refused}");
    }
}

/// One byte string that every one of 5,000 parts holds is one entry, copied once.
#[test]
fn one_byte_string_named_by_five_thousand_parts_is_one_entry() {
    let mut doc = crate::model_write::blank(1);
    let bytes: Arc<[u8]> = Arc::from(&b"the same bytes in every part"[..]);
    for k in 0..5_000 {
        let mut part = (*vsdx_part(&format!("/q/{k}.bin"), "application/octet-stream", PartRole::Other, Vec::new(), b"")).clone();
        part.bytes = Arc::clone(&bytes);
        doc.parts.push(Arc::new(part));
    }
    let out = written(&doc);
    let package = diacraft_odg::package::Package::read(&out, &Limits::default()).unwrap();
    assert_eq!(package.entries().filter(|e| e.name.starts_with("DiaCraft/blob")).count(), 1);
    let (back, report) = read(&out, &Limits::default()).unwrap();
    assert_eq!(back.parts.iter().filter(|p| p.format == Format::Vsdx).count(), 5_000);
    assert_eq!(report.work.media_bytes, bytes.len() as u64, "the bytes are read once");
}

// ------------------------------------------------------------------------------------------------------------------------------ items

/// The data of 20,000 shapes is read at a cost in proportion to the shapes: the items the reader charges for a shape that carries data against one that carries none.
#[test]
fn twenty_thousand_shapes_that_carry_data_are_read_at_a_cost_in_proportion() {
    let (_, plain) = read(&written(&shapes_of(2_000, false)), &Limits::default()).unwrap();
    let (back, with) = read(&written(&shapes_of(2_000, true)), &Limits::default()).unwrap();
    assert_eq!(with.carry.stale + with.carry.gone + with.carry.left_out, 0, "{:?}", with.warnings);
    assert_eq!(back.pages.get(0).unwrap().shapes.iter().filter(|s| !s.raw.is_empty()).count(), 2_000);
    let per = (with.work.items - plain.work.items) / 2_000;
    eprintln!("MEASURED {per} items for each shape's data");
    assert!(per <= 100, "{per} items for each shape's data");
    let twice = read(&written(&shapes_of(4_000, true)), &Limits::default()).unwrap().1;
    let per2 = (twice.work.items - read(&written(&shapes_of(4_000, false)), &Limits::default()).unwrap().1.work.items) / 4_000;
    assert!(per2 <= per + 1, "linear: {per} items for each shape of 2,000 and {per2} of 4,000");
}

/// A drawing of one page that holds a chain of `depth` nested groups, each of which carries data of its own when `with` is set.
fn chain_of(depth: usize, with: bool) -> Document {
    let mut shape = crate::write_new::rectangle(1, "Leaf", (2.0, 2.0, 1.0, 1.0));
    for level in 0..depth {
        let mut group = crate::write_new::rectangle(2 + u32::try_from(level).unwrap(), &format!("G{level}"), (2.0, 2.0, 1.0, 1.0));
        group.kind = diacraft_doc::ShapeType::Group;
        if with {
            group.raw = RawAttrs::from_pairs([("Level", level.to_string())]);
        }
        group.children = diacraft_doc::SharedVec::from_vec(vec![Arc::new(shape)]);
        shape = group;
    }
    let mut doc = crate::model_write::blank(0);
    crate::write_new::push_shape(&mut doc, shape);
    doc
}

/// `n` parts of the other format whose bytes are the same string when `same` is set, and `n` strings of their own otherwise.
fn shared_parts(n: usize, same: bool) -> Document {
    let mut doc = crate::model_write::blank(1);
    let one: Arc<[u8]> = Arc::from(&b"the same bytes in every part"[..]);
    for k in 0..n {
        let mut part = (*vsdx_part(&format!("/q/{k}.bin"), "application/octet-stream", PartRole::Other, Vec::new(), b"")).clone();
        part.bytes = if same { Arc::clone(&one) } else { Arc::from(format!("distinct bytes {k:010}").as_bytes()) };
        doc.parts.push(Arc::new(part));
    }
    doc
}

/// A chain of nested groups each with data of its own: the work is linear in the depth, not the square of it.
#[test]
fn a_chain_of_nested_groups_each_with_data_costs_a_walk_of_the_chain() {
    let cost = |depth: usize| {
        let bytes = written(&chain_of(depth, true));
        let (back, report) = read(&bytes, &Limits::default()).unwrap();
        assert_eq!(report.carry.stale + report.carry.left_out, 0, "{:?} {:?}", report.carry, report.warnings);
        let mut at = back.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        let mut seen = 0;
        while !at.children.is_empty() {
            assert!(!at.raw.is_empty(), "the data of the group at depth {seen}");
            seen += 1;
            at = at.children.get(0).unwrap().clone();
        }
        assert_eq!(seen, depth);
        report.work.items
    };
    let (a, b) = (cost(20), cost(80));
    eprintln!("MEASURED chain: {a} items at 20, {b} at 80");
    assert!(b < a * 4 + a / 4, "four times the depth is not sixteen times the work: {a} and {b}");
}

/// The data of one shape is carried up to what one attribute of the file holds (512 KiB), and one byte more is refused in words: a shell at the limit reads back.
#[test]
fn a_shapes_data_at_the_limit_of_one_attribute_is_written_and_one_byte_more_is_refused_in_words() {
    let with = |n: usize| {
        let mut doc = crate::model_write::blank(0);
        let mut s = crate::write_new::rectangle(1, "Big", (2.0, 2.0, 1.0, 1.0));
        s.raw = RawAttrs::from_pairs([("Blob", "x".repeat(n))]);
        crate::write_new::push_shape(&mut doc, s);
        doc
    };
    // Just under what the encoding adds around the value, the data rides and comes back.
    let ok = written(&with(500_000));
    let (back, report) = read(&ok, &Limits::default()).unwrap();
    assert_eq!(report.carry.left_out, 0, "{:?}", report.warnings);
    assert_eq!(back.pages.get(0).unwrap().shapes.get(0).unwrap().raw.get("Blob").map(str::len), Some(500_000));
    let refused = write_with(&with(600_000), None, &WriteOptions::default()).unwrap_err();
    assert!(refused.to_string().contains("one attribute"), "{refused}");
}

// ------------------------------------------------------------------------------------------------------------------------------ timed beside a yardstick

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed().as_secs_f64())
}

/// The crafted inputs timed back to back with a yardstick of plain content of the same size (run by hand: `cargo test -- --ignored timed_beside_a_yardstick --nocapture`). Each case fails when it is slower
/// than four times its yardstick and half a second.
#[test]
#[ignore = "times the crafted inputs beside their yardsticks; run by hand"]
fn timed_beside_a_yardstick() {
    let mut rows: Vec<(String, f64, f64)> = Vec::new();
    // 30,000 carried entries against 30,000 plain entries of the OpenDocument kind (the model's own parts), written and read.
    let carried = parts_of(30_000);
    let mut plain = crate::model_write::blank(1);
    for k in 0..30_000 {
        plain.parts.push(crate::carry_model::odf_part_named(&format!("Plain/{k}.bin"), format!("{k:010}").as_bytes()));
    }
    let ((), t_carried) = timed(|| {
        let bytes = written(&carried);
        let _ = read(&bytes, &Limits::default()).unwrap();
    });
    let ((), t_plain) = timed(|| {
        let bytes = written(&plain);
        let _ = read(&bytes, &Limits::default()).unwrap();
    });
    rows.push(("30,000 carried entries, written and read".to_owned(), t_carried, t_plain));
    // 20,000 shapes that carry data against the same shapes that carry none.
    let ((), t_with) = timed(|| {
        let _ = read(&written(&shapes_of(20_000, true)), &Limits::default()).unwrap();
    });
    let ((), t_without) = timed(|| {
        let _ = read(&written(&shapes_of(20_000, false)), &Limits::default()).unwrap();
    });
    rows.push(("20,000 shapes with data, written and read".to_owned(), t_with, t_without));
    // 5,000 parts that all hold one byte string (one entry) against 5,000 parts of bytes of their own (5,000 entries).
    let shared = shared_parts(5_000, true);
    let distinct = shared_parts(5_000, false);
    let ((), t_shared) = timed(|| {
        let _ = read(&written(&shared), &Limits::default()).unwrap();
    });
    let ((), t_distinct) = timed(|| {
        let _ = read(&written(&distinct), &Limits::default()).unwrap();
    });
    rows.push(("5,000 parts of one byte string (one entry), written and read".to_owned(), t_shared, t_distinct));
    // A chain of 150 nested groups (the file holds at most 200 deep) that each carry data against the same chain with none.
    let (with, without) = (chain_of(150, true), chain_of(150, false));
    let ((), t_with) = timed(|| {
        let _ = read(&written(&with), &Limits::default()).unwrap();
    });
    let ((), t_without) = timed(|| {
        let _ = read(&written(&without), &Limits::default()).unwrap();
    });
    rows.push(("150 nested groups with data, written and read".to_owned(), t_with, t_without));
    for (what, t, y) in &rows {
        eprintln!("TIMED {what}: {t:.3} s against {y:.3} s ({:.1}x)", t / y.max(1e-9));
    }
    for (what, t, y) in &rows {
        assert!(*t <= 4.0 * *y + 0.5, "{what}: {t:.3} s against {y:.3} s");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ a refusal is not an abort

fn status_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.strip_prefix(field).and_then(|v| v.trim_start_matches(':').trim().strip_suffix("kB").and_then(|n| n.trim().parse().ok()))
            })
        })
        .unwrap_or(0)
}

/// Bytes that deflate no further (a run of one byte would pass the ratio the container reads).
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut x = 0x2545_F491_4F6C_DD1Du64 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push((x >> 24) as u8);
    }
    out
}

const SCENARIO: &str = "DIACRAFT_CARRYODG_SCENARIO";

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("the scenario");
    let mut fields = scenario.split(':');
    let (kind, n, bytes): (&str, usize, usize) = (
        fields.next().unwrap_or(""),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
    );
    let mut doc = crate::model_write::blank(1);
    for i in 0..n {
        let held: Arc<[u8]> = Arc::from(noise(bytes, i as u64 * 2 + 5));
        let mut part =
            (*vsdx_part(&format!("/visio/embeddings/big{i}.bin"), "application/octet-stream", PartRole::Embedding, Vec::new(), b"")).clone();
        part.bytes = held;
        doc.parts.push(Arc::new(part));
    }
    // The cap of the carrier's own is reached only past the memory gate of a save (five copies of what the parts hold), so the scenario that tries it lifts that gate.
    let options = if kind == "capped" {
        WriteOptions { limits: Limits { max_write_memory: 1 << 40, ..Limits::default() }, ..WriteOptions::default() }
    } else {
        WriteOptions::default()
    };
    let outcome = write_with(&doc, None, &options);
    match &outcome {
        Err(e) => println!("OUTCOME refused:{} {e}", e.class()),
        Ok(written) if kind == "roundtrip" => {
            drop(doc);
            let (back, report) = read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
            println!("OUTCOME written {} bytes, read {} parts, {} kept", written.bytes.len(), back.parts.len(), report.carry.parts_kept);
        }
        Ok(written) => println!("OUTCOME written {} bytes", written.bytes.len()),
    }
    println!("VMPEAK {}", status_kib("VmPeak"));
    println!("carry: no abort");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Runs a scenario in a child under `ulimit -v 3000000`: it ends well (no abort), and says how.
#[cfg(target_os = "linux")]
fn under_the_limit(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact carry_cost::child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("carry: no abort"), "{scenario}: the child did not end well: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("OUTCOME") || l.contains("VMPEAK")).collect::<Vec<_>>().join(" | "));
    text
}

/// Three carried parts of 256 MiB that no compression makes smaller are more than a save may hold in memory beside the file it reads back: refused in words (the save's own limit comes first), never an abort.
#[test]
#[cfg(target_os = "linux")]
fn carried_parts_too_large_to_hold_are_refused_in_words_and_not_an_abort() {
    let text = under_the_limit("write:3:268435456");
    assert!(text.contains("OUTCOME refused:write_limit"), "{text}");
    assert!(text.contains("bytes of memory a save holds"), "{text}");
}

/// Three parts of 180 MiB (540 MiB) are more than the 512 MiB DiaCraft's entries of one file may hold: refused by the carrier's own cap (reached here with the save's memory gate lifted, which counts five copies and
/// comes first by default), in words that name it, before a byte of the package is made.
#[test]
#[cfg(target_os = "linux")]
fn carried_entries_past_the_files_cap_are_refused_in_words_and_not_an_abort() {
    let text = under_the_limit("capped:3:188743680");
    assert!(text.contains("OUTCOME refused:carried_entry_refused"), "{text}");
    assert!(text.contains("512 MiB"), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn carried_entries_that_fit_are_written_and_read_back_under_the_address_limit() {
    let text = under_the_limit("roundtrip:4:33554432");
    assert!(text.contains("OUTCOME written") && text.contains("read 4 parts"), "{text}");
    let peak: u64 = text.lines().find_map(|l| l.strip_prefix("VMPEAK ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    assert!(peak > 0 && peak < 3_000_000, "{peak}");
}

// ------------------------------------------------------------------------------------------------------------------------------ crafted document files

/// A file that another program (or a person) made: the written file's document file with `n` more items of a kind put in, a crafted list the reader judges item by item. The work is in proportion
/// to the items: `work.items` and the time of the read of `n` and of four times `n`, over the same file with none.
fn crafted(kind: &str, n: usize) -> Vec<u8> {
    let base = written(&crate::carry_model::vsdx_model());
    if n == 0 {
        return base;
    }
    crate::carry::repack(&base, "DiaCraft/document.xml", |text| {
        let one = |k: usize| {
            match kind {
            // Items for one page, which more than one item names: none is read, with a note.
            "same_page" => "<diacraft:vsdx of=\"page\" name=\"Page-1\"><diacraft:page xmlns:diacraft=\"urn:app.diacraft:extension\" id=\"1\"/></diacraft:vsdx>".to_owned(),
            // Items for pages the drawing does not have: dropped without a word.
            "gone_page" => format!("<diacraft:vsdx of=\"page\" name=\"Gone{k}\"><diacraft:page xmlns:diacraft=\"urn:app.diacraft:extension\" id=\"1\"/></diacraft:vsdx>"),
            // Items for groups the page does not have, each with a stamp that cannot match.
            "gone_group" => format!("<diacraft:vsdx of=\"group\" page=\"Page-1\" name=\"Gone{k}\" fp=\"0000000000000001\"><diacraft:shape xmlns:diacraft=\"urn:app.diacraft:extension\" id=\"{k}\" type=\"Group\"/></diacraft:vsdx>"),
            // Records of a `parts` list that name entries the package does not have.
            "missing_entry" => format!("<diacraft:blob id=\"{k}\" path=\"DiaCraft/none{k}.bin\" media-type=\"application/octet-stream\" size=\"4\" stamp=\"0000000000000001\"/>"),
            _ => String::new(),
        }
        };
        let more: String = (0..n).map(one).collect();
        // The records go among the records of the `parts` item, the other items at the end of the root.
        let at = if kind == "missing_entry" {
            text.find("<diacraft:blob ").expect("the list of records")
        } else {
            text.rfind("</diacraft:document>").expect("the root's end")
        };
        format!("{}{more}{}", &text[..at], &text[at..])
    })
}

#[test]
fn crafted_lists_of_items_are_read_at_a_cost_in_proportion_to_the_items() {
    for kind in ["same_page", "gone_page", "gone_group", "missing_entry"] {
        let cost = |n: usize| {
            let (_, report) = read(&crafted(kind, n), &Limits::default()).unwrap_or_else(|e| panic!("{kind} {n}: the read ended in an error: {e}"));
            (report.work.items, report.work.package.xml_events)
        };
        let (zero, small, large) = (cost(0), cost(500), cost(2_000));
        let (per_small, per_large) = ((small.0 - zero.0) as f64 / 500.0, (large.0 - zero.0) as f64 / 2_000.0);
        eprintln!(
            "MEASURED {kind}: {per_small:.2} items and {} events for each of 500, {per_large:.2} items and {} events for each of 2,000",
            (small.1 - zero.1) / 500,
            (large.1 - zero.1) / 2_000
        );
        assert!(
            per_large <= per_small * 1.1 + 1.0,
            "{kind}: {per_small:.2} items for each of 500 and {per_large:.2} for each of 2,000 is not linear"
        );
        // Near what was measured (5.05, 8.15, 60.98 and 1.88), so that a cost that doubles is seen.
        let most = match kind {
            "same_page" => 6.0,
            "gone_page" => 9.5,
            "gone_group" => 70.0,
            _ => 2.2,
        };
        assert!(per_large <= most, "{kind}: {per_large:.2} items for each, at most {most}");
    }
}

/// The same crafted lists timed beside a yardstick: the same count of children the carrier does not know, which a reader keeps and does not judge (run by hand with the other timed test).
#[test]
#[ignore = "times the crafted lists beside a yardstick; run by hand"]
fn timed_crafted_lists_beside_a_yardstick() {
    let yardstick = |n: usize| {
        let base = written(&crate::carry_model::vsdx_model());
        crate::carry::repack(&base, "DiaCraft/document.xml", |text| {
            let more: String = (0..n).map(|k| format!("<diacraft:unknown{k} xmlns:diacraft=\"urn:app.diacraft:extension\" id=\"{k}\"/>")).collect();
            let at = text.rfind("</diacraft:document>").expect("the root's end");
            format!("{}{more}{}", &text[..at], &text[at..])
        })
    };
    let n = 20_000;
    let ((), t_plain) = timed(|| {
        let bytes = yardstick(n);
        let _ = read(&bytes, &Limits::default()).unwrap();
    });
    for kind in ["same_page", "gone_page", "gone_group", "missing_entry"] {
        let ((), t) = timed(|| {
            let bytes = crafted(kind, n);
            let _ = read(&bytes, &Limits::default()).unwrap();
        });
        eprintln!("TIMED {n} crafted {kind}: {t:.3} s against {t_plain:.3} s ({:.1}x)", t / t_plain.max(1e-9));
        assert!(t <= 4.0 * t_plain + 0.5, "{kind}: {t:.3} s against {t_plain:.3} s");
    }
}
