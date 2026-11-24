//! What a layout is given: runs of text, each with the character and paragraph format the caller resolved for it, and the
//! block's own format.
//!
//! **Resolved values, not cells.** Everything here is a value as the ShapeSheet cell holds it after the caller has done the
//! resolving: a theme marker is already a face name or a colour, a `Font` of 0 is already
//! the root style's name. Lengths are paper inches, as the cells hold them: text
//! ignores the drawing scale, so the caller multiplies a block's drawing-unit width by `PageScale` over `DrawingScale` once,
//! and nothing here ever sees the scale.
//!
//! **Defaults** are the root style's values, which every file's root holds (of both batches), so a format built with
//! `Default` draws as a shape with no cell of its own does: 12 points, centred, lines at 120%.

use std::sync::Arc;

pub use diacraft_color::Rgba;

/// `Style` bits: one switch each, tested with AND and never by equality.
pub mod style {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 2;
    pub const UNDERLINE: u8 = 4;
    pub const SMALL_CAPS: u8 = 8;
    /// Bold for complex script letters.
    pub const COMPLEX_BOLD: u8 = 16;
    /// Italic for complex script letters.
    pub const COMPLEX_ITALIC: u8 = 32;
}

/// One `Character` row's cells, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct CharFormat {
    /// `Font`: the face for Latin letters and for everything no other face is named for. Empty means none specified.
    pub font: String,
    /// `AsianFont`: the face for East Asian letters. Empty or `"0"` hands them to `font`.
    pub asian_font: String,
    /// `ComplexScriptFont`: the face for complex script letters. Empty or `"0"` hands them to `font`.
    pub complex_font: String,
    /// `Size`, paper inches. 0 draws nothing and a negative reads as 0.
    pub size: f64,
    /// `ComplexScriptSize`: above 0 inches, 0 the same as `size`, below 0 that share of `size`, so -1 is the same.
    pub complex_size: f64,
    /// `Style` bits: see [`style`].
    pub style: u8,
    /// `Case`: 0 as stored, 1 capitals, 2 the first letter of each word a capital.
    pub case: u8,
    /// `Pos`: 0 on the baseline, 1 raised, 2 lowered.
    pub pos: u8,
    /// `FontScale`, 1 being normal. 0 reads as 1.
    pub scale: f64,
    /// `Letterspace`, paper inches added after each letter.
    pub letterspace: f64,
    /// `Color`, resolved to red, green and blue (the alpha is ignored: see `transparency`).
    pub color: Rgba,
    /// `ColorTrans`, 0 opaque to 1 clear.
    pub transparency: f64,
    /// `DblUnderline`.
    pub double_underline: bool,
    /// `Overline`.
    pub overline: bool,
    /// `Strikethru`.
    pub strikethrough: bool,
    /// `DoubleStrikethrough`.
    pub double_strikethrough: bool,
    /// `LangID`, a culture name such as `en-US` or `tr-TR`; it picks capitals for `Case` and the shaper's language forms.
    pub lang: String,
}

impl Default for CharFormat {
    fn default() -> Self {
        CharFormat {
            font: String::new(),
            asian_font: String::new(),
            complex_font: String::new(),
            size: 1.0 / 6.0,
            complex_size: -1.0,
            style: 0,
            case: 0,
            pos: 0,
            scale: 1.0,
            letterspace: 0.0,
            color: Rgba::BLACK,
            transparency: 0.0,
            double_underline: false,
            overline: false,
            strikethrough: false,
            double_strikethrough: false,
            lang: "en-US".to_owned(),
        }
    }
}

/// One tab stop of a `Tabs` row: where, and how text sits on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    /// `Position<j>`, paper inches from the text area's left edge.
    pub position: f64,
    /// `Alignment<j>`: 0 starts there, 1 centres on it, 2 ends there, 3 and above line a decimal point up.
    pub alignment: u8,
}

/// One `Paragraph` row's cells and the `Tabs` row beside it, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct ParaFormat {
    /// `Bullet`: 0 none, 1 to 7 a mark from the table.
    pub bullet: u8,
    /// `BulletFont`: empty or `"0"` for the table's face.
    pub bullet_font: String,
    /// `BulletFontSize`: above 0 inches, 0 the first character's size, below 0 that share of it.
    pub bullet_size: f64,
    /// `BulletStr`: replaces the table's mark when not empty.
    pub bullet_string: String,
    /// `Flags`: 1 is right to left, anything else is left to right.
    pub flags: u8,
    /// `HorzAlign`: 0 left, 1 centred, 2 right, 3 justified, 4 force-justified; 6 to 8 justify as 3, anything else reads as 1.
    pub halign: u8,
    /// `IndFirst`, from `ind_left`, for a paragraph's first line.
    pub ind_first: f64,
    /// `IndLeft`.
    pub ind_left: f64,
    /// `IndRight`.
    pub ind_right: f64,
    /// `SpAfter`.
    pub sp_after: f64,
    /// `SpBefore`.
    pub sp_before: f64,
    /// `SpLine`: a multiple of the largest size on the line below 0, a distance above 0, and one size's height at 0.
    pub sp_line: f64,
    /// `TextPosAfterBullet`: from the bullet's start; 0 or less means 0.25 inch.
    pub text_pos_after_bullet: f64,
    /// The stops of the `Tabs` row, in any order.
    pub tabs: Vec<TabStop>,
}

impl Default for ParaFormat {
    fn default() -> Self {
        ParaFormat {
            bullet: 0,
            bullet_font: String::new(),
            bullet_size: -1.0,
            bullet_string: String::new(),
            flags: 0,
            halign: 1,
            ind_first: 0.0,
            ind_left: 0.0,
            ind_right: 0.0,
            sp_after: 0.0,
            sp_before: 0.0,
            sp_line: -1.2,
            text_pos_after_bullet: 0.0,
            tabs: Vec::new(),
        }
    }
}

/// The cells of `Text Block Format`, and the block's height.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockFormat {
    pub left_margin: f64,
    pub right_margin: f64,
    pub top_margin: f64,
    pub bottom_margin: f64,
    /// `VerticalAlign`: 0 top, 1 middle, 2 bottom, 3 and above top.
    pub vertical_align: u8,
    /// `TextDirection`: 1 sets text down the block, laid out by the `vertical` module; anything else is across.
    pub text_direction: u8,
    /// `DefaultTabStop`, paper inches: automatic tab stops lie at whole multiples of it from the text area's left edge.
    pub default_tab_stop: f64,
    /// `TextBkgnd` resolved (`None` for 0 and 255) and `TextBkgndTrans`.
    pub background: Option<(Rgba, f64)>,
    /// The block's height (`TxtHeight` on paper), which vertical alignment and the overflow flag need. `None` lays the text out
    /// in a block exactly as tall as it and its margins, so nothing overflows.
    pub height: Option<f64>,
}

impl Default for BlockFormat {
    fn default() -> Self {
        BlockFormat {
            left_margin: 0.0,
            right_margin: 0.0,
            top_margin: 0.0,
            bottom_margin: 0.0,
            vertical_align: 1,
            text_direction: 0,
            default_tab_stop: 0.5,
            background: None,
            height: None,
        }
    }
}

/// A stretch of a shape's text between markers (`cp`, `pp`, `tp`, `fld`) with the formats in force for it. The runs given to a
/// layout are the text in order: a run ends where any marker falls, and a `fld`'s displayed characters are a run of their own.
#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub char: Arc<CharFormat>,
    pub para: Arc<ParaFormat>,
}

impl Run {
    pub fn new(text: impl Into<String>, char: Arc<CharFormat>, para: Arc<ParaFormat>) -> Run {
        Run { text: text.into(), char, para }
    }
}
