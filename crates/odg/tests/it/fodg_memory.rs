//! What reading and saving a `.fodg` costs in memory: each case in a child process under an address-space limit
//! (`ulimit -v 3000000`), as `write_memory.rs` runs its children. A process that is out of memory *aborts*, and a child that aborts is a failure of the
//! parent: a save that cannot have the memory must end as an error of a class, never as an abort.
//!
//! Three things are measured: a document of 300,000 shapes (30 MB of XML, near the most the save's gate admits of rectangles: 302,000 at 64 bytes an item;  330,000 saved at 54)
//! saves; a document of one inline
//! picture of 150 MB of base64 (the one part is past what a save may hold beside its models) ends as a save or as `write_limit`, and, with the gate open (the
//! allocations behind it tried), as a save or an error of a class; and a package of a 100 MB picture ends as a save or a limit.

#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_odg::{Form, Limits, OriginalPackage, WriteOptions, WriteWork, read, write_counted};

use crate::drawing::Drawing;
use crate::flat::{Flat, frame, picture};
use crate::write_core::{assert_reads_back, custom, move_by, open_bytes};

const SCENARIO: &str = "ODG_FODG_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("this test needs /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status"))
}

/// A flat document of `n` rectangles, the last the one a test moves.
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
    Flat::of(&Drawing::a4().page(&body)).bytes()
}

/// A flat document of one inline picture of `megabytes` MB of base64 text (a PNG's signature and then filler), and a shape that moves.
fn big_picture(megabytes: usize) -> Vec<u8> {
    let mut png = picture(64);
    png.resize(megabytes * 1_000_000 / 4 * 3, 7);
    let text = crate::flat::base64_lines(&png, 76, "\n");
    let image = format!("<draw:image draw:mime-type=\"image/png\"><office:binary-data>{text}</office:binary-data></draw:image>");
    let mut d = Drawing::a4().page(&format!("{}{}", frame(&image), custom("mover", "1cm")));
    d.meta = Some(crate::write_core::full_meta());
    Flat::of(&d).bytes()
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    match scenario.as_str() {
        "rects_300k_saves" => {
            let mut o = open_bytes(rects(300_000));
            move_by(&mut o.doc, 0, 300_000, 1.0, 0.0);
            let mut work = WriteWork::default();
            let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
                .unwrap_or_else(|e| panic!("the save of 300,000 rectangles was refused: {} ({e})", e.class()));
            assert!(saved.edited);
            println!("\nRESULT rects_300k_saves class=saved gate={} VmHWM={}KiB", work.estimated_memory, status_kib("VmHWM"));
            // The document made is read as the reader reads one, with what the save held let go.
            let made = saved.bytes;
            drop(o);
            assert_reads_back("rects_300k_saves", &made);
        }
        "picture_150mb" | "picture_150mb_gate_open" => {
            let bytes = big_picture(150);
            let Ok((doc, report)) = read(&bytes, &Limits::default()) else { panic!("the document did not read") };
            let mut o = crate::write_core::Open { bytes, doc, index: report.index };
            let mover = o.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("mover")).map(|s| s.id).unwrap();
            move_by(&mut o.doc, 0, mover, 1.0, 0.0);
            let limits = if scenario.ends_with("gate_open") { Limits { max_write_memory: u64::MAX, ..Limits::default() } } else { Limits::default() };
            let mut work = WriteWork::default();
            let saved = write_counted(
                &o.doc,
                Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }),
                &WriteOptions { limits, ..WriteOptions::default() },
                &mut work,
            );
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!("\nRESULT {scenario} class={class} gate={} VmHWM={}KiB", work.estimated_memory, status_kib("VmHWM"));
            // Saved, or refused as a limit or as memory: never anything else, and never an abort.
            assert!(["saved", "write_limit", "out_of_memory"].contains(&class), "{class}");
            if let Ok(made) = saved {
                drop(o);
                assert_reads_back(&scenario, &made.bytes);
            }
        }
        "package_of_big_pictures" => {
            // A package whose one picture, named by an image, is 100 MB that does not deflate: the conversion holds it several times over.
            let frame = frame("<draw:image xlink:href=\"Pictures/big.png\"/>");
            let mut d = Drawing::a4().page(&frame);
            // Bytes that do not deflate to nothing (a picture of one byte repeated is past the ratio the container allows): a generator's, 100 MB of them.
            let mut big = picture(64);
            let mut x = 0x2545_F491_4F6C_DD1Du64;
            big.extend((0..100_000_000).map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            }));
            d.files.push(("Pictures/big.png".to_owned(), big));
            let bytes = d.bytes();
            let Ok((doc, report)) = read(&bytes, &Limits::default()) else { panic!("the package did not read") };
            let saved = write_counted(
                &doc,
                Some(&OriginalPackage { bytes: &bytes, index: &report.index }),
                &WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() },
                &mut WriteWork::default(),
            );
            let class = saved.as_ref().err().map_or("saved", |e| e.class());
            println!("\nRESULT package_of_big_pictures class={class} VmHWM={}KiB", status_kib("VmHWM"));
            // Saved, or refused as a limit (by the conversion's estimate, or by the gate of the save that follows it): never an abort.
            assert!(["saved", "write_limit"].contains(&class), "{class}");
            if let Ok(made) = saved {
                drop((doc, report, bytes));
                assert_reads_back("package_of_big_pictures", &made.bytes);
            }
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact fodg_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

#[test]
fn a_save_of_300000_rectangles_in_a_fodg_still_saves_under_the_address_limit() {
    let (ended_well, stdout) = run_child("rects_300k_saves", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let result = stdout.lines().find(|l| l.starts_with("RESULT rects_300k_saves class=saved")).unwrap_or_else(|| panic!("no result line: {stdout}"));
    eprintln!("{result}");
}

#[test]
fn a_save_of_a_fodg_of_one_150_mb_picture_ends_as_a_save_or_a_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("picture_150mb", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_save_of_a_fodg_of_one_150_mb_picture_past_the_gate_never_aborts_where_memory_runs_out() {
    let (ended_well, stdout) = run_child("picture_150mb_gate_open", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

#[test]
fn a_package_of_a_100_mb_picture_saved_as_a_fodg_ends_as_a_save_or_a_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("package_of_big_pictures", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// One conversion writes at most 200,000 pictures inline, counted over the parts together: 140,000 of `content.xml` and 60,001 of `styles.xml` are past it, and
/// 140,000 and 60,000 are not.
#[test]
fn the_pictures_of_a_conversion_are_capped_over_both_parts_at_200000() {
    let image = "<draw:image xlink:href=\"Pictures/a.png\"/>";
    let make = |content: usize, styles: usize| {
        let mut d = Drawing::a4().page(&frame(image).repeat(content));
        // The master styles hold a frame whose fill image names the picture, `styles` times: fill images of the common styles.
        d.common = format!(
            "<draw:fill-image draw:name=\"A\" draw:display-name=\"A\" xlink:href=\"Pictures/a.png\"/>{}",
            "<draw:fill-image draw:name=\"B\" xlink:href=\"Pictures/a.png\"/>".repeat(styles.saturating_sub(1))
        );
        if styles == 0 {
            d.common = String::new();
        }
        d.files.push(("Pictures/a.png".to_owned(), crate::write_core::PNG.to_vec()));
        d.bytes()
    };
    let result = |bytes: Vec<u8>| {
        let Ok((doc, report)) = read(&bytes, &Limits::default()) else { panic!("the package did not read") };
        write_counted(
            &doc,
            Some(&OriginalPackage { bytes: &bytes, index: &report.index }),
            &WriteOptions { form: Some(Form::Flat), ..WriteOptions::default() },
            &mut WriteWork::default(),
        )
        .map(|w| w.bytes.len())
        .map_err(|e| e.class())
    };
    // The named definitions of a family are at most 65,536, so the fill images are 60,000 and the frames 140,000.
    assert_eq!(result(make(140_000, 60_001)), Err("write_limit"));
    assert!(result(make(140_000, 60_000)).is_ok());
}
