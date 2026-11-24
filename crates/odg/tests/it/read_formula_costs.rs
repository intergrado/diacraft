//! Of the independent review of 7344b8c: what a formula cell costs, the expressions a path parameter keeps, the
//! handles that are not editable, the rows a section may hold, the ids of glue points, and the nesting a translation may reach. Each test
//! fails on 7344b8c.

use std::time::{Duration, Instant};

use diacraft_doc::{Document, SectionKey};
use diacraft_odg::{Limits, ReadReport, read_partial};

use crate::drawing::{Drawing, formula, num, only, page, row_cell, shape};

/// A custom shape of 4 cm by 2 cm whose enhanced geometry is `attrs` (with its `>`) and `inner`.
fn custom(attrs: &str, inner: &str) -> String {
    format!(
        r#"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" {attrs}>{inner}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

/// A balanced sum of `2^depth` ones: `(((1+1)+(1+1))+...)`. Depth 15 is the review's 131,069 bytes.
pub fn balanced(depth: u32) -> String {
    let mut s = "1".to_owned();
    for _ in 0..depth {
        s = format!("({s}+{s})");
    }
    s
}

fn equation(formula: &str) -> String {
    format!(r#"<draw:equation draw:name="f0" draw:formula="{formula}"/>"#)
}

fn read_with(shapes: &str, limits: &Limits) -> (Result<Document, diacraft_odg::OdgError>, ReadReport, Duration) {
    let bytes = Drawing::a4().page(shapes).bytes();
    let started = Instant::now();
    let (result, report) = read_partial(&bytes, limits);
    (result, report, started.elapsed())
}

fn read_ok(shapes: &str) -> (Document, ReadReport) {
    Drawing::a4().page(shapes).read()
}

/// The longest formula text in `Geometry` section `geo` of `s`.
fn longest_formula(s: &diacraft_doc::Shape, geo: u32) -> usize {
    let section = s.sheet.section(&SectionKey::new("Geometry", Some(geo))).expect("the section");
    section.rows.iter().flat_map(|r| r.cells.values()).filter_map(|c| c.formula.text()).map(str::len).max().unwrap_or(0)
}

// ----: the charge of a formula cell and the expression a parameter keeps

#[test]
fn a_path_parameter_past_the_formula_limit_is_a_value_and_a_close_does_not_parse_it_again() {
    // The review's input A: a 131,069-byte sum as the first point, then closes that draw a line back to it.
    let sum = balanced(15);
    assert_eq!(sum.len(), 131_069);
    let path = format!("M {sum} 0 {}", "L 0 0 Z ".repeat(100));
    let (result, report, took) = read_with(&custom(&format!(r#"draw:enhanced-path="{path}">"#), ""), &Limits::default());
    let doc = result.expect("reads");
    assert!(took < Duration::from_secs(30), "{took:?}");
    let s = only(&doc);
    // The point is a value: no cell of the path holds its expression, and so a close holds no copy of it.
    assert!(longest_formula(s, 0) < 600, "a cell holds a formula of {} bytes", longest_formula(s, 0));
    assert!(row_cell(s, "Geometry", Some(0), 1, "X").is_some_and(|c| c.value.is_some()));
    crate::drawing::close(num(row_cell(s, "Geometry", Some(0), 1, "X")), 4.0 / 2.54 * 32768.0 / 21600.0);
    // And the hundred closes cost what a hundred lines cost, not a hundred copies of a formula of 131 KB.
    assert!(report.work.items < 100_000, "{} items", report.work.items);
}

#[test]
fn a_parameter_within_the_formula_limit_keeps_its_expression() {
    // 2^5 ones is 93 bytes: kept as a formula that names the user row's value, as before.
    let sum = balanced(5);
    let (doc, _) = read_ok(&custom(&format!(r#"draw:enhanced-path="M {sum} 0 L 0 0">"#), ""));
    let s = only(&doc);
    let text = formula(row_cell(s, "Geometry", Some(0), 1, "X")).expect("a formula");
    assert!(text.starts_with("Width*") && text.contains("1+1"), "{text}");
}

#[test]
fn a_parameter_expression_of_the_limit_is_kept_and_one_character_past_it_is_a_value() {
    // 500 characters: 249 terms of `1+` and `11`; 501: the last term of three digits.
    let at = format!("{}11", "1+".repeat(249));
    let past = format!("{}111", "1+".repeat(249));
    assert_eq!((at.len(), past.len()), (500, 501));
    for (text, keeps) in [(&at, true), (&past, false)] {
        let (doc, _) = read_ok(&custom(&format!(r#"draw:enhanced-path="M 0 0 L {text} 0">"#), ""));
        // A value is a share of the frame (`Width*0.0...`); an expression is carried into the cell.
        let carried = formula(row_cell(only(&doc), "Geometry", Some(0), 2, "X")).is_some_and(|f| f.contains("1+1+1"));
        assert_eq!(carried, keeps, "{} characters", text.len());
    }
}

#[test]
fn a_formula_cell_is_charged_by_its_nodes_and_not_by_its_text() {
    // 2^12 ones: 8,191 nodes in 16,381 bytes of text. At the formula crate's 72 to 128 bytes a node that is 1 MB, 16,000 items; the text
    // alone, three times, was 770.
    let sum = balanced(12);
    let (result, report, _) = read_with(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equation(&sum)), &Limits::default());
    result.expect("reads");
    assert!(report.work.items >= 8_191 * 2, "{} items", report.work.items);
}

#[test]
fn many_shapes_each_with_a_long_equation_are_refused_by_the_items_before_their_trees_fill_the_memory() {
    // The review's input B at a size a test can hold: shapes each with one equation of 131 KB. Each tree is charged by its 65,535 nodes.
    let sum = balanced(15);
    let shapes: String = (0..30).map(|_| custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equation(&sum))).collect();
    let (result, report, took) = read_with(&shapes, &Limits::default());
    result.expect("thirty shapes read");
    assert!(report.work.items >= 30 * 65_535 * 2, "{} items", report.work.items);
    assert!(took < Duration::from_secs(60), "{took:?}");
    // The same file is refused at a smaller cap, after its first dozen shapes, and not read to the end.
    let limits = Limits { max_items: 1_800_000, ..Limits::default() };
    let (result, _, _) = read_with(&shapes, &limits);
    assert_eq!(result.err().map(|e| e.class()), Some("too_many_items"));
}

#[test]
fn a_tree_the_documents_budget_of_nodes_cannot_hold_is_not_built_and_the_document_is_past_its_items() {
    // 1,023 nodes against a budget of 1,000 (2,000 items at 64 bytes an item and 128 bytes a node): the tree is not built, and the read does
    // not go on with a cell that holds the text alone.
    let sum = balanced(9);
    let limits = Limits { max_items: 2_000, ..Limits::default() };
    let (result, _, _) = read_with(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equation(&sum)), &limits);
    assert_eq!(result.err().map(|e| e.class()), Some("too_many_items"));
    // A shorter one, 511 nodes, is built (and charged its 1,022 items), and the same file reads at a larger cap.
    let (result, report, _) =
        read_with(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equation(&balanced(8))), &Limits { max_items: 3_000, ..Limits::default() });
    result.expect("reads");
    assert!(report.work.formula_nodes >= 511);
}

#[test]
fn a_shared_formula_text_is_parsed_and_charged_once() {
    // A hundred shapes of one size: their cells hold one parse of each of the reader's own formulas, charged when it is made.
    let shapes: String = (0..100).map(|_| custom(r#"draw:enhanced-path="M 0 0 L 21600 21600">"#, "")).collect();
    let (result, report, _) = read_with(&shapes, &Limits::default());
    result.expect("reads");
    // A few formulas (`Width`, `Height`, `Width*0.5`, ...), parsed once each and not a hundred times.
    assert!(report.work.formula_nodes < 100, "{} nodes", report.work.formula_nodes);
}

#[test]
fn the_nodes_of_a_tree_are_counted() {
    let sum = balanced(12);
    let (result, report, _) = read_with(&custom(r#"draw:enhanced-path="M 0 0 L ?f0 0">"#, &equation(&sum)), &Limits::default());
    result.expect("reads");
    assert!((8_191..8_300).contains(&report.work.formula_nodes), "{} nodes", report.work.formula_nodes);
}

// ----: a handle with ranges is kept and not editable

#[test]
fn odg_shapes_a_handle_with_a_range_is_kept_and_makes_no_row() {
    let handle = |extra: &str| {
        custom(r#"draw:modifiers="1" draw:enhanced-path="M 0 0 L $0 0">"#, &format!(r#"<draw:handle draw:handle-position="$0 top"{extra}/>"#))
    };
    // A plain handle is a row, so that the rest of this test says something.
    let (doc, report) = read_ok(&handle(""));
    assert!(
        crate::drawing::cell_at(&only(&doc).sheet, &diacraft_doc::CellAddr::in_row("Control", None, diacraft_doc::RowKey::name("Row_1"), "X"))
            .is_some()
    );
    assert_eq!(report.kept("handle"), 0);
    for attr in [
        "draw:handle-range-x-minimum",
        "draw:handle-range-x-maximum",
        "draw:handle-range-y-minimum",
        "draw:handle-range-y-maximum",
        "draw:handle-radius-range-minimum",
        "draw:handle-radius-range-maximum",
    ] {
        let (doc, report) = read_ok(&handle(&format!(r#" {attr}="0""#)));
        assert!(only(&doc).sheet.section(&SectionKey::new("Control", None)).is_none(), "{attr}");
        assert_eq!(report.kept("handle"), 1, "{attr}");
    }
}

// ----: Control rows (formula spelling: `Controls.`) are capped as the rows of the other sections are

#[test]
fn more_handles_than_a_section_holds_rows_are_refused_as_too_complex_before_the_rest_are_read() {
    let handles = |n: usize| -> String {
        let one = r#"<draw:handle draw:handle-position="$0 top"/>"#;
        format!(
            r#"<draw:custom-shape svg:width="1cm" svg:height="1cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="1" draw:enhanced-path="M 0 0 L 1 1">{}</draw:enhanced-geometry></draw:custom-shape>"#,
            one.repeat(n)
        )
    };
    let (result, _, took) = read_with(&handles(320_001), &Limits::default());
    assert_eq!(result.err().map(|e| e.class()), Some("xml_too_complex"));
    assert!(took < Duration::from_secs(60), "{took:?}");
    // 320,000 is the section's own limit and reads.
    let (result, _, _) = read_with(&handles(320_000), &Limits::default());
    assert!(result.is_ok());
}

// ----: ids of glue points

#[test]
fn a_glue_point_element_that_repeats_the_id_of_a_listed_point_makes_no_row() {
    // Two listed points are rows 4 and 5; elements with ids 4 and 5 clash with them and an element with id 6 runs on.
    let elements = r#"<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="5" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="6" svg:x="0cm" svg:y="0cm"/>"#;
    let shape = format!(
        r#"<draw:custom-shape svg:width="4cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:glue-points="0 0 21600 21600" draw:enhanced-path="M 0 0 L 1 1"/>{elements}</draw:custom-shape>"#
    );
    let (doc, report) = read_ok(&shape);
    let section = only(&doc).sheet.section(&SectionKey::new("Connection", None)).expect("the connection section");
    let ixs: Vec<Option<u32>> = section.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ixs, [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)], "each row once");
    assert_eq!(report.warned("glue_point_id"), 2);
}

#[test]
fn a_connector_naming_a_glue_point_no_shape_can_hold_is_glued_to_the_whole_shape() {
    // 4294967295 would make `ToPart` saturate; no section holds so many rows, so the end names no point.
    let shapes = concat!(
        r#"<draw:rect xml:id="a" svg:width="1cm" svg:height="1cm"/>"#,
        r#"<draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:end-shape="a" draw:end-glue-point="4294967295"/>"#
    );
    let (doc, report) = read_ok(shapes);
    let connect = page(&doc, 0).connects.get(0).expect("a Connect row");
    assert_eq!((connect.to_part, connect.to_cell.as_deref()), (Some(3), Some("PinX")));
    assert_eq!(report.warned("glue_point_id"), 1);
    // The largest a section holds: 319,999 is a point's row, 320,000 is not.
    for (id, part) in [(319_999u32, Some(100 + 319_999)), (320_000, Some(3))] {
        let shapes = format!(
            r#"<draw:rect xml:id="a" svg:width="1cm" svg:height="1cm"/><draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="1cm" svg:y2="1cm" draw:end-shape="a" draw:end-glue-point="{id}"/>"#
        );
        let (doc, _) = read_ok(&shapes);
        assert_eq!(page(&doc, 0).connects.get(0).map(|c| c.to_part), Some(part), "{id}");
    }
}

#[test]
fn a_glue_point_element_whose_id_no_section_can_hold_makes_no_row() {
    let elements = r#"<draw:glue-point draw:id="4294967295" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="320000" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="319999" svg:x="0cm" svg:y="0cm"/>"#;
    let (doc, report) = read_ok(&format!(r#"<draw:rect svg:width="1cm" svg:height="1cm">{elements}</draw:rect>"#));
    let section = shape(&doc, 0, 0).sheet.section(&SectionKey::new("Connection", None)).expect("the connection section");
    let ixs: Vec<Option<u32>> = section.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ixs, [Some(0), Some(1), Some(2), Some(3), Some(319_999)]);
    assert_eq!(report.warned("glue_point_id"), 2);
}

// ----: a repeated glue id is a warning

#[test]
fn a_glue_point_element_that_repeats_an_earlier_id_gives_a_warning() {
    let elements = r#"<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm"/><draw:glue-point draw:id="7" svg:x="0cm" svg:y="0cm"/><draw:glue-point draw:id="4" svg:x="2cm" svg:y="2cm"/>"#;
    let (doc, report) = read_ok(&format!(r#"<draw:rect svg:width="4cm" svg:height="4cm">{elements}</draw:rect>"#));
    let s = shape(&doc, 0, 0);
    let section = s.sheet.section(&SectionKey::new("Connection", None)).expect("the connection section");
    let ixs: Vec<Option<u32>> = section.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ixs, [Some(0), Some(1), Some(2), Some(3), Some(4), Some(7)], "the first of a repeated id stands");
    assert_eq!(report.warned("glue_point_repeated"), 2);
    // The first one's place (0cm from the centre) and not a later one's.
    crate::drawing::close(num(row_cell(s, "Connection", None, 4, "X")), 4.0 / 2.54 * 0.5);
}

// ----: the nesting of a translation

/// `if(if(...(1,1,1)...,1,1),1,1)` with `n` ifs, each nested in the condition of the one outside it: the translation brackets each
/// condition, which costs a level more than the source's own.
fn ifs_in_conditions(n: usize) -> String {
    let mut s = "1".to_owned();
    for _ in 0..n {
        s = format!("if({s},1,1)");
    }
    s
}

#[test]
fn an_equation_whose_translation_nests_past_the_limit_is_unreadable_and_one_within_it_parses() {
    // 50 ifs in conditions nest 100 levels in the translation (two each), the limit of the formula crate; 51 do not.
    let at_limit = r#"draw:enhanced-path="M 0 0 L ?f0 0">"#;
    let (doc, report) = read_ok(&custom(at_limit, &equation(&ifs_in_conditions(50))));
    let s = only(&doc);
    let eq = crate::drawing::cell_at(&s.sheet, &diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name("ODFEq0"), "Value"))
        .expect("the row");
    let text = eq.formula.text().expect("the translation is the cell's formula");
    assert!(diacraft_formula::parse(text).is_parsed(), "a translation of 50 ifs parses");
    assert!(eq.error.is_none());
    assert_eq!(report.warned("equation_unreadable"), 0);
    let (doc, report) = read_ok(&custom(at_limit, &equation(&ifs_in_conditions(51))));
    let eq = crate::drawing::cell_at(&only(&doc).sheet, &diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name("ODFEq0"), "Value"))
        .expect("the row");
    assert_eq!(eq.error.as_deref(), Some("#NAME?"));
    assert_eq!(report.warned("equation_unreadable"), 1);
}

#[test]
fn a_path_parameter_that_nests_to_the_limit_is_a_formula_that_parses_or_a_value() {
    // A parameter in 100 brackets nests 100 deep, the limit. As an x it goes into its cell as it is (`Width*(...)/21600`), so the cell nests
    // 100 deep and keeps its formula; as a y it goes into `Height*(1-(...)/21600)`, one level more, past the formula crate's limit, and the
    // cell holds the value alone. In 90 brackets both fit.
    let nested = |depth: usize| format!("{}5{}", "(".repeat(depth), ")".repeat(depth));
    let value = 4.0 / 2.54 * 5.0 / 21600.0;
    for depth in [90usize, 100] {
        let (doc, _) = read_ok(&custom(&format!(r#"draw:enhanced-path="M 0 0 L {} 0">"#, nested(depth)), ""));
        let c = row_cell(only(&doc), "Geometry", Some(0), 2, "X");
        let text = formula(c).expect("an x of 100 brackets keeps its formula");
        assert!(diacraft_formula::parse(text).is_parsed(), "{depth}");
        crate::drawing::close(num(c), value);
    }
    let (doc, _) = read_ok(&custom(&format!(r#"draw:enhanced-path="M 0 0 L 0 {}">"#, nested(90)), ""));
    let text = formula(row_cell(only(&doc), "Geometry", Some(0), 2, "Y")).expect("a y of 90 brackets keeps its formula");
    assert!(diacraft_formula::parse(text).is_parsed());
    let (doc, _) = read_ok(&custom(&format!(r#"draw:enhanced-path="M 0 0 L 0 {}">"#, nested(100)), ""));
    let c = row_cell(only(&doc), "Geometry", Some(0), 2, "Y");
    assert_eq!(formula(c), None, "a y of 100 brackets is past the limit in its cell: the value alone");
    crate::drawing::close(num(c), 2.0 / 2.54 * (1.0 - 5.0 / 21600.0));
}
