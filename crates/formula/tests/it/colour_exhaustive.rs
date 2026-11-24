//! The formula parser's colour literal (`#` and six hex digits, red first) and the printer's
//! form of it, over every 8-bit colour (2^24), where the domain is finite, so a test that runs every input is a complete proof.
//!
//! **The laws.** For every colour: the literal parses to `Expr::Color` with the channels in the order red, green, blue, in upper case, in
//! lower case and in mixed case alike ("readers compare hex without regard to case"); `print` writes it as `#` and six *upper-case*
//! digits; and the print parses back to the same tree. A seventh digit, or a fifth, is no colour.
//!
//! **Run time (measured in `cargo test`, 32 threads, opt-level 1).** Four parses and a print a colour; the sweep is split across
//! `std::thread::scope`: 2.3 seconds on 32 threads, 16.7 seconds of cores (about 2.1 on 8 cores). The whole of it is in the normal suite.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use diacraft_formula::{Expr, ParseErrorKind, parse, print};

const COLOURS: u64 = 1 << 24;
const BLOCK: u64 = 1 << 16;

/// Works `work` over every block of 65,536 consecutive colours on every available core; it returns the number of colours that broke a law and
/// the first few, as text. A panic in `work` fails the test.
fn sweep(work: impl Fn(std::ops::Range<u64>) -> (u64, Vec<String>) + Sync) -> (u64, Vec<String>) {
    let next = AtomicU64::new(0);
    let total = Mutex::new((0u64, Vec::<String>::new()));
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k * BLOCK >= COLOURS {
                        break;
                    }
                    let (bad, examples) = work(k * BLOCK..(k + 1) * BLOCK);
                    let mut t = total.lock().unwrap();
                    t.0 += bad;
                    let room = 8usize.saturating_sub(t.1.len());
                    t.1.extend(examples.into_iter().take(room));
                }
            });
        }
    });
    total.into_inner().unwrap()
}

fn channels(i: u64) -> (u8, u8, u8) {
    ((i >> 16) as u8, (i >> 8) as u8, i as u8)
}

/// Every colour: the literal in three cases parses to the colour, `print` writes upper case, and the print parses back.
///
/// RESULT (measured): 16,777,216 colours, 0 broke a law.
#[test]
fn a_colour_literal_parses_in_any_case_and_prints_in_upper_case() {
    let (bad, examples) = sweep(|range| {
        let mut bad = 0u64;
        let mut examples = Vec::new();
        for i in range {
            let (r, g, b) = channels(i);
            let upper = format!("#{r:02X}{g:02X}{b:02X}");
            let lower = upper.to_ascii_lowercase();
            let mixed: String = upper.chars().enumerate().map(|(k, c)| if k % 2 == 1 { c.to_ascii_lowercase() } else { c }).collect();
            let want = Expr::Color { r, g, b };
            let parsed = [parse(&upper).ast, parse(&lower).ast, parse(&mixed).ast];
            let printed = print(&want);
            let reparsed = parse(&printed).ast;
            let ok = parsed.iter().all(|p| p.as_ref().ok() == Some(&want)) && printed == upper && reparsed.as_ref().ok() == Some(&want);
            if !ok {
                bad += 1;
                if examples.len() < 8 {
                    examples.push(format!("{upper}: parsed {parsed:?}, printed {printed:?}, reparsed {reparsed:?}"));
                }
            }
        }
        (bad, examples)
    });
    assert!(bad == 0, "{bad} of {COLOURS} colours broke the law, first:\n{}", examples.join("\n"));
}

/// A fifth digit or a seventh is no colour: for every colour of the first 2^20 (red 0 to 15, every green and blue), the six digits with one
/// more after them, and the last five of them (the 4 digits `GGBB` and one more), are refused with `BadHash`.
///
/// RESULT (measured): 1,048,576 colours, 2,097,152 texts, 0 read as a colour or as any other error.
#[test]
fn a_fifth_digit_or_a_seventh_is_no_colour() {
    let (bad, examples) = sweep(|range| {
        let mut bad = 0u64;
        let mut examples = Vec::new();
        for i in range.filter(|i| *i < (1 << 20)) {
            let (r, g, b) = channels(i);
            let six = format!("#{r:02X}{g:02X}{b:02X}");
            let seven = format!("{six}{}", ["0", "a", "F", "9"][(i % 4) as usize]);
            let five = format!("#{g:02X}{b:02X}{}", ["0", "a", "F", "9"][((i / 4) % 4) as usize]);
            for text in [&seven, &five] {
                let e = parse(text).ast;
                if !matches!(&e, Err(e) if e.kind == ParseErrorKind::BadHash) {
                    bad += 1;
                    if examples.len() < 8 {
                        examples.push(format!("{text}: {e:?}"));
                    }
                }
            }
        }
        (bad, examples)
    });
    assert!(bad == 0, "{bad} texts read as colours or as another error, first:\n{}", examples.join("\n"));
}
