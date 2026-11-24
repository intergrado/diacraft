//! What the theme functions read from a theme part, as the reader took it: the colour
//! variants, the style variants, and the lists of `a:fmtScheme` and `vt:fmtConnectorScheme` with the VSDX extensions beside them.
//!
//! This is a **view of the bytes**, never their replacement: `Theme::bytes` stays verbatim and is what a save writes. A slot that could not be
//! read is `None` (a list position that is unreadable keeps its place, so that an entry number still names the entry it names), and the
//! reader warns. Every list is capped (the `MAX_` constants), so a hostile part cannot make the view larger than the bytes it came from.
//!
//! Units are the file's: EMU for widths and distances, 60000ths of a degree for angles, 100000ths for a modifier's value. What a cell makes of
//! them is `calc`'s.

use diacraft_color::{ColorTransform, Rgba, SchemeSlot};

/// Most entries kept of one list (a fill, line or effect list, a variant list, the list of a font group). Real lists hold 3 to 7
/// A part with more is read up to this and warned of. Labelled choice.
pub const MAX_THEME_ENTRIES: usize = 256;
/// Most gradient stops kept of one entry. Real entries hold 2 to 8. Labelled choice.
pub const MAX_THEME_STOPS: usize = 64;
/// Most modifiers kept of one colour. Real colours hold up to 2 or 3. Labelled choice.
pub const MAX_THEME_MODIFIERS: usize = 16;

/// What a theme colour starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeBase {
    /// An `a:srgbClr`, or an `a:sysClr` with its `lastClr`.
    Rgb(Rgba),
    /// An `a:schemeClr`: `phClr` (the placeholder the selected colour stands in for), `dk1`, `accent1`, `tx1` and the rest.
    Slot(SchemeSlot),
}

/// A colour of a theme entry: a base and the modifiers that follow it, in the order the part states them. Only the seven the functions
/// apply are kept (`tint`, `shade`, `lumMod`, `lumOff`, `satMod`, `satOff`, `alpha`).
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeColour {
    pub base: ThemeBase,
    pub modifiers: Vec<ColorTransform>,
}

/// One stop of a gradient entry: `pos` is 0 to 1 (`a:gs`'s `pos` over 100000).
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeStop {
    pub pos: f64,
    pub colour: ThemeColour,
}

/// The shape of a gradient entry: `a:lin` (with its angle in degrees, clockwise as DrawingML measures it) or `a:path`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        degrees: f64,
    },
    /// `a:path`'s `path` is `circle`.
    Circle,
    /// `rect`.
    Rect,
    /// `shape`.
    Shape,
    /// Neither `a:lin` nor `a:path`.
    Unstated,
}

/// A gradient entry of a fill or line list.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientEntry {
    pub stops: Vec<ThemeStop>,
    pub kind: GradientKind,
    /// `rotWithShape`, where the part states it.
    pub rotate_with_shape: Option<bool>,
}

/// What a fill (or the fill of a line) is in an entry.
#[derive(Clone, Debug, PartialEq)]
pub enum FillEntry {
    /// `a:solidFill`.
    Solid(ThemeColour),
    /// `a:gradFill`.
    Gradient(GradientEntry),
    /// `a:noFill`.
    NoFill,
    /// `a:pattFill`, `a:blipFill`, `a:grpFill`, or a fill the reader does not read: MS-VSDX says to ignore them.
    Other,
}

/// An entry of `a:lnStyleLst`: the `a:ln`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LineEntry {
    /// `w`, in EMU.
    pub width_emu: Option<i64>,
    /// `cap`: `rnd`, `flat` or `sq`.
    pub cap: Option<String>,
    /// `cmpd`.
    pub compound: Option<String>,
    /// `a:prstDash`'s `val`.
    pub dash: Option<String>,
    /// The `a:ln`'s fill. `None` where it holds none.
    pub fill: Option<FillEntry>,
}

/// An outer shadow of an effect entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowEntry {
    /// `blurRad`, in EMU.
    pub blur_emu: i64,
    /// `dist`, in EMU.
    pub dist_emu: i64,
    /// `dir`, in 60000ths of a degree.
    pub dir: i64,
    /// `sx` and `sy` over 100000, 1 where the part states none.
    pub scale_x: f64,
    pub scale_y: f64,
    pub colour: ThemeColour,
}

/// An entry of `a:effectStyleLst`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EffectEntry {
    pub outer_shadow: Option<ShadowEntry>,
    /// An `a:innerShdw` is there (and nothing is read from it).
    pub has_inner_shadow: bool,
}

/// The three lists of an `a:fmtScheme` or a `vt:fmtConnectorScheme`. A position that could not be read is `None`. `a:bgFillStyleLst` is
/// not read: MS-VSDX's viewer ignores it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StyleLists {
    pub fills: Vec<Option<FillEntry>>,
    pub lines: Vec<Option<LineEntry>>,
    pub effects: Vec<Option<EffectEntry>>,
}

/// A `vt:lineEx`: what the line list's entry says that DrawingML has no place for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LineEx {
    /// `rndg`, in EMU.
    pub rounding_emu: Option<i64>,
    pub start: Option<u32>,
    pub start_size: Option<u32>,
    pub end: Option<u32>,
    pub end_size: Option<u32>,
    pub pattern: Option<u32>,
}

/// A `vt:fillProps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FillProps {
    pub pattern: Option<u32>,
}

/// A `vt:fontProps`: `style` is bit flags as in a `Style` cell, and `colour` is the `vt:color`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontProps {
    pub style: Option<u32>,
    pub colour: Option<ThemeColour>,
}

/// A `vt:variationClrScheme`: `monotone` (false where absent) and the seven `vt:varColor`s.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourVariant {
    pub monotone: bool,
    pub colours: [Option<Rgba>; 7],
}

/// One `vt:varStyle` of a style variant: the entry numbers it picks, counting from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VarStyle {
    pub fill_idx: Option<u32>,
    pub line_idx: Option<u32>,
    pub effect_idx: Option<u32>,
    pub font_idx: Option<u32>,
}

/// A `vt:variationStyleScheme`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StyleVariant {
    pub embellishment: Option<u32>,
    pub styles: Vec<VarStyle>,
}

/// Everything of the part that the theme functions read, in one place (see the module).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ThemeEntries {
    /// `vt:bkgnd`, the thirteenth colour.
    pub background: Option<Rgba>,
    /// `vt:variationClrSchemeLst`. Empty where the part has none.
    pub colour_variants: Vec<Option<ColourVariant>>,
    /// `vt:variationStyleSchemeLst`. Empty where the part has none.
    pub style_variants: Vec<Option<StyleVariant>>,
    /// `a:fmtScheme`.
    pub effect: StyleLists,
    /// `vt:fmtConnectorScheme`.
    pub connector: StyleLists,
    /// `vt:fmtSchemeLineStyles` and `vt:fmtConnectorSchemeLineStyles`: one more entry than the line list.
    pub line_ex: Vec<Option<LineEx>>,
    pub connector_line_ex: Vec<Option<LineEx>>,
    /// `vt:fillStyles`.
    pub fill_props: Vec<Option<FillProps>>,
    /// `vt:fontStyles` and `vt:connectorFontStyles`.
    pub fonts: Vec<Option<FontProps>>,
    pub connector_fonts: Vec<Option<FontProps>>,
}

impl ThemeEntries {
    /// The lists of the effect scheme, or of the connector scheme.
    pub fn lists(&self, connector: bool) -> &StyleLists {
        if connector { &self.connector } else { &self.effect }
    }

    /// The `vt:lineEx` list that goes with that scheme's line list.
    pub fn line_ex_for(&self, connector: bool) -> &[Option<LineEx>] {
        if connector { &self.connector_line_ex } else { &self.line_ex }
    }

    /// The font list of that scheme.
    pub fn fonts_for(&self, connector: bool) -> &[Option<FontProps>] {
        if connector { &self.connector_fonts } else { &self.fonts }
    }
}
