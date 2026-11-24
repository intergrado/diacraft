//! `diacraft-cli oracle`: the oracle harness over files, and the aggregation of what it printed.
//!
//! **This is where the reader meets the comparer**. `diacraft-vsdx` reads a file into the document model and says what it could
//! not hold; `diacraft-calc` works every formula cell out and compares the result with the `V` and `E` the file saved, and decides what is left
//! out of a score and why; this module reads, asks, and prints what may cross (`AGENTS.md`: a hash, a category, a score, cell and function
//! names, an error class, expected and actual values; no formula text, no text content, no name a person made up). It lives in the app layer
//! and not in a format crate, so the format crate and the web build carry no JSON, no name lists and no harness.
//!
//! * [`file`] is one file's report. [`score`] turns `calc`'s report into what may be printed. [`control`] is the salted control that every run
//!   begins with. [`aggregate`] adds reports up. [`refusal`] words why a file would not read. [`names`] is the spec's name lists, and [`words`]
//!   the rules for using them.
//! * **The control.** A score of 100% proves nothing by itself, so every run first scores a document with known-wrong values, through the
//!   function a file goes through. When any salted cell fails to differ, or an honest one fails to agree, the run has no score: each report
//!   says so, the command ends with status 1, and a test shows that a calculator which hands back the saved values is caught.
//! * **The real-file salt**. The control is one synthetic document, and a calculator that hands back the saved values agrees with every
//!   cell of a real file, so every scored file is salted too: a seeded sample of the real cells that agreed (the first of each kind of value, and
//!   one in a thousand of the rest) gets a wrong saved value of its kind, is worked out again and compared by the same comparer. The report says how
//!   many were caught (`calc.real_salt`). A salted cell that still agrees, but for `ATAN2`'s zero-length rule, withholds the file's score and the
//!   run ends with status 1.
//! * **Coverage.** A report says how many cells it met, how many were compared, left out (by reason) and not calculated (by function, or the
//!   budget), how many hold no formula, and the aggregate says how many files were refused and how many were scored. A mean of scores over
//!   the files that scored is never the whole story, and is never printed alone.

mod aggregate;
mod control;
mod file;
mod names;
mod refusal;
mod score;
mod sha256;
pub(crate) use sha256::sha256_hex;

/// A cell name as a report may print it: the spec's own spelling when the spec lists it, else a fixed word (a name a file made up is content).
pub(crate) fn cell_word(name: &str) -> &'static str {
    words::listed_cell(name).unwrap_or(words::UNLISTED)
}

/// A section name as a report may print it: one of the 17 predefined names, else a fixed word.
pub(crate) fn section_word(name: &str) -> &'static str {
    words::section_name_word(name)
}
#[cfg(test)]
mod tests;
mod words;

use std::path::Path;

use diacraft_calc::Native;
use diacraft_engine::file::read_file;
use diacraft_engine::shown;
use diacraft_mcp::json_line;
use diacraft_vsdx::Limits;
use serde_json::Value;

use crate::args::OracleArgs;
use crate::{Failure, say, write_line};

use aggregate::{FileLine, aggregate, aggregate_json, parse_report};
use file::{OracleReport, report};

/// Bytes a file may hold beyond what a package may inflate to, as the container's own overhead: the largest file `read_file` reads is the
/// package cap and this.
const FILE_SLACK: u64 = 64 * 1024 * 1024;

/// The most bytes of reports `--aggregate` reads from one file. A report is a few kilobytes, so this is tens of thousands of them.
const MAX_REPORTS_BYTES: u64 = 256 * 1024 * 1024;

/// The most characters of a file name a message repeats.
const MOST_PATH: usize = 300;

pub fn run(args: OracleArgs) -> Result<(), Failure> {
    if args.aggregate {
        return aggregate_reports(&args);
    }
    let limits = Limits::default();
    let max = limits.opc.max_package_bytes.saturating_add(FILE_SLACK);
    let mut calc = Native;
    // The control comes first, once for the run, through the path a file takes.
    let control = control::run(&mut calc);
    if !args.json {
        write_line(&control.words())?;
    }
    let mut unread = 0usize;
    let mut salt_failed = 0usize;
    for file in &args.files {
        let bytes = match read_file(Path::new(file), max) {
            Ok(bytes) => bytes,
            Err(e) => {
                say(std::io::stderr(), &format!("diacraft-cli: {}: {e}\n", shown(file, MOST_PATH)));
                unread += 1;
                continue;
            }
        };
        let r = report(&bytes, &limits, args.source.as_deref(), &mut calc, &control);
        if !r.salt_passed() {
            salt_failed += 1;
        }
        if args.json {
            let line = json_line(r.to_json()).map_err(|e| Failure::Message(format!("cannot encode a report: {e}")))?;
            write_line(&line)?;
        } else {
            for line in words(&r) {
                write_line(&line)?;
            }
        }
    }
    verdict(&control, salt_failed, unread)
}

/// How a run ends once every report is printed: loudly when the salted control failed or a file failed the real-file salt, and for files that
/// could not be read.
fn verdict(control: &control::Control, salt_failed: usize, unread: usize) -> Result<(), Failure> {
    if !control.passed() {
        return Err(Failure::Message(format!("the salted control failed, so no score of this run can be trusted: {}", control.words())));
    }
    if salt_failed > 0 {
        return Err(Failure::Message(format!(
            "{salt_failed} file(s) failed the real-file salt: a cell given a wrong saved value still agreed with its result, so the calculator \
             reads the saved value or the comparer cannot tell them apart, and no score of those files can be trusted (see `calc.real_salt`)"
        )));
    }
    if unread > 0 {
        return Err(Failure::Message(format!("{unread} file(s) could not be read")));
    }
    Ok(())
}

/// A report as a few lines of words.
fn words(r: &OracleReport) -> Vec<String> {
    let mut lines = vec![format!("sha256 {}", r.sha256), format!("bytes {}", r.bytes), format!("source {}", r.source.as_deref().unwrap_or("-"))];
    match (&r.refusal, &r.category) {
        (Some(refusal), _) => {
            let detail = refusal.detail.as_deref().map(|d| format!(", {d}")).unwrap_or_default();
            lines.push(format!("refused {} ({}{detail})", refusal.class, refusal.kind));
        }
        (None, Some(c)) => {
            let variant = c
                .variant
                .map_or_else(|| "unknown variant".to_owned(), |(kind, m)| if m { format!("{kind}, macros allowed") } else { kind.to_owned() });
            lines.push(format!(
                "read: {variant}; writer {}; oracle usable {}; routing usable {}",
                c.writer,
                yes(c.oracle_usable),
                yes(c.routing_usable)
            ));
            let stats = &r.to_json()["read_report"]["stats"];
            let n = |key: &str| stats[key].as_u64().unwrap_or(0);
            lines.push(format!(
                "pages {}, masters {}, styles {}, shapes {}, cells {}, formula cells {}",
                n("pages"),
                n("masters"),
                n("styles"),
                n("shapes"),
                n("cells"),
                n("formula_cells")
            ));
        }
        (None, None) => {}
    }
    if let Some(s) = &r.calc {
        lines.push(format!(
            "formula cells {}: compared {} ({} agreed, {} differed), left out {}, not calculated {}; cells without a formula {}",
            s.formula_cells,
            s.compared(),
            s.agreed,
            s.differed,
            s.left_out,
            s.not_calculated,
            s.cells_in_document.saturating_sub(s.formula_cells)
        ));
        lines.push(format!(
            "real-file salt: {} cells given a wrong saved value, {} still agreed{}",
            s.salted,
            s.salt_missed,
            if s.salt_passed() { " (none)" } else { " (FAILED)" }
        ));
    }
    lines.push(match &r.calc {
        None => "score - (the file did not read)".to_owned(),
        Some(s) => match r.score() {
            Some(score) => {
                let coverage = s.coverage().map_or_else(String::new, |c| format!(", coverage {c:.4}"));
                format!("score {score:.4} ({} of {}){coverage}", s.agreed, s.compared())
            }
            None if !r.control_passed && s.compared() > 0 => "score - (withheld: the control failed)".to_owned(),
            None if !r.salt_passed() && s.compared() > 0 => "score - (withheld: the real-file salt failed)".to_owned(),
            None => "score - (nothing compared)".to_owned(),
        },
    });
    lines
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// `--aggregate`: the reports in each file, a line of JSON each, added up: totals, and the score by file, by source and capped.
fn aggregate_reports(args: &OracleArgs) -> Result<(), Failure> {
    let mut lines: Vec<FileLine> = Vec::new();
    for file in &args.files {
        let bytes = read_file(Path::new(file), MAX_REPORTS_BYTES).map_err(|e| Failure::Message(format!("{}: {e}", shown(file, MOST_PATH))))?;
        let text = String::from_utf8(bytes).map_err(|_| Failure::Message(format!("{}: not UTF-8 text", shown(file, MOST_PATH))))?;
        for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let value: Value = serde_json::from_str(line)
                .map_err(|e| Failure::Message(format!("{} line {}: not JSON: {}", shown(file, MOST_PATH), n + 1, shown(&e.to_string(), 200))))?;
            let parsed = parse_report(&value).map_err(|why| {
                Failure::Message(format!("{} line {}: not a report of `oracle --json`: {}", shown(file, MOST_PATH), n + 1, shown(&why, 300)))
            })?;
            lines.push(parsed);
        }
    }
    let total = aggregate(&lines, args.cap);
    let line = json_line(&aggregate_json(&total)).map_err(|e| Failure::Message(format!("cannot encode the aggregate: {e}")))?;
    write_line(&line)?;
    // The line is printed first, so that it can be read; the status says that part of it is not to be trusted.
    if total.real_salt_failed > 0 {
        return Err(Failure::Message(format!(
            "{} file(s) failed the real-file salt, and are in no figure of the aggregate: a cell given a wrong saved value still agreed",
            total.real_salt_failed
        )));
    }
    Ok(())
}
