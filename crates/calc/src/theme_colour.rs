//! The colour modifiers of a theme entry: `tint` and `shade` in linear light, `lumMod`,
//! `lumOff`, `satMod` and `satOff` on real-number HSL, `alpha` leaving the colour, applied in the order they stand on channels from 0 to 1,
//! and the result rounded to the nearest whole byte **once, at the end**.
//!
//! Worked examples of ECMA-376 and the spec: `tint` and `shade` of 50 percent on `#00FF00` are `#BCFFBC` and `#00BC00`, `satMod` of 20 percent is
//! `#669966`, and `lumMod` of 50 percent is `#008000` (HSL arithmetic, not ECMA-376's `#007500`).

use diacraft_color::ColorTransform;

use crate::value::Rgb;

/// A colour on channels from 0 to 1.
type Chan = [f64; 3];

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Hue (0 to 1), saturation and lightness of a colour, in real numbers.
fn to_hsl([r, g, b]: Chan) -> (f64, f64, f64) {
    let (top, low) = (r.max(g).max(b), r.min(g).min(b));
    let l = (top + low) / 2.0;
    let span = top - low;
    if span <= 0.0 {
        return (0.0, 0.0, l);
    }
    let s = if l <= 0.5 { span / (top + low) } else { span / (2.0 - top - low) };
    let sector = if top == r {
        (g - b) / span
    } else if top == g {
        2.0 + (b - r) / span
    } else {
        4.0 + (r - g) / span
    };
    let h = sector / 6.0;
    (if h < 0.0 { h + 1.0 } else { h }, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> Chan {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let w = 2.0 * l - q;
    let channel = |t: f64| {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        if t < 1.0 / 6.0 {
            w + (q - w) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            w + (q - w) * (2.0 / 3.0 - t) * 6.0
        } else {
            w
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

/// The share a modifier's value stands for: the file's 100000ths.
fn share(v: i32) -> f64 {
    f64::from(v) / 100_000.0
}

/// One modifier on a colour. The seven the specs apply; any other leaves the colour as it is.
fn step(c: Chan, alpha: &mut f64, m: ColorTransform) -> Chan {
    match m {
        ColorTransform::Tint(v) => {
            let k = share(v);
            c.map(|x| from_linear(to_linear(x) * k + (1.0 - k)))
        }
        ColorTransform::Shade(v) => {
            let k = share(v);
            c.map(|x| from_linear(to_linear(x) * k))
        }
        ColorTransform::LumMod(v) => {
            let (h, s, l) = to_hsl(c);
            from_hsl(h, s, l * share(v))
        }
        ColorTransform::LumOff(v) => {
            let (h, s, l) = to_hsl(c);
            from_hsl(h, s, l + share(v))
        }
        ColorTransform::SatMod(v) => {
            let (h, s, l) = to_hsl(c);
            from_hsl(h, s * share(v), l)
        }
        ColorTransform::SatOff(v) => {
            let (h, s, l) = to_hsl(c);
            from_hsl(h, s + share(v), l)
        }
        ColorTransform::Alpha(v) => {
            *alpha = share(v).clamp(0.0, 1.0);
            c
        }
        _ => c,
    }
}

fn byte(x: f64) -> u8 {
    if x.is_finite() { (x.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 }
}

/// `colour` with the modifiers applied in order, and the alpha they state (1 where none does).
pub(crate) fn apply(colour: Rgb, modifiers: &[ColorTransform]) -> (Rgb, f64) {
    let mut c: Chan = [f64::from(colour.r) / 255.0, f64::from(colour.g) / 255.0, f64::from(colour.b) / 255.0];
    let mut alpha = 1.0;
    for m in modifiers {
        c = step(c, &mut alpha, *m);
    }
    (Rgb { r: byte(c[0]), g: byte(c[1]), b: byte(c[2]) }, alpha)
}
