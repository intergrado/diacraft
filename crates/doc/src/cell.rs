//! A cell: a formula and a cached value with a unit, as a file holds them.
//!
//! What a cell *owns*: a local formula, and a local value. A cell with `F="Inh"` owns no
//! formula, and a value it holds beside `Inh` is a cached result of evaluating the formula it inherits, on this sheet
//! The model keeps that as the file did: [`FormulaSlot::Inherit`] and the value, with no copy of the source's formula
//! text anywhere. [`Cell::origin`] says which of the cases a cell is.

use std::sync::Arc;

use diacraft_formula::{Expr, Formula};

use crate::package::{Ext, Kept};
use crate::raw::{Passthrough, RawAttrs};
use crate::value::Value;

/// What a cell's `F` attribute said.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FormulaSlot {
    /// No `F`: no formula here. With a `V` the cell is a local constant; the search for a formula ends at it.
    #[default]
    Absent,
    /// A formula of its own, whether or not it parses: its text is `source`, verbatim.
    Local(Arc<Formula>),
    /// `F="Inh"`: the formula is inherited. Nothing here ends the search.
    Inherit,
    /// `F="No Formula"`: no formula, deleted or blocked locally. The search ends at it (keeps the marker).
    NoFormula,
}

/// The marker `F` holds for an inherited formula.
pub const INHERIT_MARKER: &str = "Inh";
/// The marker `F` holds for no formula.
pub const NO_FORMULA_MARKER: &str = "No Formula";

impl FormulaSlot {
    /// What an `F` attribute holds. The two markers are the exact words; anything else is a formula, parsed with what is left of
    /// `budget` nodes (`formula::parse_with_budget`: a document keeps a total of its own) and taken off it. The text is kept
    /// as given, and a formula that does not parse, or that the budget cannot afford, is kept just the same.
    pub fn read(f: &str, budget: &mut usize) -> FormulaSlot {
        match f {
            INHERIT_MARKER => FormulaSlot::Inherit,
            NO_FORMULA_MARKER => FormulaSlot::NoFormula,
            source => {
                let formula = diacraft_formula::parse_with_budget(source, *budget);
                *budget = budget.saturating_sub(formula.node_count());
                FormulaSlot::Local(Arc::new(formula))
            }
        }
    }

    /// The text `F` holds: the marker's words, or the formula's own text. `None` when there is no `F`.
    pub fn text(&self) -> Option<&str> {
        match self {
            FormulaSlot::Absent => None,
            FormulaSlot::Local(f) => Some(&f.source),
            FormulaSlot::Inherit => Some(INHERIT_MARKER),
            FormulaSlot::NoFormula => Some(NO_FORMULA_MARKER),
        }
    }
}

/// What a cell owns, as and the brief of the model describe it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A local formula, a `No Formula` marker, or no formula and a value of its own: the search ends here.
    Local,
    /// `F="Inh"` with a `V` of its own: the cached result of the inherited formula, evaluated on this sheet.
    InheritedWithCachedValue,
    /// `F="Inh"` with no `V` (943 cells directly under a shape in the census, 703 in rows): the source's value stands.
    Inherited,
}

/// The things a cell rarely has, kept out of the cell so that the many cells that have none stay small.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellExtra {
    /// Text inside the element, beside or instead of `V`: `<Cell N="Copyright">Some text</Cell>`.
    pub text: Option<Arc<str>>,
    /// Elements, comments and processing instructions inside the cell: a `RefBy`, `<?NewValue V?>` (R12 of the preservation spec).
    pub passthrough: Passthrough,
    /// What the element holds that no field reads, tagged by format; see [`crate::package`]. It lives here, in the part of a cell that few have, so that
    /// the slot costs a cell nothing.
    pub ext: Ext<Kept>,
}

/// A cell. Cloning one copies a few reference counts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    /// `F`: see [`FormulaSlot`].
    pub formula: FormulaSlot,
    /// `V`, as text, if there is one. No `V` is a malformed cell with a default of its own.
    pub value: Option<Value>,
    /// `U`, verbatim: a unit the model knows (`IN`, `MM`, `DEG`) or any other text (`NURBS`, `POLYLINE`, `GUID`). Read it as
    /// a unit with [`diacraft_units::parse_unit`] when it is one.
    pub unit: Option<Arc<str>>,
    /// `E`, verbatim: the error of the last evaluation. `V` then holds the last good value.
    pub error: Option<Arc<str>>,
    /// Attributes the model has no field for, in file order.
    pub raw: RawAttrs,
    /// Text and child nodes, if any. **`Some` holds something**: the readers (`sheetxml`'s) make it only for a cell that has text or child nodes, and no
    /// command makes `Some(CellExtra::default())`. The derived equality would tell that from `None`, so a writer that builds one empty is wrong, and
    /// the reader's rule is pinned by a test (`crates/sheetxml/tests/it/reading.rs`).
    pub extra: Option<Arc<CellExtra>>,
}

impl Cell {
    /// An empty cell: no formula, no value.
    pub fn new() -> Cell {
        Cell::default()
    }

    /// A cell from the four attributes a file gives it. `f` is parsed as [`FormulaSlot::read`] does, with `budget`. Nothing is
    /// normalised: `v`, `u` and `e` are kept as the strings they are.
    pub fn read(f: Option<&str>, v: Option<&str>, u: Option<&str>, e: Option<&str>, budget: &mut usize) -> Cell {
        Cell {
            formula: f.map_or(FormulaSlot::Absent, |f| FormulaSlot::read(f, budget)),
            value: v.map(Value::from_text),
            unit: u.map(Arc::from),
            error: e.map(Arc::from),
            raw: RawAttrs::new(),
            extra: None,
        }
    }

    /// A local constant: a value and no formula.
    pub fn constant(value: Value) -> Cell {
        Cell { value: Some(value), ..Cell::default() }
    }

    /// A local formula, parsed with the default limits of the parser, and its value.
    pub fn with_formula(source: &str, value: Option<Value>) -> Cell {
        let formula = diacraft_formula::parse(source);
        Cell { formula: FormulaSlot::Local(Arc::new(formula)), value, ..Cell::default() }
    }

    /// An inherited formula (`F="Inh"`) with the result it was last evaluated to on this sheet, if there is one.
    pub fn inherited(value: Option<Value>) -> Cell {
        Cell { formula: FormulaSlot::Inherit, value, ..Cell::default() }
    }

    /// The same cell with unit `u`.
    pub fn with_unit(mut self, u: &str) -> Cell {
        self.unit = Some(Arc::from(u));
        self
    }

    /// What the cell owns: see [`Origin`].
    pub fn origin(&self) -> Origin {
        match (&self.formula, &self.value) {
            (FormulaSlot::Inherit, Some(_)) => Origin::InheritedWithCachedValue,
            (FormulaSlot::Inherit, None) => Origin::Inherited,
            _ => Origin::Local,
        }
    }

    /// Whether the cell holds a formula of its own, which is what only a local cell can.
    pub fn owns_formula(&self) -> bool {
        matches!(self.formula, FormulaSlot::Local(_))
    }

    /// Whether the search for a formula ends at this cell: step 1. An expression, `No Formula` and no `F`
    /// end it; `Inh` does not. (reports one reader treating a cell with only a `U` as contributing
    /// nothing; the spec's rule is followed here, and this is the one place to change it.)
    pub fn ends_search(&self) -> bool {
        !matches!(self.formula, FormulaSlot::Inherit)
    }

    /// Whether the cell's formula is `GUARD(...)`: its outermost call is `GUARD`. A formula that does not
    /// parse, a marker and a plain value are not guarded.
    pub fn is_guarded(&self) -> bool {
        match &self.formula {
            FormulaSlot::Local(f) => matches!(&f.ast, Ok(Expr::Call(call)) if call.is("GUARD")),
            _ => false,
        }
    }

    /// The text inside the element, if any.
    pub fn text(&self) -> Option<&str> {
        self.extra.as_ref().and_then(|e| e.text.as_deref())
    }
}
