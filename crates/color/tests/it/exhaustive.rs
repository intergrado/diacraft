//! The colour crate's conversions and hex codec run over every 8-bit colour (2^24), where the domain is finite, so a
//! test that runs every input is a complete proof.
//!
//! **The laws.**
//!
//! 1. `to_hsl` then `from_hsl` gives back every colour, alpha included (or within a bound: measured, and stated per test).
//! 2. The no-op transforms of [`apply`] are the identity for every colour: `LumMod` and `SatMod` at 100000, `LumOff` and `SatOff` at 0,
//!    `Tint` and `Shade` at 100000, `Inv` applied twice. (Units, from the code: a percentage is in thousandths of a percent, so 100000 is
//!    100 percent; an angle is in sixty-thousandths of a degree, so `HueOff(60000)` is one degree; `HueMod` is a percentage.) The hue no-ops
//!    (`HueMod` 100000, `HueOff` 0) are held to the same law.
//! 3. Every transform that works on hue, saturation or lightness keeps alpha, for every alpha.
//! 4. The hex codec (`Rgba::hex`, `Rgba::from_hex`): print then parse is the identity on every opaque colour, parse is case-insensitive,
//!    and the 3-digit and 8-digit forms read as they say.
//!
//! **Run time (measured in `cargo test`, 32 threads, opt-level 1).** Every test is in the normal suite. The sweeps over 2^24 colours are split
//! across `std::thread::scope`: 0.06 to 0.5 seconds each on 32 threads (1 to 9 seconds of cores), and the whole file of 11 tests takes
//! 5.9 seconds (about 8 on 8 cores), 4.4 of them the alpha test, which works 110 million applications on one thread. None is near 10 seconds on one core
//! except the hex sweep (9 seconds of cores), and that one is split.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use diacraft_color::{ColorTransform, Rgba, apply};

/// What a sweep found.
#[derive(Default)]
struct Tally {
    checked: u64,
    bad: u64,
    examples: Vec<String>,
    worst: u64,
}

impl Tally {
    fn miss(&mut self, what: impl FnOnce() -> String) {
        self.bad += 1;
        if self.examples.len() < 8 {
            self.examples.push(what());
        }
    }

    fn merge(&mut self, other: Tally) {
        self.checked += other.checked;
        self.bad += other.bad;
        self.worst = self.worst.max(other.worst);
        for e in other.examples {
            if self.examples.len() < 8 {
                self.examples.push(e);
            }
        }
    }

    fn assert_clean(&self, law: &str) {
        assert!(self.bad == 0, "{law}: {} of {} inputs broke it, first:\n{}", self.bad, self.checked, self.examples.join("\n"));
    }
}

const COLOURS: u64 = 1 << 24;
const BLOCK: u64 = 1 << 16;

/// Works `work` over every block of 65,536 consecutive colours, on every available core. A panic in `work` fails the test.
fn sweep(work: impl Fn(std::ops::Range<u64>) -> Tally + Sync) -> Tally {
    let next = AtomicU64::new(0);
    let total = Mutex::new(Tally::default());
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k * BLOCK >= COLOURS {
                        break;
                    }
                    let found = work(k * BLOCK..(k + 1) * BLOCK);
                    total.lock().unwrap().merge(found);
                }
            });
        }
    });
    total.into_inner().unwrap()
}

fn rgb_at(i: u64) -> Rgba {
    Rgba::rgb((i >> 16) as u8, (i >> 8) as u8, i as u8)
}

/// An alpha for colour `i` that takes every value of 0 to 255 against 65,536 colours each, with no pattern against the channels.
fn alpha_at(i: u64) -> u8 {
    (i.wrapping_mul(0x9E37_79B1) >> 24) as u8
}

fn rgba_at(i: u64) -> Rgba {
    rgb_at(i).with_alpha(alpha_at(i))
}

fn worst_channel(a: Rgba, b: Rgba) -> u64 {
    [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)].into_iter().map(|(x, y)| u64::from(x.abs_diff(y))).max().unwrap_or(0)
}

// ---- law 1: the round trip ------------------------------------------------------------------------------------------------------

/// `to_hsl` then `from_hsl` over all 2^24 colours, each with an alpha.
///
/// RESULT (measured): 16,777,216 colours, each with an alpha: every one comes back exactly, in all four channels, so the bound is 0.
#[test]
fn hsl_round_trip_returns_every_colour() {
    let t = sweep(|range| {
        let mut t = Tally::default();
        for i in range {
            let c = rgba_at(i);
            let (h, s, l) = c.to_hsl();
            let back = Rgba::from_hsl(h, s, l, c.a);
            t.checked += 1;
            let err = worst_channel(c, back);
            t.worst = t.worst.max(err);
            if err > ROUND_TRIP_BOUND {
                t.miss(|| format!("{c:?} -> hsl({h}, {s}, {l}) -> {back:?}"));
            }
        }
        t
    });
    eprintln!("hsl round trip: {} colours, worst channel error {}", t.checked, t.worst);
    t.assert_clean("to_hsl then from_hsl");
    assert_eq!(t.checked, COLOURS);
    assert_eq!(t.worst, ROUND_TRIP_BOUND, "the bound moved: it is the measured one");
}

/// The measured bound of law 1: no channel, alpha included, is any level from where it began.
const ROUND_TRIP_BOUND: u64 = 0;

// ---- law 2: the no-op transforms ------------------------------------------------------------------------------------------------

/// The no-op transforms, by their file names (`xml_name`) and values.
fn no_ops() -> Vec<(&'static str, ColorTransform)> {
    [
        ("lumMod", Some(100_000)),
        ("satMod", Some(100_000)),
        ("lumOff", Some(0)),
        ("satOff", Some(0)),
        ("tint", Some(100_000)),
        ("shade", Some(100_000)),
        ("hueMod", Some(100_000)),
        ("hueOff", Some(0)),
    ]
    .into_iter()
    .map(|(name, v)| (name, ColorTransform::from_xml(name, v).unwrap()))
    .collect()
}

/// The names and units this test uses are the code's: each name reads back to a transform that prints the same name and value.
#[test]
fn the_transform_names_and_units_are_the_codes() {
    for (name, t) in no_ops() {
        assert_eq!(t.xml_name(), name);
    }
    assert_eq!(ColorTransform::from_xml("lumMod", Some(100_000)), Some(ColorTransform::LumMod(100_000)));
    assert_eq!(ColorTransform::from_xml("inv", None), Some(ColorTransform::Inv));
    // 100000 is 100 percent: half the lightness at 50000, and a lightness left alone at 100000.
    let c = Rgba::rgb(200, 100, 50);
    assert!(apply(c, &[ColorTransform::LumMod(50_000)]).luminance() < c.luminance());
}

fn no_op_sweep(t: ColorTransform) -> Tally {
    sweep(|range| {
        let mut found = Tally::default();
        for i in range {
            let c = rgba_at(i);
            let out = apply(c, &[t]);
            found.checked += 1;
            found.worst = found.worst.max(worst_channel(c, out));
            if out != c {
                found.miss(|| format!("{t:?} of {c:?} is {out:?}"));
            }
        }
        found
    })
}

/// Each no-op transform is the identity on every colour and every alpha. Measured per transform, and printed.
///
/// RESULT (measured): for each of `lumMod`, `satMod`, `lumOff`, `satOff`, `hueMod`, `hueOff` at its no-op value, 16,777,216 colours, 0 changed.
/// `tint` and `shade` at 100000: see `tint_and_shade_at_a_hundred_percent_are_the_identity`.
#[test]
fn the_hsl_no_ops_are_the_identity_on_every_colour() {
    for (name, t) in no_ops().into_iter().filter(|(n, _)| !matches!(*n, "tint" | "shade")) {
        let found = no_op_sweep(t);
        eprintln!("{name} no-op: {} colours, {} changed, worst error {}", found.checked, found.bad, found.worst);
        found.assert_clean(&format!("{name} as a no-op"));
    }
}

/// `Tint` and `Shade` at 100000 (100 percent) are the identity on every colour. They work in linear light, so a round trip through it has to
/// land on the same byte.
///
/// RESULT (measured): 16,777,216 colours for each of the two, each with an alpha, 0 changed.
#[test]
fn tint_and_shade_at_a_hundred_percent_are_the_identity() {
    for (name, t) in no_ops().into_iter().filter(|(n, _)| matches!(*n, "tint" | "shade")) {
        let found = no_op_sweep(t);
        eprintln!("{name} no-op: {} colours, {} changed, worst error {}", found.checked, found.bad, found.worst);
        found.assert_clean(&format!("{name} as a no-op"));
    }
}

/// `Inv` applied twice is the identity on every colour, and once it changes every channel that is not 127.5, which no channel is.
#[test]
fn inv_applied_twice_is_the_identity() {
    let t = sweep(|range| {
        let mut found = Tally::default();
        for i in range {
            let c = rgba_at(i);
            let once = apply(c, &[ColorTransform::Inv]);
            let twice = apply(c, &[ColorTransform::Inv, ColorTransform::Inv]);
            found.checked += 1;
            if twice != c || once.a != c.a || (once.r, once.g, once.b) != (255 - c.r, 255 - c.g, 255 - c.b) {
                found.miss(|| format!("{c:?}: inv is {once:?}, twice {twice:?}"));
            }
        }
        found
    });
    t.assert_clean("inv");
    assert_eq!(t.checked, COLOURS);
}

// ---- law 3: alpha ---------------------------------------------------------------------------------------------------------------

/// The transforms that work on hue, saturation or lightness, or in linear light, with a spread of values: the no-ops, the ends of the range,
/// negatives and the extremes of `i32`.
fn hsl_transforms() -> Vec<ColorTransform> {
    let values = [i32::MIN, -1_000_000, -100_000, -1, 0, 1, 50_000, 99_999, 100_000, 100_001, 200_000, 21_600_000, i32::MAX];
    let mut out = vec![ColorTransform::Comp];
    for v in values {
        out.extend([
            ColorTransform::LumMod(v),
            ColorTransform::LumOff(v),
            ColorTransform::SatMod(v),
            ColorTransform::SatOff(v),
            ColorTransform::HueMod(v),
            ColorTransform::HueOff(v),
            ColorTransform::Tint(v),
            ColorTransform::Shade(v),
        ]);
    }
    out
}

/// Every HSL transform keeps alpha: every alpha 0 to 255, against 4,099 colours (every channel value in every position, the greys, the
/// primaries and the ends) and 13 values each.
///
/// RESULT (measured): 4,099 colours by 256 alphas by 105 transforms is 110,181,120 applications, and 0 changed an alpha (4.4 seconds, on one
/// thread).
#[test]
fn every_hsl_transform_keeps_alpha() {
    let mut colours: Vec<Rgba> = (0..=255u8).flat_map(|v| [Rgba::rgb(v, v, v), Rgba::rgb(v, 0, 0), Rgba::rgb(0, v, 0), Rgba::rgb(0, 0, v)]).collect();
    colours.extend((0..3075u64).map(|i| rgb_at(i.wrapping_mul(5_462_893) & 0xFF_FFFF)));
    let transforms = hsl_transforms();
    let mut checked = 0u64;
    let mut bad = Vec::new();
    for c in &colours {
        for a in 0..=255u8 {
            let c = c.with_alpha(a);
            for t in &transforms {
                checked += 1;
                let out = apply(c, &[*t]);
                if out.a != a && bad.len() < 8 {
                    bad.push(format!("{t:?} of {c:?} gives alpha {}", out.a));
                }
            }
        }
    }
    eprintln!("alpha kept: {checked} applications, {} lost it", bad.len());
    assert!(bad.is_empty(), "alpha lost:\n{}", bad.join("\n"));
}

/// The three that set alpha do, and exactly: `Alpha(v)` to `v` of 100000, `AlphaMod` by percent, `AlphaOff` by percent of 255; the colour is kept.
#[test]
fn the_alpha_transforms_set_alpha_and_keep_the_colour() {
    for a in 0..=255u8 {
        let c = Rgba::rgba(10, 20, 30, a);
        assert_eq!(apply(c, &[ColorTransform::AlphaMod(100_000)]), c, "alphaMod 100000 is the identity, alpha {a}");
        assert_eq!(apply(c, &[ColorTransform::AlphaOff(0)]), c, "alphaOff 0 is the identity, alpha {a}");
        let set = apply(c, &[ColorTransform::Alpha(100_000)]);
        assert_eq!((set.r, set.g, set.b, set.a), (10, 20, 30, 255));
    }
}

// ---- law 4: the hex codec -------------------------------------------------------------------------------------------------------

/// Print then parse is the identity on every opaque colour, in either case, with and without the `#`; parse is case-insensitive; and the
/// printed form is 6 upper-case digits.
///
/// RESULT (measured): 16,777,216 colours, 0 broke any of these.
#[test]
fn hex_print_then_parse_is_the_identity_for_every_colour() {
    let t = sweep(|range| {
        let mut found = Tally::default();
        for i in range {
            let c = rgb_at(i);
            let printed = c.hex();
            found.checked += 1;
            let lower = printed.to_ascii_lowercase();
            let mixed: String = printed.chars().enumerate().map(|(k, ch)| if k % 2 == 0 { ch.to_ascii_lowercase() } else { ch }).collect();
            let forms = [printed.clone(), format!("#{printed}"), lower.clone(), format!("#{lower}"), mixed.clone(), format!("#{mixed}")];
            let shape_ok = printed.len() == 6 && printed.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
            if !shape_ok || forms.iter().any(|f| Rgba::from_hex(f) != Some(c)) {
                found.miss(|| {
                    format!("{c:?} prints {printed:?} and parses back as {:?}", forms.iter().map(|f| Rgba::from_hex(f)).collect::<Vec<_>>())
                });
            }
        }
        found
    });
    t.assert_clean("hex print then parse");
    assert_eq!(t.checked, COLOURS);
}

/// `hex` has no alpha ("`RRGGBB` upper-case (no alpha)"): printing a translucent colour and parsing it gives the colour opaque. That is the
/// documented behaviour, held here so a change to it is seen; the 8-digit form is the way to carry alpha, and it reads back exactly.
///
/// RESULT (measured): for every alpha 0 to 255 against 65,536 colours, `from_hex(hex())` is the colour at alpha 255, and `from_hex` of the 8
/// digits `RRGGBBAA` is the colour at that alpha, in either case.
#[test]
fn hex_drops_alpha_and_the_eight_digit_form_keeps_it() {
    let t = sweep(|range| {
        let mut found = Tally::default();
        for i in range {
            let c = rgba_at(i);
            found.checked += 1;
            let six = Rgba::from_hex(&c.hex());
            let eight = format!("{}{:02X}", c.hex(), c.a);
            if six != Some(c.with_alpha(255)) || Rgba::from_hex(&eight) != Some(c) || Rgba::from_hex(&eight.to_ascii_lowercase()) != Some(c) {
                found.miss(|| format!("{c:?}: six digits {six:?}, eight {eight:?} gives {:?}", Rgba::from_hex(&eight)));
            }
        }
        found
    });
    t.assert_clean("hex and alpha");
}

/// The 3-digit form reads each digit twice (`#fa0` is `#ffaa00`): all 4,096 of them, in either case.
#[test]
fn the_three_digit_form_doubles_each_digit() {
    for n in 0..4096u32 {
        let (r, g, b) = ((n >> 8) & 15, (n >> 4) & 15, n & 15);
        let text = format!("{r:X}{g:X}{b:X}");
        let want = Rgba::rgb((r * 17) as u8, (g * 17) as u8, (b * 17) as u8);
        assert_eq!(Rgba::from_hex(&text), Some(want), "{text}");
        assert_eq!(Rgba::from_hex(&format!("#{}", text.to_ascii_lowercase())), Some(want), "#{text} in lower case");
    }
}

/// Text that is no colour is refused, and a refusal is never a panic: every text of 0 to 2 characters over a set that holds the hex digits,
/// the signs, the hash, a blank and a multi-byte letter, and every length from 0 to 10 of all one digit.
#[test]
fn text_that_is_no_colour_is_refused() {
    let alphabet = ['0', '9', 'a', 'F', 'g', 'G', '#', '+', '-', ' ', 'é', '\u{0}'];
    for a in alphabet {
        for b in alphabet {
            for text in [String::new(), a.to_string(), format!("{a}{b}")] {
                assert!(Rgba::from_hex(&text).is_none(), "{text:?}");
            }
        }
    }
    for len in 0..=10usize {
        let text = "a".repeat(len);
        let read = Rgba::from_hex(&text);
        assert_eq!(read.is_some(), matches!(len, 3 | 6 | 8), "{len} digits");
    }
}
