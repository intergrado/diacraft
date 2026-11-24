//! The three hex codecs, over every 8-bit colour (2^24), where the domain is finite, so a test that runs every input is a
//! complete proof:
//!
//! - `diacraft_color::Rgba::hex` and `Rgba::from_hex` (`RRGGBB`, upper case, with or without `#` on the way in);
//! - `diacraft_calc::Rgb::hex` and `Rgb::from_hex` (`#rrggbb`, lower case on the way out; either case and a `#` on the way in);
//! - the formula parser's colour literal and the printer's form of it, `diacraft_formula::parse` and `print` (`#RRGGBB`, either case on the
//!   way in, upper case out).
//!
//! **The laws.** For every colour: each codec's print then parse is the identity; each parses in upper, lower and mixed case alike where the
//! spec says readers compare hex without regard to case; and the three agree: the same colour reads from, and prints to, the same
//! digits up to case and the `#`. The three differ in what else they accept (a missing `#`, three or eight digits, blanks): one test pins
//! that, so a change to any is seen, and it is reported, not fixed.
//!
//! **Run time (measured in `cargo test`, 32 threads, opt-level 1).** The text codecs cost about a microsecond a colour and the sweep (2.3 seconds on 32
//! threads, 26 seconds of cores, about 3.3 on 8) is split across `std::thread::scope`; it is in the normal suite. The same colours as formulas through the evaluator cost about 10 microseconds
//! each, so that check has a strided sample in the normal suite (0.2 seconds) and a full run marked `#[ignore]` (329 seconds of cores: 14.6 seconds on 32 threads; under a quota of 8 cores it and the six sweeps of `colour_exhaustive.rs` take 89 seconds together):
//!
//! ```text
//! cargo test -p diacraft-calc --test it colour_hex -- --ignored --test-threads=1
//! ```

use diacraft_calc::{Rgb, Value};
use diacraft_color::Rgba;
use diacraft_formula::{Expr, parse, print};

use crate::colour_exhaustive::{ALL, SAMPLE_STRIDE, Tally, colour_extras, drive, eval_batch, rgb_at};

/// The same text with every other letter in lower case, starting with the second character.
fn mixed(upper: &str) -> String {
    upper.chars().enumerate().map(|(k, c)| if k % 2 == 1 { c.to_ascii_lowercase() } else { c }).collect()
}

/// What the formula parser makes of a whole text: the colour, if it is a colour literal and nothing else.
fn formula_colour(text: &str) -> Option<[u8; 3]> {
    match parse(text).ast {
        Ok(Expr::Color { r, g, b }) => Some([r, g, b]),
        _ => None,
    }
}

/// Every colour through all three codecs: each reads what it printed, in any case, and they agree on the digits.
///
/// RESULT (measured): 16,777,216 colours, 0 broke a law. The calc codec prints `#rrggbb` (lower), the colour crate `RRGGBB` (upper) and
/// the formula printer `#RRGGBB` (upper): equal after folding case and dropping the `#`.
#[test]
fn the_three_hex_codecs_agree_on_every_colour() {
    let t = drive(1 << 24, 1, &[], |idx| {
        let mut t = Tally::default();
        for &i in idx {
            let [r, g, b] = rgb_at(i);
            t.checked += 1;
            let calc = Rgb { r, g, b };
            let calc_text = calc.hex();
            let colour_text = Rgba::rgb(r, g, b).hex();
            let formula_text = print(&Expr::Color { r, g, b });
            let upper = format!("#{colour_text}");
            let shape = calc_text.len() == 7 && calc_text.bytes().skip(1).all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
            let same_digits = calc_text.to_ascii_uppercase() == upper && formula_text == upper;
            let calc_reads = [calc_text.as_str(), upper.as_str(), mixed(&upper).as_str()].map(|s| Rgb::from_hex(s) == Some(calc));
            let colour_reads = [colour_text.as_str(), calc_text.as_str(), upper.as_str(), mixed(&upper).as_str()]
                .map(|s| Rgba::from_hex(s) == Some(Rgba::rgb(r, g, b)));
            let formula_reads =
                [calc_text.as_str(), upper.as_str(), mixed(&upper).as_str(), formula_text.as_str()].map(|s| formula_colour(s) == Some([r, g, b]));
            let cross = Rgb::from_hex(&formula_text) == Some(calc) && Rgb::from_hex(&upper) == Some(calc);
            let all_read = calc_reads.iter().chain(&colour_reads).chain(&formula_reads).all(|ok| *ok);
            if !(shape && same_digits && all_read && cross) {
                t.miss(|| {
                    format!(
                        "{r},{g},{b}: calc prints {calc_text:?}, colour {colour_text:?}, formula {formula_text:?}; reads {calc_reads:?} {colour_reads:?} {formula_reads:?}"
                    )
                });
            }
        }
        t
    });
    t.assert_clean("the three hex codecs");
    assert_eq!(t.checked, 1 << 24);
}

/// What a codec reads a text as: the colour, or nothing.
type Reads = Option<[u8; 3]>;

/// What each codec takes beyond `#` and six digits, one row per text: `(text, colour crate, calc, formula)`, each the colour it reads or `None`.
/// The colour crate reads the sibling's forms (no `#`, three digits, eight with alpha), calc's reader wants the `#` and six digits and the formula
/// parser wants the `#`, six digits and no name character after them. All three ignore blanks round the whole text. Measured, and pinned here.
#[test]
fn the_codecs_differ_in_what_they_accept_beyond_a_hash_and_six_digits() {
    let some = |r, g, b| Some([r, g, b]);
    let cases: Vec<(&str, Reads, Reads, Reads)> = vec![
        ("#ffaa00", some(255, 170, 0), some(255, 170, 0), some(255, 170, 0)),
        ("#FFAA00", some(255, 170, 0), some(255, 170, 0), some(255, 170, 0)),
        ("ffaa00", some(255, 170, 0), None, None),
        ("#fa0", some(255, 170, 0), None, None),
        ("#ffaa0080", some(255, 170, 0), None, None),
        (" #ffaa00 ", some(255, 170, 0), some(255, 170, 0), some(255, 170, 0)),
        ("#ffaa0", None, None, None),
        ("#ffaa00x", None, None, None),
        ("# ffaa00", None, None, None),
        ("##ffaa00", None, None, None),
        ("#+faa00", None, None, None),
        ("", None, None, None),
    ];
    let mut wrong = Vec::new();
    for (text, colour, calc, formula) in cases {
        let got_colour = Rgba::from_hex(text).map(|c| [c.r, c.g, c.b]);
        let got_calc = Rgb::from_hex(text).map(|c| [c.r, c.g, c.b]);
        let got_formula = formula_colour(text);
        if (got_colour, got_calc, got_formula) != (colour, calc, formula) {
            wrong.push(format!("{text:?}: colour {got_colour:?} calc {got_calc:?} formula {got_formula:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The colour literal as a cell's formula, through the evaluator, is the colour of `RGB(r, g, b)`: in upper and lower case, and printed back by
/// `Rgb::hex`. `stride` 1 is every colour.
fn literal_sweep(stride: u64) -> Tally {
    drive(1 << 24, stride, &colour_extras(), |idx| {
        let cols: Vec<[u8; 3]> = idx.iter().map(|&i| rgb_at(i)).collect();
        let mut formulas = Vec::new();
        for [r, g, b] in &cols {
            let upper = format!("#{r:02X}{g:02X}{b:02X}");
            formulas.extend([upper.to_ascii_lowercase(), upper, format!("RGB({r},{g},{b})")]);
        }
        let values = eval_batch(&formulas);
        let mut t = Tally::default();
        for (c, got) in cols.iter().zip(values.chunks(3)) {
            t.checked += 1;
            let want = Value::Color(Rgb { r: c[0], g: c[1], b: c[2] });
            let printed = Rgb { r: c[0], g: c[1], b: c[2] }.hex();
            if got.iter().any(|v| *v != want) || printed != format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) {
                t.miss(|| format!("{c:?}: lower, upper and RGB() give {got:?}, expected {want:?}"));
            }
        }
        t
    })
}

/// The literal through the evaluator, in the normal suite: every 97th colour and the edges.
#[test]
fn a_colour_literal_evaluates_to_the_colour_on_a_sample() {
    literal_sweep(SAMPLE_STRIDE).assert_clean("a colour literal through the evaluator");
}

/// The same over every colour.
#[test]
#[ignore = "full sweep: with the 6 of colour_exhaustive, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_hex -- --ignored --test-threads=1"]
fn a_colour_literal_evaluates_to_the_colour_for_every_colour() {
    let t = literal_sweep(ALL);
    assert!(t.checked > 1 << 24);
    t.assert_clean("a colour literal through the evaluator");
}
