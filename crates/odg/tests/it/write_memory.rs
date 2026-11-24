//! What a save costs in memory (brief, Memory): measured, not claimed, each case in a child process under an
//! address-space limit, as the OPC crate's `memory.rs` runs its children. The parent tests run the child by name under `ulimit -v 3000000` and
//! check how it ended: a process that is out of memory *aborts*, and a child that aborts is a failure of the parent.
//!
//! **What was claimed, and was false.** The first version of this file said that a save over the address space is an error and not an abort. It is
//! not by itself: a save holds the caller's model while it reads the file again to compare the model with, so it holds two models; a file near
//! the reader's caps (the reader's own `memory_every_budget_at_once`, plus one rectangle that moves) read in 2.0 GB (1.98 GB: with 720 MB of parts that were never read, see below) and
//! aborted the save at 2.93 GB. A save now estimates what it will hold from the cost the read left on the index ([`diacraft_odg::ReadCost`]) and is refused as
//! `write_limit` before it reads, when that is more than `Limits::max_write_memory` (2.9 GB by default); a file the estimate admits saves.
//!
//! **What went weak, and was put right (2026-11-19).** The every-budget file of this module (`every_budget_input`) was built as the reader's
//! was, with 720 MB of parts that nothing in the drawing names. A read inflates only the parts the drawing names, so those
//! bytes cost the read nothing and the save's estimate nothing (it takes the inflated bytes from the read; a save copies an entry nothing changed by its compressed
//! bytes, a few kilobytes of zeros): the file held a picture of 250 MB, not a package at its 1 GiB, and read in 1.98 GB where the case said 2.0 to 2.7 GB. The
//! save was refused all the same, which is why nothing failed. The 720 MB are now three pictures of 240 MB that frames show, as the reader's case holds them
//! (`read_hostile::every_budget`), so that all 970 MB of media are read and counted, and the child **asserts what it claims** of the read before it saves: the
//! pictures, the bytes inflated, the items and the tree are at the figures the name says (`every_budget_claims`). A change that makes part of the input cost
//! nothing again fails the child, where it only lowered a peak.
//!
//! The figures each child prints on its `RESULT` line say what its input cost and what the save did with it: `items`, `media`, `inflated` and `tree` are what
//! the read left on the index (the units of the estimate), `gate` is the estimate, `baseline_items` the items of the file read again (0 when the save was refused
//! before it read), and `scanned` and `copied` the bytes the save scanned and copied. A case whose line does not hold what its name claims has gone weak.
//!
//! The measurement of the largest part of the census is an ignored test, run by hand:
//!
//! ```text
//! (ulimit -v 3000000; cargo test -p diacraft-odg --test it write_memory -- --ignored --nocapture --test-threads=1 a_save_of_a_23_mb_part)
//! ```
//!
//! It builds a package whose `content.xml` is about 23 MB (230,000 `draw:rect`s, the size of the largest drawing of the census), reads it, and
//! saves it twice, with a move of the first shape and with a move of the last, and prints on lines starting `RESULT` what each step added to
//! what was resident (`VmHWM` after a reset of the peak through `/proc/self/clear_refs`) and what it had in all. A machine with no `/proc`
//! fails loudly: it measures nothing there, and a green run would say it had.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Value};
use diacraft_odg::{Limits, OriginalPackage, ReadCost, ShapeIndex, WriteOptions, WriteWork, read_partial, write_counted, write_with};

use crate::drawing::Drawing;
use crate::read_hostile::{SHAPES_AT_ONCE, part_by_part, picture, unshared_rects};
use crate::write_core::{Open, PNG, assert_reads_back, move_by, open_bytes};

const SCENARIO: &str = "ODG_WRITE_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes: a missing one is a failure and not a zero.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// Forgets the peak so far and gives what is resident. A system that refuses the reset leaves the peak holding the earlier steps, and the
/// line says `unreset`.
fn begin() -> (u64, bool) {
    let reset = std::fs::write("/proc/self/clear_refs", "5").is_ok();
    (status_kib("VmRSS"), reset)
}

fn report(what: &str, (base, reset): (u64, bool)) {
    let peak = status_kib("VmHWM");
    let added = if reset { format!("{}KiB", peak.saturating_sub(base)) } else { "unreset".to_owned() };
    println!("\nRESULT {what} Added={added} VmHWM={peak}KiB VmRSS={}KiB VmPeak={}KiB", status_kib("VmRSS"), status_kib("VmPeak"));
}

/// A package of one page of `n` rectangles, the `k`-th at `k` to a few centimetres.
fn rects(n: usize) -> Vec<u8> {
    let body: String = (1..=n)
        .map(|k| {
            format!(
                "\n    <draw:rect draw:name=\"s{k}\" svg:x=\"{}.{}cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"/>",
                k % 997,
                k % 89 + 1
            )
        })
        .collect();
    Drawing::a4().page(&body).bytes()
}

/// The pictures of the every-budget file, in bytes: one of 250 MB shown by four frames and three of 240 MB shown by one frame each. Every one of them is a part the
/// drawing names, so the read inflates it and counts it (`ReadCost::media_bytes` and `inflated_bytes`): **parts nothing names are not inflated, and would cost
/// nothing** (the file held 720 MB of them before a change).
const EVERY_BUDGET_MEDIA: u64 = 250_000_000 + 3 * 240_000_000;

/// What the every-budget file must cost the read for the case to hold what its name says: all of its media read and inflated, the items near their cap of 20 million
/// and a tree near its cap of 4 million. Asserted by the child before it saves, from the cost the read left on the index.
fn every_budget_claims(cost: &ReadCost) {
    assert_eq!(cost.media_bytes, EVERY_BUDGET_MEDIA, "every picture of the file is read and counted once: {cost:?}");
    assert!(cost.inflated_bytes >= EVERY_BUDGET_MEDIA, "the read inflated every picture, and the parts besides: {cost:?}");
    assert!(cost.items >= 19_500_000, "the items are near their cap of 20 million: {cost:?}");
    assert!(cost.tree_nodes >= 3_900_000, "a tree is near its cap of 4 million: {cost:?}");
}

/// The reader's every-budget input (`read_hostile::every_budget`: the package at its 1 GiB **and all of it parts the drawing names**, both trees near their caps, the
/// items and the text near theirs, a 250 MB picture shown by four frames and three of 240 MB shown by one frame each) with one rectangle, `mover`, first on the page.
fn every_budget_input() -> Vec<u8> {
    let polygon = r#"<draw:regular-polygon svg:width="1cm" svg:height="1cm" draw:corners="49000" draw:concave="true" draw:sharpness="50%"/>"#;
    let frame = r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/big.png"/></draw:frame>"#;
    let spaces =
        format!(r#"<draw:rect svg:width="1cm" svg:height="1cm"><text:p>{}</text:p></draw:rect>"#, r#"<text:s text:c="65536"/>"#.repeat(1_023));
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:filler""#);
    d.fonts = format!(r#"<style:font-face style:name="F" svg:font-family="{}"/>"#, "f".repeat(1_000_000));
    let named: String = (0..4_000)
        .map(|k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:text-properties style:font-name="F"/></style:style>"#))
        .collect();
    // **The filler is held ** (a child of `office:master-styles` and of `office:drawing` that nothing reads is kept for the `.diacraft` copy), at 13 items for each
    // child that stands alone and 3 for each inside one that holds them all (`unread.rs`): the filler is **one element that holds the nodes**, so that the tree is near its cap with
    // the items still the path points' (the case was 154,999 corners in each of two polygons with the filler uncharged; it is 49,000, since the filler now takes 11.3 million items).
    d.common = named;
    d.masters.push_str(&format!("<x:w>{}</x:w>", "<x:i/>".repeat(diacraft_odg::limits::MAX_READ_XML_NODES / 2 - 20_000)));
    let mover = r#"<draw:rect draw:name="mover" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#;
    // The big picture shown by four frames, and each of the three others by one.
    let frames = frame.repeat(4) + &["b", "c", "d"].iter().map(|n| frame.replace("big", n)).collect::<String>();
    d.pages = format!(
        r#"<draw:page draw:name="p" draw:master-page-name="Default">{mover}{frames}{polygon}{polygon}{spaces}{}</draw:page><x:w>{}</x:w>"#,
        unshared_rects(SHAPES_AT_ONCE),
        "<x:i/>".repeat(diacraft_odg::limits::MAX_READ_XML_NODES / 2 - 60_000)
    );
    let extra = ["Pictures/big.png", "Pictures/b.png", "Pictures/c.png", "Pictures/d.png"];
    part_by_part(d, &extra, &|name| picture(if name.ends_with("big.png") { 250_000_000 } else { 240_000_000 }))
}

/// The review's input: `heavy` rectangles each with a foreign attribute of 1 MB of random letters (which neither compresses nor
/// shares), then `rows` custom shapes of every row kind, and one rectangle, `mover`, first on the page. With 250 and 175 the file is about 189 MB
/// packed, its `content.xml` 254 MB, and its reader's items 16.6 million: the reader reads it under `ulimit -v 3000000`, and the save once
/// aborted the process in the buffer it deflated the part into.
fn kept_random_input(heavy: usize, rows: usize) -> Vec<u8> {
    let mut d = Drawing::a4();
    d.ns.push_str(r#" xmlns:x="urn:example:x""#);
    let mut seed = 12_345u64;
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut random = |len: usize| -> String {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                alphabet[((seed >> 33) % 62) as usize] as char
            })
            .collect()
    };
    let kept: String = (0..heavy)
        .map(|k| format!(r#"<draw:rect x:data="{}" svg:x="{}mm" svg:width="1cm" svg:height="1cm"/>"#, random(1_000_000), k % 100))
        .collect();
    let points: String = (0..2_000).map(|i| format!("{} {}", i * 10, i * 5)).collect::<Vec<_>>().join(" ");
    let path = "M 0 0 L 100 100 C 1 2 3 4 5 6 Q 1 2 3 4 U 10800 10800 100 100 0 360 W 0 0 100 100 10 10 50 50 ".repeat(40);
    let custom: String = (0..rows)
        .map(|k| {
            format!(
                r#"<draw:custom-shape svg:x="{}mm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="1 2" draw:glue-points="{points}" draw:text-areas="0 0 21600 10800" draw:enhanced-path="{path}"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry><text:p>t</text:p></draw:custom-shape>"#,
                k % 200
            )
        })
        .collect();
    let mover = r#"<draw:rect draw:name="mover" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#;
    d.page(&format!("{mover}{kept}{custom}")).bytes()
}

/// What a read of the input cost and what the save did with it, as fields of a `RESULT` line: the items, the media, the bytes inflated and the tree the read left on
/// the index (the units the estimate is made of), and the bytes the save scanned and copied. A case's name and doc claim these; the line is how a rerun sees whether
/// they still hold (a save refused before it read has scanned and copied nothing).
fn cost_fields(cost: &ReadCost, work: &WriteWork) -> String {
    format!(
        "items={} media={} inflated={} tree={} scanned={} copied={}",
        cost.items, cost.media_bytes, cost.inflated_bytes, cost.tree_nodes, work.scan_bytes, work.bytes_copied
    )
}

/// Reads `bytes` as the caller of a save does, moves the first shape and saves, printing on a `RESULT` line how it ended, what the save
/// estimated it would hold, what the input cost the read (`cost_fields`) and the process's peak. `index` is the one a read made unless the scenario passes
/// another. Gives how the save ended and what the read cost.
fn save_and_report(scenario: &str, bytes: Vec<u8>, index_of: impl FnOnce(ShapeIndex) -> ShapeIndex) -> (Result<usize, &'static str>, ReadCost) {
    let (read, report_of_read) = read_partial(&bytes, &Limits::default());
    let Ok(doc) = read else { panic!("the file did not read: {:?}", read.err().map(|e| e.class())) };
    let read_cost = *report_of_read.index.read_cost();
    let mut o = Open { bytes, doc, index: index_of(report_of_read.index.clone()) };
    drop(report_of_read);
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let reader_figure = o.index.read_cost().save_bytes();
    // A scenario named `..._open` runs with the gate open, which is how the allocations behind it are tried: they must fail as an error.
    let limits = if scenario.ends_with("_open") { Limits { max_write_memory: u64::MAX, ..Limits::default() } } else { Limits::default() };
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), limits, ..WriteOptions::default() };
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    let how = match &saved {
        Ok(w) => Ok(w.bytes.len()),
        Err(e) => Err(e.class()),
    };
    println!(
        "\nRESULT {scenario} class={} gate={} reader_figure={reader_figure} file={} {} baseline_items={} VmHWM={}KiB VmPeak={}KiB",
        how.as_ref().err().copied().unwrap_or("saved"),
        work.estimated_memory,
        o.bytes.len(),
        cost_fields(&read_cost, &work),
        work.baseline_read.items,
        status_kib("VmHWM"),
        status_kib("VmPeak")
    );
    // A save that was made gives a file the reader reads: checked with everything the save held let go, so that the check costs one read.
    let made = saved.ok().map(|w| w.bytes);
    drop(o);
    if let Some(bytes) = made {
        assert_reads_back(scenario, &bytes);
    }
    (how, read_cost)
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    match scenario.as_str() {
        // The reader's every-budget input: it reads, and the save that follows is refused with a class, where it once aborted the process.
        "every_budget_refused" => {
            let (how, cost) = save_and_report(&scenario, every_budget_input(), |index| index);
            every_budget_claims(&cost);
            assert_eq!(how.as_ref().err().copied(), Some("write_limit"));
        }
        // The same file with the index nothing read (the only one code outside the crate can make): refused before a byte of it is read.
        "every_budget_default_index" => {
            let (how, cost) = save_and_report(&scenario, every_budget_input(), |_| ShapeIndex::default());
            every_budget_claims(&cost);
            assert_eq!(how.as_ref().err().copied(), Some("write_mismatch"));
        }
        // A drawing of 300,000 rectangles, 16.5 million items, near the most the save's gate admits of them (308,000 at 64 bytes an item; 330,000 saved at
        // 54, and the most was about 349,000): it saves.
        "rects_300k_saves" => {
            let mut o = open_bytes(rects(300_000));
            move_by(&mut o.doc, 0, 300_000, 1.0, 0.0);
            let mut work = WriteWork::default();
            let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work)
                .unwrap_or_else(|e| panic!("the save of 300,000 rectangles was refused: {} ({e})", e.class()));
            assert!(saved.edited);
            println!(
                "\nRESULT rects_300k_saves class=saved gate={} reader_figure={} file={} {} baseline_items={} VmHWM={}KiB VmPeak={}KiB",
                work.estimated_memory,
                o.index.read_cost().save_bytes(),
                o.bytes.len(),
                cost_fields(o.index.read_cost(), &work),
                work.baseline_read.items,
                status_kib("VmHWM"),
                status_kib("VmPeak")
            );
            let made = saved.bytes;
            drop(o);
            assert_reads_back("rects_300k_saves", &made);
        }
        // Security finding (MEDIUM): a paragraph of a thousand `text:s` of `text:c="999999"` each, which the reader clamps to 65,536 and reads as 65 MB of
        // spaces (under its cap of 64 MiB). A retext of it was a million characters built for each element and a copy of the paragraph as characters
        // four bytes each; it is now refused in words before anything is built, and the process stays small. A paragraph of five such elements, which
        // the retext holds, is written and reads back.
        "retext_many_spaces" => {
            let spaces = |n: usize| format!("<text:p>a{}b</text:p>", r#"<text:s text:c="999999"/>"#.repeat(n));
            let geometry = r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/>"#;
            let shape = |n: usize| {
                format!(
                    r#"<draw:custom-shape draw:name="s" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm">{}{geometry}</draw:custom-shape>"#,
                    spaces(n)
                )
            };
            let retype = |o: &mut Open| {
                let s = crate::write_core::shape_mut(&mut o.doc, 0, 1);
                let parts = s.text.as_ref().expect("text").parts.clone();
                let parts = parts
                    .into_iter()
                    .map(|p| match p {
                        diacraft_doc::TextPart::Chars(c) if c.ends_with('b') => {
                            diacraft_doc::TextPart::Chars(Arc::from(format!("{}c", &c[..c.len() - 1])))
                        }
                        other => other,
                    })
                    .collect();
                s.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
            };
            let mut o = open_bytes(Drawing::a4().page(&shape(1_000)).bytes());
            assert!(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().text.is_some(), "the reader read the paragraph");
            retype(&mut o);
            let refused = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default());
            let class = refused.as_ref().err().map_or("saved", |e| e.class());
            println!("\nRESULT retext_many_spaces class={class} VmHWM={}KiB", status_kib("VmHWM"));
            assert_eq!(class, "write_unsupported", "{:?}", refused.map(|w| w.bytes.len()));
            drop(o);
            let mut o = open_bytes(Drawing::a4().page(&shape(5)).bytes());
            retype(&mut o);
            let saved = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default())
                .expect("a paragraph of five saves");
            let back = open_bytes(saved.bytes);
            let text = |d: &diacraft_doc::Document| match d.pages.get(0).unwrap().shapes.get(0).unwrap().text.as_ref().unwrap().parts.first() {
                Some(diacraft_doc::TextPart::Chars(c)) => c.to_string(),
                _ => String::new(),
            };
            assert!(text(&back.doc).ends_with('c') && text(&back.doc).chars().count() == 2 + 5 * 65_536, "{}", text(&back.doc).chars().count());
        }
        // A paragraph of 60 MiB of raw white space (clean, peak 316 MB, and refused as a mismatch). It is written: the white
        // space of raw text is one character to the reader and to the writer, counted as such before anything is built.
        "retext_huge_white_space" => {
            let geometry = r#"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/>"#;
            let spaces = " ".repeat(60 << 20);
            let body = format!(
                r#"<draw:custom-shape draw:name="s" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:p>a{spaces}b</text:p>{geometry}</draw:custom-shape>"#
            );
            drop(spaces);
            let mut o = open_bytes(Drawing::a4().page(&body).bytes());
            drop(body);
            let s = crate::write_core::shape_mut(&mut o.doc, 0, 1);
            s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from("a c"))], ..Default::default() }));
            let saved = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default());
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!("\nRESULT retext_huge_white_space class={class} VmHWM={}KiB", status_kib("VmHWM"));
            if let Ok(made) = &saved {
                assert_reads_back("retext_huge_white_space", &made.bytes);
            }
            assert_eq!(class, "saved", "{:?}", saved.map(|w| w.bytes.len()));
        }
        // The review's input: 150,000 rectangles read, then 1.9 GB of text put into their cells after the read. The model grew past what
        // the read's cost says, which the gate once did not count; it is refused before the file is read again and before a shape is compared.
        "grown_by_text_refused" => {
            let mut o = open_bytes(rects(150_000));
            let big = "x".repeat(12_700);
            {
                let page = Arc::make_mut(o.doc.pages.get_mut(0).expect("a page"));
                for at in 0..150_000usize {
                    let shape = Arc::make_mut(page.shapes.get_mut(at).expect("a shape"));
                    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text(&big)));
                }
            }
            move_by(&mut o.doc, 0, 1, 1.0, 0.0);
            let mut work = WriteWork::default();
            let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!(
                "\nRESULT grown_by_text_refused class={class} gate={} grown={} compared={} baseline_items={} {} VmHWM={}KiB VmPeak={}KiB",
                work.estimated_memory,
                work.grown_bytes,
                work.shapes_compared,
                work.baseline_read.items,
                cost_fields(o.index.read_cost(), &work),
                status_kib("VmHWM"),
                status_kib("VmPeak")
            );
            assert_eq!(class, "write_limit");
            assert_eq!(work.shapes_compared, 0, "refused before a shape was compared");
            assert_eq!(work.baseline_read.items, 0, "refused before the file was read again");
            assert!(work.grown_bytes >= 1_900_000_000, "the growth is the text put in: {}", work.grown_bytes);
        }
        // 330,000 rectangles read, then 1,200 `User` rows named 1 MiB each put on one shape (1.26 GB of row names, which the
        // figures did not see: the gate passed it and the save aborted the process at the second read, status 134). Refused as `write_limit` before
        // the file is read again and before a shape is compared.
        "rows_grown_refused" => {
            let mut o = open_bytes(rects(330_000));
            let name = "y".repeat(1 << 20);
            {
                let page = Arc::make_mut(o.doc.pages.get_mut(0).expect("a page"));
                let shape = Arc::make_mut(page.shapes.get_mut(5).expect("a shape"));
                let mut user = diacraft_doc::Section::new("User", None);
                for k in 0..1_200 {
                    user.rows.push(Arc::new(diacraft_doc::Row { name: Some(Arc::from(format!("{name}{k}"))), ..diacraft_doc::Row::default() }));
                }
                Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
            }
            drop(name);
            move_by(&mut o.doc, 0, 1, 1.0, 0.0);
            let mut work = WriteWork::default();
            let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!(
                "\nRESULT rows_grown_refused class={class} gate={} grown={} compared={} baseline_items={} {} VmHWM={}KiB VmPeak={}KiB",
                work.estimated_memory,
                work.grown_bytes,
                work.shapes_compared,
                work.baseline_read.items,
                cost_fields(o.index.read_cost(), &work),
                status_kib("VmHWM"),
                status_kib("VmPeak")
            );
            assert_eq!(class, "write_limit");
            assert_eq!(work.shapes_compared, 0, "refused before a shape was compared");
            assert_eq!(work.baseline_read.items, 0, "refused before the file was read again");
            assert!(work.grown_bytes >= 1_200 * (1 << 20), "the growth is the names put in: {}", work.grown_bytes);
        }
        // The review's input: 250 shapes with a foreign attribute of 1 MB of random letters each, and 175 custom shapes of every row
        // kind. A clean refusal, a class and no abort; and with 100 such shapes a save or a clean refusal.
        "kept_random_250" => {
            let (how, _) = save_and_report(&scenario, kept_random_input(250, 175), |index| index);
            assert!(matches!(how.as_ref().err().copied(), Some("write_limit" | "out_of_memory")), "{how:?}");
        }
        "kept_random_250_open" => {
            let (how, _) = save_and_report(&scenario, kept_random_input(250, 175), |index| index);
            assert!(how.is_ok() || matches!(how.as_ref().err().copied(), Some("write_limit" | "out_of_memory")), "{how:?}");
        }
        "kept_random_100" => {
            let (how, _) = save_and_report(&scenario, kept_random_input(100, 175), |index| index);
            assert!(how.is_ok() || matches!(how.as_ref().err().copied(), Some("write_limit" | "out_of_memory")), "{how:?}");
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact write_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// The peak (`VmHWM`) a child printed on its `RESULT` line, in KiB.
fn peak_of(stdout: &str) -> u64 {
    stdout.split("VmHWM=").nth(1).and_then(|rest| rest.split("KiB").next()).and_then(|n| n.parse().ok()).unwrap_or(u64::MAX)
}

/// A child's  line, in words: what it ended as, the gate's estimate and the peak it reached (the report's table is made of these).
fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

/// The number after  on a child's  line.
fn field_of(stdout: &str, key: &str) -> u64 {
    let line = result_line(stdout);
    line.split(key)
        .nth(1)
        .and_then(|rest| rest.trim_start_matches('=').split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(u64::MAX)
}

#[test]
fn a_save_of_the_every_budget_file_is_refused_with_a_class_under_the_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("every_budget_refused", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT every_budget_refused class=write_limit"), "{stdout}");
    // Refused before the second read, not after it: the peak is the first read's (2.66 GB measured on beast, 2026-11-19, release: the pictures are read), not two
    // models' (a second read does not fit the child's address space at all; the estimate for this file is 6.1 GB).
    assert!(peak_of(&stdout) < 2_800_000, "peak {} KiB: the file was read again", peak_of(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_model_grown_by_2_gb_of_cell_text_after_the_read_is_refused_before_the_file_is_read_again_and_does_not_abort() {
    // The gate counted what the read cost and not what the caller had done to the model since.
    let (ended_well, stdout) = run_child("grown_by_text_refused", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT grown_by_text_refused class=write_limit"), "{stdout}");
    assert!(field_of(&stdout, "grown") >= 1_900_000_000, "{}", result_line(&stdout));
    assert_eq!(field_of(&stdout, "compared"), 0, "{}", result_line(&stdout));
    // The model and its text, and not a second model: the peak is the growth and the first read.
    assert!(peak_of(&stdout) < 2_800_000, "peak {} KiB", peak_of(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_model_grown_by_1_2_gb_of_row_names_after_the_read_is_refused_before_the_file_is_read_again_and_does_not_abort() {
    // The figures counted cells and text and not rows, sections, kept attributes or names.
    let (ended_well, stdout) = run_child("rows_grown_refused", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT rows_grown_refused class=write_limit"), "{stdout}");
    assert!(field_of(&stdout, "grown") >= 1_200 * (1 << 20), "{}", result_line(&stdout));
    assert_eq!(field_of(&stdout, "compared"), 0, "{}", result_line(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_retext_of_a_paragraph_of_a_thousand_counted_spaces_is_refused_before_anything_is_built_and_does_not_abort() {
    let (ended_well, stdout) = run_child("retext_many_spaces", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT retext_many_spaces class=write_unsupported"), "{stdout}");
    // The model of a 65 MB paragraph, the file read again and the part: well under a second paragraph built as characters.
    assert!(peak_of(&stdout) < 1_500_000, "peak {} KiB", peak_of(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_retext_of_a_paragraph_of_60_mib_of_white_space_is_written_and_does_not_abort() {
    let (ended_well, stdout) = run_child("retext_huge_white_space", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT retext_huge_white_space class=saved"), "{stdout}");
    assert!(peak_of(&stdout) < 1_500_000, "peak {} KiB", peak_of(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_save_of_300000_rectangles_still_saves_under_the_limit() {
    let (ended_well, stdout) = run_child("rects_300k_saves", 3_000_000);
    assert!(ended_well, "{stdout}");
    assert!(stdout.contains("RESULT rects_300k_saves class=saved"), "{stdout}");
    // 1.78 GB measured (beast, 2026-11-22, the test profile: 16.5 million items, the part scanned and copied, 60 MB; 1.99 GB before, for 18.2 million
    // items, release); the gate's figure, 2.82 GB at 64 bytes an item, is above it and under the cap (2.9 GB).
    assert!(peak_of(&stdout) < 2_600_000, "peak {} KiB", peak_of(&stdout));
    assert!(peak_of(&stdout) * 1_024 <= field_of(&stdout, "gate"), "peak {} KiB over the estimate {}", peak_of(&stdout), field_of(&stdout, "gate"));
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_save_of_the_file_with_250_big_attributes_is_refused_with_a_class_and_does_not_abort() {
    // The re-review's input: a 189 MB file whose cost is in text the reader's figures do not see. The save aborted at 2.75 GB.
    let (ended_well, stdout) = run_child("kept_random_250", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("class=write_limit") || stdout.contains("class=out_of_memory"), "{stdout}");
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_save_of_the_file_with_250_big_attributes_past_the_gate_never_aborts_where_memory_runs_out() {
    // With the gate open the estimate stops nothing, and the allocations behind it are what is tried. At 2.6 GB of address space the read of the
    // file again fits (it maps 2.36 GB) and the edit does not; at 3.0 GB the edit's buffers just fit (it maps 2.98 GB: 15 MB, half a percent, to spare, measured on
    // beast, 2026-11-19, release; a model that grew by that much would end this run as an error with a class, which the case accepts), where on the code before
    // the buffer the part is deflated into doubled to 256 MiB and aborted the process. Each ends as a save or as an error with a class.
    for limit in [2_600_000, 3_000_000] {
        let (ended_well, stdout) = run_child("kept_random_250_open", limit);
        assert!(ended_well, "the child aborted at {limit} KiB, or its assertion failed: {stdout}");
        assert!(stdout.contains("RESULT kept_random_250_open"), "{stdout}");
        // The case tries the allocations that fail, so at the smaller limit the save must end as the error: a save there would mean the case no longer reaches the
        // point where memory runs out, and has to be given a lower limit.
        if limit == 2_600_000 {
            assert!(stdout.contains("class=out_of_memory"), "at {limit} KiB the edit does not fit, and ends as an error of its own class: {stdout}");
        }
        eprintln!("at {limit} KiB: {}", result_line(&stdout));
    }
}

#[test]
fn a_save_of_the_file_with_100_big_attributes_is_made_or_refused_with_a_class() {
    let (ended_well, stdout) = run_child("kept_random_100", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("RESULT kept_random_100"), "{stdout}");
    // A save that is made peaked under what the gate said it would hold.
    if stdout.contains("class=saved") {
        assert!(
            peak_of(&stdout) * 1_024 <= field_of(&stdout, "gate"),
            "peak {} KiB over the estimate {}",
            peak_of(&stdout),
            field_of(&stdout, "gate")
        );
    }
    eprintln!("{}", result_line(&stdout));
}

#[test]
fn a_save_with_the_index_nothing_read_is_refused_before_the_file_is_read() {
    //  is the only index code outside the crate can make: with the every-budget file it carries no cost to go by, and the
    // read it used to start aborted the process.
    let (ended_well, stdout) = run_child("every_budget_default_index", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert!(stdout.contains("class=write_mismatch"), "{stdout}");
    // The peak is the first read's (2.66 GB measured, as above): nothing was read again.
    assert!(peak_of(&stdout) < 2_800_000, "peak {} KiB", peak_of(&stdout));
    eprintln!("{}", result_line(&stdout));
}

#[test]
#[ignore = "a measurement of a 23 MB part, run by hand under `ulimit -v`; see the module's documentation"]
fn a_save_of_a_23_mb_part_at_its_start_and_at_its_end() {
    const SHAPES: usize = 230_000;
    let bytes = rects(SHAPES);
    println!("\nPACKAGE {} bytes packed, {SHAPES} shapes", bytes.len());

    let mark = begin();
    let mut o = open_bytes(bytes);
    report("read", mark);

    for (what, id) in [("save, move of the first shape", 1u32), ("save, move of the last shape", SHAPES as u32)] {
        move_by(&mut o.doc, 0, id, 1.0, 0.0);
        let mark = begin();
        let options = WriteOptions { thumbnail: Some(PNG.to_vec()), limits: Limits::default(), ..WriteOptions::default() };
        let saved = write_with(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options)
            .unwrap_or_else(|e| panic!("the save did not finish: {} ({e})", e.class()));
        assert!(saved.edited, "{what}: the move is an edit");
        println!(
            "\nWORK {what}: scanned {} copied {} elements {} edits {} gate {} items {}",
            saved.work.scan_bytes,
            saved.work.bytes_copied,
            saved.work.elements_walked,
            saved.work.edits,
            saved.work.estimated_memory,
            saved.index.read_cost().items
        );
        report(what, mark);
        // The next step starts from the saved package, as a second save does.
        o.bytes = saved.bytes;
        o.index = saved.index;
    }
    // The package the last save made is read as the reader reads one (the first was read by the second save, as its original).
    let made = std::mem::take(&mut o.bytes);
    drop(o);
    assert_reads_back("a 23 MB part", &made);
}
