//! Names: references, calls, and `TRUE` and `FALSE`.
//!
//! `[qualifier!]...[qualifier!]target`, each part a dotted path (`Door.7`, `Prop.Cost.Label`, `Pages[Site-2]`) or, for a name
//! alone before brackets, a call (`LISTSHEETREF(2)`). The last part is the cell or the call, and the parts before it are sheet
//! qualifiers. Nothing is looked up: names are kept as written.

use std::sync::Arc;

use super::{MAX_INDEX_BYTES, Node, Parser, Res, is_name_char, is_name_start};
use crate::{Call, CellRef, Expr, Index, ParseErrorKind, Qualifier, Segment};

/// The most qualifiers before a cell. Real formulas have one or two (`Pages[A]!Sheet.9!EndY`).
const MAX_QUALIFIERS: usize = 16;
/// The longest name, in bytes: the scan for its end stops here.
const MAX_NAME_BYTES: usize = 1024;

/// One part of a chain joined by `!`.
enum Part {
    Path { segments: Vec<Segment>, quoted_first: bool },
    Call { name: Arc<str>, args: Vec<Option<Expr>> },
}

impl Part {
    fn into_qualifier(self) -> Qualifier {
        match self {
            Part::Path { segments, .. } => Qualifier::Path(segments),
            Part::Call { name, args } => Qualifier::Call { name, args },
        }
    }
}

/// `TRUE` or `FALSE`, in any case.
fn boolean(name: &str) -> Option<bool> {
    if name.eq_ignore_ascii_case("TRUE") {
        Some(true)
    } else if name.eq_ignore_ascii_case("FALSE") {
        Some(false)
    } else {
        None
    }
}

impl Parser<'_> {
    /// A reference, a call, or a Boolean word. The position is at a name or a quote.
    pub(super) fn reference(&mut self) -> Res<Node> {
        let mut qualifiers = Vec::new();
        let mut deepest = 0;
        loop {
            let (part, depth) = self.part()?;
            deepest = deepest.max(depth);
            let before = self.pos;
            self.skip_ws();
            if !self.eat('!') {
                self.pos = before;
                return self.finish(qualifiers, part, deepest);
            }
            if qualifiers.len() >= MAX_QUALIFIERS {
                return self.fail(ParseErrorKind::TooDeep);
            }
            qualifiers.push(part.into_qualifier());
        }
    }

    fn finish(&mut self, qualifiers: Vec<Qualifier>, part: Part, deepest: usize) -> Res<Node> {
        let expr = match part {
            Part::Call { name, args } => Expr::Call(Box::new(Call { qualifiers, name, args })),
            Part::Path { segments, quoted_first } => {
                // A word only when it stands alone: not qualified, not quoted, no index, not the start of a path.
                let word = match segments.as_slice() {
                    [only] if qualifiers.is_empty() && !quoted_first && only.index.is_none() => boolean(&only.name),
                    _ => None,
                };
                match word {
                    // The name was counted as a segment before it was known to be a word: a word is one node.
                    Some(value) => {
                        self.nodes = self.nodes.saturating_sub(1);
                        Expr::Bool(value)
                    }
                    None => Expr::Ref(Box::new(CellRef { qualifiers, path: segments })),
                }
            }
        };
        self.node(expr, deepest)
    }

    /// A dotted path, or a name and the argument list after it. The second value is the depth of the arguments.
    fn part(&mut self) -> Res<(Part, usize)> {
        let (segments, quoted_first) = self.path()?;
        if !quoted_first
            && let [only] = segments.as_slice()
            && only.index.is_none()
            && self.paren_follows()
        {
            let name = Arc::clone(&only.name);
            let (args, depth) = self.arguments(name.eq_ignore_ascii_case("USE"))?;
            return Ok((Part::Call { name, args }, depth));
        }
        Ok((Part::Path { segments, quoted_first }, 0))
    }

    /// Whether an opening bracket comes next, after any white space, and if so leaves the position at it.
    fn paren_follows(&mut self) -> bool {
        let before = self.pos;
        self.skip_ws();
        let found = self.peek() == Some('(');
        if !found {
            self.pos = before;
        }
        found
    }

    /// `( [slot {, slot}] )`: an empty slot keeps its place, `F()` has none. The position is at the `(`. `takes_guid` is
    /// true for `USE`, whose one argument may be a GUID in braces.
    fn arguments(&mut self, takes_guid: bool) -> Res<(Vec<Option<Expr>>, usize)> {
        self.pos += 1;
        self.enter()?;
        let read = self.slots(takes_guid);
        self.leave();
        read
    }

    fn slots(&mut self, takes_guid: bool) -> Res<(Vec<Option<Expr>>, usize)> {
        let mut args = Vec::new();
        let mut deepest = 0;
        self.skip_ws();
        if self.eat(')') {
            return Ok((args, deepest));
        }
        if takes_guid && self.peek() == Some('{') {
            return self.guid_argument();
        }
        loop {
            self.skip_ws();
            if matches!(self.peek(), Some(',' | ')')) {
                self.count(1)?;
                args.push(None);
            } else {
                let node = self.binary(1)?;
                deepest = deepest.max(node.depth);
                args.push(Some(node.expr));
            }
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(')') => {
                    self.pos += 1;
                    return Ok((args, deepest));
                }
                _ => return self.fail(ParseErrorKind::ExpectedArgumentSeparator),
            }
        }
    }

    /// The GUID that is the one argument of `USE`: the token, blanks, and the closing bracket, and nothing else in the
    /// list: not a second argument, not an operator after it. The position is at the `{`.
    fn guid_argument(&mut self) -> Res<(Vec<Option<Expr>>, usize)> {
        let node = self.guid()?;
        self.skip_ws();
        if !self.eat(')') {
            return self.fail(ParseErrorKind::BadGuid);
        }
        Ok((vec![Some(node.expr)], node.depth))
    }

    /// `name { "." name }`, each name with an optional `[index]`. White space may not sit inside it. After a `!` it may sit
    /// before the first name.
    fn path(&mut self) -> Res<(Vec<Segment>, bool)> {
        self.skip_ws();
        let quoted_first = self.peek() == Some('\'');
        let mut segments = vec![self.segment(true)?];
        while self.eat('.') {
            segments.push(self.segment(false)?);
        }
        Ok((segments, quoted_first))
    }

    /// One name and the `[index]` after it. The first name starts with a letter or an underscore, or is quoted; the others may
    /// start with a digit too (`Door.7`).
    fn segment(&mut self, first: bool) -> Res<Segment> {
        self.count(1)?;
        let name = match self.peek() {
            Some('\'') => self.quoted_name()?,
            Some(c) if (first && is_name_start(c)) || (!first && is_name_char(c)) => self.identifier()?,
            None if first => return self.fail(ParseErrorKind::UnexpectedEnd),
            _ if first => return self.fail(ParseErrorKind::ExpectedOperand),
            _ => return self.fail(ParseErrorKind::BadName),
        };
        let index = if self.peek() == Some('[') { Some(self.index()?) } else { None };
        Ok(Segment { name, index })
    }

    /// Letters, digits and underscores.
    fn identifier(&mut self) -> Res<Arc<str>> {
        let rest = self.rest();
        let len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
        if len > MAX_NAME_BYTES {
            return self.fail(ParseErrorKind::BadName);
        }
        self.pos += len;
        Ok(Arc::from(rest.get(..len).unwrap_or("")))
    }

    /// A name in single quotes, a quote inside it doubled. Not empty. The position is at the opening quote.
    fn quoted_name(&mut self) -> Res<Arc<str>> {
        let start = self.pos;
        self.pos += 1;
        let mut name = String::new();
        loop {
            let rest = self.rest();
            let Some(end) = rest.find('\'') else { return self.fail_at(ParseErrorKind::BadQuotedName, start) };
            name.push_str(rest.get(..end).unwrap_or(""));
            self.pos += end + 1;
            if self.peek() == Some('\'') {
                name.push('\'');
                self.pos += 1;
            } else {
                break;
            }
            if name.len() > MAX_NAME_BYTES {
                return self.fail_at(ParseErrorKind::BadQuotedName, start);
            }
        }
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return self.fail_at(ParseErrorKind::BadQuotedName, start);
        }
        Ok(Arc::from(name))
    }

    /// `[2]` or `[Site-2]` or `['2']`: a whole number is a row number, anything else a name; text in quotes is always a name.
    /// The position is at the `[`.
    fn index(&mut self) -> Res<Index> {
        self.pos += 1;
        self.skip_ws();
        let index = if self.peek() == Some('\'') {
            let name = self.quoted_name()?;
            self.skip_ws();
            Index::Name(name)
        } else {
            let rest = self.rest();
            let Some(end) = rest.bytes().take(MAX_INDEX_BYTES + 1).position(|b| b == b']') else {
                return self.fail(ParseErrorKind::BadIndex);
            };
            let raw = rest.get(..end).unwrap_or("").trim();
            if raw.is_empty() {
                return self.fail(ParseErrorKind::BadIndex);
            }
            self.pos += end;
            match raw.bytes().all(|b| b.is_ascii_digit()).then(|| raw.parse::<u32>().ok()).flatten() {
                Some(row) => Index::Number(row),
                None => Index::Name(Arc::from(raw)),
            }
        };
        if self.eat(']') { Ok(index) } else { self.fail(ParseErrorKind::BadIndex) }
    }
}
