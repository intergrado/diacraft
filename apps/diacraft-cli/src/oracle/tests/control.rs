//! The salted control, and the calculators it must catch: one that hands back the saved values (the echo), one that finds a difference
//! everywhere, one that does not calculate, and the evaluator itself.

use std::sync::Arc;

use diacraft_calc::{Calculator, CellAddress, CellEval, CellKind, Evaluator, Native, Value};
use diacraft_vsdx::Limits;
use serde_json::Value as Json;

use super::super::control::{self, Control, package, run as run_control};
use super::super::file::{examine, report};
use super::{native_control, one_shape};

/// A calculator that hands back what the file saved, for every formula cell: the instrument the control exists to catch. It agrees with
/// everything, in the value and in the error, and so scores 1.0 on any file.
struct Echo;

impl Calculator for Echo {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            if let Some(saved) = e.saved.clone() {
                e.raw = saved.clone();
                e.view = saved;
            }
            e.error = e.saved_error;
            e.unsupported = None;
        }
        Arc::new(e)
    }
}

/// A calculator that gives a number no formula gives, for every cell: it differs from everything, a comparer that cannot agree.
struct Wrong;

impl Calculator for Wrong {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            e.raw = Value::num(-123_456.5);
            e.view = e.raw.clone();
            e.error = None;
            e.unsupported = None;
        }
        Arc::new(e)
    }
}

/// A calculator that has no function: every formula cell is unsupported, and the saved value stands.
struct Nothing;

impl Calculator for Nothing {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            e.unsupported = Some(Arc::from("NOTHING"));
        }
        Arc::new(e)
    }
}

/// A calculator that works every cell out, but the first thing it is asked: it gives that one the saved value.
struct EchoesOne {
    asked: usize,
}

impl Calculator for EchoesOne {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        self.asked += 1;
        if self.asked == 1 { Echo.calculate(ev, at) } else { ev.evaluate_cell(at) }
    }
}

#[test]
fn the_evaluator_passes_its_own_control() {
    let c = native_control();
    assert!(c.passed(), "{}", c.words());
    assert!(c.salted >= 15 && c.honest >= 5, "{c:?}");
    assert_eq!((c.caught, c.honest_agreed, c.not_compared), (c.salted, c.honest, 0));
    assert!(c.missed.is_empty() && c.wrong.is_empty() && c.unbuilt.is_none());
    assert!(c.words().starts_with("control passed: "), "{}", c.words());
}

#[test]
fn the_control_is_a_package_that_reads_and_holds_every_cell_it_says() {
    let bytes = package().unwrap();
    let examined = examine(&bytes, &Limits::default(), &mut Native);
    let read = examined.outcome.expect("the control reads");
    assert!(examined.read_report.warnings.is_empty(), "{:?}", examined.read_report.warnings);
    // Every formula cell is compared, and each salted one differs in the report's own list.
    let r = &read.recalc;
    assert_eq!(r.cells, r.scored(), "{r:?}");
    assert_eq!((r.agreed, r.differed.len() as u64 + r.differed_unlisted), (native_control().honest as u64, native_control().salted as u64));
}

#[test]
fn an_echo_calculator_agrees_with_every_salted_cell_and_the_control_catches_it() {
    // The instrument the narrow check found: a calculator echoing the saved values scored agreed=104, differed=0.
    let control = run_control(&mut Echo);
    assert!(!control.passed(), "{}", control.words());
    assert_eq!(control.caught, 0, "the echo agrees with every salted cell: {control:?}");
    assert_eq!(control.missed.len(), control.salted);
    assert_eq!(control.honest_agreed, control.honest, "and with every honest one");
    assert!(control.words().starts_with("control FAILED: 0 of "), "{}", control.words());
    assert!(control.words().contains("s01_document_sheet"), "{}", control.words());
}

#[test]
fn a_file_scored_by_an_echo_calculator_is_flagged_and_has_no_score() {
    // A file that the evaluator would score at 1/3, scored by the echo: it agrees with all three, and the report must not present that.
    let cells = r#"<Cell N="Width" V="4" F="1+1"/><Cell N="Height" V="9" F="2+2"/><Cell N="PinX" V="2" F="1+1"/>"#;
    let bytes = one_shape(cells);
    let honest = report(&bytes, &Limits::default(), None, &mut Native, &native_control());
    assert_eq!((honest.score(), honest.control_passed), (Some(1.0 / 3.0), true));
    let control = run_control(&mut Echo);
    let echoed = report(&bytes, &Limits::default(), None, &mut Echo, &control);
    assert_eq!(echoed.calc.as_ref().map(|s| (s.agreed, s.differed)), Some((3, 0)), "the echo agrees with all three");
    assert!(!echoed.control_passed);
    assert_eq!(echoed.score(), None, "an echo does not score well: it has no score");
    let json = echoed.to_json();
    assert_eq!(json["score"], Json::Null);
    assert_eq!(json["score_withheld"], "control_failed");
    assert_eq!(json["calc"]["control"]["passed"], false);
    assert_eq!(json["calc"]["control"]["caught"], 0);
    assert!(json["calc"]["control"]["missed"].as_array().unwrap().len() >= 15);
    // The honest report has no `score_withheld`.
    assert!(honest.to_json().get("score_withheld").is_none());
}

#[test]
fn an_echo_on_a_real_file_fails_the_real_salt_even_where_the_control_is_taken_to_have_passed() {
    // The control is one synthetic document. The report of the first recalculation score had 17 of 17 control cells caught while an echo agreed
    // with all 15.3 million real cells: so a file is salted too. Here the control is the evaluator's own, passed to the echo's report,
    // which is what a calculator that cheats on real files and not on the control would look like.
    let cells =
        r#"<Cell N="Width" V="4" F="1+3"/><Cell N="Height" V="9" F="2+2"/><Cell N="PinX" V="2" F="1+1"/><Cell N="PinY" V="x" F="&quot;x&quot;"/>"#;
    let bytes = one_shape(cells);
    let honest = report(&bytes, &Limits::default(), None, &mut Native, &native_control());
    assert!(honest.salt_passed() && honest.control_passed);
    let json = honest.to_json();
    let salt = &json["calc"]["real_salt"];
    assert_eq!(salt["passed"], true);
    assert!(salt["salted"].as_u64().unwrap() >= 1, "{salt}");
    assert_eq!(salt["caught"], salt["salted"]);
    assert_eq!((&salt["missed"], &salt["by_allowance"]), (&Json::from(0), &Json::from(0)));
    assert!(json.get("score_withheld").is_none());
    assert!(honest.score().is_some());

    let echoed = report(&bytes, &Limits::default(), None, &mut Echo, &native_control());
    assert!(echoed.control_passed, "the control it was given passed");
    assert!(!echoed.salt_passed());
    assert_eq!(echoed.score(), None, "an echo has no score");
    let json = echoed.to_json();
    assert_eq!(json["score"], Json::Null);
    assert_eq!(json["score_withheld"], "real_salt_failed");
    let salt = &json["calc"]["real_salt"];
    assert_eq!(salt["passed"], false);
    assert_eq!(salt["caught"], 0);
    assert_eq!(salt["missed"], salt["salted"]);
    // The misses are named by the spec's names and fixed words, with no formula.
    let misses = salt["misses"].as_array().unwrap();
    assert!(!misses.is_empty() && misses.len() <= 20, "{salt}");
    assert!(misses[0]["kind"].is_string() && misses[0]["sheet"]["where"].is_string(), "{salt}");
    assert!(!json.to_string().contains("1+3"), "no formula crosses");
    // The words say so, and the run ends loudly.
    assert!(super::super::words(&echoed).iter().any(|l| l.contains("real-file salt") && l.contains("FAILED")));
    assert!(super::super::words(&echoed).iter().any(|l| l.contains("withheld: the real-file salt failed")));
    let ends = super::super::verdict(&native_control(), 1, 0);
    assert!(matches!(&ends, Err(crate::Failure::Message(m)) if m.contains("real-file salt")));
    assert!(super::super::verdict(&native_control(), 0, 0).is_ok());
    // A control that failed still comes first.
    let failed = run_control(&mut Echo);
    assert!(matches!(super::super::verdict(&failed, 0, 0), Err(crate::Failure::Message(m)) if m.contains("salted control failed")));
}

#[test]
fn a_calculator_that_differs_from_everything_is_caught_by_the_honest_cells() {
    let control = run_control(&mut Wrong);
    assert!(!control.passed(), "{}", control.words());
    assert_eq!(control.caught, control.salted, "every salted cell differs, as it should");
    assert_eq!(control.honest_agreed, 0);
    assert_eq!(control.wrong.len(), control.honest);
}

#[test]
fn a_calculator_that_calculates_nothing_is_caught_because_the_control_cells_are_not_compared() {
    let control = run_control(&mut Nothing);
    assert!(!control.passed(), "{}", control.words());
    assert!(control.not_compared > 0, "{control:?}");
    assert_eq!(control.caught, 0);
}

#[test]
fn a_calculator_that_echoes_a_single_cell_is_caught_by_that_cell() {
    let control = run_control(&mut EchoesOne { asked: 0 });
    assert!(!control.passed(), "{}", control.words());
    assert_eq!(control.missed.len(), 1, "exactly the cell it echoed: {control:?}");
    assert_eq!(control.caught + 1, control.salted);
}

#[test]
fn every_probe_has_its_own_id_and_the_salted_ones_cover_every_kind_of_sheet_and_comparison() {
    let c = native_control();
    let json = c.json();
    assert_eq!(json["passed"], true);
    assert_eq!(json["missed"].as_array().unwrap().len(), 0);
    // The probes' ids are unique words (they name the misses in a report), and the control's JSON has no formula or text of the file.
    let wrong = run_control(&mut Echo);
    let missed = wrong.missed.clone();
    let mut unique = missed.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), missed.len(), "ids repeat: {missed:?}");
    for kind in [
        "document_sheet",
        "style_sheet",
        "page_sheet",
        "master_sheet",
        "master_shape",
        "nested_shape_text",
        "colour",
        "unlike_kinds",
        "section_ix_and_row",
        "error_unexpected",
        "error_missing",
        "error_differs",
        "error_beside_themed",
        "error_beside_empty",
        "inherited",
        "inherited_error_without_value",
    ] {
        assert!(missed.iter().any(|id| id.ends_with(kind)), "no salted cell for {kind}: {missed:?}");
    }
}

#[test]
fn a_control_that_cannot_be_built_or_read_fails_the_run_and_says_why() {
    let c = Control {
        salted: 3,
        caught: 0,
        honest: 1,
        honest_agreed: 0,
        missed: Vec::new(),
        wrong: Vec::new(),
        not_compared: 0,
        unbuilt: Some("the control does not read: zip_corrupt".to_owned()),
    };
    assert!(!c.passed());
    assert_eq!(c.words(), "control FAILED: the control does not read: zip_corrupt");
    assert_eq!(c.json()["unbuilt"], "the control does not read: zip_corrupt");
    // And a control that holds nothing passes nothing.
    let empty = Control { salted: 0, caught: 0, honest: 0, honest_agreed: 0, missed: Vec::new(), wrong: Vec::new(), not_compared: 0, unbuilt: None };
    assert!(!empty.passed());
    let _ = control::package;
}

/// schema 7, at the harness: a master shape whose scheme cells follow a page that holds none, a theme part whose `lt1` is `#3a6b9e`, and
/// a `FillForegnd` calling `THEMEVAL("Light")` (`#ffffff` with no theme). The cell is left out when it saves the theme's colour and scored when it saves
/// the other two, and the number it leaves out is `calc.left_out.stale_theme_colour`. An echo agrees with the stale one at the first reading, so it scores
/// it and is not hidden by the rule: the control fails it as it fails any echo.
#[test]
fn a_stale_colour_is_left_out_only_when_the_theme_gives_it_and_an_echo_is_not_hidden_by_the_rule() {
    use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
    use diacraft_testkit::theme::ThemeXml;

    use super::reference_app_props;

    let mut part = ThemeXml::default();
    part.colours[1] = "3a6b9e";
    let bytes_of = |saved: &str| {
        let cells = format!(
            r##"<Cell N="ThemeIndex" V="65534"/><Cell N="ColorSchemeIndex" V="65534"/><Cell N="EffectSchemeIndex" V="65534"/><Cell N="ConnectorSchemeIndex" V="65534"/><Cell N="FontSchemeIndex" V="65534"/><Cell N="FillForegnd" V="{saved}" F="THEMEVAL(&quot;Light&quot;)"/>"##
        );
        VsdxBuilder::new()
            .theme(part.xml().as_bytes())
            .master(MasterSpec::new(1, "M").shapes(&shape(5, &cells)))
            .page(PageSpec::new(1, "P").shapes(&shape(1, "")))
            .app(reference_app_props())
            .build()
    };
    let words = |saved: &str| {
        let calc = report(&bytes_of(saved), &Limits::default(), None, &mut Native, &native_control()).to_json()["calc"].clone();
        (calc["left_out"]["stale_theme_colour"].as_u64(), calc["compared"].as_u64(), calc["matched"].as_u64())
    };
    assert_eq!(words("#3a6b9e"), (Some(1), Some(0), Some(0)), "the theme's colour is the stale kind and is left out");
    assert_eq!(words("#ffffff"), (Some(0), Some(1), Some(1)), "the colour with no theme is a pass");
    assert_eq!(words("#00ff00"), (Some(0), Some(1), Some(0)), "a colour neither gives is a difference");
    // The cells left out are tallied by the spec's names of the functions their formulas call, so that a jump in the count can be traced;
    // a cell that is scored is in no tally.
    let tally = |saved: &str| {
        report(&bytes_of(saved), &Limits::default(), None, &mut Native, &native_control()).to_json()["calc"]["stale_theme_colour_by_function"].clone()
    };
    assert_eq!(tally("#3a6b9e"), serde_json::json!({ "THEMEVAL": 1 }));
    assert_eq!(tally("#ffffff"), serde_json::json!({}));
    let control = run_control(&mut Echo);
    let echoed = report(&bytes_of("#3a6b9e"), &Limits::default(), None, &mut Echo, &control);
    assert_eq!(echoed.calc.as_ref().map(|s| (s.agreed, s.differed)), Some((1, 0)), "the echo agrees with it at the first reading, and so scores it");
    assert!(!echoed.control_passed);
    assert_eq!(echoed.score(), None, "and the control withholds the score");
}
