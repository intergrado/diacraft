//! The real-file salt: the comparer is shown to compare on the cells of the file it scores, and not only on the synthetic control.
//!
//! **Why.** A calculator that hands back the saved values (an echo) agrees with every cell of a real file, and the fixed control the harness
//! scores first is one small synthetic document: the first recalculation score had 17 of 17 salted control cells caught while an echo scored 100%
//! on the 15.3 million cells of the real sample files. So every file is salted too. For a seeded sample of the real cells that **agreed**, the saved
//! value (or, for a cell compared by its error alone, the saved error) is put right out: a wrong value of its own kind, made so that it cannot
//! agree with what the calculator gave. The cell is then worked out again by the same calculator and compared by the same comparer. A salted cell
//! that is **caught** shows the comparer compares it; one that still agrees means the calculator reads the saved value in place of working it out
//! (an echo), or the comparer cannot tell the two apart, and the run fails.
//!
//! **What is salted.** The first agreed cell of each kind of value in a file (a number, a date, a Boolean, a text, a colour, a currency, a point,
//! a constructor, and an error), so every comparison has a cell, and one more in a thousand of the others, chosen from a fixed seed by the cell's
//! place in the walk (`SaltPolicy`). A file with a cell that agreed has at least one salted cell.
//!
//! **What is not counted as a miss.** Two allowances let a wrong value pass by design, and are counted apart (`RealSalt::by_allowance`): `ATAN2`'s
//! zero-length rule (any saved value agrees where both operands read under 1E-6), and the curve of an elliptical arc's
//! `C` and `D` (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`, `compare.rs`: the angle of a circle is no part of its curve, so a salted angle beside a ratio of 1
//! still names the same ellipse, and so does the other end of an axis). A salted `_ELLIPSE_*` cell of any other kind is caught, and an echo, which
//! agrees by raw value before any allowance is asked, misses every one.
//!
//! **What it costs.** The cells are chosen in the walk and worked out again after it, on the budget the walk left, and the counters of the
//! evaluation are put back after: the salt changes no score and cannot end a run as `budget_exhausted`. A cell whose value is too large to copy
//! is not salted and is counted (`RealSalt::too_large`).

use std::collections::BTreeMap;

use diacraft_doc::{CellAddr, SheetRef};
use diacraft_units::{Quantity, Unit};

use crate::cells::{CellEval, CellKey};
use crate::recalc::SheetPlace;
use crate::value::{CellError, Rgb, Value};

/// The most values the plan of a report holds: at one in a thousand that is a file of 20 million agreeing cells, ten times the most the reader keeps.
pub const MAX_PLANNED: usize = 20_000;

/// The most misses a report keeps, by place and kind, to say where the comparer failed.
pub const MAX_MISSES_KEPT: usize = 20;

/// The longest text or list a salted value is made from. A value past it is not salted.
const MAX_SALTED_ITEMS: usize = 4096;

/// How the real cells are chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaltPolicy {
    /// The seed of the choice: fixed, so a second run salts the same cells.
    pub seed: u64,
    /// One cell in this many is salted besides the first of each kind (at least 1).
    pub one_in: u64,
}

impl SaltPolicy {
    /// The salt of every scored run: one cell in a thousand, from a fixed seed.
    pub const STANDARD: SaltPolicy = SaltPolicy { seed: 0x5A17_0DD5_EED5_A17E, one_in: 1_000 };

    /// Whether the candidate numbered `index` (counting every candidate of the file from 0) is one of the sample.
    fn picks(&self, index: u64) -> bool {
        mix(self.seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15)).is_multiple_of(self.one_in.max(1))
    }
}

/// splitmix64: a well-spread 64-bit number from a 64-bit number.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The kind of value (or of comparison) a salted cell was compared by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SaltClass {
    Number,
    Date,
    Bool,
    Text,
    Colour,
    Currency,
    Point,
    Constructor,
    /// A cell compared by its error alone.
    Error,
}

impl SaltClass {
    /// Every class, once.
    pub const ALL: [SaltClass; 9] = [
        SaltClass::Number,
        SaltClass::Date,
        SaltClass::Bool,
        SaltClass::Text,
        SaltClass::Colour,
        SaltClass::Currency,
        SaltClass::Point,
        SaltClass::Constructor,
        SaltClass::Error,
    ];

    /// The word a report uses.
    pub fn word(self) -> &'static str {
        match self {
            SaltClass::Number => "number",
            SaltClass::Date => "date",
            SaltClass::Bool => "bool",
            SaltClass::Text => "text",
            SaltClass::Colour => "colour",
            SaltClass::Currency => "currency",
            SaltClass::Point => "point",
            SaltClass::Constructor => "constructor",
            SaltClass::Error => "error",
        }
    }

    fn slot(self) -> usize {
        SaltClass::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// What was put right out in a cell: the saved value and the saved error it is compared with in place of the file's.
#[derive(Clone, Debug)]
pub(crate) struct Salted {
    pub saved: Option<Value>,
    pub saved_error: Option<CellError>,
    pub class: SaltClass,
}

/// A cell chosen to be salted, and what it is salted with.
#[derive(Clone, Debug)]
pub(crate) struct Planned {
    pub key: CellKey,
    pub salted: Salted,
    /// The cell's own tolerance and colour step, so that it is compared as it was.
    pub tolerance: f64,
    pub colour_step: i32,
}

/// The choice of cells during a walk.
#[derive(Debug)]
pub(crate) struct SaltRun {
    policy: SaltPolicy,
    /// Candidates met so far: agreed cells that a wrong value can be made for.
    candidates: u64,
    first_met: [bool; 9],
    pub plan: Vec<Planned>,
    /// Candidates that no wrong value could be made for (too large).
    pub too_large: u64,
}

impl SaltRun {
    pub fn new(policy: SaltPolicy) -> SaltRun {
        SaltRun { policy, candidates: 0, first_met: [false; 9], plan: Vec::new(), too_large: 0 }
    }

    /// Offers an agreed cell: it joins the plan when it is the first of its kind or the sample picks it.
    pub fn offer(&mut self, sheet: &SheetRef, addr: &CellAddr, outcome: &CellEval, tolerance: f64, colour_step: i32) {
        let Some(class) = class_of(outcome) else { return };
        let index = self.candidates;
        self.candidates += 1;
        let slot = class.slot();
        let first = !self.first_met.get(slot).copied().unwrap_or(true);
        if !(first || self.policy.picks(index)) || self.plan.len() >= MAX_PLANNED {
            return;
        }
        match wrong_saved(outcome) {
            Some(salted) => {
                if let Some(met) = self.first_met.get_mut(slot) {
                    *met = true;
                }
                self.plan.push(Planned { key: (sheet.clone(), addr.clone()), salted, tolerance, colour_step });
            }
            None => self.too_large += 1,
        }
    }
}

/// How the salted cells came out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaltTally {
    pub salted: u64,
    pub caught: u64,
}

/// A salted cell that still agreed: where it is, by numbers and fixed words, and what it was compared by.
#[derive(Clone, Debug, PartialEq)]
pub struct SaltMiss {
    pub place: SheetPlace,
    pub address: CellAddr,
    pub functions: Vec<String>,
    pub class: SaltClass,
}

/// What salting the real cells of a file found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealSalt {
    /// Cells salted and worked out again: `salted` is `caught + by_allowance + missed`.
    pub salted: u64,
    /// Salted cells that differed from the calculator's result, as they must.
    pub caught: u64,
    /// Salted cells that agreed only through an allowance a wrong value can pass by design: `ATAN2`'s zero-length rule,
    /// which agrees with any saved value, and the curve of an elliptical arc's pair (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`), which agrees with the angle
    /// of a circle.
    pub by_allowance: u64,
    /// Salted cells that agreed with no allowance to say why: the comparer or the calculator reads the saved value in place of the result.
    pub missed: u64,
    /// Cells chosen and not worked out again, because the budget the walk left was spent.
    pub not_checked: u64,
    /// Agreed cells that were not salted because their value is too large to copy.
    pub too_large: u64,
    /// Salted and caught, by kind of value (every kind with a cell).
    pub by_class: BTreeMap<SaltClass, SaltTally>,
    /// The first misses, at most [`MAX_MISSES_KEPT`].
    pub misses: Vec<SaltMiss>,
}

impl RealSalt {
    /// Whether no salted cell agreed.
    pub fn passed(&self) -> bool {
        self.missed == 0
    }

    pub(crate) fn tally(&mut self, class: SaltClass, caught: bool) {
        let t = self.by_class.entry(class).or_default();
        t.salted += 1;
        t.caught += u64::from(caught);
    }

    /// Adds another file's salt to this one (a source's files).
    pub(crate) fn merge(&mut self, other: RealSalt) {
        self.salted += other.salted;
        self.caught += other.caught;
        self.by_allowance += other.by_allowance;
        self.missed += other.missed;
        self.not_checked += other.not_checked;
        self.too_large += other.too_large;
        for (class, t) in other.by_class {
            let e = self.by_class.entry(class).or_default();
            e.salted += t.salted;
            e.caught += t.caught;
        }
        for miss in other.misses {
            if self.misses.len() < MAX_MISSES_KEPT {
                self.misses.push(miss);
            }
        }
    }
}

/// A number the comparer cannot take for the one it was: further from it than any tolerance of the comparer, which is relative to its size
/// above 1, so a distance of its size or of 1. A number that is not finite becomes 0, which agrees with no non-finite
/// number. Where the other side is a colour, the number is an index and a fraction is no index: it moves by a half.
fn away(x: f64, computed: &Value) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    if matches!(computed, Value::Color(_)) {
        return x + 0.5;
    }
    x + x.abs().max(1.0)
}

/// An error that is not `e`.
fn other_error(e: CellError) -> CellError {
    let all = CellError::ALL;
    let at = all.iter().position(|x| *x == e).unwrap_or(0);
    all.get((at + 1) % all.len()).copied().unwrap_or(CellError::Na)
}

fn moved(q: &Quantity, computed: &Value) -> Quantity {
    let mut m = *q;
    m.stored = away(q.stored, computed);
    m
}

/// What an agreed cell was compared by, or `None` when it is no candidate (it was compared by neither a typed value nor its error).
fn class_of(outcome: &CellEval) -> Option<SaltClass> {
    match (&outcome.raw, &outcome.saved) {
        (Value::Error(_), _) => (outcome.error.is_some() && outcome.saved_error.is_some()).then_some(SaltClass::Error),
        (Value::Unsupported(_), _) | (_, None | Some(Value::Unsupported(_))) => None,
        (_, Some(Value::Number(q))) if q.unit == Unit::Date => Some(SaltClass::Date),
        (_, Some(Value::Number(_))) => Some(SaltClass::Number),
        (_, Some(Value::Bool(_))) => Some(SaltClass::Bool),
        (_, Some(Value::Text(_))) => Some(SaltClass::Text),
        (_, Some(Value::Color(_))) => Some(SaltClass::Colour),
        (_, Some(Value::Currency { .. })) => Some(SaltClass::Currency),
        (_, Some(Value::Point(_))) => Some(SaltClass::Point),
        (_, Some(Value::Constructor(_))) => Some(SaltClass::Constructor),
        (_, Some(Value::Sheet(_) | Value::Error(_))) => None,
    }
}

/// The wrong value for an agreed cell, of the kind the cell was compared by, or `None` when the cell is no candidate or its value is too large.
/// The value is made from the saved one, to differ from it and from what the calculator gave (`computed`) by more than any rule of the comparer.
pub(crate) fn wrong_saved(outcome: &CellEval) -> Option<Salted> {
    class_of(outcome)?;
    match (&outcome.raw, &outcome.saved) {
        // Compared by the error alone: the file's error is another.
        (Value::Error(_), _) => {
            let saved_error = outcome.saved_error.map(other_error)?;
            Some(Salted { saved: outcome.saved.clone(), saved_error: Some(saved_error), class: SaltClass::Error })
        }
        (computed, Some(saved)) => {
            let (wrong, class) = wrong_value(computed, saved)?;
            Some(Salted { saved: Some(wrong), saved_error: outcome.saved_error, class })
        }
        _ => None,
    }
}

fn wrong_value(computed: &Value, saved: &Value) -> Option<(Value, SaltClass)> {
    Some(match saved {
        // A date holds whole seconds in a file and agrees within one: a day further is no date of the same instant.
        Value::Number(q) if q.unit == Unit::Date => {
            let mut m = *q;
            m.stored = if q.stored.is_finite() { q.stored + 1.0 } else { 0.0 };
            (Value::Number(m), SaltClass::Date)
        }
        Value::Number(q) => (Value::Number(moved(q, computed)), SaltClass::Number),
        Value::Bool(b) => (Value::Bool(!*b), SaltClass::Bool),
        // A text that no number reads as and that is not the text: it is exact against a text, and no number against a number.
        Value::Text(t) => (Value::text(if &**t == "\u{1}" { "\u{2}" } else { "\u{1}" }), SaltClass::Text),
        // Every channel half a range off: past any step the comparer allows, and no palette entry it was.
        Value::Color(c) => (Value::Color(Rgb { r: c.r ^ 0x80, g: c.g ^ 0x80, b: c.b ^ 0x80 }), SaltClass::Colour),
        Value::Currency { amount, currency } => (Value::Currency { amount: away(*amount, computed), currency: *currency }, SaltClass::Currency),
        Value::Point(p) => {
            let mut m = p.clone();
            m.x = moved(&p.x, computed);
            m.y = moved(&p.y, computed);
            (Value::Point(m), SaltClass::Point)
        }
        Value::Constructor(c) => {
            if c.numbers.len() > MAX_SALTED_ITEMS {
                return None;
            }
            let mut m = c.clone();
            // An empty constructor gains a number: the counts differ.
            if m.numbers.is_empty() {
                m.numbers.push(1.0);
            }
            for n in &mut m.numbers {
                *n = away(*n, computed);
            }
            (Value::Constructor(m), SaltClass::Constructor)
        }
        Value::Sheet(_) | Value::Error(_) | Value::Unsupported(_) => return None,
    })
}
