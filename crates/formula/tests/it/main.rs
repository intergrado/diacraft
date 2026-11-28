//! The formula crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test. Vectors are those of
//! the `-V` items, named in the comments
//! so a row says where it comes from. A vector that asks for a value (`-3^2` is 9) is checked here as the shape that gives the
//! value (`(-3)^2`): the value is `calc`'s (Task 6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod budget;
mod calls;
mod colour_exhaustive;
mod currency;
mod grammar;
mod limits;
mod literals;
mod names;
mod page_refs;
mod print;
mod props;
mod refs;
mod spans;
mod units;

use std::sync::mpsc;
use std::time::Duration;

use diacraft_formula::{BinaryOp, Expr, Index, Number, ParseError, Qualifier, Segment, UnaryOp, parse};

/// The comparison of the spec's vectors ("Compare numbers within 1E-9").
pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Parses `src`, which must parse.
pub fn ok(src: &str) -> Expr {
    match parse(src).ast {
        Ok(e) => e,
        Err(e) => panic!("{src:?} does not parse: {e}"),
    }
}

/// Parses `src`, which must not.
pub fn err(src: &str) -> ParseError {
    match parse(src).ast {
        Ok(e) => panic!("{src:?} parses, to {}", sexp(&e)),
        Err(e) => e,
    }
}

/// The shape of a tree as text, written here and independent of `print`: numbers and units as written, `@` before a
/// reference, `~` for an empty argument slot, `(op a b)` for an operator, `(call NAME args)` for a call. A test of the
/// printer cannot lean on it, and a test of the parser reads easily.
pub fn sexp(e: &Expr) -> String {
    match e {
        Expr::Number(n) => number(n),
        Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Expr::Str(s) => format!("{s:?}"),
        Expr::Color { r, g, b } => format!("#{r:02X}{g:02X}{b:02X}"),
        Expr::Currency { value, code } => format!("{value}{code}"),
        Expr::Guid(g) => g.to_string(),
        Expr::Ref(r) => format!("@{}{}", r.qualifiers.iter().map(|q| format!("{}!", qualifier(q))).collect::<String>(), path(&r.path)),
        Expr::Call(c) => {
            let quals = c.qualifiers.iter().map(|q| format!("{}!", qualifier(q))).collect::<String>();
            format!("({quals}call {}{})", c.name, args(&c.args))
        }
        Expr::Unary(op, x) => format!("({} {})", if *op == UnaryOp::Minus { "neg" } else { "pos" }, sexp(x)),
        Expr::Binary(op, a, b) => format!("({} {} {})", op.symbol(), sexp(a), sexp(b)),
        Expr::Percent(x) => format!("(% {})", sexp(x)),
    }
}

fn args(a: &[Option<Expr>]) -> String {
    a.iter().map(|x| format!(" {}", x.as_ref().map_or("~".to_string(), sexp))).collect()
}

fn number(n: &Number) -> String {
    match n {
        Number::Measured { value, unit, power } => {
            format!("{value}{}{}", unit.suffix().unwrap_or(unit.code()), power.map_or(String::new(), |p| format!("^{p}")))
        }
        Number::Implicit { value, unit, measure } => format!("{value}[{},{}]", unit.suffix().unwrap_or(unit.code()), measure.flag()),
        Number::FeetInches { feet, inches } => format!("{feet}FT{inches}IN"),
        Number::DegreesMinutesSeconds { degrees, minutes, seconds } => format!("{degrees}deg{minutes}min{seconds}sec"),
        Number::CiceroDidot { ciceros, didots } => format!("{ciceros}C{didots}"),
        Number::PicaPoints { picas, points } => format!("{picas}P{points}"),
    }
}

fn qualifier(q: &Qualifier) -> String {
    match q {
        Qualifier::Path(p) => path(p),
        Qualifier::Call { name, args: a } => format!("{name}({})", args(a).trim_start()),
    }
}

fn path(p: &[Segment]) -> String {
    p.iter()
        .map(|s| match &s.index {
            None => s.name.to_string(),
            Some(Index::Number(n)) => format!("{}[{n}]", s.name),
            Some(Index::Name(i)) => format!("{}[{i:?}]", s.name),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// `sexp(ok(src))`.
pub fn shape(src: &str) -> String {
    sexp(&ok(src))
}

/// A plain number node.
pub fn n(v: f64) -> Expr {
    Expr::Number(Number::plain(v))
}

pub fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    Expr::Binary(op, Box::new(a), Box::new(b))
}

pub fn neg(a: Expr) -> Expr {
    Expr::Unary(UnaryOp::Minus, Box::new(a))
}

/// Runs `f` on its own thread and fails the test if it takes longer than `secs`: a hang must be a failure, not a stuck
/// gate. The bound is generous, since the machine may be busy.
pub fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    // A deep tree must not need more than a thread's ordinary stack; this is the default's size, made explicit.
    within_stack(secs, 2 * 1024 * 1024, f)
}

/// [`within`] on a thread whose stack is `stack` bytes: for a test that a limit holds on a small stack (a 512 KiB stack is
/// what a browser's worker or a thread of a pool may have), where the default's size would hide a deep recursion.
pub fn within_stack<T: Send + 'static>(secs: u64, stack: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            // The receiver is gone only after a timeout, which has failed the test already.
            let _ = tx.send(f());
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(secs)).unwrap_or_else(|_| panic!("took longer than {secs} s"))
}
