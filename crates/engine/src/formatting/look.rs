//! What a shape's formatting cells resolve to, for the window's controls (`shape.formatting`), and the reading the format commands use to
//! decide what to write.
//!
//! **Read as the renderer reads** (`crates/render/src/read.rs`): the value saved, found by inheritance; a shape with no
//! style link finds nothing for a cell and reads the root style's (the renderer's assumption, no specification settles it); a value that is the
//! theme marker is asked of `calc` by running the cell's own formula on the sheet that asked, and a theme that cannot answer leaves the root
//! style's cell. So the controls show what the picture draws. The face of a
//! run is read by the renderer's own function (`diacraft_render::FaceReader`), not by a copy of its rules here (`face.rs`).
//!
//! **Units**: a weight and a size are points on paper (the cells hold inches on paper); `rounding` is in the page's drawing
//! units, as the cell holds it, and the command turns it into paper inches.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value as Calc};
use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, Document, Reading, Resolver, RowKey, ShapeRef, SheetRef, Text, TextPart};
use diacraft_fonts::FontDb;
use diacraft_render::with_face_reader;

use super::effects::{GradientLook, ShadowLook, fx_look};
use super::face::{Faces, query_fonts};
use super::runs::{Kind, last_row, paragraph_bounds, rows_in_range, rows_when_empty};
use super::text::{check_range, single, total_chars};
use super::{Rgb, leaf_below};
use crate::draw::Problem;

/// The steps `calc` may spend on the themed cells of one reading: a `THEMEVAL()` is a handful, and the answers are kept.
const THEME_STEPS: u64 = 4_000_000;

/// A bound on a length a file writes as `1.#INF`: past every page, short of overflowing arithmetic.
const BIG: f64 = 1.0e15;

/// The state `calc`'s evaluator borrows for a reading: cached operands, which is all a themed cell's formula needs (it names no cell).
pub(crate) fn eval_ctx() -> EvalCtx {
    EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, Budget::with_steps(THEME_STEPS))
}

/// What a cell holds, read for display.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    Missing,
    Num(f64),
    Colour(Rgb),
    Text(Arc<str>),
}

/// Reads cells of one document as the renderer does.
pub(crate) struct Reader<'a, 'c> {
    doc: &'a Document,
    pub res: Resolver<'a>,
    ev: RefCell<Evaluator<'a, 'c>>,
    themed: RefCell<HashMap<(SheetRef, CellAddr), Val>>,
}

impl<'a, 'c> Reader<'a, 'c> {
    pub(crate) fn new(doc: &'a Document, ctx: &'c mut EvalCtx) -> Reader<'a, 'c> {
        Reader { doc, res: Resolver::new(doc), ev: RefCell::new(Evaluator::new(doc, ctx)), themed: RefCell::new(HashMap::new()) }
    }

    /// What the cell holds, found by inheritance.
    pub(crate) fn val(&self, at: &SheetRef, addr: &CellAddr) -> Val {
        let Some(found) = self.res.resolve(at, addr) else { return Val::Missing };
        // A shape with no style link of a cell's family finds nothing for it, and reads the root style's value.
        if found.value.cell.is_none()
            && matches!(at, SheetRef::Shape(_))
            && let Some(root) = self.res.resolve(&SheetRef::Style(0), addr)
            && root.value.cell.is_some()
        {
            return self.val_of(at, addr, root.value.cell.and_then(|c| c.value.as_ref()));
        }
        self.val_of(at, addr, found.value.cell.and_then(|c| c.value.as_ref()))
    }

    fn val_of(&self, at: &SheetRef, addr: &CellAddr, value: Option<&diacraft_doc::Value>) -> Val {
        let Some(value) = value else { return Val::Missing };
        match value.reading() {
            Reading::Number(n) => Val::Num(n),
            Reading::Empty => Val::Missing,
            Reading::Infinite { negative } => Val::Num(if negative { -BIG } else { BIG }),
            Reading::Themed => self.themed(at, addr),
            Reading::Text => {
                let text = value.text();
                // A colour is `#rrggbb`: a face called `Decade` is not one, though its letters are hex digits.
                if text.starts_with('#')
                    && let Some(rgba) = Rgba::from_hex(text)
                {
                    Val::Colour(Rgb(rgba.r, rgba.g, rgba.b))
                } else if text.eq_ignore_ascii_case("true") {
                    Val::Num(1.0)
                } else if text.eq_ignore_ascii_case("false") {
                    Val::Num(0.0)
                } else {
                    Val::Text(Arc::from(text))
                }
            }
        }
    }

    /// The theme's answer for a cell whose value is the marker, kept per cell; the root style's cell when the theme cannot answer.
    fn themed(&self, at: &SheetRef, addr: &CellAddr) -> Val {
        let key = (at.clone(), addr.clone());
        let known = self.themed.borrow().get(&key).cloned();
        if let Some(known) = known {
            return known;
        }
        let asked = CellAddress::new(at.clone(), addr.clone());
        let answer = self.ev.borrow_mut().evaluate(&asked);
        let val = match answer {
            Calc::Number(q) if q.stored.is_finite() => Val::Num(q.stored),
            Calc::Bool(b) => Val::Num(if b { 1.0 } else { 0.0 }),
            Calc::Color(c) => Val::Colour(Rgb(c.r, c.g, c.b)),
            Calc::Text(t) => Val::Text(t),
            Calc::Error(CellError::Na) => Val::Missing,
            _ => self.root_fallback(addr),
        };
        self.themed.borrow_mut().insert(key, val.clone());
        val
    }

    /// What a marker reads as when the theme cannot answer: the root style's cell of the same name, the text colour of any run reading the root's
    /// first row (as the renderer does).
    fn root_fallback(&self, addr: &CellAddr) -> Val {
        let in_character = addr.section.as_ref().is_some_and(|s| s.name.as_str() == "Character");
        let root_row;
        let addr = if in_character && addr.cell.as_str() == "Color" {
            root_row = CellAddr::in_row("Character", None, RowKey::Ix(0), "Color");
            &root_row
        } else {
            addr
        };
        let Some(value) = self.res.resolve(&SheetRef::Style(0), addr).and_then(|r| r.value.cell?.value.as_ref()) else { return Val::Missing };
        match value.reading() {
            Reading::Number(n) => Val::Num(n),
            Reading::Text if value.text().starts_with('#') => Rgba::from_hex(value.text()).map_or(Val::Missing, |c| Val::Colour(Rgb(c.r, c.g, c.b))),
            _ => Val::Missing,
        }
    }

    /// The number the cell holds, or `default` when it holds none.
    pub(crate) fn num(&self, at: &SheetRef, addr: &CellAddr, default: f64) -> f64 {
        match self.val(at, addr) {
            Val::Num(n) if n.is_finite() => n,
            _ => default,
        }
    }

    /// A colour cell: `#rrggbb`, or an index of the fixed palette or the document's table; an index that names nothing is colour 0.
    pub(crate) fn colour(&self, at: &SheetRef, addr: &CellAddr) -> Option<Rgb> {
        match self.val(at, addr) {
            Val::Colour(c) => Some(c),
            Val::Num(n) => {
                let index = if n.is_finite() && (0.0..1.0e9).contains(&n) { n.round() as i64 } else { 0 };
                let c = self.doc.color(index).or_else(|| self.doc.color(0)).unwrap_or(Rgba::BLACK);
                Some(Rgb(c.r, c.g, c.b))
            }
            Val::Text(_) | Val::Missing => None,
        }
    }
}

/// A cell of a row of a section.
pub(crate) fn row_addr(section: &str, ix: u32, name: &str) -> CellAddr {
    CellAddr::in_row(section, None, RowKey::Ix(ix), name)
}

/// The text a shape shows: its own `Text`, or when it has none its master shape's.
pub(crate) fn text_of(res: &Resolver<'_>, at: &ShapeRef) -> Option<Arc<Text>> {
    let shape = res.shape(at)?;
    if let Some(own) = &shape.text {
        return Some(Arc::clone(own));
    }
    match res.master_step(at) {
        Ok(Some(step)) => res.shape(&step.shape).and_then(|m| m.text.clone()),
        _ => None,
    }
}

/// The first `Character` and `Paragraph` rows a text uses: those of its first `cp` and `pp` markers, else row 0.
pub(crate) fn first_rows(text: Option<&Text>) -> (u32, u32) {
    let first = |pick: fn(&TextPart) -> Option<u32>| text.and_then(|t| t.parts.iter().find_map(pick)).unwrap_or(0);
    (first(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }), first(|p| if let TextPart::Pp(ix) = p { Some(*ix) } else { None }))
}

/// How a paragraph's lines sit between the margins (`HorzAlign`). **Distributed** is 4 (`ParagraphDistributed`; the Text dialog's Alignment list has it, and no ribbon button does):
/// it justifies the last line too, where Justify leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
    Distributed,
}

impl Align {
    /// The number `HorzAlign` holds.
    pub fn cell(self) -> u32 {
        match self {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
            Align::Justify => 3,
            Align::Distributed => 4,
        }
    }

    /// The word a command takes and gives.
    pub fn word(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
            Align::Distributed => "distributed",
        }
    }

    /// From a word, in any case.
    pub fn from_word(word: &str) -> Option<Align> {
        [Align::Left, Align::Center, Align::Right, Align::Justify, Align::Distributed].into_iter().find(|a| a.word().eq_ignore_ascii_case(word))
    }

    /// From the number a cell holds: 3 justifies and 4 distributes (as the text layout reads them); a number past the list reads as left.
    pub fn from_cell(n: f64) -> Align {
        match n.round() as i64 {
            1 => Align::Center,
            2 => Align::Right,
            3 => Align::Justify,
            4 => Align::Distributed,
            _ => Align::Left,
        }
    }
}

/// Where the stack of lines rests in the text block (`VerticalAlign`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

impl VAlign {
    pub fn cell(self) -> u32 {
        match self {
            VAlign::Top => 0,
            VAlign::Middle => 1,
            VAlign::Bottom => 2,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        }
    }

    pub fn from_word(word: &str) -> Option<VAlign> {
        [VAlign::Top, VAlign::Middle, VAlign::Bottom].into_iter().find(|a| a.word().eq_ignore_ascii_case(word))
    }

    /// From the number a cell holds: 3 and more is top as well.
    pub fn from_cell(n: f64) -> VAlign {
        match n.round() as i64 {
            1 => VAlign::Middle,
            2 => VAlign::Bottom,
            _ => VAlign::Top,
        }
    }
}

/// How the letters of a run are shown: the four items of Change Case. The first three are the `Case` cell and
/// small capitals are bit 8 of `Style`. The stored text is the same in all four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    /// As typed.
    Normal,
    /// Every letter a capital.
    AllCaps,
    /// The first letter of each word a capital.
    InitialCaps,
    /// A lower-case letter drawn as a smaller capital.
    SmallCaps,
}

impl Case {
    /// The words a command takes and gives.
    pub fn word(self) -> &'static str {
        match self {
            Case::Normal => "normal",
            Case::AllCaps => "all_caps",
            Case::InitialCaps => "initial_caps",
            Case::SmallCaps => "small_caps",
        }
    }

    /// From a word, in any case.
    pub fn from_word(word: &str) -> Option<Case> {
        [Case::Normal, Case::AllCaps, Case::InitialCaps, Case::SmallCaps].into_iter().find(|c| c.word().eq_ignore_ascii_case(word))
    }

    /// The number `Case` holds: 0 for small capitals as well, which are a style bit.
    pub fn cell(self) -> u32 {
        match self {
            Case::AllCaps => 1,
            Case::InitialCaps => 2,
            Case::Normal | Case::SmallCaps => 0,
        }
    }

    /// The case a run shows, from its `Case` cell and its `Style` bits: a capital case wins over small capitals, which draw no smaller capital under it.
    pub fn from_cells(case: f64, style: u32) -> Case {
        match case.round() as i64 {
            1 => Case::AllCaps,
            2 => Case::InitialCaps,
            _ if style & SMALL_CAPS_BIT != 0 => Case::SmallCaps,
            _ => Case::Normal,
        }
    }
}

/// The `Style` bit of small capitals.
pub(crate) const SMALL_CAPS_BIT: u32 = 8;

/// A step of Decrease Indent or Increase Indent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Increase,
    Decrease,
}

impl Indent {
    pub fn word(self) -> &'static str {
        match self {
            Indent::Increase => "increase",
            Indent::Decrease => "decrease",
        }
    }

    pub fn from_word(word: &str) -> Option<Indent> {
        [Indent::Increase, Indent::Decrease].into_iter().find(|i| i.word().eq_ignore_ascii_case(word))
    }
}

/// The reading direction of a paragraph (`Paragraph` `Flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

impl Direction {
    /// The number `Flags` holds.
    pub fn cell(self) -> u32 {
        match self {
            Direction::LeftToRight => 0,
            Direction::RightToLeft => 1,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Direction::LeftToRight => "ltr",
            Direction::RightToLeft => "rtl",
        }
    }

    pub fn from_word(word: &str) -> Option<Direction> {
        [Direction::LeftToRight, Direction::RightToLeft].into_iter().find(|d| d.word().eq_ignore_ascii_case(word))
    }

    /// From the number `Flags` holds: 1 is right to left, and anything else reads left to right.
    pub fn from_cell(n: f64) -> Direction {
        if n.round() as i64 == 1 { Direction::RightToLeft } else { Direction::LeftToRight }
    }
}

/// A shape's formatting, resolved: what the window's controls show. Sizes and weights in points on paper.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeLook {
    pub id: u32,
    /// The fill colour; `None` when the fill paints nothing (`FillPattern` 0).
    pub fill: Option<Rgb>,
    /// The line colour; `None` when the line is not drawn (`LinePattern` 0).
    pub line: Option<Rgb>,
    /// `LineWeight` in points.
    pub weight: f64,
    pub pattern: u32,
    pub begin_arrow: u32,
    pub end_arrow: u32,
    /// `BeginArrowSize` and `EndArrowSize`, 0 to 6 (2, medium, where a shape holds none: the root style's value).
    pub begin_arrow_size: u32,
    pub end_arrow_size: u32,
    /// `Rounding`, in the page's drawing units.
    pub rounding: f64,
    /// The face the first run is drawn in, named as the document or the theme gives it, and when no cell names one the family of the font database's
    /// default (`face.rs`). `None` only where a database with no face at all has no name to give.
    pub font: Option<String>,
    /// The first run's size, in points.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Rgb,
    pub align: Align,
    pub valign: VAlign,
    /// The first paragraph's `Bullet`: 0 for none, 1 to 7 a mark.
    pub bullet: u8,
    /// The first paragraph's `IndLeft`, in paper inches.
    pub ind_left: f64,
    /// How the first run's letters show: Change Case.
    pub case: Case,
    /// The first paragraph's reading direction (`Flags`).
    pub direction: Direction,
    /// Whether the text runs down the block (`TextDirection` 1).
    pub vertical: bool,
    /// The fill's and line's transparency, patterns, gradients and the shadow.
    pub fx: FxLook,
}

/// What the controls of transparency, patterns, gradients and shadows show of a shape (`shape.formatting`). Percentages are 0 to 100.
#[derive(Clone, Debug, PartialEq)]
pub struct FxLook {
    /// `FillPattern` as it reads: 0 none, 1 solid, 2 to 24 a hatch or stipple, 25 to 40 a gradient the old way, 254 a master.
    pub fill_pattern: u32,
    /// The fill's transparency in percent (`FillForegndTrans`).
    pub fill_transparency: f64,
    /// `FillBkgnd`: what lies between the marks of a pattern; none for a fill that is not one.
    pub fill_background: Option<Rgb>,
    pub line_transparency: f64,
    /// 0 round, 1 flat, 2 square.
    pub line_cap: u8,
    /// 0 to 4.
    pub compound: u8,
    /// The fill's gradient, when the fill paints one.
    pub gradient: Option<GradientLook>,
    pub line_gradient: Option<GradientLook>,
    /// The shadow the shape shows, when it shows one.
    pub shadow: Option<ShadowLook>,
}

/// The default text size of a run no cell sets: 12 points.
const DEFAULT_SIZE_INCHES: f64 = 1.0 / 6.0;

/// The resolved formatting of the shapes `ids` of a page, in the order named: for a group, its first member with no members of its own
/// Shapes that are not on the page are refused. The font, size, style, colour and alignment are the first run's and paragraph's.
pub fn look_of(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<ShapeLook>, Problem> {
    look_of_in(doc, page, ids, &query_fonts())
}

/// [`look_of`] with the font database the renderer draws in, which names the face of a run no cell gives one (`face.rs`).
pub fn look_of_in(doc: &Document, page: u32, ids: &[u32], fonts: &FontDb) -> Result<Vec<ShapeLook>, Problem> {
    let mut ctx = eval_ctx();
    let reader = Reader::new(doc, &mut ctx);
    with_face_reader(doc, |names| {
        let named = |at: &ShapeRef, run: u32| names.named_font(at, run);
        let faces = Faces::new(&named, fonts);
        let mut out = Vec::with_capacity(ids.len());
        // Where each shape named is, in one walk of the page, where a search for each walked the page from its top for each; the first one named that is missing is refused.
        let named = crate::draw::Named::of(doc, page, ids.iter().copied())?;
        for id in ids {
            let at = leaf_below(doc, named.require(*id)?);
            let sheet = SheetRef::Shape(at.clone());
            let text = text_of(&reader.res, &at);
            let (run, para) = first_rows(text.as_deref());
            out.push(look_at(&reader, *id, &at, page, &run_props(&reader, &faces, &at, run), &paragraph_props(&reader, &sheet, para)));
        }
        Ok(out)
    })
}

/// What one `Character` row says that the window's Font group shows.
struct RunProps {
    font: Option<String>,
    /// Points.
    size: f64,
    /// The `Style` bit set: 1 bold, 2 italic, 4 underline.
    style: u32,
    strike: bool,
    color: Rgb,
    case: Case,
}

/// The cells of the `Character` row `run` of a shape, as the renderer reads them. The face is the one the renderer sets the run in.
fn run_props(r: &Reader<'_, '_>, faces: &Faces<'_>, at: &ShapeRef, run: u32) -> RunProps {
    #[cfg(test)]
    super::counts::row_read(Kind::Run);
    let sheet = &SheetRef::Shape(at.clone());
    let in_run = |name: &str| row_addr("Character", run, name);
    RunProps {
        font: faces.font_of(at, run),
        size: r.num(sheet, &in_run("Size"), DEFAULT_SIZE_INCHES) * 72.0,
        style: r.num(sheet, &in_run("Style"), 0.0).clamp(0.0, 255.0) as u32,
        strike: r.num(sheet, &in_run("Strikethru"), 0.0) != 0.0,
        color: r.colour(sheet, &in_run("Color")).unwrap_or(Rgb(0, 0, 0)),
        case: Case::from_cells(r.num(sheet, &in_run("Case"), 0.0), r.num(sheet, &in_run("Style"), 0.0).clamp(0.0, 255.0) as u32),
    }
}

/// What one `Paragraph` row says that the window's Paragraph group shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ParaProps {
    align: Align,
    bullet: u8,
    /// Paper inches.
    ind_left: f64,
    direction: Direction,
}

/// The cells of the `Paragraph` row `para` of a shape (a missing `HorzAlign` is centred).
fn paragraph_props(r: &Reader<'_, '_>, sheet: &SheetRef, para: u32) -> ParaProps {
    #[cfg(test)]
    super::counts::row_read(Kind::Paragraph);
    let cell = |name: &str, default: f64| r.num(sheet, &row_addr("Paragraph", para, name), default);
    ParaProps {
        align: Align::from_cell(cell("HorzAlign", 1.0)),
        bullet: cell("Bullet", 0.0).clamp(0.0, 255.0).round() as u8,
        ind_left: cell("IndLeft", 0.0),
        direction: Direction::from_cell(cell("Flags", 0.0)),
    }
}

fn look_at(r: &Reader<'_, '_>, id: u32, at: &ShapeRef, page: u32, run: &RunProps, para: &ParaProps) -> ShapeLook {
    let sheet = SheetRef::Shape(at.clone());
    let direct = CellAddr::direct;
    let number = |name: &str, default: f64| r.num(&sheet, &direct(name), default);
    let fill_pattern = number("FillPattern", 0.0);
    let line_pattern = number("LinePattern", 0.0);
    ShapeLook {
        id,
        fill: (fill_pattern != 0.0).then(|| r.colour(&sheet, &direct("FillForegnd")).unwrap_or(Rgb(255, 255, 255))),
        line: (line_pattern != 0.0).then(|| r.colour(&sheet, &direct("LineColor")).unwrap_or(Rgb(0, 0, 0))),
        weight: number("LineWeight", 0.01) * 72.0,
        pattern: whole(line_pattern),
        begin_arrow: whole(number("BeginArrow", 0.0)),
        end_arrow: whole(number("EndArrow", 0.0)),
        begin_arrow_size: whole(number("BeginArrowSize", 2.0)),
        end_arrow_size: whole(number("EndArrowSize", 2.0)),
        rounding: number("Rounding", 0.0),
        font: run.font.clone(),
        size: run.size,
        bold: run.style & 1 != 0,
        italic: run.style & 2 != 0,
        underline: run.style & 4 != 0,
        strike: run.strike,
        color: run.color,
        align: para.align,
        valign: VAlign::from_cell(number("VerticalAlign", 1.0)),
        bullet: para.bullet,
        ind_left: para.ind_left,
        case: run.case,
        direction: para.direction,
        vertical: number("TextDirection", 0.0).round() as i64 == 1,
        fx: fx_look(r, &sheet, page),
    }
}

/// Which of the properties a range shows differ between the runs (or paragraphs) it touches. A property that differs has no one value to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mixed {
    pub font: bool,
    pub size: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: bool,
    pub align: bool,
    pub bullet: bool,
    pub case: bool,
    pub direction: bool,
}

impl Mixed {
    /// The names of the mixed properties, in the order of the reply: `font`, `size`, `bold`, `italic`, `underline`, `strike`, `color`, `align`, `bullet`,
    /// `case`, `direction`.
    pub fn names(self) -> Vec<&'static str> {
        let all = [
            ("font", self.font),
            ("size", self.size),
            ("bold", self.bold),
            ("italic", self.italic),
            ("underline", self.underline),
            ("strike", self.strike),
            ("color", self.color),
            ("align", self.align),
            ("bullet", self.bullet),
            ("case", self.case),
            ("direction", self.direction),
        ];
        all.into_iter().filter_map(|(name, mixed)| mixed.then_some(name)).collect()
    }

    /// Whether every property of a run is mixed already: no row read after this can change the answer.
    fn all_of_a_run(self) -> bool {
        self.font && self.size && self.bold && self.italic && self.underline && self.strike && self.color && self.case
    }
}

impl RunProps {
    /// Marks in `mixed` the properties in which `other` differs from this run. The face is compared in any case (matches faces so), the size to
    /// a millionth of a point (what the reply shows), and the style bit by bit, so that two numbers that differ in another bit agree about this one.
    fn compare(&self, other: &RunProps, mixed: &mut Mixed) {
        mixed.font |= match (&self.font, &other.font) {
            (None, None) => false,
            (Some(a), Some(b)) => !a.eq_ignore_ascii_case(b),
            _ => true,
        };
        mixed.size |= (self.size * 1.0e6).round() != (other.size * 1.0e6).round();
        let differing = self.style ^ other.style;
        mixed.bold |= differing & 1 != 0;
        mixed.italic |= differing & 2 != 0;
        mixed.underline |= differing & 4 != 0;
        mixed.strike |= self.strike != other.strike;
        mixed.color |= self.color != other.color;
        mixed.case |= self.case != other.case;
    }
}

/// A shape's formatting over a range of its characters: [`ShapeLook`] with the character properties of the **first** run the range touches, and
/// `mixed` saying which of them the range's runs do not share (the value in `look` is not the range's then).
#[derive(Clone, Debug, PartialEq)]
pub struct RangeLook {
    pub look: ShapeLook,
    pub mixed: Mixed,
}

/// The rows of `Character` and of `Paragraph` the characters `start..end` of a text are in: the runs the range touches, with a
/// point read as the one character it stands after (the first character, at the start of the text), which is the run typing there joins;
/// the paragraphs `text.format` takes for `align` (`paragraph_bounds`), the last `pp` when a point stands after the final line break. A text with no
/// characters has the rows its first characters would join.
fn rows_of_range(own: &[TextPart], start: usize, end: usize, total: usize) -> (Vec<u32>, Vec<u32>) {
    if total == 0 {
        let (run, para) = rows_when_empty(own);
        return (vec![run], vec![para]);
    }
    let (from, to) = if start < end {
        (start, end)
    } else {
        let home = start.saturating_sub(1);
        (home, home.saturating_add(1))
    };
    let runs = rows_in_range(own, Kind::Run, from, to);
    let paragraphs = match paragraph_bounds(own, start, end) {
        Some((first, last)) => rows_in_range(own, Kind::Paragraph, first, last),
        None => vec![last_row(own, Kind::Paragraph)],
    };
    (runs, paragraphs)
}

/// The formatting of the characters `start..end` of the one shape `ids` names: the shape's own cells as [`look_of`] gives
/// them and, for the character properties and the paragraph's alignment, the state of the runs and paragraphs the range touches, each property one
/// value or [`Mixed`]. The offsets are checked as `text.format` checks them, and a range is of one shape that is not a group. **Cost:** one pass over
/// the text's parts that stops where the range ends, one read of each distinct row the range touches (and none after every property is mixed).
pub fn look_of_range(doc: &Document, page: u32, ids: &[u32], start: usize, end: usize) -> Result<RangeLook, Problem> {
    look_of_range_in(doc, page, ids, start, end, &query_fonts())
}

/// [`look_of_range`] with the font database the renderer draws in (`face.rs`).
pub fn look_of_range_in(doc: &Document, page: u32, ids: &[u32], start: usize, end: usize, fonts: &FontDb) -> Result<RangeLook, Problem> {
    let at = single(doc, page, ids)?;
    let own: &[TextPart] = doc.shape(&at).and_then(|s| s.text.as_ref()).map_or(&[], |t| t.parts.as_slice());
    let total = total_chars(own);
    check_range(start, end, total)?;
    let (run_rows, para_rows) = rows_of_range(own, start, end, total);
    let mut ctx = eval_ctx();
    let reader = Reader::new(doc, &mut ctx);
    let sheet = SheetRef::Shape(at.clone());
    with_face_reader(doc, |names| {
        let named = |shape: &ShapeRef, run: u32| names.named_font(shape, run);
        let faces = Faces::new(&named, fonts);
        let mut mixed = Mixed::default();
        let mut runs = run_rows.iter();
        let first = run_props(&reader, &faces, &at, runs.next().copied().unwrap_or(0));
        for row in runs {
            if mixed.all_of_a_run() {
                break;
            }
            first.compare(&run_props(&reader, &faces, &at, *row), &mut mixed);
        }
        let mut paragraphs = para_rows.iter();
        let para = paragraph_props(&reader, &sheet, paragraphs.next().copied().unwrap_or(0));
        for row in paragraphs {
            if mixed.align && mixed.bullet && mixed.direction {
                break;
            }
            let other = paragraph_props(&reader, &sheet, *row);
            mixed.align |= other.align != para.align;
            mixed.bullet |= (other.bullet == 0) != (para.bullet == 0);
            mixed.direction |= other.direction != para.direction;
        }
        let id = ids.first().copied().unwrap_or(0);
        Ok(RangeLook { look: look_at(&reader, id, &at, page, &first, &para), mixed })
    })
}

/// A cell's number as a whole number: 0 for one that is negative or not a number.
fn whole(n: f64) -> u32 {
    if n.is_finite() && (0.0..=f64::from(u32::MAX)).contains(&n) { n.round() as u32 } else { 0 }
}
