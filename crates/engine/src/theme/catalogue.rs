//! The themes DiaCraft ships. **They are originals** (`docs/provenance.md`): the names are coined here and none is a name of another
//! application's theme; the colours were picked by hand for these palettes and no colour of another application's theme was looked up or
//! copied (each palette is six accents and the dark, light and link colours that go with them, chosen together by hand and checked only against
//! the rule that the dark colours read on the light ones); the typefaces are open families (Liberation, DejaVu) that DiaCraft may name, since a face is named in a drawing and never embedded;
//! and the lists of formats (`kit.rs`) are plain numbers of our own.
//!
//! **What a theme is** here: a [`ThemeDef`] (`diacraft_vsdx::ThemeDef`), written into the drawing as a theme part when it is first used. The
//! five components of a theme carry one number (`scheme`), as the pages of real files hold one number in all five cells:
//! 1001 to 1005 for these, far from the 33 to 60 the samples' parts carry, so that a drawing that
//! holds both never selects the wrong part by the number. **Four variants** each: four colour variants (the accents in
//! their own order, one place on, one hue in seven tones, three places on) and four style variants (kit.rs), which `VariationColorIndex` and
//! `VariationStyleIndex` pick.

use std::sync::{Arc, OnceLock};

use diacraft_color::Rgba;
use diacraft_doc::{SchemeId, Theme};
use diacraft_vsdx::{ThemeDef, read_theme_part, theme_part_bytes};

use super::kit::{Look, background_fills, connector_background_fills, entries};

/// The names of the four variants a theme has, in the order `VariationColorIndex` and `VariationStyleIndex` number them.
pub const VARIANT_NAMES: [&str; 4] = ["Original", "Shifted", "Single hue", "Opposite"];

/// A theme of the catalogue: what the Themes gallery shows, and the part the drawing gets.
#[derive(Debug)]
pub struct CatalogueTheme {
    /// The id commands take: lower case letters.
    pub id: &'static str,
    /// The name the gallery shows.
    pub name: &'static str,
    /// The theme as a definition.
    pub def: ThemeDef,
    /// The part's bytes, as [`theme_part_bytes`] makes them.
    pub bytes: Arc<[u8]>,
}

impl CatalogueTheme {
    /// The number the theme's five components carry.
    pub fn scheme(&self) -> u32 {
        self.def.scheme.scheme_enum
    }

    /// The theme as the model holds one, named `part`.
    pub fn theme(&self, part: &str) -> Theme {
        read_theme_part(part, &self.bytes)
    }

    /// The twelve colours of the palette, in the order of `a:clrScheme`.
    pub fn colours(&self) -> [Rgba; 12] {
        self.def.colours
    }
}

/// A theme's recipe: the numbers it is made from.
struct Recipe {
    id: &'static str,
    name: &'static str,
    scheme: u32,
    /// dk1, lt1, dk2, lt2, accent1 to accent6, hlink, folHlink, as 0xRRGGBB.
    colours: [u32; 12],
    background: u32,
    major: &'static str,
    minor: &'static str,
    look: Look,
}

const fn rgb(hex: u32) -> Rgba {
    Rgba::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Thin lines, square corners, light shadows.
const PLAIN: Look = Look {
    widths: [12_700, 19_050, 28_575, 12_700, 6_350],
    rounding: 0,
    shadows: [Some((50_800, 25_400, 35_000)), Some((76_200, 38_100, 45_000)), Some((12_700, 12_700, 30_000))],
    gradient: (60_000, 82_000, 90.0),
};

/// Hairline weights, rounded corners, wide soft shadows.
const SOFT: Look = Look {
    widths: [9_525, 15_875, 25_400, 12_700, 6_350],
    rounding: 76_200,
    shadows: [Some((63_500, 25_400, 25_000)), Some((101_600, 38_100, 32_000)), Some((25_400, 12_700, 22_000))],
    gradient: (50_000, 88_000, 90.0),
};

/// Heavy lines, square corners, hard deep shadows, gradients on the diagonal.
const BOLD: Look = Look {
    widths: [19_050, 28_575, 38_100, 19_050, 9_525],
    rounding: 0,
    shadows: [Some((38_100, 38_100, 45_000)), Some((63_500, 50_800, 55_000)), Some((12_700, 19_050, 45_000))],
    gradient: (70_000, 70_000, 45.0),
};

/// Fine lines, square corners and no shadow at all.
const CRISP: Look =
    Look { widths: [9_525, 12_700, 19_050, 9_525, 6_350], rounding: 0, shadows: [None, None, None], gradient: (75_000, 90_000, 90.0) };

/// Soft, with rounder corners.
const ROUNDER: Look = Look {
    widths: [12_700, 19_050, 25_400, 12_700, 6_350],
    rounding: 114_300,
    shadows: [Some((50_800, 19_050, 22_000)), Some((88_900, 31_750, 30_000)), Some((19_050, 12_700, 20_000))],
    gradient: (55_000, 85_000, 90.0),
};

const RECIPES: [Recipe; 5] = [
    Recipe {
        id: "tidepool",
        name: "Tidepool",
        scheme: 1001,
        colours: [0x14232B, 0xFFFFFF, 0x1D4E5F, 0xE4F0F2, 0x1F8CA8, 0x2B5FA8, 0x4FB39A, 0xE5B04B, 0xD8644A, 0x8573C2, 0x1565A8, 0x6D5BA6],
        background: 0xF3F9FA,
        major: "Liberation Sans",
        minor: "Liberation Sans",
        look: SOFT,
    },
    Recipe {
        id: "fernwood",
        name: "Fernwood",
        scheme: 1002,
        colours: [0x1E2A20, 0xFFFFFF, 0x2F4B38, 0xEEF2E6, 0x4C9A52, 0x2F7A5C, 0xA7C957, 0xD6A53A, 0x9A6A42, 0x5BA3A0, 0x2B7A4B, 0x7B5E8C],
        background: 0xF7F8F1,
        major: "DejaVu Serif",
        minor: "DejaVu Sans",
        look: PLAIN,
    },
    Recipe {
        id: "cinderglow",
        name: "Cinderglow",
        scheme: 1003,
        colours: [0x2A1B17, 0xFFFFFF, 0x5A2E22, 0xF8EDE6, 0xD9532B, 0xE8912D, 0xB63A3A, 0xEFC24F, 0x7B4A3A, 0x5E7C8A, 0xB0412A, 0x7A4E7E],
        background: 0xFBF5F0,
        major: "Liberation Serif",
        minor: "Liberation Sans",
        look: BOLD,
    },
    Recipe {
        id: "inkstone",
        name: "Inkstone",
        scheme: 1004,
        colours: [0x1B1F27, 0xFFFFFF, 0x3A4150, 0xE9ECF0, 0x3E4A5E, 0x6F7C90, 0x9AA7B8, 0xE4572E, 0x17A398, 0xF2B134, 0x2F6FB3, 0x6B5B95],
        background: 0xF5F6F8,
        major: "DejaVu Sans",
        minor: "DejaVu Sans",
        look: CRISP,
    },
    Recipe {
        id: "plumline",
        name: "Plumline",
        scheme: 1005,
        colours: [0x231A2B, 0xFFFFFF, 0x4B2E63, 0xF3ECF7, 0x8E3FA6, 0x5B4DB1, 0xD1498A, 0xF28F6B, 0x3C9DBF, 0x7DB36A, 0x6A3CA0, 0x9A4F7E],
        background: 0xFAF6FC,
        major: "Liberation Sans",
        minor: "Liberation Sans",
        look: ROUNDER,
    },
];

fn build(r: &Recipe) -> Option<CatalogueTheme> {
    let colours = r.colours.map(rgb);
    let accents = [colours[4], colours[5], colours[6], colours[7], colours[8], colours[9]];
    let def = ThemeDef {
        name: r.name.to_owned(),
        // The GUID is ignored for every number but 65535; one is given because every sample's `vt:schemeID` has one.
        scheme: SchemeId { scheme_enum: r.scheme, scheme_guid: Some(Arc::from(format!("{{D1AC4AF7-0000-4000-8000-{:012X}}}", r.scheme).as_str())) },
        colours,
        major_font: r.major.to_owned(),
        minor_font: r.minor.to_owned(),
        entries: entries(&r.look, rgb(r.background), accents, colours[2]),
        effect_background_fills: background_fills(&r.look),
        connector_background_fills: connector_background_fills(),
    };
    let bytes: Arc<[u8]> = Arc::from(theme_part_bytes(&def).ok()?);
    Some(CatalogueTheme { id: r.id, name: r.name, def, bytes })
}

/// The themes, in the order the Themes gallery lists them. Made once.
pub fn catalogue() -> &'static [CatalogueTheme] {
    static THEMES: OnceLock<Vec<CatalogueTheme>> = OnceLock::new();
    THEMES.get_or_init(|| RECIPES.iter().filter_map(build).collect())
}

/// The theme with this id (`tidepool`), matched without regard to case.
pub fn find(id: &str) -> Option<&'static CatalogueTheme> {
    catalogue().iter().find(|t| t.id.eq_ignore_ascii_case(id))
}

/// The theme whose five components carry this number.
pub fn by_scheme(scheme: u32) -> Option<&'static CatalogueTheme> {
    catalogue().iter().find(|t| t.scheme() == scheme)
}
