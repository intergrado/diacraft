//! Custom shapes: a `draw:custom-shape`'s
//! `draw:enhanced-geometry` read into `Geometry` sections, `User` rows for its modifiers and equations, `Control` rows for its handles,
//! `Connection` rows for its glue points, and the text block's cells.
//!
//! **Quantities with their formulas.** A coordinate of a path is in the view box's units. It is a plain number, or it names a modifier
//! or an equation, and then its cell is a formula that reads the `User` row (`Width*User.ODFEq0/21600`). Every quantity is a [`Q`]: its
//! value, worked out, and the ShapeSheet expression for it when it is not a plain number. An arc's end and middle points are worked out from
//! its own parameters with `ATAN2`, `SIN` and `COS` when any parameter is a formula, by doing the arithmetic on [`Q`]s, which carries the
//! expression along with the value; an expression past [`MOST_FORMULA`] characters is let go of and the cell holds its value alone.
//!
//! **Cost.** Every row and cell is charged to the item cap as it is made, every command group to the page's path budget, a path that makes
//! more subpaths or rows than a section holds (or a shape more handles than that) is `xml_too_complex`, and equations are bounded and
//! evaluated once each ([`super::equations`]). A quantity's expression is bounded, whether it came from a parameter or from arithmetic on
//! quantities: past [`MOST_FORMULA`] characters, or nested past the formula crate's limit in the cell that holds it, it is let go of and the
//! cell holds the value alone, so an arc costs a constant number of rows and cells and a close of a subpath that began at a long expression
//! does not parse it again. A formula cell is charged by the nodes of its parse (`cells::Forms`), not by its text; an equation's compiled
//! form is charged by the bytes it holds ([`equations::Compiled::bytes`]); and a parameter is compiled once and its text is charged, one item
//! for 64 bytes, when it is read ([`Ctx::charge`]), because an arc of no sweep reads its parameters and makes no row to pay for them. Nothing is made for a modifier that no formula names beyond the
//! one `User` row it is given.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;
use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{Cell, CellName, Row, Section};

use super::Reader;
use super::cells::{self, round9};
use super::equations::{self, ByName, Compiled, Env, Eval};
use super::geometry::{Budget, ViewBox};
use super::names::{Attrs, Names, Ns};
use super::shapes::{Units, row};
use super::units;
use crate::error::OdgError;

/// The longest expression a [`Q`] keeps; past it the quantity is its value alone.
const MOST_FORMULA: usize = 500;

/// A quantity of the geometry in the view box's units: its value, the expression for it when it is not a plain number, and the error it
/// is when it is one.
#[derive(Clone, Debug)]
struct Q {
    v: f64,
    e: Option<String>,
    err: Option<&'static str>,
}

impl Q {
    fn num(v: f64) -> Q {
        Q { v: if v.is_finite() { v } else { 0.0 }, e: None, err: None }
    }

    /// The text this quantity is written as in a larger expression.
    fn text(&self) -> String {
        match &self.e {
            Some(e) => e.clone(),
            None => cells::number(self.v).text().to_owned(),
        }
    }

    fn combine(v: f64, parts: &[&Q], make: impl FnOnce() -> String) -> Q {
        let err = parts.iter().find_map(|p| p.err);
        let formula = parts.iter().any(|p| p.e.is_some());
        let e = if formula && err.is_none() {
            let text = make();
            (text.len() <= MOST_FORMULA).then_some(text)
        } else {
            None
        };
        if v.is_finite() { Q { v, e, err } } else { Q { v: 0.0, e: None, err: err.or(Some("#NUM!")) } }
    }

    fn add(&self, o: &Q) -> Q {
        Q::combine(self.v + o.v, &[self, o], || format!("({}+{})", self.text(), o.text()))
    }

    fn sub(&self, o: &Q) -> Q {
        Q::combine(self.v - o.v, &[self, o], || format!("({}-{})", self.text(), o.text()))
    }

    fn mul(&self, o: &Q) -> Q {
        Q::combine(self.v * o.v, &[self, o], || format!("({}*{})", self.text(), o.text()))
    }

    fn scaled(&self, k: f64) -> Q {
        self.mul(&Q::num(k))
    }

    fn sin(&self) -> Q {
        Q::combine(self.v.sin(), &[self], || format!("SIN({})", self.text()))
    }

    fn cos(&self) -> Q {
        Q::combine(self.v.cos(), &[self], || format!("COS({})", self.text()))
    }

    fn atan2(&self, x: &Q) -> Q {
        Q::combine(self.v.atan2(x.v), &[self, x], || format!("ATAN2({},{})", self.text(), x.text()))
    }

    /// An angle in degrees as radians.
    fn to_radians(&self) -> Q {
        Q::combine(self.v.to_radians(), &[self], || format!("({}*PI()/180)", self.text()))
    }

    fn neg(&self) -> Q {
        Q::combine(-self.v, &[self], || format!("(-{})", self.text()))
    }
}

/// A point of the geometry.
type Pt = (Q, Q);

/// The point where the ray from `c` at angle `theta` (radians, the y axis down, so clockwise as seen) meets the ellipse of half-axes `r`.
/// It is worked out through the ellipse's parameter, `tau = ATAN2(wR*SIN(theta), hR*COS(theta))` (the parameter of the point on the ray), as
/// `c + (wR*COS(tau), hR*SIN(tau))`, which keeps the expression short (the angle is named once in each coordinate) and divides by nothing.
fn ray_point(c: &Pt, r: &Pt, theta: &Q) -> Pt {
    let tau = r.0.mul(&theta.sin()).atan2(&r.1.mul(&theta.cos()));
    (c.0.add(&r.0.mul(&tau.cos())), c.1.add(&r.1.mul(&tau.sin())))
}

/// What a geometry's quantities are worked out against.
struct Ctx<'a> {
    by_name: ByName<'a>,
    vb: ViewBox,
    stretch: (f64, f64),
    log: (f64, f64),
    has_fill: bool,
    has_stroke: bool,
    mods: Vec<f64>,
    values: Vec<Eval>,
    work: equations::Work,
    /// The modifiers any formula named, to the number of times each was named: the rows of those past `mods` are made.
    named_mods: BTreeSet<u32>,
    /// The bytes of the expressions compiled since the last charge ([`Ctx::charge`]).
    unpaid: usize,
}

impl Ctx<'_> {
    fn env(&self) -> Env<'_> {
        Env { vb: self.vb, stretch: self.stretch, log: self.log, has_fill: self.has_fill, has_stroke: self.has_stroke, mods: &self.mods }
    }

    /// The values of the equations `compiled`, each worked out once (`equations::evaluate_all`).
    fn evaluate(&mut self, compiled: &[Compiled]) {
        let env = Env { vb: self.vb, stretch: self.stretch, log: self.log, has_fill: self.has_fill, has_stroke: self.has_stroke, mods: &self.mods };
        self.values = equations::evaluate_all(compiled, &env, &mut self.work);
    }

    /// A parameter of a path: a number, or an expression, and `None` for one that breaks the syntax (the path is drawn as its
    /// frame). The expression is compiled once.
    fn param_strict(&mut self, token: &str) -> Option<Q> {
        if let Some(v) = plain_number(token) {
            return Some(Q::num(v));
        }
        let compiled = self.compile(token);
        if compiled.error.is_some() { None } else { Some(self.quantity(compiled)) }
    }

    /// A parameter of a path, a handle or a glue point: a number, or an expression; one that breaks the syntax is the error `#NAME?`.
    fn param(&mut self, token: &str) -> Q {
        if let Some(v) = plain_number(token) {
            return Q::num(v);
        }
        let compiled = self.compile(token);
        self.quantity(compiled)
    }

    /// An expression compiled: counted, and owed to the items by its length ([`Ctx::charge`]).
    fn compile(&mut self, token: &str) -> Compiled {
        self.work.compiled = self.work.compiled.saturating_add(token.len() as u64);
        self.unpaid = self.unpaid.saturating_add(token.len());
        equations::compile(token, &self.by_name, &self.env())
    }

    /// Charges the items the expressions compiled since the last call stand for (one for each 64 bytes of their text), before the next is
    /// read: the work of reading a parameter is made whether or not the geometry it belongs to makes a row (an arc of no sweep makes none).
    fn charge(&mut self, r: &mut Reader) -> Result<(), OdgError> {
        r.take_model_bytes(std::mem::take(&mut self.unpaid))
    }

    /// The quantity an expression compiled is, with its value worked out against the equations.
    fn quantity(&mut self, compiled: Compiled) -> Q {
        if let Some(n) = compiled.plain {
            return Q::num(n);
        }
        for n in &compiled.mods {
            self.named_mods.insert(*n);
        }
        let value = {
            let env =
                Env { vb: self.vb, stretch: self.stretch, log: self.log, has_fill: self.has_fill, has_stroke: self.has_stroke, mods: &self.mods };
            equations::evaluate_one(&compiled, &env, &self.values, &mut self.work)
        };
        // An expression past the limit is let go of, as the arithmetic on quantities does it: the value stands, and no cell holds the text
        // (a long one was parsed again by every close of a subpath that began at it).
        let keep = compiled.error.is_none() && compiled.text.len() <= MOST_FORMULA;
        Q { v: value.value, e: keep.then_some(compiled.text), err: value.error }
    }
}

/// A token that is one number written plainly, as a path writes them.
fn plain_number(token: &str) -> Option<f64> {
    let first = token.bytes().next()?;
    if !(first.is_ascii_digit() || first == b'-' || first == b'+' || first == b'.') {
        return None;
    }
    token.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Whether an expression's text is one term, which needs no brackets round it.
fn single_term(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'!'))
}

fn paren(text: &str) -> String {
    if single_term(text) || whole_bracket(text) { text.to_owned() } else { format!("({text})") }
}

/// Whether the text is one bracket from end to end: its first `(` is closed by its last character.
fn whole_bracket(text: &str) -> bool {
    if !(text.starts_with('(') && text.ends_with(')')) {
        return false;
    }
    let mut depth = 0usize;
    for (at, b) in text.bytes().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1 == text.len();
                }
            }
            _ => {}
        }
    }
    false
}

/// Which axis a coordinate is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// The frame's size and units, and the view box, for turning quantities into cells.
#[derive(Clone, Copy)]
struct Space {
    w: f64,
    h: f64,
    u: Units,
    vb: ViewBox,
}

impl Space {
    fn size(&self, axis: Axis) -> f64 {
        if axis == Axis::X { self.w } else { self.h }
    }

    fn unit(&self, axis: Axis) -> Option<&'static str> {
        if axis == Axis::X { self.u.width } else { self.u.height }
    }

    fn word(axis: Axis) -> &'static str {
        if axis == Axis::X { "Width" } else { "Height" }
    }

    /// The share of the frame a view-box value on `axis` is (y flipped).
    fn share(&self, axis: Axis, v: f64) -> f64 {
        match axis {
            Axis::X => (v - self.vb.left) / self.vb.width,
            Axis::Y => 1.0 - (v - self.vb.top) / self.vb.height,
        }
    }

    /// The expression for the share of a quantity with expression `e` (`(<E>-<L>)/<VW>` and `1-(<E>-<T>)/<VH>`).
    fn share_text(&self, axis: Axis, e: &str) -> String {
        let (origin, extent) = if axis == Axis::X { (self.vb.left, self.vb.width) } else { (self.vb.top, self.vb.height) };
        let inner = if origin == 0.0 { e.to_owned() } else { format!("{e}-{}", cells::number(origin).text()) };
        let part = format!("{}/{}", paren(&inner), cells::number(extent).text());
        if axis == Axis::X { part } else { format!("1-{part}") }
    }
}

/// A formula cell for `text`, or a cell that holds `value` alone when the text would nest deeper than the formula crate reads (the quantities
/// are composed into larger expressions, which add brackets to the ones they hold: a parameter that nests to the limit does not fit in its
/// cell), so that no cell holds a formula that does not parse for its nesting.
fn formula_cell(forms: &mut cells::Forms, text: &str, value: f64, unit: Option<&str>) -> Cell {
    if equations::nesting(text) > equations::MOST_NESTING { cells::length(value, unit) } else { forms.cell(text, value, unit) }
}

/// An error cell: the value 0 and the error's name.
fn error_cell(code: &'static str, unit: Option<&str>) -> Cell {
    let mut cell = cells::length(0.0, unit);
    cell.error = Some(Arc::from(code));
    cell
}

/// A path's row being made: its type and cells.
type RowOut = (&'static str, Vec<(&'static str, Cell)>);

/// The subpaths of a geometry: each one's rows, and whether it is not filled and not stroked.
type Parts = Vec<(Vec<RowOut>, bool, bool)>;

/// A subpath being built.
struct Sec {
    rows: Vec<RowOut>,
    start: Pt,
    /// A full ellipse that is the whole subpath: its centre and half-axes (one `Ellipse` row).
    ellipse: Option<(Pt, Pt)>,
    no_fill: bool,
    no_line: bool,
}

/// What a path is made of, and the command that made the last row.
struct Builder<'x, 'r, 'a> {
    r: &'x mut Reader<'r>,
    ctx: &'x mut Ctx<'a>,
    budget: &'x mut Budget,
    space: Space,
    done: Vec<Sec>,
    cur: Option<Sec>,
    /// Where the current set of subpaths begins in `done`, and the set's flags (`F`, `S`).
    set_from: usize,
    set_no_fill: bool,
    set_no_line: bool,
    pen: Pt,
}

/// The path is broken: it is drawn as its frame.
#[derive(Debug)]
enum PathFault {
    Malformed,
    Error(OdgError),
}

impl From<OdgError> for PathFault {
    fn from(e: OdgError) -> PathFault {
        PathFault::Error(e)
    }
}

/// The commands, and how many parameters one group of each takes.
fn arity(command: char) -> Option<usize> {
    Some(match command {
        'M' | 'L' | 'X' | 'Y' => 2,
        'Q' | 'G' => 4,
        'C' | 'T' | 'U' => 6,
        'A' | 'B' | 'V' | 'W' => 8,
        'Z' | 'N' | 'F' | 'S' => 0,
        _ => return None,
    })
}

/// A token of a path: a command or a parameter.
enum Tok<'p> {
    Cmd(char),
    Param(&'p str),
}

/// A path's tokens: parameters are separated by white space or commas, and a command letter may stand against the parameter after it.
fn tokens(path: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    for word in path.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|w| !w.is_empty()) {
        let mut rest = word;
        while let Some(first) = rest.chars().next() {
            if first.is_ascii_uppercase() {
                out.push(Tok::Cmd(first));
                rest = rest.get(1..).unwrap_or("");
            } else {
                out.push(Tok::Param(rest));
                break;
            }
        }
    }
    out
}

impl Builder<'_, '_, '_> {
    fn new_section(&mut self, at: Pt) -> Result<(), PathFault> {
        self.close_section()?;
        if self.done.len() + 1 > diacraft_doc::MAX_SECTIONS_PER_SHEET.saturating_sub(4) {
            return Err(OdgError::TooComplex { what: "subpaths in one shape", max: diacraft_doc::MAX_SECTIONS_PER_SHEET as u64 }.into());
        }
        self.r.take_items(super::cost::SECTION)?;
        let mut sec = Sec { rows: Vec::new(), start: at.clone(), ellipse: None, no_fill: false, no_line: false };
        let cells = vec![("X", self.abs(Axis::X, &at.0)), ("Y", self.abs(Axis::Y, &at.1))];
        self.push_row_to(&mut sec, "MoveTo", cells)?;
        self.pen = at;
        self.cur = Some(sec);
        Ok(())
    }

    fn close_section(&mut self) -> Result<(), PathFault> {
        if let Some(mut sec) = self.cur.take() {
            if let Some((c, rad)) = sec.ellipse.take() {
                let a = (c.0.add(&rad.0), c.1.clone());
                let top = (c.0.clone(), c.1.sub(&rad.1));
                let cells = vec![
                    ("X", self.abs(Axis::X, &c.0)),
                    ("Y", self.abs(Axis::Y, &c.1)),
                    ("A", self.abs(Axis::X, &a.0)),
                    ("B", self.abs(Axis::Y, &a.1)),
                    ("C", self.abs(Axis::X, &top.0)),
                    ("D", self.abs(Axis::Y, &top.1)),
                ];
                sec.rows.clear();
                self.push_row_to(&mut sec, "Ellipse", cells)?;
            }
            self.done.push(sec);
        }
        Ok(())
    }

    /// Charges a row and its cells, then keeps it in `sec`.
    fn push_row_to(&mut self, sec: &mut Sec, ty: &'static str, cells: Vec<(&'static str, Cell)>) -> Result<(), PathFault> {
        if sec.rows.len() >= diacraft_doc::MAX_ROWS_PER_SECTION {
            return Err(OdgError::TooComplex { what: "rows in one subpath", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 }.into());
        }
        // The formulas the cells hold were parsed when the cells were made, and are charged by their nodes with these items.
        self.r.take_items(super::cost::ROW)?;
        self.r.take_cells(cells.len())?;
        sec.rows.push((ty, cells));
        Ok(())
    }

    fn push_row(&mut self, ty: &'static str, cells: Vec<(&'static str, Cell)>) -> Result<(), PathFault> {
        let Some(mut sec) = self.cur.take() else { return Ok(()) };
        sec.ellipse = None;
        let result = self.push_row_to(&mut sec, ty, cells);
        self.cur = Some(sec);
        result
    }

    /// The section the pen is in, one started at the pen when there is none.
    fn ensure(&mut self) -> Result<(), PathFault> {
        if self.cur.is_none() {
            let at = self.pen.clone();
            self.new_section(at)?;
        }
        Ok(())
    }

    /// The absolute cell of a coordinate: `Width*k`, `Height*k`, or the formula of R11.
    fn abs(&mut self, axis: Axis, q: &Q) -> Cell {
        let (size, unit) = (self.space.size(axis), self.space.unit(axis));
        if let Some(code) = q.err {
            return error_cell(code, unit);
        }
        let share = self.space.share(axis, q.v);
        match &q.e {
            None => cells::share(&mut self.r.forms, Space::word(axis), share, size, unit),
            Some(e) => {
                let word = Space::word(axis);
                let text = match axis {
                    Axis::X => format!("{word}*{}", self.space.share_text(axis, e)),
                    Axis::Y => format!("{word}*({})", self.space.share_text(axis, e)),
                };
                formula_cell(&mut self.r.forms, &text, share * size, unit)
            }
        }
    }

    /// The share cell of a coordinate, for the `Rel` rows.
    fn rel(&mut self, axis: Axis, q: &Q) -> Cell {
        if let Some(code) = q.err {
            return error_cell(code, None);
        }
        let share = self.space.share(axis, q.v);
        match &q.e {
            None => cells::plain(round9(share)),
            Some(e) => {
                let text = self.space.share_text(axis, e);
                formula_cell(&mut self.r.forms, &text, share, None)
            }
        }
    }

    fn line_to(&mut self, p: &Pt) -> Result<(), PathFault> {
        let cells = vec![("X", self.abs(Axis::X, &p.0)), ("Y", self.abs(Axis::Y, &p.1))];
        self.push_row("LineTo", cells)?;
        self.pen = p.clone();
        Ok(())
    }

    /// Whether the pen is at `p` (within the view box's rounding).
    fn at(&self, p: &Pt) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
        near(self.pen.0.v, p.0.v) && near(self.pen.1.v, p.1.v)
    }

    /// The half-axis along `axis` as an absolute length for `D`: its expression and value in the frame.
    fn half_axis(&self, axis: Axis, q: &Q) -> (String, f64) {
        let extent = if axis == Axis::X { self.space.vb.width } else { self.space.vb.height };
        let word = Space::word(axis);
        let value = self.space.size(axis) * q.v / extent;
        match &q.e {
            None => {
                let k = round9(q.v / extent);
                (if k == 1.0 { word.to_owned() } else { format!("{word}*{}", cells::number(k).text()) }, value)
            }
            Some(e) => (format!("{word}*{}/{}", paren(e), cells::number(extent).text()), value),
        }
    }

    /// One arc, from the ray `start` (radians) sweeping `sweep` (radians, positive clockwise as seen) on the ellipse of half-axes `rad`
    /// about `c`: a line to its start (or a move, `jump`), then one `EllipticalArcTo` for each piece of at most half a turn.
    #[allow(clippy::too_many_arguments)]
    fn arc(&mut self, c: &Pt, rad: &Pt, start: &Q, sweep: &Q, jump: bool, from_pen: bool, ellipse: bool) -> Result<(), PathFault> {
        // An arc of no radius or no sweep makes no row, and no point of it is worked out unless a section begins at it: its expressions are
        // not built for nothing (340,000 such arcs of long parameters cost seconds the items never saw).
        let degenerate = rad.0.v.abs() < 1e-12 || rad.1.v.abs() < 1e-12 || sweep.v == 0.0 || sweep.err.is_some() || start.err.is_some();
        if jump {
            self.new_section(ray_point(c, rad, start))?;
        } else if !from_pen && !degenerate {
            self.ensure()?;
            let first = ray_point(c, rad, start);
            if !self.at(&first) {
                self.line_to(&first)?;
            }
        } else {
            self.ensure()?;
        }
        if degenerate {
            return Ok(());
        }
        // A turn or more is one turn, cut into two halves; only a whole turn can be an `Ellipse` row.
        let whole = sweep.v.abs() >= 2.0 * PI - 1e-9;
        let sweep = if whole { Q::num(2.0 * PI.copysign(sweep.v)) } else { sweep.clone() };
        let pieces = ((sweep.v.abs() / PI) - 1e-9).ceil().max(1.0) as usize;
        let (ra, rb) = (self.half_axis(Axis::X, &rad.0), self.half_axis(Axis::Y, &rad.1));
        let ratio_text = format!("({})/({})", ra.0, rb.0);
        let ratio = if rb.1 != 0.0 { ra.1 / rb.1 } else { 0.0 };
        for k in 1..=pieces {
            let (end_angle, mid_angle) =
                (start.add(&sweep.scaled(k as f64 / pieces as f64)), start.add(&sweep.scaled((k as f64 - 0.5) / pieces as f64)));
            let (end, mid) = (ray_point(c, rad, &end_angle), ray_point(c, rad, &mid_angle));
            let cells = vec![
                ("X", self.abs(Axis::X, &end.0)),
                ("Y", self.abs(Axis::Y, &end.1)),
                ("A", self.abs(Axis::X, &mid.0)),
                ("B", self.abs(Axis::Y, &mid.1)),
                ("C", cells::plain(0.0)),
                ("D", formula_cell(&mut self.r.forms, &ratio_text, ratio, None)),
            ];
            self.push_row("EllipticalArcTo", cells)?;
            self.pen = end;
        }
        if ellipse
            && whole
            && let Some(sec) = self.cur.as_mut()
        {
            sec.ellipse = Some((c.clone(), (rad.0.clone(), rad.1.clone())));
        }
        Ok(())
    }

    /// Ends the current set of subpaths: its flags go on its sections (`F` not filled, `S` not stroked, both not shown).
    fn end_set(&mut self) -> Result<(), PathFault> {
        self.close_section()?;
        let (fill, line) = (self.set_no_fill, self.set_no_line);
        for sec in self.done.iter_mut().skip(self.set_from) {
            sec.no_fill = fill;
            sec.no_line = line;
        }
        self.set_from = self.done.len();
        self.set_no_fill = false;
        self.set_no_line = false;
        Ok(())
    }

    /// One group of parameters of `command`.
    fn group(&mut self, command: char, args: &[Q]) -> Result<(), PathFault> {
        let p = |i: usize| args.get(i).cloned().unwrap_or_else(|| Q::num(0.0));
        match command {
            'M' => self.new_section((p(0), p(1)))?,
            'L' => {
                self.ensure()?;
                self.line_to(&(p(0), p(1)))?;
            }
            'C' => {
                self.ensure()?;
                let end = (p(4), p(5));
                let cells = vec![
                    ("X", self.rel(Axis::X, &end.0)),
                    ("Y", self.rel(Axis::Y, &end.1)),
                    ("A", self.rel(Axis::X, &p(0))),
                    ("B", self.rel(Axis::Y, &p(1))),
                    ("C", self.rel(Axis::X, &p(2))),
                    ("D", self.rel(Axis::Y, &p(3))),
                ];
                self.push_row("RelCubBezTo", cells)?;
                self.pen = end;
            }
            'Q' => {
                self.ensure()?;
                let end = (p(2), p(3));
                let cells = vec![
                    ("X", self.rel(Axis::X, &end.0)),
                    ("Y", self.rel(Axis::Y, &end.1)),
                    ("A", self.rel(Axis::X, &p(0))),
                    ("B", self.rel(Axis::Y, &p(1))),
                ];
                self.push_row("RelQuadBezTo", cells)?;
                self.pen = end;
            }
            'T' | 'U' => {
                let (c, rad) = ((p(0), p(1)), (p(2).clone_abs(), p(3).clone_abs()));
                let (t0, t1) = (p(4), p(5));
                // From the ray at t0 clockwise to the ray at t1, angles mod 360; equal rays draw a whole ellipse only when they are 360 apart.
                let full = (t1.v - t0.v).abs() == 360.0;
                let sweep_deg = t1.sub(&t0);
                let wrapped = Q { v: sweep_deg.v.rem_euclid(360.0), ..sweep_deg.clone() };
                let sweep = if full { Q::num(360.0).to_radians() } else { wrapped.to_radians() };
                self.arc(&c, &rad, &t0.to_radians(), &sweep, command == 'U', false, command == 'U')?;
            }
            'A' | 'B' | 'V' | 'W' => self.box_arc(command, args)?,
            'G' => self.swing(args)?,
            'X' | 'Y' => self.quarter(command, &(p(0), p(1)))?,
            _ => return Err(PathFault::Malformed),
        }
        Ok(())
    }

    /// `A`, `B`, `V`, `W`: the ellipse in a box, between the rays towards two points.
    fn box_arc(&mut self, command: char, args: &[Q]) -> Result<(), PathFault> {
        let p = |i: usize| args.get(i).cloned().unwrap_or_else(|| Q::num(0.0));
        let half = Q::num(0.5);
        let c = (p(0).add(&p(2)).mul(&half), p(1).add(&p(3)).mul(&half));
        let rad = (p(2).sub(&p(0)).mul(&half), p(3).sub(&p(1)).mul(&half));
        let rad = (rad.0.clone_abs(), rad.1.clone_abs());
        let start = p(5).sub(&c.1).atan2(&p(4).sub(&c.0));
        let end = p(7).sub(&c.1).atan2(&p(6).sub(&c.0));
        let clockwise = matches!(command, 'V' | 'W');
        // Between the two rays the way the command goes, as a share of a turn in [0, 2 pi).
        let turn = 2.0 * PI;
        let raw = if clockwise { end.sub(&start) } else { start.sub(&end) };
        let swing = Q { v: raw.v.rem_euclid(turn), ..raw };
        let sweep = if clockwise { swing } else { swing.neg() };
        self.arc(&c, &rad, &start, &sweep, matches!(command, 'B' | 'V'), false, false)
    }

    /// `G`: an arc starting at the pen, on the ellipse of half-axes `wR`, `hR`, the pen at ray angle `st`, sweeping `sw` degrees.
    fn swing(&mut self, args: &[Q]) -> Result<(), PathFault> {
        let p = |i: usize| args.get(i).cloned().unwrap_or_else(|| Q::num(0.0));
        let rad = (p(0), p(1));
        let start = p(2).to_radians();
        let origin = (Q::num(0.0), Q::num(0.0));
        let offset = ray_point(&origin, &rad, &start);
        let pen = self.pen.clone();
        let c = (pen.0.sub(&offset.0), pen.1.sub(&offset.1));
        self.ensure()?;
        self.arc(&c, &rad, &start, &p(3).to_radians(), false, true, false)
    }

    /// `X` and `Y`: a quarter ellipse from the pen to `to`, the first tangent to the x axis (`X`) or the y axis (`Y`).
    fn quarter(&mut self, command: char, to: &Pt) -> Result<(), PathFault> {
        self.ensure()?;
        let pen = self.pen.clone();
        let c = if command == 'X' { (pen.0.clone(), to.1.clone()) } else { (to.0.clone(), pen.1.clone()) };
        let rad = (to.0.sub(&pen.0).clone_abs(), to.1.sub(&pen.1).clone_abs());
        let start = pen.1.sub(&c.1).atan2(&pen.0.sub(&c.0));
        let end = to.1.sub(&c.1).atan2(&to.0.sub(&c.0));
        // The short way round: a quarter, in the direction that takes the start to the end.
        let mut turn = end.v - start.v;
        while turn > PI {
            turn -= 2.0 * PI;
        }
        while turn < -PI {
            turn += 2.0 * PI;
        }
        self.arc(&c, &rad, &start, &Q::num(turn), false, true, false)
    }
}

impl Q {
    /// The quantity's absolute value: itself when it is not negative (its expression is kept as it is, so a half-axis that is an equation
    /// reads through it), and `ABS(...)` of it when it is.
    fn clone_abs(&self) -> Q {
        if self.v >= 0.0 {
            return self.clone();
        }
        Q::combine(-self.v, &[self], || format!("ABS({})", self.text()))
    }
}

/// The subpaths of an `draw:enhanced-path`, or the fault that makes it a path drawn as its box.
fn path_sections(r: &mut Reader, ctx: &mut Ctx<'_>, budget: &mut Budget, space: Space, path: &str) -> Result<Parts, PathFault> {
    let toks = tokens(path);
    let mut b = Builder {
        r,
        ctx,
        budget,
        space,
        done: Vec::new(),
        cur: None,
        set_from: 0,
        set_no_fill: false,
        set_no_line: false,
        pen: (Q::num(0.0), Q::num(0.0)),
    };
    let mut at = 0usize;
    while let Some(tok) = toks.get(at) {
        at += 1;
        let Tok::Cmd(mut command) = *tok else { return Err(PathFault::Malformed) };
        let Some(n) = arity(command) else { return Err(PathFault::Malformed) };
        if n == 0 {
            b.budget.take(1)?;
            match command {
                'Z' => {
                    if let Some(start) = b.cur.as_ref().map(|s| s.start.clone()) {
                        if !b.at(&start) {
                            b.line_to(&start)?;
                        }
                        b.pen = start;
                    }
                }
                'N' => b.end_set()?,
                'F' => b.set_no_fill = true,
                _ => b.set_no_line = true,
            }
            continue;
        }
        // One or more groups of parameters: after the first, the command repeats; pairs after an `M` are lines, and `X` and `Y` alternate.
        let mut groups = 0usize;
        while let Some(group) = toks.get(at..at + n) {
            if !group.iter().all(|t| matches!(t, Tok::Param(_))) {
                break;
            }
            b.budget.take(1)?;
            let mut args: Vec<Q> = Vec::with_capacity(n);
            for t in group {
                if let Tok::Param(text) = t {
                    args.push(b.ctx.param_strict(text).ok_or(PathFault::Malformed)?);
                }
            }
            // What the group's expressions cost to read, before the group is made into rows (it may make none).
            b.ctx.charge(b.r)?;
            at += n;
            b.group(command, &args)?;
            groups += 1;
            command = match command {
                'M' => 'L',
                'X' => 'Y',
                'Y' => 'X',
                other => other,
            };
        }
        // The command ended before its first group was whole, or its parameters were not parameters.
        if groups == 0 {
            return Err(PathFault::Malformed);
        }
    }
    b.end_set()?;
    Ok(b.done.into_iter().map(|s| (s.rows, s.no_fill, s.no_line)).collect())
}

/// Reads the whole of a custom shape's geometry.
pub(crate) struct Custom {
    /// The `Geometry` sections (IX from 0), or none when the shape is drawn as its box by the caller.
    pub sections: Vec<Arc<Section>>,
    /// The `User` section: `ODFMod<n>` and `ODFEq<i>` rows.
    pub user: Option<Section>,
    /// The `Control` section: a row for each editable handle.
    pub controls: Option<Section>,
    /// Rows `IX` 4 on of the `Connection` section: a custom shape's listed glue points.
    pub glue: Vec<Arc<Row>>,
    /// Whether the standard rows 0 to 3 are made.
    pub standard_glue: bool,
    /// The text block's cells, but its turn.
    pub text_block: Vec<(&'static str, Cell)>,
    /// `draw:text-rotate-angle`, degrees, as the file says it (0 when it says none or a turn that is no number): the caller makes `TxtAngle` of it beside the shape's
    /// flips (`read::shapes::text_turn`).
    pub text_turn: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// The geometry draws as the frame's box: the caller gives it the rectangle.
    pub as_box: bool,
    /// What the shape is, when it is an object DiaCraft cannot draw: `extrusion` or `fontwork`.
    pub object: Option<&'static str>,
}

impl Default for Custom {
    fn default() -> Custom {
        Custom {
            sections: Vec::new(),
            user: None,
            controls: None,
            glue: Vec::new(),
            standard_glue: true,
            text_block: Vec::new(),
            text_turn: 0.0,
            mirror_x: false,
            mirror_y: false,
            as_box: true,
            object: None,
        }
    }
}

/// The mirrors a custom shape's `draw:enhanced-geometry` (its attributes `attrs`) gives the model as `FlipX` and `FlipY`: none for an object the
/// reader does not draw (an extrusion or a fontwork), which keeps no mirror. What [`read`] and the shape's caller do, for the first pass of a page to ask first.
pub(crate) fn mirrors(attrs: &Attrs<'_>) -> (bool, bool) {
    let flag = |local: &str| attrs.peek(Ns::Draw, local).and_then(units::boolean).unwrap_or(false);
    if flag("extrusion") || flag("text-path") { (false, false) } else { (flag("mirror-horizontal"), flag("mirror-vertical")) }
}

/// Whether the geometry whose attributes `draw:extrusion`, `draw:text-path` and `svg:viewBox` have these values makes [`read`] stop **before it reads a glue point**: an object the reader does not
/// draw (an extrusion or a fontwork) returns at once, and so does a view box with no width or height (the shape is drawn as its frame). A custom shape that lists glue points and
/// is one of these has the four standard rows and none of the listed ones, so what its geometry lists says nothing of the rows it has: the writer asks this to count the points of the
/// geometry element a file held that are rows of the model. Each value is as the file spells it, `None` for no such attribute.
pub(crate) fn reads_no_glue_points(extrusion: Option<&str>, text_path: Option<&str>, view_box: Option<&str>) -> bool {
    let on = |value: Option<&str>| value.and_then(units::boolean).unwrap_or(false);
    on(extrusion) || on(text_path) || matches!(view_box.map(super::geometry::view_box), Some(Some(Err(_))))
}

/// How the geometry's type reads: a preset (`ooxml-*`), or another name.
pub(crate) fn preset_name(ty: &str) -> Option<&str> {
    ty.strip_prefix("ooxml-").filter(|n| !n.is_empty())
}

/// The `Geometry` sections of the preset `name` at the frame's size: one section for each of its
/// paths, as `Rel` rows of values (a share of the frame, y flipped), each row and cell charged as it is made. `None` for a name the gallery
/// does not hold.
fn preset_sections(r: &mut Reader, name: &str, space: Space, mods: &[f64]) -> Result<Option<Parts>, OdgError> {
    use kurbo::PathEl;
    let Some((geometry, dw, dh)) = super::preset::geometry_of(name, space.w, space.h, mods) else { return Ok(None) };
    if geometry.paths.len() > diacraft_doc::MAX_SECTIONS_PER_SHEET.saturating_sub(4) {
        return Err(OdgError::TooComplex { what: "subpaths in one shape", max: diacraft_doc::MAX_SECTIONS_PER_SHEET as u64 });
    }
    let share = |p: kurbo::Point| (round9(p.x / dw), round9(1.0 - p.y / dh));
    let mut out = Vec::with_capacity(geometry.paths.len());
    for sub in &geometry.paths {
        let mut rows: Vec<RowOut> = Vec::new();
        let mut start: Option<(f64, f64)> = None;
        let mut pen = (0.0, 0.0);
        for el in sub.path.elements() {
            let made: Option<RowOut> = match *el {
                PathEl::MoveTo(p) => {
                    let (x, y) = share(p);
                    start = Some((x, y));
                    pen = (x, y);
                    Some(("RelMoveTo", vec![("X", cells::plain(x)), ("Y", cells::plain(y))]))
                }
                PathEl::LineTo(p) => {
                    let (x, y) = share(p);
                    pen = (x, y);
                    Some(("RelLineTo", vec![("X", cells::plain(x)), ("Y", cells::plain(y))]))
                }
                PathEl::QuadTo(q, p) => {
                    let ((a, b), (x, y)) = (share(q), share(p));
                    pen = (x, y);
                    Some(("RelQuadBezTo", vec![("X", cells::plain(x)), ("Y", cells::plain(y)), ("A", cells::plain(a)), ("B", cells::plain(b))]))
                }
                PathEl::CurveTo(c1, c2, p) => {
                    let ((a, b), (c, d), (x, y)) = (share(c1), share(c2), share(p));
                    pen = (x, y);
                    Some((
                        "RelCubBezTo",
                        vec![
                            ("X", cells::plain(x)),
                            ("Y", cells::plain(y)),
                            ("A", cells::plain(a)),
                            ("B", cells::plain(b)),
                            ("C", cells::plain(c)),
                            ("D", cells::plain(d)),
                        ],
                    ))
                }
                // A close is a line back to the subpath's first point, left out when the pen is there already.
                PathEl::ClosePath => start.filter(|s| *s != pen).map(|(x, y)| {
                    pen = (x, y);
                    ("RelLineTo", vec![("X", cells::plain(x)), ("Y", cells::plain(y))])
                }),
            };
            if let Some((ty, cells_out)) = made {
                if rows.len() >= diacraft_doc::MAX_ROWS_PER_SECTION {
                    return Err(OdgError::TooComplex { what: "rows in one subpath", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
                }
                r.take_items(super::cost::ROW)?;
                r.take_cells(cells_out.len())?;
                rows.push((ty, cells_out));
            }
        }
        r.take_items(super::cost::SECTION)?;
        out.push((rows, sub.fill == super::preset::FillMode::None, !sub.stroke));
    }
    Ok(Some(out))
}

/// Reads `eg`, the `draw:enhanced-geometry` of a custom shape of `w` by `h` inches. `has_fill` and `has_stroke` are what
/// `hasfill` and `hasstroke` read. The element's own namespace declarations are in force: the caller entered it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    eg: &'t El,
    space_of: (f64, f64, Units),
    pcx_budget: &mut Budget,
    has: (bool, bool),
    object_first: bool,
) -> Result<Custom, OdgError> {
    let (w, h, u) = space_of;
    let mut attrs = names.attrs(eg);
    r.saw_attrs(attrs.len());
    let mut out = Custom::default();
    let flag = |attrs: &mut Attrs<'_>, local: &str| attrs.peek(Ns::Draw, local).and_then(units::boolean).unwrap_or(false);
    (out.mirror_x, out.mirror_y) = (flag(&mut attrs, "mirror-horizontal"), flag(&mut attrs, "mirror-vertical"));
    if object_first {
        for (local, kind) in [("extrusion", "extrusion"), ("text-path", "fontwork")] {
            if flag(&mut attrs, local) {
                out.object = Some(kind);
                return Ok(out);
            }
        }
    }
    let ty = attrs.peek(Ns::Draw, "type").unwrap_or("non-primitive");
    let vb = match attrs.peek(Ns::Svg, "viewBox").map(super::geometry::view_box) {
        Some(Some(Ok(vb))) => vb,
        Some(Some(Err(_))) => {
            r.warn("view_box_empty", || "a custom shape's view box has a width or height of 0; it is drawn as its frame".to_owned());
            return Ok(out);
        }
        _ => {
            r.warn("view_box_unreadable", || "a custom shape's view box is missing or cannot be read; 0 0 21600 21600 is used".to_owned());
            ViewBox { left: 0.0, top: 0.0, width: 21600.0, height: 21600.0 }
        }
    };
    let space = Space { w, h, u, vb };
    let mods = modifiers(r, attrs.peek(Ns::Draw, "modifiers"))?;
    let mut equations_in = Vec::new();
    for child in eg.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "equation") {
            equations_in.push(child);
            if equations_in.len() > equations::MOST_EQUATIONS {
                names.leave(mark);
                return Err(OdgError::TooComplex { what: "equations in one shape", max: equations::MOST_EQUATIONS as u64 });
            }
        }
        names.leave(mark);
    }
    let mut ctx = Ctx {
        by_name: ByName::new(),
        vb,
        stretch: (
            attrs.peek(Ns::Draw, "path-stretchpoint-x").and_then(units::number_any).unwrap_or(0.0),
            attrs.peek(Ns::Draw, "path-stretchpoint-y").and_then(units::number_any).unwrap_or(0.0),
        ),
        log: (w * 2540.0, h * 2540.0),
        has_fill: has.0,
        has_stroke: has.1,
        mods,
        values: Vec::new(),
        work: equations::Work::default(),
        named_mods: BTreeSet::new(),
        unpaid: 0,
    };
    let user_rows = equation_rows(r, names, &equations_in, &mut ctx)?;
    out.as_box = false;
    let path = attrs.peek(Ns::Draw, "enhanced-path");
    match path {
        Some(text) => match path_sections(r, &mut ctx, pcx_budget, space, text) {
            Ok(parts) => out.sections = sections_of(r, parts)?,
            Err(PathFault::Error(e)) => return Err(e),
            Err(PathFault::Malformed) => {
                r.warn("geometry_unreadable", || "a custom shape's path breaks the syntax; it is drawn as its frame".to_owned());
                out.as_box = true;
            }
        },
        None => {
            // A type and no path. An `ooxml-` name the preset gallery holds is drawn from the preset, evaluated at the frame's size
            // with the modifiers as its adjust values and read as values; any other type is the frame's box and counted. The element is
            // kept either way.
            match preset_name(ty).map(|name| preset_sections(r, name, space, &ctx.mods)).transpose()? {
                Some(Some(parts)) => {
                    r.keep("shape_type_preset");
                    out.sections = sections_of(r, parts)?;
                }
                _ => {
                    out.as_box = true;
                    r.keep("shape_type");
                    // A `rectangle` is its frame's box exactly: counted, as R31 says, and not said to a person as a stand-in (`unknown_shape_type`).
                    if ty != "rectangle" {
                        r.report.boxed_shapes = r.report.boxed_shapes.saturating_add(1);
                    }
                }
            }
        }
    }
    text_block(r, &mut ctx, space, attrs.peek(Ns::Draw, "text-areas"), attrs.peek(Ns::Draw, "text-rotate-angle"), &mut out)?;
    out.standard_glue =
        !(attrs.peek(Ns::Draw, "glue-point-type") == Some("none") && attrs.peek(Ns::Draw, "glue-points").is_none_or(|g| g.trim().is_empty()));
    out.glue = listed_glue(r, &mut ctx, space, attrs.peek(Ns::Draw, "glue-points"), attrs.peek(Ns::Draw, "glue-point-leaving-directions"))?;
    let handles = handles(r, names, eg, &mut ctx, space)?;
    out.controls = handles.0;
    out.user = user_section(r, &mut ctx, user_rows, handles.1)?;
    r.report.work.equations = r.report.work.equations.saturating_add(ctx.work.equations);
    r.report.work.equation_ops = r.report.work.equation_ops.saturating_add(ctx.work.ops);
    r.report.work.equation_steps = r.report.work.equation_steps.saturating_add(ctx.work.steps);
    r.report.work.expression_bytes = r.report.work.expression_bytes.saturating_add(ctx.work.compiled);
    Ok(out)
}

/// The values of `draw:modifiers`: numbers separated by white space; one that is not a number reads as 0, with a warning.
fn modifiers(r: &mut Reader, text: Option<&str>) -> Result<Vec<f64>, OdgError> {
    let Some(text) = text else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for item in text.split_ascii_whitespace() {
        if out.len() >= diacraft_doc::MAX_ROWS_PER_SECTION {
            return Err(OdgError::TooComplex { what: "modifiers in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
        }
        out.push(units::number_any(item).unwrap_or_else(|| {
            r.warn("modifier_unreadable", || "a modifier is not a number; it reads as 0".to_owned());
            0.0
        }));
    }
    r.path_units(out.len());
    Ok(out)
}

/// The equations' `User` rows: compiled, evaluated once each in dependency order, and made into cells charged as they go.
fn equation_rows<'t>(r: &mut Reader, names: &mut Names<'t>, elements: &[&'t El], ctx: &mut Ctx<'t>) -> Result<Vec<Arc<Row>>, OdgError> {
    let mut sources: Vec<(&'t str, &'t str)> = Vec::with_capacity(elements.len());
    for el in elements {
        let mark = names.enter(el);
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        sources.push((attrs.peek(Ns::Draw, "name").unwrap_or(""), attrs.peek(Ns::Draw, "formula").unwrap_or("")));
        names.leave(mark);
    }
    for (i, (name, _)) in sources.iter().enumerate() {
        if !name.is_empty() {
            ctx.by_name.entry(name).or_insert(u32::try_from(i).unwrap_or(u32::MAX));
        }
    }
    let mut compiled: Vec<Compiled> = Vec::with_capacity(sources.len());
    for (_, formula) in &sources {
        r.path_units(1);
        ctx.work.compiled = ctx.work.compiled.saturating_add(formula.len() as u64);
        let c = equations::compile(formula, &ctx.by_name, &ctx.env());
        // What the compiled formula holds (its operations, names and translation) is held with the others of the shape until they are
        // evaluated: charged before the next is read. The cell's tree is charged by its nodes when the cell is made.
        r.take_model_bytes(c.bytes())?;
        for n in &c.mods {
            ctx.named_mods.insert(*n);
        }
        compiled.push(c);
    }
    ctx.evaluate(&compiled);
    let mut rows = Vec::with_capacity(compiled.len());
    for (i, c) in compiled.iter().enumerate() {
        let value = ctx.values.get(i).copied().unwrap_or(Eval { value: 0.0, error: None });
        let mut cell = match (&c.error, c.text.is_empty()) {
            (None, false) => r.forms.cell(&c.text, value.value, None),
            _ => cells::plain(0.0),
        };
        if let Some(code) = value.error {
            cell.error = Some(Arc::from(code));
            if c.error.is_some() {
                r.warn("equation_unreadable", || "an equation cannot be read; its cell is an error".to_owned());
            }
        }
        r.take_items(super::cost::ROW)?;
        r.take_cells(1)?;
        rows.push(named_row(&format!("ODFEq{i}"), vec![("Value", cell)]));
    }
    Ok(rows)
}

/// A row keyed by its name.
fn named_row(name: &str, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    for (n, cell) in cells {
        row.cells.insert(CellName::new(n), cell);
    }
    Arc::new(row)
}

/// The `Geometry` sections of what the builder made, charged already.
fn sections_of(r: &mut Reader, parts: Parts) -> Result<Vec<Arc<Section>>, OdgError> {
    let mut out = Vec::with_capacity(parts.len());
    for (n, (rows, no_fill, no_line)) in parts.into_iter().enumerate() {
        let mut section = Section::new("Geometry", Some(u32::try_from(n).unwrap_or(u32::MAX)));
        let flags: &[(&str, bool)] = match (no_fill, no_line) {
            (true, true) => &[("NoShow", true)],
            (true, false) => &[("NoFill", true)],
            (false, true) => &[("NoLine", true)],
            (false, false) => &[],
        };
        for (name, on) in flags {
            if *on {
                r.take_items(super::cost::SECTION_CELLS)?;
                r.take_cells(1)?;
                section.cells.insert(CellName::new(name), cells::plain(1.0));
            }
        }
        for (i, (ty, cells)) in rows.into_iter().enumerate() {
            section.rows.push(row(u32::try_from(i + 1).unwrap_or(u32::MAX), Some(ty), cells));
        }
        out.push(Arc::new(section));
    }
    Ok(out)
}

/// `draw:text-areas`' first rectangle and `draw:text-rotate-angle`.
fn text_block(r: &mut Reader, ctx: &mut Ctx<'_>, space: Space, areas: Option<&str>, angle: Option<&str>, out: &mut Custom) -> Result<(), OdgError> {
    if let Some(areas) = areas {
        let tokens: Vec<&str> = areas.split_ascii_whitespace().collect();
        if let [l, t, rr, b, ..] = tokens.as_slice() {
            let (l, t, rr, b) = (ctx.param(l), ctx.param(t), ctx.param(rr), ctx.param(b));
            ctx.charge(r)?;
            if let Some(code) = [&l, &t, &rr, &b].iter().find_map(|q| q.err) {
                r.warn("text_areas_unreadable", || format!("a custom shape's text area is the error {code}; the text block is left to the frame"));
            } else {
                let vb = space.vb;
                let n = |v: f64| cells::number(v).text().to_owned();
                let (vw, vh, left, top) = (n(vb.width), n(vb.height), n(vb.left), n(vb.top));
                let text = |q: &Q| q.text();
                let width = format!("Width*(({})-({}))/{vw}", text(&rr), text(&l));
                let height = format!("Height*(({})-({}))/{vh}", text(&b), text(&t));
                let pin_x = format!("Width*((({})+({}))/2-{left})/{vw}", text(&l), text(&rr));
                let pin_y = format!("Height*(1-((({})+({}))/2-{top})/{vh})", text(&t), text(&b));
                let value = |a: &Q, z: &Q, size: f64, extent: f64| size * (z.v - a.v) / extent;
                let cells_out = [
                    ("TxtWidth", width, value(&l, &rr, space.w, vb.width), space.u.width),
                    ("TxtHeight", height, value(&t, &b, space.h, vb.height), space.u.height),
                    ("TxtPinX", pin_x, space.w * ((l.v + rr.v) / 2.0 - vb.left) / vb.width, space.u.width),
                    ("TxtPinY", pin_y, space.h * (1.0 - ((t.v + b.v) / 2.0 - vb.top) / vb.height), space.u.height),
                ];
                r.take_cells(cells_out.len())?;
                for (name, formula, v, unit) in cells_out {
                    out.text_block.push((name, formula_cell(&mut r.forms, &formula, v, unit)));
                }
            }
        }
    }
    if let Some(deg) = angle.and_then(units::angle_degrees).filter(|d| d.is_finite()) {
        out.text_turn = deg;
    }
    Ok(())
}

/// A custom shape's listed glue points, `draw:glue-points`: rows `IX` 4 on, `X` and `Y` by the mapping, the way out from
/// `draw:glue-point-leaving-directions` when it gives one.
fn listed_glue(r: &mut Reader, ctx: &mut Ctx<'_>, space: Space, points: Option<&str>, directions: Option<&str>) -> Result<Vec<Arc<Row>>, OdgError> {
    let Some(points) = points else { return Ok(Vec::new()) };
    // Counted before they are collected: a list longer than a section holds rows is refused first.
    if points.split_ascii_whitespace().count() / 2 > diacraft_doc::MAX_ROWS_PER_SECTION.saturating_sub(4) {
        return Err(OdgError::TooComplex { what: "glue points in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
    }
    let tokens: Vec<&str> = points.split_ascii_whitespace().collect();
    let angles: Vec<f64> = directions
        .map(|d| {
            d.split(|c: char| c == ';' || c == ',' || c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .map(|s| units::angle_degrees(s).unwrap_or(f64::NAN))
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    for (j, pair) in tokens.as_chunks::<2>().0.iter().enumerate() {
        let [px, py] = *pair;
        let (x, y) = (ctx.param(px), ctx.param(py));
        ctx.charge(r)?;
        let ix = u32::try_from(j + 4).unwrap_or(u32::MAX);
        let mut cells_out: Vec<(&'static str, Cell)> = vec![("X", glue_cell(r, space, Axis::X, &x)), ("Y", glue_cell(r, space, Axis::Y, &y))];
        if let Some(a) = angles.get(j).copied().filter(|a| a.is_finite()) {
            let (dx, dy) = (round15(a.to_radians().cos()), round15(-a.to_radians().sin()));
            cells_out.push(("DirX", cells::plain(dx)));
            cells_out.push(("DirY", cells::plain(dy)));
        }
        r.take_items(super::cost::ROW)?;
        r.take_cells(cells_out.len())?;
        rows.push(row(ix, None, cells_out));
    }
    Ok(rows)
}

/// An angle in degrees as radians' cosine or sine rounded to 15 places, as R16 says; `a` is read in degrees here.
fn round15(n: f64) -> f64 {
    let r = (n * 1.0e15).round() / 1.0e15;
    if r.is_finite() { r } else { 0.0 }
}

/// The cell of a glue point's coordinate: `Width*k` or the formula of R11.
fn glue_cell(r: &mut Reader, space: Space, axis: Axis, q: &Q) -> Cell {
    let (size, unit) = (space.size(axis), space.unit(axis));
    if let Some(code) = q.err {
        return error_cell(code, unit);
    }
    let share = space.share(axis, q.v);
    match &q.e {
        None => cells::share(&mut r.forms, Space::word(axis), share, size, unit),
        Some(e) => {
            let word = Space::word(axis);
            let part = space.share_text(axis, e);
            let text = if axis == Axis::X { format!("{word}*{part}") } else { format!("{word}*({part})") };
            formula_cell(&mut r.forms, &text, share * size, unit)
        }
    }
}

/// What a handle's position component is.
enum Comp {
    /// `$n`: driven by the modifier.
    Modifier(u32),
    /// A number or a view-box keyword.
    Constant(f64),
    /// Anything else: not editable.
    Formula,
}

fn component(text: &str, vb: &ViewBox) -> Comp {
    let text = text.trim();
    if let Some(n) = text.strip_prefix('$') {
        return n.parse::<u32>().map_or(Comp::Formula, Comp::Modifier);
    }
    match text {
        "left" => Comp::Constant(vb.left),
        "top" => Comp::Constant(vb.top),
        "right" => Comp::Constant(vb.left + vb.width),
        "bottom" => Comp::Constant(vb.top + vb.height),
        "width" => Comp::Constant(vb.width),
        "height" => Comp::Constant(vb.height),
        _ => plain_number(text).map_or(Comp::Formula, Comp::Constant),
    }
}

/// A modifier a handle drives: the formula its `User` row gets.
type Driven = BTreeMap<u32, String>;

/// The rows of the `Control` section (a formula spells it `Controls.`) of the editable handles, and the formulas of the modifiers they drive.
fn handles<'t>(r: &mut Reader, names: &mut Names<'t>, eg: &'t El, ctx: &mut Ctx<'_>, space: Space) -> Result<(Option<Section>, Driven), OdgError> {
    let mut section = Section::new(crate::sections::CONTROL, None);
    let mut driven = Driven::new();
    // `Row_<k>` names the handle by its place among the shape's handles, from 1 (so a row's name does not change with which handles before
    // it are editable).
    let mut k = 0usize;
    for child in eg.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "handle") {
            let attrs = names.attrs(child);
            r.saw_attrs(attrs.len());
            k += 1;
            if section.rows.len() >= diacraft_doc::MAX_ROWS_PER_SECTION {
                names.leave(mark);
                return Err(OdgError::TooComplex { what: "handles in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
            }
            let editable = read_handle(r, &attrs, ctx, space, k, &mut driven)?;
            match editable {
                Some(handle_row) => section.rows.push(handle_row),
                None => r.keep("handle"),
            }
        }
        names.leave(mark);
    }
    Ok(((!section.rows.is_empty()).then_some(section), driven))
}

/// The cell of a handle's position on `axis` when the view-box value is `v`: `Width*<share>` or `Height*<share>`, every digit of the share (the formula of the modifier it
/// drives, `X/Width*<VW>` or `(1-Y/Height)*<VH>`, gives `v` back from it, and the y share is measured as that formula measures it: from the bottom of the frame). A share
/// that is not a finite number, or a value that is not, leaves the cell the number it was.
fn handle_cell(r: &mut Reader, space: Space, axis: Axis, v: f64) -> Cell {
    let (size, unit) = (space.size(axis), space.unit(axis));
    let k = space.share(axis, v);
    if k.is_finite() && (k * size).is_finite() {
        cells::share_exact(&mut r.forms, Space::word(axis), k, size, unit)
    } else {
        cells::length(k * size, unit)
    }
}

/// One handle: its `Control` row when it is Cartesian with a position of `$n`s, numbers and keywords, and has no range, mirror or switch.
fn read_handle(
    r: &mut Reader,
    attrs: &Attrs<'_>,
    ctx: &mut Ctx<'_>,
    space: Space,
    k: usize,
    driven: &mut Driven,
) -> Result<Option<Arc<Row>>, OdgError> {
    let polar = attrs.peek(Ns::Draw, "handle-polar").is_some();
    let switched = attrs.peek(Ns::Draw, "handle-switched").and_then(units::boolean).unwrap_or(false)
        || attrs.peek(Ns::Draw, "handle-mirror-horizontal").and_then(units::boolean).unwrap_or(false)
        || attrs.peek(Ns::Draw, "handle-mirror-vertical").and_then(units::boolean).unwrap_or(false);
    let pair = attrs.peek(Ns::Draw, "handle-position");
    let (px, py) = match (attrs.peek(Ns::Draw, "handle-position-x"), attrs.peek(Ns::Draw, "handle-position-y"), pair) {
        (Some(x), Some(y), _) => (x, y),
        (_, _, Some(both)) => {
            let mut it = both.split_ascii_whitespace();
            match (it.next(), it.next()) {
                (Some(x), Some(y)) => (x, y),
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    // Polar, ranged, mirrored and switched handles are kept and not editable. A range is any `draw:handle-range-*` or
    // `draw:handle-radius-range-*` attribute.
    let ranged = attrs.iter().any(|a| a.ns == Ns::Draw && (a.local.starts_with("handle-range-") || a.local.starts_with("handle-radius-range-")));
    if polar || switched || ranged {
        return Ok(None);
    }
    let (cx, cy) = (component(px, &space.vb), component(py, &space.vb));
    let value = |ctx: &Ctx<'_>, c: &Comp| match c {
        Comp::Modifier(n) => Some(ctx.mods.get(*n as usize).copied().unwrap_or(0.0)),
        Comp::Constant(v) => Some(*v),
        Comp::Formula => None,
    };
    let (Some(vx), Some(vy)) = (value(ctx, &cx), value(ctx, &cy)) else { return Ok(None) };
    let name = format!("Row_{k}");
    let (x, y) = (space.w * space.share(Axis::X, vx), space.h * space.share(Axis::Y, vy));
    // The position is a share of the frame, so that a resize of the shape moves the handle with it and the modifier it drives, which reads the share back, stays what the
    // file says (`$n` is a value of the view box, which the frame maps).
    let (x_cell, y_cell) = (handle_cell(r, space, Axis::X, vx), handle_cell(r, space, Axis::Y, vy));
    let n = |v: f64| cells::number(v).text().to_owned();
    if let Comp::Modifier(m) = cx {
        let origin = if space.vb.left == 0.0 { String::new() } else { format!("+{}", n(space.vb.left)) };
        driven.entry(m).or_insert_with(|| format!("Controls.{name}.X/Width*{}{origin}", n(space.vb.width)));
        ctx.named_mods.insert(m);
    }
    if let Comp::Modifier(m) = cy {
        let origin = if space.vb.top == 0.0 { String::new() } else { format!("+{}", n(space.vb.top)) };
        driven.entry(m).or_insert_with(|| format!("(1-Controls.{name}.Y/Height)*{}{origin}", n(space.vb.height)));
        ctx.named_mods.insert(m);
    }
    let cells_out = vec![
        ("X", x_cell),
        ("Y", y_cell),
        ("XDyn", r.forms.cell(&format!("Controls.{name}.X"), x, space.u.width)),
        ("YDyn", r.forms.cell(&format!("Controls.{name}.Y"), y, space.u.height)),
    ];
    r.take_items(super::cost::ROW)?;
    r.take_cells(cells_out.len())?;
    let mut handle_row = Row { name: Some(Arc::from(name.as_str())), ..Row::default() };
    for (cell_name, cell) in cells_out {
        handle_row.cells.insert(CellName::new(cell_name), cell);
    }
    Ok(Some(Arc::new(handle_row)))
}

/// The `User` section: `ODFMod<n>` for each modifier, as the number or, when a handle drives it, the formula of R13, and a row of 0 for
/// each modifier past the list that a formula names, then the equations' rows.
fn user_section(r: &mut Reader, ctx: &mut Ctx<'_>, equations: Vec<Arc<Row>>, driven: Driven) -> Result<Option<Section>, OdgError> {
    let past: Vec<u32> = ctx.named_mods.iter().copied().filter(|n| *n as usize >= ctx.mods.len()).collect();
    let rows = ctx.mods.len() + past.len() + equations.len();
    if rows == 0 {
        return Ok(None);
    }
    if rows > diacraft_doc::MAX_ROWS_PER_SECTION {
        return Err(OdgError::TooComplex { what: "modifiers and equations in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
    }
    r.take_items(super::cost::SECTION)?;
    let mut section = Section::new(crate::sections::USER, None);
    for (n, v) in ctx.mods.clone().into_iter().enumerate() {
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        let cell = match driven.get(&n) {
            Some(formula) => r.forms.cell(formula, v, None),
            None => cells::plain(v),
        };
        r.take_items(super::cost::ROW)?;
        r.take_cells(1)?;
        section.rows.push(named_row(&format!("ODFMod{n}"), vec![("Value", cell)]));
    }
    for n in past {
        r.warn("modifier_missing", || "a formula names a modifier past the end of draw:modifiers; it reads as 0".to_owned());
        r.take_items(super::cost::ROW)?;
        r.take_cells(1)?;
        let cell = match driven.get(&n) {
            Some(formula) => r.forms.cell(formula, 0.0, None),
            None => cells::plain(0.0),
        };
        section.rows.push(named_row(&format!("ODFMod{n}"), vec![("Value", cell)]));
    }
    section.rows.extend(equations);
    Ok(Some(section))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A geometry the reader stops at before its glue points (an extrusion or a fontwork, `true` exactly; a view box with no width or height) is told by the values of three attributes
    /// (the writer counts the points a geometry lists that are rows of the model by it).
    #[test]
    fn a_geometry_the_reader_stops_at_before_its_glue_points_is_told_by_three_attributes() {
        assert!(reads_no_glue_points(Some("true"), None, None));
        assert!(reads_no_glue_points(None, Some("true"), None));
        assert!(reads_no_glue_points(None, None, Some("0 0 0 21600")));
        assert!(reads_no_glue_points(None, None, Some("0,0,21600,0")));
        assert!(reads_no_glue_points(Some("false"), Some("false"), Some("0 0 -5 21600")));
        assert!(!reads_no_glue_points(None, None, None));
        assert!(!reads_no_glue_points(Some("false"), Some("false"), Some("0 0 21600 21600")));
        assert!(!reads_no_glue_points(Some("1"), Some("TRUE"), Some("0 0 21600 21600")), "`true` and `false` are the only booleans the reader reads");
        assert!(!reads_no_glue_points(None, None, Some("0 0 21600")), "a view box of three numbers is unreadable and read as the default");
        assert!(!reads_no_glue_points(None, None, Some("")));
    }

    #[test]
    fn a_plain_number_is_a_token_that_starts_like_one_and_is_finite() {
        for (token, want) in [("5", Some(5.0)), ("-5", Some(-5.0)), ("+5", Some(5.0)), (".5", Some(0.5)), ("0", Some(0.0)), ("1e2", Some(100.0))] {
            assert_eq!(plain_number(token), want, "{token}");
        }
        for token in ["", "abc", "?f0", "$0", "inf", "nan", "-inf", "+", "-", ".", "left", "1e999", "5x", " 5"] {
            assert_eq!(plain_number(token), None, "{token:?}");
        }
    }

    #[test]
    fn a_formula_cell_that_would_nest_past_the_limit_holds_its_value_alone() {
        let mut forms = cells::Forms::default();
        let nested = |depth: usize| format!("{}5{}", "(".repeat(depth), ")".repeat(depth));
        let at_limit = formula_cell(&mut forms, &nested(equations::MOST_NESTING), 5.0, Some("IN"));
        assert_eq!(at_limit.formula.text().map(str::len), Some(2 * equations::MOST_NESTING + 1));
        let past = formula_cell(&mut forms, &nested(equations::MOST_NESTING + 1), 5.0, Some("IN"));
        assert!(past.formula.text().is_none());
        assert_eq!((past.value.as_ref().and_then(diacraft_doc::Value::number), past.unit.as_deref()), (Some(5.0), Some("IN")));
        assert_eq!(past.value, at_limit.value);
    }

    #[test]
    fn a_single_term_has_no_operator_bracket_or_space() {
        for text in ["User.ODFEq0", "User.ODFMod12", "5", "Width", "a_b", "Sheet.3!X"] {
            assert!(single_term(text), "{text}");
        }
        for text in ["", "a+b", "(a)", "a b", "-a", "a*b", "a,b", "f(x)"] {
            assert!(!single_term(text), "{text:?}");
        }
        assert_eq!(paren("User.ODFEq0"), "User.ODFEq0");
        assert_eq!(paren("a+b"), "(a+b)");
        assert_eq!(paren("(a+b)"), "(a+b)");
        assert_eq!(paren("(a)+(b)"), "((a)+(b))", "the first bracket closes before the end");
        assert_eq!(paren("(a)*(b)+1"), "((a)*(b)+1)");
    }

    #[test]
    fn a_whole_bracket_is_closed_by_the_last_character() {
        for text in ["(a)", "(a+(b))", "((a)+(b))", "()"] {
            assert!(whole_bracket(text), "{text}");
        }
        for text in ["", "a", "(a", "a)", "(a)(b)", "(a)+(b)", "((a)", ")("] {
            assert!(!whole_bracket(text), "{text:?}");
        }
    }

    #[test]
    fn quantities_carry_their_expression_only_when_a_part_has_one() {
        let (a, b) = (Q::num(2.0), Q { v: 3.0, e: Some("User.ODFEq0".to_owned()), err: None });
        assert_eq!(a.add(&Q::num(1.0)).e, None);
        assert_eq!(a.add(&b).e.as_deref(), Some("(2+User.ODFEq0)"));
        assert_eq!(b.sub(&a).e.as_deref(), Some("(User.ODFEq0-2)"));
        assert_eq!(a.mul(&b).e.as_deref(), Some("(2*User.ODFEq0)"));
        assert_eq!(b.sin().e.as_deref(), Some("SIN(User.ODFEq0)"));
        assert_eq!(b.cos().e.as_deref(), Some("COS(User.ODFEq0)"));
        assert_eq!(b.atan2(&a).e.as_deref(), Some("ATAN2(User.ODFEq0,2)"));
        assert_eq!(b.to_radians().e.as_deref(), Some("(User.ODFEq0*PI()/180)"));
        assert_eq!(b.neg().e.as_deref(), Some("(-User.ODFEq0)"));
        assert_eq!(b.scaled(2.0).v, 6.0);
        assert_eq!(b.clone_abs().e.as_deref(), Some("User.ODFEq0"));
        let negative = Q { v: -3.0, e: Some("User.ODFEq0".to_owned()), err: None };
        assert_eq!((negative.clone_abs().v, negative.clone_abs().e.as_deref()), (3.0, Some("ABS(User.ODFEq0)")));
        // An error is carried, and its expression is let go of; a result that is not a number is an error.
        let bad = Q { v: 0.0, e: None, err: Some("#REF!") };
        assert_eq!(a.add(&bad).err, Some("#REF!"));
        assert_eq!(Q::num(f64::NAN).v, 0.0);
        assert_eq!(Q::num(1e308).mul(&Q::num(10.0)).err, Some("#NUM!"));
        // An expression past the limit is let go of and the value stands: no expression is longer than the limit, however long the chain.
        let mut long = b.clone();
        for step in 0..200 {
            long = long.add(&b);
            assert!(long.e.as_ref().is_none_or(|e| e.len() <= MOST_FORMULA), "step {step}");
            assert_eq!(long.v, 3.0 * (step as f64 + 2.0));
        }
    }

    #[test]
    fn the_space_maps_view_box_values_to_shares_and_expressions() {
        let vb = ViewBox { left: 100.0, top: 200.0, width: 1000.0, height: 500.0 };
        let space = Space { w: 4.0, h: 2.0, u: Units { width: None, height: None }, vb };
        assert_eq!(space.share(Axis::X, 600.0), 0.5);
        assert_eq!(space.share(Axis::Y, 450.0), 0.5);
        assert_eq!(space.share(Axis::Y, 200.0), 1.0, "y flips");
        assert_eq!(space.share_text(Axis::X, "User.ODFEq0"), "(User.ODFEq0-100)/1000");
        assert_eq!(space.share_text(Axis::Y, "User.ODFEq0"), "1-(User.ODFEq0-200)/500");
        let origin = Space { vb: ViewBox { left: 0.0, top: 0.0, width: 21600.0, height: 21600.0 }, ..space };
        assert_eq!(origin.share_text(Axis::X, "User.ODFEq0"), "User.ODFEq0/21600");
        assert_eq!(origin.share_text(Axis::Y, "a+b"), "1-(a+b)/21600");
        assert_eq!((origin.size(Axis::X), origin.size(Axis::Y)), (4.0, 2.0));
        assert_eq!((Space::word(Axis::X), Space::word(Axis::Y)), ("Width", "Height"));
    }

    #[test]
    fn a_path_is_split_into_commands_and_parameters() {
        let shown = |path: &str| -> String {
            tokens(path)
                .iter()
                .map(|t| match t {
                    Tok::Cmd(c) => format!("[{c}]"),
                    Tok::Param(p) => (*p).to_owned(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(shown("M 0 0 L 1,2 Z N"), "[M] 0 0 [L] 1 2 [Z] [N]");
        // A letter is a command at the start of a token only: a number runs to the next white space or comma.
        assert_eq!(shown("M0 0L1 2Z"), "[M] 0 0L1 2Z");
        assert_eq!(shown("M ?f0 $1 left"), "[M] ?f0 $1 left");
        assert_eq!(shown(""), "");
        for (c, n) in [
            ('M', 2),
            ('L', 2),
            ('X', 2),
            ('Y', 2),
            ('Q', 4),
            ('G', 4),
            ('C', 6),
            ('T', 6),
            ('U', 6),
            ('A', 8),
            ('B', 8),
            ('V', 8),
            ('W', 8),
            ('Z', 0),
            ('N', 0),
            ('F', 0),
            ('S', 0),
        ] {
            assert_eq!(arity(c), Some(n), "{c}");
        }
        assert_eq!(arity('m'), None);
        assert_eq!(arity('K'), None);
    }

    #[test]
    fn components_of_a_handle_position() {
        let vb = ViewBox { left: 10.0, top: 20.0, width: 100.0, height: 200.0 };
        assert!(matches!(component("$3", &vb), Comp::Modifier(3)));
        assert!(matches!(component(" $0 ", &vb), Comp::Modifier(0)));
        for (word, want) in
            [("left", 10.0), ("top", 20.0), ("right", 110.0), ("bottom", 220.0), ("width", 100.0), ("height", 200.0), ("7", 7.0), ("-7.5", -7.5)]
        {
            assert!(matches!(component(word, &vb), Comp::Constant(v) if v == want), "{word}");
        }
        for text in ["?f0", "$x", "$-1", "1+1", "", "nope"] {
            assert!(matches!(component(text, &vb), Comp::Formula), "{text:?}");
        }
        assert_eq!(round15(0.123_456_789_012_345_67), 0.123_456_789_012_346);
        assert_eq!(round15(f64::NAN), 0.0);
    }
}
