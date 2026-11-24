//! B1 and B2 of the F1 re-review: **a save into a VSDX file that holds large embedded objects saves where `main` saves.** The model holds every
//! part it does not rebuild, and the save read the file it spliced into and the file it made each into a package of their own: every held part was in memory three or four times over (the
//! model, the original's package, the package of the check), where `main`, whose model held none, had it twice. Three parts of 240 MiB were refused ("does not read back ... out of memory")
//! and two parts of 256 MiB beside 20,000 shapes aborted, where `main` saves both, at 2.27 and 2.53 GB. Both packages now share the model's bytes where an entry holds them
//! (`Package::read_knowing`: the size and the checksum, the data read and kept nowhere).
//!
//! Each case runs in a child under `ulimit -v 3000000`, on a file made before the child starts: the drawing is opened, a shape moved, saved as a new file and saved again after a second move.
use std::process::Command;

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder};

use crate::odg_build::Dir;

const SCENARIO: &str = "ENGINE_VSDX_EMBEDDED_PEAK";
const CASES: &str = "ENGINE_VSDX_EMBEDDED_PEAK_CASES";
const CT_OLE: &str = "application/vnd.openxmlformats-officedocument.oleObject";
const MIB: usize = 1 << 20;

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// `n` bytes no compression makes smaller.
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

/// A shape of a diagram: a box with its geometry, a few cells with formulas, and a text.
fn diagram_shape(i: usize) -> String {
    let (x, y) = (format!("{}.{}", i % 997, i % 89), format!("{}.{}", i / 997, i % 83));
    let mut rows = String::new();
    for (ix, (t, px, py)) in
        [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")]
            .iter()
            .enumerate()
    {
        rows.push_str(&format!("<Row T=\"{t}\" IX=\"{}\"><Cell N=\"X\" V=\"1\" F=\"{px}\"/><Cell N=\"Y\" V=\"1\" F=\"{py}\"/></Row>", ix + 1));
    }
    format!(
        "<Shape ID=\"{}\" Type=\"Shape\" Name=\"Box\" NameU=\"Box\"><Cell N=\"PinX\" V=\"{x}\"/><Cell N=\"PinY\" V=\"{y}\"/><Cell N=\"Width\" V=\"1\"/><Cell N=\"Height\" V=\"0.75\"/>\
         <Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><Cell N=\"Angle\" V=\"0\"/><Cell N=\"FlipX\" V=\"0\"/><Cell N=\"FlipY\" V=\"0\"/>\
         <Cell N=\"ResizeMode\" V=\"0\"/><Cell N=\"LineWeight\" V=\"0.01\"/><Cell N=\"LineColor\" V=\"0\"/><Cell N=\"FillForegnd\" V=\"1\"/><Cell N=\"FillPattern\" V=\"1\"/>\
         <Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>{rows}</Section><Text><cp IX=\"0\"/>Label</Text></Shape>",
        i + 1
    )
}

/// What a case's file holds: `parts` embedded objects of `mib` MiB each (zeros, or bytes no compression makes smaller), stored, beside `shapes` shapes on one page.
#[derive(Clone, Copy, Debug)]
struct Case {
    parts: usize,
    mib: usize,
    shapes: usize,
    noise: bool,
}

impl Case {
    /// `parts:mib:shapes`, with an `n` before it for parts of noise.
    fn parse(text: &str) -> Case {
        let (noise, text) = text.strip_prefix('n').map_or((false, text), |rest| (true, rest));
        let n: Vec<usize> = text.split(':').map(|x| x.parse().unwrap()).collect();
        Case { parts: n[0], mib: n[1], shapes: n.get(2).copied().unwrap_or(1), noise }
    }

    fn write(self, dir: &Dir) -> (String, usize) {
        let zeros = if self.noise { Vec::new() } else { vec![0u8; self.mib * MIB] };
        let mut pkg = Pkg::new().stored();
        for k in 1..=self.parts {
            let name = format!("/visio/embeddings/oleObject{k}.bin");
            pkg = if self.noise { pkg.part(&name, CT_OLE, noise(self.mib * MIB, k as u64)) } else { pkg.part(&name, CT_OLE, &zeros) };
        }
        drop(zeros);
        let body: String = (0..self.shapes).map(diagram_shape).collect();
        let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&body)).with(pkg).pkg().stored().build();
        let len = bytes.len();
        (dir.write("in.vsdx", &bytes), len)
    }
}

fn first_shape(s: &Session) -> Vec<u32> {
    s.document().and_then(|d| d.pages.get(0)).and_then(|p| p.shapes.iter().next().map(|x| vec![x.id])).unwrap_or_default()
}

fn said<T>(result: &Result<T, diacraft_engine::EngineError>) -> String {
    match result {
        Ok(_) => "ok".to_owned(),
        Err(e) => format!("refused: {e}"),
    }
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a file in the environment"]
fn embedded_peak_child() {
    let path = std::env::var(SCENARIO).expect("the path of the file");
    let dir = Dir::new();
    let mut s = Session::new();
    let open = s.execute("file.open", &serde_json::json!({ "path": path }));
    println!("OPEN {}: VmPeak {} MiB", said(&open), status_kib("VmPeak") >> 10);
    if open.is_err() {
        println!("embedded peak: no abort");
        return;
    }
    let ids = first_shape(&s);
    println!("EDIT {}", said(&s.execute("shape.move", &serde_json::json!({ "ids": ids, "dx": 0.5, "dy": 0.0 }))));
    let save = s.execute("file.save_as", &serde_json::json!({ "path": dir.path("out.vsdx") }));
    println!("SAVE {}: VmPeak {} MiB", said(&save), status_kib("VmPeak") >> 10);
    if save.is_ok() {
        let ids = first_shape(&s);
        println!("EDIT {}", said(&s.execute("shape.move", &serde_json::json!({ "ids": ids, "dx": 0.5, "dy": 0.0 }))));
        let again = s.execute("file.save", &serde_json::json!({}));
        println!("SAVE AGAIN {}: VmPeak {} MiB", said(&again), status_kib("VmPeak") >> 10);
    }
    println!("embedded peak: no abort");
}

/// The child's lines, and whether it ended without an abort.
fn child(path: &str) -> (String, bool) {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact vsdx_embedded_peak::embedded_peak_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, path).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| {
            ["OPEN", "EDIT", "SAVE"].iter().any(|w| l.starts_with(w))
                || l.contains(" OPEN ")
                || l.contains("no abort")
                || l.contains("memory allocation")
                || l.contains("panicked")
        })
        .collect();
    (format!("status {:?}\n{}", out.status, lines.join("\n")), out.status.success() && text.contains("embedded peak: no abort"))
}

/// The case is opened, edited and saved twice, in a child under the address limit.
#[track_caller]
#[cfg(target_os = "linux")]
fn assert_saved_twice(case: Case) {
    let dir = Dir::new();
    let (path, _) = case.write(&dir);
    let (text, ended) = child(&path);
    assert!(ended, "{case:?} aborted: {text}");
    assert!(text.contains("OPEN ok"), "{case:?}: {text}");
    assert!(text.contains("SAVE ok"), "{case:?}: the save is refused where main saves it: {text}");
    assert!(text.contains("SAVE AGAIN ok"), "{case:?}: the second save is refused: {text}");
    eprintln!("{case:?}: {text}");
}

/// B1: three embedded objects of 240 MiB each, stored (`main` saves them at a peak of 2.27 GB; the branch refused them in words, "does not read back ... out of memory").
#[test]
#[cfg(target_os = "linux")]
fn three_embedded_objects_of_240_mib_are_saved_twice_under_the_address_limit() {
    assert_saved_twice(Case { parts: 3, mib: 240, shapes: 1, noise: false });
}

/// B2: two embedded objects of 256 MiB each beside 20,000 shapes (`main` saves them at a peak of 2.53 GB; the branch aborted).
#[test]
#[cfg(target_os = "linux")]
fn two_embedded_objects_of_256_mib_beside_20000_shapes_are_saved_twice_under_the_address_limit_and_do_not_abort() {
    assert_saved_twice(Case { parts: 2, mib: 256, shapes: 20_000, noise: false });
}

/// The measurement by hand: `ENGINE_VSDX_EMBEDDED_PEAK_CASES=3:256,3:240,4:192,2:256:20000,n3:200` runs each case in a child and prints what it did and its peaks. It asserts nothing.
#[test]
#[ignore = "a measurement, run by hand with the cases in the environment"]
fn embedded_peaks_by_hand() {
    let cases = std::env::var(CASES).unwrap_or_default();
    for case in cases.split(',').filter(|c| !c.is_empty()).map(Case::parse) {
        let dir = Dir::new();
        let (path, len) = case.write(&dir);
        let (text, ended) = child(&path);
        eprintln!("PEAK {case:?} file {len} bytes, ended {ended}: {text}");
    }
}
