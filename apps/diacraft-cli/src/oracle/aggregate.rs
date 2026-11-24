//! `oracle --aggregate`: the reports a run printed, added up honestly.
//!
//! A mean of file scores says nothing about the files that were not scored, so the aggregate states **totals first**: how many reports, how
//! many distinct files (one hash in two sources is one file), how many were refused, how many read but failed the control, how many read but
//! failed the real-file salt (a cell given a wrong saved value that still agreed: they are in no figure either), how many were scored; the cells compared, agreed and differed; the cells left out, by reason; the cells not calculated; and the cells that hold no
//! formula. The three means (by file, by source, capped by source) are over the scored files, and the coverage beside them says
//! what share of the formula cells they stand for.
//!
//! **A line that cannot be true is an error, not a number.** A report whose matched cells exceed its compared ones (it once gave a score of
//! 10.0), whose compared, left-out and not-calculated cells do not add up to its formula cells, whose real-file salt does not add up, or
//! whose score disagrees with its counts or stands under a salt that failed, is refused with the reason, and the aggregate is not made.

use std::collections::BTreeMap;

use serde_json::{Value as Json, json};

use super::file::SCHEMA;
use super::score::LEFT_OUT_WORDS;

/// What an aggregate reads of one report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLine {
    /// The repository or host, as the sample-file manifest names it. A file with none is its own source.
    pub source: Option<String>,
    pub sha256: String,
    /// Whether the file read. A refused file has no cells.
    pub read: bool,
    pub control_passed: bool,
    pub matched: u64,
    pub compared: u64,
    pub formula_cells: u64,
    pub cells_in_document: u64,
    pub not_formulas: u64,
    pub left_out: BTreeMap<&'static str, u64>,
    pub not_calculated: u64,
    /// The real-file salt of the file: cells salted, caught, agreeing through `ATAN2`'s zero-length rule, agreeing with no allowance
    /// (a failure), and chosen but not worked out again.
    pub salt: SaltCounts,
}

/// The counts of the real-file salt of one file, or of a set of files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaltCounts {
    pub salted: u64,
    pub caught: u64,
    pub by_allowance: u64,
    pub missed: u64,
    pub not_checked: u64,
}

impl SaltCounts {
    fn add(&mut self, other: SaltCounts) {
        self.salted = self.salted.saturating_add(other.salted);
        self.caught = self.caught.saturating_add(other.caught);
        self.by_allowance = self.by_allowance.saturating_add(other.by_allowance);
        self.missed = self.missed.saturating_add(other.missed);
        self.not_checked = self.not_checked.saturating_add(other.not_checked);
    }
}

fn number(v: &Json, path: &[&str]) -> Result<u64, String> {
    let mut at = v;
    for key in path {
        at = at.get(key).ok_or_else(|| format!("`{}` is missing", path.join(".")))?;
    }
    at.as_u64().ok_or_else(|| format!("`{}` is not a whole number", path.join(".")))
}

/// Reads one report's line, or says why it cannot be one.
pub fn parse_report(report: &Json) -> Result<FileLine, String> {
    let schema = report.get("schema").and_then(Json::as_u64).ok_or("not a report of `oracle --json`: no `schema`")?;
    if schema != u64::from(SCHEMA) {
        return Err(format!("a report of schema {schema}, and this reads {SCHEMA}: run the oracle again"));
    }
    let sha256 = report.get("sha256").and_then(Json::as_str).ok_or("no `sha256`")?;
    if sha256.is_empty() || sha256.len() > 128 || sha256.chars().any(char::is_whitespace) {
        return Err("`sha256` is not a hash".to_owned());
    }
    let source = match report.get("source") {
        None | Some(Json::Null) => None,
        Some(Json::String(s)) => Some(s.clone()),
        Some(_) => return Err("`source` is not text".to_owned()),
    };
    let read = report.get("read").and_then(Json::as_bool).ok_or("no `read`")?;
    let calc = report.get("calc").ok_or("no `calc`")?;
    let control_passed = calc.pointer("/control/passed").and_then(Json::as_bool).ok_or("no `calc.control.passed`")?;
    let mut line = FileLine {
        source,
        sha256: sha256.to_owned(),
        read,
        control_passed,
        matched: 0,
        compared: 0,
        formula_cells: 0,
        cells_in_document: 0,
        not_formulas: 0,
        left_out: BTreeMap::new(),
        not_calculated: 0,
        salt: SaltCounts::default(),
    };
    if !read {
        // A refused file has a refusal and no cells.
        if report.pointer("/refusal/class").and_then(Json::as_str).is_none() {
            return Err("a file that did not read has no `refusal.class`".to_owned());
        }
        if calc.get("compared").is_some() || report.get("score").is_some_and(|s| !s.is_null()) {
            return Err("a file that did not read has a score".to_owned());
        }
        return Ok(line);
    }
    let compared = number(calc, &["compared"])?;
    let matched = number(calc, &["matched"])?;
    let differed = number(calc, &["differed"])?;
    if matched > compared {
        return Err(format!("{matched} cells matched of {compared} compared"));
    }
    if matched.checked_add(differed) != Some(compared) {
        return Err(format!("{matched} matched and {differed} differed are not the {compared} compared"));
    }
    let formula = number(calc, &["cells", "formula"])?;
    let not_formulas = number(calc, &["cells", "not_formulas", "total"])?;
    let in_document = number(calc, &["cells", "in_document"])?;
    if formula.checked_add(not_formulas) != Some(in_document) {
        return Err(format!("{formula} formula cells and {not_formulas} without are not the {in_document} cells of the document"));
    }
    let left_out_total = number(calc, &["left_out_total"])?;
    let mut left_out = BTreeMap::new();
    let mut summed = 0u64;
    let words = calc.get("left_out").and_then(Json::as_object).ok_or("no `calc.left_out`")?;
    for (word, n) in words {
        let known = LEFT_OUT_WORDS.iter().find(|w| **w == word.as_str()).ok_or_else(|| format!("`{word}` is no reason a cell is left out"))?;
        let n = n.as_u64().ok_or("a count of cells left out is not a whole number")?;
        summed = summed.checked_add(n).ok_or("cells left out overflow")?;
        left_out.insert(*known, n);
    }
    if summed != left_out_total {
        return Err(format!("the reasons add up to {summed} cells left out, and the report says {left_out_total}"));
    }
    let not_calculated = number(calc, &["not_calculated", "total"])?;
    let accounted = compared.checked_add(left_out_total).and_then(|n| n.checked_add(not_calculated));
    if accounted != Some(formula) {
        return Err(format!(
            "{compared} compared, {left_out_total} left out and {not_calculated} not calculated are not the {formula} formula cells"
        ));
    }
    let salt = SaltCounts {
        salted: number(calc, &["real_salt", "salted"])?,
        caught: number(calc, &["real_salt", "caught"])?,
        by_allowance: number(calc, &["real_salt", "by_allowance"])?,
        missed: number(calc, &["real_salt", "missed"])?,
        not_checked: number(calc, &["real_salt", "not_checked"])?,
    };
    if salt.caught.checked_add(salt.by_allowance).and_then(|n| n.checked_add(salt.missed)) != Some(salt.salted) {
        return Err(format!(
            "the real-file salt: {} caught, {} through an allowance and {} missed are not the {} salted",
            salt.caught, salt.by_allowance, salt.missed, salt.salted
        ));
    }
    match report.get("score") {
        None | Some(Json::Null) => {}
        Some(score) => {
            if !control_passed {
                return Err("a score under a control that failed".to_owned());
            }
            if salt.missed > 0 {
                return Err("a score under a real-file salt that failed".to_owned());
            }
            if number(score, &["matched"])? != matched || number(score, &["compared"])? != compared {
                return Err("the score is not the counts".to_owned());
            }
            let given = score.get("score").and_then(Json::as_f64).ok_or("the score is not a number")?;
            if compared == 0 || (given - matched as f64 / compared as f64).abs() > 1e-9 {
                return Err(format!("a score of {given} for {matched} of {compared}"));
            }
        }
    }
    line.matched = matched;
    line.compared = compared;
    line.formula_cells = formula;
    line.cells_in_document = in_document;
    line.not_formulas = not_formulas;
    line.left_out = left_out;
    line.not_calculated = not_calculated;
    line.salt = salt;
    Ok(line)
}

/// The score of a set of reports.
#[derive(Clone, Debug, PartialEq)]
pub struct Aggregate {
    /// Lines read.
    pub reports: usize,
    /// Distinct files among them, by hash.
    pub files: usize,
    /// Lines dropped because their file was already counted, and the files whose two lines disagreed about what they found.
    pub duplicates_dropped: usize,
    pub duplicate_conflicts: usize,
    /// Distinct files that did not read.
    pub refused: usize,
    /// Distinct files that read but whose run's control failed: left out of every figure below.
    pub control_failed: usize,
    /// Distinct files that read, whose control passed, and in which a cell given a wrong saved value still agreed with its result (the
    /// real-file salt): left out of every figure below too, and the aggregate fails.
    pub real_salt_failed: usize,
    /// The real-file salt over the files that read and whose control passed, those that failed it included.
    pub salt: SaltCounts,
    /// Distinct files that were scored: read, control passed, something compared.
    pub scored: usize,
    pub sources: usize,
    /// Over the files that read and whose control passed.
    pub formula_cells: u64,
    pub cells_in_document: u64,
    pub not_formulas: u64,
    pub compared: u64,
    pub matched: u64,
    pub left_out: BTreeMap<&'static str, u64>,
    pub not_calculated: u64,
    /// The mean of the file scores, each scored file counting once.
    pub by_file: Option<f64>,
    /// The mean over sources of each source's mean file score.
    pub by_source: Option<f64>,
    /// The mean of the file scores with each source cut to its first `cap` files by SHA-256.
    pub capped: Option<f64>,
    pub cap: usize,
    /// Matched over compared, pooled over every cell.
    pub pooled: Option<f64>,
    /// Compared over formula cells, pooled.
    pub coverage: Option<f64>,
    /// Each source with its distinct files, those refused, those scored, and the mean score of those.
    pub per_source: BTreeMap<String, SourceFigure>,
}

/// One source's part of an aggregate.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceFigure {
    pub files: usize,
    pub refused: usize,
    pub scored: usize,
    pub score: Option<f64>,
}

/// Aggregates reports. `cap` is the most files one source adds to the capped figure.
pub fn aggregate(lines: &[FileLine], cap: usize) -> Aggregate {
    // One line for each file: the same hash in two sources is one file, kept under the least source name (a source over none), so that the
    // choice does not depend on the order the reports came in.
    let mut order: Vec<&FileLine> = lines.iter().collect();
    order.sort_by(|a, b| (&a.sha256, a.source.is_none(), &a.source).cmp(&(&b.sha256, b.source.is_none(), &b.source)));
    let mut files: Vec<&FileLine> = Vec::new();
    let mut duplicate_conflicts = 0usize;
    for line in order {
        match files.last() {
            Some(kept) if kept.sha256 == line.sha256 => {
                let same = (kept.read, kept.control_passed, kept.matched, kept.compared, kept.formula_cells)
                    == (line.read, line.control_passed, line.matched, line.compared, line.formula_cells);
                if !same {
                    duplicate_conflicts += 1;
                }
            }
            _ => files.push(line),
        }
    }
    let mut a = Aggregate {
        reports: lines.len(),
        files: files.len(),
        duplicates_dropped: lines.len().saturating_sub(files.len()),
        duplicate_conflicts,
        refused: 0,
        control_failed: 0,
        real_salt_failed: 0,
        salt: SaltCounts::default(),
        scored: 0,
        sources: 0,
        formula_cells: 0,
        cells_in_document: 0,
        not_formulas: 0,
        compared: 0,
        matched: 0,
        left_out: LEFT_OUT_WORDS.iter().map(|w| (*w, 0)).collect(),
        not_calculated: 0,
        by_file: None,
        by_source: None,
        capped: None,
        cap,
        pooled: None,
        coverage: None,
        per_source: BTreeMap::new(),
    };
    let mut by_source: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut figures: BTreeMap<String, SourceFigure> = BTreeMap::new();
    for f in &files {
        let key = f.source.clone().unwrap_or_else(|| format!("(no source) {}", f.sha256));
        let figure = figures.entry(key.clone()).or_insert(SourceFigure { files: 0, refused: 0, scored: 0, score: None });
        figure.files += 1;
        if !f.read {
            a.refused += 1;
            figure.refused += 1;
            continue;
        }
        if !f.control_passed {
            a.control_failed += 1;
            continue;
        }
        a.salt.add(f.salt);
        if f.salt.missed > 0 {
            a.real_salt_failed += 1;
            continue;
        }
        a.formula_cells = a.formula_cells.saturating_add(f.formula_cells);
        a.cells_in_document = a.cells_in_document.saturating_add(f.cells_in_document);
        a.not_formulas = a.not_formulas.saturating_add(f.not_formulas);
        a.compared = a.compared.saturating_add(f.compared);
        a.matched = a.matched.saturating_add(f.matched);
        a.not_calculated = a.not_calculated.saturating_add(f.not_calculated);
        for (word, n) in &f.left_out {
            let total = a.left_out.entry(word).or_insert(0);
            *total = total.saturating_add(*n);
        }
        if f.compared > 0 {
            a.scored += 1;
            figure.scored += 1;
            by_source.entry(key).or_default().push((f.sha256.clone(), f.matched as f64 / f.compared as f64));
        }
    }
    for list in by_source.values_mut() {
        list.sort_by(|x, y| x.0.cmp(&y.0));
    }
    let mean = |xs: &[f64]| (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64);
    let all: Vec<f64> = by_source.values().flatten().map(|(_, s)| *s).collect();
    for (key, list) in &by_source {
        if let Some(figure) = figures.get_mut(key) {
            figure.score = mean(&list.iter().map(|(_, s)| *s).collect::<Vec<_>>());
        }
    }
    let source_means: Vec<f64> = figures.values().filter_map(|f| f.score).collect();
    let capped: Vec<f64> = by_source.values().flat_map(|v| v.iter().take(cap).map(|(_, s)| *s)).collect();
    a.sources = figures.len();
    a.by_file = mean(&all);
    a.by_source = mean(&source_means);
    a.capped = mean(&capped);
    a.pooled = (a.compared > 0).then(|| a.matched as f64 / a.compared as f64);
    a.coverage = (a.formula_cells > 0).then(|| a.compared as f64 / a.formula_cells as f64);
    a.per_source = figures;
    a
}

/// An aggregate as JSON.
pub fn aggregate_json(a: &Aggregate) -> Json {
    json!({
        "schema": SCHEMA,
        "reports": a.reports,
        "files": a.files,
        "duplicates_dropped": a.duplicates_dropped,
        "duplicate_conflicts": a.duplicate_conflicts,
        "refused": a.refused,
        "control_failed": a.control_failed,
        "real_salt_failed": a.real_salt_failed,
        "real_salt": {
            "salted": a.salt.salted,
            "caught": a.salt.caught,
            "by_allowance": a.salt.by_allowance,
            "missed": a.salt.missed,
            "not_checked": a.salt.not_checked,
        },
        "scored": a.scored,
        "sources": a.sources,
        "cells": { "in_document": a.cells_in_document, "formula": a.formula_cells, "not_formulas": a.not_formulas },
        "compared": a.compared,
        "matched": a.matched,
        "left_out": a.left_out,
        "left_out_total": a.left_out.values().fold(0u64, |sum, n| sum.saturating_add(*n)),
        "not_calculated": a.not_calculated,
        "coverage": a.coverage,
        "pooled": a.pooled,
        "by_file": a.by_file,
        "by_source": a.by_source,
        "capped": a.capped,
        "cap": a.cap,
        "per_source": a
            .per_source
            .iter()
            .map(|(k, f)| (k.clone(), json!({ "files": f.files, "refused": f.refused, "scored": f.scored, "score": f.score })))
            .collect::<BTreeMap<_, _>>(),
    })
}
