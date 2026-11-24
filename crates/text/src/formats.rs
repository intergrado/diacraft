//! The formats of a drawing, made safe to compute with: every number finite and inside bounds, every name cut short.
//!
//! A drawing is hostile, and so are the numbers in its cells: NaN, infinity, 1E300. Nothing after this module divides or multiplies
//! a value from a drawing without it having come through here. A bad number reads as its default and the layout says so with a
//! [`Note::BadNumber`](crate::Note::BadNumber), since a drawing that holds one is damaged, and no file does (the census's values
//! are all finite).

use crate::limits::{MAX_LENGTH, MAX_NAME_CHARS};
use crate::model::{BlockFormat, CharFormat, ParaFormat, Rgba, style};

/// Tracks whether any number had to be replaced.
#[derive(Debug, Default)]
pub(crate) struct Sanitizer {
    pub bad: bool,
}

impl Sanitizer {
    /// A length in inches: finite and no more than [`MAX_LENGTH`] either way, else `default`.
    pub fn len(&mut self, v: f64, default: f64) -> f64 {
        if v.is_finite() && v.abs() <= MAX_LENGTH {
            v
        } else {
            self.bad = true;
            default
        }
    }

    /// A share or a scale: finite and no more than a million either way, else `default`.
    pub fn share(&mut self, v: f64, default: f64) -> f64 {
        if v.is_finite() && v.abs() <= 1.0e6 {
            v
        } else {
            self.bad = true;
            default
        }
    }
}

/// A name cut at [`MAX_NAME_CHARS`] characters, without copying it.
pub(crate) fn short(name: &str) -> &str {
    name.char_indices().nth(MAX_NAME_CHARS).map_or(name, |(i, _)| name.get(..i).unwrap_or(name))
}

/// Does a face name say that no face is named: empty, or the number 0?
pub(crate) fn no_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n == "0"
}

/// The lines a character format asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decor {
    pub underline: bool,
    pub double_underline: bool,
    pub overline: bool,
    pub strike: bool,
    pub double_strike: bool,
}

/// One character format as the layout reads it.
#[derive(Clone, Debug)]
pub(crate) struct RunStyle<'a> {
    pub font: &'a str,
    pub asian: &'a str,
    pub complex: &'a str,
    /// `Size`: 0 or less draws nothing.
    pub size: f64,
    /// The size complex script letters draw at.
    pub complex_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub complex_bold: bool,
    pub complex_italic: bool,
    pub pos: u8,
    pub scale: f64,
    pub letterspace: f64,
    pub color: Rgba,
    pub opacity: f64,
    pub decor: Decor,
    /// `LangID`, as a culture name: what the shaper picks a script's local forms by. Empty for none.
    pub lang: &'a str,
}

impl<'a> RunStyle<'a> {
    pub fn new(c: &'a CharFormat, san: &mut Sanitizer) -> RunStyle<'a> {
        let size = san.len(c.size, 0.0).max(0.0);
        let complex_raw = san.share(c.complex_size, -1.0);
        let complex_size = if complex_raw > 0.0 {
            complex_raw.min(MAX_LENGTH)
        } else if complex_raw < 0.0 {
            size * -complex_raw
        } else {
            size
        };
        // FontScale 0 reads as 1, so a stored 0 doesn't lose the text; a negative one is read the same way.
        let scale = san.share(c.scale, 1.0);
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let transparency = san.share(c.transparency, 0.0).clamp(0.0, 1.0);
        RunStyle {
            font: short(&c.font),
            asian: short(&c.asian_font),
            complex: short(&c.complex_font),
            size,
            complex_size: complex_size.min(MAX_LENGTH),
            bold: c.style & style::BOLD != 0,
            italic: c.style & style::ITALIC != 0,
            complex_bold: c.style & style::COMPLEX_BOLD != 0,
            complex_italic: c.style & style::COMPLEX_ITALIC != 0,
            pos: c.pos,
            scale,
            letterspace: san.len(c.letterspace, 0.0),
            color: c.color,
            opacity: 1.0 - transparency,
            decor: Decor {
                underline: c.style & style::UNDERLINE != 0,
                double_underline: c.double_underline,
                overline: c.overline,
                strike: c.strikethrough,
                double_strike: c.double_strikethrough,
            },
            lang: short(&c.lang),
        }
    }
}

/// How the alignment cell reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Centre,
    Right,
    /// Stretch every line but a paragraph's last.
    Justify,
    /// Stretch every line.
    Distribute,
}

/// How a tab stop aligns the text after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabAlign {
    Left,
    Centre,
    Right,
    Decimal,
}

/// One paragraph format as the layout reads it.
#[derive(Clone, Debug)]
pub(crate) struct ParaStyle {
    pub align: Align,
    pub rtl: bool,
    pub ind_first: f64,
    pub ind_left: f64,
    pub ind_right: f64,
    pub sp_before: f64,
    pub sp_after: f64,
    pub sp_line: f64,
    /// Custom tab stops, nearest first, positions from the text area's left edge.
    pub tabs: Vec<(f64, TabAlign)>,
}

impl ParaStyle {
    pub fn new(p: &ParaFormat, san: &mut Sanitizer) -> ParaStyle {
        let align = match p.halign {
            0 => Align::Left,
            2 => Align::Right,
            3 | 6..=8 => Align::Justify,
            4 => Align::Distribute,
            // 1 is centred, and so is what the root holds, which is what any other value reads as.
            _ => Align::Centre,
        };
        let mut tabs: Vec<(f64, TabAlign)> = p
            .tabs
            .iter()
            .take(64)
            .filter_map(|t| {
                let position = san.len(t.position, f64::NAN);
                let align = match t.alignment {
                    0 => TabAlign::Left,
                    1 => TabAlign::Centre,
                    2 => TabAlign::Right,
                    _ => TabAlign::Decimal,
                };
                position.is_finite().then_some((position, align))
            })
            .collect();
        tabs.sort_by(|a, b| a.0.total_cmp(&b.0));
        ParaStyle {
            align,
            rtl: p.flags == 1,
            ind_first: san.len(p.ind_first, 0.0),
            ind_left: san.len(p.ind_left, 0.0),
            ind_right: san.len(p.ind_right, 0.0),
            sp_before: san.len(p.sp_before, 0.0),
            sp_after: san.len(p.sp_after, 0.0),
            sp_line: san.len(p.sp_line, -1.2),
            tabs,
        }
    }

    /// The height of a line box whose largest size is `size`: a multiple of it below 0, a distance above 0, one size at 0.
    pub fn line_height(&self, size: f64) -> f64 {
        if self.sp_line < 0.0 {
            -self.sp_line * size
        } else if self.sp_line > 0.0 {
            self.sp_line
        } else {
            size
        }
    }
}

/// The block's format with numbers made safe.
#[derive(Clone, Debug)]
pub(crate) struct BlockStyle {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    /// 0 top, 1 middle, 2 bottom.
    pub valign: u8,
    pub default_tab: f64,
    pub height: Option<f64>,
    pub background: Option<(Rgba, f64)>,
}

impl BlockStyle {
    pub fn new(b: &BlockFormat, san: &mut Sanitizer) -> BlockStyle {
        BlockStyle {
            left: san.len(b.left_margin, 0.0),
            right: san.len(b.right_margin, 0.0),
            top: san.len(b.top_margin, 0.0),
            bottom: san.len(b.bottom_margin, 0.0),
            valign: match b.vertical_align {
                1 => 1,
                2 => 2,
                // 0 is the top, and so is anything of 3 or more.
                _ => 0,
            },
            default_tab: san.len(b.default_tab_stop, 0.0),
            height: b.height.map(|h| san.len(h, 0.0)),
            background: b.background.map(|(c, t)| (c, san.share(t, 0.0).clamp(0.0, 1.0))),
        }
    }
}
