//! The calc crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic. The vectors are those of and the `functions-*.md`
//! batches, written as tables of `(vector id, formula, expected value)` so that a row names the vector it comes from. A formula is worked out
//! as the one formula of a probe cell (`User.Probe`) on a shape of a page, built by the testkit, with the cells the vector says around it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod cells_sheets;
mod colour;
mod colour_exhaustive;
mod colour_hex;
mod commit_cases;
mod date_time;
mod doc_info;
mod ellipse;
mod empty_text;
mod geometry;
mod hostile;
mod hostile_lookups;
mod hostile_shapes;
mod inheritance;
mod language;
mod linked_reads;
mod logic;
mod marker;
mod math;
mod operators;
mod oracle;
mod oracle_cases;
mod oracle_comparer;
mod oracle_leave_outs;
mod oracle_two_readings;
mod path_functions;
mod recalc_fixes;
mod recalc_rules;
mod scoped;
mod scoped_repeats;
mod score_gaps;
mod text;
mod text_measure;
mod theme;
mod theme_root;
mod trig;
mod vectors_unnamed;
mod whole_page;

use diacraft_calc::{CellAddress, CellError, Clock, EvalCtx, EvalOptions, Evaluator, OperandMode, Value};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Unit;

/// The comparison of the specs' vectors: "Compare numbers within 1E-9".
pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// What a vector expects.
#[derive(Clone, Debug)]
pub enum E {
    /// A plain number: no unit, dimension 0.
    N(f64),
    /// A number with this stored value and unit, at the unit's own dimension.
    U(f64, Unit),
    /// A number with this stored value, unit and dimension.
    D(f64, Unit, i8),
    /// A Boolean.
    B(bool),
    /// A string.
    T(&'static str),
    /// An error.
    Err(CellError),
    /// A date, by its serial.
    Date(f64),
    /// A currency: the amount and the code.
    Cy(f64, &'static str),
    /// A colour.
    Col(u8, u8, u8),
    /// A number with this stored value, whatever its unit: for a vector that gives no unit.
    Any(f64),
    /// An unsupported function.
    Unsupported,
}

/// Why a value is not what a vector expects, or nothing.
pub fn mismatch(v: &Value, e: &E) -> Option<String> {
    let bad = |why: &str| Some(format!("{why}: got {v:?}, expected {e:?}"));
    match (v, e) {
        (Value::Number(q), E::N(x)) => {
            (!(close(q.stored, *x) && q.dim() == 0 && q.unit == Unit::Unitless)).then(|| format!("got {v:?}, expected {e:?}"))
        }
        (Value::Number(q), E::U(x, u)) => {
            (!(close(q.stored, *x) && q.unit == *u && q.dim() == q.unit.natural_dim())).then(|| format!("got {v:?}, expected {e:?}"))
        }
        (Value::Number(q), E::D(x, u, d)) => (!(close(q.stored, *x) && q.unit == *u && q.dim() == *d)).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Number(q), E::Any(x)) => (!close(q.stored, *x)).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Number(q), E::Date(x)) => (!(close(q.stored, *x) && q.unit == Unit::Date)).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Bool(a), E::B(b)) => (a != b).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Text(a), E::T(b)) => (&**a != *b).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Error(a), E::Err(b)) => (a != b).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Currency { amount, currency }, E::Cy(x, code)) => {
            (!(close(*amount, *x) && currency.map(|c| c.code) == Some(*code))).then(|| format!("got {v:?}, expected {e:?}"))
        }
        (Value::Color(c), E::Col(r, g, b)) => ((c.r, c.g, c.b) != (*r, *g, *b)).then(|| format!("got {v:?}, expected {e:?}")),
        (Value::Unsupported(_), E::Unsupported) => None,
        _ => bad("wrong kind"),
    }
}

/// The cell a probe formula sits in: `User.Probe`, cell `Value`.
pub fn probe_addr() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("Probe"), "Value")
}

/// A document with one page and one shape (`ID` 1) that holds the probe formula and what `setup` adds, and the address of the probe.
pub fn probe(setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, formula: &str) -> (Document, CellAddress) {
    let shape = setup(ShapeBuilder::new(1)).row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    (doc, at)
}

/// Works the probe formula out on a blank shape, with operands worked out live.
pub fn eval(formula: &str) -> Value {
    eval_on(|s| s, formula)
}

/// Works the probe formula out on the shape `setup` makes.
pub fn eval_on(setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, formula: &str) -> Value {
    eval_with(setup, formula, EvalOptions::default())
}

/// As [`eval_on`], with options.
pub fn eval_with(setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, formula: &str, options: EvalOptions) -> Value {
    let (doc, at) = probe(setup, formula);
    let mut ctx = EvalCtx::new(options, Default::default());
    Evaluator::new(&doc, &mut ctx).evaluate(&at)
}

/// A clock fixed at an instant, in a zone `offset_hours` from UTC.
pub fn fixed_clock(now: &str, offset_seconds: i32) -> Clock {
    Clock { now: Some(jiff::civil::DateTime::strptime("%Y-%m-%dT%H:%M:%S", now).unwrap()), utc_offset_seconds: offset_seconds }
}

/// Options with the operands read as saved.
pub fn cached() -> EvalOptions {
    EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }
}

/// Checks a table of vectors against a blank shape: every row `(id, formula, expected)`, reporting all the rows that fail.
pub fn check_table(rows: &[(&str, &str, E)]) {
    let mut failures = Vec::new();
    for (id, formula, expected) in rows {
        let v = eval(formula);
        if let Some(why) = mismatch(&v, expected) {
            failures.push(format!("{id}: `{formula}`: {why}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

/// As [`check_table`], on the shape `setup` makes for every row.
pub fn check_table_on(setup: impl Fn(ShapeBuilder) -> ShapeBuilder, rows: &[(&str, &str, E)]) {
    let mut failures = Vec::new();
    for (id, formula, expected) in rows {
        let v = eval_on(&setup, formula);
        if let Some(why) = mismatch(&v, expected) {
            failures.push(format!("{id}: `{formula}`: {why}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

/// A page of several top-level shapes, with the probe formula added to the one at `at` (an index into `shapes`), worked out there.
pub fn eval_page(shapes: Vec<ShapeBuilder>, at: usize, formula: &str) -> Value {
    eval_page_with(shapes, at, formula, EvalOptions::default())
}

/// As [`eval_page`], with options.
pub fn eval_page_with(shapes: Vec<ShapeBuilder>, at: usize, formula: &str, options: EvalOptions) -> Value {
    let (doc, address) = page_probe(shapes, at, formula);
    let mut ctx = EvalCtx::new(options, Default::default());
    Evaluator::new(&doc, &mut ctx).evaluate(&address)
}

/// The document of [`eval_page`] and the address of its probe.
pub fn page_probe(shapes: Vec<ShapeBuilder>, at: usize, formula: &str) -> (Document, CellAddress) {
    let mut page = PageBuilder::new();
    for (i, shape) in shapes.into_iter().enumerate() {
        let shape = if i == at { shape.row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0") } else { shape };
        page = page.shape(shape);
    }
    let doc = DocBuilder::new().page(1, page).build();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), u32::try_from(at).unwrap()));
    (doc, CellAddress::new(sheet, probe_addr()))
}

/// The probe formula of the first shape of page 1 of `doc`, worked out.
pub fn evaluate_at_probe(doc: &Document) -> Value {
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    Evaluator::new(doc, &mut EvalCtx::default()).evaluate(&at)
}

/// A `Relationships` formula, as files write it: a `SUM` of `DEPENDSON` calls, each a code and the shapes after it by `Sheet.<id>!SheetRef()`
/// A target of 0 is a lost one, `REF()`.
pub fn relationships(codes: &[(u32, &[u32])]) -> String {
    let calls: Vec<String> = codes
        .iter()
        .map(|(code, targets)| {
            let mut args = vec![code.to_string()];
            for t in *targets {
                args.push(if *t == 0 { "REF()".to_owned() } else { format!("Sheet.{t}!SheetRef()") });
            }
            format!("DEPENDSON({})", args.join(","))
        })
        .collect();
    format!("SUM({})", calls.join(","))
}
