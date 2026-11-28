//! The references to a section's rows in formula text, found and renumbered (design §6.4).
//!
//! A row of the `Connection` section is named in a formula by its number (`Connections.X5`: the row's `IX` plus one), by no number
//! (`Connections.X`: row 1), by a number in brackets (`Connections.DirX[7]`) or by its name (`Connections.Row_3.X`, the name matched without regard to letter
//! case), on the formula's own sheet or after `Sheet.<id>!`. When a command removes a row, or one moves up to close the gap, every such
//! reference to it has to follow it, and **only the number or the name changes**: [`renumber`] gives the text back character for character everywhere else,
//! which is what `draw/remap.rs` does for `Sheet.<id>!`, and what a program that keeps its formulas must see.
//!
//! **What is read.** The scanner reads text as `remap.rs` does: text in a string (`"..."`) or a quoted name (`'...'`), a doubled quote inside either being a
//! quote, is no reference. A reference is a *chain* of dotted paths joined by `!`: the last is the target, those before it qualifiers. A target is ours only when
//! it has no qualifier or just one, `Sheet.<id>` (with the number of digits an id has); one after another qualifier (`Pages[P]!Sheet.5!Connections.X1`), after a
//! shape's name, `ThePage!`, `TheDoc!` or a call (`LISTSHEETREF()!`) names a shape or a page that is not the one asked for and is left. The section's spelling is
//! found whatever its case (a reader that binds only the exact spelling leaves such a reference unbound, and a renumbering keeps it right for one that does
//! not), a row's name whatever its case, and **a cell's name exactly**, among the cells a row of the section holds.
//!
//! **Hostile text.** The scan is one pass: every byte is passed over once and charged one step of the caller's budget when it is, whatever the text holds (quotes
//! that never close, brackets that never end, a name of 8 KiB, 65,536 references), and a renumbering charges the bytes it writes too. A budget that runs out is
//! an error in words, and no text comes out of a refused renumbering. A row number past 4,294,967,295 is an error in words and is never wrapped.

use diacraft_doc::MAX_NAME_BYTES;

/// The spelling of the one section this scanner reads: `Connection` rows are named by `Connections` in a reference (the table).
pub const CONNECTIONS: &str = "Connections";

/// The cells a row of the `Connections` section holds (design §6.1), which a reference names exactly. A name that is none of these is no reference to a row
/// (`Connections.Row_3` with no cell is not one: the name has a digit at its end and no cell is called `Row_`).
const CONNECTION_CELLS: [&str; 11] = ["X", "Y", "DirX", "DirY", "Type", "A", "B", "C", "D", "AutoGen", "Prompt"];

/// The most digits an `ID` has (`u32::MAX` has ten).
const MAX_SHEET_DIGITS: usize = 10;

/// The segments of one path that are kept. A target is ours with two or three (`Connections.X1`, `Connections.Row_3.X`) and a qualifier with two (`Sheet.5`):
/// a path with more is none of them, and only its count is kept.
const MAX_KEPT_SEGMENTS: usize = 4;

/// The row a reference names, as the text says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowName {
    /// A number: the row's `IX` plus one. A number of 0 names no row.
    Number(u32),
    /// A name, as the text spells it (a quoted name without its quotes and with its doubled quotes made one).
    Name(String),
    /// No row at all (`Connections.X`): row 1. [`renumber`] gives a map `Number(1)` for it.
    Default,
}

/// One reference to a row, found in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowRef {
    /// Where the part of the reference that says the cell and the row is, in bytes: after `Connections.` and up to the end of the reference. `X12` for
    /// `Connections.X12`, `DirX[7]` for `Connections.DirX[7]`, `Row_3.X` for `Connections.Row_3.X`, `X` for a bare `Connections.X`. It is what
    /// [`renumber`] replaces, and only that.
    pub start: usize,
    pub end: usize,
    /// The shape the reference names by `Sheet.<id>!`; `None` for a reference with no qualifier, which names the sheet the formula is on.
    pub sheet: Option<u32>,
    pub row: RowName,
    /// The cell, as the text spells it: `X`, `DirY`.
    pub cell: String,
}

/// Whose rows a renumbering is about: shape `sheet`, and whether a reference with no qualifier is its (the formula is on its own sheet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowTarget {
    /// The shape's `ID`, which `Sheet.<id>!` names.
    pub sheet: u32,
    /// Whether a reference with no qualifier names this shape too: true for a formula of the shape's own sheet.
    pub unqualified: bool,
}

impl RowTarget {
    /// The rows of shape `sheet` in a formula of its own sheet: the references with no qualifier and those that name it.
    pub fn own(sheet: u32) -> RowTarget {
        RowTarget { sheet, unqualified: true }
    }

    /// The rows of shape `sheet` in a formula of another sheet: only the references that name it.
    pub fn of_sheet(sheet: u32) -> RowTarget {
        RowTarget { sheet, unqualified: false }
    }
}

/// Why a text was not read or could not be given a new row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowRefError {
    /// The budget ran out: the text is longer than the work the caller allowed.
    BudgetSpent,
    /// A row number in a reference does not fit 4,294,967,295; `at` is the byte where its digits start.
    NumberTooLarge { at: usize },
    /// The section is none that this scanner reads.
    UnknownSection(String),
    /// The row a map gave cannot be written: why.
    Unwritable(String),
}

impl std::fmt::Display for RowRefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RowRefError::BudgetSpent => {
                f.write_str("the formula is too long to look through for references to the rows of a section (the work budget is spent)")
            }
            RowRefError::NumberTooLarge { at } => write!(f, "a row number in a reference is larger than {} (at byte {at})", u32::MAX),
            RowRefError::UnknownSection(name) => write!(f, "references to the rows of the section `{name}` are not read"),
            RowRefError::Unwritable(why) => write!(f, "a reference cannot be written: {why}"),
        }
    }
}

impl std::error::Error for RowRefError {}

/// Whether the text holds the section's word in any letter case: one pass of its bytes that allocates nothing, so a caller that holds a great many formulas
/// reads each once before it pays for [`find`] or [`renumber`]. The caller charges the pass (the text's length).
pub fn mentions(text: &str, section: &str) -> bool {
    let word = section.as_bytes();
    !word.is_empty() && text.as_bytes().windows(word.len()).any(|window| window.eq_ignore_ascii_case(word))
}

/// The value of a text of ASCII digits: `None` for no digits, a byte that is no digit, and a number that does not fit a `u32`. Leading zeros are allowed. No
/// wrap: every step is checked.
pub fn digits_value(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for byte in digits {
        let digit = byte.checked_sub(b'0').filter(|d| *d <= 9)?;
        value = value.checked_mul(10)?.checked_add(u32::from(digit))?;
    }
    Some(value)
}

/// Every reference to a row of `section` in `text`, in the order they stand, whichever sheet they name (`RowRef::sheet`): the ones with no qualifier and those
/// after `Sheet.<id>!`. `section` is its spelling in a reference, `Connections` ([`CONNECTIONS`]), in any letter case; another is refused and costs nothing.
///
/// Every byte of the text costs one step of `budget`; when it holds fewer, the call stops with [`RowRefError::BudgetSpent`] and `budget` is 0. A row number
/// that does not fit is [`RowRefError::NumberTooLarge`].
pub fn find(text: &str, section: &str, budget: &mut usize) -> Result<Vec<RowRef>, RowRefError> {
    let found = scan(text, section, budget)?;
    if let Some(at) = found.iter().find_map(|f| f.overflow) {
        return Err(RowRefError::NumberTooLarge { at });
    }
    Ok(found.into_iter().map(|f| f.reference).collect())
}

/// The text with each row of `of` that `map` moves given its new number or name, and every other byte as it was; `None` when nothing changes, so a caller keeps
/// the text it has. A reference is of `of` when it names the shape by `Sheet.<id>!`, or has no qualifier and `of.unqualified` holds ([`RowTarget`]); the others
/// are left, however they read.
///
/// `map` is given the row as the text names it (a bare `Connections.X` as `Number(1)`) and answers the row it is to name, or `None` to leave it. A row that it
/// names as it is, in any form (`Number(1)` for `X01`), changes no byte. A row written is in the form the text had: `X5` stays `X<n>`, `DirX[5]` stays `DirX[<n>]`,
/// a name stays `<name>.X` and is quoted when it is no plain word; a number given for a name, or a name for a number, takes the other form. A number of 0 and
/// a name that is empty or longer than the model's names are [`RowRefError::Unwritable`].
///
/// The budget is charged as [`find`] does, and the bytes written are charged too, one step each.
pub fn renumber(
    text: &str,
    section: &str,
    of: RowTarget,
    map: &dyn Fn(&RowName) -> Option<RowName>,
    budget: &mut usize,
) -> Result<Option<String>, RowRefError> {
    let found = scan(text, section, budget)?;
    let mut out = String::new();
    // The text before `copied` is in `out`.
    let mut copied = 0_usize;
    let mut changed = false;
    for f in &found {
        let applies = match f.reference.sheet {
            None => of.unqualified,
            Some(id) => id == of.sheet,
        };
        if !applies {
            continue;
        }
        if let Some(at) = f.overflow {
            return Err(RowRefError::NumberTooLarge { at });
        }
        let current = match &f.reference.row {
            RowName::Default => RowName::Number(1),
            other => other.clone(),
        };
        let new = match map(&current) {
            None => continue,
            Some(RowName::Default) => RowName::Number(1),
            Some(row) => row,
        };
        if new == current {
            continue;
        }
        let tail = spell(f, &new)?;
        spend(budget, tail.len())?;
        let (Some(before), true) = (text.get(copied..f.reference.start), f.reference.start >= copied) else { continue };
        out.push_str(before);
        out.push_str(&tail);
        copied = f.reference.end;
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    if let Some(rest) = text.get(copied..) {
        out.push_str(rest);
    }
    Ok(Some(out))
}

/// Charges `n` steps: an error, with nothing left, when there are fewer.
fn spend(budget: &mut usize, n: usize) -> Result<(), RowRefError> {
    match budget.checked_sub(n) {
        Some(left) => {
            *budget = left;
            Ok(())
        }
        None => {
            *budget = 0;
            Err(RowRefError::BudgetSpent)
        }
    }
}

/// How a reference spells its row, which is the form a new row is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// `X12`: the cell and the number run together.
    Attached,
    /// `DirX[7]`.
    Bracket,
    /// `X`: no number.
    Bare,
    /// `Row_3.X` or `'Row 3'.X`.
    Named,
}

/// A reference found, and what the renumbering needs of it beyond [`RowRef`].
struct Found {
    reference: RowRef,
    form: Form,
    /// Where a row number that does not fit starts: the reference is then no more than a place for an error.
    overflow: Option<usize>,
}

/// The text a reference puts in place of its tail (`RowRef::start` to `RowRef::end`) to name `new`.
fn spell(found: &Found, new: &RowName) -> Result<String, RowRefError> {
    let cell = &found.reference.cell;
    match new {
        RowName::Number(0) => Err(RowRefError::Unwritable("row numbers start at 1".to_owned())),
        RowName::Number(n) if found.form == Form::Bracket => Ok(format!("{cell}[{n}]")),
        RowName::Number(n) => Ok(format!("{cell}{n}")),
        RowName::Default => Ok(format!("{cell}1")),
        RowName::Name(name) if name.is_empty() => Err(RowRefError::Unwritable("a row name cannot be empty".to_owned())),
        RowName::Name(name) if name.len() > MAX_NAME_BYTES => Err(RowRefError::Unwritable(format!("a row name is at most {MAX_NAME_BYTES} bytes"))),
        RowName::Name(name) => Ok(format!("{}.{cell}", name_text(name))),
    }
}

/// A row's name as a reference writes it: as it is when every byte is a letter, digit or underscore of ASCII, else in single quotes with a quote doubled.
fn name_text(name: &str) -> String {
    if name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { name.to_owned() } else { format!("'{}'", name.replace('\'', "''")) }
}

/// A byte of a name: a letter, a digit, `_` or a byte of a character that is not ASCII (a name may hold any letter).
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// A byte a name may start with, in the first part of a path.
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

/// A byte that, right before a name, makes it the middle or the end of something else and not the start of a path.
fn is_joiner(b: u8) -> bool {
    is_ident_byte(b) || matches!(b, b'.' | b'[' | b']' | b'\'' | b'"')
}

/// What the qualifiers before a target say about its sheet.
#[derive(Clone, Copy)]
enum Quals {
    /// None: the sheet the formula is on.
    None,
    /// Just `Sheet.<id>`.
    Sheet(u32),
    /// Anything else: another qualifier, a second one, a name, a call.
    Other,
}

/// One segment of a path: a name or a quoted name, and the row number in brackets after it if there is one.
struct Seg {
    start: usize,
    end: usize,
    quoted: bool,
    index: Option<Index>,
}

/// `[7]`: where the digits are, and the byte after the closing bracket.
struct Index {
    digits: (usize, usize),
    end: usize,
}

/// A path: its first segments, and how many it had.
struct Part {
    segs: Vec<Seg>,
    count: usize,
}

/// The one pass over a text.
struct Scan<'a> {
    text: &'a str,
    bytes: &'a [u8],
    section: &'a str,
    at: usize,
    /// The last byte passed over that is not white space: 0 at the start. A name right after a `!` is qualified by what came before it.
    last: u8,
    budget: &'a mut usize,
    found: Vec<Found>,
}

/// The references of `text` and the work done: see [`find`].
fn scan(text: &str, section: &str, budget: &mut usize) -> Result<Vec<Found>, RowRefError> {
    if !section.eq_ignore_ascii_case(CONNECTIONS) {
        return Err(RowRefError::UnknownSection(section.to_owned()));
    }
    let mut scan = Scan { text, bytes: text.as_bytes(), section, at: 0, last: 0, budget, found: Vec::new() };
    scan.run()?;
    Ok(scan.found)
}

impl Scan<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// How many bytes from here on satisfy `keep`: a look, which the caller takes at once.
    fn run_of(&self, keep: impl Fn(u8) -> bool) -> usize {
        self.bytes.get(self.at..).map_or(0, |rest| rest.iter().take_while(|b| keep(**b)).count())
    }

    /// Passes over `n` bytes, charging one step each.
    fn take(&mut self, n: usize) -> Result<(), RowRefError> {
        spend(self.budget, n)?;
        let end = self.at.saturating_add(n).min(self.bytes.len());
        if let Some(b) = self.bytes.get(self.at..end).and_then(|taken| taken.iter().rev().find(|b| !b.is_ascii_whitespace())) {
            self.last = *b;
        }
        self.at = end;
        Ok(())
    }

    fn skip_white(&mut self) -> Result<(), RowRefError> {
        let n = self.run_of(|b| b.is_ascii_whitespace());
        self.take(n)
    }

    /// The whole text, one byte at a time outside a string or a quoted name, a path at a time where one starts.
    fn run(&mut self) -> Result<(), RowRefError> {
        let (mut in_text, mut in_name) = (false, false);
        while let Some(b) = self.peek() {
            match b {
                b'"' if !in_name => {
                    in_text = !in_text;
                    self.take(1)?;
                }
                b'\'' if !in_text => {
                    in_name = !in_name;
                    self.take(1)?;
                }
                _ if in_text || in_name => self.take(1)?,
                _ if self.starts_path(b) => self.chain()?,
                _ => self.take(1)?,
            }
        }
        Ok(())
    }

    /// Whether a path starts at this name: nothing right before it joins it to another name, a dot, a bracket or a quote.
    fn starts_path(&self, b: u8) -> bool {
        is_ident_start(b) && self.at.checked_sub(1).and_then(|before| self.bytes.get(before)).is_none_or(|before| !is_joiner(*before))
    }

    /// A chain of paths joined by `!`, from a name: the last is the target, the others qualify it. A call, a path that is no path or a name that is not followed by
    /// what a name is stops the chain; the bytes it passed over are never read again.
    fn chain(&mut self) -> Result<(), RowRefError> {
        // A name right after a `!` that this chain did not read (the arguments of a call, a bracket, a quoted name came between) has a qualifier we know nothing of.
        let mut quals = if self.last == b'!' { Quals::Other } else { Quals::None };
        loop {
            let Some(part) = self.part()? else { return Ok(()) };
            self.skip_white()?;
            match self.peek() {
                // A call: its arguments are scanned on their own, and a path after its `!` is read as qualified by a call.
                Some(b'(') => return Ok(()),
                Some(b'!') => {
                    self.take(1)?;
                    quals = match (quals, self.sheet_id(&part)) {
                        (Quals::None, Some(id)) => Quals::Sheet(id),
                        _ => Quals::Other,
                    };
                    self.skip_white()?;
                    if !matches!(self.peek(), Some(b) if is_ident_start(b) || b == b'\'') {
                        return Ok(());
                    }
                }
                _ => {
                    self.record(quals, &part);
                    return Ok(());
                }
            }
        }
    }

    /// A path: segments joined by dots, each a name or a quoted name with a row number in brackets after it if it has one. `None` where the text stops being
    /// one (a dot with no name after it, brackets that hold more than a number).
    fn part(&mut self) -> Result<Option<Part>, RowRefError> {
        let mut part = Part { segs: Vec::new(), count: 0 };
        loop {
            let Some(seg) = self.seg(part.count == 0)? else { return Ok(None) };
            part.count = part.count.saturating_add(1);
            if part.segs.len() < MAX_KEPT_SEGMENTS {
                part.segs.push(seg);
            }
            if self.peek() != Some(b'.') {
                return Ok(Some(part));
            }
            self.take(1)?;
            if !matches!(self.peek(), Some(b) if is_ident_byte(b) || b == b'\'') {
                return Ok(None);
            }
        }
    }

    fn seg(&mut self, first: bool) -> Result<Option<Seg>, RowRefError> {
        let start = self.at;
        let quoted = self.peek() == Some(b'\'');
        if quoted {
            if !self.quoted_name()? {
                return Ok(None);
            }
        } else {
            let n = self.run_of(is_ident_byte);
            if n == 0 || (first && self.peek().is_some_and(|b| !is_ident_start(b))) {
                return Ok(None);
            }
            self.take(n)?;
        }
        let end = self.at;
        let index = if self.peek() == Some(b'[') {
            match self.index()? {
                Some(index) => Some(index),
                None => return Ok(None),
            }
        } else {
            None
        };
        Ok(Some(Seg { start, end, quoted, index }))
    }

    /// A quoted name, from its opening quote: a doubled quote in it is one. `false` when it never closes (the rest of the text is passed over).
    fn quoted_name(&mut self) -> Result<bool, RowRefError> {
        self.take(1)?;
        loop {
            let to_quote = self.bytes.get(self.at..).and_then(|rest| rest.iter().position(|b| *b == b'\''));
            let Some(n) = to_quote else {
                let rest = self.bytes.len().saturating_sub(self.at);
                self.take(rest)?;
                return Ok(false);
            };
            self.take(n.saturating_add(1))?;
            if self.peek() == Some(b'\'') {
                self.take(1)?;
            } else {
                return Ok(true);
            }
        }
    }

    /// `[`, white space, digits, white space, `]`: the row number of a cell written as `DirX[7]`. `None` for anything else, the bytes up to the one that did not
    /// fit passed over.
    fn index(&mut self) -> Result<Option<Index>, RowRefError> {
        self.take(1)?;
        self.skip_white()?;
        let first = self.at;
        let n = self.run_of(|b| b.is_ascii_digit());
        self.take(n)?;
        let digits = (first, self.at);
        self.skip_white()?;
        if n == 0 || self.peek() != Some(b']') {
            return Ok(None);
        }
        self.take(1)?;
        Ok(Some(Index { digits, end: self.at }))
    }

    /// The `ID` that a path `Sheet.<id>` names: the word `Sheet` in any case, one name of digits that fit an `ID`, and nothing else.
    fn sheet_id(&self, part: &Part) -> Option<u32> {
        let [word, id] = part.segs.as_slice() else { return None };
        if part.count != 2 || word.quoted || id.quoted || word.index.is_some() || id.index.is_some() {
            return None;
        }
        if !self.text.get(word.start..word.end)?.eq_ignore_ascii_case("sheet") {
            return None;
        }
        let digits = self.bytes.get(id.start..id.end)?;
        if digits.len() > MAX_SHEET_DIGITS {
            return None;
        }
        digits_value(digits)
    }

    /// Keeps the target if it is a reference to a row of the section on the sheet the qualifiers name.
    fn record(&mut self, quals: Quals, part: &Part) {
        let sheet = match quals {
            Quals::None => None,
            Quals::Sheet(id) => Some(id),
            Quals::Other => return,
        };
        if let Some(found) = self.reference(sheet, part) {
            self.found.push(found);
        }
    }

    fn reference(&self, sheet: Option<u32>, part: &Part) -> Option<Found> {
        let first = part.segs.first()?;
        if first.quoted || first.index.is_some() || !self.text.get(first.start..first.end)?.eq_ignore_ascii_case(self.section) {
            return None;
        }
        match (part.count, part.segs.as_slice()) {
            (2, [_, one]) => self.numbered(sheet, one),
            (3, [_, row, cell]) => self.named(sheet, row, cell),
            _ => None,
        }
    }

    /// `X12`, `X` and `DirX[7]`: a cell and the row's number, a missing number being row 1.
    fn numbered(&self, sheet: Option<u32>, seg: &Seg) -> Option<Found> {
        if seg.quoted {
            return None;
        }
        let name = self.text.get(seg.start..seg.end)?;
        let (cell, digits, form, end) = match &seg.index {
            Some(index) => (name, self.text.get(index.digits.0..index.digits.1)?, Form::Bracket, index.end),
            None => {
                let cut = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
                let (cell, digits) = (name.get(..cut)?, name.get(cut..)?);
                (cell, digits, if digits.is_empty() { Form::Bare } else { Form::Attached }, seg.end)
            }
        };
        if !CONNECTION_CELLS.contains(&cell) {
            return None;
        }
        let (row, overflow) = if form == Form::Bare {
            (RowName::Default, None)
        } else {
            match digits_value(digits.as_bytes()) {
                Some(n) => (RowName::Number(n), None),
                None => (RowName::Number(0), Some(self.digits_at(seg, digits.len()))),
            }
        };
        Some(Found { reference: RowRef { start: seg.start, end, sheet, row, cell: cell.to_owned() }, form, overflow })
    }

    /// Where the digits of a row number start: in the brackets, or at the end of the name that has them last.
    fn digits_at(&self, seg: &Seg, digits: usize) -> usize {
        match &seg.index {
            Some(index) => index.digits.0,
            None => seg.end.saturating_sub(digits),
        }
    }

    /// `Row_3.X` and `'Row 3'.X`: a row by name and a cell.
    fn named(&self, sheet: Option<u32>, row: &Seg, cell: &Seg) -> Option<Found> {
        if row.index.is_some() || cell.index.is_some() || cell.quoted {
            return None;
        }
        let cell_name = self.text.get(cell.start..cell.end)?;
        if !CONNECTION_CELLS.contains(&cell_name) {
            return None;
        }
        let name = if row.quoted {
            self.text.get(row.start.checked_add(1)?..row.end.checked_sub(1)?)?.replace("''", "'")
        } else {
            self.text.get(row.start..row.end)?.to_owned()
        };
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return None;
        }
        let reference = RowRef { start: row.start, end: cell.end, sheet, row: RowName::Name(name), cell: cell_name.to_owned() };
        Some(Found { reference, form: Form::Named, overflow: None })
    }
}
