//! DiaCraft colour: sRGB colours, theme colour slots and DrawingML-style colour transforms
//! (tint, shade, luminance modulation/offset, alpha…), and the fixed palette of a colour index.
//!
//! The colour model is copied from DeckCraft's colour crate (`docs/provenance.md`). Cut: the colour-picker grid
//! (`theme_variants`, the standard colours), the preset and system colour name lists, which nothing here reads yet, and the dialog wording of
//! `SchemeSlot::label`, which had no user and no test. **The transforms are unverified:** they are the sibling's reading of how a theme's colours
//! change, and no spec of ours supports it, in progress, decides it, and the code is checked
//! against that spec when it lands. New: [`fixed_color`], the palette of the integers 0 to 23.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(kani)]
mod kani_proofs;
mod palette;

pub use palette::{FIXED_PALETTE, fixed_color};

use serde::{Deserialize, Serialize};

/// An sRGB colour with straight alpha, components 0–255.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::BLACK
    }
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }
    #[allow(clippy::self_named_constructors, reason = "the sibling apps' name for it; the colour crate's callers are copied from them")]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }
    /// `RRGGBB` or `#RRGGBB` (also `RRGGBBAA`).
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match s.len() {
            6 => Some(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Rgba::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            3 => {
                let n = |i: usize| s.get(i..i + 1).and_then(|h| u8::from_str_radix(h, 16).ok()).map(|v| v * 17);
                Some(Rgba::rgb(n(0)?, n(1)?, n(2)?))
            }
            _ => None,
        }
    }
    /// `RRGGBB` uppercase (no alpha).
    pub fn hex(&self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Rgba { a, ..self }
    }
    pub fn to_f32(self) -> [f32; 4] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0]
    }
    pub fn from_f64(r: f64, g: f64, b: f64, a: f64) -> Self {
        let c = |v: f64| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
        Rgba { r: c(r), g: c(g), b: c(b), a: c(a) }
    }
    /// Relative luminance (WCAG), 0–1.
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let v = c as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
    /// WCAG contrast ratio between two colours (1–21).
    pub fn contrast(self, other: Rgba) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
    pub fn to_hsl(self) -> (f64, f64, f64) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if (max - min).abs() < 1e-12 {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if (max - r).abs() < 1e-12 {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < 1e-12 {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }
    pub fn from_hsl(h: f64, s: f64, l: f64, a: u8) -> Self {
        let h = ((h % 360.0) + 360.0) % 360.0 / 360.0;
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        if s == 0.0 {
            return Rgba::from_f64(l, l, l, a as f64 / 255.0);
        }
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let f = |t: f64| {
            let t = if t < 0.0 {
                t + 1.0
            } else if t > 1.0 {
                t - 1.0
            } else {
                t
            };
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Rgba::from_f64(f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0), a as f64 / 255.0)
    }
    /// Linear interpolation (straight alpha).
    pub fn lerp(self, o: Rgba, t: f64) -> Rgba {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8;
        Rgba { r: m(self.r, o.r), g: m(self.g, o.g), b: m(self.b, o.b), a: m(self.a, o.a) }
    }
    /// The grey of the same lightness: red, green and blue each become the [`luma`] of the three, and alpha is kept. What a print in black and
    /// white draws for every colour.
    pub const fn gray(self) -> Rgba {
        let y = luma(self.r, self.g, self.b);
        Rgba { r: y, g: y, b: y, a: self.a }
    }
}

/// The luma of an sRGB colour, 0 to 255: `Y' = 0.299 R' + 0.587 G' + 0.114 B'`, the weights of ITU-R BT.601 on the encoded values, in
/// thousandths and rounded half up, in whole numbers so that it is the same on every machine. It lies between the least and the most of the
/// three (the weights add up to 1), so a grey (`r == g == b`) is its own luma. [`ColorTransform::Gray`] works the same weights in floating
/// point and gives the same grey, but for the 3,464 colours whose luma is exactly a half, which it rounds down.
pub const fn luma(r: u8, g: u8, b: u8) -> u8 {
    // At most 1000 * 255 + 500 before the division: no overflow, and the quotient is at most 255.
    let y = (299 * r as u32 + 587 * g as u32 + 114 * b as u32 + 500) / 1000;
    if y > 255 { 255 } else { y as u8 }
}

/// Turns pixels of four bytes (red, green, blue, alpha) to their [`luma`] in place, alpha kept. Straight or premultiplied alike: the luma is a
/// weighted mean, so a premultiplied pixel (no channel above its alpha) stays one, and turning pixels after they were blended gives what
/// turning each colour before would. Bytes past the last whole pixel are left as they are.
pub fn gray_pixels(rgba: &mut [u8]) {
    for [r, g, b, _] in rgba.as_chunks_mut::<4>().0 {
        let y = luma(*r, *g, *b);
        (*r, *g, *b) = (y, y, y);
    }
}

/// The twelve theme colour slots, plus the mapped aliases (`bg1`, `tx1`…) and `phClr` (the
/// placeholder colour a style matrix entry is applied with).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SchemeSlot {
    Dk1,
    Lt1,
    Dk2,
    Lt2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hlink,
    FolHlink,
    Bg1,
    Tx1,
    Bg2,
    Tx2,
    PhClr,
}

impl SchemeSlot {
    pub const THEME: [SchemeSlot; 12] = [
        SchemeSlot::Dk1,
        SchemeSlot::Lt1,
        SchemeSlot::Dk2,
        SchemeSlot::Lt2,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
        SchemeSlot::Hlink,
        SchemeSlot::FolHlink,
    ];
    pub fn xml_name(self) -> &'static str {
        match self {
            SchemeSlot::Dk1 => "dk1",
            SchemeSlot::Lt1 => "lt1",
            SchemeSlot::Dk2 => "dk2",
            SchemeSlot::Lt2 => "lt2",
            SchemeSlot::Accent1 => "accent1",
            SchemeSlot::Accent2 => "accent2",
            SchemeSlot::Accent3 => "accent3",
            SchemeSlot::Accent4 => "accent4",
            SchemeSlot::Accent5 => "accent5",
            SchemeSlot::Accent6 => "accent6",
            SchemeSlot::Hlink => "hlink",
            SchemeSlot::FolHlink => "folHlink",
            SchemeSlot::Bg1 => "bg1",
            SchemeSlot::Tx1 => "tx1",
            SchemeSlot::Bg2 => "bg2",
            SchemeSlot::Tx2 => "tx2",
            SchemeSlot::PhClr => "phClr",
        }
    }
    pub fn from_xml(s: &str) -> Option<Self> {
        Some(match s {
            "dk1" => SchemeSlot::Dk1,
            "lt1" => SchemeSlot::Lt1,
            "dk2" => SchemeSlot::Dk2,
            "lt2" => SchemeSlot::Lt2,
            "accent1" => SchemeSlot::Accent1,
            "accent2" => SchemeSlot::Accent2,
            "accent3" => SchemeSlot::Accent3,
            "accent4" => SchemeSlot::Accent4,
            "accent5" => SchemeSlot::Accent5,
            "accent6" => SchemeSlot::Accent6,
            "hlink" => SchemeSlot::Hlink,
            "folHlink" => SchemeSlot::FolHlink,
            "bg1" => SchemeSlot::Bg1,
            "tx1" => SchemeSlot::Tx1,
            "bg2" => SchemeSlot::Bg2,
            "tx2" => SchemeSlot::Tx2,
            "phClr" => SchemeSlot::PhClr,
            _ => return None,
        })
    }
}

/// A colour scheme: the twelve theme colours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorScheme {
    pub name: String,
    pub colors: [Rgba; 12],
}

impl ColorScheme {
    pub fn get(&self, slot: SchemeSlot) -> Rgba {
        let i = match slot {
            SchemeSlot::Dk1 | SchemeSlot::Tx1 => 0,
            SchemeSlot::Lt1 | SchemeSlot::Bg1 => 1,
            SchemeSlot::Dk2 | SchemeSlot::Tx2 => 2,
            SchemeSlot::Lt2 | SchemeSlot::Bg2 => 3,
            SchemeSlot::Accent1 => 4,
            SchemeSlot::Accent2 => 5,
            SchemeSlot::Accent3 => 6,
            SchemeSlot::Accent4 => 7,
            SchemeSlot::Accent5 => 8,
            SchemeSlot::Accent6 => 9,
            SchemeSlot::Hlink => 10,
            SchemeSlot::FolHlink => 11,
            SchemeSlot::PhClr => 4,
        };
        self.colors.get(i).copied().unwrap_or(Rgba::BLACK)
    }
    pub fn set(&mut self, slot: SchemeSlot, c: Rgba) {
        if let Some(i) = SchemeSlot::THEME.iter().position(|s| *s == slot)
            && let Some(v) = self.colors.get_mut(i)
        {
            *v = c;
        }
    }
}

/// One colour transform, in file units (percentages ×1000, angles ×60000), applied in order.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op", content = "val")]
pub enum ColorTransform {
    Tint(i32),
    Shade(i32),
    LumMod(i32),
    LumOff(i32),
    SatMod(i32),
    SatOff(i32),
    HueMod(i32),
    HueOff(i32),
    Alpha(i32),
    AlphaMod(i32),
    AlphaOff(i32),
    Comp,
    Inv,
    Gray,
    RedMod(i32),
    GreenMod(i32),
    BlueMod(i32),
}

impl ColorTransform {
    pub fn xml_name(&self) -> &'static str {
        match self {
            ColorTransform::Tint(_) => "tint",
            ColorTransform::Shade(_) => "shade",
            ColorTransform::LumMod(_) => "lumMod",
            ColorTransform::LumOff(_) => "lumOff",
            ColorTransform::SatMod(_) => "satMod",
            ColorTransform::SatOff(_) => "satOff",
            ColorTransform::HueMod(_) => "hueMod",
            ColorTransform::HueOff(_) => "hueOff",
            ColorTransform::Alpha(_) => "alpha",
            ColorTransform::AlphaMod(_) => "alphaMod",
            ColorTransform::AlphaOff(_) => "alphaOff",
            ColorTransform::Comp => "comp",
            ColorTransform::Inv => "inv",
            ColorTransform::Gray => "gray",
            ColorTransform::RedMod(_) => "redMod",
            ColorTransform::GreenMod(_) => "greenMod",
            ColorTransform::BlueMod(_) => "blueMod",
        }
    }
    pub fn value(&self) -> Option<i32> {
        match *self {
            ColorTransform::Tint(v)
            | ColorTransform::Shade(v)
            | ColorTransform::LumMod(v)
            | ColorTransform::LumOff(v)
            | ColorTransform::SatMod(v)
            | ColorTransform::SatOff(v)
            | ColorTransform::HueMod(v)
            | ColorTransform::HueOff(v)
            | ColorTransform::Alpha(v)
            | ColorTransform::AlphaMod(v)
            | ColorTransform::AlphaOff(v)
            | ColorTransform::RedMod(v)
            | ColorTransform::GreenMod(v)
            | ColorTransform::BlueMod(v) => Some(v),
            _ => None,
        }
    }
    pub fn from_xml(name: &str, val: Option<i32>) -> Option<Self> {
        let v = val.unwrap_or(0);
        Some(match name {
            "tint" => ColorTransform::Tint(v),
            "shade" => ColorTransform::Shade(v),
            "lumMod" => ColorTransform::LumMod(v),
            "lumOff" => ColorTransform::LumOff(v),
            "satMod" => ColorTransform::SatMod(v),
            "satOff" => ColorTransform::SatOff(v),
            "hueMod" => ColorTransform::HueMod(v),
            "hueOff" => ColorTransform::HueOff(v),
            "alpha" => ColorTransform::Alpha(v),
            "alphaMod" => ColorTransform::AlphaMod(v),
            "alphaOff" => ColorTransform::AlphaOff(v),
            "comp" => ColorTransform::Comp,
            "inv" => ColorTransform::Inv,
            "gray" => ColorTransform::Gray,
            "redMod" => ColorTransform::RedMod(v),
            "greenMod" => ColorTransform::GreenMod(v),
            "blueMod" => ColorTransform::BlueMod(v),
            _ => return None,
        })
    }
}

/// Apply transforms to a base colour.
pub fn apply(base: Rgba, transforms: &[ColorTransform]) -> Rgba {
    let mut c = base;
    for t in transforms {
        c = apply_one(c, *t);
    }
    c
}

fn pct(v: i32) -> f64 {
    v as f64 / 100_000.0
}

fn apply_one(c: Rgba, t: ColorTransform) -> Rgba {
    match t {
        // tint: towards white by (1 - value); shade: towards black by (1 - value), done in linear light. That is the sibling crate's
        // reading, and no spec of ours supports it: how a document's colour transforms act is unverified here, and a
        // specification in progress decides it. Until then this is a placeholder to check against that spec.
        ColorTransform::Tint(v) => {
            let k = pct(v).clamp(0.0, 1.0);
            map_linear(c, |x| x * k + (1.0 - k))
        }
        ColorTransform::Shade(v) => {
            let k = pct(v).clamp(0.0, 1.0);
            map_linear(c, |x| x * k)
        }
        ColorTransform::LumMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s, l * pct(v), c.a)
        }
        ColorTransform::LumOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s, l + pct(v), c.a)
        }
        ColorTransform::SatMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s * pct(v), l, c.a)
        }
        ColorTransform::SatOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s + pct(v), l, c.a)
        }
        ColorTransform::HueMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h * pct(v), s, l, c.a)
        }
        ColorTransform::HueOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h + v as f64 / 60_000.0, s, l, c.a)
        }
        ColorTransform::Alpha(v) => c.with_alpha((pct(v).clamp(0.0, 1.0) * 255.0).round() as u8),
        ColorTransform::AlphaMod(v) => c.with_alpha((c.a as f64 * pct(v)).round().clamp(0.0, 255.0) as u8),
        ColorTransform::AlphaOff(v) => c.with_alpha((c.a as f64 + pct(v) * 255.0).round().clamp(0.0, 255.0) as u8),
        ColorTransform::Comp => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h + 180.0, s, l, c.a)
        }
        ColorTransform::Inv => Rgba { r: 255 - c.r, g: 255 - c.g, b: 255 - c.b, a: c.a },
        ColorTransform::Gray => {
            let y = (0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64).round().clamp(0.0, 255.0) as u8;
            Rgba { r: y, g: y, b: y, a: c.a }
        }
        ColorTransform::RedMod(v) => Rgba { r: (c.r as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
        ColorTransform::GreenMod(v) => Rgba { g: (c.g as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
        ColorTransform::BlueMod(v) => Rgba { b: (c.b as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
    }
}

fn to_lin(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
fn from_lin(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}
fn map_linear(c: Rgba, f: impl Fn(f64) -> f64) -> Rgba {
    Rgba::from_f64(from_lin(f(to_lin(c.r))), from_lin(f(to_lin(c.g))), from_lin(f(to_lin(c.b))), c.a as f64 / 255.0)
}
