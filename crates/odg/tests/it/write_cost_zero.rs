//! What a retext of paragraphs holding many zero-width elements costs: the elements of each paragraph the plan edits
//! or takes out are listed once, carried over by the block's prefix and suffix, put in once, and read back once. Every scenario runs at n and 4n, and the work the
//! save counts ([`diacraft_odg::WriteWork`]: the elements listed, carried and put in, the bytes tokenized and copied, the edits, the steps of the order) must grow with
//! n and not with its square. The worst shapes for each loop are built by hand: many elements at one offset, many fields, many paragraphs that each hold one, many
//! paragraphs taken out whose elements all go to one place, and many spans that each hold one.

use diacraft_doc::TextPart;
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, open, open_bytes};
use crate::write_runs::{chars, chars_of, set_parts, set_row, the_shape};

const STYLES: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;

/// A custom shape holding `inner` as its text.
fn drawing(inner: &str) -> Drawing {
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{inner}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    );
    Drawing::a4().auto(STYLES).page(&body)
}

/// The counts of a save of `o`'s model, and the characters the saved file reads as.
fn counted(o: &Open) -> (WriteWork, String) {
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let saved = saved.unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes);
    let text = chars_of(the_shape(&back.doc));
    (work, text)
}

fn bold(o: &mut Open) {
    set_row(o, "Character", 1, &[("Style", 1.0)]);
}

/// The units the retext of one scenario walked: the elements counted, the bytes tokenized, copied and written, the edits, and the steps of the order.
fn units(w: &WriteWork) -> [(&'static str, u64); 6] {
    [
        ("zero_steps", w.zero_steps),
        ("tokenized_bytes", w.tokenized_bytes),
        ("bytes_copied", w.bytes_copied),
        ("bytes_written", w.bytes_written),
        ("edits", w.edits),
        ("order_steps", w.order_steps),
    ]
}

/// A scenario at 0, n and 4n: each unit grows at most 4 times (and a little) from the empty scenario's fixed cost to n's and from n's to 4n's. The base is the
/// count at no element, which is the fixed cost of the package and the shape.
fn linear(name: &str, n: usize, make: impl Fn(usize) -> Open) {
    let base = counted(&make(1)).0;
    let (small, small_text) = counted(&make(n));
    let (large, large_text) = counted(&make(4 * n));
    assert!(!small_text.is_empty() && !large_text.is_empty());
    for ((unit, b), ((_, s), (_, l))) in units(&base).into_iter().zip(units(&small).into_iter().zip(units(&large))) {
        println!("COST {name}: {unit}: base {b}, n={n} {s}, n={} {l}", 4 * n);
        // (l - b) <= 4.1 * (s - b) + 64: a quadratic cost would be 16 times.
        let (s_extra, l_extra) = (s.saturating_sub(b), l.saturating_sub(b));
        assert!(l_extra * 10 <= s_extra * 41 + 640, "{name}: {unit} grows more than 4 times for 4 times the elements: base {b}, {s}, {l}");
        assert!(l >= s, "{name}: {unit}");
    }
    // Something was walked: the scenario is not an empty one.
    assert!(large.zero_steps > small.zero_steps || name.contains("no element"), "{name}: the elements are counted");
}

/// One paragraph with n bookmarks between words, its last word bold: the paragraph is written whole and every element is put back.
#[test]
fn n_and_4n_marks_in_one_paragraph_written_whole_cost_n_and_4n() {
    linear("marks in one paragraph", 1500, |n| {
        let inner: String = (0..n).map(|k| format!("w<text:bookmark text:name=\"B{k:05}\"/>")).collect();
        let mut o = open(&drawing(&format!("<text:p>{inner}end</text:p>")));
        bold(&mut o);
        let text: String = "w".repeat(n);
        set_parts(&mut o, vec![chars(&text), TextPart::Cp(1), chars("end")]);
        o
    });
}

/// n elements that all land on one offset (between two runs): put in once each, in their order, and not looked for from the start for each.
#[test]
fn n_and_4n_marks_at_one_offset_cost_n_and_4n() {
    linear("marks at one offset", 1500, |n| {
        let inner: String = (0..n).map(|k| format!("<text:reference-mark-start text:name=\"R{k:05}\"/>")).collect();
        let mut o = open(&drawing(&format!("<text:p>ab{inner}cd</text:p>")));
        bold(&mut o);
        set_parts(&mut o, vec![chars("ab"), TextPart::Cp(1), chars("cd")]);
        o
    });
}

/// n fields, all kept: each is written again with its bytes and none is searched for.
#[test]
fn n_and_4n_fields_in_one_paragraph_cost_n_and_4n() {
    linear("fields in one paragraph", 1000, |n| {
        let inner: String = (0..n).map(|_| "x<text:title>t</text:title>").collect();
        let mut o = open(&drawing(&format!("<text:p>{inner}z</text:p>")));
        bold(&mut o);
        let text: String = "xt".repeat(n);
        set_parts(&mut o, vec![chars(&text), TextPart::Cp(1), chars("z")]);
        o
    });
}

/// n paragraphs that each hold a mark and are all written whole at once (a format over the whole text): one block of n paragraphs, each element carried over once.
#[test]
fn n_and_4n_paragraphs_with_a_mark_each_written_whole_cost_n_and_4n() {
    linear("paragraphs with a mark each", 1500, |n| {
        let inner: String = (0..n).map(|k| format!("<text:p>w<text:bookmark text:name=\"B{k:05}\"/></text:p>")).collect();
        let mut o = open(&drawing(&inner));
        bold(&mut o);
        let text = vec!["w"; n].join("\n");
        set_parts(&mut o, vec![TextPart::Cp(1), chars(&text)]);
        o
    });
}

/// n paragraphs taken out, each with a mark, none kept after them: the marks all go to the end of the one paragraph kept, put in once each.
#[test]
fn n_and_4n_paragraphs_taken_out_with_a_mark_each_cost_n_and_4n() {
    linear("paragraphs taken out", 1500, |n| {
        let inner: String = (0..=n).map(|k| format!("<text:p>w{k:05}<text:bookmark text:name=\"B{k:05}\"/></text:p>")).collect();
        let mut o = open(&drawing(&inner));
        set_parts(&mut o, vec![chars("w0")]);
        o
    });
}

/// n paragraphs taken out, each with a mark, and one kept after them: the marks all go to the start of that one.
#[test]
fn n_and_4n_paragraphs_taken_out_before_a_kept_one_cost_n_and_4n() {
    linear("paragraphs taken out before a kept one", 1500, |n| {
        let inner: String = (0..=n).map(|k| format!("<text:p>w{k:05}<text:bookmark text:name=\"B{k:05}\"/></text:p>")).collect();
        let mut o = open(&drawing(&format!("{inner}<text:p>kept</text:p>")));
        set_parts(&mut o, vec![chars("kept")]);
        o
    });
}

/// n spans that each hold a mark, the whole text formatted again: the marks all land in the one run, and are put in its span one by one.
#[test]
fn n_and_4n_spans_with_a_mark_each_cost_n_and_4n() {
    linear("spans with a mark each", 1000, |n| {
        let inner: String =
            (0..n).map(|k| format!("<text:span text:style-name=\"T1\">a<text:bookmark text:name=\"B{k:05}\"/>b</text:span>")).collect();
        let mut o = open(&drawing(&format!("<text:p>{inner}</text:p>")));
        set_row(&mut o, "Character", 9, &[("Style", 2.0)]);
        let text: String = "ab".repeat(n);
        set_parts(&mut o, vec![TextPart::Cp(9), chars(&text)]);
        o
    });
}

/// A text with no element counts no element work beyond the bytes of the paragraphs taken out that it looked at for a tag: a paragraph written whole with none
/// lists none, a paragraph taken out with no tag in it is not tokenized, and one with a tag that is no element is read once and finds none.
#[test]
fn a_text_with_no_element_counts_no_element_work() {
    let mut o = open(&drawing("<text:p>plain</text:p><text:p>other</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![TextPart::Cp(1), chars("plain\nother")]);
    let (work, _) = counted(&o);
    assert_eq!(work.zero_steps, 0);
    let mut o = open(&drawing("<text:p>plain</text:p><text:p>other</text:p>"));
    set_parts(&mut o, vec![chars("plain")]);
    let (work, _) = counted(&o);
    assert_eq!((work.zero_steps, work.tokenized_bytes), ("other".len() as u64, 0), "its bytes are looked at for a tag, and it is not tokenized");
    let span = "<text:span text:style-name=\"T1\">other</text:span>";
    let mut o = open(&drawing(&format!("<text:p>plain</text:p><text:p>{span}</text:p>")));
    set_parts(&mut o, vec![chars("plain")]);
    let (work, _) = counted(&o);
    assert_eq!(
        (work.zero_steps, work.tokenized_bytes),
        (span.len() as u64, span.len() as u64),
        "a tag that is no element is read once and finds none"
    );
}

/// Crafted shapes timed beside a yardstick (run by hand: `cargo test -p diacraft-odg --release --test it -- --ignored --nocapture timed_zero_width`; nothing here
/// asserts a time, which the loaded machines the suite runs on would fail): the same text edit of a paragraph of `n` words, with no element (the yardstick),
/// with `n` marks between the words, with `n` marks at one offset, with `n` fields, and with `n` paragraphs that each hold a mark and are all taken out.
#[test]
#[ignore = "timed by hand, beside a yardstick"]
fn timed_zero_width_shapes_beside_a_yardstick() {
    use std::time::Instant;
    let time = |name: &str, n: usize, o: Open| {
        let start = Instant::now();
        let (work, _) = counted(&o);
        println!("TIMED {name} n={n}: {:?} (zero steps {}, tokenized {})", start.elapsed(), work.zero_steps, work.tokenized_bytes);
    };
    for n in [20_000usize, 80_000, 320_000] {
        let words: String = "w".repeat(n);
        let edit_tail = |inner: String, text: String| {
            let mut o = open(&drawing(&format!("<text:p>{inner}end</text:p>")));
            bold(&mut o);
            set_parts(&mut o, vec![chars(&text), TextPart::Cp(1), chars("end")]);
            o
        };
        time("yardstick: n words, no element", n, edit_tail(words.clone(), words.clone()));
        time("n marks between words", n, edit_tail((0..n).map(|k| format!("w<text:bookmark text:name=\"B{k:05}\"/>")).collect(), words.clone()));
        time("n marks at one offset", n, edit_tail((0..n).map(|k| format!("<text:bookmark text:name=\"B{k:05}\"/>")).collect(), String::new()));
        time("n fields", n, edit_tail("x<text:title>t</text:title>".repeat(n), "xt".repeat(n)));
        let removed: String = (0..=n).map(|k| format!("<text:p>w<text:bookmark text:name=\"B{k:05}\"/></text:p>")).collect();
        let mut o = open(&drawing(&removed));
        set_parts(&mut o, vec![chars("w")]);
        time("n paragraphs taken out, a mark each", n, o);
    }
}
