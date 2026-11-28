//! Reading a reference: which sheet a qualifier names, and which cell a dotted path is.
//!
//! A qualifier sets the reference context of the one call or reference right after it: `Sheet.12!PinX`, `Door!Width`,
//! `Pages[Site-2]!Sheet.9!EndY`, `LISTSHEETREF()!Width`, `ThePage!User.Origin`, `TheDoc!User.Count`, `Styles!Callout!LineWeight`.
//! A path is a cell directly under the sheet, or a section, a row and a cell (`Scratch.B3`, `Char.Size[2]`, `Geometry2.X4`,
//! `Prop.Cost.Label`), with the row numbers. **A cell directly under the sheet that the catalogue lists is named in any letter case** (`WIDTH` reads
//! the cell `Width`: the project's test PC showed the reference application do so); a row's name matches in any case too.
//!
//! **A reference outside a formula**: [`cell_address`] reads the text of one reference with no sheet before it, by the formula parser and
//! the same mapping of its path, and [`reference_name`] is its inverse, so that a command names a cell as a formula does and gives back the name it was
//! given. Both are bounded ([`MAX_REFERENCE_BYTES`]), and a refusal is a [`RefError`] that says why.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use diacraft_doc::{
    CellAddr, Keying, Lookup, MAX_NAME_BYTES, Owner, RowKey, SECTION_SPELLINGS, SectionKey, ShapeRef, SheetRef, cell_info, direct_cell_spelling,
    ix_to_row_number, keying, listed_cells, row_number_to_ix, section_spelling,
};
use diacraft_formula::{CellRef, Expr, Index, ParseError, ParseErrorKind, Qualifier, Segment};

use crate::eval::{Env, Evaluator};
use crate::row_names::{RowNames, Spelled, fold};
use crate::rules::{KEY_BYTES, LOOP_ERROR, MISSING_ROW_ERROR, NAME_ENTRY_BYTES, UNITS_PER_STEP};
use crate::shapes::{ShapeIndex, peer_names};
use crate::value::{CellError, Value};

/// The cell a named row reads when the reference gives none (the table).
fn default_cell(section: &str) -> &'static str {
    match section {
        "Actions" => "Menu",
        "Control" | "ActionTag" => "X",
        "Hyperlink" => "Description",
        _ => "Value",
    }
}

/// A trailing number split off a name: `B3` is `B` and 3, `X` has none. A name that is all digits has no name part. Digits that do not fit a `u32` are
/// an error, never a name with its digits left off: `A4294967296` was read as `A`, row 1, and a write to it would have landed on the wrong cell.
fn split_digits(name: &str) -> Result<(&str, Option<u32>), RefError> {
    let cut = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    match (name.get(..cut), name.get(cut..)) {
        (Some(head), Some(tail)) if cut > 0 && !tail.is_empty() => Ok((head, Some(tail.parse().map_err(|_| RefError::NumberTooLarge)?))),
        _ => Ok((name, None)),
    }
}

/// The section and its `IX`: `Char` is `Character`, `Geometry2` is `Geometry` with `IX` 1 (the section's own number is `IX` plus
/// one). A name the table does not know is an extension section, which a reference may also name.
fn section_of(name: &str) -> Result<(String, Option<u32>), RefError> {
    if let Some(number) = name.strip_prefix("Geometry") {
        let n: u32 = match number.parse() {
            Ok(n) => n,
            Err(_) if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => return Err(RefError::NumberTooLarge),
            Err(_) => return Err(RefError::GeometryNumber),
        };
        return Ok(("Geometry".to_owned(), Some(n.checked_sub(1).ok_or(RefError::GeometryNumber)?)));
    }
    match SECTION_SPELLINGS.iter().find(|(spelled, _)| *spelled == name) {
        Some((_, n)) => Ok(((*n).to_owned(), None)),
        None => Ok((name.to_owned(), None)),
    }
}

/// The cell a dotted path names, or `None` for a form no row of the table has.
pub(crate) fn address_of(path: &[Segment]) -> Option<CellAddr> {
    read_path(path).ok()
}

/// The cell a dotted path names, or why the path names none: the words of [`cell_address`].
fn read_path(path: &[Segment]) -> Result<CellAddr, RefError> {
    match path {
        [] => Err(RefError::NotOneReference),
        [one] => {
            if one.index.is_some() {
                return Err(RefError::CellTakesNoRow);
            }
            // A cell the catalogue lists is named in any letter case: `WIDTH` is `Width` (observed in the reference application). Any other name is as spelled. A name the model
            // knows (the common case) is not looked for again: only one it holds an allocation of can be a listed cell in another case.
            let addr = CellAddr::direct(&one.name);
            match addr.cell.is_allocated().then(|| direct_cell_spelling(&one.name)).flatten() {
                Some(listed) => Ok(CellAddr::direct(listed)),
                None => Ok(addr),
            }
        }
        [section, rest @ ..] => {
            if section.index.is_some() {
                return Err(RefError::SectionTakesNoRow);
            }
            let (section_name, section_ix) = section_of(&section.name)?;
            let is_geometry = section_name == "Geometry";
            match rest {
                [one] => {
                    let indexed = match keying(&section_name) {
                        Keying::Indexed => true,
                        Keying::Named => false,
                        // `Connection` and any section no spec lists: a number after the name makes it a row's place. Digits too long to be a number
                        // are part of a name here, as a row's name may end in them.
                        Keying::Either => one.index.is_some() || matches!(split_digits(&one.name), Ok((_, Some(_)))),
                    };
                    if indexed {
                        let (name, number) = match &one.index {
                            Some(Index::Number(n)) => (one.name.as_ref(), Some(*n)),
                            Some(Index::Name(_)) => return Err(RefError::RowNotANumber),
                            None => split_digits(&one.name)?,
                        };
                        match (number, is_geometry) {
                            // `Geometry1.NoFill`: a cell of the section itself.
                            (None, true) => Ok(CellAddr::in_section(&section_name, section_ix, name)),
                            // A missing row number is row 1.
                            (None, false) => Ok(CellAddr::in_row(&section_name, section_ix, RowKey::Ix(0), name)),
                            (Some(n), _) => Ok(CellAddr::in_row(
                                &section_name,
                                section_ix,
                                RowKey::Ix(row_number_to_ix(is_geometry, n).ok_or(RefError::RowZero)?),
                                name,
                            )),
                        }
                    } else {
                        if one.index.is_some() {
                            return Err(RefError::NamedRowTakesNoRow);
                        }
                        Ok(CellAddr::in_row(&section_name, section_ix, RowKey::name(&one.name), default_cell(&section_name)))
                    }
                }
                [row, cell] => {
                    if row.index.is_some() || cell.index.is_some() {
                        return Err(RefError::NamedRowTakesNoRow);
                    }
                    Ok(CellAddr {
                        section: Some(SectionKey::new(&section_name, section_ix)),
                        row: Some(RowKey::name(&row.name)),
                        cell: diacraft_doc::CellName::new(&cell.name),
                    })
                }
                _ => Err(RefError::TooManyParts),
            }
        }
    }
}

/// The longest text [`cell_address`] reads, in bytes: three names of the model's longest (1,024 bytes), each in single quotes with every character a
/// quote that is doubled, and their dots, make 6,152, so no honest reference is refused. A project rule that bounds what a hostile text costs to parse.
pub const MAX_REFERENCE_BYTES: usize = 8_192;

/// The most nodes of the tree [`cell_address`] lets the parser build: a reference is a few, one name a segment.
const MAX_REFERENCE_NODES: usize = 1_024;

/// Why a text is not the reference of one cell, in words a person can act on (the `Display` of each).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefError {
    /// Nothing but white space.
    Empty,
    /// More than [`MAX_REFERENCE_BYTES`].
    TooLong,
    /// The text begins with `=`: a formula, not the name of a cell.
    LeadingEquals,
    /// The text does not parse as a formula's reference: what went wrong and where (the byte offset is into the text given).
    Parse(ParseError),
    /// The text parses and is no reference: a number, an operator, a call, `TRUE`.
    NotOneReference,
    /// A sheet is named before the reference (`Sheet.4!Width`); the sheet is named by the call's own parameters.
    Qualified,
    /// `Width[2]`: a cell directly under the sheet has no row.
    CellTakesNoRow,
    /// `Char[2].Size`: the row goes after the cell.
    SectionTakesNoRow,
    /// `Geometry.X1`, `Geometry0.X1`: a Geometry section is named with its number from 1.
    GeometryNumber,
    /// A number that does not fit 4,294,967,295.
    NumberTooLarge,
    /// `Scratch.A[x]`: the row of an indexed section is a whole number.
    RowNotANumber,
    /// `Scratch.A0`: rows are numbered from 1.
    RowZero,
    /// `Prop.Cost[2]`: a named row takes no `[index]`.
    NamedRowTakesNoRow,
    /// More than a section, a row and a cell.
    TooManyParts,
}

impl std::fmt::Display for RefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefError::Empty => f.write_str("name the cell by its reference, such as Width or Geometry1.X2"),
            RefError::TooLong => write!(f, "a cell reference is at most {MAX_REFERENCE_BYTES} bytes long"),
            RefError::LeadingEquals => f.write_str("a cell is named by its reference without a leading =: write Width, not =Width"),
            RefError::Parse(e) => write!(f, "not a cell reference: {e}"),
            RefError::NotOneReference => f.write_str("not one cell reference: give a reference such as Width, Geometry1.X2 or Prop.Cost.Label"),
            RefError::Qualified => f.write_str("name the sheet with id, page, master, style or document, not in the reference"),
            RefError::CellTakesNoRow => f.write_str("a cell directly under the sheet takes no [row]"),
            RefError::SectionTakesNoRow => f.write_str("a section's name takes no [row]: the row goes after the cell, as in Char.Size[2]"),
            RefError::GeometryNumber => f.write_str("a Geometry section is named with its number, from 1: Geometry1"),
            RefError::NumberTooLarge => write!(f, "a number in the reference is larger than {}", u32::MAX),
            RefError::RowNotANumber => f.write_str("a row of an indexed section is a whole number: Scratch.A3 or Char.Size[2]"),
            RefError::RowZero => f.write_str("row numbers start at 1"),
            RefError::NamedRowTakesNoRow => f.write_str("a named row takes no [index]: Prop.Cost.Label"),
            RefError::TooManyParts => f.write_str("a reference has at most a section, a row and a cell: Prop.Cost.Label"),
        }
    }
}

impl std::error::Error for RefError {}

/// The cell a text names: **one reference with no sheet before it**, as a formula spells it (`Width`, `Geometry1.X2`, `Scratch.B3`, `Char.Size[2]`,
/// `Prop.Cost`, `Prop.Cost.Label`, `User.Total.Prompt`). The text is read by the formula parser and its path
/// is mapped as a formula's reference is, so a cell is named in a command as in a formula. White space at the ends is allowed; a leading `=` is not,
/// since the text names a cell and is no formula. A reference with a sheet (`Sheet.4!Width`) is refused: the sheet is named by the call.
///
/// The row of a named section is as the text spells it. A formula that names a row in another letter case finds the row the file spells, which
/// needs a document, and a text read here has none. At most [`MAX_REFERENCE_BYTES`] are read, and nothing is evaluated.
pub fn cell_address(text: &str) -> Result<CellAddr, RefError> {
    if text.len() > MAX_REFERENCE_BYTES {
        return Err(RefError::TooLong);
    }
    if text.trim_start().starts_with('=') {
        return Err(RefError::LeadingEquals);
    }
    let formula = diacraft_formula::parse_with_budget(text, MAX_REFERENCE_NODES);
    match formula.ast {
        Ok(Expr::Ref(r)) if r.qualifiers.is_empty() => read_path(&r.path),
        Ok(Expr::Ref(_)) => Err(RefError::Qualified),
        Ok(_) => Err(RefError::NotOneReference),
        Err(e) if e.kind == ParseErrorKind::Empty => Err(RefError::Empty),
        Err(e) => Err(RefError::Parse(e)),
    }
}

/// The reference a cell is spelled with, as a formula names it and [`cell_address`] reads it: the inverse of `cell_address`. `None` where no reference
/// form exists: a name that is empty or longer than the model's 1,024 bytes, an unnamed row of a section whose rows are named, a Geometry section with
/// no number, a flag of a section that is not Geometry, a number a reference cannot hold, and a section another spelling takes (`Prop`, which a
/// reference reads as `Property`).
///
/// **The text is checked by reading it back**: it is given only when `cell_address` of it is exactly this cell, so a reference is never given for
/// another cell. A name that is no plain name is in single quotes (`'My cell'`, a quote in it doubled), a row of an indexed section is spelled with its
/// number (the row's `IX` plus one, a Geometry row's `IX`) as `Scratch.B3` for a cell of one letter and `Char.Size[2]` for any other, a row of a
/// named section with its name and cell (`Prop.Cost.Value`, the cell always given), and a Geometry section's number is its `IX` plus one.
pub fn reference_name(addr: &CellAddr) -> Option<String> {
    let text = spell(addr)?;
    (cell_address(&text).ok().as_ref() == Some(addr)).then_some(text)
}

/// The text `reference_name` tries, before it reads it back.
fn spell(addr: &CellAddr) -> Option<String> {
    let cell = addr.cell.as_str();
    let Some(section) = &addr.section else {
        return match addr.row {
            None => name_text(cell, true),
            Some(_) => None,
        };
    };
    let spelled = section_text(section)?;
    match &addr.row {
        // A flag of a Geometry section: `Geometry1.NoFill`.
        None if section.name.as_str() == "Geometry" => Some(format!("{spelled}.{}", name_text(cell, false)?)),
        None => None,
        Some(RowKey::Name(row)) => Some(format!("{spelled}.{}.{}", name_text(row, false)?, name_text(cell, false)?)),
        Some(RowKey::Ix(n)) => {
            let number = ix_to_row_number(section.name.as_str() == "Geometry", *n)?;
            let cell_part = if is_one_letter(cell) { format!("{cell}{number}") } else { format!("{}[{number}]", name_text(cell, false)?) };
            Some(format!("{spelled}.{cell_part}"))
        }
    }
}

/// How a reference spells a section: `Geometry3` for the one of `IX` 2, `Char` for `Character`, the name itself for a section no table lists.
fn section_text(key: &SectionKey) -> Option<String> {
    let name = key.name.as_str();
    if name == "Geometry" {
        return Some(format!("Geometry{}", key.ix?.checked_add(1)?));
    }
    if key.ix.is_some() {
        return None;
    }
    match section_spelling(name) {
        Some(spelled) => Some(spelled.to_owned()),
        None => name_text(name, true),
    }
}

/// Whether a cell's name is one ASCII letter, which a reference writes with the row's number after it (`B3`).
fn is_one_letter(cell: &str) -> bool {
    let mut chars = cell.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_alphabetic())
}

/// A name as a reference writes it: as it is when it is plain (letters, digits and underscores, not starting with a digit where it is the first of a
/// path, and no `TRUE` or `FALSE` there), else in single quotes with a quote in it doubled. `None` for a name the parser would refuse: empty, or more
/// than the model's [`MAX_NAME_BYTES`].
fn name_text(name: &str, first: bool) -> Option<String> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return None;
    }
    let starts_well = !first || name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
    let plain = starts_well && name.chars().all(|c| c.is_alphanumeric() || c == '_') && !(first && is_boolean_word(name));
    Some(if plain { name.to_owned() } else { format!("'{}'", name.replace('\'', "''")) })
}

/// `TRUE` or `FALSE` in any case, which the parser reads as a Boolean when it stands alone and is not quoted.
fn is_boolean_word(name: &str) -> bool {
    name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE")
}

/// What a qualifier's path has chosen so far besides the sheet: the part whose shapes, or the styles, a following `Sheet.<id>` or
/// name looks in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Part(Owner),
    Styles,
}

fn owner_of(sheet: &SheetRef) -> Option<Owner> {
    match sheet {
        SheetRef::Shape(s) => Some(s.owner),
        SheetRef::Page(p) => Some(Owner::Page(*p)),
        SheetRef::Master(m) => Some(Owner::Master(*m)),
        SheetRef::Document | SheetRef::Style(_) => None,
    }
}

fn sheet_of_owner(owner: Owner) -> SheetRef {
    match owner {
        Owner::Page(p) => SheetRef::Page(p),
        Owner::Master(m) => SheetRef::Master(m),
    }
}

fn is_word(segment: &Segment, word: &str) -> bool {
    segment.index.is_none() && segment.name.eq_ignore_ascii_case(word)
}

impl Evaluator<'_, '_> {
    /// The context a call or reference is read in once its qualifiers are applied: `None` when there are none and it is the
    /// formula's own. An error is the value to give: `#REF!` for a sheet that is not there, or the error a sheet-returning call gave.
    pub(crate) fn qualified(&mut self, qualifiers: &[Qualifier], env: &Env) -> Result<Option<Env>, Value> {
        if qualifiers.is_empty() {
            return Ok(None);
        }
        let mut here: Option<SheetRef> = Some(env.sheet.clone());
        let mut container: Option<Container> = None;
        let mut first = true;
        for q in qualifiers {
            if !self.steps(1) {
                return Err(Value::Error(LOOP_ERROR));
            }
            match q {
                Qualifier::Call { name, args } => {
                    // The arguments are copied to make the call, whether or not they are worked out: a node each.
                    if !self.steps(args.iter().flatten().map(diacraft_formula::Expr::node_count).sum::<usize>()) {
                        return Err(Value::Error(LOOP_ERROR));
                    }
                    let call = diacraft_formula::Call { qualifiers: Vec::new(), name: Arc::clone(name), args: args.clone() };
                    let context = match &here {
                        Some(s) => env.on(s.clone()),
                        None => return Err(Value::Error(CellError::Ref)),
                    };
                    let v = self.eval_call_in(&call, env, &context);
                    match v {
                        Value::Sheet(s) => {
                            here = Some(*s);
                            container = None;
                        }
                        Value::Error(_) | Value::Unsupported(_) => return Err(v),
                        // A call that is not a sheet cannot qualify.
                        _ => return Err(Value::Error(CellError::Value)),
                    }
                }
                Qualifier::Path(segments) => {
                    let rebind_ok = first;
                    match self.qualifier_path(segments, env, &here, container, rebind_ok) {
                        Some((sheet, next)) => {
                            here = sheet;
                            container = next;
                        }
                        None => return Err(Value::Error(CellError::Ref)),
                    }
                }
            }
            first = false;
        }
        match here {
            Some(sheet) => Ok(Some(env.on(sheet))),
            None => Err(Value::Error(CellError::Ref)),
        }
    }

    /// One qualifier that is a path: the sheet it names (`None` while a container such as `Styles` waits for its member) and the
    /// container it leaves for the next.
    fn qualifier_path(
        &mut self,
        segments: &[Segment],
        env: &Env,
        here: &Option<SheetRef>,
        container: Option<Container>,
        use_rebind: bool,
    ) -> Option<(Option<SheetRef>, Option<Container>)> {
        match segments {
            [one] if is_word(one, "ThePage") => {
                let owner = owner_of(here.as_ref()?)?;
                Some((Some(sheet_of_owner(owner)), Some(Container::Part(owner))))
            }
            [one] if is_word(one, "TheDoc") => Some((Some(SheetRef::Document), None)),
            [one] if one.name.eq_ignore_ascii_case("Pages") && one.index.is_some() => {
                let id = self.page_by_index(one.index.as_ref()?)?;
                Some((Some(SheetRef::Page(id)), Some(Container::Part(Owner::Page(id)))))
            }
            [one] if one.name.eq_ignore_ascii_case("Masters") && one.index.is_some() => {
                let id = self.master_by_index(one.index.as_ref()?)?;
                Some((Some(SheetRef::Master(id)), Some(Container::Part(Owner::Master(id)))))
            }
            [one] if is_word(one, "Styles") => Some((None, Some(Container::Styles))),
            [word, id] if is_word(word, "Sheet") && word.index.is_none() && id.index.is_none() => {
                let n: u32 = id.name.parse().ok()?;
                self.sheet_by_id(n, env, here, container, use_rebind)
            }
            [name] if name.index.is_none() => self.sheet_by_name(&name.name, None, env, here, container),
            [name, id] if name.index.is_none() && id.index.is_none() => {
                let n: u32 = id.name.parse().ok()?;
                self.sheet_by_name(&name.name, Some(n), env, here, container)
            }
            _ => None,
        }
    }

    /// `Sheet.<id>`: a shape of the part the reference is in (by the instance's own copy where the formula was found on
    /// a master), a shape of the page or master the container chose, or a style.
    fn sheet_by_id(
        &mut self,
        id: u32,
        env: &Env,
        here: &Option<SheetRef>,
        container: Option<Container>,
        use_rebind: bool,
    ) -> Option<(Option<SheetRef>, Option<Container>)> {
        match container {
            Some(Container::Styles) => {
                self.res.style(id)?;
                Some((Some(SheetRef::Style(id)), None))
            }
            Some(Container::Part(owner)) => {
                let shape = self.shape_by_id(owner, id)?;
                Some((Some(SheetRef::Shape(shape)), Some(Container::Part(owner))))
            }
            None => {
                if use_rebind && let Some(rebind) = &env.rebind {
                    let shape = self.res.rebind(rebind, id)?;
                    let owner = shape.owner;
                    return Some((Some(SheetRef::Shape(shape)), Some(Container::Part(owner))));
                }
                let owner = owner_of(here.as_ref()?)?;
                let shape = self.shape_by_id(owner, id)?;
                Some((Some(SheetRef::Shape(shape)), Some(Container::Part(owner))))
            }
        }
    }

    /// A name: a peer of the shape the reference is in, a shape of the chosen page or master by `NameU`, or a style.
    fn sheet_by_name(
        &mut self,
        name: &str,
        id: Option<u32>,
        env: &Env,
        here: &Option<SheetRef>,
        container: Option<Container>,
    ) -> Option<(Option<SheetRef>, Option<Container>)> {
        let _ = env;
        match container {
            Some(Container::Styles) => {
                let style = self.style_named(name)?;
                Some((Some(SheetRef::Style(style)), None))
            }
            Some(Container::Part(owner)) => {
                // The top level of a part, by name: `Pages[Site-2]!Door!Width`.
                let place = self.peer_place(owner, &[], name, id)?;
                Some((Some(SheetRef::Shape(ShapeRef::top(owner, place))), Some(Container::Part(owner))))
            }
            None => {
                // A peer: a shape of the same level as the one the reference is in.
                let SheetRef::Shape(shape) = here.as_ref()? else { return None };
                let parent_path: Vec<u32> = shape.path.split_last().map(|(_, up)| up.to_vec()).unwrap_or_default();
                let place = self.peer_place(shape.owner, &parent_path, name, id)?;
                let mut path = parent_path;
                path.push(place);
                Some((Some(SheetRef::Shape(ShapeRef { owner: shape.owner, path })), Some(Container::Part(shape.owner))))
            }
        }
    }

    /// The place, in the level at `parent` (empty for the top level of the part), of the shape called `name` whose `ID` is `id`, or the first
    /// of that name when there is no `ID`. The level's names are listed once, a step for each shape, and then found in one lookup.
    fn peer_place(&mut self, owner: Owner, parent: &[u32], name: &str, id: Option<u32>) -> Option<u32> {
        let key = (owner, parent.to_vec());
        if !self.peers.contains_key(&key) {
            let level =
                if parent.is_empty() { self.res.shapes(owner)? } else { &self.res.shape(&ShapeRef { owner, path: parent.to_vec() })?.children };
            let ctx = &mut *self.ctx;
            let names = peer_names(level, &mut |n| ctx.take_steps(n as u64) && ctx.hold_bytes((n as u64).saturating_mul(NAME_ENTRY_BYTES)));
            self.peers.insert(key.clone(), names);
        }
        let level = if parent.is_empty() { self.res.shapes(owner)? } else { &self.res.shape(&ShapeRef { owner, path: parent.to_vec() })?.children };
        self.peers.get(&key)?.get(name)?.pick(level, id)
    }

    /// The shape with this `ID` in a part: the first in document order, found through the part's index (`Document::find_shape` walks the
    /// whole part for each call).
    pub(crate) fn shape_by_id(&mut self, owner: Owner, id: u32) -> Option<ShapeRef> {
        if !self.owners.contains_key(&owner) {
            let list = self.res.shapes(owner)?;
            let ctx = &mut *self.ctx;
            let index = ShapeIndex::build(list, &mut |n| ctx.take_steps(n as u64) && ctx.hold_bytes((n as u64).saturating_mul(NAME_ENTRY_BYTES)));
            self.owners.insert(owner, index);
        }
        let path = self.owners.get(&owner)?.path(id)?;
        Some(ShapeRef { owner, path })
    }

    fn page_by_index(&mut self, index: &Index) -> Option<u32> {
        match index {
            Index::Name(name) => self.page_named(name, true),
            Index::Number(n) => self.page_named(&n.to_string(), false),
        }
    }

    fn master_by_index(&mut self, index: &Index) -> Option<u32> {
        match index {
            Index::Name(name) => self.master_named(name),
            Index::Number(n) => self.master_named(&n.to_string()),
        }
    }

    /// The cell a reference names, and the sheet it is on.
    pub(crate) fn locate(&mut self, r: &CellRef, env: &Env) -> Result<(SheetRef, CellAddr), Value> {
        let target = self.qualified(&r.qualifiers, env)?;
        let sheet = match &target {
            Some(t) => t.sheet.clone(),
            None => env.sheet.clone(),
        };
        let Some(addr) = address_of(&r.path) else { return Err(Value::Error(CellError::Ref)) };
        let addr = self.respell_row(&sheet, addr);
        Ok((sheet, addr))
    }

    /// A row named in another letter case is the row the file spells: the exact spelling first, then without regard to
    /// case, then the lower `IX`. The resolver's lookup is exact and is the common case, so the sheet itself is asked first; only a name it does not
    /// find is looked up in the index of the section's rows (`row_names.rs`), which is listed once for a sheet and a section.
    fn respell_row(&mut self, sheet: &SheetRef, mut addr: CellAddr) -> CellAddr {
        let (Some(section), Some(RowKey::Name(name))) = (&addr.section, &addr.row) else { return addr };
        if keying(section.name.as_str()) == Keying::Indexed {
            return addr;
        }
        let held = self.res.sheet(sheet).is_some_and(|here| matches!(self.res.lookup(here, &addr), Lookup::Found(_) | Lookup::Deleted));
        if held {
            return addr;
        }
        let (section, name) = (section.clone(), Arc::clone(name));
        // The name is folded: a pass over it, charged by its length.
        if !self.work(name.len()) {
            return addr;
        }
        let folded = fold(&name);
        if let Some(names) = self.row_names_of(sheet, &section)
            && let Spelled::As(spelling) = names.spell(&name, &folded)
        {
            addr.row = Some(RowKey::Name(spelling));
        }
        addr
    }

    /// The names of the rows of `section` as `sheet` sees them, listed on first use and kept. Each row is a step and its name's bytes are charged,
    /// as the listing is built.
    fn row_names_of(&mut self, sheet: &SheetRef, section: &SectionKey) -> Option<Rc<RowNames>> {
        let key = (sheet.clone(), section.clone());
        if let Some(known) = self.row_names.get(&key) {
            return Some(Rc::clone(known));
        }
        let layers = self.res.section_layers(sheet, section)?;
        let ctx = &mut *self.ctx;
        let names = RowNames::build(&layers, &mut |bytes| {
            ctx.take_steps((bytes / UNITS_PER_STEP) as u64 + 1) && ctx.hold_bytes((bytes as u64).saturating_mul(2).saturating_add(NAME_ENTRY_BYTES))
        });
        let names = Rc::new(names);
        if self.ctx.hold_bytes(KEY_BYTES) {
            self.row_names.insert(key, Rc::clone(&names));
        }
        Some(names)
    }

    /// Notes that the formula being worked out named a cell its section can never hold and found nothing.
    pub(crate) fn note_unholdable(&mut self, addr: &CellAddr) {
        if never_holds(addr) {
            self.ctx.must_fail_touched = true;
        }
    }

    /// `Expr::Ref`: the value of the cell it names, read as the operand mode says. A row nothing supplies is [`MISSING_ROW_ERROR`].
    pub(crate) fn eval_ref(&mut self, r: &CellRef, env: &Env) -> Value {
        let (sheet, addr) = match self.locate(r, env) {
            Ok(x) => x,
            Err(v) => return v,
        };
        let read = self.read_cell(&sheet, &addr);
        // In a string read as a formula a name that finds nothing makes the string unreadable: the
        // caller reads `#REF!` from it as "not a formula".
        if !read.found && (addr.row.is_some() || env.string_depth > 0) {
            self.note_unholdable(&addr);
            return Value::Error(MISSING_ROW_ERROR);
        }
        read.view.clone()
    }
}

/// The sections the catalogue lists the cells of: its batches are catalogued in full (names the ones
/// that are not, and `Reviewer` is not listed yet; `User`, `Property` and `Hyperlink` are catalogued so `Prop.<name>.Ask`,
/// a cell no `Property` row holds, is one of these), so the cells it lists for one are the cells MS-VSDX gives its rows.
fn catalogued_sections() -> &'static HashSet<&'static str> {
    static SECTIONS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SECTIONS.get_or_init(|| listed_cells().filter_map(|(section, _, _)| section).collect())
}

/// Whether the cell of a row is one its section can never hold, by the list of that section's cells (a `Tabs`
/// reference to cell `Q`, where a `Tabs` row holds only `Position` and `Alignment` pairs). Only a section the catalogue lists in full is judged.
/// Two forms of a `Tabs` reference name no cell by a letter and are left alone: `Tabs.12`, a row and a stop run together, and `Tabs.X1`,
/// which reads as the cell `X` of row 1 where the cell `X1` is the one the catalogue lists.
fn never_holds(addr: &CellAddr) -> bool {
    let (Some(section), Some(_)) = (&addr.section, &addr.row) else { return false };
    let (section, cell) = (section.name.as_str(), addr.cell.as_str());
    if !catalogued_sections().contains(section) {
        return false;
    }
    if section == "Tabs" && (cell == "X" || cell.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    !cell_info(Some(section), cell).listed
}
