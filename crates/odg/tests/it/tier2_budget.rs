//! The security review of `write/styles.rs` and `write/own.rs` (MEDIUM): the decodes of the tier 2 that styles carry share one budget for the whole
//! save and a style that many shapes name is decoded once, and the comparison of a shape's sections and the merge of what its style carries with what the
//! model holds do work in proportion to what they hold, with every turn of their loops charged to the save's own count.
//!
//! Each input is a drawing written by hand: rectangles on automatic styles that carry DiaCraft's attributes in their `style:graphic-properties`.

use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{move_by, open};

/// `xml` as the value of an attribute: the quotes escaped.
fn value(xml: &str) -> String {
    xml.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// An automatic graphic style `name` whose graphic properties carry `attrs`.
fn style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#)
}

/// A rectangle named `R<k>` on the style `name`, 2 cm by 1 cm.
fn rect(name: &str, k: usize) -> String {
    format!(
        r#"<draw:rect draw:style-name="{name}" draw:name="R{k}" svg:x="{}cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#,
        1 + k % 10
    )
}

/// The cells a style carries: `cells` user cells.
fn cells_attr(cells: usize) -> String {
    let body: String = (0..cells).map(|k| format!(r#"<Cell N="Note{k}" V="v{k}"/>"#)).collect();
    format!(r#"diacraft:version="1" diacraft:cells="{}""#, value(&format!("<Sheet>{body}</Sheet>")))
}

/// What saving the drawing with every shape of its page moved costs.
fn move_all(drawing: &Drawing) -> WriteWork {
    let mut o = open(drawing);
    let ids: Vec<u32> = o.doc.pages.get(0).map(|p| p.shapes.iter().map(|s| s.id).collect()).unwrap_or_default();
    for id in ids {
        move_by(&mut o.doc, 0, id, 0.25, 0.0);
    }
    let mut work = WriteWork::default();
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("not saved: {e}"));
    work
}

fn linear(what: &str, small: u64, big: u64) {
    // Four times the input is at most four times the work, less or more a few for what a drawing with one shape costs.
    assert!(big <= small.saturating_mul(4).saturating_add(64), "{what}: {small} for n and {big} for 4n");
    assert!(big.saturating_add(64) >= small.saturating_mul(3), "{what}: {small} for n and {big} for 4n: not in proportion");
}

/// One style named by `n` shapes: decoded once, and each shape's use of it charged what the decode took.
#[test]
fn a_style_that_many_shapes_name_is_decoded_once_and_each_use_is_charged() {
    let shared = |n: usize| {
        let shapes: String = (0..n).map(|k| rect("gr1", k)).collect();
        move_all(&Drawing::a4().auto(&style("gr1", &cells_attr(20))).page(&shapes))
    };
    let (small, big) = (shared(50), shared(200));
    assert_eq!(small.tier2_decodes, 1, "50 shapes name one style, which is decoded once");
    assert_eq!(big.tier2_decodes, 1, "200 shapes name one style, which is decoded once");
    assert!(small.tier2_items >= 50, "every use is charged: {}", small.tier2_items);
    linear("items charged for the uses of one style", small.tier2_items, big.tier2_items);
}

/// A style for each shape: each decoded once, under one budget for all of them, and the work in proportion to their number.
#[test]
fn the_decodes_of_many_styles_share_one_budget_and_cost_in_proportion_to_their_number() {
    let distinct = |n: usize| {
        let mut drawing = Drawing::a4();
        let mut shapes = String::new();
        for k in 0..n {
            drawing = drawing.auto(&style(&format!("g{k}"), &cells_attr(20)));
            shapes.push_str(&rect(&format!("g{k}"), k));
        }
        move_all(&drawing.page(&shapes))
    };
    let (small, big) = (distinct(50), distinct(200));
    assert_eq!(small.tier2_decodes, 50);
    assert_eq!(big.tier2_decodes, 200);
    linear("items the decodes of many styles take", small.tier2_items, big.tier2_items);
    linear("steps the merges of many styles take", small.steps_used(), big.steps_used());
}

/// A shape whose style carries `sections` sections of its own, each with a row, and whose model holds all of them.
fn sections_attrs(sections: usize) -> String {
    let mut out = String::from(r#"diacraft:version="1" diacraft:id="9""#);
    for k in 0..sections {
        let xml = value(&format!(r#"<Sheet><Section N="N{k}"><Row N="a"><Cell N="Value" V="{k}"/></Row></Section></Sheet>"#));
        out.push_str(&format!(r#" diacraft:s-N{k}="{xml}""#));
    }
    out
}

/// What saving the drawing of one shape on a style that carries `sections` sections costs, with the shape moved and, when `change` says so, every section
/// of the model changed: the work it took and the sections the model's own lookups scanned.
fn sections_cost(sections: usize, change: bool) -> (WriteWork, u64) {
    let drawing = Drawing::a4().auto(&style("gr1", &sections_attrs(sections))).page(&rect("gr1", 0));
    let mut o = open(&drawing);
    move_by(&mut o.doc, 0, 9, 0.25, 0.0);
    if change {
        let shape = crate::write_core::shape_mut(&mut o.doc, 0, 9);
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        for section in &mut sheet.sections {
            let section = std::sync::Arc::make_mut(section);
            for row in section.rows.iter_mut() {
                let row = std::sync::Arc::make_mut(row);
                if let Some(cell) = row.cells.get_mut("Value") {
                    cell.value = diacraft_doc::Value::from_number(4242.0);
                }
            }
        }
    }
    diacraft_doc::work::reset();
    let mut work = WriteWork::default();
    write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{sections} sections: {e}"));
    (work, diacraft_doc::work::sections_examined())
}

/// The merge of what a style carries with the model's sections looked each section up with a scan of the model's: the scans are counted, and are in proportion to
/// the sections and not to their square.
#[test]
fn the_sections_a_style_carries_are_merged_with_the_models_without_a_scan_for_each() {
    let ((_, small), (_, big)) = (sections_cost(200, false), sections_cost(800, false));
    assert!(
        big <= small.saturating_mul(4).saturating_add(64),
        "{small} sections scanned for 200 sections and {big} for 800: more than in proportion"
    );
}

/// The comparison of two lists of sections and the merge of what a style carries with the model's, for sections that all changed: the steps the save counts are
/// in proportion to the sections (the loops' own turns are charged, and none is a search among the others).
#[test]
fn a_comparison_of_many_changed_sections_costs_steps_in_proportion_to_their_number() {
    let ((small, _), (big, _)) = (sections_cost(250, true), sections_cost(1_000, true));
    linear("steps of comparing and merging changed sections", small.steps_used(), big.steps_used());
    // Every section is looked at by each of the passes: the index of the baseline's, the lookup of each of the model's, the pass that finds what is gone, the
    // index of what the style carries, and the merge: at least five turns for each section, and the count says so.
    assert!(small.steps_used() >= 5 * 250, "{} steps for 250 changed sections", small.steps_used());
    assert!(big.steps_used() >= 5 * 1_000, "{} steps for 1,000 changed sections", big.steps_used());
}

/// Removals the style holds, taken out of the list of what is gone with the cells an edit puts back, in one pass: a style that says many cells are gone and an
/// edit that puts a few of them back is charged for the pass over the list, which a search of it for each cell put back would have run for each one.
#[test]
fn what_an_edit_puts_back_is_taken_out_of_what_a_style_says_is_gone_in_one_pass() {
    let cost = |gone_cells: usize, back: usize| {
        let gone: String = (0..gone_cells).map(|k| format!("c.Gone{k}")).collect::<Vec<_>>().join(" ");
        let attrs = format!(r#"diacraft:version="1" diacraft:id="9" diacraft:gone="{gone}""#);
        let drawing = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", 0));
        let mut o = open(&drawing);
        let shape = crate::write_core::shape_mut(&mut o.doc, 0, 9);
        let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
        for k in 0..back {
            sheet
                .cells
                .insert(diacraft_doc::CellName::new(&format!("Gone{k}")), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("back")));
        }
        let mut work = WriteWork::default();
        write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
            .unwrap_or_else(|e| panic!("{gone_cells} cells gone, {back} put back: {e}"));
        work
    };
    let (small, big) = (cost(2_000, 50), cost(8_000, 200));
    linear("steps of putting cells back", small.steps_used(), big.steps_used());
    // The pass over the list is counted: at least the length of it.
    assert!(small.steps_used() >= 2_000, "{} steps for a list of 2,000", small.steps_used());
    assert!(big.steps_used() >= 8_000, "{} steps for a list of 8,000", big.steps_used());
}
