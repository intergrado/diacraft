//! The aggregation: totals first (reports, distinct files, refused, scored, cells compared, left out and not calculated), the three means
//! beside them, a file counted once however many sources hold it, and a line that cannot be true refused with its reason.

use serde_json::{Value, json};

use super::super::aggregate::{Aggregate, FileLine, aggregate, aggregate_json, parse_report};
use super::super::file::SCHEMA;
use super::super::score::LEFT_OUT_WORDS;

/// A report of a file that read, as `oracle --json` prints one: `matched` of `compared` cells, `left_out` clock readers (the rest of the
/// reasons zero), `unsupported` cells not calculated, and `value_only` cells with no formula.
fn read_report(sha: &str, source: Option<&str>, matched: u64, compared: u64, left_out: u64, unsupported: u64, value_only: u64) -> Value {
    let formula = compared + left_out + unsupported;
    let mut reasons = serde_json::Map::new();
    for w in LEFT_OUT_WORDS {
        reasons.insert(w.to_owned(), json!(if w == "reads_clock_or_machine" { left_out } else { 0 }));
    }
    json!({
        "schema": SCHEMA, "sha256": sha, "source": source, "read": true,
        "calc": {
            "available": true,
            "control": { "passed": true },
            "cells": { "in_document": formula + value_only, "formula": formula, "not_formulas": { "total": value_only } },
            "compared": compared, "matched": matched, "differed": compared - matched,
            "left_out": reasons, "left_out_total": left_out,
            "not_calculated": { "total": unsupported },
            // Three cells salted with a wrong saved value, all caught: what a file with an agreeing cell reports.
            "real_salt": { "passed": true, "salted": 3, "caught": 3, "by_allowance": 0, "missed": 0, "not_checked": 0 },
        },
        "score": if compared > 0 { json!({ "score": matched as f64 / compared as f64, "matched": matched, "compared": compared }) } else { Value::Null },
    })
}

fn refused_report(sha: &str, source: Option<&str>) -> Value {
    json!({
        "schema": SCHEMA, "sha256": sha, "source": source, "read": false,
        "refusal": { "class": "not_zip", "kind": "not_a_package", "detail": null },
        "calc": { "available": true, "control": { "passed": true } },
        "score": null,
    })
}

fn lines(reports: &[Value]) -> Vec<FileLine> {
    reports.iter().map(|r| parse_report(r).unwrap()).collect()
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-12)
}

fn aggregated(reports: &[Value], cap: usize) -> Aggregate {
    aggregate(&lines(reports), cap)
}

#[test]
fn a_score_is_reported_by_file_by_source_and_with_each_source_capped() {
    // Source A holds four files at 1.0 and source B one at 0.0: by file the mean is 0.8, by source 0.5, and capped at two files a source 2/3.
    let a = aggregated(
        &[
            read_report("01", Some("a/a"), 10, 10, 0, 0, 0),
            read_report("02", Some("a/a"), 10, 10, 0, 0, 0),
            read_report("03", Some("a/a"), 10, 10, 0, 0, 0),
            read_report("04", Some("a/a"), 10, 10, 0, 0, 0),
            read_report("05", Some("b/b"), 0, 10, 0, 0, 0),
            read_report("06", Some("c/c"), 0, 0, 3, 0, 0),
            read_report("07", None, 5, 10, 0, 0, 0),
        ],
        2,
    );
    assert_eq!((a.files, a.scored, a.sources), (7, 6, 4));
    assert!(near(a.by_file, (4.0 + 0.0 + 0.5) / 6.0));
    // A file with no source is its own source: three scored sources of means 1.0, 0.0 and 0.5 (c/c scored nothing).
    assert!(near(a.by_source, 0.5));
    assert!(near(a.capped, (2.0 + 0.0 + 0.5) / 4.0));
    assert_eq!(a.per_source["a/a"].files, 4);
    assert!(near(a.per_source["a/a"].score, 1.0));
    assert_eq!(a.per_source["c/c"].scored, 0, "a source with nothing scored is listed, and has no score");
    assert_eq!(a.per_source["c/c"].score, None);
    let json = aggregate_json(&a);
    assert_eq!(json["sources"], 4);
    assert_eq!(json["per_source"]["a/a"]["files"], 4);
}

#[test]
fn the_cap_takes_the_same_files_whatever_order_they_arrive_in() {
    let mut reports = vec![
        read_report("03", Some("a/a"), 0, 10, 0, 0, 0),
        read_report("01", Some("a/a"), 10, 10, 0, 0, 0),
        read_report("02", Some("a/a"), 10, 10, 0, 0, 0),
    ];
    let one = aggregated(&reports, 2).capped;
    reports.reverse();
    assert_eq!(aggregated(&reports, 2).capped, one);
    assert!(near(one, 1.0), "the first two by hash are 01 and 02");
}

#[test]
fn nothing_scored_is_no_figure() {
    let a = aggregated(&[read_report("1", Some("a"), 0, 0, 2, 0, 0)], 5);
    assert_eq!((a.files, a.scored, a.by_file, a.by_source, a.capped, a.pooled, a.coverage), (1, 0, None, None, None, None, Some(0.0)));
}

#[test]
fn refused_files_never_vanish_and_a_mean_of_the_rest_says_how_few_they_are() {
    // The review's I5: three of four refused gave `files: 1, by_file: 1.0`. Now the aggregate says four reports, three refused, one scored.
    let a = aggregated(
        &[
            read_report("aa", Some("a/a"), 1, 1, 0, 0, 0),
            refused_report("bb", Some("a/a")),
            refused_report("cc", Some("b/b")),
            refused_report("dd", None),
        ],
        5,
    );
    assert_eq!((a.reports, a.files, a.refused, a.scored), (4, 4, 3, 1));
    assert!(near(a.by_file, 1.0));
    assert_eq!(a.per_source["a/a"].refused, 1);
    assert_eq!(a.per_source["b/b"].refused, 1);
    let json = aggregate_json(&a);
    assert_eq!((json["reports"].as_u64(), json["refused"].as_u64(), json["scored"].as_u64()), (Some(4), Some(3), Some(1)));
}

#[test]
fn a_score_over_one_easy_cell_has_its_coverage_beside_it() {
    // One cell compared and agreed, a thousand left out or not calculated: a score of 1.0 and a coverage of a thousandth.
    let a = aggregated(&[read_report("aa", Some("a/a"), 1, 1, 600, 399, 50)], 5);
    assert!(near(a.by_file, 1.0) && near(a.pooled, 1.0));
    assert!(near(a.coverage, 1.0 / 1000.0), "{:?}", a.coverage);
    assert_eq!((a.formula_cells, a.compared, a.not_calculated, a.not_formulas, a.cells_in_document), (1000, 1, 399, 50, 1050));
    assert_eq!(a.left_out["reads_clock_or_machine"], 600);
    let json = aggregate_json(&a);
    assert_eq!(json["left_out_total"], 600);
    assert_eq!(json["not_calculated"], 399);
}

#[test]
fn one_file_in_two_sources_counts_once() {
    // The review's I5: one hash in two sources counted twice. It is one file, kept under the lesser source name whatever order the lines come.
    let one = read_report("ab", Some("z/z"), 5, 10, 0, 0, 0);
    let two = read_report("ab", Some("a/a"), 5, 10, 0, 0, 0);
    for reports in [[one.clone(), two.clone()], [two, one]] {
        let a = aggregated(&reports, 5);
        assert_eq!((a.reports, a.files, a.duplicates_dropped, a.duplicate_conflicts, a.scored), (2, 1, 1, 0, 1));
        assert_eq!(a.compared, 10, "its cells are counted once");
        assert_eq!(a.sources, 1);
        assert!(a.per_source.contains_key("a/a") && !a.per_source.contains_key("z/z"));
    }
    // A source beats none.
    let a = aggregated(&[read_report("ab", None, 5, 10, 0, 0, 0), read_report("ab", Some("q/q"), 5, 10, 0, 0, 0)], 5);
    assert!(a.per_source.contains_key("q/q") && a.per_source.len() == 1);
}

#[test]
fn two_lines_of_one_hash_that_disagree_are_counted_as_a_conflict() {
    let a = aggregated(&[read_report("ab", Some("a/a"), 5, 10, 0, 0, 0), read_report("ab", Some("b/b"), 9, 10, 0, 0, 0)], 5);
    assert_eq!((a.files, a.duplicate_conflicts), (1, 1));
}

#[test]
fn a_file_whose_control_failed_is_in_no_figure_and_is_counted() {
    let mut bad = read_report("bb", Some("a/a"), 10, 10, 0, 0, 0);
    bad["calc"]["control"]["passed"] = json!(false);
    bad["score"] = Value::Null;
    let a = aggregated(&[read_report("aa", Some("a/a"), 5, 10, 0, 0, 0), bad], 5);
    assert_eq!((a.files, a.control_failed, a.scored), (2, 1, 1));
    assert_eq!(a.compared, 10, "the cells of the run whose control failed are not added");
    assert!(near(a.by_file, 0.5));
}

#[test]
fn a_file_whose_real_cells_salted_with_a_wrong_value_still_agreed_is_in_no_figure_and_the_aggregate_says_so() {
    // The real-file salt: an echo calculator agrees with every cell of a real file. A file with a salted cell that agreed is withheld.
    let mut bad = read_report("bb", Some("a/a"), 10, 10, 0, 0, 0);
    bad["calc"]["real_salt"] = json!({ "passed": false, "salted": 3, "caught": 0, "by_allowance": 0, "missed": 3, "not_checked": 0 });
    bad["score"] = Value::Null;
    bad["score_withheld"] = json!("real_salt_failed");
    let a = aggregated(&[read_report("aa", Some("a/a"), 5, 10, 0, 0, 0), bad.clone()], 5);
    assert_eq!((a.files, a.real_salt_failed, a.control_failed, a.scored), (2, 1, 0, 1));
    assert_eq!(a.compared, 10, "the cells of the file whose salt failed are not added");
    assert!(near(a.by_file, 0.5));
    // The salt itself is added up, the failed file's too, so that the misses can be read.
    assert_eq!((a.salt.salted, a.salt.caught, a.salt.missed), (6, 3, 3));
    let json = aggregate_json(&a);
    assert_eq!(json["real_salt_failed"], 1);
    assert_eq!(json["real_salt"], json!({ "salted": 6, "caught": 3, "by_allowance": 0, "missed": 3, "not_checked": 0 }));
    // A score under a failed salt is a line that cannot be true, and so is a salt that does not add up.
    let mut scored = bad.clone();
    scored["score"] = json!({ "score": 1.0, "matched": 10, "compared": 10 });
    assert!(parse_report(&scored).unwrap_err().contains("real-file salt that failed"));
    let mut sums = read_report("cc", None, 10, 10, 0, 0, 0);
    sums["calc"]["real_salt"]["caught"] = json!(2);
    assert!(parse_report(&sums).unwrap_err().contains("are not the 3 salted"));
    // And ATAN2's zero-length rule is an allowance and no failure.
    let mut allowed = read_report("dd", None, 10, 10, 0, 0, 0);
    allowed["calc"]["real_salt"] = json!({ "passed": true, "salted": 3, "caught": 2, "by_allowance": 1, "missed": 0, "not_checked": 0 });
    let a = aggregated(&[allowed], 5);
    assert_eq!((a.real_salt_failed, a.scored, a.salt.by_allowance), (0, 1, 1));
}

#[test]
fn a_report_of_an_older_schema_is_refused_because_its_reasons_are_not_this_ones() {
    // Schema 2 counted a cell reading a `Themed` operand as an unsupported function; a mix of the two would add unlike things.
    let mut old = read_report("aa", None, 1, 1, 0, 0, 0);
    old["schema"] = json!(2);
    assert!(parse_report(&old).unwrap_err().contains("schema 2"));
    // A line of schema 3 is a score by other rules (a group's scheme cells, a row named in another letter case, the table of names).
    old["schema"] = json!(3);
    let why = parse_report(&old).unwrap_err();
    assert!(why.contains("schema 3") && why.contains("run the oracle again"), "{why}");
    // A line of schema 5 is a score without `_ELLIPSE_THETA` and `_ELLIPSE_ECC`, about 418,000 cells that are compared in schema 6;
    // added to a schema-6 line it would be a mean of two scores of different sets of cells.
    old["schema"] = json!(5);
    let why = parse_report(&old).unwrap_err();
    assert!(why.contains("schema 5") && why.contains("reads 7") && why.contains("run the oracle again"), "{why}");
    // A line of schema 6 is a score by other rules (`CEILING` and `FLOOR` on the number shown, the small term of the text measures, `Embellishment`
    // 2 with no theme, the narrowed stale-colour rule) and has the reason `stale_theme_property`, which schema 7 does not; added to a schema-7 line the two
    // would be a mean of unlike scores.
    old["schema"] = json!(6);
    let why = parse_report(&old).unwrap_err();
    assert!(why.contains("schema 6") && why.contains("reads 7") && why.contains("run the oracle again"), "{why}");
}

#[test]
fn a_line_that_cannot_be_true_is_refused_with_its_reason() {
    let ok = read_report("aa", Some("a/a"), 5, 10, 2, 1, 4);
    assert!(parse_report(&ok).is_ok());
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut v = ok.clone();
        f(&mut v);
        parse_report(&v).unwrap_err()
    };
    // Matched past compared: the line that once gave a score of 10.0.
    let why = mutate(&|v| {
        v["calc"]["matched"] = json!(100);
        v["calc"]["differed"] = json!(0);
        v["score"] = json!({ "score": 10.0, "matched": 100, "compared": 10 });
    });
    assert!(why.contains("100 cells matched of 10"), "{why}");
    assert!(mutate(&|v| v["calc"]["differed"] = json!(7)).contains("are not the 10 compared"));
    assert!(mutate(&|v| v["calc"]["cells"]["formula"] = json!(99)).contains("99 formula cells and 4 without"));
    assert!(mutate(&|v| v["calc"]["cells"]["in_document"] = json!(99)).contains("cells of the document"));
    assert!(mutate(&|v| v["calc"]["left_out_total"] = json!(9)).contains("reasons add up"));
    assert!(mutate(&|v| v["calc"]["not_calculated"]["total"] = json!(9)).contains("not the 13 formula cells"));
    assert!(mutate(&|v| v["calc"]["left_out"]["made_up"] = json!(0)).contains("no reason a cell is left out"));
    assert!(mutate(&|v| v["score"]["matched"] = json!(6)).contains("the score is not the counts"));
    assert!(mutate(&|v| v["score"]["score"] = json!(0.9)).contains("a score of 0.9"));
    assert!(mutate(&|v| v["schema"] = json!(1)).contains("schema 1"));
    assert!(mutate(&|v| v["calc"]["control"]["passed"] = json!(false)).contains("control that failed"));
    assert!(mutate(&|v| v["calc"]["compared"] = json!("ten")).contains("not a whole number"));
    assert!(mutate(&|v| v["sha256"] = json!("")).contains("not a hash"));
    assert!(mutate(&|v| v["source"] = json!(3)).contains("not text"));
    assert!(mutate(&|v| v["read"] = json!("yes")).contains("`read`"));
    // A refused file with cells and a score is not one.
    let mut refused = refused_report("bb", None);
    refused["score"] = json!({ "score": 1.0, "matched": 1, "compared": 1 });
    assert!(parse_report(&refused).unwrap_err().contains("did not read has a score"));
    let mut refused = refused_report("bb", None);
    refused["refusal"] = Value::Null;
    assert!(parse_report(&refused).unwrap_err().contains("refusal.class"));
}

#[test]
fn the_figures_of_an_aggregate_add_up_to_the_cells() {
    let a = aggregated(
        &[read_report("aa", Some("a/a"), 5, 10, 2, 1, 4), read_report("bb", Some("b/b"), 3, 4, 1, 0, 0), refused_report("cc", Some("b/b"))],
        5,
    );
    assert_eq!(a.compared + a.left_out.values().sum::<u64>() + a.not_calculated, a.formula_cells);
    assert_eq!(a.formula_cells + a.not_formulas, a.cells_in_document);
    let json = aggregate_json(&a);
    assert_eq!(json["cells"], json!({ "in_document": 22, "formula": 18, "not_formulas": 4 }));
    assert_eq!(json["compared"], 14);
    assert_eq!(json["matched"], 8);
    assert!(near(a.pooled, 8.0 / 14.0));
}

/// The cells left out because a text was measured in a stand-in that is not the name's twin are a reason of their own, summed over the files
/// like the others, and shown beside the score and not in it.
#[test]
fn the_cells_measured_in_a_stand_in_are_summed_apart_from_the_score() {
    let with_stand_in = |sha: &str, n: u64| {
        let mut report = read_report(sha, Some("a/a"), 4, 4, 0, 0, 0);
        report["calc"]["left_out"]["text_stand_in"] = json!(n);
        report["calc"]["left_out_total"] = json!(n);
        report["calc"]["cells"]["formula"] = json!(4 + n);
        report["calc"]["cells"]["in_document"] = json!(4 + n);
        report
    };
    let a = aggregated(&[with_stand_in("01", 5), with_stand_in("02", 7), read_report("03", Some("b/b"), 3, 3, 0, 0, 0)], 10);
    assert_eq!(a.left_out["text_stand_in"], 12);
    let json = aggregate_json(&a);
    assert_eq!(json["left_out"]["text_stand_in"], 12);
    assert_eq!(json["left_out_total"], 12);
    // The score is of the cells compared: the stand-in cells are in neither the matched nor the compared.
    assert_eq!((a.compared, a.matched), (11, 11));
}
