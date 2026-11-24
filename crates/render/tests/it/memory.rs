//! What a hostile shadow blur costs, measured under an address-space limit.
//!
//! `vello_cpu` gives each blurred layer a pixmap as large as the shape plus three deviations on every side, and keeps every layer's
//! pixmap until the frame is finished. A blur cell of 20 inches on a 1-inch shape at 100 pixels to the inch added 514 MB, and the same at
//! 770 pixels to the inch asked for 15.5 GB, which is an abort (a failed allocation is not a panic; the review found 6.4 GB at 100
//! inches and 100 pixels to the inch). So the claims "a frame costs a bounded amount whatever the cell says" and "memory that runs out
//! does not abort" are measured the way `crates/opc/tests/it/memory.rs` measures its own: each
//! scenario runs in a child process under `ulimit -v`, and the parent reads what the child printed and how it ended. Linux only.
//!
//! The child is an ignored test (`child`) that the parent runs by name. It prints one `RESULT` line of `key=value` pairs: the time of the
//! frame, the peak (`VmHWM`) above what was resident before it, the address space (`VmPeak`), the shadows noted as approximate, and the
//! darkest pixel in the part of the shadow that the shape does not cover (255 means the shadow was not drawn).

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use diacraft_render::{Parallel, Renderer, SkipKind, View};
use kurbo::Affine;

use crate::support::*;

const SCENARIO: &str = "RENDER_MEMORY_SCENARIO";

/// How many threads the child draws on (a whole number; none is the renderer's own way, on one), and, if set, that the budgets that hold the
/// threads' memory to the frame's are lifted: a control, to see what the budgets save (run by hand).
const THREADS: &str = "RENDER_MEMORY_THREADS";
const UNBUDGETED: &str = "RENDER_MEMORY_UNBUDGETED";

/// The address space a child that draws on threads may have: 52 GB, so that the frame is drawn on the 31 threads it asks for (each thread has a stack of its own and a heap of its own address
/// space, and the threads of a frame are held to the ones whose 192 MiB fits in an eighth of what the limit leaves, `diacraft_container::spare::threads_fit`: under 3 GB that is two). What the
/// frame adds is read from the memory resident, which the limit does not move.
const PAR_CHILD_LIMIT_KIB: u64 = 52_000_000;

/// The address space a child may have: a failed allocation past it aborts the child, which fails the test.
const CHILD_LIMIT_KIB: u64 = 1_500_000;

/// What one frame may add to the memory resident before it, in kibibytes. Measured at worst 228 MiB (`huge:770:20:20`; the frame's own
/// pixels and their copy are 72 MiB of that, the blurred layers the rest), and stated here at twice that. Before the cap on a blur's
/// spread, `one:100:20` added 514 MB, `one:770:1.4` 240 MB, and `one:770:20` asked for 15.5 GB, which is an abort.
const EXTRA_KIB_BOUND: u64 = 480_000;

/// The longest a frame may take, in milliseconds: a hang guard and no measure of cost (what a frame costs here is its memory, bounded above).
/// Measured at worst 0.3 s, and stated at two hundred times that, since a loaded machine runs these tests side by side. Before the cap
/// `one:100:20` took 0.65 s.
const MILLIS_BOUND: u64 = 60_000;

fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_else(|e| panic!("these tests need /proc/self/status (Linux): {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or_else(|| panic!("no {key} line in /proc/self/status:\n{text}"))
}

/// A red square of `size` inches at `(x, y)` with a grey shadow offset by 0.3 of its size right and down, blurred by `blur` (a cell's text:
/// `1.#INF` is an infinity).
fn shadowed(id: u32, x: f64, y: f64, size: f64, blur: &str) -> diacraft_testkit::build::ShapeBuilder {
    rect(id, x, y, size, size)
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "0")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", &(size * 0.3).to_string())
        .cell("ShapeShdwOffsetY", &(-size * 0.3).to_string())
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", blur)
}

/// The darkest red channel in a rectangle of pixels of the frame (premultiplied, over white paper, so a grey shadow is below 255).
fn darkest(rendered: &diacraft_render::Rendered, x0: u32, y0: u32, x1: u32, y1: u32) -> u8 {
    let mut least = 255u8;
    for y in y0..y1.min(rendered.height) {
        for x in x0..x1.min(rendered.width) {
            let i = ((y * rendered.width + x) * 4) as usize;
            if let Some(p) = rendered.rgba_premul.get(i..i + 4) {
                // A pixel that is not the red shape: its green channel is as high as its red.
                if p[3] > 0 && p[1] >= p[0].saturating_sub(4) {
                    least = least.min(p[0]);
                }
            }
        }
    }
    least
}

/// The scenarios the child runs, one per call: `one:<ppi>:<blur>` is one shape on a 4 by 4 inch page, and
/// `many:<ppi>:<blur>:<count>` is `count` shapes of a tenth of an inch (a grid of 20 by 20) in a view of 2,000 pixels square.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    if scenario == "usable" {
        // How many threads a renderer asked for 31 has now, with the address space this process was given.
        println!("\nRESULT usable={}", Parallel::eager(31).usable_threads());
        return;
    }
    let parts: Vec<&str> = scenario.split(':').collect();
    let ppi: f64 = parts[1].parse().expect("a number of pixels to the inch");
    let blur = parts[2];
    let mut renderer = Renderer::new();
    if let Some(threads) = std::env::var(THREADS).ok().and_then(|v| v.parse::<usize>().ok()) {
        let par = Parallel::eager(threads);
        let par = if std::env::var(UNBUDGETED).is_ok() { Parallel { scratch_bytes: u64::MAX, blur_pixels: u64::MAX, ..par } } else { par };
        renderer.set_parallel(par);
    }
    // The part of the frame to look at for the shadow, in pixels: the shape's lower right, where only the shadow is.
    let (doc, view, look) = match parts[0] {
        "one" => {
            let doc = one_page(vec![shadowed(1, 1.5, 1.5, 1.0, blur)]);
            let view = View::fit_page(&doc, 1, ppi).expect("the page has a size");
            let at = |x: f64, y: f64| ((x * ppi) as u32, ((4.0 - y) * ppi) as u32);
            let (x0, y0) = at(2.0, 1.45);
            let (x1, y1) = at(2.7, 1.25);
            (doc, view, (x0, y0, x1, y1))
        }
        "cover" => {
            // `count` shapes of 3 by 3 inches (most of the page), each with a shadow 0.9 inch to the right and below: layers as large as the
            // view. The look is the band of shadow past the shapes' lower right.
            let count: u32 = parts[3].parse().expect("a count");
            let mut page = page(4.0, 4.0);
            for i in 0..count {
                page = page.shape(shadowed(i + 1, 0.2, 0.9, 3.0, blur));
            }
            let doc = base().page(1, page).build();
            let view = View::fit_page(&doc, 1, ppi).expect("the page has a size");
            let at = |x: f64, y: f64| ((x * ppi) as u32, ((4.0 - y) * ppi) as u32);
            let (x0, y0) = at(3.4, 2.5);
            let (x1, y1) = at(3.9, 0.5);
            (doc, view, (x0, y0, x1, y1))
        }
        "huge" => {
            // `count` shapes of 50 by 50 inches, whose right edge is 3.5 inches along a page of 4: far larger than the view. The look is the
            // strip of shadow between the shapes' edge and the page's.
            let count: u32 = parts[3].parse().expect("a count");
            let mut page = page(4.0, 4.0);
            for i in 0..count {
                page = page.shape(shadowed(i + 1, -46.5, -20.0, 50.0, blur));
            }
            let doc = base().page(1, page).build();
            let view = View::fit_page(&doc, 1, ppi).expect("the page has a size");
            let at = |x: f64, y: f64| ((x * ppi) as u32, ((4.0 - y) * ppi) as u32);
            let (x0, y0) = at(3.6, 3.0);
            let (x1, y1) = at(3.9, 1.0);
            (doc, view, (x0, y0, x1, y1))
        }
        "many" => {
            let count: u32 = parts[3].parse().expect("a count");
            let mut page = page(20.0, 20.0);
            for i in 0..count {
                let (col, row) = (i % 20, i / 20);
                page = page.shape(shadowed(i + 1, 0.05 + f64::from(col) * 0.13, 0.05 + f64::from(row) * 0.13, 0.1, blur));
            }
            let doc = base().page(1, page).build();
            // The paper's lower left corner at the view's lower left: 2,000 pixels cover 2,000 / ppi inches.
            let view = View { to_px: Affine::new([ppi, 0.0, 0.0, -ppi, 0.0, 2000.0]), width: 2000, height: 2000 };
            (doc, view, (0, 0, 2000, 2000))
        }
        other => panic!("no scenario {other:?}"),
    };
    println!("\nSTART VmSize={}KiB", status_kib("VmSize"));
    // Forget the peak so far, which holds the cost of building the document, and note what is resident.
    let reset = std::fs::write("/proc/self/clear_refs", "5").is_ok();
    let base_rss = status_kib("VmRSS");
    let started = Instant::now();
    let rendered = renderer.render(&doc, 1, &view);
    let millis = started.elapsed().as_millis();
    let hwm = status_kib("VmHWM");
    let extra = if reset { hwm.saturating_sub(base_rss) } else { hwm };
    let (x0, y0, x1, y1) = look;
    println!(
        "\nRESULT ms={millis} extra_kib={extra} hwm_kib={hwm} vmpeak_kib={} approx={} shadow_least={} pixels={} reset={reset} skipped={:?}",
        status_kib("VmPeak"),
        rendered.count(SkipKind::ShadowApproximate),
        darkest(&rendered, x0, y0, x1, y1),
        rendered.rgba_premul.len() / 4,
        rendered.skipped.iter().map(|s| (s.kind, s.count)).collect::<Vec<_>>(),
    );
}

/// What a child ended with: whether it ended normally, and the pairs of its `RESULT` line.
struct Outcome {
    ok: bool,
    fields: HashMap<String, String>,
    stdout: String,
}

impl Outcome {
    fn number(&self, key: &str) -> u64 {
        self.fields.get(key).and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("no number {key} in the child's output:\n{}", self.stdout))
    }
}

/// Runs `scenario` in a child under an address-space limit, drawing on `threads` threads (0 is the renderer's own way, on one).
fn run_child(scenario: &str, threads: usize) -> Outcome {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let limit = if threads > 0 { PAR_CHILD_LIMIT_KIB } else { CHILD_LIMIT_KIB };
    let script = format!("ulimit -v {limit} && exec \"$0\" --exact memory::child --ignored --nocapture --test-threads=1");
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).arg(exe).env(SCENARIO, scenario);
    if threads > 0 {
        command.env(THREADS, threads.to_string());
    }
    let out = command.output().expect("run /bin/sh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        eprintln!("child `{scenario}` on {threads} threads under {limit} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    let line = stdout.lines().find_map(|l| l.strip_prefix("RESULT ")).unwrap_or("");
    // `skipped=[...]` holds spaces, so the pairs are cut at the key that starts each: only the numbers are read.
    let fields = line.split(' ').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    eprintln!("MEASURED {scenario} on {threads} threads under {limit} KiB: {line}");
    Outcome { ok: out.status.success(), fields, stdout }
}

/// What every scenario must show: it ended normally within the bounds, and the shadow is drawn.
fn assert_bounded(scenario: &str) {
    assert_bounded_on(scenario, 0);
}

/// As [`assert_bounded`], the frame drawn on `threads` threads: **the same bounds**, since the threads hold no more between them than one does.
fn assert_bounded_on(scenario: &str, threads: usize) {
    let o = run_child(scenario, threads);
    assert!(o.ok, "{scenario} on {threads} threads: the child must end normally, not abort:\n{}", o.stdout);
    assert!(
        o.number("extra_kib") <= EXTRA_KIB_BOUND,
        "{scenario} on {threads} threads: a frame added {} KiB, past {EXTRA_KIB_BOUND}",
        o.number("extra_kib")
    );
    assert!(o.number("ms") <= MILLIS_BOUND, "{scenario}: a frame took {} ms, past {MILLIS_BOUND}", o.number("ms"));
    assert!(o.number("shadow_least") < 250, "{scenario}: the shadow is not drawn (darkest {})", o.number("shadow_least"));
}

#[test]
fn a_huge_blur_at_100_pixels_to_the_inch_is_bounded_and_the_shadow_still_draws() {
    // Blur cells in inches: 20 asked for 521 MB, 50 for 3.2 GB, 100 for 6.4 GB, 150 for 14 GB (an abort), and 300 for a panic inside
    // the rasteriser. `1.#INF` is the text of an infinity.
    for blur in ["20", "50", "100", "150", "300", "1.#INF"] {
        assert_bounded(&format!("one:100:{blur}"));
    }
}

#[test]
fn a_huge_blur_at_770_pixels_to_the_inch_is_bounded_and_the_shadow_still_draws() {
    // 770 pixels to the inch is a 2x screen at 400%: the case in which a blur of 13 inches asked for 6.4 GB and one of 1.4 inches
    // (the largest a real drawing holds is a quarter of that) cost 150 MB for each shadow in view.
    for blur in ["1.4", "20", "50", "100", "150", "300", "1.#INF"] {
        assert_bounded(&format!("one:770:{blur}"));
    }
}

#[test]
fn blurred_shadows_as_large_as_the_view_are_bounded_together() {
    // 30 shapes of 3 by 3 inches on a 4 by 4 inch page. At 770 pixels to the inch each layer alone is 11 million pixels or more, and 30
    // of them would be a third of a gigabyte for the layers and as much again for the rasteriser's scratch.
    for (ppi, blur) in [("100", "0.1"), ("770", "0.1"), ("770", "20")] {
        assert_bounded(&format!("cover:{ppi}:{blur}:30"));
    }
}

#[test]
fn blurred_shadows_far_larger_than_the_view_are_bounded_together() {
    // 20 shapes of 50 inches: at 770 pixels to the inch 38,500 pixels across. The layer is cut to the view before it is counted, and the
    // rasteriser's own layer is cut to it as well.
    for (ppi, blur) in [("100", "0.1"), ("770", "0.1"), ("770", "20")] {
        assert_bounded(&format!("huge:{ppi}:{blur}:20"));
    }
}

#[test]
fn many_blurred_shadows_on_one_page_are_bounded_together() {
    // 400 shapes (the most blurs a frame makes) in a view of 2,000 pixels square. Each layer alone is within the cap on its spread, but
    // the layers are kept together until the frame is done, so it is their sum that is bounded.
    for (ppi, blur) in [("100", "1"), ("100", "20"), ("770", "0.5"), ("770", "20")] {
        assert_bounded(&format!("many:{ppi}:{blur}:400"));
    }
}

#[test]
fn a_frame_drawn_on_31_threads_adds_no_more_than_one_drawn_on_one_and_does_not_abort() {
    // The scenarios above whose frames are the biggest or whose layers are the most, drawn on 31 threads under 3 GB of address space. Each
    // thread's context holds the rows above its band, and each band holds the layers of the blurs it meets, so without the budgets that hold the
    // threads together to what a frame is allowed (`Parallel::scratch_bytes`, `Parallel::blur_pixels`) the frame is up to 31 times these.
    for scenario in ["cover:770:0.1:30", "cover:770:20:30", "huge:770:0.1:20", "huge:770:20:20", "many:770:0.5:400", "many:100:20:400", "one:770:20"]
    {
        assert_bounded_on(scenario, 31);
    }
}

/// How many threads a child asked for 31 may use under `ulimit -v limit_kib`.
fn usable_under(limit_kib: u64) -> u64 {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, "usable").output().expect("run /bin/sh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the child ended with {:?}:\n{stdout}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    let line = stdout.lines().find_map(|l| l.strip_prefix("RESULT usable=")).expect("a RESULT line");
    line.trim().parse().expect("a number")
}

#[test]
fn a_process_with_little_address_space_left_draws_on_fewer_threads_and_one_with_plenty_on_all_of_them() {
    // Each thread takes address space for good (its stack, its allocator's arena and the heaps the arena keeps), and a thread that cannot have it
    // is an abort: engine's `odg_memory` children, which have a drawing of 2.5 GB and 3 GB of address space, aborted at the thumbnail of a save on
    // 8 threads, and the save of 50,000 new shapes after a thumbnail drawn on 12 or more. The threads are held by the rule of a save and an open
    // (`diacraft_container::spare::threads_fit`: 192 MiB a thread in an eighth of what the limit leaves beside what is in use and `Parallel::room`).
    assert_eq!(usable_under(60_000_000), 31, "60 GB of address space: every thread asked for");
    assert_eq!(usable_under(PAR_CHILD_LIMIT_KIB), 31, "the children that draw on threads draw on all 31 they ask for");
    let few = usable_under(700_000);
    assert!((1..=8).contains(&few), "700 MB, of which the test binary has taken some: {few} threads");
    assert_eq!(usable_under(150_000), 1, "150 MB is all the process holds: the calling thread draws");
}
