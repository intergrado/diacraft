//! The parser: hand-written, by precedence climbing over the grammar, with the limits of the crate
//! root (text size, nesting, tree depth, node count) checked as it goes.
//!
//! It works on the text by byte offsets and never indexes it: every read goes through `get`. A parse is one pass, and what
//! looks ahead (a number's unit, a mixed number, a compound literal) looks a bounded distance and puts the position back when
//! it finds nothing, so the work is linear in the text.

mod literal;
mod name;
mod number;

use crate::{BinaryOp, Expr, MAX_DEPTH, MAX_NESTING, MAX_NODES, MAX_SOURCE_BYTES, ParseError, ParseErrorKind, RefSpan, SegmentSpan, UnaryOp};

pub(crate) type Res<T> = Result<T, ParseError>;

/// The longest text of an unquoted index (`[Site-2]`), in bytes. A quoted one may be as long as a name. The printer quotes what is
/// over this, so that its text reads back.
pub(crate) const MAX_INDEX_BYTES: usize = 256;

/// The longest name, in bytes: the scan for its end stops here, and a quoted name (`'Left-Door'`, an index in quotes) may be no longer.
pub(crate) const MAX_NAME_BYTES: usize = 1024;

/// A parsed expression and the depth of the tree it roots (a leaf is 1).
pub(crate) struct Node {
    pub expr: Expr,
    pub depth: usize,
}

/// What [`crate::reference_spans`] has the parser note as it reads: where each reference and each part of a path sits in the source. A parse that notes nothing
/// (every parse but that one) holds none and pays for none of it.
#[derive(Default)]
pub(crate) struct Recorder {
    /// One slot for each name chain begun, in the order they began; the slot is filled when what was read is a reference (a call and a `TRUE` leave it empty).
    refs: Vec<Option<RefSpan>>,
    /// The segments of the path being read.
    segments: Vec<SegmentSpan>,
    /// The segments of the last path read: the path of the reference that has just been read, if it is one.
    last_path: Vec<SegmentSpan>,
}

pub(crate) struct Parser<'a> {
    src: &'a str,
    pos: usize,
    /// Brackets, signs and argument lists open now: the recursion of the parser.
    nesting: usize,
    /// Nodes made so far.
    nodes: usize,
    /// The most nodes there may be: `MAX_NODES`, or less by the caller's budget.
    max_nodes: usize,
    /// Set by [`parse_spans`] only.
    rec: Option<Recorder>,
}

/// Parses `source` as a formula: one leading `=` is allowed. At most `budget` nodes, and never more than `MAX_NODES`.
pub(crate) fn parse(source: &str, budget: usize) -> Result<Expr, ParseError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ParseError { kind: ParseErrorKind::TooLong, offset: MAX_SOURCE_BYTES });
    }
    Parser { src: source, pos: 0, nesting: 0, nodes: 0, max_nodes: budget.min(MAX_NODES), rec: None }.formula()
}

/// Parses `source` as [`parse`] does, with the limits of the crate, and gives where each cell reference sits in it, in the order they begin.
pub(crate) fn parse_spans(source: &str) -> Result<Vec<RefSpan>, ParseError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ParseError { kind: ParseErrorKind::TooLong, offset: MAX_SOURCE_BYTES });
    }
    let mut parser = Parser { src: source, pos: 0, nesting: 0, nodes: 0, max_nodes: MAX_NODES, rec: Some(Recorder::default()) };
    parser.formula()?;
    Ok(parser.rec.map(|rec| rec.refs.into_iter().flatten().collect()).unwrap_or_default())
}

pub(crate) fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

pub(crate) fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// How tightly an operator binds: the rank of R12 turned round, so that the loop of [`Parser::binary`] can compare. `^` is 6 and
/// the equalities are 1.
fn tightness(op: BinaryOp) -> u8 {
    9 - op.rank()
}

/// The word forms of the exact comparisons, and the underscored ones, read in any case.
const WORD_OPERATORS: [(&str, BinaryOp); 12] = [
    ("_LT_", BinaryOp::ExactLt),
    ("_LE_", BinaryOp::ExactLe),
    ("_GT_", BinaryOp::ExactGt),
    ("_GE_", BinaryOp::ExactGe),
    ("_EQ_", BinaryOp::ExactEq),
    ("_NE_", BinaryOp::ExactNe),
    ("LT", BinaryOp::ExactLt),
    ("LE", BinaryOp::ExactLe),
    ("GT", BinaryOp::ExactGt),
    ("GE", BinaryOp::ExactGe),
    ("EQ", BinaryOp::ExactEq),
    ("NE", BinaryOp::ExactNe),
];

impl<'a> Parser<'a> {
    // ---- reading the text

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek2(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// Takes `c` if it is next.
    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += c.len_utf8();
        }
        found
    }

    fn skip_ws(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn fail<T>(&self, kind: ParseErrorKind) -> Res<T> {
        self.fail_at(kind, self.pos)
    }

    fn fail_at<T>(&self, kind: ParseErrorKind, offset: usize) -> Res<T> {
        Err(ParseError { kind, offset })
    }

    // ---- the limits

    /// Counts `n` more nodes.
    fn count(&mut self, n: usize) -> Res<()> {
        self.nodes = self.nodes.saturating_add(n);
        if self.nodes > self.max_nodes { self.fail(ParseErrorKind::TooManyNodes) } else { Ok(()) }
    }

    fn enter(&mut self) -> Res<()> {
        self.nesting += 1;
        if self.nesting > MAX_NESTING { self.fail(ParseErrorKind::TooDeep) } else { Ok(()) }
    }

    fn leave(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }

    /// A node of the tree whose deepest child is `child` deep.
    fn node(&mut self, expr: Expr, child: usize) -> Res<Node> {
        let depth = child + 1;
        if depth > MAX_DEPTH {
            return self.fail(ParseErrorKind::TooDeep);
        }
        self.count(1)?;
        Ok(Node { expr, depth })
    }

    fn leaf(&mut self, expr: Expr) -> Res<Node> {
        self.node(expr, 0)
    }

    // ---- the grammar

    /// `formula = [ "=" ] equal`.
    fn formula(&mut self) -> Res<Expr> {
        self.skip_ws();
        self.eat('=');
        self.skip_ws();
        if self.at_end() {
            return self.fail(ParseErrorKind::Empty);
        }
        let node = self.binary(1)?;
        self.skip_ws();
        if self.at_end() { Ok(node.expr) } else { self.fail(ParseErrorKind::ExpectedOperator) }
    }

    /// The binary operators that bind at least as tightly as `min`, from the left: `a - b - c` is `(a - b) - c`, and
    /// the right operand of an operator is read for the operators tighter than it.
    fn binary(&mut self, min: u8) -> Res<Node> {
        let mut lhs = self.unary()?;
        loop {
            self.skip_ws();
            let Some((op, len)) = self.operator() else { break };
            let tight = tightness(op);
            if tight < min {
                break;
            }
            self.pos += len;
            let rhs = self.binary(tight + 1)?;
            let depth = lhs.depth.max(rhs.depth);
            lhs = self.node(Expr::Binary(op, Box::new(lhs.expr), Box::new(rhs.expr)), depth)?;
        }
        Ok(lhs)
    }

    /// The binary operator at the position and its length in bytes, or `None`.
    fn operator(&self) -> Option<(BinaryOp, usize)> {
        let mut chars = self.rest().chars();
        let first = chars.next()?;
        let second = chars.next();
        Some(match (first, second) {
            ('+', _) => (BinaryOp::Add, 1),
            ('-', _) => (BinaryOp::Sub, 1),
            ('*', _) => (BinaryOp::Mul, 1),
            ('/', _) => (BinaryOp::Div, 1),
            ('^', _) => (BinaryOp::Pow, 1),
            ('&', _) => (BinaryOp::Cat, 1),
            ('<', Some('=')) => (BinaryOp::Le, 2),
            ('<', Some('>')) => (BinaryOp::Ne, 2),
            ('<', _) => (BinaryOp::Lt, 1),
            ('>', Some('=')) => (BinaryOp::Ge, 2),
            ('>', _) => (BinaryOp::Gt, 1),
            ('=', _) => (BinaryOp::Eq, 1),
            (c, _) if is_name_start(c) => return self.word_operator(),
            _ => return None,
        })
    }

    /// `_GT_`, `GT` and the like. The six with underscores end where their own underscores do, whatever follows, so `5_GT_2` is
    /// read (blanks may be left out); the words without them (`GT`, `LE`...) are read when the whole name is one, as `GT2`
    /// and `GTX` are names. At most 4 bytes are looked at.
    fn word_operator(&self) -> Option<(BinaryOp, usize)> {
        let rest = self.rest();
        if let Some(head) = rest.get(..4)
            && let Some((_, op)) = WORD_OPERATORS.iter().find(|(w, _)| w.starts_with('_') && w.eq_ignore_ascii_case(head))
        {
            return Some((*op, 4));
        }
        let mut len = 0;
        for c in rest.chars() {
            if !is_name_char(c) {
                break;
            }
            len += c.len_utf8();
            if len > 4 {
                return None;
            }
        }
        let word = rest.get(..len)?;
        WORD_OPERATORS.iter().find(|(w, _)| w.eq_ignore_ascii_case(word)).map(|(_, op)| (*op, len))
    }

    /// `sign = ( "+" | "-" ) sign | pct`: a sign ranks above `^`, so `-4^2` is `(-4)^2`.
    fn unary(&mut self) -> Res<Node> {
        self.skip_ws();
        let op = match self.peek() {
            Some('+') => UnaryOp::Plus,
            Some('-') => UnaryOp::Minus,
            _ => return self.percent(),
        };
        self.pos += 1;
        self.enter()?;
        let operand = self.unary();
        self.leave();
        let operand = operand?;
        let depth = operand.depth;
        self.node(Expr::Unary(op, Box::new(operand.expr)), depth)
    }

    /// `pct = atom { "%" }`.
    fn percent(&mut self) -> Res<Node> {
        let mut node = self.atom()?;
        loop {
            let before = self.pos;
            self.skip_ws();
            if !self.eat('%') {
                self.pos = before;
                return Ok(node);
            }
            let depth = node.depth;
            node = self.node(Expr::Percent(Box::new(node.expr)), depth)?;
        }
    }

    /// `atom = literal | call | reference | "(" equal ")"`.
    fn atom(&mut self) -> Res<Node> {
        self.skip_ws();
        let Some(c) = self.peek() else { return self.fail(ParseErrorKind::UnexpectedEnd) };
        match c {
            '0'..='9' => self.number(),
            '.' if self.peek2().is_some_and(|d| d.is_ascii_digit()) => self.number(),
            '"' => self.string(),
            '#' => self.hash(),
            // A GUID is read only as the one argument of `USE` (see `arguments`); anywhere else it is no formula.
            '{' => self.fail(ParseErrorKind::BadGuid),
            '(' => self.group(),
            c if c == '\'' || is_name_start(c) => self.reference(),
            _ => self.fail(ParseErrorKind::ExpectedOperand),
        }
    }

    /// `"(" equal ")"`: brackets leave no node.
    fn group(&mut self) -> Res<Node> {
        self.pos += 1;
        self.enter()?;
        let inner = self.binary(1);
        self.leave();
        let inner = inner?;
        self.skip_ws();
        if self.eat(')') { Ok(inner) } else { self.fail(ParseErrorKind::ExpectedCloseParen) }
    }
}
