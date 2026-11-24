//! The re-review's item 5: **the peak of the save of a `.vsdx` that the reader made.** The writer makes the file and then **reads it back** to check it, which holds
//! a second copy of the drawing beside the first: at 70,000 shapes over three pages that is 2.7 GiB, and at 76,000 it is more than the 3,000,000 KiB a process has. The save refuses a drawing
//! whose peak would pass it, in words, before it makes anything; a drawing of what fits is saved. Each case runs in a child under `ulimit -v 3000000`.
//!
//! **Priced a node of a map of cells at the law of the memory tests** (the bytes asked for, an eighth of them for mimalloc's size classes and 24: 1,344 bytes
//! where glibc's chunk was 1,168), and a shape of these holds nine such nodes, so the save's estimate of the drawing and its copy is about a tenth higher: the most it saves is 69,000
//! shapes over three pages (2,650 MiB of address space at the peak), and 70,000 are refused (an estimate of 2,891 MiB against the 2,883 a save may hold), where 76,000 were saved before.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::time::Instant;

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};

use crate::odg_build::Dir;

const SCENARIO: &str = "ENGINE_VSDX_SAVE_PEAK_SHAPES";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// A shape of the kind a diagram has: fourteen cells, two formulas, a path of five rows and a text. **Its path is its own** (a value that depends on `i`): a reader that shares what is
/// equal shares nothing of these, which is the worst a file can make and the model these tests were measured on.
fn shape(i: usize) -> String {
    let (x, y) = (format!("{}.{}", i % 997, i % 89), format!("{}.{}", i / 997, i % 83));
    let cells = format!(
        "<Cell N=\"PinX\" V=\"{x}\"/><Cell N=\"PinY\" V=\"{y}\"/><Cell N=\"Width\" V=\"1\"/><Cell N=\"Height\" V=\"0.75\"/><Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><Cell N=\"Angle\" V=\"0\"/><Cell N=\"FlipX\" V=\"0\"/><Cell N=\"FlipY\" V=\"0\"/><Cell N=\"ResizeMode\" V=\"0\"/><Cell N=\"LineWeight\" V=\"0.01\"/><Cell N=\"LineColor\" V=\"0\"/><Cell N=\"FillForegnd\" V=\"1\"/><Cell N=\"FillPattern\" V=\"1\"/>"
    );
    let points = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(ix, (t, px, py))| {
            format!("<Row T=\"{t}\" IX=\"{}\"><Cell N=\"X\" V=\"1\" F=\"{px}\"/><Cell N=\"Y\" V=\"{i}\" F=\"{py}\"/></Row>", ix + 1)
        })
        .collect();
    format!(
        "<Shape ID=\"{}\" Type=\"Shape\" Name=\"Process\" NameU=\"Process\">{cells}<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section><Text><cp IX=\"0\"/>Label</Text></Shape>",
        i + 1
    )
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the number of shapes in the environment"]
fn vsdx_save_peak_child() {
    let n: usize = std::env::var(SCENARIO).ok().and_then(|v| v.parse().ok()).expect("the number of shapes");
    let (pages, per) = (3usize, n.div_ceil(3));
    let mut builder = VsdxBuilder::new();
    for p in 0..pages {
        let body: String = (p * per..((p + 1) * per).min(n)).map(shape).collect();
        builder = builder.page(PageSpec::new(u32::try_from(p + 1).unwrap(), &format!("Page {}", p + 1)).shapes(&body));
    }
    let bytes = builder.build();
    drop(builder);
    let dir = Dir::new();
    let path = dir.write("a.vsdx", &bytes);
    drop(bytes);
    let mut s = Session::new();
    s.execute("file.open", &serde_json::json!({ "path": path })).expect("the reader admits the package");
    let held = s.model_held().unwrap_or(0) >> 20;
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().take(1).map(|x| x.id).collect();
    s.execute("shape.move", &serde_json::json!({ "ids": ids, "dx": 0.5, "dy": 0.0 })).expect("an edit");
    let started = Instant::now();
    let out = s.execute("file.save_as", &serde_json::json!({ "path": dir.path("out.vsdx") }));
    let words = out.as_ref().map(|_| "saved".to_owned()).map_err(ToString::to_string);
    println!("PEAK {n} shapes over {pages} pages: held {held} MiB, {words:?} in {:?}, VmPeak {} MiB", started.elapsed(), status_kib("VmPeak") >> 10);
    match n {
        // What fits is saved.
        0..=69_000 => assert!(out.is_ok(), "a drawing of {n} shapes fits: {words:?}"),
        // What does not is refused in words and is not made.
        _ => {
            let message = out.expect_err("a drawing whose save reads the file back past the address space").to_string();
            assert!(message.contains("could not be saved") && message.contains("second copy"), "{message}");
        }
    }
    println!("peak: no abort");
}

#[cfg(target_os = "linux")]
fn run(shapes: usize) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact vsdx_save_peak::vsdx_save_peak_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, shapes.to_string()).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("peak: no abort"), "{shapes}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("PEAK")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn a_vsdx_of_sixty_nine_thousand_shapes_is_saved_as_a_vsdx_under_the_address_limit() {
    run(69_000);
}

#[test]
#[cfg(target_os = "linux")]
fn a_vsdx_of_eighty_thousand_shapes_is_refused_in_words_at_the_save_and_does_not_run_out_of_address_space() {
    run(80_000);
}

const BESIDE: &str = "ENGINE_VSDX_BESIDE";

/// The `.vsdx` of `n` shapes over three pages, written to `dir` as `name`.
fn vsdx_file(dir: &Dir, name: &str, n: usize) -> String {
    let (pages, per) = (3usize, n.div_ceil(3));
    let mut builder = VsdxBuilder::new();
    for p in 0..pages {
        let body: String = (p * per..((p + 1) * per).min(n)).map(shape).collect();
        builder = builder.page(PageSpec::new(u32::try_from(p + 1).unwrap(), &format!("Page {}", p + 1)).shapes(&body));
    }
    let bytes = builder.build();
    drop(builder);
    dir.write(name, &bytes)
}

/// **A drawing that was saved does not take the room of a read beside it**: a session saves a drawing of `a` shapes (the model the save kept is held beside the drawing, in
/// the session's figure for an edit), and another session in the process opens a file of `c` shapes, which is refused where the reader refuses it and not because the first was saved.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the numbers of shapes in the environment"]
fn vsdx_beside_child() {
    let spec = std::env::var(BESIDE).expect("the shapes of the drawing and of the file read beside it");
    let (a, c) = spec.split_once(':').map(|(a, c)| (a.parse::<usize>().unwrap(), c.parse::<usize>().unwrap())).expect("a:c");
    let dir = Dir::new();
    let first = vsdx_file(&dir, "a.vsdx", a);
    let second = vsdx_file(&dir, "c.vsdx", c);
    let mut saved = Session::new();
    saved.execute("file.open", &serde_json::json!({ "path": first })).expect("the reader admits the first drawing");
    let ids: Vec<u32> = saved.document().unwrap().pages.get(0).unwrap().shapes.iter().take(1).map(|x| x.id).collect();
    saved.execute("shape.move", &serde_json::json!({ "ids": ids, "dx": 0.5, "dy": 0.0 })).expect("an edit");
    saved.execute("file.save_as", &serde_json::json!({ "path": dir.path("out.vsdx") })).expect("the first drawing is saved");
    let (held, process) = (saved.model_held().unwrap_or(0) >> 20, diacraft_engine::memory::held() >> 20);
    let mut other = Session::new();
    let read = other.execute("file.open", &serde_json::json!({ "path": second }));
    println!(
        "BESIDE {a} saved ({held} MiB held, the process holds {process} MiB), then {c} opened beside it: {:?}, process {} MiB, VmPeak {} MiB",
        read.as_ref().map(|_| "opened").map_err(ToString::to_string),
        diacraft_engine::memory::held() >> 20,
        status_kib("VmPeak") >> 10
    );
    read.expect("the file opens beside the saved drawing, as it did before the count");
    println!("beside: no abort");
}

#[cfg(target_os = "linux")]
fn run_beside(a: usize, c: usize) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact vsdx_save_peak::vsdx_beside_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(BESIDE, format!("{a}:{c}")).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("beside: no abort"), "{a}:{c}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("BESIDE")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn a_vsdx_of_forty_thousand_shapes_opens_beside_a_session_that_saved_a_drawing_of_forty_thousand() {
    run_beside(40_000, 40_000);
}

#[test]
#[cfg(target_os = "linux")]
fn a_vsdx_of_twenty_thousand_shapes_opens_beside_a_session_that_saved_a_drawing_of_sixty_eight_thousand() {
    // 70,000 before a change, whose save the estimate now refuses (see the module's documentation).
    run_beside(68_000, 20_000);
}
