//! Colours and colour keywords (S-R17).
//!
//! A colour value is `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`; `rgb()`, `rgba()`, `hsl()` or `hsla()` with commas or spaces; a CSS colour name (any case of ASCII letters; the 148 of
//! [`css_names`], and `transparent`); `light-dark(a, b)`, whose first colour is the light-mode one (the importer has no dark mode); or one of six keywords that name a colour by the
//! rules of the cell and its parent: `default`, `inherit`, `fillColor`, `strokeColor`, `parentFillColor` and `parentStrokeColor`. [`parse`] reads one into a [`Spec`]; the keywords are
//! resolved against the cell and its parent by `Style::paints`, one level and never a loop.
//!
//! **A colour is short.** A value longer than [`MAX_COLOUR_BYTES`] is no colour, before any of it is looked at, so that a megabyte of `light-dark(` or of spaces costs nothing. Nothing
//! here recurses: `light-dark` takes the first colour of its arguments and that one may not be a `light-dark`, and the arguments of a function are at most four.

use super::css_names;
use crate::decode::{hex_digit, trim_js};
use crate::style::keys::{Key, parse_decimal};

/// The longest colour value, in bytes: 256 holds `light-dark(rgba(255, 255, 255, 0.5), rgba(0, 0, 0, 0.5))` three times over. A longer value is a bad value.
pub const MAX_COLOUR_BYTES: usize = 256;

/// A colour: sRGB bytes and an opacity (255 is opaque).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Opacity, 0 (clear) to 255 (opaque).
    pub a: u8,
}

impl Colour {
    /// Black, `#000000`: the light-mode `default` of `fontColor`, `strokeColor` and `gradientColor` (S-R7).
    pub const BLACK: Colour = Colour::rgb(0, 0, 0);
    /// White, `#ffffff`: the light-mode `default` of `fillColor`, `labelBackgroundColor` and `swimlaneFillColor` (S-R7).
    pub const WHITE: Colour = Colour::rgb(255, 255, 255);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Colour {
        Colour { r, g, b, a: 255 }
    }

    /// `#RRGGBB`, or `#RRGGBBAA` when it is not opaque.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

/// What a colour value says, before the cell's own keywords are resolved (S-R17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    /// A colour of its own.
    Colour(Colour),
    /// `default`: the light-mode colour of the key it is set on (S-R7).
    Default,
    /// `inherit`: the same key of the parent cell.
    Inherit,
    /// `fillColor`: the cell's own fill.
    Fill,
    /// `strokeColor`: the cell's own line.
    Stroke,
    /// `parentFillColor`: the parent's fill.
    ParentFill,
    /// `parentStrokeColor`: the parent's line.
    ParentStroke,
}

/// The light-mode colour `default` is for `key` (S-R7): white for the fills and backgrounds, black for the lines, the text and the end of a gradient.
pub fn default_colour(key: Key) -> Colour {
    match key {
        Key::FillColor | Key::LabelBackgroundColor | Key::SwimlaneFillColor => Colour::WHITE,
        _ => Colour::BLACK,
    }
}

/// The colour value `text` says, or `None` when it is no colour (S-R17: the caller reads it as the key's default, with a warning). `light-dark(a, b)` is `a`.
pub fn parse(text: &str) -> Option<Spec> {
    if text.len() > MAX_COLOUR_BYTES {
        return None;
    }
    if let Some(args) = function_args(text, "light-dark") {
        let (light, _dark) = split_top_level_comma(args)?;
        return parse_plain(trim_js(light));
    }
    parse_plain(text)
}

/// A colour that is not a `light-dark`: a keyword, a hexadecimal colour, a function or a name.
fn parse_plain(text: &str) -> Option<Spec> {
    match text {
        "default" => return Some(Spec::Default),
        "inherit" => return Some(Spec::Inherit),
        "fillColor" => return Some(Spec::Fill),
        "strokeColor" => return Some(Spec::Stroke),
        "parentFillColor" => return Some(Spec::ParentFill),
        "parentStrokeColor" => return Some(Spec::ParentStroke),
        _ => {}
    }
    if let Some(digits) = text.strip_prefix('#') {
        return parse_hex(digits).map(Spec::Colour);
    }
    if text.eq_ignore_ascii_case("transparent") {
        return Some(Spec::Colour(Colour { r: 0, g: 0, b: 0, a: 0 }));
    }
    if let Some(args) = function_args(text, "rgba").or_else(|| function_args(text, "rgb")) {
        return parse_rgb(args).map(Spec::Colour);
    }
    if let Some(args) = function_args(text, "hsla").or_else(|| function_args(text, "hsl")) {
        return parse_hsl(args).map(Spec::Colour);
    }
    css_names::lookup(text).map(|[r, g, b]| Spec::Colour(Colour::rgb(r, g, b)))
}

/// The text between the brackets of `name(...)`, when `text` is that call: the name in any case of ASCII letters, the bracket right after it and the closing one last.
fn function_args<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let head = text.as_bytes().get(..name.len())?;
    if !head.eq_ignore_ascii_case(name.as_bytes()) {
        return None;
    }
    // The first `name.len()` bytes are ASCII, so the cut is on a character.
    let rest = text.get(name.len()..)?.strip_prefix('(')?;
    trim_js(rest).strip_suffix(')')
}

/// `args` cut at its first comma outside brackets, or `None` when it has none.
fn split_top_level_comma(args: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (i, b) in args.bytes().enumerate() {
        match b {
            b'(' => depth = depth.saturating_add(1),
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => return Some((args.get(..i)?, args.get(i + 1..)?)),
            _ => {}
        }
    }
    None
}

/// The digits of `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
fn parse_hex(digits: &str) -> Option<Colour> {
    let bytes = digits.as_bytes();
    if !matches!(bytes.len(), 3 | 4 | 6 | 8) {
        return None;
    }
    let mut v = [0u8; 8];
    for (slot, b) in v.iter_mut().zip(bytes) {
        *slot = hex_digit(*b)?;
    }
    let [a, b, c, d, e, f, g, h] = v;
    Some(if bytes.len() <= 4 {
        // A single digit stands for both: #f is #ff.
        Colour { r: a * 17, g: b * 17, b: c * 17, a: if bytes.len() == 4 { d * 17 } else { 255 } }
    } else {
        Colour { r: a << 4 | b, g: c << 4 | d, b: e << 4 | f, a: if bytes.len() == 8 { g << 4 | h } else { 255 } }
    })
}

/// The three or four parts of a function's arguments, cut at commas, slashes and white space: `255, 0, 0`, `255 0 0 / 50%`.
fn parts(args: &str) -> Option<([&str; 4], usize)> {
    let mut out = [""; 4];
    let mut n = 0usize;
    for part in args.split(|c: char| c == ',' || c == '/' || c.is_ascii_whitespace()).filter(|p| !p.is_empty()) {
        *out.get_mut(n)? = part;
        n += 1;
    }
    (n == 3 || n == 4).then_some((out, n))
}

/// A byte from a number: rounded, and held to 0 to 255. NaN is 0.
pub fn to_u8(v: f64) -> u8 {
    if v.is_nan() { 0 } else { v.clamp(0.0, 255.0).round() as u8 }
}

/// A part that is a number, or a percentage of `whole`.
fn share(part: &str, whole: f64) -> Option<f64> {
    match part.strip_suffix('%') {
        Some(percent) => parse_decimal(percent).map(|p| p / 100.0 * whole),
        None => parse_decimal(part),
    }
}

/// An opacity part: a number 0 to 1 or a percentage, as a byte.
fn alpha(part: &str) -> Option<u8> {
    share(part, 1.0).map(|a| to_u8(a.clamp(0.0, 1.0) * 255.0))
}

/// The arguments of `rgb()` or `rgba()`.
fn parse_rgb(args: &str) -> Option<Colour> {
    let ([r, g, b, a], n) = parts(args)?;
    Some(Colour { r: to_u8(share(r, 255.0)?), g: to_u8(share(g, 255.0)?), b: to_u8(share(b, 255.0)?), a: if n == 4 { alpha(a)? } else { 255 } })
}

/// The arguments of `hsl()` or `hsla()`: a hue in degrees (or turns), a saturation and a lightness, in percent.
fn parse_hsl(args: &str) -> Option<Colour> {
    let ([h, s, l, a], n) = parts(args)?;
    let hue =
        if let Some(turns) = h.strip_suffix("turn") { parse_decimal(turns)? * 360.0 } else { parse_decimal(h.strip_suffix("deg").unwrap_or(h))? };
    // A number with no percent sign is read as a percentage (CSS Color 4).
    let percent = |p: &str| parse_decimal(p.strip_suffix('%').unwrap_or(p)).map(|v| v / 100.0);
    let [r, g, b] = hsl_to_rgb(hue, percent(s)?, percent(l)?);
    Some(Colour { r: to_u8(r * 255.0), g: to_u8(g * 255.0), b: to_u8(b * 255.0), a: if n == 4 { alpha(a)? } else { 255 } })
}

/// The red, green and blue, each 0 to 1, of the colour with hue `h` degrees (any number: it wraps), saturation `s` and lightness `l` (each held to 0 to 1).
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let clamp = |v: f64| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    let (s, l) = (clamp(s), clamp(l));
    let h = if h.is_finite() { h.rem_euclid(360.0) } else { 0.0 };
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let sector = h / 60.0;
    let x = c * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [clamp(r + m), clamp(g + m), clamp(b + m)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_gives_the_primaries() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), [1.0, 0.0, 0.0]);
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), [0.0, 1.0, 0.0]);
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), [0.0, 0.0, 1.0]);
        assert_eq!(hsl_to_rgb(-120.0, 1.0, 0.5), [0.0, 0.0, 1.0]);
        for n in [0.0, 0.5, 1.0] {
            let [r, g, b] = hsl_to_rgb(f64::NAN, n, n);
            assert!([r, g, b].iter().all(|c| (0.0..=1.0).contains(c)));
        }
    }

    #[test]
    fn a_byte_is_rounded_and_held() {
        assert_eq!(
            [to_u8(-3.0), to_u8(0.4), to_u8(0.5), to_u8(254.5), to_u8(1e300), to_u8(f64::NAN), to_u8(f64::INFINITY)],
            [0, 0, 1, 255, 255, 0, 255]
        );
    }
}
