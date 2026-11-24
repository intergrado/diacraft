//! The colours a theme gives a colour palette, and what picking one writes.
//!
//! **A theme pick is a formula, so that the colour follows the theme** (ours: no source gives what a
//! palette pick writes, and the specifications are silent on the form). A pick of a theme colour writes the cell as `THEMEGUARD(THEMEVAL("AccentColor2"))`, or with a
//! tint `THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))`:
//!
//! * `THEMEVAL("<name>")` is the call that answers a scheme colour of the page's theme (`Dark`, `Light`, `AccentColor` to `AccentColor6`,
//!   `BackgroundColor`) or a colour of the page's colour variant (`VariantColor1` to `VariantColor7`), and a theme that changes re-evaluates it (the
//!   recalculation of a theme works the whole page out again: `apply.rs`);
//! * `MSOTINT(colour, percent)` is the tint of a shade row (above 0 the lightness rises by that share of the room to the top, below 0 it falls by that
//!   share of itself, so "Lighter 40%" is 40 and "Darker 25%" is -25);
//! * `THEMEGUARD(...)` is the wrapper that marks a format cell as following the theme. It evaluates to its argument, and a later format replaces the
//!   formula, as it does a `THEMEVAL()`.
//!
//! A **Standard Colors** pick is not one of these: it is the fixed colour `RGB(r,g,b)`, which no theme changes.
//!
//! **The palette** is [`colours`]: **ten columns** (the six published images of the desktop palette show ten, the first four white, black, white and
//! black in every theme and the last six the accents) with the shade rows of each, seven variant colours and ten standard colours. **The third and fourth columns repeat the first two**
//! (Light and Dark), until a source settles what they are, and a pick of one writes the form of the first two: no file shows a pick of a third or a fourth column as anything but a `Light` or
//! a `Dark` form. Background is no column, though a command may still name it (`theme:background`). Each swatch is **worked out by the evaluator**, on a copy of the
//! drawing with the formula in a scratch cell of the page, so a swatch shows the colour the cell will hold: what the palette shows and what a pick writes cannot differ.

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, OperandMode, Value as Calc};
use diacraft_doc::{Cell, CellAddr, CellEdit, CellWrite, Document, SheetRef, WriteOrigin};

use super::apply::selection_of;
use crate::draw::Problem;
use crate::formatting::Rgb;

/// The steps the evaluator may spend on the palette of one page: a call of `THEMEVAL` is a handful, and the palette is sixty-one of them.
const PALETTE_STEPS: u64 = 4_000_000;

/// The prefix of a theme pick in a command's parameter: `theme:accent2:40`.
pub const PICK_PREFIX: &str = "theme:";

/// The largest tint a pick holds, in percent (holds `delta` to -100 to 100).
pub const MAX_TINT: i8 = 100;

/// A colour a theme names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSlot {
    /// `lt1`, the theme's light colour (`THEMEVAL("Light")`).
    Light,
    /// `dk1`, the theme's dark colour (`THEMEVAL("Dark")`).
    Dark,
    /// The theme's background (`THEMEVAL("BackgroundColor")`).
    Background,
    /// One of the six accents, 1 to 6 (`THEMEVAL("AccentColor")`, `"AccentColor2"` and on).
    Accent(u8),
    /// One of the seven colours of the page's colour variant, 1 to 7 (`THEMEVAL("VariantColor1")` and on).
    Variant(u8),
}

impl ThemeSlot {
    /// The name `THEMEVAL` is asked for the colour by (`AccentColor` is the first accent: there is no `AccentColor1`).
    pub fn themeval_name(self) -> String {
        match self {
            ThemeSlot::Light => "Light".to_owned(),
            ThemeSlot::Dark => "Dark".to_owned(),
            ThemeSlot::Background => "BackgroundColor".to_owned(),
            ThemeSlot::Accent(1) => "AccentColor".to_owned(),
            ThemeSlot::Accent(n) => format!("AccentColor{n}"),
            ThemeSlot::Variant(n) => format!("VariantColor{n}"),
        }
    }

    /// The word a command takes for it: `light`, `dark`, `background`, `accent1` to `accent6`, `variant1` to `variant7`.
    pub fn id(self) -> String {
        match self {
            ThemeSlot::Light => "light".to_owned(),
            ThemeSlot::Dark => "dark".to_owned(),
            ThemeSlot::Background => "background".to_owned(),
            ThemeSlot::Accent(n) => format!("accent{n}"),
            ThemeSlot::Variant(n) => format!("variant{n}"),
        }
    }

    /// The caption a palette gives it: `Accent 2`.
    pub fn label(self) -> String {
        match self {
            ThemeSlot::Light => "Light".to_owned(),
            ThemeSlot::Dark => "Dark".to_owned(),
            ThemeSlot::Background => "Background".to_owned(),
            ThemeSlot::Accent(n) => format!("Accent {n}"),
            ThemeSlot::Variant(n) => format!("Variant {n}"),
        }
    }

    /// The slot a word names: its [`id`](ThemeSlot::id), or the name `THEMEVAL` takes (`AccentColor2`), without regard to case.
    pub fn parse(word: &str) -> Option<ThemeSlot> {
        let word = word.trim().to_ascii_lowercase();
        let numbered = |prefix: &str, longer: &str, most: u8| -> Option<u8> {
            let digits = word.strip_prefix(longer).or_else(|| word.strip_prefix(prefix))?;
            let n: u8 = digits.parse().ok()?;
            (1..=most).contains(&n).then_some(n)
        };
        match word.as_str() {
            "light" => return Some(ThemeSlot::Light),
            "dark" => return Some(ThemeSlot::Dark),
            "background" | "backgroundcolor" => return Some(ThemeSlot::Background),
            // THEMEVAL's first accent has no number.
            "accentcolor" => return Some(ThemeSlot::Accent(1)),
            _ => {}
        }
        if let Some(n) = numbered("accent", "accentcolor", 6) {
            return Some(ThemeSlot::Accent(n));
        }
        numbered("variant", "variantcolor", 7).map(ThemeSlot::Variant)
    }
}

/// A colour picked from a theme: the colour and a tint of it, in percent (0 is the colour itself, above 0 lighter, below 0 darker).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePick {
    pub slot: ThemeSlot,
    pub tint: i8,
}

impl ThemePick {
    /// The pick a parameter's text names, after its `theme:` prefix: `accent2`, or `accent2:40` with a tint from -100 to 100.
    pub fn parse(text: &str) -> Result<ThemePick, String> {
        let (word, tint) = match text.split_once(':') {
            Some((word, tint)) => (word, Some(tint)),
            None => (text, None),
        };
        let slot = ThemeSlot::parse(word).ok_or_else(|| {
            format!("`{}` names no theme colour: use light, dark, background, accent1 to accent6 or variant1 to variant7", crate::error::shown(word))
        })?;
        let tint = match tint {
            None => 0,
            Some(t) => match t.trim().parse::<i8>() {
                Ok(n) if (-MAX_TINT..=MAX_TINT).contains(&n) => n,
                _ => return Err(format!("the tint `{}` must be a whole number from -100 (darker) to 100 (lighter)", crate::error::shown(t))),
            },
        };
        Ok(ThemePick { slot, tint })
    }

    /// The word a parameter takes for it: `theme:accent2:40`.
    pub fn word(self) -> String {
        if self.tint == 0 { format!("{PICK_PREFIX}{}", self.slot.id()) } else { format!("{PICK_PREFIX}{}:{}", self.slot.id(), self.tint) }
    }

    /// The formula the cell holds: `THEMEGUARD(THEMEVAL("AccentColor2"))`, or with a tint `THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))`.
    pub fn formula(self) -> String {
        let call = format!("THEMEVAL(\"{}\")", self.slot.themeval_name());
        if self.tint == 0 { format!("THEMEGUARD({call})") } else { format!("THEMEGUARD(MSOTINT({call},{}))", self.tint) }
    }

    /// What its swatch is called: `Accent 2`, `Accent 2, Lighter 40%`, `Accent 2, Darker 25%`.
    pub fn label(self) -> String {
        match shade_label(self.tint) {
            Some(shade) => format!("{}, {shade}", self.slot.label()),
            None => self.slot.label(),
        }
    }
}

/// The shade a tint is: `Lighter 40%`, `Darker 25%`, and none for the colour itself.
pub fn shade_label(tint: i8) -> Option<String> {
    match tint {
        0 => None,
        t if t > 0 => Some(format!("Lighter {t}%")),
        t => Some(format!("Darker {}%", -i16::from(t))),
    }
}

/// The tints of a column's five shade rows under its colour (the colour itself is the top row). Accents are lightened three times and darkened twice, light colours
/// darkened five times and dark colours lightened five times (the tints the files saved by the reference app of the sample files hold are 80, 60, 40, -25 and -50 on an accent, 5 to 50 on
/// `Dark` and -5 to -50 on `Light`).
pub fn shades(slot: ThemeSlot) -> [i8; 5] {
    match slot {
        ThemeSlot::Light | ThemeSlot::Background => [-5, -15, -25, -35, -50],
        ThemeSlot::Dark => [50, 35, 25, 15, 5],
        ThemeSlot::Accent(_) | ThemeSlot::Variant(_) => [80, 60, 40, -25, -50],
    }
}

/// The columns of the Theme Colors section, left to right: ten. The third and fourth repeat the first two until a source settles what they are.
pub const COLUMNS: [ThemeSlot; 10] = [
    ThemeSlot::Light,
    ThemeSlot::Dark,
    ThemeSlot::Light,
    ThemeSlot::Dark,
    ThemeSlot::Accent(1),
    ThemeSlot::Accent(2),
    ThemeSlot::Accent(3),
    ThemeSlot::Accent(4),
    ThemeSlot::Accent(5),
    ThemeSlot::Accent(6),
];

/// A standard colour: our own set of ten (ten swatches and no page lists them), picked for this application, from dark
/// red round to purple.
pub const STANDARD_COLOURS: [(&str, Rgb); 10] = [
    ("Dark red", Rgb(0xA6, 0x1C, 0x1C)),
    ("Red", Rgb(0xE5, 0x39, 0x35)),
    ("Orange", Rgb(0xF5, 0x7C, 0x00)),
    ("Yellow", Rgb(0xFD, 0xD8, 0x35)),
    ("Light green", Rgb(0x8B, 0xC3, 0x4A)),
    ("Green", Rgb(0x2E, 0x9E, 0x4F)),
    ("Light blue", Rgb(0x29, 0xB6, 0xF6)),
    ("Blue", Rgb(0x1E, 0x6F, 0xD9)),
    ("Dark blue", Rgb(0x17, 0x35, 0x8F)),
    ("Purple", Rgb(0x7B, 0x3F, 0xB5)),
];

/// The name of a colour in words, for a swatch's tooltip: `Turquoise` ("Turquoise, Accent 6, Darker 25%"). The names are ours, by hue,
/// with `Light` or `Dark` in front of a hue that is very light or dark, and the greys by lightness.
pub fn colour_name(Rgb(r, g, b): Rgb) -> String {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (l, d) = ((max + min) / 2.0, max - min);
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    if s < 0.12 {
        return match l {
            l if l < 0.1 => "Black",
            l if l < 0.35 => "Dark grey",
            l if l < 0.7 => "Grey",
            l if l < 0.93 => "Light grey",
            _ => "White",
        }
        .to_owned();
    }
    let hue = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let base = match hue {
        h if !(15.0..345.0).contains(&h) => "red",
        h if h < 40.0 => "orange",
        h if h < 65.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 200.0 => "turquoise",
        h if h < 255.0 => "blue",
        h if h < 290.0 => "purple",
        _ => "pink",
    };
    let prefix = if l > 0.8 {
        "Light "
    } else if l < 0.28 {
        "Dark "
    } else {
        ""
    };
    let word = format!("{prefix}{base}");
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or(word)
}

/// One swatch of the palette: what it is called, the colour it comes to on this page, and what picking it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub pick: ThemePick,
    /// The colour the formula works out to, or `None` where the page's theme cannot answer (a drawing's own theme part that lacks it).
    pub colour: Option<Rgb>,
}

impl Swatch {
    /// The tooltip: the name of the colour, the theme's name for it and its shade: `Turquoise, Accent 6, Darker 25%`.
    pub fn tooltip(&self) -> String {
        match self.colour {
            Some(colour) => format!("{}, {}", colour_name(colour), self.pick.label()),
            None => self.pick.label(),
        }
    }
}

/// A column of the Theme Colors section: the colour and its five shades under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    /// Where the column stands, 1 to 10, left to right: two columns name the same colour (the third repeats the first), so the slot alone does not say which is which.
    pub column: usize,
    pub slot: ThemeSlot,
    /// The colour itself, then its shades, top to bottom (six swatches).
    pub swatches: Vec<Swatch>,
}

/// What a palette shows for a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub page: u32,
    /// Whether the page selects a theme. With none, the colours are the table that `THEMEVAL` gives with no theme, which is what a pick would be worked out to.
    pub themed: bool,
    pub columns: Vec<Column>,
    /// The seven colours of the page's colour variant.
    pub variants: Vec<Swatch>,
}

/// The colours the formulas of `picks` work out to on `page` of `doc`, in order. The formulas are put in scratch cells of a copy of the page's sheet, so the drawing is not touched, and
/// each is evaluated by `calc` as the cell would be.
pub fn evaluate_picks(doc: &Document, page: u32, picks: &[ThemePick]) -> Result<Vec<Option<Rgb>>, Problem> {
    if doc.page(page).is_none() {
        return Err(Problem::bad(format!("there is no page {page}")));
    }
    let mut scratch = doc.clone();
    let name = |n: usize| format!("ScratchColor{n}");
    let writes: Vec<CellWrite> = picks
        .iter()
        .enumerate()
        .map(|(n, pick)| CellWrite {
            addr: CellAddr::direct(&name(n)),
            edit: CellEdit::Cell(Cell::with_formula(&pick.formula(), None)),
            origin: WriteOrigin::Drawing,
        })
        .collect();
    scratch.write_cells(&SheetRef::Page(page), writes)?;
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, Budget::with_steps(PALETTE_STEPS));
    let mut evaluator = Evaluator::new(&scratch, &mut ctx);
    Ok((0..picks.len())
        .map(|n| match evaluator.evaluate(&CellAddress::new(SheetRef::Page(page), CellAddr::direct(&name(n)))) {
            Calc::Color(c) => Some(Rgb(c.r, c.g, c.b)),
            _ => None,
        })
        .collect())
}

/// The colour a pick comes to on `page`.
pub fn pick_colour(doc: &Document, page: u32, pick: ThemePick) -> Option<Rgb> {
    evaluate_picks(doc, page, &[pick]).ok().and_then(|found| found.first().copied().flatten())
}

/// The palette of `page`: the Theme Colors columns with their shades and the seven Variant Colors, each worked out as a cell holding the pick's formula would be. A colour two columns
/// share is worked out once and its swatches given to both.
pub fn colours(doc: &Document, page: u32) -> Result<Palette, Problem> {
    let themed = selection_of(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?.is_themed();
    // The colours the columns name, each once and in the order of their first column: the ten columns name eight.
    let mut slots: Vec<ThemeSlot> = Vec::with_capacity(COLUMNS.len());
    for slot in COLUMNS {
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    let mut picks: Vec<ThemePick> = Vec::with_capacity(slots.len() * 6 + 7);
    for slot in &slots {
        picks.push(ThemePick { slot: *slot, tint: 0 });
        picks.extend(shades(*slot).into_iter().map(|tint| ThemePick { slot: *slot, tint }));
    }
    picks.extend((1..=7).map(|n| ThemePick { slot: ThemeSlot::Variant(n), tint: 0 }));
    let found = evaluate_picks(doc, page, &picks)?;
    let mut swatches = picks.into_iter().zip(found).map(|(pick, colour)| Swatch { pick, colour });
    let worked: Vec<(ThemeSlot, Vec<Swatch>)> = slots.iter().map(|slot| (*slot, swatches.by_ref().take(6).collect())).collect();
    let variants = swatches.collect();
    let columns = COLUMNS
        .iter()
        .enumerate()
        .map(|(at, slot)| Column {
            column: at.saturating_add(1),
            slot: *slot,
            swatches: worked.iter().find(|(held, _)| held == slot).map(|(_, found)| found.clone()).unwrap_or_default(),
        })
        .collect();
    Ok(Palette { page, themed, columns, variants })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_names_a_slot_in_either_spelling() {
        assert_eq!(ThemeSlot::parse("accent2"), Some(ThemeSlot::Accent(2)));
        assert_eq!(ThemeSlot::parse("AccentColor2"), Some(ThemeSlot::Accent(2)));
        assert_eq!(ThemeSlot::parse("AccentColor"), Some(ThemeSlot::Accent(1)));
        assert_eq!(ThemeSlot::parse(" BACKGROUND "), Some(ThemeSlot::Background));
        assert_eq!(ThemeSlot::parse("variant7"), Some(ThemeSlot::Variant(7)));
        assert_eq!(ThemeSlot::parse("VariantColor3"), Some(ThemeSlot::Variant(3)));
        for no in ["accent0", "accent7", "variant8", "variant0", "accent", "", "accent-1", "accent 2", "accent2x"] {
            assert_eq!(ThemeSlot::parse(no), None, "{no:?}");
        }
    }

    #[test]
    fn every_slot_round_trips_through_its_id_and_its_themeval_name() {
        let all = COLUMNS.iter().copied().chain((1..=7).map(ThemeSlot::Variant));
        for slot in all {
            assert_eq!(ThemeSlot::parse(&slot.id()), Some(slot));
            assert_eq!(ThemeSlot::parse(&slot.themeval_name()), Some(slot));
        }
    }

    #[test]
    fn a_pick_is_written_as_a_theme_guarded_call_of_themeval_with_the_tint_of_its_row() {
        let pick = |slot, tint| ThemePick { slot, tint };
        assert_eq!(pick(ThemeSlot::Accent(2), 0).formula(), r#"THEMEGUARD(THEMEVAL("AccentColor2"))"#);
        assert_eq!(pick(ThemeSlot::Accent(1), 0).formula(), r#"THEMEGUARD(THEMEVAL("AccentColor"))"#);
        assert_eq!(pick(ThemeSlot::Accent(2), 40).formula(), r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor2"),40))"#);
        assert_eq!(pick(ThemeSlot::Accent(6), -25).formula(), r#"THEMEGUARD(MSOTINT(THEMEVAL("AccentColor6"),-25))"#);
        assert_eq!(pick(ThemeSlot::Variant(3), 0).formula(), r#"THEMEGUARD(THEMEVAL("VariantColor3"))"#);
        assert_eq!(pick(ThemeSlot::Background, 0).formula(), r#"THEMEGUARD(THEMEVAL("BackgroundColor"))"#);
    }

    #[test]
    fn a_pick_parses_from_its_word_and_the_word_round_trips() {
        for text in ["accent2", "accent2:40", "dark:-5", "light:-100", "variant4:100", "background"] {
            let pick = ThemePick::parse(text).unwrap();
            assert_eq!(pick.word(), format!("{PICK_PREFIX}{text}"));
        }
        let refusals = ["accent9", "accent2:101", "accent2:-101", "accent2:x", "accent2:", "accent2:40:3", "nope", ""];
        for text in refusals {
            assert!(ThemePick::parse(text).is_err(), "{text:?}");
        }
        let words = ThemePick::parse("accent2:200").unwrap_err();
        assert!(words.contains("-100") && words.contains("100"), "{words}");
    }

    #[test]
    fn a_swatch_is_named_by_colour_slot_and_shade() {
        let pick = ThemePick { slot: ThemeSlot::Accent(6), tint: -25 };
        assert_eq!(pick.label(), "Accent 6, Darker 25%");
        assert_eq!(ThemePick { slot: ThemeSlot::Accent(6), tint: 60 }.label(), "Accent 6, Lighter 60%");
        assert_eq!(ThemePick { slot: ThemeSlot::Dark, tint: 0 }.label(), "Dark");
        let swatch = Swatch { pick, colour: Some(Rgb(0x1A, 0xA3, 0xB0)) };
        assert_eq!(swatch.tooltip(), "Turquoise, Accent 6, Darker 25%");
        assert_eq!(Swatch { pick, colour: None }.tooltip(), "Accent 6, Darker 25%");
    }

    #[test]
    fn colours_are_named_by_hue_and_greys_by_lightness() {
        assert_eq!(colour_name(Rgb(0, 0, 0)), "Black");
        assert_eq!(colour_name(Rgb(255, 255, 255)), "White");
        assert_eq!(colour_name(Rgb(128, 128, 128)), "Grey");
        assert_eq!(colour_name(Rgb(255, 0, 0)), "Red");
        assert_eq!(colour_name(Rgb(0, 160, 0)), "Green");
        assert_eq!(colour_name(Rgb(0, 0, 255)), "Blue");
        assert_eq!(colour_name(Rgb(255, 235, 235)), "Light red");
        assert_eq!(colour_name(Rgb(40, 0, 0)), "Dark red");
    }

    #[test]
    fn the_columns_are_light_dark_light_dark_and_the_six_accents_and_the_shade_ladders_are_those_of_the_sample_files() {
        // Ten columns, the first four white, black, white and black in every theme, the last six Accent 1 to 6.
        let want = [
            ThemeSlot::Light,
            ThemeSlot::Dark,
            ThemeSlot::Light,
            ThemeSlot::Dark,
            ThemeSlot::Accent(1),
            ThemeSlot::Accent(2),
            ThemeSlot::Accent(3),
            ThemeSlot::Accent(4),
            ThemeSlot::Accent(5),
            ThemeSlot::Accent(6),
        ];
        assert_eq!(COLUMNS, want);
        assert_eq!(shades(ThemeSlot::Light), [-5, -15, -25, -35, -50]);
        assert_eq!(shades(ThemeSlot::Dark), [50, 35, 25, 15, 5]);
        for n in 1..=6 {
            assert_eq!(shades(ThemeSlot::Accent(n)), [80, 60, 40, -25, -50]);
        }
        // Background is still a colour a command can name, though no column shows it.
        assert_eq!(ThemeSlot::parse("background"), Some(ThemeSlot::Background));
        assert!(!COLUMNS.contains(&ThemeSlot::Background));
    }

    #[test]
    fn a_palette_has_ten_columns_that_know_their_place_and_a_repeated_colour_is_worked_out_once() {
        let doc = Document::new_blank();
        let palette = colours(&doc, 1).unwrap();
        assert_eq!(palette.columns.iter().map(|c| c.column).collect::<Vec<_>>(), (1..=10).collect::<Vec<_>>());
        assert!(palette.columns.iter().all(|c| c.swatches.len() == 6));
        assert_eq!(palette.columns[2].swatches, palette.columns[0].swatches);
        assert_eq!(palette.columns[3].swatches, palette.columns[1].swatches);
        assert_eq!(palette.variants.len(), 7);
    }

    #[test]
    fn the_shade_rows_are_five_and_the_standard_colours_ten() {
        for slot in COLUMNS {
            assert_eq!(shades(slot).len(), 5);
        }
        assert_eq!(STANDARD_COLOURS.len(), 10);
        let mut seen: Vec<Rgb> = STANDARD_COLOURS.iter().map(|(_, c)| *c).collect();
        seen.sort_by_key(|c| (c.0, c.1, c.2));
        seen.dedup();
        assert_eq!(seen.len(), 10, "ten different colours");
    }
}
