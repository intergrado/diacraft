//! Of the narrow re-review of 3905d3c: the charge of an equation's compiled form, and the work an arc of no sweep
//! costs. Each test fails on 3905d3c (the first two fail on a change of the charge that the suite let through), the two timed inputs print
//! what they cost.

use std::time::{Duration, Instant};

use diacraft_odg::{Limits, ReadReport, read_partial};

use crate::drawing::Drawing;

/// A custom shape of 4 cm by 2 cm whose enhanced geometry is `attrs` (with its `>`) and `inner`.
fn custom(attrs: &str, inner: &str) -> String {
    format!(
        r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" {attrs}>{inner}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

fn read_with(shapes: &str, limits: &Limits) -> (Result<diacraft_doc::Document, diacraft_odg::OdgError>, ReadReport, Duration) {
    let bytes = Drawing::a4().page(shapes).bytes();
    let started = Instant::now();
    let (result, report) = read_partial(&bytes, limits);
    (result, report, started.elapsed())
}

// ----: the compiled form of an equation is charged by what it holds

/// A flat chain `1+1+...+1` of `terms` terms: an operation for each token, and no tree (the formula crate refuses a chain that deep).
fn chain(terms: usize) -> String {
    "1+".repeat(terms - 1) + "1"
}

fn equations_of(text: &str, count: usize) -> String {
    (0..count).map(|i| format!(r#"<draw:equation draw:name="f{i}" draw:formula="{text}"/>"#)).collect()
}

#[test]
fn a_shape_s_equations_are_charged_for_the_operations_they_compile_to() {
    // 100 equations of a flat chain of 10,000 terms: 19,999 operations of 16 bytes each, held together until the shape's equations are
    // evaluated. At least 16 bytes an operation is 250,000 items for the hundred and the text alone (a mutant of the charge) is 31,000.
    let text = chain(10_000);
    assert_eq!(text.len(), 19_999);
    let shape = custom(r#"draw:enhanced-path="M 0 0 L 1 1""#, &equations_of(&text, 100));
    let (result, report, _) = read_with(&shape, &Limits::default());
    result.expect("reads");
    let operations = 100 * 19_999u64;
    assert!(report.work.items >= operations * 16 / 64, "{} items for {operations} operations", report.work.items);
    // And a cap that those operations pass is refused: the compiled forms are what a read holds.
    let (result, _, _) = read_with(&shape, &Limits { max_items: 300_000, ..Limits::default() });
    assert_eq!(result.err().map(|e| e.class()), Some("too_many_items"));
}

#[test]
fn an_equation_of_a_flat_chain_costs_its_operations_one_at_a_time() {
    // The charge is taken before the next equation is read, so the cap stops the shape's equations at the one that passes it: far fewer than
    // the 1,900 of the review's input are compiled.
    let text = chain(60_000);
    let shape = custom(r#"draw:enhanced-path="M 0 0 L 1 1""#, &equations_of(&text, 300));
    let (result, report, took) = read_with(&shape, &Limits { max_items: 2_000_000, ..Limits::default() });
    assert_eq!(result.err().map(|e| e.class()), Some("too_many_items"));
    // 119,999 operations of 16 bytes, 30,000 items each: the cap of two million is passed by the 67th, and the read stops there.
    assert!(report.work.items > 1_900_000 && report.work.items <= 2_000_000, "{} items", report.work.items);
    assert!(took < Duration::from_secs(30), "{took:?}");
}

// ----: an arc of no sweep

/// `shapes` custom shapes of 100 `T` arcs each: six parameters of 119 characters that sum references to one equation, and the same
/// expression for both angles, so that no arc has a sweep. The parameter is an expression or, for the control, a plain number as long.
fn arcs_of_no_sweep(shapes: usize, expression: bool) -> (String, usize) {
    let sum = "?g0+".repeat(29) + "?g0";
    assert_eq!(sum.len(), 119);
    // The control is as long a text of plain numbers (no expression to compile, an arc of no radius), so that the markup the shape keeps
    // is as long too and is charged the same.
    let zeros = format!("0.{}", "0".repeat(117));
    let p = if expression { sum.as_str() } else { zeros.as_str() };
    let arc = format!("T {p} {p} {p} {p} {p} {p} ");
    let path = format!("M 0 0 {}", arc.repeat(100));
    let shape = custom(&format!(r#"draw:enhanced-path="{path}""#), r#"<draw:equation draw:name="g0" draw:formula="1"/>"#);
    (shape.repeat(shapes), if expression { 6 * 119 } else { 0 })
}

#[test]
fn arcs_of_no_sweep_pay_for_their_parameters_and_compile_each_once() {
    let shapes = 40;
    let ((with, per_arc), (without, _)) = (arcs_of_no_sweep(shapes, true), arcs_of_no_sweep(shapes, false));
    let (a, report, _) = read_with(&with, &Limits::default());
    a.expect("reads");
    let (b, control, _) = read_with(&without, &Limits::default());
    b.expect("reads");
    // Each parameter compiled once: the six of each arc, and the equation's formula "1" of each shape.
    assert_eq!(report.work.expression_bytes, (shapes * 100 * per_arc + shapes) as u64);
    assert_eq!(control.work.expression_bytes, shapes as u64);
    // The bytes of the six parameters are items, one for each 64 and whole items for each arc (714 bytes: 12 items), though no arc makes a row.
    assert_eq!(report.work.items - control.work.items, (shapes * 100 * per_arc.div_ceil(64)) as u64);
    assert_eq!(per_arc.div_ceil(64), 12);
    assert_eq!(report.stats.cells, control.stats.cells, "no arc made a cell more");
}

/// The review's input, a tenth of it: 340 shapes of 100 arcs, 25 MB of `content.xml`. The count above holds a small one to its charge; this
/// holds the large one to the same charge and its time to a bound, and prints it.
#[test]
fn many_arcs_of_no_sweep_and_long_parameters_are_charged_and_read_in_linear_time() {
    let (shapes, per_arc) = arcs_of_no_sweep(340, true);
    let (result, report, took) = read_with(&shapes, &Limits::default());
    eprintln!("340 shapes of 100 arcs of no sweep: {took:?}, items {}, bytes compiled {}", report.work.items, report.work.expression_bytes);
    result.expect("reads");
    assert!(took < Duration::from_secs(60), "{took:?}");
    assert_eq!(report.work.expression_bytes, (340 * 100 * per_arc + 340) as u64);
    // The same shapes with parameters that are plain numbers of the same length: the difference is the 12 items an arc's parameters cost.
    let (control, _) = arcs_of_no_sweep(340, false);
    let (result, plain, _) = read_with(&control, &Limits::default());
    result.expect("reads");
    assert_eq!(report.work.items - plain.work.items, 340 * 100 * 12);
}
