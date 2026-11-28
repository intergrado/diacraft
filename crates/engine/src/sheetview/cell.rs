//! One cell as a sheet shows it (design 5.6): its formula as the file has it, the value it holds in its unit, where it comes from, and whether it parsed and can be worked out.
//!
//! **Nothing is worked out here.** The value is the cached `V` the resolver finds (the model keeps it up to date on every commit): a read shows what the file or the last
//! commit saved. Only a cell found nowhere that reads a printed formula as its default (the seven cells of Text Transform, the two picture sizes) has that formula worked out, on
//! the sheet asked, within a budget of its own.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;

use diacraft_calc::{
    Budget, CellAddress, EvalCtx, EvalOptions, EvalResult, Evaluator, OperandMode, Value as Calc, evaluate_text_with, function_names, reference_name,
    spell_row,
};
use diacraft_doc::{CellAddr, CellClass, Document, FormulaSlot, Keying, Resolved, Resolver, RowKey, SheetRef, Source, ValueKind, keying};
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

/// What a read of a cell is given: the resolver of the document, the sheet, and the evaluator that works out the cells that hold a formula and no saved value
/// Which shows them as the picture draws them and not as the default of their kind.
pub(crate) struct Ctx<'r, 'd> {
    pub res: &'r Resolver<'d>,
    pub sheet: &'r SheetRef,
    ev: RefCell<Evaluator<'d, 'r>>,
}

impl<'r, 'd> Ctx<'r, 'd> {
    /// A read of cells of `sheet`, working the cells with no saved value out in `eval` ([`read_eval_ctx`]: one for as many cells as the read has).
    pub(crate) fn new(res: &'r Resolver<'d>, sheet: &'r SheetRef, eval: &'r mut EvalCtx) -> Ctx<'r, 'd> {
        Ctx { res, sheet, ev: RefCell::new(Evaluator::new(res.document(), eval)) }
    }
}

/// The most steps a read of cells (a window of up to 2,000 lines, or one cell) works the cells with no saved value out in: a themed call is 25, so the lines of a window
/// are far within it. Past them the cells that are left show as they did before, with the default of their kind.
pub const READ_UNSAVED_STEPS: u64 = 1_000_000;

/// The state the evaluator of a read works in: the cells a formula reads are worked out as well (a cell it reads may have no saved value either), and the steps are
/// [`READ_UNSAVED_STEPS`].
pub(crate) fn read_eval_ctx() -> EvalCtx {
    EvalCtx::new(EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() }, Budget::with_steps(READ_UNSAVED_STEPS))
}

/// What a cell that holds a formula and no saved value shows as: the formula worked out on the sheet, **typed as a saved value would be** (a number is in the unit the cell
/// shows in, `unit_attr` and the kind, and not in the unit the formula's own parts had: `Width*1.5` is inches here as it is for a saved value), and its error. `None` for a
/// cell that is no such cell, a formula that gives no result (it does not parse, or names a function this evaluator lacks) and a read whose steps ran out: the cell then shows
/// as it did before.
fn worked_typed(
    ctx: &Ctx<'_, '_>,
    resolved: &Resolved<'_>,
    addr: &CellAddr,
    unit_attr: Option<&str>,
    kind: ValueKind,
) -> Option<(Typed, Option<String>)> {
    if !resolved.holds_unsaved_formula() {
        return None;
    }
    let mut ev = ctx.ev.borrow_mut();
    let answer = ev.evaluate_worked_out(&CellAddress::new(ctx.sheet.clone(), addr.clone()))?;
    if ev.exhausted() || matches!(answer, Calc::Unsupported(_)) {
        return None;
    }
    let typed = match &answer {
        Calc::Number(q) if q.stored.is_finite() => text::typed(&format!("{}", q.stored), unit_attr, kind),
        Calc::Color(c) => text::typed(&c.hex(), unit_attr, kind),
        Calc::Text(t) => text::typed(t, unit_attr, kind),
        Calc::Bool(b) => text::typed(if *b { "TRUE" } else { "FALSE" }, unit_attr, kind),
        other => text::typed_calc(other),
    };
    Some((typed, text::error_of(&answer).map(str::to_owned)))
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
            let (typed, e) = worked_typed(ctx, &resolved, addr, unit_attr, kind).unwrap_or_else(|| default_typed(ctx, &resolved, addr));
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
/// case**, which `cell_address` cannot see, having no document). A row the sheet reads under exactly the name is left as it is, whether it holds the cell or not and even when it is
/// deleted here; otherwise the row the evaluator would read for the name is meant (`diacraft_calc::spell_row`: the nearer sheet, then the lower `IX`, then the earlier place), so
/// that this reading and a formula's agree on the row. A name no row matches in any case is left as given (it names a cell that is not there, and reads its default).
pub(crate) fn respell(res: &Resolver<'_>, sheet: &SheetRef, addr: CellAddr) -> CellAddr {
    let (Some(section), Some(RowKey::Name(name))) = (&addr.section, &addr.row) else { return addr };
    if keying(section.name.as_str()) == Keying::Indexed {
        return addr;
    }
    if res.row_cell_names(sheet, section, &RowKey::Name(name.clone())).is_none_or(|cells| !cells.is_empty()) {
        return addr;
    }
    let Some(layers) = res.section_layers(sheet, section) else { return addr };
    match spell_row(&layers, name) {
        Some(spelling) => CellAddr { row: Some(RowKey::Name(spelling)), ..addr },
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

    /// A shape of a document with `n` cells `User.R<i>` that each hold `2+3` and no value.
    fn doc_with_unsaved(n: u32) -> (Document, SheetRef) {
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
        let mut shape = ShapeBuilder::new(1);
        for i in 0..n {
            shape = shape.set(CellAddr::in_row("User", None, RowKey::name(&format!("R{i}")), "Value"), diacraft_doc::Cell::with_formula("2+3", None));
        }
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        (doc, SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0)))
    }

    fn shown(ctx: &Ctx<'_, '_>, i: u32) -> String {
        let addr = CellAddr::in_row("User", None, RowKey::name(&format!("R{i}")), "Value");
        view(ctx, &addr, "Value", None, None, &mut Counts::default()).unwrap().shown
    }

    #[test]
    fn the_steps_of_a_read_run_out_and_the_cells_past_them_show_as_they_did_before() {
        // Counted: what one cell costs is measured, and a read with the steps for two and a half shows the first two worked out and the rest with the default of their kind.
        let (doc, sheet) = doc_with_unsaved(6);
        let res = Resolver::new(&doc);
        let one = {
            let mut eval = read_eval_ctx();
            let ctx = Ctx::new(&res, &sheet, &mut eval);
            assert_eq!(shown(&ctx, 0), "5");
            drop(ctx);
            READ_UNSAVED_STEPS - eval.steps_left()
        };
        assert!(one > 0);
        let mut eval = EvalCtx::new(EvalOptions::default(), Budget::with_steps(one * 2 + one / 2));
        let ctx = Ctx::new(&res, &sheet, &mut eval);
        let shown: Vec<String> = (0..6).map(|i| shown(&ctx, i)).collect();
        assert_eq!(shown[..2], ["5", "5"], "{shown:?}");
        assert!(shown[2..].iter().all(|s| s == "0"), "past the steps, the default: {shown:?}");
    }
}
