//! The equations of an enhanced geometry: each `draw:formula` is read
//! once into a program, translated term by term into the ShapeSheet text of its `User` row, and evaluated once, in dependency order.
//!
//! **Why not WordCraft's evaluator** (`storytold/wordcraft`, `crates/formats/src/odt/geometry.rs`, Apache-2.0 or MIT): it works out a
//! value by recursive descent through the equations a formula names, to a depth of 64, so a chain of 10,000 equations each naming the one
//! before is past it; it keeps no text for a cell and counts nothing. This module is written for what this reader needs: an explicit stack in
//! the parser (a formula of a million opening parentheses is a refusal, not a stack overflow), the translation of R10 made in the same pass,
//! an evaluation by Tarjan's walk of the equations on stacks of its own that looks at each equation and each dependency once (a diamond,
//! each equation naming the two before, is linear, and so is a chain with a back-edge from every entry to its first), a cycle named as
//! `#REF!` on the members of each strongly connected component of more than one equation (or one that names itself), and counters on
//! equations, steps and operations.
//!
//! **Cost.** A formula is read once, in time linear in its length, into at most one operation for each token; what the compiled formula holds
//! ([`Compiled::bytes`]) is charged by the caller before it reads the next, and the tree of the cell it goes into by its nodes
//! (`cells::Forms`). An equation is evaluated once: [`Work::equations`] counts the equations visited and
//! [`Work::ops`] the operations run, and neither depends on how many equations name one.
//!
//! **Errors.** A formula that breaks the syntax, names an equation that is not there, calls a function this module does not know, or nests
//! deeper than [`MOST_NESTING`] is `#NAME?`; a cycle is `#REF!` on each of its members; a division by zero is `#DIV/0!`; any other result
//! that is not a finite number is `#NUM!`. An error reaches every equation that reads the one that has it, as a cell reads an erroring cell.

use std::collections::HashMap;

use super::geometry::ViewBox;

/// The most equations one shape's geometry holds that are read.
pub(crate) const MOST_EQUATIONS: usize = 10_000;

/// The deepest nesting in one formula's translation: the formula crate's own limit on the text a cell holds (`diacraft_formula::MAX_NESTING`),
/// so that every translation can be parsed back. It is counted on the translation, which nests more than the source does: an `if` becomes
/// `IF((`, a call and a bracket round its condition, so two levels for one of the source's; `logwidth` after a `/` is bracketed and holds a
/// bracket of its own; a sign nests until its operand ends. [`nesting`] counts what the formula crate counts, and [`compile`] refuses a
/// formula whose translation is past it, as it refuses one that breaks the syntax.
pub(crate) const MOST_NESTING: usize = 100;

/// The nesting the formula crate counts in `text`, a ShapeSheet formula written by this reader: each `(` (a bracket, or the arguments of a
/// call) is one level until its `)`, and each sign (a `-` where an operand is expected) is one until its operand ends. The count stops past
/// [`MOST_NESTING`], so a longer text costs no more to count than that and the open brackets are never more than that.
pub(crate) fn nesting(text: &str) -> usize {
    let mut depth = 0usize;
    let mut deepest = 0usize;
    // The signs open now (before their operand), and the signs that were open outside each bracket now open.
    let mut signs = 0usize;
    let mut outer: Vec<usize> = Vec::new();
    // True where an operand is expected: at the start, after an operator, after `(` and after `,`.
    let mut operand = true;
    for b in text.bytes() {
        match b {
            b' ' | b'\t' => {}
            b'(' => {
                outer.push(signs);
                signs = 0;
                depth += 1;
                operand = true;
            }
            b')' => {
                // The signs of the last operand end with it, and so do those that wrapped the bracket, when it closes.
                depth = depth.saturating_sub(signs);
                let wrapping = outer.pop().unwrap_or(0);
                depth = depth.saturating_sub(1 + wrapping);
                signs = 0;
                operand = false;
            }
            b'-' if operand => {
                signs += 1;
                depth += 1;
            }
            b'+' | b'-' | b'*' | b'/' | b'^' | b'&' | b'<' | b'>' | b'=' | b',' => {
                depth = depth.saturating_sub(signs);
                signs = 0;
                operand = true;
            }
            _ => operand = false,
        }
        deepest = deepest.max(depth);
        if deepest > MOST_NESTING {
            break;
        }
    }
    deepest
}

/// What an expression is read against: the view box, the frame, and the shape's own switches.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Env<'a> {
    pub vb: ViewBox,
    /// `draw:path-stretchpoint-x` and `-y`, 0 when absent.
    pub stretch: (f64, f64),
    /// The frame's width and height in hundredths of a millimetre (`logwidth`, `logheight`).
    pub log: (f64, f64),
    pub has_fill: bool,
    pub has_stroke: bool,
    /// The values of `draw:modifiers`.
    pub mods: &'a [f64],
}

/// The identifiers of a formula that stand for a number of the shape's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Var {
    Left,
    Top,
    Right,
    Bottom,
    Width,
    Height,
    LogWidth,
    LogHeight,
    XStretch,
    YStretch,
    HasFill,
    HasStroke,
    Pi,
}

/// One function of one argument, or two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Func {
    Abs,
    Sqrt,
    Sin,
    Cos,
    Tan,
    Atan,
    Min,
    Max,
    Atan2,
}

impl Func {
    fn of(name: &str) -> Option<(Func, usize)> {
        Some(match name {
            "abs" => (Func::Abs, 1),
            "sqrt" => (Func::Sqrt, 1),
            "sin" => (Func::Sin, 1),
            "cos" => (Func::Cos, 1),
            "tan" => (Func::Tan, 1),
            "atan" => (Func::Atan, 1),
            "min" => (Func::Min, 2),
            "max" => (Func::Max, 2),
            "atan2" => (Func::Atan2, 2),
            _ => return None,
        })
    }

    /// The ShapeSheet function the translation calls.
    fn shown(self) -> &'static str {
        match self {
            Func::Abs => "ABS",
            Func::Sqrt => "SQRT",
            Func::Sin => "SIN",
            Func::Cos => "COS",
            Func::Tan => "TAN",
            Func::Atan => "ATAN",
            Func::Min => "MIN",
            Func::Max => "MAX",
            Func::Atan2 => "ATAN2",
        }
    }
}

/// One operation of a program, run on a stack of numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Num(f64),
    /// `$n`: the n-th modifier, 0 when there is none.
    Modifier(u32),
    /// `?name`: the value of the equation at this index.
    Equation(u32),
    Var(Var),
    Neg,
    Add,
    Sub,
    Mul,
    Div,
    Call(Func),
    /// `if`'s test is on the stack: take it, and go to this operation when it is not positive.
    ElseWhenNotPositive(u32),
    /// Go to this operation.
    Jump(u32),
}

/// A formula read: the operations, the equations it names, and its translation.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Compiled {
    ops: Vec<Op>,
    /// The equations it names, in the order it names them (a name more than once is more than once).
    pub deps: Vec<u32>,
    /// The modifiers it names (`$n`), in the order it names them.
    pub mods: Vec<u32>,
    /// The ShapeSheet text of the formula, empty when the formula is in error.
    pub text: String,
    /// The error a formula that cannot be read is: `#NAME?`.
    pub error: Option<&'static str>,
    /// Whether the formula is one number written out (`5400`, `-2700`, `0.5`): such a parameter is a plain number in a cell, not a formula.
    pub plain: Option<f64>,
}

/// The value of an expression: a number, or the error it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Eval {
    pub value: f64,
    pub error: Option<&'static str>,
}

impl Eval {
    const ZERO: Eval = Eval { value: 0.0, error: None };

    fn number(value: f64) -> Eval {
        if value.is_finite() { Eval { value, error: None } } else { Eval { value: 0.0, error: Some("#NUM!") } }
    }

    fn failed(error: &'static str) -> Eval {
        Eval { value: 0.0, error: Some(error) }
    }
}

/// The work an evaluation did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Work {
    /// Equations evaluated: each once, whatever number of equations and parameters read it.
    pub equations: u64,
    /// Operations run, over all the equations and parameters evaluated.
    pub ops: u64,
    /// Steps of the walk over the equations' dependencies: each dependency looked at, once, and each equation put in a cycle, once. The
    /// walk is linear in the equations and their dependencies whatever shape they make.
    pub steps: u64,
    /// Bytes of expression text compiled: each equation's formula and each parameter's, once.
    pub compiled: u64,
}

/// The names of a shape's equations, to the index of the first equation of each name.
pub(crate) type ByName<'a> = HashMap<&'a str, u32>;

fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// What stands open while a formula is read: a bracket, or a call and what it has had.
#[derive(Clone, Copy, Debug)]
enum Frame {
    /// A parenthesis, and where the operators on the stack below it end.
    Bracket,
    /// A call of a function of `arity` arguments, `seen` of them finished.
    Call { func: Func, arity: usize, seen: usize },
    /// `if`: the arguments finished, and the operation to patch when the then-branch ends and the else-branch starts.
    If { seen: usize, to_else: usize, to_end: usize },
}

/// An operator on the stack the formula is read with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Neg,
    Add,
    Sub,
    Mul,
    Div,
    /// The mark of an open frame: operators above it belong to the argument being read.
    Mark,
}

impl Pending {
    fn power(self) -> u8 {
        match self {
            Pending::Mark => 0,
            Pending::Add | Pending::Sub => 1,
            Pending::Mul | Pending::Div => 2,
            Pending::Neg => 3,
        }
    }

    fn op(self) -> Option<Op> {
        Some(match self {
            Pending::Neg => Op::Neg,
            Pending::Add => Op::Add,
            Pending::Sub => Op::Sub,
            Pending::Mul => Op::Mul,
            Pending::Div => Op::Div,
            Pending::Mark => return None,
        })
    }
}

/// A formula that cannot be read: the program and the text are left empty.
fn broken() -> Compiled {
    Compiled { error: Some("#NAME?"), ..Compiled::default() }
}

/// Reads `source` and translates it in one pass over its bytes, with a stack of its own: `names` gives each `?name` its
/// equation's index and `env` the numbers `left` to `bottom`, `width`, `height` and the stretch points stand for. A formula that cannot be
/// read is [`Compiled::error`].
pub(crate) fn compile(text: &str, names: &ByName<'_>, env: &Env<'_>) -> Compiled {
    let bytes = text.as_bytes();
    let mut out = Compiled::default();
    let mut shown = String::with_capacity(text.len() + text.len() / 4);
    let mut at = 0usize;
    // True where an operand is expected: at the start, after an operator, after `(` and after `,`.
    let mut operand = true;
    let mut stack: Vec<Pending> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    // The last thing written to `shown` was a `/`, so a compound identifier that follows is put in brackets.
    let mut after_slash = false;
    let flush = |stack: &mut Vec<Pending>, ops: &mut Vec<Op>, down_to: u8| {
        while let Some(top) = stack.last().copied() {
            if top.power() < down_to || top == Pending::Mark {
                break;
            }
            stack.pop();
            if let Some(op) = top.op() {
                ops.push(op);
            }
        }
    };
    while let Some(&b) = bytes.get(at) {
        if b.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if operand {
            match b {
                b'-' | b'+' => {
                    at += 1;
                    if b == b'-' {
                        stack.push(Pending::Neg);
                        shown.push('-');
                    }
                }
                b'(' => {
                    at += 1;
                    if frames.len() >= MOST_NESTING {
                        return broken();
                    }
                    frames.push(Frame::Bracket);
                    stack.push(Pending::Mark);
                    shown.push('(');
                    after_slash = false;
                }
                b'$' => {
                    let digits = bytes.get(at + 1..).map_or(0, |r| r.iter().take_while(|d| d.is_ascii_digit()).count());
                    let Some(n) = text.get(at + 1..at + 1 + digits).and_then(|d| d.parse::<u32>().ok()) else { return broken() };
                    at += 1 + digits;
                    out.ops.push(Op::Modifier(n));
                    out.mods.push(n);
                    shown.push_str("User.ODFMod");
                    shown.push_str(&n.to_string());
                    operand = false;
                    after_slash = false;
                }
                b'?' => {
                    let len = bytes.get(at + 1..).map_or(0, |r| r.iter().take_while(|c| is_word(**c)).count());
                    let Some(name) = text.get(at + 1..at + 1 + len).filter(|n| !n.is_empty()) else { return broken() };
                    let Some(&index) = names.get(name) else { return broken() };
                    at += 1 + len;
                    out.ops.push(Op::Equation(index));
                    out.deps.push(index);
                    shown.push_str("User.ODFEq");
                    shown.push_str(&index.to_string());
                    operand = false;
                    after_slash = false;
                }
                b'0'..=b'9' | b'.' => {
                    let end = number_end(bytes, at);
                    let Some(literal) = text.get(at..end) else { return broken() };
                    let Some(n) = literal.parse::<f64>().ok().filter(|n| n.is_finite()) else { return broken() };
                    out.ops.push(Op::Num(n));
                    // The digits as written, when they are plain; any other form (`.5`, `1e2`) as the number's own decimal.
                    if literal.bytes().all(|d| d.is_ascii_digit() || d == b'.') && !literal.starts_with('.') && !literal.ends_with('.') {
                        shown.push_str(literal);
                    } else {
                        shown.push_str(&n.to_string());
                    }
                    at = end;
                    operand = false;
                    after_slash = false;
                }
                c if is_word_start(c) => {
                    let len = bytes.get(at..).map_or(0, |r| r.iter().take_while(|c| is_word(**c)).count());
                    let Some(word) = text.get(at..at + len) else { return broken() };
                    at += len;
                    let next = bytes.get(at..).and_then(|r| r.iter().find(|c| !c.is_ascii_whitespace())).copied();
                    if next == Some(b'(') {
                        // A call: `(` follows, maybe after white space.
                        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                            at += 1;
                        }
                        at += 1;
                        if frames.len() >= MOST_NESTING {
                            return broken();
                        }
                        if word == "if" {
                            frames.push(Frame::If { seen: 0, to_else: usize::MAX, to_end: usize::MAX });
                            shown.push_str("IF((");
                        } else {
                            let Some((func, arity)) = Func::of(word) else { return broken() };
                            frames.push(Frame::Call { func, arity, seen: 0 });
                            shown.push_str(func.shown());
                            shown.push('(');
                        }
                        stack.push(Pending::Mark);
                        after_slash = false;
                        continue;
                    }
                    let Some((op, translation, compound)) = variable(word, env) else { return broken() };
                    out.ops.push(op);
                    if compound && after_slash {
                        shown.push('(');
                        shown.push_str(&translation);
                        shown.push(')');
                    } else {
                        shown.push_str(&translation);
                    }
                    operand = false;
                    after_slash = false;
                }
                _ => return broken(),
            }
            continue;
        }
        // After an operand: an operator, a comma, or a closing bracket.
        match b {
            b'+' | b'-' | b'*' | b'/' => {
                at += 1;
                let incoming = match b {
                    b'+' => Pending::Add,
                    b'-' => Pending::Sub,
                    b'*' => Pending::Mul,
                    _ => Pending::Div,
                };
                // Left to right: an operator of the same or higher power on the stack is done first.
                flush(&mut stack, &mut out.ops, incoming.power());
                stack.push(incoming);
                shown.push(b as char);
                after_slash = b == b'/';
                operand = true;
            }
            b',' => {
                at += 1;
                flush(&mut stack, &mut out.ops, 1);
                match frames.last_mut() {
                    Some(Frame::Call { arity, seen, .. }) => {
                        *seen += 1;
                        if *seen >= *arity {
                            return broken();
                        }
                    }
                    Some(Frame::If { seen, to_else, to_end }) => {
                        *seen += 1;
                        match *seen {
                            1 => {
                                *to_else = out.ops.len();
                                out.ops.push(Op::ElseWhenNotPositive(0));
                                shown.push_str(")>0");
                            }
                            2 => {
                                *to_end = out.ops.len();
                                out.ops.push(Op::Jump(0));
                                // The else-branch starts here.
                                let here = u32::try_from(out.ops.len()).unwrap_or(u32::MAX);
                                if let Some(op) = out.ops.get_mut(*to_else) {
                                    *op = Op::ElseWhenNotPositive(here);
                                }
                            }
                            _ => return broken(),
                        }
                    }
                    _ => return broken(),
                }
                shown.push(',');
                after_slash = false;
                operand = true;
            }
            b')' => {
                at += 1;
                flush(&mut stack, &mut out.ops, 1);
                // The mark of the frame being closed.
                if stack.pop() != Some(Pending::Mark) {
                    return broken();
                }
                match frames.pop() {
                    Some(Frame::Bracket) => {}
                    Some(Frame::Call { func, arity, seen }) => {
                        if seen + 1 != arity {
                            return broken();
                        }
                        out.ops.push(Op::Call(func));
                    }
                    Some(Frame::If { seen, to_end, .. }) => {
                        if seen != 2 {
                            return broken();
                        }
                        let here = u32::try_from(out.ops.len()).unwrap_or(u32::MAX);
                        if let Some(op) = out.ops.get_mut(to_end) {
                            *op = Op::Jump(here);
                        }
                    }
                    None => return broken(),
                }
                shown.push(')');
                after_slash = false;
            }
            _ => return broken(),
        }
    }
    if operand || !frames.is_empty() {
        return broken();
    }
    flush(&mut stack, &mut out.ops, 1);
    if !stack.is_empty() {
        return broken();
    }
    // The translation nests more than the source (see `MOST_NESTING`): a formula the formula crate would not read back is not written.
    if nesting(&shown) > MOST_NESTING {
        return broken();
    }
    out.plain = text.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    out.text = shown;
    out
}

/// Where the number that starts at `at` ends: digits, a fraction, and an exponent with at least one digit.
fn number_end(bytes: &[u8], at: usize) -> usize {
    let digits = |from: usize| bytes.get(from..).map_or(0, |r| r.iter().take_while(|d| d.is_ascii_digit()).count());
    let mut i = at + digits(at);
    if bytes.get(i) == Some(&b'.') {
        i += 1 + digits(i + 1);
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(i + 1), Some(b'+' | b'-')));
        let exponent = digits(i + 1 + sign);
        if exponent > 0 {
            i += 1 + sign + exponent;
        }
    }
    i
}

/// An identifier that stands for a number: its operation, its translation (the numbers of the view box and the stretch points written
/// out), and whether the translation is more than one term (a product), which a divisor is put in brackets for.
fn variable(word: &str, env: &Env<'_>) -> Option<(Op, String, bool)> {
    let vb = &env.vb;
    let n = |v: f64| super::cells::number(v).text().to_owned();
    let (var, text, compound) = match word {
        "pi" => (Var::Pi, "PI()".to_owned(), false),
        "left" => (Var::Left, n(vb.left), false),
        "top" => (Var::Top, n(vb.top), false),
        "right" => (Var::Right, n(vb.left + vb.width), false),
        "bottom" => (Var::Bottom, n(vb.top + vb.height), false),
        "width" => (Var::Width, n(vb.width), false),
        "height" => (Var::Height, n(vb.height), false),
        "logwidth" => (Var::LogWidth, "Width/(1 in)*2540".to_owned(), true),
        "logheight" => (Var::LogHeight, "Height/(1 in)*2540".to_owned(), true),
        "xstretch" => (Var::XStretch, n(env.stretch.0), false),
        "ystretch" => (Var::YStretch, n(env.stretch.1), false),
        "hasfill" => (Var::HasFill, "IF(FillPattern=0,0,1)".to_owned(), false),
        "hasstroke" => (Var::HasStroke, "IF(LinePattern=0,0,1)".to_owned(), false),
        _ => return None,
    };
    Some((Op::Var(var), text, compound))
}

impl Compiled {
    /// The bytes of memory this formula holds while it is kept: its operations, the names it reads, and its translation. The caller charges
    /// them before it reads the next formula, so that the formulas of a shape that are held together are charged together.
    pub(crate) fn bytes(&self) -> usize {
        use std::mem::size_of;
        size_of::<Compiled>()
            .saturating_add(self.ops.capacity().saturating_mul(size_of::<Op>()))
            .saturating_add(self.deps.capacity().saturating_add(self.mods.capacity()).saturating_mul(size_of::<u32>()))
            .saturating_add(self.text.capacity())
    }

    /// Runs the program against the values of the equations it names (`values`, by index) and `env`. Each operation run is counted in
    /// `work`. An equation it names that has an error is the error.
    pub(crate) fn run(&self, env: &Env<'_>, values: &[Eval], work: &mut Work) -> Eval {
        if let Some(error) = self.error {
            return Eval::failed(error);
        }
        let mut stack: Vec<f64> = Vec::with_capacity(8);
        let mut at = 0usize;
        // `if` skips the branch not taken, so an operation of it is not run; a jump only goes forward, so every operation runs at most once.
        while let Some(op) = self.ops.get(at).copied() {
            work.ops = work.ops.saturating_add(1);
            at += 1;
            let value = match op {
                Op::Num(n) => n,
                Op::Modifier(n) => env.mods.get(n as usize).copied().unwrap_or(0.0),
                Op::Equation(i) => match values.get(i as usize) {
                    Some(Eval { error: Some(error), .. }) => return Eval::failed(error),
                    Some(Eval { value, .. }) => *value,
                    None => return Eval::failed("#REF!"),
                },
                Op::Var(var) => match var {
                    Var::Left => env.vb.left,
                    Var::Top => env.vb.top,
                    Var::Right => env.vb.left + env.vb.width,
                    Var::Bottom => env.vb.top + env.vb.height,
                    Var::Width => env.vb.width,
                    Var::Height => env.vb.height,
                    Var::LogWidth => env.log.0,
                    Var::LogHeight => env.log.1,
                    Var::XStretch => env.stretch.0,
                    Var::YStretch => env.stretch.1,
                    Var::HasFill => f64::from(u8::from(env.has_fill)),
                    Var::HasStroke => f64::from(u8::from(env.has_stroke)),
                    Var::Pi => std::f64::consts::PI,
                },
                Op::Neg => {
                    let Some(a) = stack.pop() else { return Eval::failed("#NAME?") };
                    -a
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div => {
                    let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else { return Eval::failed("#NAME?") };
                    match op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        _ => {
                            if b == 0.0 {
                                return Eval::failed("#DIV/0!");
                            }
                            a / b
                        }
                    }
                }
                Op::Call(func) => match func {
                    Func::Abs | Func::Sqrt | Func::Sin | Func::Cos | Func::Tan | Func::Atan => {
                        let Some(a) = stack.pop() else { return Eval::failed("#NAME?") };
                        match func {
                            Func::Abs => a.abs(),
                            Func::Sqrt => {
                                if a < 0.0 {
                                    return Eval::failed("#NUM!");
                                }
                                a.sqrt()
                            }
                            Func::Sin => a.sin(),
                            Func::Cos => a.cos(),
                            Func::Tan => a.tan(),
                            _ => a.atan(),
                        }
                    }
                    Func::Min | Func::Max | Func::Atan2 => {
                        let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else { return Eval::failed("#NAME?") };
                        match func {
                            Func::Min => a.min(b),
                            Func::Max => a.max(b),
                            _ => a.atan2(b),
                        }
                    }
                },
                Op::ElseWhenNotPositive(to) => {
                    let Some(test) = stack.pop() else { return Eval::failed("#NAME?") };
                    if test <= 0.0 {
                        at = to as usize;
                    }
                    continue;
                }
                Op::Jump(to) => {
                    at = to as usize;
                    continue;
                }
            };
            if !value.is_finite() {
                return Eval::failed("#NUM!");
            }
            stack.push(value);
        }
        match (stack.pop(), stack.is_empty()) {
            (Some(value), true) => Eval::number(value),
            _ => Eval::failed("#NAME?"),
        }
    }
}

/// The values of every equation, in `compiled` order, each worked out once in dependency order. This is Tarjan's walk for strongly connected
/// components, made with stacks of its own: it finds the components in the order a component's dependencies come before it, so each equation
/// is run when every equation it names has its value, and an equation that is on a cycle (a component of more than one equation, or one that
/// names itself) is `#REF!` and is not run. An equation that reads one has the error too. Each equation and each dependency is looked at once,
/// whatever the shape the dependencies make: `work` counts them ([`Work::steps`]), each equation once ([`Work::equations`]) and each operation
/// run.
pub(crate) fn evaluate_all(compiled: &[Compiled], env: &Env<'_>, work: &mut Work) -> Vec<Eval> {
    const NONE: usize = usize::MAX;
    let n = compiled.len();
    let mut values = vec![Eval::ZERO; n];
    // The order each equation was first reached in, the lowest order reachable from it, and whether it is waiting for its component.
    let mut order = vec![NONE; n];
    let mut low = vec![0usize; n];
    let mut waiting = vec![false; n];
    let mut component: Vec<usize> = Vec::new();
    // The equations on the walk's way down, each with the next of its dependencies to look at.
    let mut walk: Vec<(usize, usize)> = Vec::new();
    let mut next = 0usize;
    for start in 0..n {
        if order.get(start) != Some(&NONE) {
            continue;
        }
        reach(start, &mut next, &mut order, &mut low, &mut waiting, &mut component, &mut walk);
        while let Some(&(at, cursor)) = walk.last() {
            let deps = compiled.get(at).map_or(&[][..], |c| c.deps.as_slice());
            if let Some(&dep) = deps.get(cursor) {
                if let Some(top) = walk.last_mut() {
                    top.1 += 1;
                }
                work.steps = work.steps.saturating_add(1);
                let dep = dep as usize;
                match order.get(dep).copied() {
                    Some(NONE) => reach(dep, &mut next, &mut order, &mut low, &mut waiting, &mut component, &mut walk),
                    Some(reached) if waiting.get(dep) == Some(&true) => {
                        if let Some(l) = low.get_mut(at) {
                            *l = (*l).min(reached);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            walk.pop();
            let (mine, first) = (low.get(at).copied().unwrap_or(0), order.get(at).copied().unwrap_or(0));
            if let Some(&(parent, _)) = walk.last()
                && let Some(l) = low.get_mut(parent)
            {
                *l = (*l).min(mine);
            }
            if mine != first {
                continue;
            }
            // `at` is the root of a component: its members are on the component stack down to it.
            let mut members: Vec<usize> = Vec::new();
            while let Some(member) = component.pop() {
                if let Some(w) = waiting.get_mut(member) {
                    *w = false;
                }
                members.push(member);
                if member == at {
                    break;
                }
            }
            let cyclic = members.len() > 1 || deps.iter().any(|d| *d as usize == at);
            work.steps = work.steps.saturating_add(deps.len() as u64);
            for member in members {
                work.equations = work.equations.saturating_add(1);
                work.steps = work.steps.saturating_add(1);
                let value = match compiled.get(member) {
                    Some(_) if cyclic => Eval::failed("#REF!"),
                    Some(c) => c.run(env, &values, work),
                    None => Eval::ZERO,
                };
                if let Some(slot) = values.get_mut(member) {
                    *slot = value;
                }
            }
        }
    }
    values
}

/// Tarjan's step on first reaching equation `at`: it gets the next order, goes on the component stack and on the walk.
fn reach(
    at: usize,
    next: &mut usize,
    order: &mut [usize],
    low: &mut [usize],
    waiting: &mut [bool],
    component: &mut Vec<usize>,
    walk: &mut Vec<(usize, usize)>,
) {
    if let (Some(o), Some(l), Some(w)) = (order.get_mut(at), low.get_mut(at), waiting.get_mut(at)) {
        *o = *next;
        *l = *next;
        *w = true;
    }
    *next += 1;
    component.push(at);
    walk.push((at, 0));
}

/// The value of a formula that names equations whose values are known (a path parameter, a handle's position).
pub(crate) fn evaluate_one(formula: &Compiled, env: &Env<'_>, values: &[Eval], work: &mut Work) -> Eval {
    formula.run(env, values, work)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env<'static> {
        Env {
            vb: ViewBox { left: 0.0, top: 0.0, width: 21600.0, height: 21600.0 },
            stretch: (0.0, 0.0),
            log: (4000.0, 2000.0),
            has_fill: true,
            has_stroke: true,
            mods: &[3000.0, 5400.0],
        }
    }

    /// The translation and the value of `source`, against equations `named`.
    fn read(source: &str, named: &[(&str, &str)]) -> (String, Eval) {
        let env = env();
        let names: ByName<'_> = named.iter().enumerate().map(|(i, (n, _))| (*n, i as u32)).collect();
        let compiled: Vec<Compiled> = named.iter().map(|(_, f)| compile(f, &names, &env)).collect();
        let mut work = Work::default();
        let values = evaluate_all(&compiled, &env, &mut work);
        let one = compile(source, &names, &env);
        let value = evaluate_one(&one, &env, &values, &mut work);
        (one.text, value)
    }

    #[test]
    fn the_translation_is_term_by_term_and_the_value_follows_the_odf_rules() {
        let (text, v) = read("logwidth*0.5", &[]);
        assert_eq!(text, "Width/(1 in)*2540*0.5", "The translation, with no brackets round the first operand of a product");
        assert_eq!(v, Eval { value: 2000.0, error: None });
        let (text, v) = read("if($0-5000,width,?f0)", &[("f0", "logwidth*0.5")]);
        assert_eq!(text, "IF((User.ODFMod0-5000)>0,21600,User.ODFEq0)");
        assert_eq!(v.value, 2000.0, "at 3000 the test is not positive, so the else-branch is taken");
        let (text, v) = read("2*pi + abs(-3) - sqrt(16)", &[]);
        assert_eq!(text, "2*PI()+ABS(-3)-SQRT(16)");
        assert!((v.value - (2.0 * std::f64::consts::PI + 3.0 - 4.0)).abs() < 1e-12);
        let (text, v) = read("min(1,2)+max(3,4)+atan2(1,1)*0+tan(0)+sin(0)+cos(0)+atan(0)", &[]);
        assert_eq!(text, "MIN(1,2)+MAX(3,4)+ATAN2(1,1)*0+TAN(0)+SIN(0)+COS(0)+ATAN(0)");
        assert_eq!(v.value, 1.0 + 4.0 + 1.0);
        let (text, _) = read("left+top+right+bottom+width+height+xstretch+ystretch", &[]);
        assert_eq!(text, "0+0+21600+21600+21600+21600+0+0");
        let (text, v) = read("hasfill+hasstroke", &[]);
        assert_eq!(text, "IF(FillPattern=0,0,1)+IF(LinePattern=0,0,1)");
        assert_eq!(v.value, 2.0);
    }

    #[test]
    fn a_compound_translation_after_a_slash_is_in_brackets() {
        let (text, v) = read("1000/logwidth", &[]);
        assert_eq!(text, "1000/(Width/(1 in)*2540)");
        assert_eq!(v.value, 0.25);
        let (text, v) = read("logheight/4", &[]);
        assert_eq!(text, "Height/(1 in)*2540/4");
        assert_eq!(v.value, 500.0);
    }

    #[test]
    fn precedence_associativity_and_unary_minus() {
        assert_eq!(read("2+3*4", &[]).1.value, 14.0);
        assert_eq!(read("10-2-3", &[]).1.value, 5.0);
        assert_eq!(read("100/10/5", &[]).1.value, 2.0);
        assert_eq!(read("-2*-3", &[]).1.value, 6.0);
        assert_eq!(read("-(1+2)*2", &[]).1.value, -6.0);
        assert_eq!(read("2*(3+4)", &[]).1.value, 14.0);
        assert_eq!(read("--5", &[]).1.value, 5.0);
        assert_eq!(read("+5", &[]).1.value, 5.0);
        assert_eq!(read("1e2+.5", &[]).1.value, 100.5);
        assert_eq!(read("$1+$0", &[]).1.value, 8400.0);
        assert_eq!(read("$9", &[]).1.value, 0.0, "a modifier past the list reads as 0");
        assert_eq!(read("if(1,2,3)*if(0-1,4,5)", &[]).1.value, 10.0);
        assert_eq!(read("if(if(1,0,1),7,8)", &[]).1.value, 8.0);
        assert_eq!(read("1 + 2", &[]).1.value, 3.0);
    }

    #[test]
    fn a_formula_that_is_one_number_is_plain() {
        let env = env();
        let n = ByName::new();
        assert_eq!(compile("5400", &n, &env).plain, Some(5400.0));
        assert_eq!(compile(" -2700 ", &n, &env).plain, Some(-2700.0));
        assert_eq!(compile("0.5", &n, &env).plain, Some(0.5));
        assert_eq!(compile("$0", &n, &env).plain, None);
        assert_eq!(compile("1+2", &n, &env).plain, None);
    }

    #[test]
    fn formulas_that_break_the_syntax_are_names_errors() {
        for source in [
            "",
            "1+",
            "*2",
            "(1",
            "1)",
            "foo",
            "foo(1)",
            "abs()",
            "abs(1,2)",
            "min(1)",
            "if(1,2)",
            "if(1,2,3,4)",
            "1 2",
            "?nope",
            "$",
            "$x",
            "?",
            "1,2",
            "sqrt(",
            "1+*2",
            "if(1,2,3",
            "pi(",
            "5e",
        ] {
            let (_, v) = read(source, &[]);
            assert_eq!(v.error, Some("#NAME?"), "{source:?}");
        }
    }

    #[test]
    fn arithmetic_errors_are_the_errors_a_cell_gives() {
        assert_eq!(read("1/0", &[]).1.error, Some("#DIV/0!"));
        assert_eq!(read("1/(2-2)", &[]).1.error, Some("#DIV/0!"));
        assert_eq!(read("sqrt(0-1)", &[]).1.error, Some("#NUM!"));
        assert_eq!(read("1e308*10", &[]).1.error, Some("#NUM!"));
        // The branch not taken is not run.
        assert_eq!(read("if(1,5,1/0)", &[]).1, Eval { value: 5.0, error: None });
        // An error reaches what reads it.
        let (_, v) = read("?f0+1", &[("f0", "1/0")]);
        assert_eq!(v.error, Some("#DIV/0!"));
    }

    #[test]
    fn a_cycle_is_ref_on_each_member_and_reaches_what_reads_it_and_nothing_else() {
        let env = env();
        let named = [("a", "?b+1"), ("b", "?c+1"), ("c", "?a+1"), ("d", "?c+1"), ("e", "7")];
        let names: ByName<'_> = named.iter().enumerate().map(|(i, (n, _))| (*n, i as u32)).collect();
        let compiled: Vec<Compiled> = named.iter().map(|(_, f)| compile(f, &names, &env)).collect();
        let mut work = Work::default();
        let values = evaluate_all(&compiled, &env, &mut work);
        assert!(values.iter().take(4).all(|v| v.error == Some("#REF!")), "{values:?}");
        assert_eq!(values.get(4), Some(&Eval { value: 7.0, error: None }));
        assert_eq!(work.equations, 5, "each equation is visited once, the cycle's too");
        // Equal to itself, directly.
        let named = [("a", "?a")];
        let names: ByName<'_> = [("a", 0u32)].into_iter().collect();
        let compiled = vec![compile("?a", &names, &env)];
        let values = evaluate_all(&compiled, &env, &mut Work::default());
        assert_eq!(values.first().and_then(|v| v.error), Some("#REF!"));
        let _ = named;
    }

    #[test]
    fn a_chain_is_visited_once_each_with_no_recursion() {
        let env = env();
        let n = MOST_EQUATIONS;
        let formulas: Vec<String> = (0..n).map(|i| if i == 0 { "1".to_owned() } else { format!("?f{}+1", i - 1) }).collect();
        // Each names the one before; the first is the last in the list, so the walk goes down the whole chain.
        let names: ByName<'_> = (0..n).map(|i| (Box::leak(format!("f{i}").into_boxed_str()) as &str, (n - 1 - i) as u32)).collect();
        let mut reversed: Vec<Compiled> = Vec::new();
        for i in (0..n).rev() {
            reversed.push(compile(formulas.get(i).map_or("", String::as_str), &names, &env));
        }
        let mut work = Work::default();
        let values = evaluate_all(&reversed, &env, &mut work);
        assert_eq!(work.equations, n as u64);
        assert_eq!(values.first().map(|v| v.value), Some(n as f64), "the last of the chain is first in the list");
        assert!(work.ops < 4 * n as u64, "{}", work.ops);
    }

    #[test]
    fn a_diamond_is_linear_and_not_two_to_the_n() {
        let env = env();
        let n = 5_000usize;
        // Equation i is `?f(i-1)+?f(i-2)`: a naive evaluation takes 2^n steps.
        let formulas: Vec<String> = (0..n).map(|i| if i < 2 { "1".to_owned() } else { format!("(?f{}+?f{})*0.5", i - 1, i - 2) }).collect();
        let names: ByName<'_> = (0..n).map(|i| (Box::leak(format!("f{i}").into_boxed_str()) as &str, i as u32)).collect();
        let compiled: Vec<Compiled> = formulas.iter().map(|f| compile(f, &names, &env)).collect();
        let mut work = Work::default();
        let values = evaluate_all(&compiled, &env, &mut work);
        assert_eq!(work.equations, n as u64);
        assert!(work.ops <= 8 * n as u64, "{}", work.ops);
        assert!(values.iter().all(|v| v.error.is_none() && (v.value - 1.0).abs() < 1e-9));
    }

    #[test]
    fn deep_nesting_is_refused_and_a_long_flat_formula_is_read() {
        let env = env();
        let n = ByName::new();
        let deep = format!("{}1{}", "(".repeat(MOST_NESTING + 1), ")".repeat(MOST_NESTING + 1));
        assert_eq!(compile(&deep, &n, &env).error, Some("#NAME?"));
        let ok = format!("{}1{}", "(".repeat(MOST_NESTING), ")".repeat(MOST_NESTING));
        assert_eq!(compile(&ok, &n, &env).error, None);
        let million = "1+".repeat(500_000) + "1";
        let c = compile(&million, &n, &env);
        assert_eq!(c.error, None);
        assert_eq!(evaluate_one(&c, &env, &[], &mut Work::default()).value, 500_001.0);
        // A million opening brackets is a refusal at the hundred-and-first, not a stack of a million.
        let opens = "(".repeat(1_000_000);
        assert_eq!(compile(&opens, &n, &env).error, Some("#NAME?"));
    }

    /// Whether `text`, in `extra` brackets, parses: the formula crate's own count of what it nests.
    fn parses_in_brackets(text: &str, extra: usize) -> bool {
        diacraft_formula::parse(&format!("{}{text}{}", "(".repeat(extra), ")".repeat(extra))).is_parsed()
    }

    /// `nesting` counts what the formula crate counts: a text of nesting `n` parses in `MOST_NESTING - n` more brackets and not in one more.
    #[test]
    fn nesting_is_what_the_formula_crate_counts() {
        let samples = [
            ("1", 0),
            ("(1)", 1),
            ("((1))", 2),
            ("SIN(1)", 1),
            ("PI()", 1),
            ("IF((1)>0,2,3)", 2),
            ("IF((IF((1)>0,2,3))>0,4,5)", 4),
            ("IF(1>0,IF(1>0,IF(1>0,1,2),3),4)", 3),
            ("-1", 1),
            ("--1", 2),
            ("1+-1", 1),
            ("1--1", 1),
            ("-1+-1", 1),
            ("-1--1", 1),
            ("-1*-1+-1", 1),
            ("1-1", 0),
            ("( -1)", 2),
            ("1 - -1", 1),
            ("SIN( -PI())", 3),
            ("-(-1)", 3),
            ("-SIN(-PI())", 4),
            ("-(1)+2", 2),
            ("2*(1+(2))-3", 2),
            ("1000/(Width/(1 in)*2540)", 2),
            ("MIN(1,MAX(2,-3))", 3),
        ];
        for (text, want) in samples {
            assert_eq!(nesting(text), want, "{text}");
            // Against the parser itself.
            assert!(parses_in_brackets(text, MOST_NESTING - want), "{text} parses in {} brackets", MOST_NESTING - want);
            assert!(!parses_in_brackets(text, MOST_NESTING - want + 1), "{text} does not parse in one more");
        }
        // The count stops past the limit, with a stack no longer than the limit, whatever the text.
        let opens = "(".repeat(1_000_000);
        assert_eq!(nesting(&opens), MOST_NESTING + 1);
        let signs = "-".repeat(1_000_000);
        assert_eq!(nesting(&signs), MOST_NESTING + 1);
    }

    /// The translation of a formula nests more than the formula: a formula whose translation would not parse back is not read.
    #[test]
    fn a_formula_whose_translation_nests_past_the_limit_is_not_read_and_one_within_it_parses_back() {
        let env = env();
        let n = ByName::new();
        let read = |text: &str| compile(text, &n, &env);
        let ifs = |count: usize| (0..count).fold("1".to_owned(), |inner, _| format!("if({inner},1,1)"));
        // Each `if` in a condition is `IF((`: two levels for one of the source's.
        for count in [1usize, 25, 50] {
            let c = read(&ifs(count));
            assert_eq!(c.error, None, "{count} ifs");
            assert_eq!(nesting(&c.text), 2 * count);
            assert!(diacraft_formula::parse(&c.text).is_parsed(), "{count} ifs parse back");
        }
        assert_eq!(read(&ifs(51)).error, Some("#NAME?"));
        assert_eq!(read(&ifs(99)).error, Some("#NAME?"), "99 ifs nest 99 in the source, and 198 in the translation");
        // In an else-branch they nest one each.
        let tail = |count: usize| (0..count).fold("1".to_owned(), |inner, _| format!("if(1,1,{inner})"));
        assert_eq!(read(&tail(99)).error, None);
        assert!(diacraft_formula::parse(&read(&tail(99)).text).is_parsed());
        // `logwidth` after a slash is a bracket with a bracket in it.
        let slash = |brackets: usize| format!("{}1/logwidth{}", "(".repeat(brackets), ")".repeat(brackets));
        assert_eq!(read(&slash(98)).error, None);
        assert!(diacraft_formula::parse(&read(&slash(98)).text).is_parsed());
        assert_eq!(read(&slash(99)).error, Some("#NAME?"));
        // A sign nests until its operand ends: the source does not count it.
        assert_eq!(read(&format!("{}1", "-".repeat(100))).error, None);
        assert_eq!(read(&format!("{}1", "-".repeat(101))).error, Some("#NAME?"));
        assert_eq!(read(&format!("{}(1)", "-".repeat(99))).error, None);
        assert_eq!(read(&format!("{}(1)", "-".repeat(100))).error, Some("#NAME?"));
    }

    /// What a compiled formula holds grows with the operations and the text, which is what the caller charges.
    #[test]
    fn a_compiled_formula_holds_its_operations_names_and_translation() {
        let env = env();
        let n = ByName::new();
        let short = compile("1+1", &n, &env);
        let long = compile(&("1+".repeat(1_000) + "1"), &n, &env);
        let own = std::mem::size_of::<Compiled>();
        assert!(short.bytes() >= own + 3 * std::mem::size_of::<Op>() + short.text.len(), "{}", short.bytes());
        assert!(long.bytes() >= own + 2_001 * std::mem::size_of::<Op>() + long.text.len(), "{}", long.bytes());
        // The sum of its parts, as the allocator was asked for them.
        for c in [&short, &long, &compile("$1+?a*logwidth", &[("a", 0)].into_iter().collect(), &env)] {
            let parts = own + c.ops.capacity() * std::mem::size_of::<Op>() + (c.deps.capacity() + c.mods.capacity()) * 4 + c.text.capacity();
            assert_eq!(c.bytes(), parts);
        }
        // A formula in error holds nothing but itself.
        assert_eq!(compile("(", &n, &env).bytes(), own);
    }

    /// Many back-edges into the first equation of a long chain, and a few cycles that overlap it: the walk's steps are linear in the
    /// equations and the dependencies they name, and every equation of the one big cycle is `#REF!` (the review of this commit: marking the
    /// stack from the target of each back-edge down was quadratic).
    #[test]
    fn many_back_edges_into_a_long_chain_cost_steps_linear_in_the_dependencies() {
        let env = env();
        let n = MOST_EQUATIONS;
        // f0 reads f1; f(i) reads f(i+1) and f0, so each entry of the chain has a back-edge to the first; the last reads f0 alone. Three more
        // equations form cycles of their own that overlap nothing but read the chain.
        let mut formulas: Vec<String> = (0..n)
            .map(|i| {
                if i == 0 {
                    "?f1".to_owned()
                } else if i + 1 == n {
                    "?f0".to_owned()
                } else {
                    format!("?f{}+?f0", i + 1)
                }
            })
            .collect();
        formulas.extend(["?x1+1".to_owned(), "?x2+?x0".to_owned(), "?x0+?f5".to_owned()]);
        let names: ByName<'_> = (0..n)
            .map(|i| (Box::leak(format!("f{i}").into_boxed_str()) as &str, i as u32))
            .chain((0..3).map(|k| (Box::leak(format!("x{k}").into_boxed_str()) as &str, (n + k) as u32)))
            .collect();
        let compiled: Vec<Compiled> = formulas.iter().map(|f| compile(f, &names, &env)).collect();
        let edges: u64 = compiled.iter().map(|c| c.deps.len() as u64).sum();
        let started = std::time::Instant::now();
        let mut work = Work::default();
        let values = evaluate_all(&compiled, &env, &mut work);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(values.iter().all(|v| v.error == Some("#REF!")), "the chain is one cycle and the others read it or are on cycles");
        assert_eq!(work.equations, (n + 3) as u64);
        assert!(work.steps <= 4 * (compiled.len() as u64 + edges), "{} steps for {} equations and {edges} dependencies", work.steps, compiled.len());
    }

    #[test]
    fn numbers_are_written_as_plain_decimals_and_read_with_their_exponents() {
        for (source, shown, value) in [
            ("5", "5", 5.0),
            ("2.50", "2.50", 2.5),
            (".5", "0.5", 0.5),
            ("5.", "5", 5.0),
            ("007", "007", 7.0),
            ("1e2", "100", 100.0),
            ("1E2", "100", 100.0),
            ("2e+2", "200", 200.0),
            ("2E-1", "0.2", 0.2),
            ("2.5e1", "25", 25.0),
            ("1e-3", "0.001", 0.001),
        ] {
            let (text, v) = read(source, &[]);
            assert_eq!(text, shown, "{source}");
            assert_eq!(v, Eval { value, error: None }, "{source}");
        }
        // A number that runs into a word is two tokens and a syntax error.
        assert_eq!(read("5e", &[]).1.error, Some("#NAME?"));
        assert_eq!(read("1e+", &[]).1.error, Some("#NAME?"));
        assert_eq!(read("1e999", &[]).1.error, Some("#NAME?"));
    }

    #[test]
    fn names_run_to_the_end_of_their_word_and_modifier_numbers_to_the_end_of_their_digits() {
        let (text, v) = read("?my_eq.1+?a2+$10+$2", &[("a2", "1"), ("my_eq.1", "2")]);
        assert_eq!(text, "User.ODFEq1+User.ODFEq0+User.ODFMod10+User.ODFMod2");
        assert_eq!(v.value, 3.0, "the modifiers past the list are 0");
        assert_eq!(read("$4294967295", &[]).1.value, 0.0);
        assert_eq!(read("$4294967296", &[]).1.error, Some("#NAME?"));
        // A name that is a prefix of another is not that other.
        let (_, v) = read("?a+?ab", &[("a", "1"), ("ab", "10")]);
        assert_eq!(v.value, 11.0);
    }

    #[test]
    fn every_function_gives_its_own_value() {
        for (source, want) in [
            ("sin(1)", 1f64.sin()),
            ("cos(1)", 1f64.cos()),
            ("tan(1)", 1f64.tan()),
            ("atan(1)", 1f64.atan()),
            ("atan2(1,2)", 1f64.atan2(2.0)),
            ("abs(0-2)", 2.0),
            ("sqrt(2)", 2f64.sqrt()),
            ("sqrt(0)", 0.0),
            ("min(3,2)", 2.0),
            ("max(3,2)", 3.0),
            ("min(2,3)", 2.0),
            ("max(2,3)", 3.0),
        ] {
            let (_, v) = read(source, &[]);
            assert_eq!(v, Eval { value: want, error: None }, "{source}");
        }
        assert_eq!(read("sqrt(0-0.5)", &[]).1.error, Some("#NUM!"));
    }

    #[test]
    fn white_space_may_stand_between_a_function_and_its_bracket_and_round_every_token() {
        let (text, v) = read("  sin (1) +cos  ( 1 )\t* if ( 1 , 2 , 3 ) ", &[]);
        assert_eq!(text, "SIN(1)+COS(1)*IF((1)>0,2,3)");
        assert_eq!(v.value, 1f64.sin() + 1f64.cos() * 2.0);
    }
}
