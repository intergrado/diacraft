//! The addendum from the review: **a save keeps no second copy of the parts the model holds.** The writer checks the file it made by reading it back, and
//! that read moved every held part (an embedded object, a VBA project, a thumbnail) into the document it handed back, which the engine keeps as the baseline of the next save: two copies of
//! every held part beside each other, one of them in no figure the session counts. Two parts of 256 MiB were opened, edited and saved at 2,149 MiB on `main`, and refused on the branch; one
//! part of 256 MiB left 780 MiB held after a save, against 268 on `main`.
//!
//! The check of the file now reads it without moving the parts, and the baseline holds the bytes of the model's own (`Arc`s of the same bytes). Each case runs in a child under
//! `ulimit -v 3000000`, on a file made before the child starts.
#[cfg(target_os = "linux")]
use std::process::Command;

use diacraft_engine::Session;
#[cfg(target_os = "linux")]
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, cell, shape};

use crate::odg_build::Dir;

const SCENARIO: &str = "ENGINE_VSDX_HELD_PARTS";
#[cfg(target_os = "linux")]
const CT_OLE: &str = "application/vnd.openxmlformats-officedocument.oleObject";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// `n` bytes no compression makes smaller.
#[cfg(target_os = "linux")]
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(n);
    out
}

/// A drawing of one page with `count` embedded objects of `each` bytes, stored (so that a file of the size is made at once), written to `dir`.
#[cfg(target_os = "linux")]
fn file_with_parts(dir: &Dir, count: usize, each: usize) -> String {
    let mut pkg = Pkg::new().stored();
    for i in 0..count {
        pkg = pkg.part(&format!("/visio/embeddings/oleObject{i}.bin"), CT_OLE, noise(each, i as u64 + 1));
    }
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).with(pkg).build();
    dir.write("held.vsdx", &bytes)
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a file and the number of saves in the environment"]
fn held_parts_child() {
    let spec = std::env::var(SCENARIO).expect("the path and the number of saves");
    let (path, saves) = spec.rsplit_once(':').map(|(p, n)| (p.to_owned(), n.parse::<usize>().unwrap())).expect("path:saves");
    let dir = Dir::new();
    let mut s = Session::new();
    s.execute("file.open", &serde_json::json!({ "path": path })).expect("the reader admits the package");
    let ids: Vec<u32> = s.document().unwrap().pages.get(0).unwrap().shapes.iter().take(1).map(|x| x.id).collect();
    s.execute("shape.move", &serde_json::json!({ "ids": ids, "dx": 0.5, "dy": 0.0 })).expect("an edit");
    for i in 0..saves {
        let out = s.execute("file.save_as", &serde_json::json!({ "path": dir.path(&format!("out{i}.vsdx")) }));
        println!(
            "SAVE {i}: {:?}, VmPeak {} MiB, VmRSS {} MiB",
            out.as_ref().map(|_| "saved").map_err(ToString::to_string),
            status_kib("VmPeak") >> 10,
            status_kib("VmRSS") >> 10
        );
        out.expect("a drawing of held parts is saved");
    }
    println!("RSS {}", status_kib("VmRSS") >> 10);
    println!("held parts: no abort");
}

#[cfg(target_os = "linux")]
fn run(path: &str, saves: usize) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact vsdx_held_parts::held_parts_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, format!("{path}:{saves}")).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("held parts: no abort"), "{path}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.starts_with("SAVE")).collect::<Vec<_>>().join("\n"));
    text
}

#[cfg(target_os = "linux")]
fn resident_mib(text: &str) -> u64 {
    text.lines().find_map(|l| l.strip_prefix("RSS ")).and_then(|v| v.trim().parse().ok()).unwrap_or(u64::MAX)
}

#[test]
#[cfg(target_os = "linux")]
fn two_held_parts_of_256_mib_are_opened_edited_and_saved_twice_under_the_address_limit() {
    let dir = Dir::new();
    let path = file_with_parts(&dir, 2, 256 << 20);
    run(&path, 2);
}

#[test]
#[cfg(target_os = "linux")]
fn a_saved_drawing_holds_its_parts_once() {
    // One part of 256 MiB: the session holds the file it opened (256 MiB, for a save into it) and the model holds the part (256 MiB); the baseline of the save is the model's. A second copy
    // of the part, which the check of the file used to leave in the baseline, would make it 780 MiB.
    let dir = Dir::new();
    let path = file_with_parts(&dir, 1, 256 << 20);
    let text = run(&path, 1);
    let held = resident_mib(&text);
    assert!(held < 640, "{held} MiB are resident after the save of a drawing with one part of 256 MiB: {text}");
}
