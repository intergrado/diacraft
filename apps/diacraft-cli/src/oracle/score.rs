//! The score of one file, as the harness reports it: `diacraft-calc`'s report turned into numbers and words that may cross.
//!
//! **What crosses** (AGENTS.md: a hash, a category, a score, cell and function names, an error class, expected and actual values; no formula
//! text and no text content): names only from the spec's own lists (`names.rs`) and counted without being named when not listed; a number, a
//! colour, a Boolean, a date, a currency, a point and the numbers of a path as the value they are; **a text only as its length** (`<text:12>`),
//! because a label a person typed is content; an error as one of the six error words; a sheet by ids and fixed words; a row by its `IX`, and
//! a named row as "named" and no more.
//!
//! **What the report is honest about**: every formula cell is in exactly one of compared, left out (by reason) or not calculated (by
//! function, or the budget), the cells that hold no formula are counted, and the score has a coverage beside it: a score over one easy cell
//! is a score, and the coverage says it is one cell.

use std::collections::{BTreeMap, HashMap};

use diacraft_calc::{Difference, DifferenceKind, RealSalt, RecalcReport, SaltMiss, SheetPlace, Skipped, ValueClass};
use diacraft_doc::{Owner, RowKey};
use serde_json::{Value as Json, json};

use super::control::Control;
use super::names::FUNCTIONS;
use super::words::{UNLISTED, error_word, listed, listed_cell, section_name_word};

/// The most differences a report lists one by one (the rest are counted, and the tallies by function and by cell say where they are).
pub const MAX_LISTED: usize = 50;

/// The most differences of one kind of cell a report lists, so that one repeated cell cannot fill the list: the first 50 in walk order were
/// all of one shape once.
pub const MAX_LISTED_PER_KIND: usize = 2;

/// The counts a report is read by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub agreed: u64,
    pub differed: u64,
    /// Formula cells.
    pub formula_cells: u64,
    /// Every cell a walk of the document meets.
    pub cells_in_document: u64,
    pub left_out: u64,
    pub not_calculated: u64,
    pub agreed_by_allowance: u64,
    /// The real-file salt (`calc.real_salt`): cells salted with a wrong value, and those that still agreed with no allowance to say why.
    pub salted: u64,
    pub salt_missed: u64,
}

impl Summary {
    pub(super) fn of(r: &RecalcReport) -> Summary {
        Summary {
            agreed: r.agreed,
            differed: r.differed.len() as u64 + r.differed_unlisted,
            formula_cells: r.cells,
            cells_in_document: r.cells_in_document(),
            left_out: r.left_out(),
            not_calculated: r.not_calculated(),
            agreed_by_allowance: r.agreed_by_allowance,
            salted: r.real_salt.salted,
            salt_missed: r.real_salt.missed,
        }
    }

    /// Whether the real-file salt found nothing: no salted cell of the file agreed without an allowance to say why.
    pub fn salt_passed(&self) -> bool {
        self.salt_missed == 0
    }

    /// Cells compared: agreed or differed.
    pub fn compared(&self) -> u64 {
        self.agreed + self.differed
    }

    /// Agreed over compared, or `None` when nothing was compared.
    pub fn score(&self) -> Option<f64> {
        (self.compared() > 0).then(|| self.agreed as f64 / self.compared() as f64)
    }

    /// Compared over formula cells: how much of what could be scored was.
    pub fn coverage(&self) -> Option<f64> {
        (self.formula_cells > 0).then(|| self.compared() as f64 / self.formula_cells as f64)
    }
}

/// The reasons a formula cell is left out of the score, as the words a report gives them.
///
/// **`stale_theme_colour` counts only the cells left out** (the narrowing): a colour under a scheme the file
/// no longer selects whose saved value is the reading under the file's first theme part and not the sheet's own. Only a run can count them. The
/// reason `stale_theme_property` is gone with schema 7: a `THEMEPROP` on a sheet that reads no theme is scored.
pub const LEFT_OUT_WORDS: [&str; 16] = [
    "action_cell",
    "formula_not_parsed",
    "reads_clock_or_machine",
    "themed_no_error",
    "themed_operand",
    "nothing_saved",
    "inherited_no_formula",
    "date_past_9999",
    "date_past_9999_operand",
    "stale_theme_colour",
    "machine_font_number",
    "text_stand_in",
    "theme_no_shadow",
    "formula_must_fail",
    "depth_limit",
    "ambiguous_sheet",
];

/// Each reason with its count, in the order of [`LEFT_OUT_WORDS`].
pub fn left_out_counts(s: &Skipped) -> [(&'static str, u64); 16] {
    [
        (LEFT_OUT_WORDS[0], s.actions),
        (LEFT_OUT_WORDS[1], s.parse_failed),
        (LEFT_OUT_WORDS[2], s.clock_readers),
        (LEFT_OUT_WORDS[3], s.themed),
        (LEFT_OUT_WORDS[4], s.themed_operand),
        (LEFT_OUT_WORDS[5], s.nothing_saved),
        (LEFT_OUT_WORDS[6], s.inherited_no_formula),
        (LEFT_OUT_WORDS[7], s.far_year),
        (LEFT_OUT_WORDS[8], s.far_year_operand),
        (LEFT_OUT_WORDS[9], s.stale_theme_colour),
        (LEFT_OUT_WORDS[10], s.machine_font_number),
        (LEFT_OUT_WORDS[11], s.text_stand_in),
        (LEFT_OUT_WORDS[12], s.theme_no_shadow),
        (LEFT_OUT_WORDS[13], s.formula_must_fail),
        (LEFT_OUT_WORDS[14], s.depth_limit),
        (LEFT_OUT_WORDS[15], s.ambiguous_sheet),
    ]
}

/// The `calc` member of a report.
pub fn calc_json(r: &RecalcReport, control: &Control) -> Json {
    let s = Summary::of(r);
    let left_out: BTreeMap<&str, u64> = left_out_counts(&r.skipped).into_iter().collect();
    // Functions the evaluator lacks, named only when the spec lists them.
    let mut unsupported: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut unsupported_unlisted = 0u64;
    for (name, n) in &r.unsupported {
        match listed(FUNCTIONS, name) {
            Some(known) => *unsupported.entry(known).or_insert(0) += n,
            None => unsupported_unlisted += n,
        }
    }
    let mut by_function: BTreeMap<&'static str, [u64; 2]> = BTreeMap::new();
    let mut by_function_unlisted = [0u64; 2];
    for (name, score) in &r.by_function {
        match listed(FUNCTIONS, name) {
            Some(known) => {
                let e = by_function.entry(known).or_insert([0, 0]);
                e[0] += score.agreed;
                e[1] += score.differed;
            }
            None => {
                by_function_unlisted[0] += score.agreed;
                by_function_unlisted[1] += score.differed;
            }
        }
    }
    // The stale colours left out (schema 7), by the spec's names of the functions their formulas call, so that a jump in the count can be traced.
    let mut stale_by_function: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut stale_unlisted = 0u64;
    for (name, n) in &r.stale_theme_colour_by_function {
        match listed(FUNCTIONS, name) {
            Some(known) => *stale_by_function.entry(known).or_insert(0) += n,
            None => stale_unlisted += n,
        }
    }
    let mut by_cell: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    for ((section, cell), score) in &r.by_cell {
        let section = if section.is_empty() { "" } else { section_name_word(section) };
        let key = match listed_cell(cell) {
            Some(name) if section.is_empty() => name.to_owned(),
            Some(name) => format!("{section}.{name}"),
            None => UNLISTED.to_owned(),
        };
        let e = by_cell.entry(key).or_insert([0, 0]);
        e[0] += score.agreed;
        e[1] += score.differed;
    }
    let listed_differences = select(&r.differed);
    let kinds = outcome_counts(&r.differed);
    json!({
        "available": true,
        "control": control.json(),
        "cells": {
            "in_document": s.cells_in_document,
            "formula": s.formula_cells,
            "not_formulas": {
                "total": r.not_formulas.total(),
                "value_only": r.not_formulas.value_only,
                "no_formula": r.not_formulas.no_formula,
                "deleted": r.not_formulas.deleted,
                "with_saved_error": r.not_formulas.with_saved_error,
            },
        },
        "compared": s.compared(),
        "matched": s.agreed,
        "differed": s.differed,
        "differed_unlisted": r.differed_unlisted,
        "agreed_by_allowance": s.agreed_by_allowance,
        "agreed_by_curve": r.agreed_by_curve,
        "coverage": s.coverage(),
        "outcomes": kinds,
        "left_out": left_out,
        "left_out_total": s.left_out,
        "stale_theme_colour_by_function": stale_by_function,
        "stale_theme_colour_by_function_unlisted": stale_unlisted,
        "not_calculated": {
            "total": s.not_calculated,
            "unsupported": unsupported,
            "unsupported_unlisted": unsupported_unlisted,
            "unreached": r.unreached,
            "budget_exhausted": r.budget_exhausted,
            "memory_exhausted": r.memory_exhausted,
        },
        "by_function": by_function.iter().map(|(k, v)| (*k, json!({ "agreed": v[0], "differed": v[1] }))).collect::<BTreeMap<_, _>>(),
        "by_function_unlisted": { "agreed": by_function_unlisted[0], "differed": by_function_unlisted[1] },
        "by_cell": by_cell.iter().map(|(k, v)| (k.clone(), json!({ "agreed": v[0], "differed": v[1] }))).collect::<BTreeMap<_, _>>(),
        "document_date_checks": { "fit": r.document_date_checks.fit, "not_fit": r.document_date_checks.not_fit },
        "real_salt": real_salt_json(&r.real_salt),
        "differences_listed": listed_differences.len(),
        "differences": listed_differences.iter().map(|d| difference_json(d)).collect::<Vec<_>>(),
    })
}

/// The classes of difference, counted over the differences kept (the comparer keeps at most `MAX_DIFFERENCES`): the value only, the error
/// the file saved and the calculation did not give, the error it gave and the file did not save, two errors that are not the same, and both.
fn outcome_counts(differed: &[Difference]) -> Json {
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    for d in differed {
        *counts.entry(outcome_word(d)).or_insert(0) += 1;
    }
    json!(counts)
}

/// The class word of a difference.
pub fn outcome_word(d: &Difference) -> &'static str {
    match (d.kind, d.saved_error.is_some(), d.actual_error.is_some()) {
        (DifferenceKind::Value, ..) => "value_differs",
        (DifferenceKind::Error, true, false) => "error_missing",
        (DifferenceKind::Error, false, true) => "error_unexpected",
        (DifferenceKind::Error, ..) => "error_differs",
        (DifferenceKind::Both, ..) => "value_and_error_differ",
    }
}

/// The differences a report lists: at most [`MAX_LISTED_PER_KIND`] of one kind of cell, and [`MAX_LISTED`] in all, in walk order within that.
pub fn select(differed: &[Difference]) -> Vec<&Difference> {
    let mut kept: Vec<&Difference> = Vec::new();
    let mut per_kind: HashMap<(String, &'static str, &'static str, Option<&'static str>, &'static str), usize> = HashMap::new();
    for d in differed {
        if kept.len() >= MAX_LISTED {
            break;
        }
        let key = kind_of_cell(d);
        let n = per_kind.entry(key).or_insert(0);
        if *n < MAX_LISTED_PER_KIND {
            *n += 1;
            kept.push(d);
        }
    }
    kept
}

/// What makes two differences the same kind of cell: where, in which section, which cell, which function, and how it differed.
fn kind_of_cell(d: &Difference) -> (String, &'static str, &'static str, Option<&'static str>, &'static str) {
    let section = d.address.section.as_ref().map_or("", |s| section_name_word(s.name.as_str()));
    let cell = listed_cell(d.address.cell.as_str()).unwrap_or(UNLISTED);
    let function = d.functions.iter().find_map(|f| listed(FUNCTIONS, f));
    (place_word(&d.place).to_owned(), section, cell, function, outcome_word(d))
}

/// `document`, `style`, `page`, `master`, or `page shape`/`master shape`.
fn place_word(p: &SheetPlace) -> &'static str {
    match p {
        SheetPlace::Document => "document",
        SheetPlace::Style(_) => "style",
        SheetPlace::Page(_) => "page",
        SheetPlace::Master(_) => "master",
        SheetPlace::Shape { owner: Owner::Page(_), .. } => "page_shape",
        SheetPlace::Shape { owner: Owner::Master(_), .. } => "master_shape",
    }
}

/// A value as a report shows it: the printed value when it is no text, and the length when it is.
fn shown(printed: &Option<String>, class: Option<ValueClass>) -> Json {
    match (printed, class) {
        (Some(_), Some(ValueClass::Text { chars })) => json!(format!("<text:{chars}>")),
        (Some(p), _) => json!(p),
        (None, _) => Json::Null,
    }
}

/// A sheet as a report words it: by ids and fixed words.
fn place_json(place: &SheetPlace) -> Json {
    match *place {
        SheetPlace::Document => json!({ "where": "document" }),
        SheetPlace::Style(id) => json!({ "where": "style", "id": id }),
        SheetPlace::Page(id) => json!({ "where": "page", "id": id }),
        SheetPlace::Master(id) => json!({ "where": "master", "id": id }),
        SheetPlace::Shape { owner, id, depth } => match owner {
            Owner::Page(page) => json!({ "where": "shape", "part": "page", "id": page, "shape": id, "depth": depth }),
            Owner::Master(master) => json!({ "where": "shape", "part": "master", "id": master, "shape": id, "depth": depth }),
        },
    }
}

/// A row as a report words it: its `IX`, or "named" and no name (a row's name is content: it is said to be there, and not what it is).
fn row_json(row: &Option<RowKey>) -> Json {
    match row {
        None => Json::Null,
        Some(RowKey::Ix(n)) => json!({ "ix": n }),
        Some(RowKey::Name(_)) => json!({ "named": true }),
    }
}

/// The functions of a cell that the spec lists, and how many it does not.
fn listed_functions(functions: &[String]) -> (Vec<&'static str>, u64) {
    let mut named: Vec<&'static str> = Vec::new();
    let mut unlisted = 0u64;
    for f in functions {
        match listed(FUNCTIONS, f) {
            Some(known) => named.push(known),
            None => unlisted += 1,
        }
    }
    (named, unlisted)
}

/// The real-file salt of a file (`diacraft-calc` `salt.rs`): how many real cells were salted with a wrong value and how many the comparer caught,
/// those that agreed through `ATAN2`'s zero-length rule (the one allowance that lets any saved value agree), and those that agreed with
/// nothing to say why, which fail the run. Counts and fixed words only.
fn real_salt_json(s: &RealSalt) -> Json {
    json!({
        "passed": s.passed(),
        "salted": s.salted,
        "caught": s.caught,
        "by_allowance": s.by_allowance,
        "missed": s.missed,
        "not_checked": s.not_checked,
        "too_large": s.too_large,
        "by_kind": s.by_class.iter().map(|(class, t)| (class.word(), json!({ "salted": t.salted, "caught": t.caught }))).collect::<BTreeMap<_, _>>(),
        "misses": s.misses.iter().map(miss_json).collect::<Vec<_>>(),
    })
}

/// A salted cell that still agreed: where it is, by the spec's names, and the kind of value it was compared by.
fn miss_json(m: &SaltMiss) -> Json {
    let a = &m.address;
    let (functions, unlisted_functions) = listed_functions(&m.functions);
    json!({
        "sheet": place_json(&m.place),
        "section": a.section.as_ref().map(|s| section_name_word(s.name.as_str())),
        "row": row_json(&a.row),
        "cell": listed_cell(a.cell.as_str()),
        "functions": functions,
        "functions_unlisted": unlisted_functions,
        "kind": m.class.word(),
    })
}

/// One difference, with the sheet by ids, the cell by the spec's names, the section's `IX` and the row, and the values by kind.
pub fn difference_json(d: &Difference) -> Json {
    let a = &d.address;
    let cell = listed_cell(a.cell.as_str());
    let (functions, unlisted_functions) = listed_functions(&d.functions);
    json!({
        "sheet": place_json(&d.place),
        "section": a.section.as_ref().map(|s| section_name_word(s.name.as_str())),
        "section_ix": a.section.as_ref().and_then(|s| s.ix),
        "row": row_json(&a.row),
        "cell": cell,
        "inherited": d.inherited,
        "functions": functions,
        "functions_unlisted": unlisted_functions,
        "outcome": outcome_word(d),
        "expected": { "v": shown(&d.saved, d.saved_class), "e": d.saved_error.as_deref().map(error_word) },
        "actual": { "v": shown(&d.actual, d.actual_class), "e": d.actual_error.as_deref().map(error_word) },
    })
}
