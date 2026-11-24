//! The lists a theme's formats are made of: the fills, lines and effects the Quick Style matrices pick from, the lines' extensions, the fonts' and
//! the four style variants. They are plain numbers of our own choosing (`docs/provenance.md`: the themes are original), and a [`Look`] sets the
//! few that give a theme its character: how thick its lines are, how round its corners, how deep its shadows.
//!
//! **What an entry is for.** The matrices of a shape pick entries by number: 1 to 6 name one, and 100 to 103 go
//! through the variant (the `lineIdx`, `fillIdx`, `effectIdx` and `fontIdx` of the first to fourth `vt:varStyle`). The Theme style this crate
//! makes (`style.rs`) holds 100 in all four matrices, so a shape's look is its variant's first style, and a variant is one click.
//!
//! * **Fills**: 1 a plain colour, 2 a pale tint of it (the first style of the first variant: dark text stays readable), 3 a light-to-colour
//!   gradient, 4 a colour-to-dark gradient, 5 a dark shade, 6 white.
//! * **Lines**: 1 to 3 three weights in a darker shade of the colour, 4 dashed, 5 hairline, 6 none. **The variants use only 1 to 3**, because a
//!   connector's list holds three entries and an entry past a list reads the no-theme table,
//!   which would paint a connector in a colour that is none of the theme's.
//! * **Effects**: 1 and 2 none, 3 to 5 a soft, a deep and a tight shadow, 6 none. A look with no shadows has none.
//! * **Fonts**: 1 to 3 the colour the shape's font colour picks, 4 to 6 white (`lt1`), 6 bold.

use diacraft_color::{ColorTransform, Rgba, SchemeSlot};
use diacraft_doc::{
    ColourVariant, EffectEntry, FillEntry, FillProps, FontProps, GradientEntry, GradientKind, LineEntry, LineEx, ShadowEntry, StyleLists,
    StyleVariant, ThemeBase, ThemeColour, ThemeEntries, ThemeStop, VarStyle,
};

/// What gives a theme its character.
#[derive(Clone, Copy, Debug)]
pub(super) struct Look {
    /// The width of lines 1 to 5, in EMU (line 6 has none).
    pub widths: [i64; 5],
    /// The rounding of a corner or a bend, in EMU: `vt:lineEx`'s `rndg`.
    pub rounding: i64,
    /// Effects 3 to 5: blur, distance (EMU) and opacity (100000ths), or none.
    pub shadows: [Option<(i64, i64, i32)>; 3],
    /// Gradient 3 starts at this tint of the colour and gradient 4 ends at this shade of it (100000ths), at this angle (degrees).
    pub gradient: (i32, i32, f64),
}

fn slot(s: SchemeSlot, modifiers: Vec<ColorTransform>) -> ThemeColour {
    ThemeColour { base: ThemeBase::Slot(s), modifiers }
}

fn ph(modifiers: Vec<ColorTransform>) -> ThemeColour {
    slot(SchemeSlot::PhClr, modifiers)
}

fn solid(modifiers: Vec<ColorTransform>) -> FillEntry {
    FillEntry::Solid(ph(modifiers))
}

fn gradient(from: ThemeColour, to: ThemeColour, degrees: f64) -> FillEntry {
    FillEntry::Gradient(GradientEntry {
        stops: vec![ThemeStop { pos: 0.0, colour: from }, ThemeStop { pos: 1.0, colour: to }],
        kind: GradientKind::Linear { degrees },
        rotate_with_shape: Some(true),
    })
}

/// The six fills of the effect scheme.
fn fills(look: &Look) -> Vec<FillEntry> {
    let (light, dark, degrees) = look.gradient;
    vec![
        solid(vec![]),
        solid(vec![ColorTransform::Tint(40_000)]),
        gradient(ph(vec![ColorTransform::Tint(light)]), ph(vec![]), degrees),
        gradient(ph(vec![]), ph(vec![ColorTransform::Shade(dark)]), degrees),
        solid(vec![ColorTransform::Shade(70_000)]),
        FillEntry::Solid(slot(SchemeSlot::Lt1, vec![])),
    ]
}

fn line(width: i64, dash: &str, fill: FillEntry) -> LineEntry {
    LineEntry {
        width_emu: Some(width),
        cap: Some("flat".to_owned()),
        compound: Some("sng".to_owned()),
        dash: Some(dash.to_owned()),
        fill: Some(fill),
    }
}

/// The six lines of the effect scheme: a darker shade of the colour, which the first of a variant's styles pairs with a pale tint.
fn lines(look: &Look) -> Vec<LineEntry> {
    let w = look.widths;
    let edge = || solid(vec![ColorTransform::Shade(75_000)]);
    vec![
        line(w[0], "solid", edge()),
        line(w[1], "solid", edge()),
        line(w[2], "solid", edge()),
        line(w[3], "dash", edge()),
        line(w[4], "solid", edge()),
        line(w[0], "solid", FillEntry::NoFill),
    ]
}

fn shadow(blur: i64, dist: i64, alpha: i32) -> EffectEntry {
    EffectEntry {
        outer_shadow: Some(ShadowEntry {
            blur_emu: blur,
            dist_emu: dist,
            // 45 degrees: down and to the right (a bearing of 90 degrees moves a shadow straight down).
            dir: 2_700_000,
            scale_x: 1.0,
            scale_y: 1.0,
            colour: ThemeColour { base: ThemeBase::Rgb(Rgba::BLACK), modifiers: vec![ColorTransform::Alpha(alpha)] },
        }),
        has_inner_shadow: false,
    }
}

/// The six effects of the effect scheme.
fn effects(look: &Look) -> Vec<EffectEntry> {
    let pick = |i: usize| look.shadows.get(i).copied().flatten().map_or_else(EffectEntry::default, |(blur, dist, alpha)| shadow(blur, dist, alpha));
    vec![EffectEntry::default(), EffectEntry::default(), pick(0), pick(1), pick(2), EffectEntry::default()]
}

/// A `vt:lineEx`: the rounding, no line ends and a line pattern (1 solid, 2 dashed, 0 none). The connector scheme's carry the same.
fn line_ex(look: &Look, pattern: u32) -> Option<LineEx> {
    Some(LineEx { rounding_emu: Some(look.rounding), start: Some(0), start_size: Some(2), end: Some(0), end_size: Some(2), pattern: Some(pattern) })
}

/// `vt:lineStyles` run one ahead of the lines: position 1 belongs to no entry, and entry `n` reads position `n + 1`.
fn line_extensions(look: &Look, entries: u32, dashed: Option<u32>, none: Option<u32>) -> Vec<Option<LineEx>> {
    (0..=entries)
        .map(|n| {
            let pattern = if Some(n) == dashed {
                2
            } else if Some(n) == none {
                0
            } else {
                1
            };
            line_ex(look, pattern)
        })
        .collect()
}

fn font(style: u32, colour: ThemeColour) -> Option<FontProps> {
    Some(FontProps { style: Some(style), colour: Some(colour) })
}

/// The six font styles of the effect scheme and the three of the connector scheme.
fn fonts() -> (Vec<Option<FontProps>>, Vec<Option<FontProps>>) {
    let own = || ph(vec![]);
    let white = || slot(SchemeSlot::Lt1, vec![]);
    let shape = vec![font(0, own()), font(0, own()), font(0, own()), font(0, white()), font(0, white()), font(1, white())];
    (shape, vec![font(0, own()), font(0, own()), font(0, own())])
}

fn var_style(fill: u32, line: u32, effect: u32, font: u32) -> VarStyle {
    VarStyle { fill_idx: Some(fill), line_idx: Some(line), effect_idx: Some(effect), font_idx: Some(font) }
}

/// The four style variants, each four styles (the matrices 100 to 103), and the embellishment of each. Lines 1 to 3 only (the module documentation).
pub(super) fn style_variants() -> Vec<Option<StyleVariant>> {
    let variant = |embellishment: u32, styles: [VarStyle; 4]| Some(StyleVariant { embellishment: Some(embellishment), styles: styles.to_vec() });
    vec![
        // Original: a pale fill with the colour's outline, a dark fill with white text, a plain colour, white with a heavy outline.
        variant(0, [var_style(2, 1, 1, 1), var_style(5, 1, 1, 4), var_style(1, 1, 1, 1), var_style(6, 2, 1, 1)]),
        // Gradient: soft gradients with a soft shadow.
        variant(1, [var_style(3, 1, 3, 1), var_style(4, 2, 3, 4), var_style(3, 2, 3, 1), var_style(6, 2, 3, 1)]),
        // Outline: white or pale, thick outlines, no shadow.
        variant(2, [var_style(6, 3, 1, 1), var_style(2, 3, 1, 1), var_style(6, 2, 1, 1), var_style(1, 3, 1, 1)]),
        // Raised: the first variant's fills with a deeper shadow.
        variant(3, [var_style(2, 1, 4, 1), var_style(5, 1, 4, 4), var_style(1, 1, 5, 1), var_style(6, 1, 5, 1)]),
    ]
}

/// A colour mixed with another, `share` (0 to 1) of the way from `a` to `b`, in sRGB: the colours of the single-hue variant.
fn mix(a: Rgba, b: Rgba, share: f64) -> Rgba {
    let channel = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * share).round().clamp(0.0, 255.0) as u8;
    Rgba::rgb(channel(a.r, b.r), channel(a.g, b.g), channel(a.b, b.b))
}

/// The four colour variants of a palette: the accents in their own order, the accents one place on, one hue in seven tones (`monotone`), and
/// the accents three places on. The seventh colour of each is the theme's second dark colour.
pub(super) fn colour_variants(accents: [Rgba; 6], dk2: Rgba) -> Vec<Option<ColourVariant>> {
    let rotated = |by: usize| {
        let mut colours = [None; 7];
        for (i, slot) in colours.iter_mut().take(6).enumerate() {
            *slot = accents.get((i + by) % 6).copied();
        }
        colours[6] = Some(dk2);
        Some(ColourVariant { monotone: false, colours })
    };
    let base = accents[0];
    let tones = [
        base,
        mix(base, Rgba::WHITE, 0.25),
        mix(base, Rgba::WHITE, 0.5),
        mix(base, Rgba::BLACK, 0.25),
        mix(base, Rgba::BLACK, 0.5),
        mix(base, Rgba::WHITE, 0.75),
        dk2,
    ];
    vec![rotated(0), rotated(1), Some(ColourVariant { monotone: true, colours: tones.map(Some) }), rotated(3)]
}

/// Everything the theme functions read, for a look and a palette.
pub(super) fn entries(look: &Look, background: Rgba, accents: [Rgba; 6], dk2: Rgba) -> ThemeEntries {
    let (shape_fonts, connector_fonts) = fonts();
    let connector_edge = |w: i64| line(w, "solid", solid(vec![]));
    ThemeEntries {
        background: Some(background),
        colour_variants: colour_variants(accents, dk2),
        style_variants: style_variants(),
        effect: StyleLists {
            fills: fills(look).into_iter().map(Some).collect(),
            lines: lines(look).into_iter().map(Some).collect(),
            effects: effects(look).into_iter().map(Some).collect(),
        },
        connector: StyleLists {
            fills: vec![Some(solid(vec![])); 3],
            lines: vec![Some(connector_edge(look.widths[0])), Some(connector_edge(look.widths[1])), Some(connector_edge(look.widths[2]))],
            effects: vec![Some(EffectEntry::default()); 3],
        },
        line_ex: line_extensions(look, 6, Some(4), Some(6)),
        connector_line_ex: line_extensions(look, 3, None, None),
        fill_props: vec![Some(FillProps { pattern: Some(1) }); 6],
        fonts: shape_fonts,
        connector_fonts,
    }
}

/// The connector scheme's `a:bgFillStyleLst`: three plain colours.
pub(super) fn connector_background_fills() -> Vec<FillEntry> {
    vec![solid(vec![]); 3]
}

/// `a:bgFillStyleLst`: a plain colour, a gradient of it and a dark shade.
pub(super) fn background_fills(look: &Look) -> Vec<FillEntry> {
    let (light, _, degrees) = look.gradient;
    vec![
        solid(vec![]),
        gradient(ph(vec![ColorTransform::Tint(light)]), ph(vec![ColorTransform::Shade(90_000)]), degrees),
        solid(vec![ColorTransform::Shade(80_000)]),
    ]
}
