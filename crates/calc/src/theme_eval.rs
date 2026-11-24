//! What a theme part answers.
//!
//! **The route.** A call finds its parts by the scheme numbers of the sheet (`vt:schemeID`'s `schemeEnum`, never a part's place; a number no part holds takes the first part):
//! the colour scheme's part for colours and variants, and the effect scheme's or
//! the connector scheme's for the lists (a 1-D shape is a connector at `QuickStyleType` 0). The Quick Style matrix of the property's
//! family then picks an entry and the Quick Style colour cell picks the colour that stands for `phClr`.
//!
//! **What stands where the theme cannot say.** A matrix of 0 and a cell the entry does not give take the root style's cell; a colour
//! past a list takes the no-theme table. A part, list or entry that could not be read (the reader left it `None`) and a
//! property this evaluator does not work out are [`Value::Unsupported`], so the saved value stands; a stop colour past its entry's list is `#N/A`
//! (project rule), which the renderer reads as a row with no colour, and its transparency and position take the root style's cell.
//!
//! **Budget.** Every part walked, every entry's modifier and every candidate of the contrast step costs a step, before the work; every list
//! is capped by the reader, so a call is bounded whatever the file holds.

use diacraft_color::{ColorTransform, SchemeSlot};
use diacraft_doc::{
    CellAddr, Document, EffectEntry, FillEntry, GradientEntry, GradientKind, LineEntry, LineEx, SchemeId, SheetRef, Theme, ThemeBase, ThemeColour,
    VarStyle,
};
use diacraft_units::{Quantity, Unit};

use crate::eval::Evaluator;
use crate::fn_color::{THEMEVAL_COLOURS, rgb_of, to_hls};
use crate::rules::LOOP_ERROR;
use crate::theme_colour::apply;
use crate::theme_names::{P, StopWhat, root_cell, table_name};
use crate::value::{CellError, Rgb, Value};

/// EMU in an inch.
const EMU_PER_INCH: f64 = 914_400.0;

/// 60000ths of a degree to a degree (DrawingML's angle unit).
const DIR_PER_DEGREE: f64 = 60_000.0;

/// MS-VSDX's contrast limits, 16.66 and 72.92 percent, on the 0 to 240 scale (our reading).
const CONTRAST_GAP: i64 = 40;
const DARK_BACKGROUND_TOP: i64 = 175;

type Out<T> = Result<T, Value>;

fn unsupported() -> Value {
    Value::unsupported("THEMEVAL")
}

fn spent() -> Value {
    Value::Error(LOOP_ERROR)
}

/// The Quick Style colour cell that stands for `phClr` in an entry of a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QColour {
    Fill,
    Line,
    Font,
    Shadow,
}

impl QColour {
    fn cell(self) -> &'static str {
        match self {
            QColour::Fill => "QuickStyleFillColor",
            QColour::Line => "QuickStyleLineColor",
            QColour::Font => "QuickStyleFontColor",
            QColour::Shadow => "QuickStyleShadowColor",
        }
    }
}

/// What a call selected: the parts and the shape's kind.
pub(crate) struct Sel<'d> {
    pub sheet: SheetRef,
    /// The part of the colour scheme: the slots, the background and the colour variants.
    pub colour: &'d Theme,
    /// The part whose lists answer: the effect scheme's, or the connector scheme's.
    pub lists: &'d Theme,
    /// The part of the font scheme.
    pub font: &'d Theme,
    pub connector: bool,
}

/// An entry the matrix picked, or why there is none.
enum Found<'d, T> {
    /// Matrix 0: the root style's cell.
    Root,
    /// A number past the list, or an entry that names none: no entry.
    Past,
    /// The entry, and its number counting from 1.
    Entry(&'d T, usize),
}

fn length(emu: i64) -> Value {
    Value::Number(Quantity::new(emu as f64 / EMU_PER_INCH, Unit::DrawingDefault))
}

fn whole(n: u32) -> Value {
    Value::num(f64::from(n))
}

fn rgb(c: diacraft_color::Rgba) -> Rgb {
    Rgb { r: c.r, g: c.g, b: c.b }
}

/// A whole, non-negative cell value as an index; anything else is none.
fn index_of(x: f64) -> Option<usize> {
    (x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 1.0e9).then_some(x as usize)
}

fn color_id(t: &Theme) -> Option<&SchemeId> {
    t.color_scheme_id.as_ref()
}

fn effect_id(t: &Theme) -> Option<&SchemeId> {
    t.effect_scheme_id.as_ref()
}

fn connector_id(t: &Theme) -> Option<&SchemeId> {
    t.connector_scheme_id.as_ref()
}

fn font_id(t: &Theme) -> Option<&SchemeId> {
    t.font_scheme_id.as_ref()
}

/// The part a scheme number selects: the first whose id has that `schemeEnum`; a number no part holds takes the first part,
/// and the flag says that it did.
fn part_for(doc: &Document, number: f64, id: fn(&Theme) -> Option<&SchemeId>) -> Option<(&Theme, bool)> {
    let wanted = index_of(number).and_then(|n| u32::try_from(n).ok());
    let found = wanted.and_then(|n| doc.themes.iter().find(|t| id(t).is_some_and(|i| i.scheme_enum == n)));
    match found {
        Some(t) => Some((&**t, false)),
        None => doc.themes.get(0).map(|t| (&**t, true)),
    }
}

impl<'d> Evaluator<'d, '_> {
    /// A number cell of the sheet, found along its masters and styles: 0 where none holds one.
    fn theme_number(&mut self, sheet: &SheetRef, name: &str) -> f64 {
        self.read_cell(sheet, &CellAddr::direct(name)).view.quantity().map_or(0.0, |q| q.stored)
    }

    /// Counts a fallback the theme batch says to warn of (`EvalStats::theme_fallbacks`).
    fn note_fallback(&mut self, happened: bool) {
        if happened {
            self.ctx.stats.theme_fallbacks = self.ctx.stats.theme_fallbacks.saturating_add(1);
        }
    }

    /// An entry number that names none: counted.
    fn past<'t, T>(&mut self) -> Out<Found<'t, T>> {
        self.note_fallback(true);
        Ok(Found::Past)
    }

    /// Whether the shape is one-dimensional: any of its ends is held (`IsShape1D`).
    fn is_one_dimensional(&mut self, sheet: &SheetRef) -> bool {
        if !matches!(sheet, SheetRef::Shape(_)) {
            return false;
        }
        ["BeginX", "BeginY", "EndX", "EndY"]
            .iter()
            .any(|cell| self.res.resolve(sheet, &CellAddr::direct(cell)).is_some_and(|r| r.formula.cell.is_some() || r.value.cell.is_some()))
    }

    /// The parts and the kind of shape a call on `sheet` reads. `None` where the file has no theme part at all, and
    /// for a custom colour scheme, which is a master's.
    pub(crate) fn select(&mut self, sheet: &SheetRef, cells: [f64; 5]) -> Out<Sel<'d>> {
        let doc: &'d Document = self.doc;
        if doc.themes.is_empty() || cells[1] == 65535.0 {
            return Err(unsupported());
        }
        // Three scans of the part list, one for each scheme the call reads.
        if !self.steps(doc.themes.len().saturating_mul(3)) {
            return Err(spent());
        }
        let kind = self.theme_number(sheet, "QuickStyleType");
        let connector = match if kind.is_finite() { kind as i64 } else { 0 } {
            3 => true,
            1 | 2 => false,
            _ => self.is_one_dimensional(sheet),
        };
        let (colour, missed_colour) = part_for(doc, cells[1], color_id).ok_or_else(unsupported)?;
        let (lists, missed_lists) =
            if connector { part_for(doc, cells[3], connector_id) } else { part_for(doc, cells[2], effect_id) }.ok_or_else(unsupported)?;
        let (font, missed_font) = part_for(doc, cells[4], font_id).ok_or_else(unsupported)?;
        for missed in [missed_colour, missed_lists, missed_font] {
            self.note_fallback(missed);
        }
        Ok(Sel { sheet: sheet.clone(), colour, lists, font, connector })
    }

    // ---- colours ---------------------------------------------------------------------------------------------------------

    /// A colour of the part's scheme by `Quick Style` colour number: 0 `dk1`, 1 `lt1`, 2 to 7 `accent1` to `accent6`, 8 the background, 100 to
    /// 106 the variant's colours (all the first on a monotone variant) and 200 to 206 the same with no monotone step.
    fn quick_colour(&mut self, sel: &Sel<'d>, number: f64) -> Out<Rgb> {
        if !number.is_finite() {
            return Err(unsupported());
        }
        match number.trunc() as i64 {
            n @ 0..=7 => sel.colour.slot_colour(slot_of(n as usize)).map(rgb).ok_or_else(unsupported),
            8 => sel.colour.entries.background.map(rgb).ok_or_else(unsupported),
            n @ 100..=106 => self.variant_colour(sel, (n - 100) as usize, true),
            n @ 200..=206 => self.variant_colour(sel, (n - 200) as usize, false),
            _ => Err(unsupported()),
        }
    }

    /// The colour `k` (0 to 6) of the variant `VariationColorIndex` picks; a monotone variant gives its first where `monotone_step` is set.
    fn variant_colour(&mut self, sel: &Sel<'d>, k: usize, monotone_step: bool) -> Out<Rgb> {
        let at = index_of(self.scheme_cell(&sel.sheet, "VariationColorIndex")).ok_or_else(unsupported)?;
        let variant = sel.colour.entries.colour_variants.get(at).and_then(Option::as_ref).ok_or_else(unsupported)?;
        let k = if variant.monotone && monotone_step { 0 } else { k };
        variant.colours.get(k).copied().flatten().map(rgb).ok_or_else(unsupported)
    }

    /// The colour a Quick Style colour cell picks, which stands for `phClr` in an entry.
    fn selected(&mut self, sel: &Sel<'d>, which: QColour) -> Out<Rgb> {
        let number = self.theme_number(&sel.sheet, which.cell());
        self.quick_colour(sel, number)
    }

    /// A colour of an entry with its modifiers, and the alpha they state (1 where none does). `phClr` is what `which` picks.
    fn resolve(&mut self, sel: &Sel<'d>, colour: &ThemeColour, which: QColour) -> Out<(Rgb, f64)> {
        if !self.steps(1 + colour.modifiers.len()) {
            return Err(spent());
        }
        let base = match colour.base {
            ThemeBase::Rgb(c) => rgb(c),
            ThemeBase::Slot(SchemeSlot::PhClr) => self.selected(sel, which)?,
            ThemeBase::Slot(slot) => sel.lists.slot_colour(slot).or_else(|| sel.colour.slot_colour(slot)).map(rgb).ok_or_else(unsupported)?,
        };
        Ok(apply(base, &colour.modifiers))
    }

    /// The root style's cell of a property, or the no-theme table's colour for a colour property past its list.
    pub(crate) fn fallback(&mut self, p: P, root: bool) -> Out<Value> {
        if !root && let Some(name) = table_name(p) {
            return THEMEVAL_COLOURS.iter().find(|(n, _)| *n == name).map(|(_, c)| rgb_of(*c)).ok_or_else(unsupported);
        }
        let addr = root_cell(p).ok_or_else(unsupported)?;
        Ok(self.read_cell(&SheetRef::Style(0), &addr).view.clone())
    }

    // ---- entries ---------------------------------------------------------------------------------------------------------

    /// The entry a Quick Style matrix cell picks from `list`: 0 is the root style, 1 to 99 an entry counting from 1, and 100 to 103
    /// read the first to fourth `vt:varStyle` of the variant `VariationStyleIndex` picks, whose number (`pick`) is the entry.
    fn entry<'t, T>(&mut self, sel: &Sel<'d>, matrix: &str, list: &'t [Option<T>], pick: fn(&VarStyle) -> Option<u32>) -> Out<Found<'t, T>> {
        let m = self.theme_number(&sel.sheet, matrix);
        if !m.is_finite() {
            return self.past();
        }
        let n = match m.trunc() as i64 {
            0 => return Ok(Found::Root),
            _ if list.is_empty() => return Err(unsupported()),
            n @ 1..=99 => n as usize,
            n @ 100..=103 => {
                let at = index_of(self.scheme_cell(&sel.sheet, "VariationStyleIndex")).ok_or_else(unsupported)?;
                let variant = sel.lists.entries.style_variants.get(at).and_then(Option::as_ref).ok_or_else(unsupported)?;
                let style = variant.styles.get((n - 100) as usize).ok_or_else(unsupported)?;
                match pick(style) {
                    Some(k) if k >= 1 => k as usize,
                    _ => return self.past(),
                }
            }
            _ => return self.past(),
        };
        match list.get(n - 1) {
            None => self.past(),
            Some(None) => Err(unsupported()),
            Some(Some(e)) => Ok(Found::Entry(e, n)),
        }
    }

    fn fill_entry(&mut self, sel: &Sel<'d>) -> Out<Found<'d, FillEntry>> {
        let list: &'d [Option<FillEntry>] = &sel.lists.entries.lists(sel.connector).fills;
        self.entry(sel, "QuickStyleFillMatrix", list, |v| v.fill_idx)
    }

    fn line_entry(&mut self, sel: &Sel<'d>) -> Out<Found<'d, LineEntry>> {
        let list: &'d [Option<LineEntry>] = &sel.lists.entries.lists(sel.connector).lines;
        self.entry(sel, "QuickStyleLineMatrix", list, |v| v.line_idx)
    }

    fn effect_entry(&mut self, sel: &Sel<'d>) -> Out<Found<'d, EffectEntry>> {
        let list: &'d [Option<EffectEntry>] = &sel.lists.entries.lists(sel.connector).effects;
        self.entry(sel, "QuickStyleEffectsMatrix", list, |v| v.effect_idx)
    }

    // ---- the colours of a family -----------------------------------------------------------------------------------------

    /// A solid entry's colour, or the selected colour plain for any other kind of fill.
    fn fill_colour_of(&mut self, sel: &Sel<'d>, fill: Option<&FillEntry>, which: QColour) -> Out<(Rgb, f64)> {
        match fill {
            Some(FillEntry::Solid(c)) => self.resolve(sel, c, which),
            _ => Ok((self.selected(sel, which)?, 1.0)),
        }
    }

    /// `FillColor`, `LineColor`, `TextColor`, `ShadowColor` as the entry gives them, before the contrast step.
    fn base_colour(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        match p {
            P::FillColour => match self.fill_entry(sel)? {
                Found::Root => self.fallback(p, true),
                Found::Past => self.fallback(p, false),
                Found::Entry(e, _) => Ok(Value::Color(self.fill_colour_of(sel, Some(e), QColour::Fill)?.0)),
            },
            P::FillColour2 => match self.fill_entry(sel)? {
                Found::Root => self.fallback(p, true),
                Found::Past => self.fallback(p, false),
                Found::Entry(FillEntry::Solid(c), _) if c.base == ThemeBase::Slot(SchemeSlot::PhClr) => Ok(rgb_of(0x4D4D4D)),
                // A solid naming a colour gives that colour, plain (its modifiers are not applied, our reading).
                Found::Entry(FillEntry::Solid(c), _) => match c.base {
                    ThemeBase::Rgb(x) => Ok(Value::Color(rgb(x))),
                    ThemeBase::Slot(s) => sel.lists.slot_colour(s).map(|x| Value::Color(rgb(x))).ok_or_else(unsupported),
                },
                Found::Entry(e, _) => Ok(Value::Color(self.fill_colour_of(sel, Some(e), QColour::Fill)?.0)),
            },
            P::LineColour => match self.line_entry(sel)? {
                Found::Root => self.fallback(p, true),
                Found::Past => self.fallback(p, false),
                Found::Entry(e, _) => Ok(Value::Color(self.fill_colour_of(sel, e.fill.as_ref(), QColour::Line)?.0)),
            },
            P::TextColour => {
                let list: &'d [Option<diacraft_doc::FontProps>] = sel.lists.entries.fonts_for(sel.connector);
                match self.entry(sel, "QuickStyleFontMatrix", list, |v| v.font_idx)? {
                    Found::Root => self.fallback(p, true),
                    Found::Past => self.fallback(p, false),
                    Found::Entry(e, _) => match &e.colour {
                        Some(c) => Ok(Value::Color(self.resolve(sel, c, QColour::Font)?.0)),
                        None => Ok(Value::Color(self.selected(sel, QColour::Font)?)),
                    },
                }
            }
            P::ShadowColour => match self.effect_entry(sel)? {
                Found::Root => self.fallback(p, true),
                Found::Past => self.fallback(p, false),
                Found::Entry(e, _) => match &e.outer_shadow {
                    Some(s) => Ok(Value::Color(self.resolve(sel, &s.colour, QColour::Shadow)?.0)),
                    // No shadow: `#000000` (our rule). The oracle leaves the cell out, since files save another colour here.
                    None => {
                        self.ctx.no_shadow_touched = true;
                        Ok(rgb_of(0x000000))
                    }
                },
            },
            _ => Err(unsupported()),
        }
    }

    /// `TextColor`, `LineColor` and `FillColor` after the contrast step of `QuickStyleVariation`: bit 2, 4 or 8.
    fn stepped(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        let base = self.base_colour(sel, p)?;
        let Value::Color(c) = base else { return Ok(base) };
        let bit = match p {
            P::TextColour => 2,
            P::LineColour => 4,
            _ => 8,
        };
        let variation = self.theme_number(&sel.sheet, "QuickStyleVariation");
        if !variation.is_finite() || (variation.trunc() as i64) & bit == 0 {
            return Ok(base);
        }
        let background = sel.colour.entries.background.map(rgb).ok_or_else(unsupported)?;
        let light = |c: Rgb| to_hls(c).1;
        let (own, back) = (light(c), light(background));
        if (own - back).abs() >= CONTRAST_GAP {
            return Ok(base);
        }
        if back <= DARK_BACKGROUND_TOP {
            return Ok(Value::Color(Rgb { r: 255, g: 255, b: 255 }));
        }
        let candidates: &[P] = if bit == 2 { &[P::TextColour, P::FillColour, P::LineColour] } else { &[P::FillColour, P::LineColour] };
        let mut best = (-1, c);
        for q in candidates {
            if !self.steps(1) {
                return Err(spent());
            }
            if let Value::Color(x) = self.base_colour(sel, *q)? {
                let gap = (light(x) - own).abs();
                if gap > best.0 {
                    best = (gap, x);
                }
            }
        }
        Ok(Value::Color(best.1))
    }

    // ---- everything else -------------------------------------------------------------------------------------------------

    /// What the theme gives for property `p`: the whole.
    pub(crate) fn property(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        match p {
            P::Slot(n) => self.slot(sel, n),
            P::VariantColour(k) => self.variant_colour(sel, usize::from(k), false).map(Value::Color),
            P::TextColour | P::FillColour | P::LineColour => self.stepped(sel, p),
            P::FillColour2 | P::ShadowColour => self.base_colour(sel, p),
            P::Stop { line, n, what } => self.stop(sel, p, line, usize::from(n), what),
            P::LineWeight
            | P::LineCap
            | P::LineCompound
            | P::LinePattern
            | P::LineRounding
            | P::LineBegin
            | P::LineBeginSize
            | P::LineEnd
            | P::LineEndSize
            | P::LineTrans
            | P::LineGradEnabled
            | P::LineGradDir
            | P::LineGradAngle => self.line_property(sel, p),
            P::FillPattern | P::FillTrans | P::FillGradEnabled | P::FillGradDir | P::FillGradAngle | P::RotateGradient => self.fill_property(sel, p),
            P::ShadowPattern | P::ShadowStyle | P::ShadowTrans | P::ShadowX | P::ShadowY | P::ShadowMag | P::ShadowDir | P::ShadowBlur => {
                self.shadow_property(sel, p)
            }
            P::LatinFont => sel.font.minor_font.as_deref().map(Value::text).ok_or_else(unsupported),
            P::TextStyle => self.text_style(sel, p),
            P::FillTransBkgnd | P::Other => Err(unsupported()),
        }
    }

    /// `Dark`, `Light`, `AccentColor`... `BackgroundColor`: the slot of the colour scheme's part.
    fn slot(&mut self, sel: &Sel<'d>, n: u8) -> Out<Value> {
        self.quick_colour(sel, f64::from(n)).map(Value::Color)
    }

    fn text_style(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        let list: &'d [Option<diacraft_doc::FontProps>] = sel.lists.entries.fonts_for(sel.connector);
        match self.entry(sel, "QuickStyleFontMatrix", list, |v| v.font_idx)? {
            Found::Root => self.fallback(p, true),
            Found::Past => self.fallback(p, false),
            Found::Entry(e, _) => e.style.map(whole).map_or_else(|| self.fallback(p, true), Ok),
        }
    }

    /// The alpha an entry's colour states, as a transparency: 0 where it states none.
    fn transparency(alpha: f64) -> Value {
        Value::num((1.0 - alpha).clamp(0.0, 1.0))
    }

    fn line_property(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        let (entry, n) = match self.line_entry(sel)? {
            Found::Root => return self.fallback(p, true),
            Found::Past => return self.fallback(p, false),
            Found::Entry(e, n) => (e, n),
        };
        match p {
            P::LineWeight => entry.width_emu.map_or_else(|| self.fallback(p, true), |w| Ok(length(w))),
            P::LineCap => match entry.cap.as_deref() {
                Some("rnd") => Ok(Value::num(0.0)),
                Some("flat") => Ok(Value::num(1.0)),
                Some("sq") => Ok(Value::num(2.0)),
                _ => self.fallback(p, true),
            },
            P::LineCompound => match entry.compound.as_deref() {
                Some("sng") => Ok(Value::num(0.0)),
                Some("dbl") => Ok(Value::num(1.0)),
                Some("thickThin") => Ok(Value::num(2.0)),
                Some("thinThick") => Ok(Value::num(3.0)),
                Some("tri") => Ok(Value::num(4.0)),
                _ => self.fallback(p, true),
            },
            P::LineTrans => match &entry.fill {
                Some(FillEntry::Solid(c)) => Ok(Self::transparency(self.resolve(sel, c, QColour::Line)?.1)),
                _ => Ok(Value::num(0.0)),
            },
            P::LineGradEnabled | P::LineGradDir | P::LineGradAngle => self.gradient_property(p, entry.fill.as_ref()),
            _ => self.line_ex_property(sel, p, entry, n),
        }
    }

    /// What a `vt:lineEx` says: the pattern, the rounding and the line ends. The list runs one ahead of the lines: the entry numbered `n`
    /// reads position `n + 1` counting from 1.
    fn line_ex_property(&mut self, sel: &Sel<'d>, p: P, entry: &LineEntry, n: usize) -> Out<Value> {
        let ex: Option<&LineEx> = sel.lists.entries.line_ex_for(sel.connector).get(n).and_then(Option::as_ref);
        let dash = || match entry.dash.as_deref() {
            Some("sysDash") => 9,
            Some("sysDot") => 10,
            Some("dash") => 2,
            _ => 1,
        };
        let value = match (p, ex) {
            (P::LinePattern, Some(LineEx { pattern: Some(x), .. })) => Some(*x),
            // A theme with no `pattern` maps four dash names to numbers and any other to solid.
            (P::LinePattern, _) => Some(dash()),
            (_, None) => return Err(unsupported()),
            (P::LineBegin, Some(e)) => e.start,
            (P::LineBeginSize, Some(e)) => e.start_size,
            (P::LineEnd, Some(e)) => e.end,
            (P::LineEndSize, Some(e)) => e.end_size,
            (P::LineRounding, Some(e)) => return e.rounding_emu.map_or_else(|| self.fallback(p, true), |r| Ok(length(r))),
            _ => None,
        };
        value.map_or_else(|| self.fallback(p, true), |x| Ok(whole(x)))
    }

    /// The direction and angle of a gradient entry (`FillGradientDir`, `FillGradientAngle`), and whether there is one.
    fn gradient_property(&mut self, p: P, fill: Option<&FillEntry>) -> Out<Value> {
        let gradient: Option<&GradientEntry> = if let Some(FillEntry::Gradient(g)) = fill { Some(g) } else { None };
        match (p, gradient) {
            (P::FillGradEnabled | P::LineGradEnabled, g) => Ok(Value::num(if g.is_some() { 1.0 } else { 0.0 })),
            (_, None) => self.fallback(p, true),
            (P::FillGradDir | P::LineGradDir, Some(g)) => Ok(Value::num(match g.kind {
                GradientKind::Linear { .. } | GradientKind::Unstated => 0.0,
                GradientKind::Circle => 3.0,
                GradientKind::Rect => 10.0,
                GradientKind::Shape => 13.0,
            })),
            (P::FillGradAngle | P::LineGradAngle, Some(g)) => match g.kind {
                // `a:lin`'s angle is the cell's, in the same degrees.
                GradientKind::Linear { degrees } => Ok(Value::Number(Quantity::new(degrees.to_radians(), Unit::AngleDefault))),
                _ => Ok(Value::Number(Quantity::new(0.0, Unit::AngleDefault))),
            },
            (P::RotateGradient, Some(g)) => g.rotate_with_shape.map_or_else(|| self.fallback(p, true), |b| Ok(Value::num(if b { 1.0 } else { 0.0 }))),
            _ => Err(unsupported()),
        }
    }

    fn fill_property(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        let (entry, n) = match self.fill_entry(sel)? {
            Found::Root => return self.fallback(p, true),
            Found::Past => return self.fallback(p, false),
            Found::Entry(e, n) => (e, n),
        };
        match p {
            P::FillPattern => {
                // A `noFill` entry paints as pattern 0; the others read `vt:fillProps` at the same number.
                if matches!(entry, FillEntry::NoFill) {
                    return Ok(Value::num(0.0));
                }
                let props = sel.lists.entries.fill_props.get(n - 1).and_then(Option::as_ref).ok_or_else(unsupported)?;
                props.pattern.map_or_else(|| self.fallback(p, true), |x| Ok(whole(x)))
            }
            P::FillTrans => match entry {
                FillEntry::Solid(c) => Ok(Self::transparency(self.resolve(sel, c, QColour::Fill)?.1)),
                _ => Ok(Value::num(0.0)),
            },
            _ => self.gradient_property(p, Some(entry)),
        }
    }

    fn shadow_property(&mut self, sel: &Sel<'d>, p: P) -> Out<Value> {
        let entry = match self.effect_entry(sel)? {
            Found::Root => return self.fallback(p, true),
            Found::Past => return self.fallback(p, false),
            Found::Entry(e, _) => e,
        };
        let Some(shadow) = &entry.outer_shadow else {
            // No shadow: the pattern is 0, and the rest is the root's. Files save other answers here, so the oracle
            // leaves the cell out; the entry decides that, and not the value saved.
            self.ctx.no_shadow_touched = true;
            return if p == P::ShadowPattern { Ok(Value::num(0.0)) } else { self.fallback(p, true) };
        };
        let bearing = (shadow.dir as f64 / DIR_PER_DEGREE).to_radians();
        let inches = |emu: i64| emu as f64 / EMU_PER_INCH;
        let inch = |x: f64| Ok(Value::Number(Quantity::new(x, Unit::DrawingDefault)));
        match p {
            P::ShadowPattern | P::ShadowStyle => Ok(Value::num(1.0)),
            P::ShadowTrans => Ok(Self::transparency(self.resolve(sel, &shadow.colour, QColour::Shadow)?.1)),
            // The offset is the distance along the bearing, with y up: a bearing of 90 degrees moves the shadow down.
            P::ShadowX => inch(inches(shadow.dist_emu) * bearing.cos()),
            P::ShadowY => inch(-inches(shadow.dist_emu) * bearing.sin()),
            P::ShadowBlur => inch(inches(shadow.blur_emu)),
            P::ShadowMag => Ok(Value::num(shadow.scale_x)),
            _ => self.fallback(p, true),
        }
    }

    /// The colour, transparency or position of a stop of a themed gradient: the entry's `n`th stop. A row past the list, and an entry that is
    /// no gradient, have no stop: the colour is `#N/A`, which a renderer reads as a row with no colour (project rule), and the
    /// transparency and the position are the root style's cell ([`Evaluator::no_such_stop`]).
    fn stop(&mut self, sel: &Sel<'d>, p: P, line: bool, n: usize, what: StopWhat) -> Out<Value> {
        let (fill, which) = if line {
            match self.line_entry(sel)? {
                Found::Root => return self.fallback(p, true),
                Found::Past => return self.fallback(p, false),
                Found::Entry(e, _) => (e.fill.as_ref(), QColour::Line),
            }
        } else {
            match self.fill_entry(sel)? {
                Found::Root => return self.fallback(p, true),
                Found::Past => return self.fallback(p, false),
                Found::Entry(e, _) => (Some(e), QColour::Fill),
            }
        };
        let Some(FillEntry::Gradient(g)) = fill else { return self.no_such_stop(p, what) };
        let Some(stop) = g.stops.get(n) else { return self.no_such_stop(p, what) };
        match what {
            StopWhat::Position => Ok(Value::num(stop.pos)),
            StopWhat::Transparency => Ok(Self::transparency(self.resolve(sel, &stop.colour, which)?.1)),
            StopWhat::Colour => Ok(Value::Color(self.resolve(sel, &stop.colour, which)?.0)),
        }
    }

    /// A stop row whose entry has no such stop (the entry is no gradient, or the row is past its list). The transparency and the position take the
    /// root style's cell (371 and 297 cells saved 0, the root's, where the call gave an error); the colour stays `#N/A`, which the
    /// renderer reads as a row with no colour and drops, and which no sample holds a value for (a stop colour under a theme holds
    /// `Themed`).
    fn no_such_stop(&mut self, p: P, what: StopWhat) -> Out<Value> {
        match what {
            StopWhat::Colour => Err(Value::Error(CellError::Na)),
            StopWhat::Transparency | StopWhat::Position => self.fallback(p, true),
        }
    }

    // ---- THEMECBV and THEMEPROP ----------------------------------------------------------------------------------------

    /// `THEMECBV(color, stop)`: the modifiers of the `stop`th stop (counting from 1) of the fill entry the matrix picks, applied to `color`; the
    /// colour as it is where the entry is no gradient, the stop has no modifiers or is past the list.
    pub(crate) fn stop_modifiers(&mut self, sel: &Sel<'d>, colour: Rgb, stop: usize) -> Out<Rgb> {
        let found = self.fill_entry(sel)?;
        let Found::Entry(FillEntry::Gradient(g), _) = found else { return Ok(colour) };
        let Some(s) = stop.checked_sub(1).and_then(|i| g.stops.get(i)) else { return Ok(colour) };
        if !self.steps(1 + s.colour.modifiers.len()) {
            return Err(spent());
        }
        let mods: Vec<ColorTransform> = s.colour.modifiers.clone();
        Ok(apply(colour, &mods).0)
    }

    /// `THEMEPROP("Embellishment")`: the sheet's `EmbellishmentIndex` where it is not 0, else the `embellishment` of the style variant that
    /// `VariationStyleIndex` picks.
    pub(crate) fn embellishment(&mut self, sel: &Sel<'d>) -> Out<Value> {
        let own = self.scheme_cell(&sel.sheet, "EmbellishmentIndex");
        if own != 0.0 {
            return Ok(Value::num(own));
        }
        let at = index_of(self.scheme_cell(&sel.sheet, "VariationStyleIndex")).ok_or_else(unsupported)?;
        let variant = sel.lists.entries.style_variants.get(at).and_then(Option::as_ref).ok_or_else(unsupported)?;
        variant.embellishment.map(whole).ok_or_else(unsupported)
    }

    /// `THEMEPROP("Multiformat")`: 1 where the variant `VariationColorIndex` picks is not `monotone`, which is also where it states none (M1
    ///); 0 where it is (our reasoning).
    pub(crate) fn multiformat(&mut self, sel: &Sel<'d>) -> Out<Value> {
        let at = index_of(self.scheme_cell(&sel.sheet, "VariationColorIndex")).ok_or_else(unsupported)?;
        let variant = sel.colour.entries.colour_variants.get(at).and_then(Option::as_ref).ok_or_else(unsupported)?;
        Ok(Value::num(if variant.monotone { 0.0 } else { 1.0 }))
    }

    /// A `Quick Style` value as the property (`THEMEVAL(101)`): the colour it names, with no entry's modifiers.
    pub(crate) fn quick_property(&mut self, sel: &Sel<'d>, number: f64) -> Out<Value> {
        self.quick_colour(sel, number).map(Value::Color)
    }
}

/// The slot of a colour scheme that `Quick Style` colour 0 to 7 names: `dk1`, `lt1`, `accent1` to `accent6`.
fn slot_of(n: usize) -> SchemeSlot {
    match n {
        0 => SchemeSlot::Dk1,
        1 => SchemeSlot::Lt1,
        2 => SchemeSlot::Accent1,
        3 => SchemeSlot::Accent2,
        4 => SchemeSlot::Accent3,
        5 => SchemeSlot::Accent4,
        6 => SchemeSlot::Accent5,
        _ => SchemeSlot::Accent6,
    }
}
