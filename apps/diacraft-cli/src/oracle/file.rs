//! What the harness says about one file: its hash, its category, the sizes that confirm the caps, the cell and function names it uses and how,
//! the saved values by kind, what did not parse, why it was refused, and the score `diacraft-calc`'s comparer gave.
//!
//! The reading is `diacraft_vsdx::read_partial`; the comparing is `diacraft_calc::recalc_all_using` (one comparer, and this is
//! the join). What is here is the part that is neither: the tallies of names (which cells and functions files use, by the spec's own lists),
//! the category in the census's terms, and the JSON. **The salted control runs through [`examine`], the same function a file does.**

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, Calculator, EvalOptions, OperandMode, RecalcReport, SaltPolicy, recalc_all_salted};
use diacraft_doc::{Cell, Document, FormulaSlot, RawNode, Reading, Row, Section, Shape, ShapeList, Sheet, Value};
use diacraft_formula::{BinaryOp, Expr, PERCENT_FUNCTION, Qualifier};
use diacraft_vsdx::{DiacraftPart, Limits, ReadReport, VsdxError, read_partial};
use serde_json::{Value as Json, json};

use super::control::Control;
use super::names::{FUNCTIONS, RESERVED_USER_ROWS, TRIGGERS};
use super::refusal::{Refusal, refusal_of};
use super::score::{Summary, calc_json};
use super::sha256::sha256_hex;
use super::words::{ERRORS, UNITS, find_in, listed, listed_cell, section_word};

/// The version of the report's format, so a harness can tell a change. 2: the score is `diacraft-calc`'s, and a report says what
/// was left out, what was not calculated, how many cells hold no formula, and what the salted control found. 3: `calc.real_salt`,
/// what salting the real cells of the file found, and the reasons `themed_operand`, `date_past_9999_operand` and `stale_theme_property`; a cell
/// that reads a `Themed` operand is left out under its own reason and no longer counted among the unsupported functions. 4: the reasons
/// `machine_font_number`, `theme_no_shadow` and `formula_must_fail`, and a score that follows the rules (a group's scheme cells, a row named in
/// another letter case, the table of `THEMEVAL`'s names), so a line of schema 3 is a score by other rules and is refused. 5: the reason
/// `text_stand_in` (a `TEXTWIDTH` or `TEXTHEIGHT` measured in a face that is not the name's open metric twin, itself or through a cell it reads), and
/// a score that includes the nine path functions, `TEXTWIDTH` and `TEXTHEIGHT`, so a line of schema 4 is a score of fewer functions and is refused.
/// 6: a score that includes `_ELLIPSE_THETA` and `_ELLIPSE_ECC`, about 418,000 cells that were not calculated and are now compared, by raw value
/// and by the ellipse the `C` and `D` of their row describe, with `calc.agreed_by_curve` counting those that agree only so, so a line of schema 5 is a
/// score without them and is refused (it would otherwise be added to schema-6 lines in an aggregate without a word).
/// 7: a score by five changed rules, so that a line of schema 6 is a score of other cells and values and is refused. `CEILING` and
/// `FLOOR` work on the number shown in its unit, and so do `INT`, `INTUP`, `ROUND`, `TRUNC`, `MODULUS`, `LN`, `LOG10`, `SQRT` and `POW` (our rule);
/// `TEXTHEIGHT` and `TEXTWIDTH` carry a small term of 5E-5 inch inside the scale for a text that holds a
/// character; `THEMEPROP("Embellishment")` is 2 where a sheet reads no theme, and **the reason `stale_theme_property` is gone** (those cells are scored; the
/// key is no longer in `calc.left_out`); and a colour under a scheme the file no longer selects (`stale_theme_colour`) is left out only when the saved
/// value is the reading under the file's first theme part and not the sheet's own, so that the cells which agree under no theme (22,976 in the third run)
/// and the misfits the theme does not explain are scored, and the key counts only the cells left out, which only this harness can measure.
/// `calc.stale_theme_colour_by_function` (with `..._unlisted`) tallies those cells by the spec's names of the functions their formulas call, so that a jump
/// in the count can be traced. **Fields added since, which change no score and so no schema:** `stats.held_parts` and `stats.held_bytes`, the
/// parts of the package the document holds beside its model and their bytes, which the model's `stats.memory_bytes` includes.
pub const SCHEMA: u32 = 7;

/// The category of a file, for the sample files' own terms ("What the terms mean").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// `drawing`, `stencil` or `template`, and whether macros are allowed; `None` when the content type is none of the six.
    pub variant: Option<(&'static str, bool)>,
    /// `reference_app`, `other` or `unknown`, by `Application` of `docProps/app.xml`: the census's writer, on its **exact** match.
    pub writer: &'static str,
    /// The writer says the reference app, the file read, and a cell holds a formula and a cached value (an empty value, `Themed` and a cell carrying an
    /// error are not one). The census's term, without its package traits, which `opc` does not expose.
    pub oracle_usable: bool,
    /// The writer says the reference app, the file read, and a page has a shape glued by a `Connect` at its begin or end, with its own `BeginX` and `EndX`
    /// and at least two `Geometry` rows holding a coordinate. The census's term, without its package traits.
    pub routing_usable: bool,
}

/// What the harness found in one file.
pub struct OracleReport {
    pub sha256: String,
    pub bytes: usize,
    pub source: Option<String>,
    pub category: Option<Category>,
    /// The class word, the kind and the rule behind it when it would not read.
    pub refusal: Option<Refusal>,
    json: Json,
    /// What the comparer found, for a file that read.
    pub calc: Option<Summary>,
    /// Whether the salted control passed on this run. A score is given only when it did.
    pub control_passed: bool,
}

impl OracleReport {
    /// The report as JSON: the harness's output. See the module documentation for what is in it and what is not.
    pub fn to_json(&self) -> &Json {
        &self.json
    }

    /// Whether the real-file salt of this file found nothing (a file that did not read has none to find).
    pub fn salt_passed(&self) -> bool {
        self.calc.as_ref().is_none_or(Summary::salt_passed)
    }

    /// Cells that agreed over cells compared, or `None` when nothing was compared, the control failed, or a salted cell of this file agreed.
    pub fn score(&self) -> Option<f64> {
        if !self.control_passed || !self.salt_passed() {
            return None;
        }
        self.calc.as_ref().and_then(Summary::score)
    }
}

/// A file read as far as it would be, and what the comparer made of it.
pub(super) struct Examined {
    pub read_report: ReadReport,
    pub outcome: Result<Read, VsdxError>,
}

/// A document that read: the tallies of its names and the score.
pub(super) struct Read {
    pub findings: Findings,
    pub recalc: RecalcReport,
}

/// The faces `TEXTWIDTH` and `TEXTHEIGHT` are measured with: the machine's and the bundled ones, found once for the run (reading the font folders
/// costs a few kilobytes a face). The spec's measures were made with the open twins of Calibri, Arial, Times New Roman and Courier New, so a run
/// on a machine without them measures in stand-ins, and the cells that did are left out under `text_stand_in` and counted there.
fn machine_fonts() -> diacraft_calc::Fonts {
    static FONTS: std::sync::OnceLock<diacraft_calc::Fonts> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| diacraft_calc::Fonts::new(diacraft_fonts::FontDb::system_and_bundled())).clone()
}

/// Reads `bytes` and scores them. A file and the salted control go through here and nothing else: what the control shows of the harness is
/// true of every file.
pub(super) fn examine(bytes: &[u8], limits: &Limits, calc: &mut dyn Calculator) -> Examined {
    let (result, read_report) = read_partial(bytes, limits);
    let outcome = result.map(|doc| {
        let findings = analyse(&doc);
        // Each cell is tested alone against what its operands were saved as: how the specs' own scans were run.
        let options = EvalOptions { operands: OperandMode::Cached, fonts: Some(machine_fonts()), ..EvalOptions::default() };
        // And a seeded sample of the real cells that agreed is salted with a wrong value and worked out again: an echo agrees with every real
        // cell, and the control above is one synthetic document.
        let recalc = recalc_all_salted(&doc, &Budget::default(), options, calc, Some(SaltPolicy::STANDARD));
        Read { findings, recalc }
    });
    Examined { read_report, outcome }
}

/// Reads `bytes` and reports on them. `source` is the repository or host the file came from, as the sample-file manifest names it, and is only
/// carried into the report for the aggregation. `calc` works the cells out, and `control` is what the salted control found on this run: with
/// it failed there is no score.
pub fn report(bytes: &[u8], limits: &Limits, source: Option<&str>, calc: &mut dyn Calculator, control: &Control) -> OracleReport {
    let sha256 = sha256_hex(bytes);
    let mut out = OracleReport {
        sha256: sha256.clone(),
        bytes: bytes.len(),
        source: source.map(str::to_owned),
        category: None,
        refusal: None,
        json: Json::Null,
        calc: None,
        control_passed: control.passed(),
    };
    let mut json = json!({ "schema": SCHEMA, "sha256": sha256, "bytes": bytes.len(), "source": source });
    let examined = examine(bytes, limits, calc);
    let read_report = examined.read_report;
    match examined.outcome {
        Err(error) => {
            let refusal = refusal_of(&error, read_report.refused_at.as_ref());
            json["refusal"] = json!({ "class": refusal.class, "kind": refusal.kind, "detail": refusal.detail });
            json["read"] = json!(false);
            json["calc"] = json!({ "available": true, "control": control.json() });
            // A refused file reports what the reader knew when it refused it: its size, its archive, the parts it got to, the budgets spent.
            json["read_report"] = read_report_json(&read_report);
            out.refusal = Some(refusal);
        }
        Ok(read) => {
            let category = category_of(&read_report, &read.findings);
            let findings = &read.findings;
            json["read"] = json!(true);
            json["category"] = json!({
                "variant": category.variant.map(|(kind, _)| kind),
                "macro_enabled": category.variant.map(|(_, m)| m),
                "writer": category.writer,
                "oracle_usable": category.oracle_usable,
                "routing_usable": category.routing_usable,
                "glued_connectors": findings.glued_connectors,
                "package_traits": "not_seen",
            });
            json["read_report"] = read_report_json(&read_report);
            json["names"] = names_json(findings);
            json["saved"] = json!({
                "value_kinds": findings.values,
                "errors": findings.errors,
                "units": findings.units,
                "triggers": findings.triggers,
                "cached_formula_cells": findings.cached_formula_cells,
            });
            json["formulas"] = json!({
                "cells_with_formula": read_report.stats.formula_cells,
                "not_parsed": read_report.formula_errors,
                "nodes": read_report.stats.formula_nodes,
                "cached_value_cells": findings.cached_formula_cells,
            });
            json["calc"] = calc_json(&read.recalc, control);
            out.calc = Some(Summary::of(&read.recalc));
            out.category = Some(category);
        }
    }
    json["score"] = match (out.score(), &out.calc) {
        (Some(score), Some(s)) => json!({ "score": score, "matched": s.agreed, "compared": s.compared(), "coverage": s.coverage() }),
        _ => Json::Null,
    };
    if !out.control_passed && out.calc.as_ref().is_some_and(|s| s.compared() > 0) {
        json["score_withheld"] = json!("control_failed");
    } else if !out.salt_passed() && out.calc.as_ref().is_some_and(|s| s.compared() > 0) {
        json["score_withheld"] = json!("real_salt_failed");
    }
    out.json = json;
    out
}

// ---------------------------------------------------------------------------------------------------------------------- the tallies

/// How the formulas of one document call things: the listed function names with their counts, and the count of the calls to any other.
#[derive(Default)]
struct Calls {
    named: BTreeMap<&'static str, u64>,
    unlisted: u64,
}

/// What the harness said about one cell kind.
#[derive(Default, Clone, Copy)]
struct CellTally {
    formula: u64,
    inherited: u64,
    no_formula: u64,
    value_only: u64,
    with_error: u64,
}

/// What the harness found in a document that read.
pub(super) struct Findings {
    cells: BTreeMap<(&'static str, &'static str), CellTally>,
    cells_unlisted: CellTally,
    calls: Calls,
    errors: BTreeMap<&'static str, u64>,
    units: BTreeMap<&'static str, u64>,
    values: BTreeMap<&'static str, u64>,
    triggers: BTreeMap<&'static str, u64>,
    pub cached_formula_cells: u64,
    glued_connectors: u64,
    reserved_user_rows: BTreeMap<&'static str, u64>,
}

impl Findings {
    fn new() -> Findings {
        Findings {
            cells: BTreeMap::new(),
            cells_unlisted: CellTally::default(),
            calls: Calls::default(),
            errors: BTreeMap::new(),
            units: BTreeMap::new(),
            values: BTreeMap::new(),
            triggers: BTreeMap::new(),
            cached_formula_cells: 0,
            glued_connectors: 0,
            reserved_user_rows: BTreeMap::new(),
        }
    }
}

fn category_of(read_report: &ReadReport, findings: &Findings) -> Category {
    // The census: the reference app is said on an exact match of the product name, with or without the company prefix, so a tool whose name merely
    // contains the word does not count (M7: no trimming and no folding of case).
    let writer = match read_report.application.as_deref() {
        None => "unknown",
        Some("Microsoft Visio" | "Visio") => "reference_app", // brand-ok: the value a file states
        Some(_) => "other",
    };
    let variant = read_report.variant.map(|v| (v.kind_word(), v.macro_enabled));
    Category {
        variant,
        writer,
        oracle_usable: writer == "reference_app" && findings.cached_formula_cells > 0,
        routing_usable: writer == "reference_app" && findings.glued_connectors > 0,
    }
}

fn read_report_json(r: &ReadReport) -> Json {
    let s = &r.stats;
    json!({
        "stats": {
            "parts": s.parts, "uncompressed_bytes": s.uncompressed_bytes, "styles": s.styles, "masters": s.masters, "pages": s.pages, "shapes": s.shapes,
            "shapes_per_page": s.shapes_per_page, "max_shapes_in_a_page": s.max_shapes_in_a_page, "max_shapes_in_a_master": s.max_shapes_in_a_master,
            "max_shape_depth": s.max_shape_depth, "cells": s.cells, "formula_cells": s.formula_cells, "formula_nodes": s.formula_nodes,
            "items": s.items, "xml_bytes": s.xml_bytes, "max_cells_in_one_place": s.max_cells_in_one_place,
            "max_sections_in_a_sheet": s.max_sections_in_a_sheet, "max_rows_in_a_section": s.max_rows_in_a_section, "media_bytes": s.media_bytes,
            "themes": s.themes, "connects": s.connects,
            "file_bytes": s.file_bytes, "zip_rows": s.zip_rows, "max_part_name_bytes": s.max_part_name_bytes, "max_part_bytes": s.max_part_bytes,
            "content_type_rows": s.content_type_rows, "max_rels_bytes": s.max_rels_bytes, "max_rels_rows": s.max_rels_rows,
            "max_xml_items": s.max_xml_items, "max_xml_depth": s.max_xml_depth, "max_xml_attr_bytes": s.max_xml_attr_bytes,
            "memory_bytes": s.memory_bytes, "memory_peak": s.memory_peak, "max_tree_bytes": s.max_tree_bytes,
            "held_parts": r.held_parts, "held_bytes": r.held_bytes,
        },
        "namespaces": {
            "ns_2012": r.namespaces.main_2012, "ns_2011": r.namespaces.core_2011, "none": r.namespaces.none, "other": r.namespaces.other,
        },
        "warnings": r.warning_counts,
        "skipped": r.skipped_counts,
        "diacraft": match &r.diacraft {
            None => Json::Null,
            Some(DiacraftPart::Absent) => json!("absent"),
            Some(DiacraftPart::Loaded { .. }) => json!("loaded"),
            Some(DiacraftPart::Unreadable(_)) => json!("unreadable"),
        },
    })
}

fn tally_json(t: &CellTally) -> Json {
    json!({ "formula": t.formula, "inherited": t.inherited, "no_formula": t.no_formula, "value_only": t.value_only, "with_error": t.with_error })
}

fn names_json(f: &Findings) -> Json {
    let cells: BTreeMap<String, Json> = f
        .cells
        .iter()
        .map(|((section, name), t)| (if section.is_empty() { (*name).to_owned() } else { format!("{section}.{name}") }, tally_json(t)))
        .collect();
    json!({
        "cells": cells,
        "cells_unlisted": tally_json(&f.cells_unlisted),
        "functions": f.calls.named,
        "functions_unlisted_calls": f.calls.unlisted,
        "reserved_user_rows": f.reserved_user_rows,
    })
}

// ------------------------------------------------------------------------------------------------------------------ analysing

/// Whether a saved value is a colour as a file spells one: `#` and six hexadecimal digits. Used to count values by kind, never to decide what
/// crosses (a text crosses only as its length).
pub(super) fn is_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

pub(super) fn kind_of(text: &str) -> &'static str {
    match Value::from_text(text).reading() {
        Reading::Empty => "empty",
        Reading::Number(_) => "number",
        Reading::Infinite { .. } => "infinite",
        Reading::Themed => "themed",
        Reading::Text if is_colour(text) => "colour",
        Reading::Text => "text",
    }
}

/// The expression nodes of a tree, with no recursion.
fn for_each_expr<'a>(root: &'a Expr, mut f: impl FnMut(&'a Expr)) {
    let mut stack: Vec<&'a Expr> = vec![root];
    while let Some(e) = stack.pop() {
        f(e);
        match e {
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            Expr::Call(call) => {
                stack.extend(call.args.iter().flatten());
                for q in &call.qualifiers {
                    if let Qualifier::Call { args, .. } = q {
                        stack.extend(args.iter().flatten());
                    }
                }
            }
            Expr::Ref(r) => {
                for q in &r.qualifiers {
                    if let Qualifier::Call { args, .. } = q {
                        stack.extend(args.iter().flatten());
                    }
                }
            }
            Expr::Number(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Color { .. } | Expr::Currency { .. } | Expr::Guid(_) => {}
        }
    }
}

/// The function name of a node that is a call or an operator: its upper-case spelling. **`^` is not `POW`**: it is an operator, and the
/// function `POW` is a call a file wrote, which the sample files count apart (5 files call it, and 857 use the operator).
fn function_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Call(call) => Some(call.name.to_ascii_uppercase()),
        Expr::Unary(op, _) => Some(op.function_name().to_owned()),
        Expr::Binary(BinaryOp::Pow, ..) => None,
        Expr::Binary(op, ..) => Some(op.function_name().to_owned()),
        Expr::Percent(_) => Some(PERCENT_FUNCTION.to_owned()),
        _ => None,
    }
}

/// The names of the calls a node makes: its own (`function_of`), and **the calls used as sheet qualifiers** (`LISTSHEETREF(2)!Width` calls
/// `LISTSHEETREF`, which is a call all the same and was never named: `CONTAINERSHEETREF` 288 files to 0 before this).
fn calls_of(e: &Expr) -> Vec<String> {
    let mut names: Vec<String> = function_of(e).into_iter().collect();
    let qualifiers: &[Qualifier] = match e {
        Expr::Call(call) => &call.qualifiers,
        Expr::Ref(r) => &r.qualifiers,
        _ => &[],
    };
    for q in qualifiers {
        if let Qualifier::Call { name, .. } = q {
            names.push(name.to_ascii_uppercase());
        }
    }
    names
}

/// Counts the calls of a formula by listed name.
fn count_calls(expr: &Expr, calls: &mut Calls) {
    for_each_expr(expr, |e| {
        for name in calls_of(e) {
            match listed(FUNCTIONS, &name) {
                Some(known) => *calls.named.entry(known).or_insert(0) += 1,
                None => calls.unlisted += 1,
            }
        }
    });
}

/// The names the document uses, and the cells it saves, tallied over every sheet. **A list of shapes that several pages or masters share (the
/// reader shares a part that two rows name) is walked once**, by the first that holds it: what a file says about its cells is said once for the
/// part, and the walk is not multiplied by the pages.
fn analyse(doc: &Document) -> Findings {
    let mut f = Findings::new();
    let mut walked: HashSet<usize> = HashSet::new();
    // A list is known by where its first shape is: the lists the reader shares are one allocation, and nothing else the reader makes has a shape in two.
    let mut first_time = |list: &ShapeList| list.get(0).is_none_or(|first| walked.insert(Arc::as_ptr(first) as usize));
    let mut sheets: Vec<&Sheet> = vec![&doc.sheet];
    for style in &doc.styles {
        sheets.push(&style.sheet);
    }
    let mut lists: Vec<&ShapeList> = Vec::new();
    for master in &doc.masters {
        sheets.push(&master.sheet);
        if first_time(&master.shapes) {
            lists.push(&master.shapes);
        }
    }
    for page in &doc.pages {
        sheets.push(&page.sheet);
        if first_time(&page.shapes) {
            lists.push(&page.shapes);
        }
    }
    for sheet in sheets {
        analyse_sheet(sheet, &mut f);
    }
    for list in lists {
        let mut stack: Vec<&ShapeList> = vec![list];
        while let Some(list) = stack.pop() {
            for shape in list.iter() {
                analyse_sheet(&shape.sheet, &mut f);
                if !shape.children.is_empty() {
                    stack.push(&shape.children);
                }
            }
        }
    }
    f.glued_connectors = glued_connectors(doc);
    f
}

fn analyse_sheet(sheet: &Sheet, f: &mut Findings) {
    let mut each = |section: Option<&Section>, row: Option<&Row>, name: &str, cell: &Cell| {
        tally_cell(f, section, row, name, cell);
        if let FormulaSlot::Local(formula) = &cell.formula
            && let Ok(expr) = &formula.ast
        {
            count_calls(expr, &mut f.calls);
        }
    };
    for (name, cell) in &sheet.cells {
        each(None, None, name.as_str(), cell);
    }
    for section in &sheet.sections {
        for (name, cell) in &section.cells {
            each(Some(section), None, name.as_str(), cell);
        }
        for row in &section.rows {
            for (name, cell) in &row.cells {
                each(Some(section), Some(row), name.as_str(), cell);
            }
        }
    }
    for kept in sheet.passthrough.iter() {
        if let RawNode::Element(el) = &kept.node
            && &*el.name == "Trigger"
            && let Some(name) = el.attrs.get("N").and_then(|n| listed(TRIGGERS, n))
        {
            *f.triggers.entry(name).or_insert(0) += 1;
        }
    }
}

fn tally_cell(f: &mut Findings, section: Option<&Section>, row: Option<&Row>, name: &str, cell: &Cell) {
    // A listed cell is named whatever section it is in: the section is one of the 17 or `<other section>`, a fixed word, and the name is the
    // spec's. (A cell in a section a file made up was counted as unlisted before: three real cell names, 608 cells.)
    let section_name = section_word(section);
    let tally = match listed_cell(name) {
        Some(listed_name) => f.cells.entry((section_name, listed_name)).or_default(),
        None => &mut f.cells_unlisted,
    };
    match &cell.formula {
        FormulaSlot::Local(_) => tally.formula += 1,
        FormulaSlot::Inherit => tally.inherited += 1,
        FormulaSlot::NoFormula => tally.no_formula += 1,
        FormulaSlot::Absent => tally.value_only += 1,
    }
    if cell.error.is_some() {
        tally.with_error += 1;
    }
    if let Some(v) = &cell.value {
        *f.values.entry(kind_of(v.text())).or_insert(0) += 1;
    }
    if let Some(e) = &cell.error {
        *f.errors.entry(find_in(ERRORS, e).unwrap_or("other")).or_insert(0) += 1;
    }
    if let Some(u) = &cell.unit {
        *f.units.entry(find_in(UNITS, u).unwrap_or("other")).or_insert(0) += 1;
    }
    if section.is_some_and(|s| s.name == "User")
        && let Some(row) = row
        && let Some(reserved) = row.name.as_deref().and_then(|n| listed(RESERVED_USER_ROWS, n))
        && name == "Value"
    {
        *f.reserved_user_rows.entry(reserved).or_insert(0) += 1;
    }
    let cached = cell.error.is_none() && cell.value.as_ref().is_some_and(|v| !matches!(kind_of(v.text()), "empty" | "themed"));
    if matches!(&cell.formula, FormulaSlot::Local(_)) && cached {
        f.cached_formula_cells += 1;
    }
}

/// How many shapes on pages are glued by a `Connect` at their begin or end and have the saved geometry the census's routing term asks for.
fn glued_connectors(doc: &Document) -> u64 {
    let mut count = 0u64;
    // A part that several pages share (its shapes and its connects are one) is looked at once.
    let mut seen_lists: HashSet<usize> = HashSet::new();
    for page in &doc.pages {
        if page.connects.is_empty() || page.shapes.get(0).is_none_or(|first| !seen_lists.insert(Arc::as_ptr(first) as usize)) {
            continue;
        }
        let mut by_id: HashMap<u32, &Shape> = HashMap::new();
        let mut stack: Vec<&ShapeList> = vec![&page.shapes];
        while let Some(list) = stack.pop() {
            for shape in list.iter() {
                by_id.entry(shape.id).or_insert(shape);
                if !shape.children.is_empty() {
                    stack.push(&shape.children);
                }
            }
        }
        let mut seen: HashSet<u32> = HashSet::new();
        for c in &page.connects {
            let glued_end = matches!(c.from_cell.as_deref(), Some("BeginX" | "EndX" | "BeginY" | "EndY")) || matches!(c.from_part, Some(9 | 12));
            if !glued_end || !seen.insert(c.from_sheet) {
                continue;
            }
            if by_id.get(&c.from_sheet).is_some_and(|s| has_routing_geometry(s)) {
                count += 1;
            }
        }
    }
    count
}

fn has_routing_geometry(shape: &Shape) -> bool {
    let has = |name: &str| shape.sheet.cell(name).is_some_and(|c| c.value.is_some());
    let with_value = |row: &Row, cell: &str| row.cells.get(cell).is_some_and(|c| c.value.is_some());
    let rows_with_coordinates: usize = shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name == "Geometry")
        .map(|s| s.rows.iter().filter(|r| with_value(r, "X") || with_value(r, "Y")).count())
        .sum();
    has("BeginX") && has("EndX") && rows_with_coordinates >= 2
}
