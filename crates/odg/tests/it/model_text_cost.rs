//! Crafted inputs for the model writer's text (D1 of the plan's shared done-criteria): each is a text a file or a person could make, written beside **a yardstick of plain text of
//! the same size**, in the units the loops walk (steps of the save, bytes copied, attributes written), with a bound near what was measured. An `#[ignore]` test of each times the case back to back
//! with its yardstick.
//!
//! * 50,000 fields on one shape (the security scan's first finding: a field found its row by a scan of the shape's `Field` rows);
//! * 10,000 elements of a paragraph that cross one another (the second: covers that cross were closed and opened again at every place where one ended, before anything was charged);
//! * 100,000 runs naming faces by the numbers of a table of 10,000 faces, each on a row of its own (the third: a face was found by a scan of the document's table for each row, and a colour was
//!   formatted for each call);
//! * 100,000 one-letter runs, a list nested to the limit and one past it, a paragraph of 10 MB.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{
    Cell, CellName, Digest, Document, FaceName, OdfParagraph, OdfRun, OdfText, OdfTextStyle, RawAttrs, RawElement, Row, Section, Stamp, TextPart,
    Value,
};
use diacraft_odg::{WriteOptions, WriteWork, write_counted};

use crate::model_text::{STYLES, chars, doc_with, drawing_with, model_of, number, row, section};

/// What a save of `doc` cost: its steps, the bytes it copied, the attributes it wrote, the classes of its notes (or the words it was refused in).
struct Cost {
    steps: u64,
    bytes: u64,
    attributes: u64,
    notes: Vec<String>,
    refused: Option<String>,
}

fn cost(doc: &Document) -> Cost {
    let mut work = WriteWork::default();
    let result = write_counted(doc, None, &WriteOptions::default(), &mut work);
    let (notes, refused) = match result {
        Ok(w) => (w.notes.iter().map(|n| format!("{} {n}", n.class())).collect(), None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    Cost { steps: work.steps_used(), bytes: work.bytes_copied, attributes: work.attributes_written, notes, refused }
}

fn timed(doc: &Document) -> f64 {
    let start = Instant::now();
    let mut work = WriteWork::default();
    let _ = write_counted(doc, None, &WriteOptions::default(), &mut work);
    start.elapsed().as_secs_f64()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ 50,000 fields

const FIELDS: usize = 50_000;

/// One shape whose text is `n` fields, a word before each: `w 0w 1w 2...`, with the `Field` row of each.
fn fields_doc(n: usize) -> Document {
    let mut parts: Vec<TextPart> = Vec::with_capacity(n * 2);
    let mut rows: Vec<Row> = Vec::with_capacity(n);
    for i in 0..n {
        parts.push(chars("w "));
        parts.push(TextPart::Fld { ix: i as u32, text: Arc::from(i.to_string()) });
        let mut r = Row { ix: Some(i as u32), ..Row::default() };
        r.cells.insert(CellName::new("Value"), Cell::read(Some("PAGENUMBER()"), Some(&i.to_string()), None, None, &mut 1_000));
        rows.push(r);
    }
    doc_with(parts, vec![section("Field", rows)])
}

/// The yardstick: the same characters, no field.
fn words_doc(n: usize) -> Document {
    let text: String = (0..n).map(|i| format!("w {i}")).collect();
    doc_with(vec![chars(&text)], Vec::new())
}

#[test]
fn fifty_thousand_fields_in_one_shape_cost_what_the_same_words_do() {
    let (case, yard) = (cost(&fields_doc(FIELDS)), cost(&words_doc(FIELDS)));
    assert!(case.refused.is_none(), "{:?}", case.refused);
    assert!(yard.refused.is_none(), "{:?}", yard.refused);
    println!(
        "FIELDS steps {} / {} bytes {} / {} attributes {} / {}",
        case.steps, yard.steps, case.bytes, yard.bytes, case.attributes, yard.attributes
    );
    // Each field is a step to make and a step to write, and a probe of the row map: a few steps each, and never a scan of the rows.
    assert!(case.steps <= 12 * FIELDS as u64, "{} steps for {FIELDS} fields", case.steps);
    // The bytes are the element tags and the text, three times the words' at most.
    assert!(case.bytes <= 8 * yard.bytes + 4_000_000, "{} bytes against {}", case.bytes, yard.bytes);
    // Doubling the fields doubles the work: linear, and not the square.
    let half = cost(&fields_doc(FIELDS / 2));
    assert!(case.steps <= 3 * half.steps, "{} steps against {} for half as many", case.steps, half.steps);
    assert!(case.notes.iter().all(|n| !n.starts_with("field_as_text")), "{:?}", case.notes);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn fifty_thousand_fields_take_no_longer_than_a_few_times_the_same_words() {
    let (case, yard) = (timed(&fields_doc(FIELDS)), timed(&words_doc(FIELDS)));
    println!("FIELDS {case:.3} s against {yard:.3} s");
    assert!(case <= 6.0 * yard + 0.5, "{case} s against {yard} s");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ crossing covers

const COVERS: usize = 10_000;
const SPAN: usize = 5_000;

/// A paragraph of `COVERS + SPAN` characters whose text holds `COVERS` elements `[i, i + SPAN)`: each crosses the next, which a file's markup never does (a forged model's held runs).
fn crossing_doc(held: usize) -> Document {
    let text: String = "a".repeat(COVERS + SPAN);
    let mut doc = doc_with(vec![chars(&text)], Vec::new());
    let runs: Vec<OdfRun> = (0..held)
        .map(|i| OdfRun {
            from: i as u32,
            to: (i + SPAN) as u32,
            style: OdfTextStyle::default(),
            element: Some(Arc::new(RawElement { name: Arc::from("text:a"), attrs: RawAttrs::new(), children: Vec::new() })),
        })
        .collect();
    // The stamp of the paragraph's characters as the reader makes it: the count, the one piece, then each run's place.
    let mut digest = Digest::new("characters");
    digest.bytes(&(text.chars().count() as u64).to_le_bytes()).text(&text);
    for r in &runs {
        digest.bytes(&u64::from(r.from).to_le_bytes()).bytes(&u64::from(r.to).to_le_bytes());
    }
    let stamp: Stamp = digest.finish();
    let paragraph = OdfParagraph { style: OdfTextStyle::default(), characters: Some(stamp), runs, element: None, levels: Vec::new() };
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let text = Arc::make_mut(shape.text.as_mut().unwrap());
    text.ext.make_mut().odf = OdfText { kept: Default::default(), paragraphs: vec![paragraph] };
    doc
}

#[test]
fn ten_thousand_elements_that_cross_one_another_are_left_out_and_said_and_cost_what_the_plain_paragraph_does() {
    let (case, yard) = (cost(&crossing_doc(COVERS)), cost(&crossing_doc(0)));
    assert!(case.refused.is_none(), "{:?}", case.refused);
    println!(
        "CROSSING steps {} / {} bytes {} / {} attributes {} / {}",
        case.steps, yard.steps, case.bytes, yard.bytes, case.attributes, yard.attributes
    );
    // One element survives (the first), and the one that starts where it ends; the others cross one of them, and are not written.
    assert!(case.notes.iter().any(|n| n.starts_with("text_elements_not_written") && n.contains("9998 elements")), "{:?}", case.notes);
    // The copy is the paragraph and one element's tags, not the tags again at every place one ends: the plain paragraph's bytes, and a small multiple.
    assert!(case.bytes <= 4 * yard.bytes + 1_000_000, "{} bytes against {}", case.bytes, yard.bytes);
    assert!(case.steps <= 8 * yard.steps + 20 * COVERS as u64, "{} steps against {}", case.steps, yard.steps);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn ten_thousand_crossing_elements_take_no_longer_than_a_few_times_the_plain_paragraph() {
    let (case, yard) = (timed(&crossing_doc(COVERS)), timed(&crossing_doc(0)));
    println!("CROSSING {case:.3} s against {yard:.3} s");
    assert!(case <= 6.0 * yard + 0.5, "{case} s against {yard} s");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ faces by number

const RUNS: usize = 100_000;
const FACES: usize = 10_000;

/// A document with `FACES` faces numbered 1 to `FACES`, and one shape whose text is `RUNS` one-letter runs on `RUNS` rows of the `Character` section: row `i` is bold and names face
/// `i % FACES + 1` when `named`, and holds the bold alone otherwise.
fn faces_doc(named: bool) -> Document {
    let mut parts: Vec<TextPart> = Vec::with_capacity(RUNS * 2);
    let mut rows: Vec<Row> = Vec::with_capacity(RUNS);
    for i in 0..RUNS {
        parts.push(TextPart::Cp((i + 1) as u32));
        parts.push(chars("a"));
        let mut cells: Vec<(&str, f64)> = vec![("Style", 1.0)];
        if named {
            cells.push(("Font", (i % FACES + 1) as f64));
        }
        rows.push(row((i + 1) as u32, &cells));
    }
    let mut doc = doc_with(parts, vec![section("Character", rows)]);
    let faces: Vec<FaceName> = (0..FACES)
        .map(|k| FaceName {
            name_u: Arc::from(format!("Face {}", k + 1)),
            raw: RawAttrs::from_pairs([("ID", (k + 1).to_string())]),
            ext: Default::default(),
        })
        .collect();
    doc.fonts = Arc::new(faces);
    doc
}

#[test]
fn a_hundred_thousand_runs_naming_faces_in_a_table_of_ten_thousand_cost_what_the_same_runs_do_without_them() {
    let (case, yard) = (cost(&faces_doc(true)), cost(&faces_doc(false)));
    // Either is a save or is refused in words (the text and its rows are DiaCraft's data, which has a size it may not pass).
    println!(
        "FACES steps {} / {} bytes {} / {} attributes {} / {} refused {:?} / {:?}",
        case.steps, yard.steps, case.bytes, yard.bytes, case.attributes, yard.attributes, case.refused, yard.refused
    );
    assert_eq!(
        case.refused.is_some(),
        yard.refused.is_some(),
        "the faces change nothing of what is refused: {:?} against {:?}",
        case.refused,
        yard.refused
    );
    assert!(case.steps <= 4 * yard.steps + 4 * (RUNS + FACES) as u64, "{} steps against {}", case.steps, yard.steps);
    assert!(case.bytes <= 8 * yard.bytes + 50_000_000, "{} bytes against {}", case.bytes, yard.bytes);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn a_hundred_thousand_runs_naming_faces_take_no_longer_than_a_few_times_the_same_runs_without_them() {
    let (case, yard) = (timed(&faces_doc(true)), timed(&faces_doc(false)));
    println!("FACES {case:.3} s against {yard:.3} s");
    assert!(case <= 6.0 * yard + 1.0, "{case} s against {yard} s");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the rest

const LETTERS: usize = 100_000;

/// `LETTERS` one-letter runs that alternate between two rows (a bold and an italic), or the same letters in one run.
fn letters_doc(runs: bool) -> Document {
    let mut parts: Vec<TextPart> = Vec::new();
    let text: String = (0..LETTERS).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    if runs {
        for (i, c) in text.chars().enumerate() {
            parts.push(TextPart::Cp((i % 2 + 1) as u32));
            parts.push(chars(&c.to_string()));
        }
    } else {
        parts.push(chars(&text));
    }
    let rows = section("Character", vec![row(1, &[("Style", 1.0)]), row(2, &[("Style", 2.0)])]);
    doc_with(parts, if runs { vec![rows] } else { Vec::new() })
}

/// The same letters as the text of a drawing a file holds: `LETTERS` spans that alternate between a bold and an italic style (the rows are the file's, so the save says all of the text natively), or
/// one plain paragraph.
fn letters_file(runs: bool) -> Document {
    let text: String = (0..LETTERS).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let inner = if runs {
        let spans: String = text.chars().enumerate().map(|(i, c)| format!("<text:span text:style-name=\"T{}\">{c}</text:span>", i % 2 + 1)).collect();
        format!("<text:p>{spans}</text:p>")
    } else {
        format!("<text:p>{text}</text:p>")
    };
    model_of(&drawing_with(STYLES, &inner))
}

#[test]
fn a_hundred_thousand_one_letter_runs_of_a_drawing_cost_a_few_steps_each_and_the_same_letters_in_one_run_one_per_part() {
    let (case, yard) = (cost(&letters_file(true)), cost(&letters_file(false)));
    println!(
        "LETTERS steps {} / {} bytes {} / {} attributes {} / {} refused {:?}",
        case.steps, yard.steps, case.bytes, yard.bytes, case.attributes, yard.attributes, case.refused
    );
    assert!(case.refused.is_none(), "{:?}", case.refused);
    assert!(case.steps <= 12 * LETTERS as u64, "{} steps for {LETTERS} runs", case.steps);
    // A span's attribute is written once for each run.
    assert!(case.attributes <= 3 * LETTERS as u64 + 1_000, "{} attributes", case.attributes);
    assert!(case.bytes <= 60 * yard.bytes + 20_000_000, "{} bytes against {}", case.bytes, yard.bytes);
}

#[test]
fn a_hundred_thousand_one_letter_runs_a_person_formatted_cost_a_few_steps_each_and_are_a_save_or_refused_in_words() {
    // The rows of a text DiaCraft made are its own, which the extension carries, and a text with so many runs is more than one attribute holds: refused in words, after the work of a few steps a run.
    let (case, yard) = (cost(&letters_doc(true)), cost(&letters_doc(false)));
    println!("LETTERS (made) steps {} / {} bytes {} / {} refused {:?}", case.steps, yard.steps, case.bytes, yard.bytes, case.refused);
    assert!(case.steps <= 12 * LETTERS as u64, "{} steps for {LETTERS} runs", case.steps);
    if let Some(words) = &case.refused {
        assert!(words.contains("past what one attribute"), "{words}");
    }
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn a_hundred_thousand_one_letter_runs_take_no_longer_than_a_few_times_the_same_letters_in_one_run() {
    let (case, yard) = (timed(&letters_file(true)), timed(&letters_file(false)));
    println!("LETTERS {case:.3} s against {yard:.3} s");
    assert!(case <= 30.0 * yard + 1.0, "{case} s against {yard} s");
}

const BIG: usize = 10 * 1024 * 1024;

/// A paragraph of 10 MB of words (one text of one paragraph), or the same bytes as paragraphs of about 100 characters.
fn big_doc(one_paragraph: bool) -> Document {
    let word = "word ";
    let text: String = word.repeat(BIG / word.len());
    let text = if one_paragraph {
        text
    } else {
        text.as_bytes().chunks(100).map(|c| String::from_utf8_lossy(c).trim().to_owned()).collect::<Vec<_>>().join("\n")
    };
    doc_with(vec![chars(&text)], Vec::new())
}

#[test]
fn a_paragraph_of_ten_megabytes_is_copied_a_few_times_as_the_same_bytes_in_many_paragraphs_are() {
    let (case, yard) = (cost(&big_doc(true)), cost(&big_doc(false)));
    println!("BIG steps {} / {} bytes {} / {} refused {:?} / {:?}", case.steps, yard.steps, case.bytes, yard.bytes, case.refused, yard.refused);
    assert!(case.refused.is_none(), "{:?}", case.refused);
    assert!(case.bytes <= 4 * yard.bytes + 40_000_000, "{} bytes against {}", case.bytes, yard.bytes);
    assert!(case.bytes >= BIG as u64, "the paragraph is copied at least once: {}", case.bytes);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn a_paragraph_of_ten_megabytes_takes_no_longer_than_a_few_times_the_same_bytes_in_many_paragraphs() {
    let (case, yard) = (timed(&big_doc(true)), timed(&big_doc(false)));
    println!("BIG {case:.3} s against {yard:.3} s");
    assert!(case <= 4.0 * yard + 1.0, "{case} s against {yard} s");
}

/// Keeps the helpers the file shares with `model_text` from being reported unused when a case is ignored.
#[allow(dead_code)]
fn unused(_: Section, _: Value) -> Cell {
    number(0.0)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ round 2 of the scan

/// The held paragraph of [`held_doc`]: the places of its runs, the name of their element, and whether its stamp is the first paragraph's.
type Held<'a> = (Vec<(u32, u32)>, &'a str, bool);

/// A drawing whose one shape holds `paragraphs` (each a text of `a`s of the length given, the last ten characters telling them apart), and, when `held` is given, one held paragraph with `runs`
/// elements named `name` at the places `place` gives, whose stamp matches none of them (the stamp is that of the first paragraph when `matches`).
fn held_doc(lengths: &[usize], held: Option<Held<'_>>) -> Document {
    let texts: Vec<String> = lengths.iter().enumerate().map(|(k, n)| format!("{}{k:010}", "a".repeat(n.saturating_sub(10)))).collect();
    let all = texts.join("\n");
    let mut doc = doc_with(vec![chars(&all)], Vec::new());
    let Some((places, name, matches)) = held else { return doc };
    let runs: Vec<OdfRun> = places
        .iter()
        .map(|(from, to)| OdfRun {
            from: *from,
            to: *to,
            style: OdfTextStyle::default(),
            element: Some(Arc::new(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: Vec::new() })),
        })
        .collect();
    let stamp = if matches {
        let first = texts.first().cloned().unwrap_or_default();
        let mut digest = Digest::new("characters");
        digest.bytes(&(first.chars().count() as u64).to_le_bytes()).text(&first);
        for r in &runs {
            digest.bytes(&u64::from(r.from).to_le_bytes()).bytes(&u64::from(r.to).to_le_bytes());
        }
        digest.finish()
    } else {
        Stamp(0)
    };
    let paragraph = OdfParagraph { style: OdfTextStyle::default(), characters: Some(stamp), runs, element: None, levels: Vec::new() };
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let text = Arc::make_mut(shape.text.as_mut().unwrap());
    text.ext.make_mut().odf = OdfText { kept: Default::default(), paragraphs: vec![paragraph] };
    doc
}

/// The deepest nesting of held elements a paragraph of a shape of a page may have under the reader's default depth: the root, the body, the drawing, the page, the shape and the paragraph, and
/// two more for what stands inside the deepest.
fn depth_limit() -> usize {
    diacraft_odg::Limits::default().container.xml.max_depth - 9
}

/// `n` held elements nested one inside the next, `[i, 2n - i)`, on a paragraph of `2n` characters.
fn nested_doc(n: usize, name: &str) -> Document {
    held_doc(&[2 * n.max(1)], Some(((0..n as u32).map(|i| (i, 2 * n as u32 - i)).collect(), name, true)))
}

#[test]
fn held_elements_nested_to_the_depth_the_part_may_have_are_written_and_one_more_is_left_out_and_said() {
    let limit = depth_limit();
    for (name, tag) in [("text:a", "<text:a>"), ("text:subject", "<text:subject>")] {
        let at = cost(&nested_doc(limit, name));
        assert!(at.refused.is_none(), "{name}: {:?}", at.refused);
        assert!(at.notes.iter().all(|n| !n.starts_with("text_elements_not_written")), "{name} at the limit: {:?}", at.notes);
        // The file reads, with every one of them.
        let written = write_counted_bytes(&nested_doc(limit, name));
        assert_eq!(String::from_utf8_lossy(&written).matches(tag).count(), limit, "{name}");
        let past = cost(&nested_doc(limit + 1, name));
        assert!(past.refused.is_none(), "{name}: {:?}", past.refused);
        assert!(
            past.notes.iter().any(|n| n.starts_with("text_elements_not_written") && n.contains("1 element ")),
            "{name} one past the limit: {:?}",
            past.notes
        );
    }
}

/// The bytes of `content.xml` of the save of `doc`, and the file read again by the real reader.
fn write_counted_bytes(doc: &Document) -> Vec<u8> {
    let written = diacraft_odg::write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("refused: {e}"));
    diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    diacraft_odg::package::Package::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap().part("content.xml").unwrap().to_vec()
}

const NESTED: usize = 10_000;

#[test]
fn ten_thousand_nested_elements_are_left_out_down_to_the_depth_and_cost_what_the_plain_paragraph_does() {
    for name in ["text:a", "text:subject"] {
        let (case, yard) = (cost(&nested_doc(NESTED, name)), cost(&held_doc(&[2 * NESTED], None)));
        println!("NESTED {name} steps {} / {} bytes {} / {} refused {:?}", case.steps, yard.steps, case.bytes, yard.bytes, case.refused);
        assert!(case.refused.is_none(), "{name}: {:?}", case.refused);
        let left = NESTED - depth_limit();
        assert!(
            case.notes.iter().any(|n| n.starts_with("text_elements_not_written") && n.contains(&format!("{left} elements"))),
            "{name}: {:?}",
            case.notes
        );
        assert!(case.bytes <= 4 * yard.bytes + 2_000_000, "{name}: {} bytes against {}", case.bytes, yard.bytes);
        assert!(case.steps <= 8 * yard.steps + 20 * NESTED as u64, "{name}: {} steps against {}", case.steps, yard.steps);
    }
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn ten_thousand_nested_elements_take_no_longer_than_a_few_times_the_plain_paragraph() {
    for name in ["text:a", "text:subject"] {
        let (case, yard) = (timed(&nested_doc(NESTED, name)), timed(&held_doc(&[2 * NESTED], None)));
        println!("NESTED {name} {case:.3} s against {yard:.3} s");
        assert!(case <= 6.0 * yard + 0.5, "{name}: {case} s against {yard} s");
    }
}

const BIG_RUNS: u32 = 100_000;

#[test]
fn a_paragraph_of_a_hundred_thousand_held_runs_is_not_hashed_for_a_model_paragraph_it_cannot_be() {
    // The held paragraph has 100,000 runs, the last ending at the 100,000th character; the model's paragraph has ten.
    let runs: Vec<(u32, u32)> = (0..BIG_RUNS).map(|i| (i, i + 1)).collect();
    let (case, yard) = (cost(&held_doc(&[10], Some((runs, "text:a", false)))), cost(&held_doc(&[10], None)));
    println!("HELDRUNS steps {} / {} bytes {} / {}", case.steps, yard.steps, case.bytes, yard.bytes);
    assert!(case.refused.is_none(), "{:?}", case.refused);
    // The runs are looked at once (a step each) to find where the last ends, and the paragraph is not hashed: a few steps for each run, none for a candidate.
    assert!(case.steps >= yard.steps + BIG_RUNS as u64, "the runs are charged: {} steps against {}", case.steps, yard.steps);
    assert!(case.steps <= yard.steps + 3 * BIG_RUNS as u64, "{} steps against {}", case.steps, yard.steps);
}

/// 500 paragraphs of 20,000 characters, none of which the one held paragraph (20,000 runs) is: a held paragraph that matches nothing is looked at again for each of them.
const MANY: usize = 500;
const LONG: usize = 20_000;

#[test]
fn a_held_paragraph_that_matches_nothing_is_charged_for_the_hashing_and_the_matching_is_held_to_what_the_texts_are() {
    let runs: Vec<(u32, u32)> = (0..LONG as u32).map(|i| (i, i + 1)).collect();
    let lengths = vec![LONG; MANY];
    let (case, yard) = (cost(&held_doc(&lengths, Some((runs, "text:a", false)))), cost(&held_doc(&lengths, None)));
    println!("MATCHING steps {} / {} bytes {} / {} refused {:?}", case.steps, yard.steps, case.bytes, yard.bytes, case.refused);
    assert!(case.refused.is_none(), "{:?}", case.refused);
    // What is charged is at least the runs looked at once, and at most eight times the paragraphs, the runs and the characters / 64, with what the rest of the save charges.
    let budget = 8 * (MANY + LONG + MANY * LONG / 64) as u64 + 1024;
    // (A candidate costs its runs and its characters / 64, and about seventy of them are hashed before the budget is spent: half of it at the least.)
    assert!(case.steps >= yard.steps + budget / 2, "the hashing of the runs is charged: {} steps against {}", case.steps, yard.steps);
    assert!(case.steps <= yard.steps + budget + 2 * LONG as u64, "{} steps against {} and a budget of {budget}", case.steps, yard.steps);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn a_held_paragraph_that_matches_nothing_takes_no_longer_than_a_few_times_the_same_text_with_none() {
    let runs: Vec<(u32, u32)> = (0..LONG as u32).map(|i| (i, i + 1)).collect();
    let lengths = vec![LONG; MANY];
    let (case, yard) = (timed(&held_doc(&lengths, Some((runs, "text:a", false)))), timed(&held_doc(&lengths, None)));
    println!("MATCHING {case:.3} s against {yard:.3} s");
    assert!(case <= 4.0 * yard + 0.5, "{case} s against {yard} s");
}

/// 20,000 paragraphs of ten characters, and the one held paragraph of 100,000 runs: none of the paragraphs can be it (its last run ends at the 100,000th character), and a candidate that is looked
/// at again for each of them, and hashed, is 20,000 times the 100,000 runs.
const SHORT: usize = 20_000;

#[test]
fn a_held_paragraph_of_a_hundred_thousand_runs_costs_its_runs_once_beside_twenty_thousand_short_paragraphs() {
    let runs: Vec<(u32, u32)> = (0..BIG_RUNS).map(|i| (i, i + 1)).collect();
    let lengths = vec![10usize; SHORT];
    let (case, yard) = (cost(&held_doc(&lengths, Some((runs, "text:a", false)))), cost(&held_doc(&lengths, None)));
    println!("SHORT steps {} / {} bytes {} / {}", case.steps, yard.steps, case.bytes, yard.bytes);
    assert!(case.refused.is_none(), "{:?}", case.refused);
    assert!(case.steps >= yard.steps + BIG_RUNS as u64, "the runs are charged once: {} steps against {}", case.steps, yard.steps);
    assert!(case.steps <= yard.steps + 3 * BIG_RUNS as u64, "{} steps against {}", case.steps, yard.steps);
}

#[test]
#[ignore = "timed: run by hand with --ignored --nocapture"]
fn a_held_paragraph_of_a_hundred_thousand_runs_beside_twenty_thousand_short_paragraphs_takes_no_longer_than_the_same_text_with_none() {
    let runs: Vec<(u32, u32)> = (0..BIG_RUNS).map(|i| (i, i + 1)).collect();
    let lengths = vec![10usize; SHORT];
    let (case, yard) = (timed(&held_doc(&lengths, Some((runs, "text:a", false)))), timed(&held_doc(&lengths, None)));
    println!("SHORT {case:.3} s against {yard:.3} s");
    assert!(case <= 4.0 * yard + 0.5, "{case} s against {yard} s");
}
