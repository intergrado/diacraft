//! The `cargo xtask ci` runner: the steps in order, stopping at the first failure, and a summary
//! block that lists every step as ok, skip (with the reason) or, after a failure, FAIL and not run.

/// What a step that did not fail did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The check ran and passed.
    Ok,
    /// The check did not apply, with the reason (a tool or file that is not there yet).
    Skipped(String),
}

/// A step's check.
pub type StepFn<'a> = Box<dyn Fn() -> Result<Outcome, String> + 'a>;

/// A named step.
pub type Step<'a> = (&'static str, StepFn<'a>);

/// Runs `steps` in order until one fails. Returns the summary text and the result: `Err` names the
/// step that failed. Progress goes to stderr as the steps run; the caller prints the summary.
pub fn run_steps(steps: &[Step<'_>]) -> (String, Result<(), String>) {
    let mut done: Vec<(&str, Outcome)> = Vec::new();
    for (index, (name, check)) in steps.iter().enumerate() {
        eprintln!("\n=== ci: {name} ===");
        match check() {
            Ok(outcome) => done.push((name, outcome)),
            Err(error) => {
                let not_run: Vec<&str> = steps.iter().skip(index + 1).map(|(n, _)| *n).collect();
                return (summary(&done, Some((name, &error)), &not_run), Err(format!("ci failed at `{name}`")));
            }
        }
    }
    let skipped = done.iter().filter(|(_, o)| matches!(o, Outcome::Skipped(_))).count();
    let text = format!("{}\nCI passed: {} steps, {} ok, {skipped} skipped\n", summary(&done, None, &[]), done.len(), done.len() - skipped);
    (text, Ok(()))
}

/// The `CI summary:` block: the steps that passed or were skipped, the step that failed with its
/// error, and the steps after it that did not run.
pub fn summary(done: &[(&str, Outcome)], failed: Option<(&str, &str)>, not_run: &[&str]) -> String {
    let mut text = String::from("\nCI summary:\n");
    for (name, outcome) in done {
        match outcome {
            Outcome::Ok => text.push_str(&format!("  ok    {name}\n")),
            Outcome::Skipped(why) => text.push_str(&format!("  skip  {name} ({why})\n")),
        }
    }
    if let Some((name, error)) = failed {
        text.push_str(&format!("  FAIL  {name}: {error}\n"));
    }
    for name in not_run {
        text.push_str(&format!("  -     {name} (not run)\n"));
    }
    text
}
