//! One cell as a sheet shows it (design 5.6): its formula as the file has it, the value it holds in its unit, where it comes from, and whether it parsed and can be worked out.
//!
//! **Nothing is worked out here.** The value is the cached `V` the resolver finds (the model keeps it up to date on every commit): a read shows what the file or the last
//! commit saved. Only a cell found nowhere that reads a printed formula as its default (the seven cells of Text Transform, the two picture sizes) has that formula worked out, on
//! the sheet asked, within a budget of its own.

use std::collections::HashSet;
use std::sync::OnceLock;

use diacraft_calc::{Budget, EvalResult, Value as Calc, evaluate_text_with, function_names, reference_name};
use diacraft_doc::{CellAddr, CellClass, Document, FormulaSlot, Resolved, Resolver, RowKey, SheetRef, Source, ValueKind};
use diacraft_formula::{Expr, Formula};
use serde_json::{Value, json};

use super::Counts;
use super::text::{self, CellValue, Typed};

/// Where a cell is found: its own, its master's or a style's, or nowhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Local,
    Inherited,
    Default,
}

impl Origin {
    pub fn word(self) -> &'static str {
        match self {
            Origin::Local => "local",
            Origin::Inherited => "inherited",
            Origin::Default => "default",
        }
    }
}

/// The sheet an inherited cell comes from: "From the master Process (ID 3)", "From the style Normal".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FromView {
    /// `master` or `style`.
    pub kind: &'static str,
    /// The master's or the style's `ID`.
    pub id: u32,
    pub name: Option<String>,
    /// The `ID` of the master's shape the cell is on.
    pub shape: Option<u32>,
    /// The family of the style link the cell was reached along: `line`, `fill` or `text`; none for the root style.
    pub family: Option<&'static str>,
}

impl FromView {
    pub fn json(&self) -> Value {
        let mut out = json!({"kind": self.kind, "id": self.id, "name": self.name});
        if let (Some(map), Some(shape)) = (out.as_object_mut(), self.shape) {
            map.insert("shape".into(), json!(shape));
        }
        if let (Some(map), Some(family)) = (out.as_object_mut(), self.family) {
            map.insert("family".into(), json!(family));
        }
        out
    }
}

/// A cell, as `sheet.read` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct CellView {
    /// The cell's name: `PinX`, `X`.
    pub name: String,
    /// The place of the cell's column in its section's `columns`, for a cell of a row.
    pub col: Option<usize>,
    /// The reference a formula names the cell by (`Geometry1.X2`), or `None` where no reference can be spelled.
    pub reference: Option<String>,
    /// The formula as the file has it, `No Formula`, or the value as a literal when the cell has no formula.
    pub formula: String,
    pub value: CellValue,
    /// The unit the value is in (`U`).
    pub unit: Option<&'static str>,
    /// The text the Values view shows.
    pub shown: String,
    /// `E`: the error of the last evaluation, `#DIV/0!`.
    pub error: Option<String>,
    pub origin: Origin,
    pub from: Option<FromView>,
    /// The formula is `GUARD(...)`.
    pub guarded: bool,
    /// The formula parsed (true for a cell with no formula).
    pub parsed: bool,
    /// The evaluator can work the formula out: it parsed and calls only functions it has.
    pub worked_out: bool,
    /// A text of the cell was cut at the most characters a read gives.
    pub truncated: bool,
}

impl CellView {
    /// The bytes of text the cell holds, as accounted by the cap on an answer.
    pub fn bytes(&self) -> usize {
        let text = match &self.value {
            CellValue::Text(t) => t.len(),
            _ => 0,
        };
        self.name.len() + self.formula.len() + self.shown.len() + text + self.reference.as_ref().map_or(0, String::len) + 64
    }

    /// The cell as a JSON object, the form of `sheet.read`'s `cells`.
    pub fn json(&self) -> Value {
        let mut out = json!({
            "ref": self.reference,
            "name": self.name,
            "formula": self.formula,
            "value": self.value.json(),
            "unit": self.unit,
            "shown": self.shown,
            "error": self.error,
            "origin": self.origin.word(),
            "from": self.from.as_ref().map(FromView::json),
            "guarded": self.guarded,
            "parsed": self.parsed,
            "worked_out": self.worked_out,
            "truncated": self.truncated,
        });
        if let (Some(map), Some(col)) = (out.as_object_mut(), self.col) {
            map.insert("col".into(), json!(col));
        }
        out
    }
}

/// The most nodes of a formula looked at to tell whether it calls a function the evaluator does not have.
const MAX_SCAN_NODES: usize = 4_096;

/// The most steps the default formula of a cell is worked out in.
const DEFAULT_FORMULA_STEPS: u64 = 20_000;

/// The functions the evaluator has, in capitals.
fn known_functions() -> &'static HashSet<&'static str> {
    static NAMES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| function_names().into_iter().collect())
}

/// Whether the formula calls a function the evaluator does not have, looking at its first [`MAX_SCAN_NODES`] nodes.
fn calls_unknown_function(formula: &Formula) -> bool {
    let Ok(root) = &formula.ast else { return false };
    let known = known_functions();
    let mut stack: Vec<&Expr> = vec![root];
    let mut seen = 0_usize;
    while let Some(expr) = stack.pop() {
        seen += 1;
        if seen > MAX_SCAN_NODES {
            return false;
        }
        match expr {
            Expr::Call(call) => {
                if !known.contains(call.name.to_ascii_uppercase().as_str()) {
                    return true;
                }
                stack.extend(call.args.iter().flatten());
            }
            Expr::Unary(_, inner) | Expr::Percent(inner) => stack.push(inner),
            Expr::Binary(_, left, right) => {
                stack.push(left);
                stack.push(right);
            }
            _ => {}
        }
    }
    false
}

/// What a read of a cell is given: the resolver of the document, and the sheet.
pub(crate) struct Ctx<'r, 'd> {
    pub res: &'r Resolver<'d>,
    pub sheet: &'r SheetRef,
}

/// Where a source is, for a view.
fn from_of(doc: &Document, source: &Source) -> Option<FromView> {
    match source {
        Source::Local | Source::Default => None,
        Source::Master(at) => {
            let id = match at.owner {
                diacraft_doc::Owner::Master(id) | diacraft_doc::Owner::Page(id) => id,
            };
            Some(FromView {
                kind: "master",
                id,
                name: doc.master(id).and_then(|m| m.name.clone().or_else(|| m.name_u.clone())).map(|n| n.to_string()),
                shape: doc.shape(at).map(|s| s.id),
                family: None,
            })
        }
        Source::Style { id, family } => Some(FromView {
            kind: "style",
            id: *id,
            name: doc.style(*id).and_then(|s| s.name.clone().or_else(|| s.name_u.clone())).map(|n| n.to_string()),
            shape: None,
            family: family.map(|f| match f {
                diacraft_doc::Family::Line => "line",
                diacraft_doc::Family::Fill => "fill",
                diacraft_doc::Family::Text => "text",
            }),
        }),
    }
}

fn origin_of(source: &Source) -> Origin {
    match source {
        Source::Local => Origin::Local,
        Source::Master(_) | Source::Style { .. } => Origin::Inherited,
        Source::Default => Origin::Default,
    }
}

/// The value a cell found nowhere reads: its kind's default, or its printed default formula worked out on the sheet.
fn default_typed(ctx: &Ctx<'_, '_>, resolved: &Resolved<'_>, addr: &CellAddr) -> (Typed, Option<String>) {
    if let Some(formula) = resolved.default.formula {
        let options = crate::measure::eval_options();
        let done = evaluate_text_with(ctx.res.document(), ctx.sheet, Some(addr), formula, &options, &Budget::with_steps(DEFAULT_FORMULA_STEPS));
        if let EvalResult::Value(v) = done.result
            && !done.budget_exhausted
            && !matches!(v, Calc::Unsupported(_))
        {
            return (text::typed_calc(&v), text::error_of(&v).map(str::to_owned));
        }
    }
    (text::default_of(resolved.kind_if_no_value()), None)
}

/// The view of the cell at `addr` on the sheet, or `None` where the sheet does not exist or the row or section it is in is deleted. `limit` cuts the texts of the
/// cell to that many characters (a read gives [`MAX_FORMULA_CHARS`](super::MAX_FORMULA_CHARS), `sheet.cell` gives them whole). `counts` is charged one cell.
pub(crate) fn view(
    ctx: &Ctx<'_, '_>,
    addr: &CellAddr,
    name: &str,
    col: Option<usize>,
    limit: Option<usize>,
    counts: &mut Counts,
) -> Option<CellView> {
    let resolved = ctx.res.resolve(ctx.sheet, addr)?;
    counts.cells = counts.cells.saturating_add(1);
    let doc = ctx.res.document();
    let origin = origin_of(&resolved.formula.source);
    let (formula_cell, value_cell) = (resolved.formula.cell, resolved.value.cell);
    let unit_attr = value_cell.and_then(|c| c.unit.as_deref()).or_else(|| formula_cell.and_then(|c| c.unit.as_deref()));
    let kind: ValueKind = resolved.default.kind;

    let mut error = value_cell.or(formula_cell).and_then(|c| c.error.as_deref()).map(str::to_owned);
    let typed = match value_cell.and_then(|c| c.value.as_ref()) {
        Some(v) => text::typed(v.text(), unit_attr, kind),
        None => {
            let (typed, e) = default_typed(ctx, &resolved, addr);
            error = error.or(e);
            typed
        }
    };

    let (mut formula, parsed, worked_out, guarded) = match formula_cell.map(|c| &c.formula) {
        Some(FormulaSlot::Local(f)) => {
            (f.source.to_string(), f.is_parsed(), f.is_parsed() && !calls_unknown_function(f), formula_cell.is_some_and(|c| c.is_guarded()))
        }
        Some(FormulaSlot::NoFormula) => ("No Formula".to_owned(), true, true, false),
        Some(FormulaSlot::Absent | FormulaSlot::Inherit) => (typed.literal.clone(), true, true, false),
        None => (resolved.default.formula.map_or_else(|| typed.literal.clone(), str::to_owned), true, true, false),
    };
    let mut shown = match (&error, parsed, worked_out) {
        (Some(e), _, _) => e.clone(),
        (None, false, _) => "not read".to_owned(),
        (None, true, false) => format!("{}?", typed.shown),
        (None, true, true) => typed.shown.clone(),
    };
    let mut value = typed.value;
    let mut truncated = false;
    if let Some(limit) = limit {
        let (cut, was) = text::cut(&formula, limit);
        (formula, truncated) = (cut, was);
        let (cut, was) = text::cut(&shown, limit);
        (shown, truncated) = (cut, truncated || was);
        if let CellValue::Text(t) = &value {
            let (cut, was) = text::cut(t, limit);
            truncated |= was;
            value = CellValue::Text(cut);
        }
    }
    Some(CellView {
        name: name.to_owned(),
        col,
        reference: reference_name(addr),
        formula,
        value,
        unit: typed.unit,
        shown,
        error,
        origin,
        from: from_of(doc, &resolved.formula.source),
        guarded,
        parsed,
        worked_out,
        truncated,
    })
}

/// The address a reference names, with a named row spelled as the file spells it (**a row's name in a reference matches without regard to letter
/// case**, which `cell_address` cannot see, having no document). A row the sheet holds under exactly the name is left as it is; otherwise the row whose name matches in
/// another case is meant, the one the sheet holds itself before one only a master or a style holds, then the first in the order of the section. A name no row matches in any
/// case is left as given (it names a cell that is not there, and reads its default).
pub(crate) fn respell(res: &Resolver<'_>, sheet: &SheetRef, addr: CellAddr) -> CellAddr {
    let (Some(section), Some(RowKey::Name(name))) = (&addr.section, &addr.row) else { return addr };
    if res.row_cell_names(sheet, section, &RowKey::Name(name.clone())).is_none_or(|cells| !cells.is_empty()) {
        return addr;
    }
    let Some(rows) = res.effective_row_keys(sheet, section) else { return addr };
    let folded = name.to_lowercase();
    let local = res.sheet(sheet).and_then(|held| held.section(section));
    let mut best: Option<(bool, &RowKey)> = None;
    for row in &rows {
        let RowKey::Name(spelled) = &row.key else { continue };
        if spelled == name {
            return addr;
        }
        if spelled.to_lowercase() == folded {
            let own = local.is_some_and(|held| held.row(&row.key).is_some());
            if best.is_none_or(|(had, _)| own && !had) {
                best = Some((own, &row.key));
            }
        }
    }
    match best {
        Some((_, key)) => CellAddr { row: Some(key.clone()), ..addr },
        None => addr,
    }
}

/// The class of a cell as a word of an answer.
pub(crate) fn class_word(class: CellClass) -> &'static str {
    match class {
        CellClass::ShapeOnly => "shape-only",
        CellClass::StyleAny => "style-any",
        CellClass::FillFamily => "fill-family",
        CellClass::LineFamily => "line-family",
        CellClass::TextFamily => "text-family",
        CellClass::PageSheet => "page-sheet",
        CellClass::StyleFlag(_) => "style-flag",
        CellClass::StyleLocal => "style-local",
        CellClass::Unlisted => "unlisted",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_formula_that_calls_a_function_the_evaluator_does_not_have_is_found() {
        let parse = |t: &str| diacraft_formula::parse(t);
        assert!(!calls_unknown_function(&parse("IF(Width>1, GUARD(2), MAX(1,2))")));
        assert!(calls_unknown_function(&parse("1+FOOBAR(2)")));
        assert!(calls_unknown_function(&parse("IF(1, 2, nosuchthing(3))")), "in any letter case, nested");
        assert!(!calls_unknown_function(&parse("1 +* 2")), "a formula that does not parse is not a call of anything");
        assert!(!calls_unknown_function(&parse("Width*2")));
    }
}
