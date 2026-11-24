//! Test-only: what a turned subject costs (row 3; spec, design "Cost bounds and hostile inputs"). Counted, never timed: the work the library reports for a
//! query (`Work`: bucket entries visited, gathers, steps of bisection) on pages of 5,000 turned shapes, beside the same page with every shape square to the page, and on the
//! crafted pages that were the cost of an earlier snap: many own lines at one place, directions a hair apart, nearly parallel directions.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craft_snap as cs;
use diacraft_doc::Document;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use super::adapter::LAST;
use super::{Extras, OwnFrame, Query, Rect, SnapPage, SnapSettings, Snapped, Subject, Use, snap_ext};

/// A shape 1 by 0.5 inch with its pin in its middle at `pin`, turned `radians`.
fn shape(id: u32, pin: (f64, f64), radians: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "1")
        .cell("Height", "0.5")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.25")
        .cell("Angle", &radians.to_string())
}

/// The page of `n` shapes laid on a grid 1.5 inches apart, shape `i` turned `angle(i)` radians.
fn page(n: u32, angle: impl Fn(u32) -> f64) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "1000").cell("PageHeight", "1000");
    for i in 0..n {
        page = page.shape(shape(i + 1, (f64::from(i % 100) * 1.5 + 2.0, f64::from(i / 100) * 1.5 + 2.0), angle(i)));
    }
    DocBuilder::new().page(1, page).build()
}

/// The box round a frame.
fn round(f: &OwnFrame) -> Rect {
    let (s, c) = f.angle.sin_cos();
    let (hw, hh) = (f.width / 2.0, f.height / 2.0);
    let (ex, ey) = (c.abs() * hw + s.abs() * hh, s.abs() * hw + c.abs() * hh);
    Rect { x0: f.centre.0 - ex, y0: f.centre.1 - ey, x1: f.centre.0 + ex, y1: f.centre.1 + ey }
}

/// What a turned subject (a shape 1 by 0.5 turned `radians`, its middle at `at`) is asked on `doc` with only the alignment on: the answer, and the library's work.
fn ask(doc: &Document, at: (f64, f64), radians: f64) -> (Snapped, cs::Work) {
    ask_on(doc, &SnapPage::read(doc, 1).unwrap(), at, radians)
}

/// [`ask`] on a page already read: the second query of a drag, which finds the page's targets built.
fn ask_on(doc: &Document, page: &SnapPage, at: (f64, f64), radians: f64) -> (Snapped, cs::Work) {
    let mut s = SnapSettings::none();
    s.snap = true;
    s.alignment_box = true;
    let frame = OwnFrame { centre: at, width: 1.0, height: 0.5, angle: radians };
    let query = Query { subject: Subject::Box(round(&frame)), using: Use::Place, moving: &[], zoom: 96.0, own_points: &[] };
    let out = snap_ext(doc, page, &s, &query, Extras { frame: Some(frame), ..Extras::default() });
    (out, LAST.with(|l| l.borrow().clone()).unwrap().work)
}

const THIRTY: f64 = std::f64::consts::PI / 6.0;

/// A line mark as (how many shapes it counts, where it starts, where it ends).
type Run = (u32, (f64, f64), (f64, f64));

#[test]
fn a_turned_subject_among_five_thousand_turned_shapes_gathers_nothing_and_costs_a_logarithm_beside_the_same_page_square() {
    // 5,000 shapes turned 30 degrees, 30, 30 and 75 in turn: a turned subject aligns by its own sides against their own lines in two directions beside the page's two: bisection in
    // the sorted lines of each direction, no gather from the index, whatever the page holds. The yardstick is the same page with every shape square to the page and a subject
    // square to it (two directions, a third of the lines).
    let doc = page(5_000, |i| if i % 4 == 3 { 75.0_f64.to_radians() } else { THIRTY });
    // Over the middle of the 31st shape of the 3rd row (a shape turned 30 degrees), 0.02 inch off its own lines along its y axis.
    let (x, y) = (f64::from(30) * 1.5 + 2.0, f64::from(2) * 1.5 + 2.0);
    let at = (x + 0.02 * -THIRTY.sin(), y + 0.02 * THIRTY.cos());
    let (out, work) = ask(&doc, at, THIRTY);
    assert!(out.hits.iter().any(|h| h.normal.is_some()), "{out:?}");
    assert_eq!((work.gathers, work.visited, work.examined), (0, 0, 0), "alignment gathers nothing: {work:?}");
    let square = page(5_000, |_| 0.0);
    let (_, base) = ask(&square, (x, y + 0.02), 0.0);
    eprintln!("turned subject, 5,000 shapes turned: bisection steps {}; the same page square: {}", work.bisect_steps, base.bisect_steps);
    // Measured: 346 steps with four directions, 321 with two: the steps are the sides' lookups in each direction's sorted lines, and the marks' (every line the subject lies on
    // is found again by bisection), at most a log2 of the lines of a direction for each.
    assert!((300..=400).contains(&work.bisect_steps), "{}", work.bisect_steps);
    assert!(
        work.bisect_steps * 4 <= 5 * base.bisect_steps,
        "own directions cost a quarter more at most: {} against {}",
        work.bisect_steps,
        base.bisect_steps
    );
}

#[test]
fn five_thousand_shapes_each_a_billionth_of_a_radian_beyond_the_last_are_five_thousand_directions_found_by_bisection() {
    // Directions are grouped by the library's tolerance (a billionth of a radian from the first of a group). Shapes turned 30 degrees and i times 2e-9 radian are 5,000 directions of
    // their own: a subject turned like the 2,500th finds its own among 10,000 by bisection (14 steps), and meets only the lines of the shapes turned within a billionth of it: its own.
    let doc = page(5_000, |i| THIRTY + f64::from(i) * 2.0e-9);
    let alike = 2_500u32;
    let (x, y) = (f64::from(alike % 100) * 1.5 + 2.0, f64::from(alike / 100) * 1.5 + 2.0);
    let angle = THIRTY + f64::from(alike) * 2.0e-9;
    let (out, work) = ask(&doc, (x + 0.02 * -angle.sin(), y + 0.02 * angle.cos()), angle);
    assert_eq!((work.gathers, work.visited), (0, 0), "{work:?}");
    let own: Vec<_> = out.hits.iter().filter(|h| h.normal.is_some()).collect();
    assert!(
        !own.is_empty() && own.iter().all(|h| h.shape == Some(alike + 1)),
        "only the shape turned within a billionth of the subject's angle is met: {out:?}"
    );
    eprintln!("5,000 directions a hair apart: bisection steps {}", work.bisect_steps);
    // Measured: 522, against 346 for six directions: the steps of finding the subject's own among 10,000 are 14 each.
    assert!((450..=600).contains(&work.bisect_steps), "{}", work.bisect_steps);
}

#[test]
fn five_thousand_shapes_whose_own_lines_are_the_same_lines_are_three_entries_of_five_thousand_and_cost_a_logarithm() {
    // 5,000 shapes turned 30 degrees whose pins are on one line along their own x axis: their own top, middle and bottom lines are three lines, each the line of 5,000 shapes (many
    // ends at one place). The mark of a line runs to the nearest shape on each side by five bisections over its members, and says how many shapes are on it.
    let (a, d) = ((2.0, 2.0), 1.5);
    let mut page = PageBuilder::new().cell("PageWidth", "10000").cell("PageHeight", "10000");
    for i in 0..5_000u32 {
        let s = f64::from(i) * d;
        page = page.shape(shape(i + 1, (a.0 + s * THIRTY.cos(), a.1 + s * THIRTY.sin()), THIRTY));
    }
    let doc = DocBuilder::new().page(1, page).build();
    // A subject between the 2,000th and the 2,001st, 0.02 inch off the shared lines along its own y axis.
    let s = 2_000.5 * d;
    let centre = (a.0 + s * THIRTY.cos() - 0.02 * THIRTY.sin(), a.1 + s * THIRTY.sin() + 0.02 * THIRTY.cos());
    let (out, work) = ask(&doc, centre, THIRTY);
    let lines: Vec<Run> = out
        .marks
        .iter()
        .filter_map(|m| match m {
            super::SnapMark::Line { normal: Some(_), count, from, to, .. } => Some((*count, *from, *to)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3, "{:?}", out.marks);
    assert!(lines.iter().all(|(count, ..)| *count == 5_000), "{lines:?}");
    // Each runs from the shape before the subject to the shape after it, and no farther.
    let run = |m: &(u32, (f64, f64), (f64, f64))| (m.2.0 - m.1.0).hypot(m.2.1 - m.1.1);
    assert!(lines.iter().all(|m| run(m) < 4.0 * d), "the marks run to the nearest shape on each side: {lines:?}");
    assert_eq!((work.gathers, work.visited), (0, 0), "{work:?}");
    eprintln!("5,000 shapes on three lines: bisection steps {}, line steps {}", work.bisect_steps, work.line_steps);
    // Measured: 555 steps and 3 line steps, whatever the number of shapes on a line (five bisections over the members of an entry).
    assert!((480..=640).contains(&work.bisect_steps) && work.line_steps <= 16, "{work:?}");
}

#[test]
fn a_subject_turned_nearly_a_quarter_turn_has_own_sides_nearly_parallel_to_the_page_and_is_never_thrown_along_a_line() {
    // Own directions a hair from the page's: a subject turned 1e-6, 1e-4 and 1e-2 radian, with a shape turned alike 0.03 inch off its own lines and a guide-free page, is pulled
    // by the larger of the reaches at most (the pair of nearly parallel directions would fix a move as long as one over the sine of the angle between them).
    for angle in [1.0e-6, 1.0e-4, 1.0e-2, 1.0e-2 + std::f64::consts::FRAC_PI_2] {
        let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
        // A shape turned alike at (5, 5), and one square to the page at (14, 5) whose page lines are the nearly parallel ones.
        page = page.shape(shape(1, (5.0, 5.0), angle)).shape(shape(2, (14.0, 5.0), 0.0));
        let doc = DocBuilder::new().page(1, page).build();
        let (s, c) = angle.sin_cos();
        // 4 inches along the first's own x axis and 0.03 inch along its own y axis.
        let at = (5.0 + 4.0 * c - 0.03 * s, 5.0 + 4.0 * s + 0.03 * c);
        let (out, _) = ask(&doc, at, angle);
        assert!(out.dx.hypot(out.dy) <= 0.2, "{angle:e}: thrown {} {}: {out:?}", out.dx, out.dy);
    }
}

#[test]
#[ignore = "a yardstick, not a test: prints the time of a turned subject's first and later queries on pages of 5,000 shapes (turned, a hair apart, on three lines) beside the same pages square to the page; run with --ignored --nocapture"]
fn timed_a_turned_subject_beside_the_same_page_square() {
    let time = |label: &str, doc: &Document, at: (f64, f64), radians: f64| {
        let page = SnapPage::read(doc, 1).unwrap();
        let first = std::time::Instant::now();
        let (out, _) = ask_on(doc, &page, at, radians);
        let first = first.elapsed();
        let n = 200u32;
        let later = std::time::Instant::now();
        for _ in 0..n {
            let _ = ask_on(doc, &page, at, radians);
        }
        let later = later.elapsed() / n;
        eprintln!("{label}: first query {first:?} (builds the targets), later queries {later:?}, {} hits", out.hits.len());
        later
    };
    let turned = page(5_000, |i| if i % 4 == 3 { 75.0_f64.to_radians() } else { THIRTY });
    let square = page(5_000, |_| 0.0);
    let at = (47.0 + 0.02 * -THIRTY.sin(), 5.0 + 0.02 * THIRTY.cos());
    let t = time("5,000 turned shapes, turned subject", &turned, at, THIRTY);
    let q = time("5,000 square shapes, square subject ", &square, (47.0, 5.02), 0.0);
    let hair = page(5_000, |i| THIRTY + f64::from(i) * 2.0e-9);
    let angle = THIRTY + 2_500.0 * 2.0e-9;
    let (x, y) = (f64::from(2_500u32 % 100) * 1.5 + 2.0, f64::from(2_500u32 / 100) * 1.5 + 2.0);
    let h = time("5,000 directions a hair apart     ", &hair, (x + 0.02 * -angle.sin(), y + 0.02 * angle.cos()), angle);
    eprintln!("turned over square {:.2}, a hair apart over square {:.2}", t.as_secs_f64() / q.as_secs_f64(), h.as_secs_f64() / q.as_secs_f64());
    assert!(t < q * 20 && h < q * 20, "a turned subject costs a few times a square one, not a multiple of the page: {t:?} {h:?} against {q:?}");
}
