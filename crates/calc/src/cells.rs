//! Reading a cell: its saved value, its formula, and what the two make.
//!
//! **A cell is worked out on the inheritor.** [`diacraft_doc::Resolver::resolve`] finds the cell that holds the formula (a local one, or
//! the master's, or a style's) and the sheet to run it on, and this module runs it there. The saved value
//! is the nearest `V` on the way; a failing cell reads at its last good value, which is that `V`.
//!
//! **What a reference reads** ([`CellEval::view`]): the new value when the formula worked, else the saved one. A formula that fails
//! keeps the saved value for readers and holds its error for the error-testing functions ([`CellEval::error`]). A function this
//! evaluator does not have, a formula that does not parse and an action cell (`Action` and its kin) read as the saved
//! value too, since drawing uses it (`Value::Unsupported` carries the name up to the oracle).

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, FormulaSlot, Lookup, Resolved, SheetRef, Source};
use diacraft_units::Unit;

use crate::eval::{CellAddress, Env, Evaluator, OperandMode};
use crate::rules::{CELL_HOP_DEPTH, KEY_BYTES, LOOP_ERROR, MEMO_ENTRY_BYTES};
use crate::store::decode;
use crate::value::{CellError, Point, Rgb, Value};
use diacraft_doc::ValueKind;
use diacraft_units::Quantity;

/// A cell as a map key.
pub(crate) type CellKey = (SheetRef, CellAddr);

/// The most cells the memo of an evaluation holds. A project rule: past it a cell is worked out again by each reader, which the budget
/// bounds; the memo of a hostile document does not grow without end.
pub(crate) const MAX_MEMO_ENTRIES: usize = 2_000_000;

/// What kind of cell it is, as far as evaluating it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// A formula that was worked out.
    Formula,
    /// A value and no formula, or no `F` at all.
    Constant,
    /// `No Formula`: no formula, deleted or blocked locally.
    NoFormula,
    /// A formula that does not parse, so its saved value stands.
    ParseFailed,
    /// `Action`, `EventDblClick` and the rest: never worked out.
    Action,
    /// No such cell, and the default of stands.
    Missing,
}

/// What reading one cell gave.
#[derive(Clone, Debug)]
pub struct CellEval {
    pub kind: CellKind,
    /// What the formula gave, an error or an unsupported function included; for a cell with no formula, its saved value or default.
    pub raw: Value,
    /// What a reference to the cell reads.
    pub view: Value,
    /// The error state an error-testing function sees: this evaluation's error, or the `E` the file holds.
    pub error: Option<CellError>,
    /// The function that is not supported, when the formula reached one.
    pub unsupported: Option<Arc<str>>,
    /// Whether any cell was found on the way (a local one, the master's or a style's): a row nothing supplies has none.
    pub found: bool,
    /// The saved value as typed, if the file holds a `V`. `Unsupported(THEMED_MARKER)` for the theme marker, `Unsupported(FAR_YEAR)` for a
    /// date past the year 9999.
    pub saved: Option<Value>,
    /// The error the file holds in `E`.
    pub saved_error: Option<CellError>,
    /// Whether the evaluation reached a clock-reading function.
    pub tainted: bool,
    /// Whether the evaluation made a text measure in a stand-in face that is not the name's open metric twin, in this cell or in a cell it
    /// reads: its widths differ from the original's, so a score leaves the cell out and says so.
    pub stand_in: bool,
    /// Whether the cell's own formula reached a `REF()`, or a reference to a cell its section can never hold that found nothing: a formula
    /// that can only fail there. A cell it reads does not count.
    pub must_fail: bool,
    /// Whether the evaluation, or a cell it read, took the no-shadow branch of a theme.
    pub no_shadow: bool,
    /// The `V` the file holds, as the document keeps it (text, shared), and its `U`: what a commit keeps when a result agrees.
    pub saved_value: Option<diacraft_doc::Value>,
    pub saved_unit: Option<Arc<str>>,
    /// Whether the formula that ran is the default one a cell's page prints, because nothing holds a formula for the cell.
    pub default_formula: bool,
    /// Whether the cell has a unit of its own kind (`Width`, `Angle`...) and not a free one (`User`, `Prop`, `Scratch`): a free cell holds the
    /// unit of a length it is given in `U`, a typed one does not.
    pub typed: bool,
    /// How many times the native depth limit was reached while the cell was worked out (the cells it read included).
    pub depth_limit_hits: u64,
}

impl CellEval {
    /// A cell that gave an error and holds nothing: `found` is false, so a row that is not there reads `#REF!`.
    pub(crate) fn failed(e: CellError) -> CellEval {
        CellEval {
            kind: CellKind::Missing,
            raw: Value::Error(e),
            view: Value::Error(e),
            error: Some(e),
            unsupported: None,
            found: false,
            saved: None,
            saved_error: None,
            tainted: false,
            stand_in: false,
            must_fail: false,
            no_shadow: false,
            saved_value: None,
            saved_unit: None,
            default_formula: false,
            typed: false,
            depth_limit_hits: 0,
        }
    }

    /// A cell that is there and could not be worked out: a cycle, a spent budget. It is found, so a reference to it reads the error the
    /// rule gives (`rules::LOOP_ERROR`), a row cell included, and not `#REF!`.
    pub(crate) fn looped(e: CellError) -> CellEval {
        CellEval { found: true, ..CellEval::failed(e) }
    }
}

/// The unit a number with no `U` has in a cell that calls for a measure: the default unit of its kind.
fn default_unit(kind: ValueKind) -> Option<Unit> {
    match kind {
        ValueKind::Length => Some(Unit::DrawingDefault),
        ValueKind::Angle => Some(Unit::AngleDefault),
        _ => None,
    }
}

/// The cells that run on an event, whose saved `V` is state and not a result.
pub(crate) fn is_action_cell(addr: &CellAddr) -> bool {
    match &addr.section {
        None => matches!(addr.cell.as_str(), "EventDblClick" | "EventDrop" | "EventMultiDrop" | "EventXFMod" | "TheData" | "TheText"),
        Some(section) => section.name == "Actions" && addr.cell == "Action",
    }
}

impl Evaluator<'_, '_> {
    /// The saved value and error of what [`diacraft_doc::Resolver::resolve`] found. Decoding a `V` costs its length, charged first.
    fn stored_parts(&mut self, res: &Resolved<'_>) -> (Option<Value>, Option<CellError>) {
        let vcell = res.value.cell;
        let fcell = res.formula.cell;
        let saved = vcell.and_then(|c| {
            let v = c.value.as_ref()?;
            if v.is_themed() {
                return Some(Value::unsupported(THEMED_MARKER));
            }
            let unit = c.unit.as_deref().or_else(|| fcell.and_then(|f| f.unit.as_deref()));
            let kind = res.default.kind;
            if v.text().is_empty() && !matches!(unit, Some(u) if u.trim().eq_ignore_ascii_case("STR")) && kind != ValueKind::Text {
                return None;
            }
            if !self.work(v.text().len()) {
                return None;
            }
            if is_far_year(v.text(), unit) {
                return Some(Value::unsupported(FAR_YEAR));
            }
            Some(decode(v.text(), unit, default_unit(kind)))
        });
        let error = vcell.or(fcell).and_then(|c| c.error.as_deref()).and_then(CellError::from_text);
        (saved, error)
    }

    /// The cell the sheet itself holds at `addr`, which is nothing of what it inherits.
    pub(crate) fn local_cell(&self, sheet: &SheetRef, addr: &CellAddr) -> Option<&Cell> {
        // Through the resolver's indexes, as every other lookup is: the document's own lookups scan the pages and the masters.
        let data = self.res.sheet(sheet)?;
        match self.res.lookup(data, addr) {
            Lookup::Found(cell) => Some(cell),
            Lookup::Missing | Lookup::Deleted => None,
        }
    }

    /// What a cell is when nothing holds a value: by the unit of the cell found, else by the kind of the
    /// cell's type.
    fn default_value(&self, res: &Resolved<'_>) -> Value {
        match res.kind_if_no_value() {
            ValueKind::Scalar => Value::num(0.0),
            ValueKind::Length => Value::Number(Quantity::new(0.0, Unit::DrawingDefault)),
            ValueKind::Angle => Value::Number(Quantity::new(0.0, Unit::AngleDefault)),
            ValueKind::Days => Value::Number(Quantity::new(0.0, Unit::DurationDefault)),
            ValueKind::Color => Value::Color(Rgb { r: 0, g: 0, b: 0 }),
            ValueKind::Text => Value::text(""),
            ValueKind::Font => Value::text("0"),
            ValueKind::Point => Value::Point(Box::new(Point {
                x: Quantity::new(0.0, Unit::DrawingDefault),
                y: Quantity::new(0.0, Unit::DrawingDefault),
                frame: None,
                mixed: false,
            })),
        }
    }

    /// Reads one cell as the operand mode says: as saved (`Cached`), or worked out and memoised (`Live`).
    pub(crate) fn read_cell(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Arc<CellEval> {
        if !self.steps(1) {
            return Arc::new(CellEval::looped(LOOP_ERROR));
        }
        match self.ctx.opts.operands {
            OperandMode::Cached => Arc::new(self.compute_cell(sheet, addr, false)),
            OperandMode::Live => self.live_cell(sheet, addr),
        }
    }

    /// A cell worked out once and remembered, a cycle being an error.
    fn live_cell(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Arc<CellEval> {
        let key: CellKey = (sheet.clone(), addr.clone());
        if let Some(known) = self.ctx.memo.get(&key) {
            if known.tainted {
                self.ctx.clock_touched = true;
            }
            if known.stand_in {
                self.ctx.stand_in_touched = true;
            }
            if known.no_shadow {
                self.ctx.no_shadow_touched = true;
            }
            // A cell that was cut short by the depth limit is cut short for each cell that reads it, as the clock is carried above.
            self.ctx.depth_seen += known.depth_limit_hits;
            return Arc::clone(known);
        }
        if self.ctx.in_progress.contains(&key) {
            self.ctx.stats.cycles += 1;
            return Arc::new(CellEval::looped(LOOP_ERROR));
        }
        self.ctx.in_progress.insert(key.clone());
        let out = Arc::new(self.compute_cell(sheet, addr, true));
        self.ctx.in_progress.remove(&key);
        // The memo is held memory: the entry and the text it holds are accounted, and past the budget's memory the run ends.
        let cost = MEMO_ENTRY_BYTES + KEY_BYTES + out.raw.heap_bytes() + out.saved.as_ref().map_or(0, Value::heap_bytes);
        if self.ctx.memo.len() < MAX_MEMO_ENTRIES && self.ctx.hold_bytes(cost) {
            self.ctx.memo.insert(key, Arc::clone(&out));
        }
        out
    }

    /// Works one cell out. With `run` false the cell's own formula is not run and its saved value stands (the cell as an operand in
    /// `Cached` mode); a default formula still runs, as it is what a missing cell reads.
    pub(crate) fn compute_cell(&mut self, sheet: &SheetRef, addr: &CellAddr, run: bool) -> CellEval {
        let Some(res) = self.res.resolve(sheet, addr) else {
            // A row or section deleted here, or a sheet that is not there.
            return CellEval::failed(CellError::Ref);
        };
        let (saved, mut saved_error) = self.stored_parts(&res);
        // The error a cell holds is its own last evaluation's. When the cell has no `V` of its own the stored value is the master's or a
        // style's, and the nearest `V` is not where the error is: a cell saved as `Inh` with an `E` and no `V` holds that `E` itself (the
        // review's I6). Looked up only for the cell being worked out, which is the one the oracle compares.
        if run
            && res.value.source != Source::Local
            && let Some(local) = self.local_cell(sheet, addr)
        {
            saved_error = local.error.as_deref().and_then(CellError::from_text);
        }
        // The real-file salt (`salt.rs`): the cell under test, and no read of it as an operand, has its saved value and error put right out.
        let (saved, saved_error) = match &self.ctx.salt {
            Some((key, salted)) if run && key.0 == *sheet && key.1 == *addr => (salted.saved.clone(), salted.saved_error),
            _ => (saved, saved_error),
        };
        let found = res.formula.cell.is_some() || res.value.cell.is_some();
        let saved_value = res.value.cell.and_then(|c| c.value.clone());
        let saved_unit = res.value.cell.and_then(|c| c.unit.clone()).or_else(|| res.formula.cell.and_then(|c| c.unit.clone()));
        // `User.Value` and `Prop.Value` are free cells, which keep the unit they are given: their `vAny` kind
        // gives a missing one 0 days and them no unit of their own (they were unlisted, and so untyped, until their batch was catalogued).
        let typed = matches!(res.default.kind, ValueKind::Length | ValueKind::Angle | ValueKind::Days) && !is_free_value(addr);
        let stored_view = |this: &Self| saved.clone().unwrap_or_else(|| this.default_value(&res));
        let base = |kind: CellKind, view: Value| CellEval {
            kind,
            raw: view.clone(),
            view,
            error: saved_error,
            unsupported: None,
            found,
            saved: saved.clone(),
            saved_error,
            tainted: false,
            stand_in: false,
            must_fail: false,
            no_shadow: false,
            saved_value: saved_value.clone(),
            saved_unit: saved_unit.clone(),
            default_formula: false,
            typed,
            depth_limit_hits: 0,
        };
        if is_action_cell(addr) {
            return base(CellKind::Action, stored_view(self));
        }
        let formula = match res.formula.cell.map(|c| &c.formula) {
            Some(FormulaSlot::Local(f)) => Some(Arc::clone(f)),
            Some(_) => None,
            // Nothing found: a few cells read the formula their page prints (`Resolved::default`).
            None => res.default.formula.map(|text| Arc::new(diacraft_formula::parse(text))),
        };
        let Some(formula) = formula else {
            let kind = match res.formula.cell.map(|c| &c.formula) {
                Some(FormulaSlot::NoFormula) => CellKind::NoFormula,
                Some(_) => CellKind::Constant,
                None => CellKind::Missing,
            };
            return base(kind, stored_view(self));
        };
        let is_default_formula = res.formula.cell.is_none();
        if !run && !is_default_formula {
            return base(CellKind::Formula, stored_view(self));
        }
        let Ok(expr) = &formula.ast else {
            return base(CellKind::ParseFailed, stored_view(self));
        };
        let mut env = Env::new(res.on.clone(), res.rebind.clone());
        env.cell = Some(addr.clone());
        let outer_clock = std::mem::take(&mut self.ctx.clock_touched);
        let outer_stand_in = std::mem::take(&mut self.ctx.stand_in_touched);
        let outer_fail = std::mem::take(&mut self.ctx.must_fail_touched);
        let outer_shadow = std::mem::take(&mut self.ctx.no_shadow_touched);
        let depth_before = self.ctx.depth_seen;
        self.ctx.stats.cells_worked = self.ctx.stats.cells_worked.saturating_add(1);
        // Reading a cell in place weighs [`CELL_HOP_DEPTH`] levels of the native depth.
        let raw = if self.enter_n(CELL_HOP_DEPTH) {
            let raw = self.eval(expr, &env);
            self.leave_n(CELL_HOP_DEPTH);
            raw
        } else {
            Value::Error(LOOP_ERROR)
        };
        let depth_limit_hits = self.ctx.depth_seen - depth_before;
        let tainted = self.ctx.clock_touched;
        self.ctx.clock_touched = outer_clock || tainted;
        let stand_in = self.ctx.stand_in_touched;
        self.ctx.stand_in_touched = outer_stand_in || stand_in;
        let raw = self.fit_to_cell(raw, &res, addr);
        let raw = clipping_path_result(raw, addr);
        // A formula that can only fail is the cell's own: what a cell it read reached is not carried. The no-shadow branch is, as the clock is.
        let must_fail = std::mem::replace(&mut self.ctx.must_fail_touched, outer_fail);
        let no_shadow = self.ctx.no_shadow_touched;
        self.ctx.no_shadow_touched = outer_shadow || no_shadow;
        let (view, error, unsupported) = match &raw {
            // A failing formula leaves its last good value to readers: the saved one. With none, the error itself.
            Value::Error(e) => (saved.clone().unwrap_or_else(|| raw.clone()), Some(*e), None),
            Value::Unsupported(name) => (saved.clone().unwrap_or_else(|| raw.clone()), saved_error, Some(Arc::clone(name))),
            v => (v.clone(), None, None),
        };
        CellEval {
            kind: CellKind::Formula,
            raw,
            view,
            error,
            unsupported,
            found,
            saved,
            saved_error,
            tainted,
            stand_in,
            must_fail,
            no_shadow,
            saved_value,
            saved_unit,
            default_formula: is_default_formula,
            typed,
            depth_limit_hits,
        }
    }

    /// A result fitted to the cell that holds it: a bare number takes the unit the cell last held, or its
    /// default. Only the unit changes, never the stored number.
    pub(crate) fn fit_to_cell(&mut self, v: Value, res: &Resolved<'_>, addr: &CellAddr) -> Value {
        if let Value::Point(p) = &v {
            return fit_point(p, addr);
        }
        // A string read into a colour cell is the colour it spells as a formula (project rule).
        if res.default.kind == ValueKind::Color
            && let Value::Text(s) = &v
        {
            let env = Env::new(res.on.clone(), res.rebind.clone());
            match self.eval_string_formula(s, &env) {
                Some(Value::Color(c)) => return Value::Color(c),
                // A function this evaluator does not have, read from a string, is unknown and not "not a colour".
                Some(u @ Value::Unsupported(_)) => return u,
                _ => return v,
            }
        }
        let Value::Number(q) = v else { return v };
        if q.unit != Unit::Unitless || q.dim() != 0 {
            return Value::Number(q);
        }
        let held = res.value.cell.or(res.formula.cell).and_then(|c| c.unit.as_deref()).and_then(diacraft_units::parse_unit);
        match held.or_else(|| default_unit(res.default.kind)) {
            Some(unit) if unit.measure() != diacraft_units::Measure::None => Value::Number(Quantity::new(q.stored, unit)),
            _ => Value::Number(q),
        }
    }

    /// Works one cell out and says what it is: the entry point of the oracle and of a recalculation. In `Live` mode the cells it reads
    /// are worked out first, a chain of them with an explicit stack and not by recursion, and remembered.
    pub fn evaluate_cell(&mut self, at: &CellAddress) -> Arc<CellEval> {
        match self.ctx.opts.operands {
            OperandMode::Cached => Arc::new(self.compute_cell(&at.sheet, &at.addr, true)),
            OperandMode::Live => {
                // A cell with the salt in it is worked out afresh and not remembered: its operands were worked out and are in the memo.
                if self.ctx.salts(&at.sheet, &at.addr) {
                    return Arc::new(self.compute_cell(&at.sheet, &at.addr, true));
                }
                self.prewarm(&at.sheet, &at.addr);
                self.live_cell(&at.sheet, &at.addr)
            }
        }
    }

    /// Whether the budget of the evaluation has run out (of steps or of memory): everything worked out after that is an error and not a result.
    pub fn exhausted(&self) -> bool {
        self.ctx.stats.budget_exhausted
    }

    /// The value of a cell: what `evaluate_cell` gives, as [`Value`]. An error is the cell's evaluation error, and an unsupported
    /// function [`Value::Unsupported`], the saved value standing in drawing. A cell with no formula, one whose formula does not parse
    /// and an action cell give their saved value.
    pub fn evaluate(&mut self, at: &CellAddress) -> Value {
        self.evaluate_cell(at).raw.clone()
    }
}

/// Whether a text spells a path of the picture's shape: `Geometry`, a number from 1 and `.Path`, exact in case and with nothing before or after
/// Whether the shape has that section is not asked here.
fn spells_a_path(text: &str) -> bool {
    let Some(number) = text.strip_prefix("Geometry").and_then(|rest| rest.strip_suffix(".Path")) else { return false };
    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && number.parse::<u32>().is_ok_and(|n| n >= 1)
}

/// `ClippingPath` evaluated: a string that is not a path is `#N/A`, with `V` left empty, and one
/// that spells a path is that string and no error. Only the empty string is attested on the error side (1,656 cells, all saving `E` `#N/A`); that
/// other text gives `#N/A` too is the rule's own reasoning. A result that is no string is left alone.
pub(crate) fn clipping_path_result(v: Value, addr: &CellAddr) -> Value {
    if addr.section.is_none() && addr.row.is_none() && addr.cell.as_str() == "ClippingPath" && matches!(&v, Value::Text(t) if !spells_a_path(t)) {
        return Value::Error(CellError::Na);
    }
    v
}

/// Whether a cell is a free one, which has no unit of its own and keeps a point whole: `User.Value`, `Prop.Value` and `Scratch.A` to `Scratch.D`
/// (the definition the batches share). The section is matched by the name the model gives it
/// (`catalogue::SECTION_NAMES`), not by the spelling a formula uses (`refs.rs`: the formula's `Prop` is the section `Property`). The other
/// cells of those rows are text (`User.Prompt`, `Property.Label`, `Format` and the rest) or a length
/// (`Scratch.X`, `Scratch.Y`), and gives a point in them its part or `#VALUE!`.
fn is_free_cell(section: Option<&str>, cell: &str) -> bool {
    matches!((section, cell), (Some("User" | "Property"), "Value") | (Some("Scratch"), "A" | "B" | "C" | "D"))
}

/// Whether a cell is the `Value` of a `User` or `Property` row: a free cell whose kind (`vAny`) types none of what it holds.
fn is_free_value(addr: &CellAddr) -> bool {
    matches!((addr.section.as_ref().map(|s| s.name.as_str()), addr.cell.as_str()), (Some("User" | "Property"), "Value"))
}

/// A point that a formula gives, in the cell that holds it: `BeginX`, `EndX`, `PinX`,
/// `TxtPinX` and `X` keep x (`Scratch.X` too), the same with `Y` keep y, and `A` and `B` of a `Geometry` row keep x and y; a
/// free cell keeps the whole point; a point in any other (typed) cell is `#VALUE!` (refuses a point as a number).
fn fit_point(p: &Point, addr: &CellAddr) -> Value {
    let section = addr.section.as_ref().map(|s| s.name.as_str());
    let cell = addr.cell.as_str();
    let coordinate = |q: Quantity| {
        if q.unit == Unit::Unitless { Value::Number(Quantity::new(q.stored, Unit::DrawingDefault)) } else { Value::Number(q) }
    };
    if is_free_cell(section, cell) {
        return Value::Point(Box::new(p.clone()));
    }
    match (section, cell) {
        (_, "BeginX" | "EndX" | "PinX" | "TxtPinX" | "X") | (Some("Geometry"), "A") => coordinate(p.x),
        (_, "BeginY" | "EndY" | "PinY" | "TxtPinY" | "Y") | (Some("Geometry"), "B") => coordinate(p.y),
        _ => Value::Error(CellError::Value),
    }
}

/// The marker a saved date past the year 9999 decodes to: the oracle leaves such a cell out. **Not a name a
/// function can have**, so a formula calling a function of that name (an unsupported function is `Unsupported(NAME)`) is never taken for a cell
/// that read such a date.
pub(crate) const FAR_YEAR: &str = "<date past 9999>";

/// The marker a saved `Themed` decodes to: no value, so a cell that reads it as an operand is not worked out. Not a
/// name a function can have, as [`FAR_YEAR`] is not.
pub(crate) const THEMED_MARKER: &str = "<themed>";

/// Whether a saved `V` of unit `DATE` holds a year of five digits or more (7 cells hold `30177-04-13`).
fn is_far_year(text: &str, unit: Option<&str>) -> bool {
    if !matches!(unit, Some(u) if u.trim().eq_ignore_ascii_case("DATE")) {
        return false;
    }
    let t = text.trim();
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    digits >= 5 && t.as_bytes().get(digits) == Some(&b'-')
}
