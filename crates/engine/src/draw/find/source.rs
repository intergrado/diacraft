//! The text of a shape that Find can look in, and what a replacement of each would be.
//!
//! **A shape has up to four kinds of place** (Shape text, Shape data, Shape name and User-defined cell), in this order:
//!
//! * **name**: the shape's `Name`, or its `NameU` when it has no `Name`;
//! * **text**: what the shape's text block shows: its characters, and the results of its fields in place (a date field shows its date, and a search finds it);
//! * **data**: the `Value` cell of each row of the shape's Shape Data section (`Property`), as the shape and its master together have it;
//! * **user**: the `Value` cell of each row of its User section, the same way.
//!
//! A data or user cell is searched when it holds text: a cell whose formula is one quoted string, a cell that is a constant, and a cell that a formula works out
//! (found, never replaced). A cell with nothing in it is not a place.
//!
//! **What a replacement may do** is held with the place ([`How`]): a text's matches that touch the result of a field are left (a field is worked out, not typed over),
//! a cell that a formula works out is left, and the others are written. The reasons are words, [`Why`].

use std::borrow::Cow;

use diacraft_doc::{FormulaSlot, Owner, Reading, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, TextPart};

use super::pattern::{Refused, Work};
use super::walk::Visit;

/// The steps a row of shape data costs to read: it is merged from the shape, its master and the styles, and its cells are resolved. **Measured** (`find_cost_timed_against_a_yardstick`): a row is
/// about 2.5 microseconds, and a step of the character loop about 5 nanoseconds, so a row is charged 400.
pub const ROW_STEPS: u64 = 400;

/// The most rows of one section that one shape may have for Find to read them: a shape of a hostile file may inherit a section of hundreds of thousands.
pub const MAX_ROWS_PER_SHAPE: usize = 20_000;

/// What kind of place a piece of text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Name,
    Text,
    Data,
    User,
}

impl Kind {
    /// The word the commands use.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Name => "shape_name",
            Kind::Text => "text",
            Kind::Data => "shape_data",
            Kind::User => "user_cell",
        }
    }

    /// The kind a word names.
    pub fn from_word(word: &str) -> Option<Kind> {
        [Kind::Name, Kind::Text, Kind::Data, Kind::User].into_iter().find(|k| k.word() == word)
    }

    fn section(self) -> Option<&'static str> {
        match self {
            Kind::Data => Some("Property"),
            Kind::User => Some("User"),
            _ => None,
        }
    }
}

/// Which kinds of place a search looks in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Within {
    pub name: bool,
    pub text: bool,
    pub data: bool,
    pub user: bool,
}

impl Within {
    /// Shape text alone.
    pub const TEXT: Within = Within { name: false, text: true, data: false, user: false };

    /// Whether `kind` is looked in.
    pub fn has(&self, kind: Kind) -> bool {
        match kind {
            Kind::Name => self.name,
            Kind::Text => self.text,
            Kind::Data => self.data,
            Kind::User => self.user,
        }
    }

    /// Whether the cells of rows are looked in.
    pub fn rows(&self) -> bool {
        self.data || self.user
    }
}

/// Why a match is not replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// It touches what a field shows.
    InField,
    /// The cell is worked out by a formula.
    Computed,
    /// The text it is in would be longer than the most a shape's text may be.
    TooLong,
    /// The result is not one that place can hold (an empty or too long name, a number in a cell of text, a control character).
    NotAllowed,
    /// The shape is on a locked layer, and a shape on a locked layer is not edited: a replace passes over it.
    Locked,
}

impl Why {
    /// The words for a person.
    pub fn words(self) -> &'static str {
        match self {
            Why::InField => "it is in a field, which is worked out and not typed over",
            Why::Computed => "its cell is worked out by a formula",
            Why::TooLong => "the text would be too long",
            Why::NotAllowed => "the result is not allowed there",
            Why::Locked => "its shape is on a locked layer, and a shape on a locked layer is not edited until the layer is unlocked",
        }
    }
}

/// How a cell of a row holds its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellHow {
    /// No formula: the value is the text.
    Constant,
    /// A formula that is one quoted string.
    Literal,
    /// Any other formula.
    Computed,
}

/// How a place is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    Name,
    /// The text of the shape, and where its fields' results are in it (`(start, end)` in characters of what it shows).
    Text {
        fields: Vec<(usize, usize)>,
    },
    Cell {
        key: RowKey,
        how: CellHow,
    },
}

/// One place of a shape, with its text.
pub struct Source<'a> {
    pub kind: Kind,
    /// The row's name, for a data or user cell.
    pub row: Option<String>,
    pub text: Cow<'a, str>,
    pub how: How,
}

/// The text a formula that is one quoted string stands for: `"a ""b"""` is `a "b"`.
pub fn literal_of(source: &str) -> Option<String> {
    let inner = source.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '"' && chars.next() != Some('"') {
            return None;
        }
        out.push(c);
    }
    Some(out)
}

/// `text` as a formula that is one quoted string.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for c in text.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// What a text shows, and where its fields' results are in it, as `(start, end)` in characters.
type Shown<'a> = (Cow<'a, str>, Vec<(usize, usize)>);

/// What a shape's text block shows, and where the fields' results are in it. A text with one run of characters and no field is borrowed.
fn shown_text(shape: &diacraft_doc::Shape) -> Option<Shown<'_>> {
    let text = shape.text.as_ref()?;
    let mut only: Option<&str> = None;
    let mut pieces = 0_usize;
    let mut has_field = false;
    for part in &text.parts {
        match part {
            TextPart::Chars(s) => {
                pieces += 1;
                only = Some(s);
            }
            TextPart::Fld { .. } => has_field = true,
            _ => {}
        }
    }
    if pieces == 0 && !has_field {
        return None;
    }
    if pieces == 1 && !has_field {
        return only.map(|s| (Cow::Borrowed(s), Vec::new()));
    }
    let mut out = String::new();
    let mut fields = Vec::new();
    let mut at = 0_usize;
    for part in &text.parts {
        match part {
            TextPart::Chars(s) => {
                out.push_str(s);
                at = at.saturating_add(s.chars().count());
            }
            TextPart::Fld { text, .. } => {
                let n = text.chars().count();
                if n > 0 {
                    fields.push((at, at.saturating_add(n)));
                }
                out.push_str(text);
                at = at.saturating_add(n);
            }
            _ => {}
        }
    }
    Some((Cow::Owned(out), fields))
}

/// The bytes the text of a shape holds, for the charge before it is read.
fn text_bytes(shape: &diacraft_doc::Shape) -> usize {
    shape.text.as_ref().map_or(0, |t| {
        t.parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(s) => s.len(),
                TextPart::Fld { text, .. } => text.len(),
                _ => 0,
            })
            .fold(0, usize::saturating_add)
    })
}

/// The places of the shape in `visit` that `within` asks for, in order, each with its text. Charged to `work` before they are read.
pub fn sources<'a>(resolver: &Resolver<'a>, visit: &Visit<'a, '_>, within: Within, work: &mut Work) -> Result<Vec<Source<'a>>, Refused> {
    let mut out: Vec<Source<'a>> = Vec::new();
    let shape = visit.shape;
    if within.name
        && let Some(name) = shape.name.as_deref().filter(|n| !n.is_empty()).or_else(|| shape.name_u.as_deref().filter(|n| !n.is_empty()))
    {
        out.push(Source { kind: Kind::Name, row: None, text: Cow::Borrowed(name), how: How::Name });
    }
    if within.text {
        work.charge(u64::try_from(text_bytes(shape) / 8).unwrap_or(u64::MAX))?;
        if let Some((text, fields)) = shown_text(shape)
            && !text.is_empty()
        {
            out.push(Source { kind: Kind::Text, row: None, text, how: How::Text { fields } });
        }
    }
    if within.rows() {
        let at = SheetRef::Shape(ShapeRef { owner: Owner::Page(visit.page), path: visit.path.to_vec() });
        for kind in [Kind::Data, Kind::User] {
            if within.has(kind)
                && let Some(section) = kind.section()
            {
                rows_of(resolver, &at, kind, section, &mut out, work)?;
            }
        }
    }
    Ok(out)
}

/// The `Value` cells of the rows of one section of the shape at `at`, as places.
fn rows_of<'a>(resolver: &Resolver<'a>, at: &SheetRef, kind: Kind, section: &str, out: &mut Vec<Source<'a>>, work: &mut Work) -> Result<(), Refused> {
    let key = SectionKey::new(section, None);
    let held = resolver.rows_held(at, &key).unwrap_or(0);
    if held == 0 {
        return Ok(());
    }
    work.charge(u64::try_from(held).unwrap_or(u64::MAX).saturating_mul(ROW_STEPS))?;
    // A shape with more rows than a shape has is left: its master is hostile, and what it would cost is the work budget's.
    let Some(Ok(rows)) = resolver.effective_rows_within(at, &key, MAX_ROWS_PER_SHAPE) else {
        work.note_unread();
        return Ok(());
    };
    for row in rows {
        let Some((_, value)) = row.cells.iter().find(|(name, _)| name.as_str() == "Value") else { continue };
        let literal = match value.formula.cell.map(|c| &c.formula) {
            Some(FormulaSlot::Local(f)) => Some(literal_of(&f.source).ok_or(())),
            _ => None,
        };
        let held_text = value.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text());
        let (text, how): (Cow<'a, str>, CellHow) = match (literal, held_text) {
            (Some(Ok(text)), _) => (Cow::Owned(text), CellHow::Literal),
            (Some(Err(())), Some(text)) => (Cow::Borrowed(text), CellHow::Computed),
            (Some(Err(())), None) => continue,
            (None, Some(text)) => (Cow::Borrowed(text), CellHow::Constant),
            (None, None) => continue,
        };
        if text.is_empty() {
            continue;
        }
        let name = match &row.key {
            RowKey::Name(n) => n.to_string(),
            RowKey::Ix(ix) => ix.to_string(),
        };
        out.push(Source { kind, row: Some(name), text, how: How::Cell { key: row.key.clone(), how } });
    }
    Ok(())
}

/// Whether the text `new`, written to a cell that held text, still reads as text: a number would change what the cell is.
pub fn stays_text(new: &str) -> bool {
    !matches!(diacraft_doc::Value::from_text(new).reading(), Reading::Number(_) | Reading::Infinite { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_string_formula_is_read_and_written_with_its_quotes_doubled() {
        for text in ["", "plain", "say \"hi\"", "\"", "\"\"", "a\"\"b", "tab\there", "é 🙂"] {
            assert_eq!(literal_of(&quoted(text)).as_deref(), Some(text), "{text:?}");
        }
        assert_eq!(quoted("a\"b"), "\"a\"\"b\"");
        assert_eq!(literal_of("  \"padded\"  ").as_deref(), Some("padded"), "a formula is trimmed");
        for not_a_literal in ["", "abc", "\"", "\"abc", "abc\"", "\"a\"b\"", "\"a\" & \"b\"", "Width*2", "\"a\"&\"b\"", "1"] {
            assert_eq!(literal_of(not_a_literal), None, "{not_a_literal:?}");
        }
    }

    #[test]
    fn a_cell_of_text_may_not_become_a_number() {
        assert!(stays_text("hello") && stays_text("") && stays_text("TRUE") && stays_text("#ff0000"));
        assert!(!stays_text("42") && !stays_text("1.5") && !stays_text("-3") && !stays_text("1.#INF"));
    }

    #[test]
    fn the_words_of_the_places_are_the_commands() {
        for kind in [Kind::Name, Kind::Text, Kind::Data, Kind::User] {
            assert_eq!(Kind::from_word(kind.word()), Some(kind));
        }
        assert_eq!(Kind::from_word("notes"), None);
        assert!(Kind::Name < Kind::Text && Kind::Text < Kind::Data && Kind::Data < Kind::User, "the order of a shape's places");
    }
}
