//! The `cargo xtask ci` runner: steps in order, stopping at the first failure, and a summary block.

use std::cell::RefCell;

use xtask::ci::{Outcome, Step, run_steps, summary};

fn ok() -> Result<Outcome, String> {
    Ok(Outcome::Ok)
}

#[test]
fn every_step_is_listed_as_ok_or_skip() {
    let steps: Vec<Step> =
        vec![("fmt", Box::new(ok)), ("deny", Box::new(|| Ok(Outcome::Skipped("cargo-deny is not installed".into())))), ("layers", Box::new(ok))];
    let (text, result) = run_steps(&steps);
    assert!(result.is_ok());
    assert_eq!(
        text,
        "\nCI summary:\n  ok    fmt\n  skip  deny (cargo-deny is not installed)\n  ok    layers\n\nCI passed: 3 steps, 2 ok, 1 skipped\n"
    );
}

#[test]
fn the_first_failure_stops_the_run() {
    let ran = RefCell::new(Vec::new());
    let (text, result) = {
        let steps: Vec<Step<'_>> = vec![
            (
                "fmt",
                Box::new(|| {
                    ran.borrow_mut().push("fmt");
                    ok()
                }),
            ),
            (
                "clippy",
                Box::new(|| {
                    ran.borrow_mut().push("clippy");
                    Err("2 warnings".into())
                }),
            ),
            (
                "test",
                Box::new(|| {
                    ran.borrow_mut().push("test");
                    ok()
                }),
            ),
            (
                "wasm",
                Box::new(|| {
                    ran.borrow_mut().push("wasm");
                    ok()
                }),
            ),
        ];
        run_steps(&steps)
    };
    assert_eq!(*ran.borrow(), ["fmt", "clippy"], "steps after the failure must not run");
    assert_eq!(result.unwrap_err(), "ci failed at `clippy`");
    assert_eq!(text, "\nCI summary:\n  ok    fmt\n  FAIL  clippy: 2 warnings\n  -     test (not run)\n  -     wasm (not run)\n");
}

#[test]
fn a_failure_after_a_skip_keeps_the_skip_in_the_summary() {
    let steps: Vec<Step> = vec![("a", Box::new(|| Ok(Outcome::Skipped("nothing to check".into())))), ("b", Box::new(|| Err("x".into())))];
    let (text, result) = run_steps(&steps);
    assert!(result.is_err());
    assert_eq!(text, "\nCI summary:\n  skip  a (nothing to check)\n  FAIL  b: x\n");
}

#[test]
fn the_summary_renders_without_running_anything() {
    assert_eq!(summary(&[], None, &[]), "\nCI summary:\n");
    let done = [("a", Outcome::Ok)];
    assert_eq!(summary(&done, Some(("b", "bad")), &["c"]), "\nCI summary:\n  ok    a\n  FAIL  b: bad\n  -     c (not run)\n");
}
