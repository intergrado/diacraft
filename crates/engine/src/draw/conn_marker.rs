//! The marker of the connection points a doubling added: the User row `DiaCraftAddedPoints` (design §3.1).
//!
//! **Which doubling made a point** is kept where every format keeps it, in a cell. The shape's `User` row `DiaCraftAddedPoints` holds in `Value` a formula that
//! names each added point of the shape by reference, with its generation as the factor, beside the marker `User.DiaCraftRoute` that DiaCraft gives its connectors:
//!
//! ```text
//! 0*(1*Connections.X5+1*Connections.X6+2*Connections.X9)
//! ```
//!
//! A reference moves with its row: the renumbering of `conn_refs` renumbers it with every other reference to the row, a copy of the shape keeps it (it names its own
//! sheet, so the copy's remapping leaves it), and a program that renumbers references when it renumbers rows keeps it right. Its value is always 0, so it means
//! nothing to a program that does not know it. A row that has a name is named by it (`1*Connections.Row_3.X`).
//!
//! **Read safely.** The marker is a list, not a formula to evaluate, and it is read by its text. A point is *added*, with generation *g*, only when the
//! formula is exactly of the form above (no white space, no qualifier, `Connections` and `X` as written, whole numbers with no leading zero, a row's name a plain word)
//! and the term gives *g* from 1 to 255 and names a row the section has, not deleted, that no other term names. **A formula of any other form** (a reference that
//! another program turned into `#REF!`, more than 256 terms, more than 8 KiB) **makes every point of the shape an original**; **a term that breaks a rule makes
//! its row an original** and the other terms stand. An original is never removed by a halving, so a marker that is lost, dropped or damaged is safe.
//!
//! **Every term that names a row counts toward it, whatever generation it gives** (a term of 0, 256 or 2^32 still names its row, so a row it shares with a term of
//! generation 1 is named twice and is an original), **and under any spelling of the row**: `Connections.X1` and `Connections.Tally.X` are two terms for the row at
//! `IX` 0 when it is named `Tally`. A row is told by its [`Found::identity`], the number the section gives it (its `IX`, or its place in a section keyed by name),
//! and the spelling a term has is kept apart from it, so that a lone term still stands, and is renumbered, as it is written.
//!
//! **A name is read as a reference reads it**: the row of that spelling first, and only when no row spells it so the first row of the name in any letter case ([`NameIndex`]).
//! A row whose name differs from another's only in case is another row, so `Connections.ROW_5.X` names the row `ROW_5` and `Connections.Row_5.X` the row `Row_5`, and a text that
//! spells a name in a case no row has (`row_5` where the rows are `Row_5` and `ROW_5`) names the first. A term keeps the spelling the section gives its row, so everything after
//! the reading (a generation, a halving's renumbering) finds the row by that spelling, and never by another case of it.
//!
//! [`parse`] reads a text against the rows a caller says there are ([`Found`]), [`read`] and [`read_in`] read a shape's marker against the rows the shape has, [`write`] writes
//! it, and [`Marker::renumbered`] turns the list into the list of the same points after a renumbering: a term of a moved row renumbered, a term of a removed row
//! dropped and **never mapped to a successor**, which could make an original an added point.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::row_refs::{RowName, digits_value};
use diacraft_doc::{
    Cell, CellAddr, Counted, Document, FormulaSlot, Lookup, MAX_NAME_BYTES, Owner, RowKey, RowShape, SectionKey, ShapeRef, Sheet, SheetRef, Value,
};

use super::read::Reader;
use super::{Problem, shape_mut};

/// The `N` of the marker's row in the shape's `User` section.
pub const MARKER_ROW: &str = "DiaCraftAddedPoints";

/// The greatest generation a term may give (a doubling reaches at most 8 under the cap of 256 points).
pub const MAX_GENERATION: u8 = 255;

/// The most terms a marker holds: a shape's points are at most 256, and a marker of more is read as void.
pub const MAX_MARKER_TERMS: usize = 256;

/// The longest marker formula, in bytes: a longer one is read as void and refused when written.
pub const MAX_MARKER_BYTES: usize = 8 * 1024;

/// The most rows of a shape's `Connection` section that a read of the marker looks at, counted over the shape, its master and its styles before any is merged: the
/// same 256 a snap reads and a doubling reads (design §9).
pub const MAX_ROWS_CHECKED: usize = 256;

/// The marker's cell: `User.DiaCraftAddedPoints.Value`.
pub fn marker_cell() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(MARKER_ROW), "Value")
}

/// One added point: its row, and which doubling of its edge made it (1 for the first, 2 for the next).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// The row by its number (the row's `IX` plus one) or by its name. Never [`RowName::Default`].
    pub row: RowName,
    /// From 1 to [`MAX_GENERATION`].
    pub generation: u8,
}

/// A row a term names, as the section has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The row in the spelling the marker keeps it in: a number as the term wrote it, a name as the section spells it. Never [`RowName::Default`].
    pub spelling: RowName,
    /// Which row it is, the same under every spelling of it: the row's `IX`, or its place in a section keyed by name (what a snap and a doubling number the row by).
    /// Two terms with one identity name the one row.
    pub identity: u32,
}

/// The added points of one shape and their generations: the terms of the marker that stand after the rules. Every other point of the shape is an
/// original.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marker {
    terms: Vec<Term>,
}

/// A row as a key that tells the same row in any spelling: a number, or a name in lower case.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Number(u32),
    Name(String),
}

fn key_of(row: &RowName) -> Key {
    match row {
        RowName::Number(n) => Key::Number(*n),
        RowName::Name(name) => Key::Name(name.to_ascii_lowercase()),
        RowName::Default => Key::Number(1),
    }
}

/// A name a marker can write: letters, digits and underscores of ASCII, at least one, within the model's longest name.
fn is_plain(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The text of one term: `<generation>*Connections.X<number>`, or `<generation>*Connections.<name>.X` for a row by its name. The inverse of [`parse_term`] for a
/// term [`Marker::with_terms`] accepts.
pub fn print_term(term: &Term) -> String {
    match &term.row {
        RowName::Number(n) => format!("{}*Connections.X{n}", term.generation),
        RowName::Name(name) => format!("{}*Connections.{name}.X", term.generation),
        RowName::Default => format!("{}*Connections.X1", term.generation),
    }
}

/// A term as the text has it, before the rules about generations and rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTerm {
    /// The generation as written; a number past `u32` is `u32::MAX`, which no generation is.
    pub generation: u32,
    /// The row; a number past `u32` is `Number(0)`, which names no row.
    pub row: RowName,
}

/// A whole number as a marker writes it.
enum Num {
    /// It fits a `u32`.
    Fits(u32),
    /// It is written in the one way and is larger than a `u32` holds.
    Past,
}

/// A whole number written the one way a marker writes it: ASCII digits, no sign, no leading zero (but a lone 0), at most 10 digits. `None` for any other text.
fn canonical_number(text: &str) -> Option<Num> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 || (bytes.len() > 1 && bytes.first() == Some(&b'0')) {
        return None;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(digits_value(bytes).map_or(Num::Past, Num::Fits))
}

/// One term of the formula by its text: the inverse of [`print_term`]. `None` for any text of another form, which makes the whole marker void.
pub fn parse_term(text: &str) -> Option<RawTerm> {
    let (generation, rest) = text.split_once('*')?;
    let generation = match canonical_number(generation)? {
        Num::Fits(n) => n,
        Num::Past => u32::MAX,
    };
    let tail = rest.strip_prefix("Connections.")?;
    let row = if let Some(name) = tail.strip_suffix(".X") {
        if !is_plain(name) {
            return None;
        }
        RowName::Name(name.to_owned())
    } else {
        match canonical_number(tail.strip_prefix('X')?)? {
            Num::Fits(n) => RowName::Number(n),
            Num::Past => RowName::Number(0),
        }
    };
    Some(RawTerm { generation, row })
}

/// The terms of a marker formula, or `None` when the text is not of the form `0*(<term>+<term>...)` within [`MAX_MARKER_BYTES`] and [`MAX_MARKER_TERMS`].
pub fn parse_formula(text: &str) -> Option<Vec<RawTerm>> {
    if text.len() > MAX_MARKER_BYTES {
        return None;
    }
    let inner = text.strip_prefix("0*(")?.strip_suffix(')')?;
    let mut terms = Vec::new();
    for piece in inner.split('+') {
        if terms.len() >= MAX_MARKER_TERMS {
            return None;
        }
        terms.push(parse_term(piece)?);
    }
    Some(terms)
}

/// The generation a term gives, if it is one: a whole number from 1 to [`MAX_GENERATION`] and no other.
pub fn term_generation(written: u32) -> Option<u8> {
    u8::try_from(written).ok().filter(|g| *g >= 1)
}

/// The terms of `raw` that stand under the rules: a row `resolve` says is there (and gives in its own spelling and by its identity), named by no other
/// term, and a generation from 1 to 255. **Every term that names a row is counted toward it before the generation is looked at**, so that a term of a generation no
/// point has still takes its row from a term of one that is: a row named twice is named by neither. Two terms that spell a name alike but for its case stand for neither,
/// though they name two rows (R63 tells the rows apart): the marker DiaCraft writes holds no such pair, because a new row's name is never one a row has in any case, so a
/// pair is another program's, and a row it names is read as an original, which is safe.
fn validate(raw: &[RawTerm], resolve: &dyn Fn(&RowName) -> Option<Found>) -> Marker {
    let named: Vec<(Found, u32)> = raw.iter().filter_map(|term| resolve(&term.row).map(|found| (found, term.generation))).collect();
    let (mut rows, mut spellings): (HashMap<u32, usize>, HashMap<Key, usize>) = (HashMap::new(), HashMap::new());
    for (found, _) in &named {
        *rows.entry(found.identity).or_insert(0) += 1;
        *spellings.entry(key_of(&found.spelling)).or_insert(0) += 1;
    }
    let terms = named
        .into_iter()
        .filter(|(found, _)| rows.get(&found.identity) == Some(&1) && spellings.get(&key_of(&found.spelling)) == Some(&1))
        .filter_map(|(found, written)| Some(Term { row: found.spelling, generation: term_generation(written)? }))
        .collect();
    Marker { terms }
}

/// Reads a marker's text against the rows `resolve` knows: `resolve` gives the row a term names as the section has it ([`Found`]), or `None` for a row it does not
/// have. The marker is void (no term stands) when the text is of another form. Also the number of terms the text had, for a caller that counts the ones that fell
/// away; `None` when the form is wrong.
pub fn parse_counted(text: &str, resolve: &dyn Fn(&RowName) -> Option<Found>) -> Option<(Marker, usize)> {
    let raw = parse_formula(text)?;
    Some((validate(&raw, resolve), raw.len()))
}

/// [`parse_counted`] without the count: a text of another form is an empty marker, which is what the points it names read as (originals).
pub fn parse(text: &str, resolve: &dyn Fn(&RowName) -> Option<Found>) -> Marker {
    parse_counted(text, resolve).map(|(marker, _)| marker).unwrap_or_default()
}

impl Marker {
    /// No added point.
    pub fn empty() -> Marker {
        Marker::default()
    }

    /// A marker of these terms, which must be ones it can write: at most [`MAX_MARKER_TERMS`], each of a generation from 1 to 255 and a row by a number from 1 or
    /// a plain name, no row twice, and the formula within [`MAX_MARKER_BYTES`]. A refusal says which rule.
    pub fn with_terms(terms: Vec<Term>) -> Result<Marker, Problem> {
        if terms.len() > MAX_MARKER_TERMS {
            return Err(Problem::bad(format!("a marker holds at most {MAX_MARKER_TERMS} terms")));
        }
        let mut seen: HashSet<Key> = HashSet::new();
        for term in &terms {
            if term.generation == 0 {
                return Err(Problem::bad("a marker's generation is a whole number from 1 to 255"));
            }
            match &term.row {
                RowName::Number(0) => return Err(Problem::bad("a marker's row numbers start at 1")),
                RowName::Number(_) => {}
                RowName::Default => return Err(Problem::bad("a marker names a row by its number or its name")),
                RowName::Name(name) if !is_plain(name) => {
                    return Err(Problem::bad("a marker names a row by a plain name: letters, digits and underscores"));
                }
                RowName::Name(_) => {}
            }
            if !seen.insert(key_of(&term.row)) {
                let row = match &term.row {
                    RowName::Name(name) => name.clone(),
                    RowName::Number(n) => n.to_string(),
                    RowName::Default => "1".to_owned(),
                };
                return Err(Problem::bad(format!("a marker names row {row} twice")));
            }
        }
        let marker = Marker { terms };
        if marker.formula_len() > MAX_MARKER_BYTES {
            return Err(Problem::bad(format!("a marker's formula is at most {MAX_MARKER_BYTES} bytes")));
        }
        Ok(marker)
    }

    /// The length of the formula [`Marker::text`] gives, counted and not built: `0*(`, the terms with a `+` between them, `)`.
    fn formula_len(&self) -> usize {
        let terms: usize = self.terms.iter().map(|t| print_term(t).len()).sum();
        terms.saturating_add(self.terms.len().saturating_sub(1)).saturating_add(4)
    }

    /// The terms that stand, in the order the formula gives them.
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Whether no point is added.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The generation of the point on `row`: 0 for an original (a row no term names). **A name is told by the spelling the term has it in**, which is the spelling the section
    /// gives the row ([`parse`] writes it so, whatever case a text spelled it in), and not in any case: a row whose name differs from another's only in case is another row
    /// (the exact spelling names the row first), so `ROW_5` is not the point a term of `Row_5` stands for. A reference with no row is row 1.
    pub fn generation_of(&self, row: &RowName) -> u8 {
        let wanted = if *row == RowName::Default { &RowName::Number(1) } else { row };
        self.terms.iter().find(|t| t.row == *wanted).map_or(0, |t| t.generation)
    }

    /// The newest generation among the terms: `None` when there are none.
    pub fn newest(&self) -> Option<u8> {
        self.terms.iter().map(|t| t.generation).max()
    }

    /// The formula of the marker's cell, `0*(1*Connections.X5+...)`; `None` for no term (the cell is not there).
    pub fn text(&self) -> Option<String> {
        if self.terms.is_empty() {
            return None;
        }
        let terms: Vec<String> = self.terms.iter().map(print_term).collect();
        Some(format!("0*({})", terms.join("+")))
    }

    /// The marker after a renumbering: each term's row as `f` gives it, and a term for which `f` gives `None` dropped. The generations stay with their points.
    pub fn renumbered(&self, f: &dyn Fn(&RowName) -> Option<RowName>) -> Marker {
        Marker { terms: self.terms.iter().filter_map(|t| f(&t.row).map(|row| Term { row, generation: t.generation })).collect() }
    }
}

/// The rows of a section that have a name, told by name as a reference in a formula tells them: **the exact spelling first**, and only when no row
/// spells the name so, the first row of that name in any letter case. Every reading of a name that a marker's term makes goes through it: the reading of the marker's text
/// ([`parse`]), and a halving's reading of which row each of the marker's terms names (`conn_points`). A look at each of two maps, however many rows share a name.
pub(crate) struct NameIndex {
    exact: HashMap<String, u32>,
    folded: HashMap<String, u32>,
}

impl NameIndex {
    /// `rows` are the named rows in the section's order, each as the number it is told by (its `IX`, or its place in a section keyed by name) and its name. Of rows that spell a
    /// name alike, and of rows that spell it alike but for its case, the first is the one found.
    pub(crate) fn new<'a>(rows: impl IntoIterator<Item = (u32, &'a str)>) -> NameIndex {
        let (mut exact, mut folded) = (HashMap::new(), HashMap::new());
        for (number, name) in rows {
            exact.entry(name.to_owned()).or_insert(number);
            folded.entry(name.to_ascii_lowercase()).or_insert(number);
        }
        NameIndex { exact, folded }
    }

    /// The number of the row `name` names: the row that spells it so, else the first row of the name in any case, else `None`.
    pub(crate) fn find(&self, name: &str) -> Option<u32> {
        self.exact.get(name).or_else(|| self.folded.get(&name.to_ascii_lowercase())).copied()
    }
}

/// The rows a section has, as `resolve` for [`parse`]: a number names the row of `IX` one less, or in a section keyed by name the row at that place; a name names
/// the row of that name (the exact spelling first, then the first of any case: R63, [`NameIndex`]). A row deleted at a step is not among `keys`. The identity of a row is
/// the number a doubling reads it by: its `IX`, or its place when it is keyed by name.
fn rows_of(keys: &[RowShape]) -> impl Fn(&RowName) -> Option<Found> + '_ {
    let names = NameIndex::new(keys.iter().enumerate().filter_map(|(place, k)| match &k.key {
        RowKey::Name(n) => u32::try_from(place).ok().map(|place| (place, n.as_ref())),
        RowKey::Ix(_) => None,
    }));
    move |row| match row {
        RowName::Number(n) => {
            let ix = n.checked_sub(1)?;
            let by_ix = keys.iter().any(|k| k.key == RowKey::Ix(ix));
            let by_place = || usize::try_from(ix).ok().and_then(|place| keys.get(place)).is_some_and(|k| matches!(k.key, RowKey::Name(_)));
            (by_ix || by_place()).then_some(Found { spelling: RowName::Number(*n), identity: ix })
        }
        RowName::Name(name) => {
            let place = names.find(name)?;
            let RowKey::Name(found) = &keys.get(usize::try_from(place).ok()?)?.key else { return None };
            Some(Found { spelling: RowName::Name(found.to_string()), identity: place })
        }
        RowName::Default => None,
    }
}

/// The text of the marker's formula on this sheet, found through inheritance, when it is a formula of the shape's own that fits.
fn formula_text(reader: &Reader<'_>, sheet: &SheetRef) -> Option<String> {
    let found = reader.resolver().resolve(sheet, &marker_cell())?;
    match &found.formula.cell?.formula {
        FormulaSlot::Local(formula) if formula.source.len() <= MAX_MARKER_BYTES => Some(formula.source.to_string()),
        _ => None,
    }
}

/// The marker of the shape at `at`, read against the rows the shape has now. A shape with no marker, a marker of another form and a shape with
/// more than [`MAX_ROWS_CHECKED`] rows give an empty one.
pub(crate) fn read(reader: &Reader<'_>, at: &ShapeRef) -> Marker {
    let sheet = SheetRef::Shape(at.clone());
    let Some(text) = formula_text(reader, &sheet) else { return Marker::empty() };
    let section = SectionKey::new("Connection", None);
    if reader.resolver().rows_held(&sheet, &section).unwrap_or(0) > MAX_ROWS_CHECKED {
        return Marker::empty();
    }
    let Some(keys) = reader.resolver().effective_row_keys(&sheet, &section) else { return Marker::empty() };
    parse(&text, &rows_of(&keys))
}

/// [`read`] for a caller that has a document and no reader.
pub fn read_in(doc: &Document, at: &ShapeRef) -> Marker {
    read(&Reader::new(doc), at)
}

/// Whether the shape's own sheet holds the marker's cell, and the text of its formula when it has one.
pub(crate) fn local_formula(sheet: &Sheet) -> Option<String> {
    match sheet.lookup(&marker_cell()) {
        Lookup::Found(cell) => cell.formula.text().filter(|_| cell.owns_formula()).map(str::to_owned),
        _ => None,
    }
}

/// Whether the shape's own sheet holds a row for the marker, with or without a formula.
fn has_row(sheet: &Sheet) -> bool {
    sheet
        .sections
        .iter()
        .any(|s| !s.del && s.name.as_str() == "User" && s.ix.is_none() && s.rows.iter().any(|r| r.name.as_deref() == Some(MARKER_ROW)))
}

/// Writes the marker as the shape's own `User` row `DiaCraftAddedPoints` with the formula of [`Marker::text`] and the value 0, or takes the row away for an
/// empty marker and the section with it when that leaves it with nothing. A document in which nothing would change is left as it is.
pub fn write(doc: &mut Document, at: &ShapeRef, marker: &Marker) -> Result<(), Problem> {
    let Owner::Page(page) = at.owner else { return Err(Problem::failed("a marker is written on a shape of a page")) };
    let text = marker.text();
    let Some(shape) = doc.shape(at) else { return Err(Problem::failed("the shape is not there")) };
    match &text {
        None if !has_row(&shape.sheet) => return Ok(()),
        Some(text) if local_formula(&shape.sheet).as_deref() == Some(text.as_str()) => return Ok(()),
        _ => {}
    }
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let mut counted = shape.counted_sheet();
    match text {
        None => remove_row(&mut counted),
        Some(text) => {
            let mut cell = match counted.sheet.lookup(&marker_cell()) {
                Lookup::Found(old) => old.clone(),
                _ => Cell::new(),
            };
            cell.formula = Cell::with_formula(&text, None).formula;
            cell.value = Some(Value::from_text("0"));
            cell.error = None;
            cell.unit = None;
            counted.set_cell(&marker_cell(), cell)?;
        }
    }
    Ok(())
}

/// Takes the marker's row out of the shape's own `User` section, and the section when nothing else is in it (the nodes that stood among the rows and the sections move with them).
fn remove_row(counted: &mut Counted<'_>) {
    let Some(at) = counted
        .sheet
        .sections
        .iter()
        .position(|s| s.name.as_str() == "User" && s.ix.is_none() && s.rows.iter().any(|r| r.name.as_deref() == Some(MARKER_ROW)))
    else {
        return;
    };
    let Some(slot) = counted.sheet.sections.get_mut(at) else { return };
    let section = Arc::make_mut(slot);
    section.retain_rows(|r| r.name.as_deref() != Some(MARKER_ROW));
    let bare = section.rows.is_empty()
        && section.cells.is_empty()
        && !section.del
        && section.raw.is_empty()
        && section.passthrough.is_empty()
        && section.ext.is_empty();
    if bare {
        counted.remove_sections(|s| {
            s.name.as_str() == "User"
                && s.ix.is_none()
                && s.rows.is_empty()
                && s.cells.is_empty()
                && !s.del
                && s.raw.is_empty()
                && s.passthrough.is_empty()
                && s.ext.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn term(row: RowName, generation: u8) -> Term {
        Term { row, generation }
    }

    #[test]
    fn a_term_prints_and_reads_back_for_every_generation_and_a_spread_of_rows() {
        for generation in 1..=MAX_GENERATION {
            for row in
                [RowName::Number(1), RowName::Number(256), RowName::Number(u32::MAX), RowName::Name("Row_3".into()), RowName::Name("X5".into())]
            {
                let t = term(row.clone(), generation);
                let back = parse_term(&print_term(&t)).unwrap();
                assert_eq!(back, RawTerm { generation: u32::from(generation), row }, "{}", print_term(&t));
            }
        }
    }

    #[test]
    fn a_term_of_another_shape_is_none() {
        for text in [
            "",
            "1",
            "1*",
            "1*Connections",
            "1*Connections.",
            "1*Connections.X",
            "1*Connections.X-1",
            "1*Connections.X 1",
            "+1*Connections.X1",
            "1*Connections.X1.X.X",
            "x*Connections.X1",
            "1*Connections.X01",
            "01*Connections.X1",
            "1*Connections..X",
            "1*Connections. .X",
            "1*Connections.a b.X",
            "11111111111*Connections.X1",
            "1*Connections.X11111111111",
        ] {
            assert_eq!(parse_term(text), None, "{text:?}");
        }
        // A number past u32 is a number that names no row, and a generation past u32 one no generation is: the term is void and not the marker.
        assert_eq!(parse_term("4294967296*Connections.X4294967296"), Some(RawTerm { generation: u32::MAX, row: RowName::Number(0) }));
        assert_eq!(parse_term("0*Connections.X0"), Some(RawTerm { generation: 0, row: RowName::Number(0) }));
    }

    #[test]
    fn a_name_names_the_row_that_spells_it_so_before_the_first_of_any_case() {
        // Rows by the number they are told by, in the section's order: three spellings of one name, one that is not, and a second row of the first spelling.
        let index = NameIndex::new([(3, "Tally"), (5, "TALLY"), (6, "tally"), (7, "Other"), (9, "Tally")]);
        assert_eq!(index.find("Tally"), Some(3), "the first of the spelling");
        assert_eq!(index.find("TALLY"), Some(5));
        assert_eq!(index.find("tally"), Some(6));
        assert_eq!(index.find("tAlLy"), Some(3), "no row spells it so: the first row of the name in any case");
        assert_eq!(index.find("OTHER"), Some(7));
        assert_eq!(index.find("Tall"), None);
        assert_eq!(index.find("Tallyx"), None);
        assert_eq!(index.find(""), None);
        assert_eq!(NameIndex::new([]).find("Tally"), None);
    }

    #[test]
    fn a_generation_is_found_by_the_spelling_of_the_term_and_not_in_any_case() {
        let marker = Marker::with_terms(vec![term(RowName::Name("Row_5".into()), 2), term(RowName::Number(3), 1)]).unwrap();
        assert_eq!(marker.generation_of(&RowName::Name("Row_5".into())), 2);
        for other in ["ROW_5", "row_5", "rOw_5", "Row_6", ""] {
            assert_eq!(marker.generation_of(&RowName::Name(other.into())), 0, "{other}");
        }
        assert_eq!((marker.generation_of(&RowName::Number(3)), marker.generation_of(&RowName::Number(4))), (1, 0));
        let first = Marker::with_terms(vec![term(RowName::Number(1), 4)]).unwrap();
        assert_eq!(first.generation_of(&RowName::Default), 4, "a reference with no row is row 1");
    }

    #[test]
    fn a_name_that_is_long_is_read_and_one_past_the_model_is_not() {
        let name = "n".repeat(MAX_NAME_BYTES);
        assert!(parse_term(&format!("1*Connections.{name}.X")).is_some());
        assert_eq!(parse_term(&format!("1*Connections.{name}n.X")), None);
    }

    #[test]
    fn the_formula_of_no_terms_or_too_many_is_none() {
        assert_eq!(parse_formula("0*()"), None);
        assert!(parse_formula("0*(1*Connections.X1)").is_some());
        let at = |n: usize| format!("0*({})", vec!["1*Connections.X1"; n].join("+"));
        assert_eq!(parse_formula(&at(256)).map(|t| t.len()), Some(256));
        assert_eq!(parse_formula(&at(257)), None);
        assert_eq!(parse_formula(&format!("0*({})", "1".repeat(MAX_MARKER_BYTES))), None);
    }
}
