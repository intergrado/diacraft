//! The printer: a tree as text that parses back to the same tree.
//!
//! The ranks are those, turned round into a number that grows with how tightly the operator binds
//! (`^` is 6, a sign 7, an atom and a percent 8). An operand is bracketed when it binds less tightly than the place it stands
//! in needs: the left operand of an operator must bind at least as tightly as the operator (equal ranks run from the left),
//! the right operand tighter, a sign's operand and a percent's as tight as a sign and an atom.

use diacraft_units::Unit;

use crate::parser::{MAX_INDEX_BYTES, is_name_char, is_name_start};
use crate::{BinaryOp, Call, Expr, Index, MAX_DEPTH, Number, Qualifier, Segment};

/// How tightly a sign binds, between `^` (6) and the atoms.
const SIGN: u8 = 7;
/// An atom, and a percent, which is read after an atom.
const ATOM: u8 = 8;

pub(crate) fn print(expr: &Expr) -> String {
    let mut out = String::new();
    expression(&mut out, expr, 0, 1);
    out
}

/// How tightly the printed form of `e` binds.
fn tightness(e: &Expr) -> u8 {
    match e {
        Expr::Binary(op, ..) => 9 - op.rank(),
        Expr::Unary(..) => SIGN,
        _ => ATOM,
    }
}

/// `e`, in brackets if it binds less tightly than `min`. Below `MAX_DEPTH` levels a tree is not followed.
fn expression(out: &mut String, e: &Expr, min: u8, depth: usize) {
    if depth > MAX_DEPTH {
        out.push_str("...");
        return;
    }
    let bracket = tightness(e) < min;
    if bracket {
        out.push('(');
    }
    bare(out, e, depth);
    if bracket {
        out.push(')');
    }
}

fn bare(out: &mut String, e: &Expr, depth: usize) {
    match e {
        Expr::Number(n) => number(out, n),
        Expr::Bool(b) => out.push_str(if *b { "TRUE" } else { "FALSE" }),
        Expr::Str(s) => {
            out.push('"');
            out.push_str(&s.replace('"', "\"\""));
            out.push('"');
        }
        Expr::Color { r, g, b } => out.push_str(&format!("#{r:02X}{g:02X}{b:02X}")),
        // No blank before the code, as `V` holds a currency. A reader takes a blank too, so `5 USD` prints
        // as `5USD` and reads back as the same tree.
        Expr::Currency { value, code } => out.push_str(&format!("{}{code}", general(*value))),
        // The text of a GUID holds its braces.
        Expr::Guid(g) => out.push_str(g),
        Expr::Ref(r) => {
            qualifiers(out, &r.qualifiers, depth);
            path(out, &r.path, r.qualifiers.is_empty());
        }
        Expr::Call(c) => call(out, c, depth),
        Expr::Unary(op, x) => {
            out.push_str(op.symbol());
            expression(out, x, SIGN, depth + 1);
        }
        Expr::Percent(x) => {
            expression(out, x, ATOM, depth + 1);
            out.push('%');
        }
        Expr::Binary(op, a, b) => binary(out, *op, a, b, depth),
    }
}

fn binary(out: &mut String, op: BinaryOp, a: &Expr, b: &Expr, depth: usize) {
    let tight = 9 - op.rank();
    // The operator `^` on a literal with a unit, `(3 in)^2`, is not `3 IN^2`, a square inch: the power of a unit is straight
    // after it, and here the blanks keep them apart, `3 IN ^ 2`. No bracket is added for it: it would be a level of nesting the text
    // did not have, and the one that makes a printed formula `TooDeep` at the limit.
    expression(out, a, tight, depth + 1);
    out.push(' ');
    out.push_str(op.symbol());
    out.push(' ');
    expression(out, b, tight + 1, depth + 1);
}

/// From this size a number is written with an exponent: its digits would run on (`1e308` is 309), and a formula of such numbers
/// would print much longer than it was.
const EXPONENT_FROM: f64 = 1e16;
/// Below this size, too.
const EXPONENT_BELOW: f64 = 1e-5;

/// A number as text that reads back exactly: the shortest digits that parse to the same double, in full between 1e-5 and 1e16
/// and with an exponent beyond. One a literal cannot hold (not finite) gives 0.
fn general(v: f64) -> String {
    let size = v.abs();
    if !v.is_finite() {
        "0".to_string()
    } else if size >= EXPONENT_FROM || (size != 0.0 && size < EXPONENT_BELOW) {
        format!("{v:e}")
    } else {
        format!("{v}")
    }
}

/// A number in digits only, no exponent: what the second part of `8 FT 4 IN`, `10°15'`, `2C4` and `3P6` is read from.
fn plain(v: f64) -> String {
    if v.is_finite() { format!("{v}") } else { "0".to_string() }
}

fn unit_text(unit: Unit) -> &'static str {
    unit.suffix().unwrap_or_else(|| unit.code())
}

fn number(out: &mut String, n: &Number) {
    match *n {
        Number::Measured { value, unit, power } => {
            out.push_str(&general(value));
            if unit != Unit::Unitless {
                out.push(' ');
                out.push_str(unit_text(unit));
            }
            if let Some(p) = power {
                out.push_str(&format!("^{p}"));
            }
        }
        Number::Implicit { value, unit, measure } => out.push_str(&format!("{}[{},{}]", general(value), unit_text(unit), measure.flag())),
        Number::FeetInches { feet, inches } => out.push_str(&format!("{} FT {} IN", general(feet), plain(inches))),
        Number::DegreesMinutesSeconds { degrees, minutes, seconds } => {
            out.push_str(&format!("{}\u{b0}", general(degrees)));
            // What is not written is 0, so trailing zero parts are left out.
            if seconds != 0.0 {
                out.push_str(&format!("{}'{}\"", plain(minutes), plain(seconds)));
            } else if minutes != 0.0 {
                out.push_str(&format!("{}'", plain(minutes)));
            }
        }
        Number::CiceroDidot { ciceros, didots } => out.push_str(&format!("{}C{}", general(ciceros), plain(didots))),
        Number::PicaPoints { picas, points } => out.push_str(&format!("{}P{}", general(picas), plain(points))),
    }
}

fn call(out: &mut String, c: &Call, depth: usize) {
    qualifiers(out, &c.qualifiers, depth);
    out.push_str(&c.name);
    arguments(out, &c.args, depth);
}

fn arguments(out: &mut String, args: &[Option<Expr>], depth: usize) {
    out.push('(');
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if let Some(e) = arg {
            expression(out, e, 0, depth + 1);
        }
    }
    out.push(')');
}

fn qualifiers(out: &mut String, list: &[Qualifier], depth: usize) {
    for q in list {
        match q {
            Qualifier::Path(segments) => path(out, segments, false),
            Qualifier::Call { name, args } => {
                out.push_str(name);
                arguments(out, args, depth);
            }
        }
        out.push('!');
    }
}

/// A name that needs no quotes: letters, digits and underscores, and for the first of a path not starting with a digit.
fn is_plain(name: &str, first: bool) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        None => false,
        Some(c) if first => is_name_start(c) && chars.all(is_name_char),
        Some(c) => is_name_char(c) && chars.all(is_name_char),
    }
}

fn quoted(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

/// A dotted path. `alone` says nothing qualifies it: a lone `TRUE` or `FALSE` would read as the word, so it is quoted.
fn path(out: &mut String, segments: &[Segment], alone: bool) {
    let lone_word = alone && matches!(segments, [only] if only.index.is_none() && is_boolean_word(&only.name));
    for (i, segment) in segments.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        if is_plain(&segment.name, i == 0) && !lone_word {
            out.push_str(&segment.name);
        } else {
            out.push_str(&quoted(&segment.name));
        }
        if let Some(index) = &segment.index {
            write_index(out, index);
        }
    }
}

fn is_boolean_word(name: &str) -> bool {
    name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE")
}

/// `[2]`, `[Site-2]`, `['2']`. A name is bare only if it reads back as itself: not a whole number, not empty, no brackets or
/// quotes in it, no blanks at its ends, and no longer than the parser reads an unquoted index (`MAX_INDEX_BYTES`; a quoted one may
/// be as long as a name).
fn write_index(out: &mut String, index: &Index) {
    match index {
        Index::Number(n) => out.push_str(&format!("[{n}]")),
        Index::Name(name) => {
            let bare = !name.is_empty()
                && name.len() <= MAX_INDEX_BYTES
                && name.trim() == &**name
                && !name.contains([']', '\''])
                && !name.bytes().all(|b| b.is_ascii_digit());
            if bare {
                out.push_str(&format!("[{name}]"));
            } else {
                out.push_str(&format!("[{}]", quoted(name)));
            }
        }
    }
}
