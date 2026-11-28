//! `TEXTWIDTH` and `TEXTHEIGHT`: the size of a shape's text laid out
//! by `diacraft_text` in the faces `diacraft_fonts` finds for the names its runs carry.
//!
//! **What is measured.** The shape's runs, with the character and paragraph formats the evaluator reads from their cells, are laid out in a
//! block as wide as `maximumWidth` (or with no limit when it is left out), and the answer is the layout's, in drawing units:
//!
//! * `TEXTWIDTH` is the widest line without its trailing spaces, plus both margins, plus the width of one space where the text ends in a line
//!   feed (the final line feed measures as a space, the font's own at the size of the run that holds it; empty text is the margins
//!   alone);
//! * `TEXTHEIGHT` is the stack of lines (`SpLine` and size, with `SpBefore` and `SpAfter` between paragraphs only), plus both margins;
//!   empty text takes one line.
//!
//! Lengths of the text, the margins and the spacing are paper inches and the answer is in drawing units, so it is divided by the page's
//! `PageScale` over `DrawingScale`.
//!
//! **The small term (a project rule).** Both measures add [`TEXT_MEASURE_TERM`], 5E-5 inch of paper, to a text that holds a
//! character (a line feed counts, as a field's shown characters do), **before** the division by the scale, so it grows with the drawing scale; a text with
//! no character, and a shape with no text element, never gets it. It is a constant where the samples drift by up to 7.1E-6 inch with size and line count,
//! and it is added in the one function every call reaches, so a formula an instance inherits from its master gets it as one the shape holds.
//!
//! **`maximumWidth` (project rule).** It is a block width, as `TxtWidth` is: the margins are inside it, so
//! `TEXTHEIGHT(TheText, TxtWidth)` wraps the text as the block draws it. Left out, the text is not wrapped.
//!
//! **A face the machine lacks.** The layout uses the stand-in `diacraft_fonts` picks. Where that is the open twin of the name
//! (Carlito for Calibri, the Liberation faces for Arial, Times New Roman and Courier New) the widths are those the spec's counts were made with;
//! where it is any other face the widths differ, by 6 to 11 percent for the bundled one. Such a measure is still given, because a text box
//! that grows with its text needs a number, and the cell that made it (and any cell that reads that one) carries [`crate::CellEval::stand_in`],
//! so that the oracle leaves it out of the score and says so (`Skipped::text_stand_in`). What counts as another stand-in: a face that is neither the
//! name's own nor its twin, a run that names no font at all, and a bold or italic the face lacks and the renderer makes up. A `TEXTHEIGHT` counts
//! only when the text wrapped (the lines a text takes in a wider face are at least the lines it takes in the original's).
//!
//! **What is not measured.** A text with a field and no text of its own (the master's, shown with the instance's field values) and a text
//! the layout would cut (past `diacraft_text::limits`) and a cell the evaluator cannot read are `Unsupported`, so the saved value stands.
//!
//! **Vertical text** (`TextDirection` 1). It is measured as it runs across, in the shape's own axes: `TEXTWIDTH` is the
//! widest line and the left and right margins, `TEXTHEIGHT` the stack of lines and the top and bottom margins, as for `TextDirection` 0, though the block draws
//! it turned. Two files of the sample files saved this for empty text: a block sized by the measure of the text's height across and of its
//! width down was one line wide and nothing tall, where a measure of the turned layout gave the other way round.
//!
//! **The language of a run** is the one it is drawn in, so that a text box sized by its measure is as wide as the text it shows (`Case` turns `i` into the capital of
//! the run's language, and a face has forms of its own for a language). The cell is found and read by `diacraft_doc::lang::saved_value` and what the layout is handed is
//! `lang::shaping_tag`, the two functions the renderer takes the language of a run from (design 5.1 of the language-proofing design): a run with no `LangID` anywhere is in
//! the drawing's language (`lang::document_language`), an empty value, `x-none`, `zxx`, `none` and 0 are no language, and a number or a text that is no tag is the
//! drawing's. The saved text of the cell is read and no formula is worked out, as the renderer reads it.
//!
//! **Cost.** The text is charged by its bytes before any of it is read (`UNITS_PER_STEP`), and each cell of each format row is a step. What the
//! formats copy is memory held until the measure is done: a face name, a bullet font and a bullet string are cut at [`MAX_NAME_BYTES`] before they are copied (a
//! language is a tag of at most 255 bytes, whatever the cell holds), and the formats, the stops and the tab table are held at their size.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use diacraft_doc::lang::{self, Tag};
use diacraft_doc::{CellAddr, CellName, Owner, RowKey, SectionKey, ShapeRef, SheetRef, Text, TextPart};
use diacraft_fonts::{FontDb, FontStyle, MatchKind, standin};
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, ParaFormat, Run, TabStop, lay_out, limits};
use diacraft_units::{Quantity, Unit};

use crate::eval::Evaluator;
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_TEXT_BYTES, TEXT_MEASURE_TERM};
use crate::value::{CellError, Value};

/// The faces text is measured with: a font database, shared. The database a caller gives is the machine's: the window shares the renderer's
/// with the engine's commits through [`Fonts::shared`], or [`Fonts::lazy`] while the machine's folders are still being read on a thread of its own
/// (the engine's `measure` module), so that a text box is sized in the faces it is drawn in, and the sample-file harness gives
/// `FontDb::system_and_bundled()`. A caller that gives none (a headless session, a test) measures with the bundled faces, which are the same on
/// every machine and reach no file (the web build compiles this).
#[derive(Clone)]
pub struct Fonts(Source);

/// Where the faces are: in hand, or found when a measure first needs them.
#[derive(Clone)]
enum Source {
    Ready(Arc<FontDb>),
    /// Found on demand: the caller's scan of the machine may still be going on a thread of its own, and a measure waits for it, which a formula
    /// that measures no text never does.
    Later(Arc<dyn Fn() -> Arc<FontDb> + Send + Sync>),
}

impl Fonts {
    /// These faces.
    pub fn new(db: FontDb) -> Fonts {
        Fonts(Source::Ready(Arc::new(db)))
    }

    /// A database the caller shares with others (the renderer's).
    pub fn shared(db: Arc<FontDb>) -> Fonts {
        Fonts(Source::Ready(db))
    }

    /// Faces `find` gives when a text is first measured, and not before: the evaluation asks for them only to measure, so one that measures no
    /// text does not wait for a scan that is still going. `find` may block; it is called once for each measure.
    pub fn lazy(find: impl Fn() -> Arc<FontDb> + Send + Sync + 'static) -> Fonts {
        Fonts(Source::Later(Arc::new(find)))
    }

    fn database(&self) -> Arc<FontDb> {
        match &self.0 {
            Source::Ready(db) => Arc::clone(db),
            Source::Later(find) => find(),
        }
    }
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Source::Ready(db) => write!(f, "Fonts({} faces)", db.face_count()),
            Source::Later(_) => write!(f, "Fonts(on demand)"),
        }
    }
}

/// The bundled faces, built once for the process.
fn bundled() -> Arc<FontDb> {
    static BUNDLED: OnceLock<Arc<FontDb>> = OnceLock::new();
    Arc::clone(BUNDLED.get_or_init(|| Arc::new(FontDb::bundled())))
}

/// Which measure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Measure {
    Width,
    Height,
}

impl Measure {
    fn name(self) -> &'static str {
        match self {
            Measure::Width => "TEXTWIDTH",
            Measure::Height => "TEXTHEIGHT",
        }
    }
}

/// The most distinct `Character`, `Paragraph` and `Tabs` rows one text reads: `diacraft_text` reads 262,144 runs, and a row of formats is
/// about thirty cells, so a text naming more rows than a section holds (65,536) is not read.
const MAX_FORMAT_ROWS: usize = 65_536;

/// The most tab stops read from a `Tabs` row.
const MAX_TAB_STOPS: usize = 64;

/// The most bytes of a face name or a bullet string a format keeps (`diacraft_fonts::standin::key` reads no more of a name than a
/// real one holds, and a bullet is a glyph or two): a cell may hold a megabyte, and a text names up to [`MAX_FORMAT_ROWS`] rows.
const MAX_NAME_BYTES: usize = 256;

/// What a format held in a map takes beside its text: the entry of the map and the counts of its `Arc`.
const FORMAT_ENTRY_BYTES: usize = 64;

/// `s` cut to [`MAX_NAME_BYTES`], at a character.
fn capped(s: &str) -> &str {
    let mut end = MAX_NAME_BYTES.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// What a character format takes, as accounted: the format, its entry, and the text it copied.
fn char_bytes(f: &CharFormat) -> u64 {
    let text = f.font.len().saturating_add(f.asian_font.len()).saturating_add(f.complex_font.len()).saturating_add(f.lang.len());
    (std::mem::size_of::<CharFormat>() + FORMAT_ENTRY_BYTES).saturating_add(text) as u64
}

/// What a paragraph format takes, as accounted: the format, its entry, the text it copied and its stops.
fn para_bytes(f: &ParaFormat) -> u64 {
    let text = f.bullet_font.len().saturating_add(f.bullet_string.len());
    let stops = f.tabs.len().saturating_mul(std::mem::size_of::<TabStop>());
    (std::mem::size_of::<ParaFormat>() + FORMAT_ENTRY_BYTES).saturating_add(text).saturating_add(stops) as u64
}

/// What the formats of one run are, found once for a row.
struct Formats {
    chars: HashMap<u32, Arc<CharFormat>>,
    paras: HashMap<(u32, u32), Arc<ParaFormat>>,
    /// The names of the `Position<j>` cells of each `Tabs` row the text names, found once for the text (`Evaluator::tab_table`).
    tab_names: HashMap<u32, Vec<CellName>>,
    /// The stops read from a row, once for a row.
    stops: HashMap<u32, Vec<TabStop>>,
    /// The bytes held for all of the above (and for the runs' formats), to let go when the measure is done.
    held: u64,
}

/// The runs of a text, and what the measure needs of its end.
struct Built {
    runs: Vec<Run>,
    /// The format of the run that holds the last character, and whether that character is a line feed.
    last: Option<(Arc<CharFormat>, bool)>,
    /// The bytes the formats are holding: the caller lets them go when it is done with the runs.
    held: u64,
    /// The characters held, line feeds included.
    chars: usize,
    /// The distinct character formats of the runs that hold characters: the faces the measure depends on.
    used: Vec<Arc<CharFormat>>,
}

fn row_cell(section: &str, ix: u32, name: &str) -> CellAddr {
    CellAddr { section: Some(SectionKey::new(section, None)), row: Some(RowKey::Ix(ix)), cell: CellName::new(name) }
}

impl Evaluator<'_, '_> {
    fn fonts(&self) -> Arc<FontDb> {
        match &self.ctx.opts.fonts {
            Some(f) => f.database(),
            None => bundled(),
        }
    }

    /// The value a format cell holds for the sheet: its own, else the root style's when the sheet has none (a shape with no style link of a
    /// cell's family reads the root style's value, as the renderer reads it), else `None`. An error, or a value the evaluator cannot work out, is
    /// handed back.
    fn format_view(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Result<Option<Value>, Value> {
        let own = self.read_themed(sheet, addr);
        let found = if own.found {
            Some(own)
        } else if matches!(sheet, SheetRef::Shape(_)) {
            let root = self.read_themed(&SheetRef::Style(0), addr);
            root.found.then_some(root)
        } else {
            None
        };
        match found {
            None => Ok(None),
            Some(read) => match &read.view {
                Value::Error(_) | Value::Unsupported(_) => Err(read.view.clone()),
                v => Ok(Some(v.clone())),
            },
        }
    }

    fn format_number(&mut self, sheet: &SheetRef, addr: &CellAddr, default: f64) -> Result<f64, Value> {
        Ok(match self.format_view(sheet, addr)? {
            Some(Value::Number(q)) if q.stored.is_finite() => q.stored,
            Some(Value::Bool(b)) => f64::from(u8::from(b)),
            _ => default,
        })
    }

    fn format_byte(&mut self, sheet: &SheetRef, addr: &CellAddr, default: u8) -> Result<u8, Value> {
        let n = self.format_number(sheet, addr, f64::from(default))?;
        // A saturating cast: the range is checked first, and the fraction is cut.
        Ok(n.clamp(0.0, 255.0) as u8)
    }

    fn format_flag(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Result<bool, Value> {
        Ok(self.format_number(sheet, addr, 0.0)? != 0.0)
    }

    /// A text cell of a format: a face name, a language or a bullet, cut to [`MAX_NAME_BYTES`] before it is copied.
    fn format_text(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Result<String, Value> {
        Ok(match self.format_view(sheet, addr)? {
            Some(Value::Text(t)) => capped(&t).to_string(),
            Some(Value::Number(q)) => crate::format_number(q.stored),
            _ => String::new(),
        })
    }

    /// A face name cell: `0` and empty say none is specified.
    fn face_name(&mut self, sheet: &SheetRef, addr: &CellAddr) -> Result<String, Value> {
        let name = self.format_text(sheet, addr)?;
        Ok(if name == "0" { String::new() } else { name })
    }

    /// The drawing's language, which a run with no `LangID` anywhere is in: found the first time a run needs it, once for the evaluator.
    fn document_language(&self) -> &Tag {
        self.document_language.get_or_init(|| lang::document_language(self.doc, None))
    }

    /// The language the run of `Character` row `ix` is measured in, as the text the layout reads (empty for no language): the language it is drawn in, found by
    /// the two functions the renderer reads it by. The text of a cell is charged by its length before it is read, as a cell's `V` is, and what the format keeps is
    /// the tag of that language, at most [`lang::MAX_TAG_BYTES`] bytes, whatever the cell holds.
    fn run_language(&mut self, sheet: &SheetRef, ix: u32) -> Result<String, Value> {
        if !self.steps(1) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let saved = lang::saved_value(&self.res, sheet, &row_cell("Character", ix, "LangID"));
        if let Some(text) = saved
            && !self.work(text.len())
        {
            return Err(Value::Error(LOOP_ERROR));
        }
        Ok(lang::shaping_tag(saved, self.document_language()))
    }

    fn char_format(&mut self, sheet: &SheetRef, ix: u32) -> Result<CharFormat, Value> {
        let cell = |name: &str| row_cell("Character", ix, name);
        let mut font = self.face_name(sheet, &cell("Font"))?;
        if font.is_empty() {
            // `Font` of 0 takes the root style's.
            let root = self.read_cell(&SheetRef::Style(0), &cell("Font"));
            if let Value::Text(t) = &root.view {
                font = capped(t).to_string();
            }
            if font == "0" {
                font.clear();
            }
        }
        let d = CharFormat::default();
        Ok(CharFormat {
            font,
            asian_font: self.face_name(sheet, &cell("AsianFont"))?,
            complex_font: self.face_name(sheet, &cell("ComplexScriptFont"))?,
            size: self.format_number(sheet, &cell("Size"), d.size)?,
            complex_size: self.format_number(sheet, &cell("ComplexScriptSize"), d.complex_size)?,
            style: self.format_byte(sheet, &cell("Style"), 0)?,
            case: self.format_byte(sheet, &cell("Case"), 0)?,
            pos: self.format_byte(sheet, &cell("Pos"), 0)?,
            scale: self.format_number(sheet, &cell("FontScale"), 1.0)?,
            letterspace: self.format_number(sheet, &cell("Letterspace"), 0.0)?,
            double_underline: self.format_flag(sheet, &cell("DblUnderline"))?,
            overline: self.format_flag(sheet, &cell("Overline"))?,
            strikethrough: self.format_flag(sheet, &cell("Strikethru"))?,
            double_strikethrough: self.format_flag(sheet, &cell("DoubleStrikethrough"))?,
            lang: self.run_language(sheet, ix)?,
            ..d
        })
    }

    /// The names of the `Position<j>` cells of the `Tabs` rows `wanted`, found in one look at the section for the whole text: a text that names
    /// many rows asks for each of them once, and the section is not merged again for each (65,535 distinct `tp` markers over a section of 320,000
    /// rows were two billion merges). The section is charged a step for each row, and a row nothing names is not kept.
    fn tab_table(&mut self, sheet: &SheetRef, wanted: &HashSet<u32>, held: &mut u64) -> Result<HashMap<u32, Vec<CellName>>, Value> {
        let mut table = HashMap::new();
        if wanted.is_empty() {
            return Ok(table);
        }
        let Some(rows) = self.res.effective_rows(sheet, &SectionKey::new("Tabs", None)) else { return Ok(table) };
        if !self.steps(rows.len().saturating_add(1)) {
            return Err(Value::Error(LOOP_ERROR));
        }
        for row in &rows {
            let RowKey::Ix(ix) = &row.key else { continue };
            if !wanted.contains(ix) {
                continue;
            }
            let names: Vec<CellName> = row
                .cells
                .iter()
                .map(|(name, _)| name)
                .filter(|name| name.as_str().strip_prefix("Position").is_some_and(|j| !j.is_empty() && j.bytes().all(|b| b.is_ascii_digit())))
                .take(MAX_TAB_STOPS)
                .cloned()
                .collect();
            self.hold_format(FORMAT_ENTRY_BYTES.saturating_add(names.len().saturating_mul(std::mem::size_of::<CellName>())) as u64, held)?;
            table.insert(*ix, names);
        }
        Ok(table)
    }

    /// The stops of the `Tabs` row `ix`: cells `Position<j>` and `Alignment<j>` for each stop `j`, whose names the table has.
    fn tab_stops(&mut self, sheet: &SheetRef, ix: u32, names: &[CellName]) -> Result<Vec<TabStop>, Value> {
        if !self.steps(names.len().saturating_add(1)) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let mut stops = Vec::with_capacity(names.len());
        for name in names {
            let j = name.as_str().strip_prefix("Position").unwrap_or_default();
            let position = self.format_number(sheet, &row_cell("Tabs", ix, name.as_str()), 0.0)?;
            let alignment = self.format_byte(sheet, &row_cell("Tabs", ix, &format!("Alignment{j}")), 0)?;
            stops.push(TabStop { position, alignment });
        }
        Ok(stops)
    }

    fn para_format(&mut self, sheet: &SheetRef, ix: u32, tabs: Vec<TabStop>) -> Result<ParaFormat, Value> {
        let cell = |name: &str| row_cell("Paragraph", ix, name);
        let d = ParaFormat::default();
        Ok(ParaFormat {
            bullet: self.format_byte(sheet, &cell("Bullet"), d.bullet)?,
            bullet_font: self.face_name(sheet, &cell("BulletFont"))?,
            bullet_size: self.format_number(sheet, &cell("BulletFontSize"), d.bullet_size)?,
            bullet_string: self.format_text(sheet, &cell("BulletStr"))?,
            flags: self.format_byte(sheet, &cell("Flags"), d.flags)?,
            halign: self.format_byte(sheet, &cell("HorzAlign"), d.halign)?,
            ind_first: self.format_number(sheet, &cell("IndFirst"), 0.0)?,
            ind_left: self.format_number(sheet, &cell("IndLeft"), 0.0)?,
            ind_right: self.format_number(sheet, &cell("IndRight"), 0.0)?,
            sp_after: self.format_number(sheet, &cell("SpAfter"), 0.0)?,
            sp_before: self.format_number(sheet, &cell("SpBefore"), 0.0)?,
            sp_line: self.format_number(sheet, &cell("SpLine"), d.sp_line)?,
            text_pos_after_bullet: self.format_number(sheet, &cell("TextPosAfterBullet"), 0.0)?,
            tabs,
        })
    }

    fn block_format(&mut self, sheet: &SheetRef) -> Result<BlockFormat, Value> {
        let d = BlockFormat::default();
        let direct = |name: &str| CellAddr::direct(name);
        Ok(BlockFormat {
            left_margin: self.format_number(sheet, &direct("LeftMargin"), 0.0)?,
            right_margin: self.format_number(sheet, &direct("RightMargin"), 0.0)?,
            top_margin: self.format_number(sheet, &direct("TopMargin"), 0.0)?,
            bottom_margin: self.format_number(sheet, &direct("BottomMargin"), 0.0)?,
            vertical_align: self.format_byte(sheet, &direct("VerticalAlign"), d.vertical_align)?,
            text_direction: self.format_byte(sheet, &direct("TextDirection"), 0)?,
            default_tab_stop: self.format_number(sheet, &direct("DefaultTabStop"), d.default_tab_stop)?,
            // A background draws nothing that is measured, and the block's height does not change what is.
            background: None,
            height: None,
        })
    }

    /// Holds `bytes` more of the formats being made, to let go when the measure is done.
    fn hold_format(&mut self, bytes: u64, held: &mut u64) -> Result<(), Value> {
        if !self.ctx.hold_bytes(bytes) {
            return Err(Value::Error(LOOP_ERROR));
        }
        *held = held.saturating_add(bytes);
        Ok(())
    }

    /// The runs of `text`, each with the formats its `cp`, `pp` and `tp` markers name. The formats are held (their text, their
    /// stops, their entries) until the caller lets `Built::held` go; on an error they are let go here.
    fn build_runs(&mut self, sheet: &SheetRef, text: &Text) -> Result<Built, Value> {
        let mut formats = Formats { chars: HashMap::new(), paras: HashMap::new(), tab_names: HashMap::new(), stops: HashMap::new(), held: 0 };
        match self.build_runs_in(sheet, text, &mut formats) {
            Ok(mut built) => {
                built.held = formats.held;
                Ok(built)
            }
            Err(e) => {
                self.ctx.release_bytes(formats.held);
                Err(e)
            }
        }
    }

    fn build_runs_in(&mut self, sheet: &SheetRef, text: &Text, formats: &mut Formats) -> Result<Built, Value> {
        let (mut cp, mut pp, mut tp) = (None::<u32>, None::<u32>, None::<u32>);
        let mut runs: Vec<Run> = Vec::new();
        let mut used: Vec<Arc<CharFormat>> = Vec::new();
        let mut last: Option<(Arc<CharFormat>, bool)> = None;
        let mut chars = 0usize;
        // The formats of the first markers, for empty text and for text with no marker before its first characters.
        let first_marker = |text: &Text, pick: fn(&TextPart) -> Option<u32>| text.parts.iter().find_map(pick);
        let (first_cp, first_pp, first_tp) = (
            first_marker(text, |p| if let TextPart::Cp(i) = p { Some(*i) } else { None }),
            first_marker(text, |p| if let TextPart::Pp(i) = p { Some(*i) } else { None }),
            first_marker(text, |p| if let TextPart::Tp(i) = p { Some(*i) } else { None }),
        );
        // The `Tabs` rows the text names, read once: the section is looked at once for all of them.
        let wanted: HashSet<u32> =
            text.parts.iter().filter_map(|p| if let TextPart::Tp(i) = p { Some(*i) } else { None }).chain(first_tp).chain([0]).collect();
        formats.tab_names = self.tab_table(sheet, &wanted, &mut formats.held)?;
        for part in &text.parts {
            if !self.steps(1) {
                return Err(Value::Error(LOOP_ERROR));
            }
            let body: &str = match part {
                TextPart::Cp(ix) => {
                    cp = Some(*ix);
                    continue;
                }
                TextPart::Pp(ix) => {
                    pp = Some(*ix);
                    continue;
                }
                TextPart::Tp(ix) => {
                    tp = Some(*ix);
                    continue;
                }
                TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => continue,
                TextPart::Chars(s) => s,
                TextPart::Fld { text, .. } => text,
            };
            // A stretch of the file's own indentation between markers draws nothing.
            if body.contains('\n') && body.contains(' ') && body.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                continue;
            }
            if body.is_empty() {
                continue;
            }
            let (c, p, t) = (cp.or(first_cp).unwrap_or(0), pp.or(first_pp).unwrap_or(0), tp.or(first_tp).unwrap_or(0));
            let char_fmt = self.char_format_of(sheet, formats, c)?;
            let para_fmt = self.para_format_of(sheet, formats, p, t)?;
            chars = chars.saturating_add(body.chars().count());
            if !used.iter().any(|u| Arc::ptr_eq(u, &char_fmt)) {
                used.push(Arc::clone(&char_fmt));
            }
            last = Some((Arc::clone(&char_fmt), body.ends_with('\n')));
            runs.push(Run::new(body, char_fmt, para_fmt));
        }
        if runs.is_empty() {
            // Empty text takes one line, by the formats of its first rows.
            let (c, p, t) = (cp.or(first_cp).unwrap_or(0), pp.or(first_pp).unwrap_or(0), tp.or(first_tp).unwrap_or(0));
            let char_fmt = self.char_format_of(sheet, formats, c)?;
            let para_fmt = self.para_format_of(sheet, formats, p, t)?;
            runs.push(Run::new("", char_fmt, para_fmt));
        }
        Ok(Built { runs, last, chars, used, held: 0 })
    }

    fn char_format_of(&mut self, sheet: &SheetRef, formats: &mut Formats, ix: u32) -> Result<Arc<CharFormat>, Value> {
        if let Some(known) = formats.chars.get(&ix) {
            return Ok(Arc::clone(known));
        }
        if formats.chars.len() >= MAX_FORMAT_ROWS {
            return Err(Value::Error(LOOP_ERROR));
        }
        let made = Arc::new(self.char_format(sheet, ix)?);
        self.hold_format(char_bytes(&made), &mut formats.held)?;
        formats.chars.insert(ix, Arc::clone(&made));
        Ok(made)
    }

    fn para_format_of(&mut self, sheet: &SheetRef, formats: &mut Formats, pp: u32, tp: u32) -> Result<Arc<ParaFormat>, Value> {
        if let Some(known) = formats.paras.get(&(pp, tp)) {
            return Ok(Arc::clone(known));
        }
        if formats.paras.len() >= MAX_FORMAT_ROWS {
            return Err(Value::Error(LOOP_ERROR));
        }
        let stops = match formats.stops.get(&tp) {
            Some(s) => s.clone(),
            None => {
                let names = formats.tab_names.get(&tp).cloned().unwrap_or_default();
                let s = self.tab_stops(sheet, tp, &names)?;
                self.hold_format(
                    FORMAT_ENTRY_BYTES.saturating_add(s.len().saturating_mul(std::mem::size_of::<TabStop>())) as u64,
                    &mut formats.held,
                )?;
                formats.stops.insert(tp, s.clone());
                s
            }
        };
        let made = Arc::new(self.para_format(sheet, pp, stops)?);
        self.hold_format(para_bytes(&made), &mut formats.held)?;
        formats.paras.insert((pp, tp), Arc::clone(&made));
        Ok(made)
    }

    /// The text a shape shows: its own `Text`, or when it has none its master shape's. `Err` for a text that can't be measured here.
    fn text_to_measure(&mut self, at: &ShapeRef, name: &str) -> Result<Option<Arc<Text>>, Value> {
        let Some(shape) = self.res.shape(at) else { return Err(Value::Error(CellError::Ref)) };
        if let Some(own) = &shape.text {
            return Ok(Some(Arc::clone(own)));
        }
        let inherited = match self.res.master_step(at) {
            Ok(Some(step)) => self.res.shape(&step.shape).and_then(|m| m.text.clone()),
            _ => None,
        };
        // An instance shows its master's text with its own field values, which are not worked out here.
        if inherited.as_ref().is_some_and(|t| t.parts.iter().any(|p| matches!(p, TextPart::Fld { .. }))) {
            return Err(Value::unsupported(name));
        }
        Ok(inherited)
    }

    /// The scale of the page the shape is on: `PageScale` over `DrawingScale`, each read as 1 when missing, zero, negative or not a number
    /// The paper length of a drawing length.
    fn page_scale(&mut self, owner: Owner) -> Result<f64, Value> {
        let page = match owner {
            Owner::Page(id) => SheetRef::Page(id),
            Owner::Master(id) => SheetRef::Master(id),
        };
        let positive = |ev: &mut Self, name: &str| -> Result<f64, Value> {
            let n = ev.format_number(&page, &CellAddr::direct(name), 1.0)?;
            Ok(if n.is_finite() && n > 0.0 { n } else { 1.0 })
        };
        let s = positive(self, "PageScale")? / positive(self, "DrawingScale")?;
        Ok(if s.is_finite() && s > 0.0 { s } else { 1.0 })
    }

    /// The width of one space in the face and at the size a run is set, in paper inches: its advance over the face's units to the
    /// em, at the run's size, wider by `FontScale`, with the letter spacing a letter takes.
    fn space_width(&self, db: &FontDb, format: &CharFormat) -> f64 {
        let style = FontStyle { bold: format.style & diacraft_text::style::BOLD != 0, italic: format.style & diacraft_text::style::ITALIC != 0 };
        let face = db.pick(&format.font, style);
        let upem = face.units_per_em();
        if face.is_null() || upem.is_nan() || upem <= 0.0 {
            return 0.0;
        }
        let scale = if format.scale > 0.0 && format.scale.is_finite() { format.scale } else { 1.0 };
        let size = if format.size > 0.0 && format.size.is_finite() { format.size } else { 0.0 };
        face.advance(face.glyph_for(' ')) / upem * size * scale + format.letterspace
    }
}

/// Whether a face found for a name is the name's own twin: the face the spec's counts were made with. `Calibri Light` takes Carlito as the nearest
/// face and no claim is made that the widths match.
fn is_twin(requested: &str, used: &str) -> bool {
    let (asked, got) = (standin::key(requested), standin::key(used));
    asked != "calibri light" && standin::stand_ins(&asked).contains(&got.as_str())
}

/// Whether the face a layout used for a name is its open twin (the face the spec's counts were made with), or a stand-in whose widths differ.
fn is_approximate(notes: &[Note]) -> bool {
    notes.iter().any(|n| match n {
        Note::StandIn { requested, used } => !is_twin(requested, used),
        Note::StandInsOmitted { .. } | Note::FaceFailed { .. } | Note::FaceSearchCut | Note::MissingGlyphs { .. } | Note::ShapingBudgetUsed => true,
        _ => false,
    })
}

/// Whether a face the database gives a format is the name's own, or its twin, with its bold and italic of its own. The layout notes a stand-in
/// only for a name that is given, and not for a bold or an italic it makes up, so the formats of the runs are asked as well: a run that names
/// no font draws in the database's default, and a bold the face lacks is made up by the renderer, and neither has the original's widths.
fn format_is_approximate(db: &FontDb, format: &CharFormat) -> bool {
    let key = standin::key(&format.font);
    if key.is_empty() || key == "0" {
        return true;
    }
    let style = FontStyle { bold: format.style & diacraft_text::style::BOLD != 0, italic: format.style & diacraft_text::style::ITALIC != 0 };
    let found = db.resolve(&format.font, style);
    if found.synthetic_bold || found.synthetic_italic {
        return true;
    }
    match found.kind {
        MatchKind::Named => false,
        MatchKind::StandIn => !is_twin(&format.font, found.face.family()),
        MatchKind::Default => true,
    }
}

/// Whether a text took more lines than it has paragraphs: it wrapped (or has a forced break, which this counts too), so a face of other widths
/// could have made another number of lines.
fn wrapped(laid: &Laid) -> bool {
    laid.lines.windows(2).any(|pair| matches!(pair, [a, b] if a.paragraph == b.paragraph))
}

/// Whether a layout left out some of the text or did not lay out what it was given, so that its size is not the text's.
fn is_cut(notes: &[Note]) -> bool {
    notes.iter().any(|n| matches!(n, Note::Truncated { .. } | Note::RunsCut { .. } | Note::LinesCut { .. } | Note::BadNumber))
}

/// `TEXTWIDTH(ref[, maximumWidth])` and `TEXTHEIGHT(ref, maximumWidth)` on the shape that holds the `TheText` cell `ref` names.
pub(crate) fn measure(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, maximum_width: Option<f64>, which: Measure) -> Value {
    // The bytes the formats of the runs hold while the text is laid out: let go on every way out.
    let mut held = 0u64;
    let result = (|| {
        let SheetRef::Shape(at) = sheet else { return Err(Value::Error(CellError::Value)) };
        let text = ev.text_to_measure(at, which.name())?;
        // The bytes are charged before any is read, and a text no formula could build is refused.
        let bytes: usize = text.as_ref().map_or(0, |t| {
            t.parts
                .iter()
                .map(|p| match p {
                    TextPart::Chars(s) => s.len(),
                    TextPart::Fld { text, .. } => text.len(),
                    _ => 8,
                })
                .fold(0usize, usize::saturating_add)
        });
        if bytes > 4 * MAX_TEXT_BYTES {
            return Err(Value::Error(LOOP_ERROR));
        }
        // A layout costs about a step for four characters.
        if !ev.work(bytes.saturating_mul(16)) {
            return Err(Value::Error(LOOP_ERROR));
        }
        let scale = ev.page_scale(at.owner)?;
        // Vertical text (`TextDirection` 1) is measured as it runs across, in the shape's own axes (it): the
        // drawing turns it, the measure does not.
        let block = BlockFormat { text_direction: 0, ..ev.block_format(sheet)? };
        let empty = Text::default();
        let built = ev.build_runs(sheet, text.as_deref().unwrap_or(&empty))?;
        held = built.held;
        if built.chars > limits::MAX_CHARS {
            return Err(Value::unsupported(which.name()));
        }
        let width = maximum_width.map(|w| w * scale);
        let db = ev.fonts();
        let laid = lay_out(&built.runs, &block, width, &db);
        if is_cut(&laid.notes) {
            return Err(Value::unsupported(which.name()));
        }
        // The cell carries that a stand-in measured it, and so does a cell that reads it. Empty text has no letters to be wide, and a height counts
        // when the text wrapped.
        let letters = built.chars > 0;
        let differs = letters && (is_approximate(&laid.notes) || built.used.iter().any(|f| format_is_approximate(&db, f)));
        if differs && (which == Measure::Width || wrapped(&laid)) {
            ev.ctx.stand_in_touched = true;
        }
        // The mark of a final line feed is a space at the end of the last line: along the line, so in the width.
        let paper = match which {
            Measure::Width => {
                let space = match &built.last {
                    Some((format, true)) => ev.space_width(&db, format),
                    _ => 0.0,
                };
                block.left_margin + block.right_margin + laid.size.width + space
            }
            Measure::Height => block.top_margin + block.bottom_margin + laid.size.height,
        };
        // The small term sits inside the scale and only a text that holds a character gets it (a project rule): the same call, local or
        // inherited, so the formulas an instance takes from its master get it as the ones it holds.
        let term = if letters { TEXT_MEASURE_TERM } else { 0.0 };
        let drawing = (paper + term) / scale;
        if drawing.is_finite() { Ok(Value::Number(Quantity::new(drawing, Unit::DrawingDefault))) } else { Err(Value::Error(BAD_ARGUMENT_ERROR)) }
    })();
    ev.ctx.release_bytes(held);
    result.unwrap_or_else(|e| e)
}
