//! The score of a file: what was compared, what was left out and why, what could not be calculated, the cells that hold no formula, and the
//! differences listed. The comparing is `diacraft-calc`'s and is tested there; what is tested here is that the harness joins it to the reader
//! and reports it without losing a cell.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use diacraft_calc::Native;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use diacraft_vsdx::Limits;
use serde_json::Value;

use super::super::file::report;
use super::super::names::CELLS;
use super::super::score::{LEFT_OUT_WORDS, MAX_LISTED, MAX_LISTED_PER_KIND};
use super::{native_control, one_page, one_shape, reference_app_props, run};

fn seam_file() -> Vec<u8> {
    let cells = r##"
        <Cell N="Height" V="2"/>
        <Cell N="Width" V="4" F="Height*2"/>
        <Cell N="PinX" V="2" F="Width/2"/>
        <Cell N="PinY" V="9" F="Height+1"/>
        <Cell N="LocPinX" V="0" F="1/0" E="#DIV/0!"/>
        <Cell N="LocPinY" V="2" F="2/1" E="#N/A"/>
        <Cell N="Angle" V="2" F="SQRT(-4)"/>
        <Cell N="FlipX" V="x.vsdx" F="FILENAME()"/>
        <Cell N="FlipY" V="x.vsdxa" F="FlipX&amp;&quot;a&quot;"/>
        <Cell N="TxtWidth" V="1" F="1+"/>
        <Cell N="TxtHeight" V="" F="2"/>
        <Cell N="LinePattern" V="1"/>
        <Cell N="LineWeight" V="7" F="FOO(1)"/>
        <Section N="Actions"><Row N="Row_1"><Cell N="Action" V="0" F="1+1"/></Row></Section>"##;
    one_shape(cells)
}

#[test]
fn every_formula_cell_is_compared_left_out_with_a_reason_or_not_calculated() {
    let r = run(&seam_file());
    let json = r.to_json().clone();
    let calc = &json["calc"];
    // Six are compared: three agree (Width, PinX, the error LocPinX) and three differ (PinY by value, LocPinY because the file saved an error
    // the formula does not give, Angle because the formula fails and the file saved none).
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64(), calc["differed"].as_u64()), (Some(6), Some(3), Some(3)), "{calc}");
    assert_eq!(r.score(), Some(0.5));
    assert_eq!(json["score"]["score"], 0.5);
    assert_eq!(json["score"]["coverage"], 0.5, "six of the twelve formula cells");
    assert_eq!(calc["outcomes"]["value_differs"], 1);
    assert_eq!(calc["outcomes"]["error_missing"], 1);
    assert_eq!(calc["outcomes"]["error_unexpected"], 1);
    let left_out = &calc["left_out"];
    assert_eq!(left_out["reads_clock_or_machine"], 2, "FlipX calls FILENAME, and FlipY reads FlipX");
    assert_eq!(left_out["formula_not_parsed"], 1);
    assert_eq!(left_out["nothing_saved"], 1);
    assert_eq!(left_out["action_cell"], 1);
    assert_eq!(calc["left_out_total"], 5);
    assert_eq!(calc["not_calculated"]["total"], 1, "the evaluator has nothing for FOO");
    assert_eq!(calc["not_calculated"]["unsupported_unlisted"], 1);
    assert!(!json.to_string().contains("FOO"), "a function the spec does not list is counted and not named");
    // Every formula cell is in one of the three.
    assert_eq!(calc["cells"]["formula"], 12);
    assert_eq!(calc["compared"].as_u64().unwrap() + calc["left_out_total"].as_u64().unwrap() + calc["not_calculated"]["total"].as_u64().unwrap(), 12);
    // And the cells with no formula are counted: Height, LinePattern, and the root style's three flags.
    assert_eq!(calc["cells"]["not_formulas"]["value_only"], 5);
    assert_eq!(calc["cells"]["in_document"], 17);
}

#[test]
fn the_cells_the_report_places_are_the_cells_the_names_tally_finds() {
    // Two walks of one document written apart: the tallies of names in this module, and the comparer's. They must find the same cells.
    let cells = r#"<Cell N="Width" V="4" F="Height*2"/><Cell N="Height" V="2"/><Cell N="PinX" V="1" F="Inh"/><Cell N="LocPinX" V="0" F="No Formula"/><Section N="Geometry" IX="0"><Row IX="1" T="MoveTo"><Cell N="X" V="1" F="Width*0.5"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row></Section>"#;
    let json = run(&one_shape(cells)).to_json().clone();
    let tallied: u64 = json["names"]["cells"]
        .as_object()
        .unwrap()
        .values()
        .chain(std::iter::once(&json["names"]["cells_unlisted"]))
        .map(|t| ["formula", "inherited", "no_formula", "value_only"].iter().map(|k| t[*k].as_u64().unwrap()).sum::<u64>())
        .sum();
    assert_eq!(json["calc"]["cells"]["in_document"].as_u64(), Some(tallied), "{}", json["calc"]["cells"]);
}

#[test]
fn a_report_places_every_cell_whatever_the_file_holds() {
    // The accounting holds for the file of every test above and below: compared + left out + not calculated = formula cells, and formula cells
    // + cells with no formula = every cell.
    for bytes in [seam_file(), one_shape(r##"<Cell N="A" V="1" F="NOW()"/><Cell N="B" V="Themed" F="THEMEVAL()"/><Cell N="C" V="1" E="#N/A"/>"##)] {
        let calc = run(&bytes).to_json()["calc"].clone();
        let n = |v: &Value| v.as_u64().unwrap();
        let formula = n(&calc["cells"]["formula"]);
        assert_eq!(n(&calc["compared"]) + n(&calc["left_out_total"]) + n(&calc["not_calculated"]["total"]), formula, "{calc}");
        assert_eq!(formula + n(&calc["cells"]["not_formulas"]["total"]), n(&calc["cells"]["in_document"]), "{calc}");
        let reasons: u64 = calc["left_out"].as_object().unwrap().values().map(n).sum();
        assert_eq!(reasons, n(&calc["left_out_total"]));
        let words: BTreeSet<&str> = calc["left_out"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(words, LEFT_OUT_WORDS.iter().copied().collect::<BTreeSet<_>>(), "every reason is in the report, with its count (zero too)");
    }
}

#[test]
fn a_cell_reading_a_themed_operand_has_its_own_reason_and_is_not_an_unlisted_function() {
    // 97,954 of the 97,961 cells the first recalculation score called `unsupported_unlisted` read an operand saved as `Themed`. `Width`
    // holds the marker and no error (`themed_no_error`); `Height` and `PinX` read it (`themed_operand`); `Angle` calls a function nobody lists.
    let cells = r##"<Cell N="Width" V="Themed" F="THEMEVAL()"/><Cell N="Height" V="4" F="Width*2"/><Cell N="PinX" V="2" F="Width"/><Cell N="Angle" V="1" F="FOO(1)"/><Cell N="LocPinX" V="3" F="1+2"/>"##;
    let r = run(&one_shape(cells));
    let calc = &r.to_json()["calc"];
    assert_eq!(calc["left_out"]["themed_no_error"], 1, "{calc}");
    assert_eq!(calc["left_out"]["themed_operand"], 2, "{calc}");
    assert_eq!(calc["left_out"]["date_past_9999_operand"], 0);
    assert_eq!(calc["not_calculated"]["unsupported_unlisted"], 1, "only the function nobody lists: {calc}");
    assert_eq!(calc["not_calculated"]["total"], 1);
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64()), (Some(1), Some(1)));
    assert_eq!(calc["left_out_total"], 3);
    // The words are among the reasons the aggregate reads.
    assert!(LEFT_OUT_WORDS.contains(&"themed_operand") && LEFT_OUT_WORDS.contains(&"date_past_9999_operand"));
    // Schema 7: a `THEMEPROP` on a sheet that reads no theme is scored, so its reason is gone.
    assert!(!LEFT_OUT_WORDS.contains(&"stale_theme_property"));
}

#[test]
fn themeprop_on_a_sheet_that_reads_no_theme_is_scored() {
    // A project rule: `Embellishment` saved as 2 with all five scheme cells absent is a pass, gives 2
    // there, and a saved 7 is a difference. It was left out under `stale_theme_property`, which scored neither.
    let cells = |saved: &str| format!(r##"<Cell N="Width" V="{saved}" F="THEMEPROP(&quot;Embellishment&quot;)"/><Cell N="Height" V="9" F="2+2"/>"##);
    let calc = run(&one_shape(&cells("2"))).to_json()["calc"].clone();
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64(), calc["differed"].as_u64()), (Some(2), Some(1), Some(1)), "{calc}");
    assert_eq!(calc["left_out_total"], 0, "{calc}");
    assert!(calc["left_out"].get("stale_theme_property").is_none(), "the reason is gone from the report: {calc}");
    let calc = run(&one_shape(&cells("7"))).to_json()["calc"].clone();
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64(), calc["differed"].as_u64()), (Some(2), Some(0), Some(2)), "{calc}");
    assert_eq!(calc["left_out"]["stale_theme_colour"], 0);
}

#[test]
fn a_font_number_a_reached_ref_and_a_theme_with_no_shadow_have_reasons_of_their_own() {
    // The project rules of a font number is the machine's, the no-shadow branch of a theme and (a formula
    // that can only fail): the file saves a face name where the formula gives 17, and a value where the formula reaches `REF()`.
    let cells = r##"<Cell N="Width" V="2" F="REF()"/><Cell N="Height" V="4" F="2+2"/><Section N="Character"><Row IX="0"><Cell N="Font" V="Arial" F="THEMEVAL(&quot;LatinFont&quot;,17)"/></Row></Section>"##;
    let r = run(&one_shape(cells));
    let calc = &r.to_json()["calc"];
    assert_eq!(calc["left_out"]["machine_font_number"], 1, "{calc}");
    assert_eq!(calc["left_out"]["formula_must_fail"], 1, "{calc}");
    assert_eq!(calc["left_out"]["theme_no_shadow"], 0, "{calc}");
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64()), (Some(1), Some(1)), "only Height is scored, and agrees: {calc}");
    assert_eq!(calc["left_out_total"], 2);
    for word in ["machine_font_number", "theme_no_shadow", "formula_must_fail"] {
        assert!(LEFT_OUT_WORDS.contains(&word), "{word}");
    }
    // And one added: the cells measured in a stand-in face.
    assert!(LEFT_OUT_WORDS.contains(&"text_stand_in"));
    assert_eq!(LEFT_OUT_WORDS.len(), 16);
}

#[test]
fn text_that_a_machine_bound_field_shows_leaves_out_the_cells_that_read_the_text() {
    // A shape's field calls FILENAME, its TxtHeight reads the text and its TxtLocPinY reads TxtHeight: all three are left
    // out, whatever the evaluator can or cannot do with `TEXTHEIGHT`.
    let cells = r#"<Cell N="TxtHeight" V="1" F="TEXTHEIGHT(TheText,TxtWidth)"/><Cell N="TxtLocPinY" V="0.5" F="TxtHeight*0.5"/><Cell N="Width" V="2" F="1+1"/><Section N="Field"><Row IX="0"><Cell N="Value" V="x.vsdx" F="FILENAME()"/></Row></Section><Text><fld IX="0">x.vsdx</fld></Text>"#;
    let calc = run(&one_shape(cells)).to_json()["calc"].clone();
    assert_eq!(calc["left_out"]["reads_clock_or_machine"], 3, "the field, the reader of the text, and the reader of that");
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64()), (Some(1), Some(1)), "only Width");
    assert_eq!(calc["not_calculated"]["total"], 0);
}

#[test]
fn a_cell_that_reads_the_machine_in_another_sheet_does_not_leave_out_a_cell_that_shares_its_name() {
    // The old harness left cells out by their name alone. The comparer follows addresses: a shape that names FILENAME in `A` does not leave
    // out the cell `A` of another shape, nor the cells that read that one.
    let a = shape(1, r#"<Cell N="A" V="x" F="FILENAME()"/>"#);
    let b = shape(2, r#"<Cell N="B" V="1" F="A+1"/><Cell N="A" V="1" F="1+0"/>"#);
    let calc = run(&one_page(&(a + &b))).to_json()["calc"].clone();
    assert_eq!(calc["left_out"]["reads_clock_or_machine"], 1, "{calc}");
    assert_eq!(calc["compared"], 2, "B and the A of the other shape: {calc}");
}

#[test]
fn a_chain_of_cells_that_each_read_the_one_after_is_left_out_to_its_end() {
    // M calls FILENAME, L reads M, K reads L, J reads K, I reads J: each is left out, in whatever order the cells are met.
    let cells = r#"<Cell N="M" V="x" F="FILENAME()"/><Cell N="L" V="1" F="M&amp;&quot;&quot;"/><Cell N="K" V="1" F="L&amp;&quot;&quot;"/><Cell N="J" V="1" F="K&amp;&quot;&quot;"/><Cell N="I" V="1" F="J&amp;&quot;&quot;"/><Cell N="Width" V="2" F="1+1"/>"#;
    let calc = run(&one_shape(cells)).to_json()["calc"].clone();
    assert_eq!(calc["left_out"]["reads_clock_or_machine"], 5);
    assert_eq!(calc["compared"], 1, "only Width");
}

#[test]
fn an_inherited_cell_is_compared_where_it_has_a_value_and_a_chain_to_a_formula() {
    let master = MasterSpec::new(2, "M").shapes(&shape(5, r#"<Cell N="Width" V="2"/><Cell N="LocPinX" V="1" F="Width*0.5"/>"#));
    let inst = shape_with(10, r#" Master="2""#, r#"<Cell N="Width" V="6"/><Cell N="LocPinX" V="3" F="Inh"/><Cell N="LocPinY" F="Inh"/>"#);
    let bytes = VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&inst)).app(reference_app_props()).build();
    let calc = run(&bytes).to_json()["calc"].clone();
    // The master's LocPinX agrees (1 = 2 * 0.5); the instance's inherited one is worked out on the instance (6 * 0.5 = 3) and agrees. The
    // inherited cell with no value of its own reads the master's saved one (0, none here), and is placed too.
    assert_eq!(calc["matched"], 2, "{calc}");
    assert_eq!(calc["compared"].as_u64().unwrap() + calc["left_out_total"].as_u64().unwrap() + calc["not_calculated"]["total"].as_u64().unwrap(), 3);
}

#[test]
fn a_saved_error_is_compared_whatever_the_value_beside_it_is() {
    // The review's I6, end to end: an `E` beside `Themed`, an empty `V` and no `V` at all is still a word the file said.
    let cells = r##"<Cell N="A" V="Themed" F="1+1" E="#N/A"/><Cell N="B" V="" F="1+1" E="#N/A"/><Cell N="C" F="1+1" E="#N/A"/><Cell N="D" V="Themed" F="1/0" E="#DIV/0!"/>"##;
    let calc = run(&one_shape(cells)).to_json()["calc"].clone();
    assert_eq!((calc["compared"].as_u64(), calc["matched"].as_u64()), (Some(4), Some(1)), "{calc}");
    assert_eq!(calc["outcomes"]["error_missing"], 3);
    assert_eq!(calc["left_out_total"], 0);
}

#[test]
fn a_cell_with_an_error_and_no_formula_is_counted_as_one_the_score_cannot_check() {
    // `E` and `V` with no `F` in 409 files; `No Formula` with an `E`.
    let cells = r##"<Cell N="A" V="1" E="#N/A"/><Cell N="B" V="1" F="No Formula" E="#N/A"/><Cell N="C" V="1"/>"##;
    let calc = run(&one_shape(cells)).to_json()["calc"].clone();
    let n = &calc["cells"]["not_formulas"];
    assert_eq!((n["value_only"].as_u64(), n["no_formula"].as_u64(), n["with_saved_error"].as_u64()), (Some(2 + 3), Some(1), Some(2)), "{calc}");
    assert_eq!(calc["cells"]["formula"], 0);
}

#[test]
fn without_a_formula_that_agrees_nothing_is_scored() {
    let r = run(&one_shape(r#"<Cell N="A" V="1"/>"#));
    assert_eq!(r.score(), None);
    assert_eq!(r.to_json()["score"], Value::Null);
    assert_eq!(r.to_json()["calc"]["compared"], 0);
}

// ---------------------------------------------------------------------------------------------------------------- the differences

fn json_of_differences(cells: &str) -> Vec<Value> {
    let json = run(&one_shape(cells)).to_json().clone();
    json["calc"]["differences"].as_array().unwrap().clone()
}

#[test]
fn a_difference_names_its_sheet_by_ids_and_its_cell_by_section_row_and_the_specs_name() {
    // The review's I7: the sheet's id, the shape's own `ID`, the section's `IX`, the row, the unit and whether the cell inherited.
    let nested = "<Shape ID=\"11\" Type=\"Group\"><Shapes><Shape ID=\"12\"><Section N=\"Geometry\" IX=\"1\"><Row IX=\"2\" T=\"LineTo\"><Cell N=\"X\" V=\"9\" F=\"SQRT(4)\" U=\"IN\"/></Row></Section><Section N=\"User\"><Row N=\"SecretRow\"><Cell N=\"Value\" V=\"7\" F=\"1+1\"/></Row></Section></Shape></Shapes></Shape>".to_owned();
    let master = MasterSpec::new(3, "M").shapes(&shape(5, r#"<Cell N="Width" V="2" F="1+1"/>"#));
    let inst = shape_with(20, r#" Master="3""#, r#"<Cell N="Width" V="7" F="Inh"/>"#);
    let bytes = VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&(nested + &inst))).app(reference_app_props()).build();
    let json = run(&bytes).to_json().clone();
    let diffs = json["calc"]["differences"].as_array().unwrap();
    assert_eq!(diffs.len(), 3, "{diffs:?}");
    let geometry = diffs.iter().find(|d| d["section"] == "Geometry").unwrap();
    assert_eq!(geometry["sheet"], serde_json::json!({ "where": "shape", "part": "page", "id": 1, "shape": 12, "depth": 2 }));
    assert_eq!((geometry["section_ix"].as_u64(), geometry["cell"].as_str()), (Some(1), Some("X")));
    assert_eq!(geometry["row"], serde_json::json!({ "ix": 2 }));
    assert_eq!(geometry["inherited"], false);
    assert_eq!((geometry["expected"]["v"].as_str(), geometry["actual"]["v"].as_str()), (Some("9 [IN]"), Some("2 [IN]")));
    assert_eq!(geometry["outcome"], "value_differs");
    assert_eq!(geometry["functions"], serde_json::json!(["SQRT"]));
    // An operator is not a function of the comparer's lists: `1+1` names none.
    assert!(diffs.iter().any(|d| d["section"] == "User" && d["functions"] == serde_json::json!([])));
    let user = diffs.iter().find(|d| d["section"] == "User").unwrap();
    assert_eq!(user["row"], serde_json::json!({ "named": true }), "a row's name is content");
    assert!(!json.to_string().contains("SecretRow"));
    let inherited = diffs.iter().find(|d| d["inherited"] == true).unwrap();
    assert_eq!(inherited["sheet"], serde_json::json!({ "where": "shape", "part": "page", "id": 1, "shape": 20, "depth": 1 }));
    assert_eq!(inherited["cell"], "Width");
    assert_eq!(inherited["row"], Value::Null);
}

#[test]
fn a_text_value_in_a_difference_is_its_length_and_never_the_text() {
    let diffs = json_of_differences(
        r#"<Section N="User"><Row N="a"><Cell N="Value" V="SECRETSAVED" F="&quot;ab&quot;&amp;&quot;cd&quot;" U="STR"/></Row></Section>"#,
    );
    assert_eq!(diffs.len(), 1);
    assert_eq!((diffs[0]["expected"]["v"].as_str(), diffs[0]["actual"]["v"].as_str()), (Some("<text:11>"), Some("<text:4>")));
    assert!(!diffs[0].to_string().contains("SECRET"));
}

#[test]
fn a_difference_list_is_varied_and_bounded_and_the_count_is_not() {
    // 120 cells of one kind, and four others: the list holds two of the first kind, so the other four are on it, and everything is counted.
    let rows: String = (0..120).map(|i| format!("<Row N=\"r{i}\"><Cell N=\"Value\" V=\"2\" F=\"1+0\"/></Row>")).collect();
    let same = format!("<Section N=\"User\">{rows}</Section>");
    let others = r#"<Cell N="Width" V="2" F="1+0"/><Cell N="Height" V="2" F="1+0"/><Cell N="PinX" V="2" F="SQRT(1)"/><Cell N="PinY" V="2" F="1*1"/>"#;
    let json = run(&one_shape(&format!("{same}{others}"))).to_json().clone();
    let calc = &json["calc"];
    assert_eq!(calc["differed"], 124);
    let diffs = calc["differences"].as_array().unwrap();
    let users = diffs.iter().filter(|d| d["section"] == "User").count();
    assert_eq!(users, MAX_LISTED_PER_KIND, "one repeated cell does not fill the list");
    for name in ["Width", "Height", "PinX", "PinY"] {
        assert!(diffs.iter().any(|d| d["cell"] == name), "{name} is not on the list");
    }
    assert_eq!(calc["differences_listed"].as_u64(), Some(diffs.len() as u64));
    // The tallies by kind of cell say where all 124 are.
    assert_eq!(calc["by_cell"]["User.Value"]["differed"], 120);
    assert_eq!(calc["by_cell"]["Width"]["differed"], 1);
}

#[test]
fn the_list_stops_at_its_cap_when_the_kinds_are_many() {
    let names: Vec<&str> = CELLS.iter().copied().filter(|n| !n.contains('<')).take(120).collect();
    let cells: String = names.iter().map(|n| format!("<Cell N=\"{n}\" V=\"2\" F=\"1+0\"/>")).collect();
    let calc = run(&one_shape(&cells)).to_json()["calc"].clone();
    let differed = calc["differed"].as_u64().unwrap();
    assert!(differed > MAX_LISTED as u64, "{differed}");
    assert_eq!(calc["differences"].as_array().unwrap().len(), MAX_LISTED);
}

#[test]
fn the_score_of_each_function_and_each_kind_of_cell_is_reported_by_the_specs_names() {
    let cells = r#"<Cell N="Width" V="2" F="SQRT(4)"/><Cell N="Height" V="3" F="SQRT(4)"/><Cell N="PinX" V="2" F="MYFUNC(1)+0"/><Cell N="PinY" V="1" F="IF(1,1,OTHERFUNC())"/>"#;
    let calc = run(&one_shape(cells)).to_json()["calc"].clone();
    assert_eq!(calc["by_function"]["SQRT"], serde_json::json!({ "agreed": 1, "differed": 1 }));
    assert_eq!(calc["by_cell"]["Width"], serde_json::json!({ "agreed": 1, "differed": 0 }));
    assert_eq!(calc["by_cell"]["Height"], serde_json::json!({ "agreed": 0, "differed": 1 }));
    // PinY is scored: the branch with the unknown function is not taken. Its function is counted and not named.
    assert_eq!(calc["by_function_unlisted"]["agreed"], 1);
    assert!(!calc.to_string().contains("OTHERFUNC") && !calc.to_string().contains("MYFUNC"));
}

// ---------------------------------------------------------------------------------------------------------------------- the cost

#[test]
fn the_report_of_a_large_file_costs_a_pass_over_it_and_no_more() {
    // 500,000 cells that read FILENAME: each is left out as a reader of the machine, none is calculated. The comparer's own budget bounds the rest.
    let cells: String = (0..1_000).map(|i| format!("<Cell N=\"C{i}\" V=\"{i}\" F=\"Width*{i}+FILENAME()\"/>")).collect();
    let shapes: String = (0..500).map(|i| shape(i, &cells)).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build();
    let started = Instant::now();
    let r = report(&bytes, &Limits::default(), None, &mut Native, &native_control());
    let took = started.elapsed();
    eprintln!("[timing] oracle report over 500,000 cells that read FILENAME: {took:?}");
    // A hang guard and no measure of cost: the pass over 500,000 cells takes a few seconds, ten at a load of 50, and the count below (every cell
    // placed, none calculated) is what the test asserts. The guard stays under the 120 s at which nextest ends a test.
    assert!(took < Duration::from_secs(90), "{took:?}");
    assert!(r.refusal.is_none());
    let calc = &r.to_json()["calc"];
    let counted = calc["compared"].as_u64().unwrap() + calc["left_out_total"].as_u64().unwrap() + calc["not_calculated"]["total"].as_u64().unwrap();
    assert_eq!(counted, 500_000, "every one of them is placed, the budget's unreached included");
}

#[test]
fn a_thousand_pages_that_name_one_part_are_reported_in_time() {
    // 1,200 shapes a use: 9.6 million items charged for the 1,000 uses, under the 12 million of the reader's items budget (1,500 were 12.004 million, which the
    // budget refuses lowered it from 20 million).
    let bytes = super::file::pages_sharing_one_part(1_000, 1_200);
    let started = Instant::now();
    let r = report(&bytes, &Limits::default(), None, &mut Native, &native_control());
    let took = started.elapsed();
    eprintln!("[timing] oracle report over 1,000 pages that share a part of 1,200 shapes: {took:?}");
    // A hang guard and no measure of cost (the report takes a second or two at load 50): the report is a read, with no refusal.
    assert!(took < Duration::from_secs(90), "{took:?}");
    assert!(r.refusal.is_none(), "{:?}", r.refusal);
}
