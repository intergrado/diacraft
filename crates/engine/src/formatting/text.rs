//! `text.format`: the Character and Paragraph rows of a shape's text and the text block's `VerticalAlign`.
//!
//! **What each parameter writes**, in a row of the shape's own `Character` or `Paragraph` section (a local row holds the
//! cells it names, and the rest of the row is the master's and the styles'):
//!
//! | Parameter | Cell |
//! |---|---|
//! | `font` | `Character` `Font`: the face's `NameU`; a face the document's font table lacks is added to it |
//! | `size` | `Size`, the points over 72 in inches on paper |
//! | `bold`, `italic`, `underline` | bits 1, 2 and 4 of `Style`, one at a time, the others kept |
//! | `strike` | `Strikethru` 1 or 0 |
//! | `color` | `Color` as `RGB(r,g,b)` with its hex; a theme colour as `THEMEGUARD(THEMEVAL("AccentColor2"))` with the colour it works out to; default as the call `THEMEVAL()` |
//! | `align` | `Paragraph` `HorzAlign`: left 0, center 1, right 2, justify 3, distributed 4 |
//! | `valign` | the shape's `VerticalAlign`: top 0, middle 1, bottom 2 |
//! | `bullet` | `Paragraph` `Bullet`: 0 none, 1 to 7 the marks of the table; a paragraph with no indent is given the hanging form a bulleted one has in files |
//! | `indent` | `Paragraph` `IndLeft`: to the next multiple of a quarter inch up or down, from 0 to 100 inches |
//! | `case` | `Character` `Case`: normal 0, all capitals 1, initial capitals 2, and small capitals as bit 8 of `Style`; the text is not rewritten |
//! | `direction` | `Paragraph` `Flags`: left to right 0, right to left 1 |
//! | `vertical` | the shape's `TextDirection`: across 0, down 1 |
//!
//! Our rule, from the files' own form: a bullet put on a paragraph whose `IndLeft` and `IndFirst` are both 0 also
//! writes `IndLeft` 0.25 and `IndFirst` -0.25 inch, so that the bullet stands at the margin and every line of the text at a quarter inch, as the bulleted
//! paragraphs of files do; a bullet taken off a paragraph that has exactly that form takes it away again; a paragraph with any other indent keeps it.
//!
//! Our rule: no source gives the step: Increase Indent moves `IndLeft` to the next multiple of 0.25 inch above it,
//! Decrease Indent to the next below it, no lower than 0; an indent of 100 inches or more is not increased. Each paragraph moves from its own.
//!
//! Change Case is the dialog's Case box: one of normal, all capitals, initial capitals and small capitals, so
//! choosing one clears the others (`Case` and bit 8 of `Style`). It changes how the letters show and never the stored text.
//!
//! **Whole text or a range.** Without a range, every row the text names (every `cp` for the Character cells, every `pp` for the Paragraph cells),
//! or row 0 when it names none or the shape has no text, is changed: a shape with no text is formatted for the text typed in it next. With a range
//! `start..end` of the shape's characters (counted as `shape.set_text` counts them), only those characters change, and the runs and paragraphs
//! they start or end in are split to give them rows of their own (`runs.rs`). A range holds characters only: a point formats
//! no run, and a paragraph's `align` takes the paragraph the point is in.
//!
//! **A split writes the rows** (see `runs.rs` for the two cases of numbering): every row the text names is a local row of the shape
//! afterwards, in text order when nothing a master or a style holds could be mistaken for it. A row that stays where it is, is not in the range
//! and is local already is left as it is; a row in the range that stays is changed in place; every other row (new, moved, or one a style supplies)
//! is written whole, as a copy of the cells its run's row resolves to with the change. **A row so written past the first holds every cell of
//! ** (`diacraft_doc::root_text::row_cells`): a cell the copy lacks is given the value the renderer assumes, since a reader of
//! the file takes nothing for a row of `IX` 1 or more from the styles.
//!
//! Our rule: a face is looked up in the font table by its name, exactly and then in any case, and the table's spelling is what `Font`
//! holds; a face not in it is added with its name alone (`NameU` is the only attribute MS-VSDX requires). `AsianFont` and
//! `ComplexScriptFont` are left alone: a letter of those scripts reads its own cell.
//!
//! Our rule: a bit that is already as asked, in a `Style` or `Strikethru` that holds a plain number, is not written. A `Style` that is
//! the theme's marker (`THEMEVAL()`) and that the theme cannot answer reads as 0 for the bits kept, so that bold on it writes 1.
//!
//! **Cost.** A text of many runs is formatted in time that grows with the runs and not with their square: the rows of a section are found once for a
//! command and held by `IX`, the cells planned for a shape are found by address, and the rows a split names are held in sets.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, Document, EffectiveRow, FaceName, FormulaSlot, Reading, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Text,
    TextPart, Value,
};

use super::look::{Align, Case, Direction, Indent, Reader, SMALL_CAPS_BIT, VAlign, Val, eval_ctx, row_addr, text_of};
use super::quick::plan_scheme;
use super::runs::{Alloc, Kind, MAX_NAMED_ROWS, number_rows_in_text_order, paragraph_bounds, plan as split, rows_named, too_many_rows};
use super::shape_cells::{Paint, parse_paint};
use super::{Edits, MAX_FACE_NAMES, New, Rgb, holds_row, targets};
use crate::draw::{Problem, require_shape, shape_mut, shown_parts};
use crate::theme::{ThemePick, follow_page_theme_refs, pick_colour, selection_of};

/// The most a size may be, in points.
pub const MAX_SIZE_POINTS: f64 = 1_000.0;

/// The highest `Bullet` there is a mark for.
pub const MAX_BULLET: u8 = 7;

/// The step of Decrease Indent and Increase Indent, in paper inches.
pub(crate) const INDENT_STEP: f64 = 0.25;
/// The most an indent step takes `IndLeft` to, in paper inches.
pub(crate) const MAX_INDENT: f64 = 100.0;
/// The indents of a bulleted paragraph in files, in paper inches: the text at a quarter inch, the first line back at the margin.
const HANG: f64 = 0.25;

/// The colour of text: a colour, a colour of the page's theme (which follows the theme), or the theme's own colour for text (Use Default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPaint {
    Colour(Rgb),
    Theme(ThemePick),
    Default,
}

impl TextPaint {
    /// The paint a command's `color` names: `#RRGGBB`, `theme:accent2` (with a tint, `theme:accent2:40`) or `default`; there is no `none` for text.
    pub fn parse(text: &str) -> Result<TextPaint, Problem> {
        match parse_paint(text, "color", false)? {
            Paint::Colour(rgb) => Ok(TextPaint::Colour(rgb)),
            Paint::Theme(pick) => Ok(TextPaint::Theme(pick)),
            Paint::Default => Ok(TextPaint::Default),
            Paint::None => Err(Problem::bad("`color` takes no `none`: a text has a colour")),
        }
    }

    fn is_themed(self) -> bool {
        !matches!(self, TextPaint::Colour(_))
    }
}

/// What `text.format` sets; a field left `None` is left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextFormat {
    pub font: Option<String>,
    /// Points.
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<TextPaint>,
    pub align: Option<Align>,
    pub valign: Option<VAlign>,
    /// The bullet of the paragraphs: 0 for none, 1 to 7 the marks of `Bullet`.
    pub bullet: Option<u8>,
    /// A step of Decrease Indent or Increase Indent on the paragraphs.
    pub indent: Option<Indent>,
    /// How the letters are shown: Change Case.
    pub case: Option<Case>,
    /// The reading direction of the paragraphs (`Flags`).
    pub direction: Option<Direction>,
    /// Whether the text runs down the block (`TextDirection`).
    pub vertical: Option<bool>,
}

impl TextFormat {
    /// Whether it sets a cell of a `Character` row.
    fn character(&self) -> bool {
        self.case.is_some()
            || self.font.is_some()
            || self.size.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strike.is_some()
            || self.color.is_some()
    }

    /// Whether it sets a cell of a `Paragraph` row.
    fn paragraph(&self) -> bool {
        self.align.is_some() || self.bullet.is_some() || self.indent.is_some() || self.direction.is_some()
    }

    /// Whether it sets nothing.
    pub fn is_empty(&self) -> bool {
        !self.character() && !self.paragraph() && self.valign.is_none() && self.vertical.is_none()
    }

    /// The face's name as it will be written: trimmed, and one a font table can hold.
    fn face(&self) -> Result<Option<String>, Problem> {
        let Some(name) = self.font.as_deref() else { return Ok(None) };
        let name = name.trim();
        diacraft_doc::check_text(name).map_err(|problem| Problem::bad(format!("`font` {problem}")))?;
        if name.chars().any(|c| matches!(c, '\u{fffe}' | '\u{ffff}')) {
            return Err(Problem::bad("`font` must not contain U+FFFE or U+FFFF"));
        }
        Ok(Some(name.to_owned()))
    }

    fn check(&self) -> Result<(), Problem> {
        if let Some(size) = self.size
            && !(size.is_finite() && size > 0.0 && size <= MAX_SIZE_POINTS)
        {
            return Err(Problem::bad(format!("`size` must be more than 0 and at most {MAX_SIZE_POINTS} points")));
        }
        if let Some(bullet) = self.bullet
            && bullet > MAX_BULLET
        {
            return Err(Problem::bad(format!("`bullet` must be from 0 to {MAX_BULLET}: 0 is none and 1 to {MAX_BULLET} are the marks of the table")));
        }
        self.face().map(|_| ())
    }
}

/// Sets what `format` names in the text of the shapes `ids` of a page (the shapes inside a group, for a group). `range` is `start`
/// and `end`, in characters of the one shape named. → how many shapes were written to; the document is not touched when that is none.
pub fn format_text(doc: &mut Document, page: u32, ids: &[u32], range: Option<(usize, usize)>, format: &TextFormat) -> Result<usize, Problem> {
    format.check()?;
    if format.is_empty() {
        return Ok(0);
    }
    let shapes = match range {
        Some(_) => vec![single(doc, page, ids)?],
        None => targets(doc, page, ids)?,
    };
    // The face the cells will name, and whether the table lacks it.
    let face = format.face()?.map(|wanted| face_in_table(doc, &wanted));
    if let Some((_, true)) = &face
        && doc.fonts.len() >= MAX_FACE_NAMES
    {
        return Err(Problem::failed(format!("the font table holds {MAX_FACE_NAMES} faces already")));
    }
    // A theme colour or default is answered by the page's theme: the shapes follow it, and read it (`shape_cells.rs`).
    let themed = format.color.is_some_and(TextPaint::is_themed) && selection_of(doc, page).is_some_and(|s| s.is_themed());
    if themed {
        // One pass over the places already resolved, charged and capped (`follow_page_theme_refs`).
        follow_page_theme_refs(doc, page, &shapes)?;
    }
    let picked = match format.color {
        Some(TextPaint::Theme(pick)) => Some(pick_colour(doc, page, pick).unwrap_or(Rgb(0, 0, 0)).hex()),
        _ => None,
    };
    let mut edits = Edits::default();
    let mut texts: Vec<(ShapeRef, Vec<TextPart>)> = Vec::new();
    let mut formatted = 0_usize;
    {
        let mut ctx = eval_ctx();
        let reader = Reader::new(doc, &mut ctx);
        let work = Work { doc, reader: &reader, f: format, face: face.as_ref().map(|(name, _)| &**name), picked: picked.as_deref() };
        for at in &shapes {
            let real = work.plan_shape(at, range, &mut edits, &mut texts)?;
            formatted = formatted.saturating_add(usize::from(real));
            if real && themed {
                let mut scheme = Edits::default();
                plan_scheme(doc, &reader, &SheetRef::Shape(at.clone()), &mut scheme);
                edits.append(scheme);
            }
        }
    }
    if formatted == 0 {
        return Ok(0);
    }
    for (at, parts) in texts {
        let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        // The text's ODF data goes with it (`Text.ext`: the file's paragraphs as it held them, which the writer matches to these by content).
        let ext = shape.text.as_ref().map(|t| t.ext.clone()).unwrap_or_default();
        shape.text = Some(Arc::new(Text { parts, ext }));
    }
    if let Some((name, true)) = face {
        Arc::make_mut(&mut doc.fonts).push(FaceName { name_u: name, raw: Default::default(), ext: Default::default() });
    }
    edits.apply(doc, page)?;
    Ok(formatted)
}

/// A range must be of the characters there are: `start` not above `end`, and `end` not past the `total` (fields are not counted). Every command that
/// takes a range (`text.format`, and `shape.formatting` with `start` and `end`) refuses a bad one in these words, with the document untouched.
pub(super) fn check_range(start: usize, end: usize, total: usize) -> Result<(), Problem> {
    if start > end || end > total {
        return Err(Problem::bad(format!(
            "the range {start} to {end} is not within the text, which has {total} characters (fields are not counted); `start` must not be more than `end`"
        )));
    }
    Ok(())
}

/// The one shape a range is about: not a group, which holds no characters of its own.
pub(super) fn single(doc: &Document, page: u32, ids: &[u32]) -> Result<ShapeRef, Problem> {
    let [id] = ids else { return Err(Problem::bad("a range of characters is of one shape: name one shape")) };
    let at = require_shape(doc, page, *id)?;
    if doc.shape(&at).is_some_and(|s| !s.children.is_empty()) {
        return Err(Problem::bad("a group holds no characters of its own: name a shape inside it"));
    }
    Ok(at)
}

/// The name the font table gives a face, and whether it is to be added.
fn face_in_table(doc: &Document, wanted: &str) -> (Arc<str>, bool) {
    if let Some(found) = doc.fonts.iter().find(|f| &*f.name_u == wanted).or_else(|| doc.fonts.iter().find(|f| f.name_u.eq_ignore_ascii_case(wanted)))
    {
        return (Arc::clone(&found.name_u), false);
    }
    (Arc::from(wanted), true)
}

/// How many characters a text holds.
pub(super) fn total_chars(parts: &[TextPart]) -> usize {
    parts.iter().map(|p| if let TextPart::Chars(s) = p { s.chars().count() } else { 0 }).fold(0, usize::saturating_add)
}

/// The rows of a section a shape resolves to, found once for a command and held by `IX`; and the rows the root style adds for a shape with no
/// style link, which the renderer reads in their place.
struct Rows<'d> {
    own: HashMap<u32, EffectiveRow<'d>>,
    root: HashMap<u32, EffectiveRow<'d>>,
}

impl<'d> Rows<'d> {
    fn new(res: &Resolver<'d>, sheet: &SheetRef, kind: Kind) -> Rows<'d> {
        let key = SectionKey::new(kind.section(), None);
        let by_ix = |at: &SheetRef| -> HashMap<u32, EffectiveRow<'d>> {
            let rows = res.effective_rows(at, &key).unwrap_or_default();
            rows.into_iter().filter_map(|row| if let RowKey::Ix(ix) = row.key { Some((ix, row)) } else { None }).collect()
        };
        let root = if matches!(sheet, SheetRef::Shape(_)) { by_ix(&SheetRef::Style(0)) } else { HashMap::new() };
        Rows { own: by_ix(sheet), root }
    }

    /// The first `IX` above every row of the section the shape resolves, and every row the text names.
    fn next_free(&self, parts: &[TextPart], kind: Kind) -> u32 {
        let named = rows_named(parts, kind).into_iter().max().unwrap_or(0);
        let held = self.own.keys().chain(self.root.keys()).copied().max().unwrap_or(0);
        named.max(held).saturating_add(1)
    }

    /// The cells the row `ix` resolves to, as local cells: the formulas as written and the values saved.
    fn copy(&self, ix: u32) -> BTreeMap<CellName, Cell> {
        let mut out: BTreeMap<CellName, Cell> = BTreeMap::new();
        for row in [self.own.get(&ix), self.root.get(&ix)].into_iter().flatten() {
            for (name, found) in &row.cells {
                if out.contains_key(name) {
                    continue;
                }
                let formula = found.formula.cell.map_or(FormulaSlot::Absent, |c| match &c.formula {
                    FormulaSlot::Local(f) => FormulaSlot::Local(Arc::clone(f)),
                    FormulaSlot::NoFormula => FormulaSlot::NoFormula,
                    FormulaSlot::Absent | FormulaSlot::Inherit => FormulaSlot::Absent,
                });
                let value = found.value.cell.and_then(|c| c.value.clone());
                if value.is_none() && formula == FormulaSlot::Absent {
                    continue;
                }
                let unit = found.formula.cell.and_then(|c| c.unit.clone()).or_else(|| found.value.cell.and_then(|c| c.unit.clone()));
                out.insert(name.clone(), Cell { formula, value, unit, ..Cell::default() });
            }
        }
        out
    }
}

/// What a command works with: the document, the reader of its cells, the format and the face it names.
struct Work<'w, 'd, 'c> {
    doc: &'d Document,
    reader: &'w Reader<'d, 'c>,
    f: &'w TextFormat,
    face: Option<&'w str>,
    /// What a theme colour of the format comes to on the page, as the text its cell holds until the recalculation works it out on the shape.
    picked: Option<&'w str>,
}

impl Work<'_, '_, '_> {
    /// Plans the writes for one shape into `edits` and `texts`. → whether anything is to change.
    fn plan_shape(
        &self,
        at: &ShapeRef,
        range: Option<(usize, usize)>,
        edits: &mut Edits,
        texts: &mut Vec<(ShapeRef, Vec<TextPart>)>,
    ) -> Result<bool, Problem> {
        let sheet = SheetRef::Shape(at.clone());
        // A range counts the characters the shape shows: its own text, or its master's when it holds none; a range that splits runs then writes the shape's own text, made of those parts.
        let own: &[TextPart] = shown_parts(self.doc, at);
        let total = total_chars(own);
        // A range is of the characters there are. A text with none has nothing to split: it is formatted whole.
        if let Some((start, end)) = range {
            check_range(start, end, total)?;
        }
        let range = match range {
            Some(_) if total == 0 => None,
            other => other,
        };
        let mut local = Edits::default();
        let mut real = false;
        let mut parts: Option<Vec<TextPart>> = None;
        if self.f.character() {
            match range {
                Some((from, to)) if from < to => {
                    let (changed, split_parts) = self.split_kind(&sheet, Kind::Run, parts.as_deref().unwrap_or(own), from, to, &mut local)?;
                    real |= changed;
                    parts = split_parts.or(parts);
                }
                Some(_) => {}
                None => real |= self.whole(at, &sheet, Kind::Run, &mut local)?,
            }
        }
        if self.f.paragraph() {
            match range {
                Some((from, to)) => {
                    let current = parts.as_deref().unwrap_or(own);
                    if let Some((start, end)) = paragraph_bounds(current, from, to) {
                        let (changed, split_parts) = self.split_kind(&sheet, Kind::Paragraph, current, start, end, &mut local)?;
                        real |= changed;
                        parts = split_parts.or(parts);
                    }
                }
                None => real |= self.whole(at, &sheet, Kind::Paragraph, &mut local)?,
            }
        }
        if let Some(v) = self.f.valign {
            real |= local.put(self.doc, &sheet, CellAddr::direct("VerticalAlign"), &New::whole(v.cell()));
        }
        if let Some(down) = self.f.vertical {
            real |= local.put(self.doc, &sheet, CellAddr::direct("TextDirection"), &New::whole(u32::from(down)));
        }
        if !real {
            return Ok(false);
        }
        edits.append(local);
        if let Some(parts) = parts {
            texts.push((at.clone(), parts));
        }
        Ok(true)
    }

    /// The whole text: the rows its markers name, or row 0, changed where they are. → whether any cell is to change.
    fn whole(&self, at: &ShapeRef, sheet: &SheetRef, kind: Kind, local: &mut Edits) -> Result<bool, Problem> {
        let text = text_of(&self.reader.res, at);
        let mut rows = text.as_deref().map(|t| rows_named(&t.parts, kind)).unwrap_or_default();
        if rows.is_empty() {
            rows.push(0);
        }
        self.in_place(sheet, kind, &rows, local)
    }

    /// Changes the rows named where they are. → whether any cell is to change. A text that names more rows than a section numbers is refused
    /// before any is planned: a row is several cells, and a text may name millions of markers.
    fn in_place(&self, sheet: &SheetRef, kind: Kind, rows: &[u32], local: &mut Edits) -> Result<bool, Problem> {
        if rows.len() > MAX_NAMED_ROWS {
            return Err(too_many_rows());
        }
        let mut real = false;
        for row in rows {
            for (name, new) in self.row_changes(sheet, kind, *row) {
                real |= local.put(self.doc, sheet, row_addr(kind.section(), *row, name), &new);
            }
        }
        Ok(real)
    }

    /// Splits the runs (or paragraphs) of `current` at the ends of `from..to` and writes the rows. → whether any cell is to change,
    /// and the parts of the text after the split when there is one.
    fn split_kind(
        &self,
        sheet: &SheetRef,
        kind: Kind,
        current: &[TextPart],
        from: usize,
        to: usize,
        local: &mut Edits,
    ) -> Result<(bool, Option<Vec<TextPart>>), Problem> {
        let rows = Rows::new(&self.reader.res, sheet, kind);
        let mut alloc = Alloc::starting_at(rows.next_free(current, kind));
        let plan = split(current, kind, from, to, &mut alloc)?;
        let Some(mut parts) = plan.parts else {
            // The range is whole runs: their rows change where they are.
            return Ok((self.in_place(sheet, kind, &plan.modify, local)?, None));
        };
        let section = kind.section();
        // What the split writes is kept apart until it is known that the format changes something: a bold on text that is bold cuts nothing.
        let mut mine = Edits::default();
        // The rows the text names now, each once, in the order they first appear (a new run's is the next free `IX`). A split writes a row for each, so
        // a text that names more than a section numbers is refused here, before a row is planned; it cannot get this far with a row above 65,535 (a
        // new marker needs a number above the highest and there is none), and the check says so in one place.
        let named = rows_named(&parts, kind);
        if named.len() > MAX_NAMED_ROWS {
            return Err(too_many_rows());
        }
        let renumber = named.first() == Some(&0) && !self.inherits_rows_above_zero(&rows, sheet, kind);
        if renumber {
            number_rows_in_text_order(&mut parts, kind, &named);
        }
        let copies: HashMap<u32, u32> = plan.copies.iter().copied().collect();
        let in_range: HashSet<u32> = plan.modify.iter().copied().collect();
        let names = touched(kind, self.f);
        let mut real = false;
        for (position, row) in named.iter().enumerate() {
            let new = if renumber { u32::try_from(position).unwrap_or(u32::MAX) } else { *row };
            let is_new = copies.contains_key(row);
            let source = copies.get(row).copied().unwrap_or(*row);
            let changing = in_range.contains(row);
            if !is_new && new == *row && holds_row(self.doc, sheet, section, *row) {
                // A row that stays and holds cells already: left as it is, or changed where it is.
                if changing {
                    real |= self.in_place(sheet, kind, &[*row], &mut mine)?;
                }
                continue;
            }
            let mut cells = rows.copy(source);
            fill_touched(self.reader, sheet, kind, source, &names, &mut cells);
            if changing {
                real |= self.change_cells(sheet, kind, source, &mut cells)?;
            }
            // A row past the first is whole: what the copy does not hold (a root that states nothing, a row an `.odg` gave) is the value the renderer assumes, and a reader of
            // the file takes nothing for such a row from the styles, nor from a master's row of another run's `IX`. Row 0 leans on the styles.
            if new >= 1 {
                diacraft_doc::root_text::complete_row_cells(section, &mut cells);
            }
            mine.set_row(sheet, section, new, cells);
        }
        if !real {
            return Ok((false, None));
        }
        if renumber {
            // The rows the old markers named past the last number are not named now: all of them go in one pass over the section (`Edits::apply`).
            for old in rows_named(current, kind) {
                if usize::try_from(old).is_ok_and(|old| old >= named.len()) && holds_row(self.doc, sheet, section, old) {
                    mine.remove_row(sheet, section, old);
                }
            }
        }
        local.append(mine);
        Ok((true, Some(parts)))
    }

    /// Whether a master or a style holds a row of the section at `IX` 1 or above for this shape, which a row of the shape's own at that `IX` would be
    /// merged with. **From the rows the command has already found** (`Rows`, once for the command), with no copy of the document: a row of the shape's
    /// effective rows with a cell that `Resolver::inherits` says a master or a style would give if the shape held none of its own, and any row of the
    /// root style at `IX` 1 or above, which `Rows` adds for every shape and which the renderer reads in place of a cell the shape and its chain lack
    /// (so the answer agrees with the rows that are copied). The cost is the cells of the shape's own rows, once.
    fn inherits_rows_above_zero(&self, rows: &Rows<'_>, sheet: &SheetRef, kind: Kind) -> bool {
        if rows.root.iter().any(|(ix, row)| *ix >= 1 && !row.cells.is_empty()) {
            return true;
        }
        rows.own.iter().any(|(ix, row)| {
            *ix >= 1 && row.cells.iter().any(|(name, _)| self.reader.res.inherits(sheet, &row_addr(kind.section(), *ix, name.as_str())))
        })
    }

    /// Changes the cells of a row held in `cells` (a copy of the row `source`), as the format says. → whether any cell is to change.
    fn change_cells(&self, sheet: &SheetRef, kind: Kind, source: u32, cells: &mut BTreeMap<CellName, Cell>) -> Result<bool, Problem> {
        let mut real = false;
        for (name, new) in self.row_changes(sheet, kind, source) {
            let old = cells.get(name);
            if old.is_some_and(Cell::is_guarded) {
                return Err(Problem::failed(format!("the cell {name} is protected by GUARD and cannot be formatted")));
            }
            let cell = new.written_over(old);
            if old != Some(&cell) {
                real = true;
                cells.insert(CellName::new(name), cell);
            }
        }
        Ok(real)
    }

    /// The cells of a row to write for the format, with the row `base` standing for what the row is now (the row itself, or the row a new one is a
    /// copy of).
    fn row_changes(&self, sheet: &SheetRef, kind: Kind, base: u32) -> Vec<(&'static str, New)> {
        let (reader, f) = (self.reader, self.f);
        let mut out: Vec<(&'static str, New)> = Vec::new();
        match kind {
            Kind::Run => {
                if let Some(face) = self.face {
                    out.push(("Font", New::Value(face.to_owned())));
                }
                if let Some(points) = f.size {
                    out.push(("Size", New::number(points / 72.0)));
                }
                if f.bold.is_some() || f.italic.is_some() || f.underline.is_some() || f.case.is_some() {
                    let addr = row_addr("Character", base, "Style");
                    let was = reader.num(sheet, &addr, 0.0).clamp(0.0, 255.0).round() as u32;
                    let mut bits = was;
                    for (flag, bit) in [(f.bold, 1_u32), (f.italic, 2), (f.underline, 4)] {
                        match flag {
                            Some(true) => bits |= bit,
                            Some(false) => bits &= !bit,
                            None => {}
                        }
                    }
                    // Small capitals are a bit of `Style`, and one case is chosen at a time.
                    match f.case {
                        Some(Case::SmallCaps) => bits |= SMALL_CAPS_BIT,
                        Some(_) => bits &= !SMALL_CAPS_BIT,
                        None => {}
                    }
                    if bits != was || !plain(reader, sheet, &addr) {
                        out.push(("Style", New::whole(bits)));
                    }
                }
                if let Some(case) = f.case {
                    let addr = row_addr("Character", base, "Case");
                    let now = reader.num(sheet, &addr, 0.0);
                    if now != f64::from(case.cell()) || !plain(reader, sheet, &addr) {
                        out.push(("Case", New::whole(case.cell())));
                    }
                }
                if let Some(on) = f.strike {
                    let addr = row_addr("Character", base, "Strikethru");
                    let was = reader.num(sheet, &addr, 0.0) != 0.0;
                    if was != on || !plain(reader, sheet, &addr) {
                        out.push(("Strikethru", New::whole(u32::from(on))));
                    }
                }
                if let Some(color) = f.color {
                    out.push((
                        "Color",
                        match color {
                            TextPaint::Colour(rgb) => New::Colour(rgb),
                            TextPaint::Theme(pick) => New::Theme { formula: pick.formula(), value: self.picked.unwrap_or("#000000").to_owned() },
                            TextPaint::Default => New::Themed,
                        },
                    ));
                }
            }
            Kind::Paragraph => {
                if let Some(align) = f.align {
                    let addr = row_addr("Paragraph", base, "HorzAlign");
                    let now = reader.num(sheet, &addr, 1.0);
                    if Align::from_cell(now) != align || now.fract() != 0.0 || !plain(reader, sheet, &addr) {
                        out.push(("HorzAlign", New::whole(align.cell())));
                    }
                }
                if let Some(bullet) = f.bullet {
                    self.bullet_changes(sheet, base, bullet, &mut out);
                }
                if let Some(step) = f.indent {
                    // The step is taken from the indent the row has after a bullet put on it in the same call: a bullet and a step make 0.5.
                    let hung = out.iter().rev().find_map(|(name, new)| match new {
                        New::Value(text) if *name == "IndLeft" => text.parse::<f64>().ok(),
                        _ => None,
                    });
                    let now = hung.unwrap_or_else(|| reader.num(sheet, &row_addr("Paragraph", base, "IndLeft"), 0.0));
                    if let Some(to) = indent_to(now, step) {
                        out.retain(|(name, _)| *name != "IndLeft");
                        out.push(("IndLeft", New::number(to)));
                    }
                }
                if let Some(direction) = f.direction {
                    let addr = row_addr("Paragraph", base, "Flags");
                    let now = reader.num(sheet, &addr, 0.0);
                    if Direction::from_cell(now) != direction || now.fract() != 0.0 || now.abs() > 1.0 || !plain(reader, sheet, &addr) {
                        out.push(("Flags", New::whole(direction.cell())));
                    }
                }
            }
        }
        out
    }
}

impl Work<'_, '_, '_> {
    /// The cells a bullet writes in the paragraph row `base`: `Bullet`, and the hanging form of the indents when the bullet is put on a paragraph that has
    /// none and taken off one that has exactly it.
    fn bullet_changes(&self, sheet: &SheetRef, base: u32, bullet: u8, out: &mut Vec<(&'static str, New)>) {
        let reader = self.reader;
        let at = |name: &str| row_addr("Paragraph", base, name);
        let now = reader.num(sheet, &at("Bullet"), 0.0);
        if now != f64::from(bullet) || !plain(reader, sheet, &at("Bullet")) {
            out.push(("Bullet", New::whole(u32::from(bullet))));
        }
        let (left, first) = (reader.num(sheet, &at("IndLeft"), 0.0), reader.num(sheet, &at("IndFirst"), 0.0));
        let was_bulleted = now != 0.0;
        if bullet != 0 && !was_bulleted && left == 0.0 && first == 0.0 {
            out.push(("IndLeft", New::number(HANG)));
            out.push(("IndFirst", New::number(-HANG)));
        } else if bullet == 0 && was_bulleted && left == HANG && first == -HANG {
            out.push(("IndLeft", New::whole(0)));
            out.push(("IndFirst", New::whole(0)));
        }
    }
}

/// Where a step of Decrease or Increase Indent takes an `IndLeft` of `now` inches, or `None` when it stays: an indent that is no number, one of `MAX_INDENT` or more that
/// is increased, one of 0 or less that is decreased, and **one so large that a quarter inch does not move it** (past about 2^51 inches, and past half the largest number, where its
/// count of quarter inches is no number).
pub(crate) fn indent_to(now: f64, step: Indent) -> Option<f64> {
    // A little under a whole number of steps is that number, so that 0.25 stepped down is 0 and not 0.25 less a hair.
    const FUZZ: f64 = 1.0e-9;
    if !now.is_finite() {
        return None;
    }
    let to = match step {
        Indent::Increase if now < MAX_INDENT => Some((((now / INDENT_STEP + FUZZ).floor() + 1.0) * INDENT_STEP).min(MAX_INDENT)),
        Indent::Decrease if now > FUZZ => Some((((now / INDENT_STEP - FUZZ).ceil() - 1.0) * INDENT_STEP).max(0.0)),
        _ => None,
    };
    to.filter(|to| to.is_finite() && *to != now)
}

/// Whether a cell holds a plain number or nothing: no formula, and not the theme's marker.
fn plain(reader: &Reader<'_, '_>, sheet: &SheetRef, addr: &CellAddr) -> bool {
    let Some(found) = reader.res.resolve(sheet, addr) else { return true };
    let formula = found.formula.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_)));
    let marker = found.value.cell.and_then(|c| c.value.as_ref()).is_some_and(|v| v.reading() == Reading::Themed);
    !formula && !marker
}

/// The names of the cells a format writes.
fn touched(kind: Kind, f: &TextFormat) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    match kind {
        Kind::Run => {
            if f.font.is_some() {
                out.push("Font");
            }
            if f.size.is_some() {
                out.push("Size");
            }
            if f.bold.is_some() || f.italic.is_some() || f.underline.is_some() || f.case.is_some() {
                out.push("Style");
            }
            if f.case.is_some() {
                out.push("Case");
            }
            if f.strike.is_some() {
                out.push("Strikethru");
            }
            if f.color.is_some() {
                out.push("Color");
            }
        }
        Kind::Paragraph => {
            if f.align.is_some() {
                out.push("HorzAlign");
            }
            if f.bullet.is_some() {
                out.extend(["Bullet", "IndLeft", "IndFirst"]);
            }
            if f.indent.is_some() && f.bullet.is_none() {
                out.push("IndLeft");
            }
            if f.direction.is_some() {
                out.push("Flags");
            }
        }
    }
    out
}

/// What a cell holds for a row that resolves to nothing there: the cell's own default.
fn default_text(name: &str) -> Option<String> {
    match name {
        "Font" | "Style" | "Strikethru" | "Color" => Some("0".to_owned()),
        "Size" => Value::from_number(1.0 / 6.0).map(|v| v.text().to_owned()),
        "HorzAlign" => Some("1".to_owned()),
        "Case" | "Bullet" | "IndLeft" | "IndFirst" | "Flags" => Some("0".to_owned()),
        _ => None,
    }
}

/// A row written whole holds every cell the command writes, with what its source row resolves to there (the cell's own default
/// when that is nothing). A reader that carries a cell from one run to the next one that does not hold it, as LibreOffice does, then shows the
/// row as it is, and not as the run before it, which is the one the command changed.
fn fill_touched(reader: &Reader<'_, '_>, sheet: &SheetRef, kind: Kind, source: u32, names: &[&'static str], cells: &mut BTreeMap<CellName, Cell>) {
    for name in names {
        if cells.contains_key(*name) {
            continue;
        }
        let text = match reader.val(sheet, &row_addr(kind.section(), source, name)) {
            Val::Num(n) => Value::from_number(n).map(|v| v.text().to_owned()),
            Val::Colour(c) => Some(c.hex()),
            Val::Text(t) => Some(t.to_string()),
            Val::Missing => default_text(name),
        };
        if let Some(text) = text {
            cells.insert(CellName::new(name), Cell::constant(Value::from_text(&text)));
        }
    }
}

#[cfg(test)]
mod indent_tests {
    use super::*;

    /// An indent that a step of a quarter inch does not move stays where it is (a step up from -9E307 inches, or down from 9E307, made its count of quarter inches no
    /// number and came to an infinity, which the write of the cell took for 0; one down from 1E300 gave the same indent back as a step).
    #[test]
    fn an_indent_a_quarter_inch_does_not_move_stays_where_it_is() {
        assert_eq!(indent_to(-8.988466e307, Indent::Increase), None);
        assert_eq!(indent_to(8.988466e307, Indent::Decrease), None);
        assert_eq!(indent_to(f64::MAX, Indent::Decrease), None);
        assert_eq!(indent_to(1.0e300, Indent::Decrease), None);
        assert_eq!(indent_to(-1.0e300, Indent::Increase), None);
        // The indents a step moves move as before.
        assert_eq!(indent_to(0.0, Indent::Increase), Some(0.25));
        assert_eq!(indent_to(0.3, Indent::Increase), Some(0.5));
        assert_eq!(indent_to(99.9, Indent::Increase), Some(100.0));
        assert_eq!(indent_to(-5.0, Indent::Increase), Some(-4.75));
        assert_eq!(indent_to(0.25, Indent::Decrease), Some(0.0));
        assert_eq!(indent_to(150.0, Indent::Decrease), Some(149.75));
        assert_eq!(indent_to(100.0, Indent::Increase), None);
        assert_eq!(indent_to(0.0, Indent::Decrease), None);
    }
}
