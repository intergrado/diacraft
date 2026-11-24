//! What the retext costs when many paragraphs are put beside one large paragraph (the security finding on `text.rs`: the paragraphs beside a new one
//! were tokenized again for each one added, so n added paragraphs beside a paragraph of 1 MB of runs cost n times the megabyte, and nothing charged it).
//! The bytes of content the retext tokenizes are counted ([`diacraft_odg::WriteWork::tokenized_bytes`], charged before the tokens are made, and to the bytes a
//! save may scan), and each scenario runs at n and 4n: the count must grow with the distinct paragraphs, and never with the edits times their neighbours.

use std::sync::Arc;

use diacraft_doc::{Text, TextPart};
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, open, open_bytes, shape_mut};

const STYLES: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;

/// A custom shape of a first paragraph of about `megabytes` MB of spans, the last of them plain, then `small` short paragraphs.
fn drawing(megabytes: usize, small: usize) -> Drawing {
    let mut big = String::new();
    while big.len() < megabytes * 1_000_000 {
        big.push_str("<text:span text:style-name=\"T1\">x</text:span>y");
    }
    let rest: String = (0..small).map(|k| format!("<text:p>small {k}</text:p>")).collect();
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>{big}</text:p>{rest}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    );
    Drawing::a4().auto(STYLES).page(&body)
}

/// The counts of a save of `o`'s model.
fn counted(o: &Open) -> WriteWork {
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let saved = saved.unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(saved.edited);
    // The paragraphs written read back as the model has them.
    let back = open_bytes(saved.bytes);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 1);
    work
}

fn set_text(o: &mut Open, mut f: impl FnMut(&str) -> String) {
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let parts: Vec<TextPart> = o
        .doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .get(0)
        .unwrap()
        .text
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(c) => TextPart::Chars(Arc::from(f(c))),
            other => other.clone(),
        })
        .collect();
    shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
}

/// `n` paragraphs added right after the large one: each has it for its neighbour before it, and nothing else of the file changes.
fn added_beside_the_large_one(n: usize) -> (WriteWork, usize) {
    let d = drawing(1, 1);
    let big = d.content_xml().len();
    let mut o = open(&d);
    // The large paragraph's line feed is in the character part that goes on to the next paragraph: `n` paragraphs go after it.
    let extra: String = (0..n).map(|k| format!("new {k}\n")).collect();
    set_text(&mut o, |c| c.replacen('\n', &format!("\n{extra}"), 1));
    (counted(&o), big)
}

/// A paragraph of 1 MB of runs is tokenized once for all the paragraphs put beside it, not once for each: the bytes tokenized at n added paragraphs and at 4n
/// differ by what the added ones themselves cost, which is nothing of the neighbour's.
#[test]
fn n_and_4n_paragraphs_added_beside_a_large_one_tokenize_it_once() {
    let (small, big) = added_beside_the_large_one(50);
    let (large, _) = added_beside_the_large_one(200);
    println!(
        "COST added beside 1 MB (file {big} bytes): 50 added {} then 200 added {} bytes tokenized",
        small.tokenized_bytes, large.tokenized_bytes
    );
    // The large paragraph's content is charged, once.
    assert!(small.tokenized_bytes >= 900_000, "the neighbour is charged: {}", small.tokenized_bytes);
    assert!(small.tokenized_bytes <= big as u64 * 2, "at most about twice the file ({big} bytes): {}", small.tokenized_bytes);
    // Four times the added paragraphs add what the added paragraphs' own neighbours cost (the paragraph after the large one is small), and nothing more of it.
    assert!(
        large.tokenized_bytes <= small.tokenized_bytes + 100_000,
        "50 then 200 added paragraphs beside 1 MB: {} then {} bytes tokenized",
        small.tokenized_bytes,
        large.tokenized_bytes
    );
    // And what is tokenized is scanned: the same bytes are in the bytes a save may scan.
    assert!(large.scan_bytes >= large.tokenized_bytes);
}

/// Many paragraphs retyped beside the large one (it is a neighbour of none that is edited: only an added paragraph looks at its neighbours): the bytes tokenized are
/// those of the paragraphs edited, 4 times for 4 times the paragraphs, and not the large paragraph's at all.
#[test]
fn n_and_4n_paragraphs_edited_beside_a_large_one_tokenize_what_they_hold() {
    let run = |n: usize| {
        let mut o = open(&drawing(1, n));
        set_text(&mut o, |c| c.replace("small ", "retyped "));
        counted(&o)
    };
    let (small, large) = (run(400), run(1600));
    println!("COST retyped beside 1 MB: 400 retyped {} then 1600 retyped {} bytes tokenized", small.tokenized_bytes, large.tokenized_bytes);
    assert!(small.tokenized_bytes > 0 && small.tokenized_bytes < 100_000, "the large paragraph is not tokenized: {}", small.tokenized_bytes);
    assert!(
        large.tokenized_bytes <= small.tokenized_bytes * 9 / 2 + 400,
        "{} then {} for 4 times the paragraphs",
        small.tokenized_bytes,
        large.tokenized_bytes
    );
}

/// A paragraph that is edited and is also the neighbour of added paragraphs is tokenized for both at most twice in all, and the paragraphs added after it add nothing.
#[test]
fn an_edited_neighbour_of_many_added_paragraphs_is_tokenized_a_bounded_number_of_times() {
    let run = |n: usize| {
        let mut o = open(&drawing(1, 1));
        // The large paragraph is edited (a character typed in its last run) and `n` paragraphs are added after it.
        let extra: String = (0..n).map(|k| format!("new {k}\n")).collect();
        set_text(&mut o, |c| if c.contains('\n') { format!("Q{}", c.replacen('\n', &format!("\n{extra}"), 1)) } else { c.to_owned() });
        counted(&o)
    };
    let (small, large) = (run(50), run(200));
    println!(
        "COST edited neighbour of added paragraphs: 50 added {} then 200 added {} bytes tokenized",
        small.tokenized_bytes, large.tokenized_bytes
    );
    assert!(large.tokenized_bytes <= small.tokenized_bytes + 100_000, "{} then {}", small.tokenized_bytes, large.tokenized_bytes);
    assert!(small.tokenized_bytes <= 2_400_000, "the large paragraph at most twice: {}", small.tokenized_bytes);
}

// ---- the cost of the other loops of the retext and of the markers, each at n and at 4n -------------------------------------------------------------------

/// `large` is at most 4.5 times `small` (4 for the work, and a little for the names, which gain a digit), where quadratic would be 16.
#[track_caller]
fn linear(what: &str, small: u64, large: u64) {
    assert!(small > 0, "{what}: nothing counted");
    assert!(large <= small * 9 / 2 + 2_000, "{what}: {small} then {large} for 4 times the work");
}

#[track_caller]
fn all_linear(scenario: &str, small: &WriteWork, large: &WriteWork) {
    linear(&format!("{scenario} edits"), small.edits, large.edits);
    linear(&format!("{scenario} bytes copied"), small.bytes_copied, large.bytes_copied);
    linear(&format!("{scenario} bytes written"), small.bytes_written, large.bytes_written);
    linear(&format!("{scenario} bytes scanned"), small.scan_bytes, large.scan_bytes);
    linear(&format!("{scenario} bytes tokenized"), small.tokenized_bytes, large.tokenized_bytes);
    linear(&format!("{scenario} steps"), small.steps_used() + small.order_steps, large.steps_used() + large.order_steps);
    println!(
        "COST {scenario}: {} then {} bytes copied, {} then {} steps",
        small.bytes_copied, large.bytes_copied, small.order_steps, large.order_steps
    );
}

/// One paragraph of `n` runs (a bold span and a plain letter, `n` times), with a character typed at the boundary of the first (the edit the writer cannot make in
/// place), so that the paragraph is written again whole from its `n` runs.
fn one_paragraph_of_runs(n: usize) -> WriteWork {
    let mut big = String::new();
    for _ in 0..n {
        big.push_str("<text:span text:style-name=\"T1\">x</text:span>y");
    }
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>{big}</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    );
    let mut o = open(&Drawing::a4().auto(STYLES).page(&body));
    let mut done = false;
    set_text(&mut o, |c| {
        if !done && c == "x" {
            done = true;
            format!("{c}Z")
        } else {
            c.to_owned()
        }
    });
    counted(&o)
}

#[test]
fn a_paragraph_of_n_and_of_4n_runs_is_written_in_linear_work() {
    let (small, large) = (one_paragraph_of_runs(5_000), one_paragraph_of_runs(20_000));
    all_linear("runs of one paragraph", &small, &large);
}

/// `n` paragraphs, each retyped, in one shape.
fn many_paragraphs_retyped(n: usize) -> WriteWork {
    let mut o = open(&drawing(0, n));
    set_text(&mut o, |c| c.replace("small ", "retyped "));
    counted(&o)
}

#[test]
fn n_and_4n_paragraphs_retyped_are_written_in_linear_work() {
    let (small, large) = (many_paragraphs_retyped(5_000), many_paragraphs_retyped(20_000));
    all_linear("paragraphs retyped", &small, &large);
}

/// `n` paragraphs added in the middle of a text of 5,000.
fn many_paragraphs_added(n: usize) -> WriteWork {
    let mut o = open(&drawing(0, 5_000));
    let extra: String = (0..n).map(|k| format!("added {k}\n")).collect();
    set_text(&mut o, |c| if c.contains("small 2500") { c.replacen("small 2500", &format!("{extra}small 2500"), 1) } else { c.to_owned() });
    counted(&o)
}

#[test]
fn n_and_4n_paragraphs_added_in_the_middle_of_a_text_are_written_in_linear_work() {
    let (small, large) = (many_paragraphs_added(2_000), many_paragraphs_added(8_000));
    // What the file holds is the same in both; the added ones are what grows: the count of what is written is linear in them.
    linear("paragraphs added: edits", small.edits, large.edits);
    linear("paragraphs added: bytes written", small.bytes_written, large.bytes_written);
    assert!(large.steps_used() + large.order_steps <= (small.steps_used() + small.order_steps) * 3 + 20_000, "steps");
    assert!(
        large.tokenized_bytes <= small.tokenized_bytes + 10_000,
        "no file paragraph is tokenized for the added ones: {} then {}",
        small.tokenized_bytes,
        large.tokenized_bytes
    );
}

/// One paragraph of `length` characters of plain text, made one run of a row (bold) by a marker: the paragraph is written again, whole, as a span.
fn long_text(length: usize) -> WriteWork {
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>{}</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>",
        "ab ".repeat(length / 3)
    );
    let mut o = open(&Drawing::a4().auto(STYLES).page(&body));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let text = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().text.clone().unwrap();
    let mut parts = vec![TextPart::Cp(1)];
    parts.extend(text.parts.iter().cloned());
    shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(Text { parts, ..Default::default() }));
    {
        let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = diacraft_doc::Section::new("Character", None);
        let mut row = diacraft_doc::Row { ix: Some(1), ..diacraft_doc::Row::default() };
        row.cells.insert(diacraft_doc::CellName::new("Style"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(1.0).unwrap()));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    }
    counted(&o)
}

#[test]
fn a_text_of_n_and_4n_characters_is_written_in_linear_work() {
    let (small, large) = (long_text(250_000), long_text(1_000_000));
    all_linear("a long text", &small, &large);
}

fn styles_text(bytes: &[u8]) -> String {
    crate::write_core::part(bytes, "styles.xml")
}

/// `n` new connectors, each with an end of one of nine numbers and the begin of another: the markers are nine, however many connectors name them.
fn many_connectors_with_ends(n: usize) -> WriteWork {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let first = o.index.next_id(0).unwrap();
    for (k, id) in (0..n).zip(first..) {
        let kinds = [2.0, 4.0, 5.0, 6.0, 10.0, 11.0, 13.0, 39.0, 42.0];
        let mut c = crate::write_new::connector(id, (1.0, 10.0, 8.0, 12.0));
        for (cell, v) in [("BeginArrow", kinds[k % 9]), ("BeginArrowSize", (k % 6) as f64), ("EndArrow", kinds[(k + 4) % 9]), ("EndArrowSize", 2.0)] {
            Arc::make_mut(&mut c.sheet)
                .cells
                .insert(diacraft_doc::CellName::new(cell), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(v).unwrap()));
        }
        crate::write_new::push_shape(&mut o.doc, c);
    }
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let saved = saved.unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert_eq!(styles_text(&saved.bytes).matches("<draw:marker ").count(), 9, "nine markers");
    work
}

#[test]
fn n_and_4n_connectors_with_line_ends_share_nine_markers_in_linear_work() {
    let (small, large) = (many_connectors_with_ends(1_000), many_connectors_with_ends(4_000));
    linear("connectors: edits", small.edits, large.edits);
    linear("connectors: bytes written", small.bytes_written, large.bytes_written);
    linear("connectors: attributes", small.attributes_written, large.attributes_written);
    linear("connectors: steps", small.steps_used() + small.order_steps, large.steps_used() + large.order_steps);
    println!(
        "COST connectors with line ends: 1000 then 4000: {} then {} edits, {} then {} bytes written, {} then {} attributes",
        small.edits, large.edits, small.bytes_written, large.bytes_written, small.attributes_written, large.attributes_written
    );
}

/// A `styles.xml` that holds `m` markers of its own and a save that adds one: the markers are walked once, charged, and read back in one pass, so the work grows with
/// the markers the file holds and not with the numbers asked for times them.
fn file_with_markers(m: usize) -> WriteWork {
    let held: String =
        (0..m).map(|k| format!("<draw:marker draw:name=\"m{k}\" svg:viewBox=\"0 0 10 10\" svg:d=\"M 0 0 L 10 0 L 5 10 Z\"/>")).collect();
    let mut o = open(&Drawing::a4().common(&held).page(&crate::write_core::custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut c = crate::write_new::connector(id, (1.0, 10.0, 8.0, 12.0));
    for (cell, v) in [("EndArrow", 4.0), ("EndArrowSize", 2.0)] {
        Arc::make_mut(&mut c.sheet)
            .cells
            .insert(diacraft_doc::CellName::new(cell), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(v).unwrap()));
    }
    crate::write_new::push_shape(&mut o.doc, c);
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let saved = saved.unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert_eq!(styles_text(&saved.bytes).matches("<draw:marker ").count(), m + 1);
    work
}

#[test]
fn a_file_with_n_and_4n_markers_costs_linear_work_to_add_one() {
    let (small, large) = (file_with_markers(5_000), file_with_markers(20_000));
    linear("markers held: bytes scanned", small.scan_bytes, large.scan_bytes);
    linear("markers held: bytes copied", small.bytes_copied, large.bytes_copied);
    linear("markers held: steps", small.steps_used() + small.order_steps, large.steps_used() + large.order_steps);
    println!(
        "COST a file holding markers, one added: 5000 then 20000: {} then {} bytes scanned, {} then {} steps",
        small.scan_bytes,
        large.scan_bytes,
        small.steps_used() + small.order_steps,
        large.steps_used() + large.order_steps
    );
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// the style of a run whose row changed is made from the style of the span the file has: the spans of a paragraph are looked at once for the paragraph, and
// each run finds its span by the place it stands in, so a paragraph of n runs costs n and not n times n.

/// A paragraph of `n` one-character spans, alternately bold (`T1`) and italic (`T2`), whose rows are two (rows 1 and 2): the `Size` of row 1 is edited, so that every
/// run of it takes the style of its own span with the size written.
fn runs_edited(n: usize) -> WriteWork {
    let styles = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:language="de"/></style:style><style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>"#;
    let spans: String = (0..n).map(|k| format!("<text:span text:style-name=\"T{}\">x</text:span>", 1 + k % 2)).collect();
    let body = format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>{spans}</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    );
    let mut o = open(&Drawing::a4().auto(styles).page(&body));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Character").expect("the runs have rows");
    let section = Arc::make_mut(section);
    let row = section.rows.iter_mut().find(|r| r.ix == Some(1)).expect("row 1");
    Arc::make_mut(row)
        .cells
        .insert(diacraft_doc::CellName::new("Size"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_number(20.0 / 72.0).expect("a number")));
    counted(&o)
}

#[test]
fn n_and_4n_runs_whose_row_changed_take_the_style_of_their_span_in_work_that_grows_with_n() {
    let (small, large) = (runs_edited(5_000), runs_edited(20_000));
    println!("COST runs edited: 5,000 runs {} steps, 20,000 runs {} steps", small.order_steps, large.order_steps);
    assert!(small.order_steps >= 5_000, "each run is a step: {}", small.order_steps);
    // Four times the runs, about four times the steps (a pass over the spans for each run is sixteen times).
    assert!(large.order_steps <= small.order_steps * 5, "{} steps at 20,000 against {} at 5,000", large.order_steps, small.order_steps);
}
