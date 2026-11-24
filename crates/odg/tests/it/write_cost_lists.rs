//! What the check of a save that put paragraphs in, took them out or made lists costs: the package made is read again through the reader, once,
//! to count the paragraphs of the shapes whose paragraphs changed, and not when only the characters of a paragraph changed; and a text that holds many lists, or
//! many items put in one list, costs in proportion to them, at n and 4n.

use diacraft_doc::TextPart;
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, open};
use crate::write_lists::bullet_row;
use crate::write_runs::{chars, set_parts, set_row, shape};

const STYLES: &str = r#"<style:style style:name="P1" style:family="paragraph"/><text:list-style style:name="N1"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style>"#;

fn drawing_of(inner: &str) -> Drawing {
    Drawing::a4().auto(STYLES).page(&shape("x", "1cm", inner))
}

fn counted(o: &Open) -> (WriteWork, usize) {
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("the save was refused: {e}"));
    assert!(saved.edited);
    (work, saved.bytes.len())
}

/// The read-back is the reader's: the paragraphs it reads are the paragraphs the model has.
#[test]
fn the_save_reads_the_package_again_once_for_a_shape_whose_paragraphs_changed_and_not_for_one_whose_characters_did() {
    let numbered = r#"<text:list text:style-name="N1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing_of(numbered));
    set_row(&mut o, "Paragraph", 1, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("one\ntwo\n"), TextPart::Pp(1), chars("new")]);
    let (work, size) = counted(&o);
    assert_eq!(work.read_back_bytes, size as u64, "the package made was read again, once");
    assert!(work.scan_bytes >= size as u64, "and what is read is charged to the bytes a save may scan");
    // A save that only edits the characters of a paragraph does not read the package again.
    let mut o = open(&drawing_of("<text:p>abc</text:p>"));
    set_parts(&mut o, vec![chars("abXc")]);
    assert_eq!(counted(&o).0.read_back_bytes, 0);
}

/// `n` paragraphs of a bulleted kind put after a plain one, each its own row apart from the first two (so that each is the item of a list of its own or an item
/// of a list): the work follows the paragraphs.
fn many_bulleted(n: usize, rows: bool) -> WorkOf {
    let mut o = open(&drawing_of("<text:p>zero</text:p>"));
    let mut parts = vec![chars("zero\n")];
    if !rows {
        // One row for all of them; with a row for each, the first paragraph's own row comes after the marker, and a row 1 would be no paragraph's.
        set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0)]);
        parts.push(TextPart::Pp(1));
    }
    for k in 0..n {
        if rows {
            // A row of its own for each: a list of its own for each.
            let ix = 2 + k as u32;
            set_row(&mut o, "Paragraph", ix, &[("Bullet", 1.0), ("IndLeft", 0.1 + f64::from(ix) / 1000.0)]);
            parts.push(TextPart::Pp(ix));
        }
        parts.push(chars(&format!("item {k}{}", if k + 1 < n { "\n" } else { "" })));
    }
    set_parts(&mut o, parts);
    let (work, size) = counted(&o);
    WorkOf { work, size }
}

struct WorkOf {
    work: WriteWork,
    size: usize,
}

fn linear(what: &str, small: u64, large: u64) {
    assert!(small > 0, "{what}: nothing counted");
    assert!(large <= small * 9 / 2 + 2_000, "{what}: {small} then {large} for 4 times the work");
}

/// Items of one list, in a row at one place: one list, a linear work. Lists each of its own row: one list style each, a linear work.
#[test]
fn n_and_4n_bulleted_paragraphs_are_written_in_linear_work() {
    for rows in [false, true] {
        let (small, large) = (many_bulleted(500, rows), many_bulleted(2_000, rows));
        let scenario = if rows { "a list for each paragraph" } else { "one list" };
        linear(&format!("{scenario}: edits"), small.work.edits, large.work.edits);
        linear(&format!("{scenario}: bytes written"), small.work.bytes_written, large.work.bytes_written);
        linear(&format!("{scenario}: attributes"), small.work.attributes_written, large.work.attributes_written);
        linear(&format!("{scenario}: steps"), small.work.steps_used() + small.work.order_steps, large.work.steps_used() + large.work.order_steps);
        linear(&format!("{scenario}: read again"), small.work.read_back_bytes, large.work.read_back_bytes);
        println!(
            "COST {scenario}: 500 then 2000 paragraphs: {} then {} edits, {} then {} bytes written, {} then {} bytes read again, {} then {} bytes made",
            small.work.edits,
            large.work.edits,
            small.work.bytes_written,
            large.work.bytes_written,
            small.work.read_back_bytes,
            large.work.read_back_bytes,
            small.size,
            large.size
        );
    }
}

/// The characters of a paragraph put in are copied once more into the element made, and charged: four times the characters cost four times the bytes copied
/// (the review).
#[test]
fn the_characters_of_an_inserted_paragraph_are_charged_as_copied() {
    let copied = |length: usize| {
        let mut o = open(&drawing_of("<text:p>zero</text:p>"));
        set_parts(&mut o, vec![chars(&format!("zero\n{}", "x".repeat(length)))]);
        counted(&o).0.bytes_copied
    };
    let (small, large) = (copied(10_000), copied(40_000));
    // The paragraph is built once and spliced in once: what is charged follows its characters, three times over.
    assert!(large - small >= 3 * 30_000, "{small} then {large} bytes copied for 10,000 then 40,000 characters");
}

/// Each marker the save writes is four attributes, charged: a file that holds the markers of the ends it names has four attributes fewer for each (the review of
#[test]
fn each_marker_written_is_four_attributes_charged() {
    use diacraft_doc::{Cell, CellName, Value};
    let connector_with = |o: &mut Open, numbers: &[f64]| {
        let id = o.index.next_id(0).unwrap();
        let mut c = crate::write_new::connector(id, (1.0, 10.0, 8.0, 12.0));
        for (k, n) in numbers.iter().enumerate() {
            let (arrow, size) = if k == 0 { ("EndArrow", "EndArrowSize") } else { ("BeginArrow", "BeginArrowSize") };
            for (cell, v) in [(arrow, *n), (size, 2.0)] {
                std::sync::Arc::make_mut(&mut c.sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_number(v).unwrap()));
            }
        }
        crate::write_new::push_shape(&mut o.doc, c);
    };
    let marker = |n: u32, d: &str| {
        format!(r#"<draw:marker draw:name="DiaCraft_20_end_20_{n}" draw:display-name="DiaCraft end {n}" svg:viewBox="0 0 10 10" svg:d="{d}"/>"#)
    };
    let attributes = |held: bool| {
        let held_markers = if held { format!("{}{}", marker(4, "M 0 0"), marker(10, "M 0 0")) } else { String::new() };
        let mut o = open(&Drawing::a4().common(&held_markers).page(&crate::write_core::custom("s1", "1cm")));
        connector_with(&mut o, &[4.0, 10.0]);
        counted(&o).0.attributes_written
    };
    assert_eq!(attributes(false) - attributes(true), 8, "two markers written, four attributes each");
}

/// A new face whose family is not the file's but whose name is: the file holds `n` faces named `F`, `F 2` to `F n` (of other families), and a run asks for the
/// family `F`: the face is named `F` and the next number, found by one look in an index of the names for each number tried, each a step. The steps follow `n`, at
/// least one for each name that is taken and at most a few times that (a look through the faces for each number tried was quadratic and charged nothing: the
/// review's security finding).
fn faces_named_alike(n: usize) -> (WriteWork, usize) {
    let o = taken_names(n);
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{e}"));
    let after = crate::write_core::part(&saved.bytes, "content.xml");
    assert!(after.contains(&format!("style:name=\"F {}\" svg:font-family=\"'F'\"", n + 1)), "the face is named after the last number the file has");
    (work, n)
}

/// The file holds `n` faces named `F`, `F 2` to `F n`, and a run of the one text asks for the family `F`.
fn taken_names(n: usize) -> Open {
    use diacraft_doc::{Cell, CellName, Row, Section, Value};
    let mut d = drawing_of("<text:p>plain</text:p>");
    let mut fonts = String::from(r#"<style:font-face style:name="F" svg:font-family="Other 1"/>"#);
    for k in 2..=n {
        fonts.push_str(&format!(r#"<style:font-face style:name="F {k}" svg:font-family="Other {k}"/>"#));
    }
    d.fonts = fonts;
    let mut o = open(&d);
    let id = crate::write_runs::id_of(&o.doc);
    {
        let sheet = std::sync::Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        let mut section = Section::new("Character", None);
        let mut row = Row { ix: Some(1), ..Row::default() };
        row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text("F")));
        section.rows.push(std::sync::Arc::new(row));
        sheet.sections.push(std::sync::Arc::new(section));
    }
    set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
    o
}

/// The numbers tried for a face's name are steps against the cap, and the walk stops where the cap is passed (it does not go on to the end of the names that are
/// taken and leave the refusal to the end of the part; the mutation of `add_face`).
#[test]
fn the_numbers_tried_for_a_face_name_stop_at_the_step_cap() {
    let o = taken_names(3_000);
    let mut work = WriteWork::default();
    let options = WriteOptions { limits: diacraft_odg::Limits { max_write_steps: 1_000, ..Default::default() }, ..Default::default() };
    let refused = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work).err().map(|e| e.class());
    assert_eq!(refused, Some("write_limit"));
    assert!(work.steps_used() <= 1_010, "{} steps taken for a cap of 1,000", work.steps_used());
}

/// Each attribute of a list style is charged: its own three, the label's indent and margin (one each, and three more for the two elements that hold them), the
/// face and the size of the bullet (the mutation of `list_style`).
#[test]
fn each_attribute_of_a_list_style_is_charged() {
    let attributes = |cells: &[(&str, f64)], words: &[(&str, &str)]| {
        let mut d = drawing_of("<text:p>zero</text:p>");
        d.fonts = r#"<style:font-face style:name="Symbol" svg:font-family="Symbol"/>"#.to_owned();
        let mut o = open(&d);
        let mut all = vec![("Bullet", 1.0)];
        all.extend_from_slice(cells);
        bullet_row(&mut o, 1, &all, words);
        set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
        counted(&o).0.attributes_written
    };
    let base = attributes(&[], &[]);
    assert_eq!(attributes(&[("IndFirst", -0.25)], &[]) - base, 4, "an indent and the two elements");
    assert_eq!(attributes(&[("IndLeft", 0.5)], &[]) - base, 4, "a margin and the two elements");
    assert_eq!(attributes(&[("IndFirst", -0.25), ("IndLeft", 0.5)], &[]) - base, 5, "both, and the two elements once");
    assert_eq!(attributes(&[], &[("BulletFont", "Symbol")]) - base, 1, "a face the file declares");
    assert_eq!(attributes(&[("BulletFontSize", 0.125)], &[]) - base, 1, "a size");
    assert_eq!(attributes(&[("IndFirst", -0.25), ("IndLeft", 0.5), ("BulletFontSize", 0.125)], &[("BulletFont", "Symbol")]) - base, 7, "all of them");
}

#[test]
fn faces_named_alike_cost_steps_that_follow_them_and_not_their_square() {
    let ((small, n), (large, m)) = (faces_named_alike(2_000), faces_named_alike(8_000));
    let (a, b) = (small.steps_used() + small.order_steps, large.steps_used() + large.order_steps);
    // The index of the families, the index of the names and one step for each number that is taken, at least (the families alone are one step a face), and linear.
    assert!(b - a >= 2 * (m - n) as u64, "the names that are taken are charged: {a} then {b} steps for {n} then {m} faces");
    assert!(b <= a * 9 / 2 + 2_000, "{a} then {b} steps for 4 times the faces");
}

/// A new list style (and so a new style of any family) beside many names that are held and no style has (a bookmark named `N2` to `Nn`): the numbers are tried
/// once each, by one look in a set, so the save takes a short time at 20,000 names and at 80,000 (a name is skipped once, and the next starts after it).
#[test]
fn a_new_list_style_beside_many_held_names_is_found_in_linear_time() {
    let run = |n: usize| {
        let marks: String = (2..=n).map(|k| format!("<text:bookmark text:name=\"N{k}\"/>")).collect();
        let mut o = open(&drawing_of(&format!("<text:p>zero{marks}</text:p>")));
        set_row(&mut o, "Paragraph", 1, &[("Bullet", 1.0)]);
        set_parts(&mut o, vec![chars("zero\n"), TextPart::Pp(1), chars("item")]);
        let started = std::time::Instant::now();
        let (work, _) = counted(&o);
        (started.elapsed(), work)
    };
    let ((small_time, small), (large_time, large)) = (run(20_000), run(80_000));
    assert!(large.steps_used() + large.order_steps <= (small.steps_used() + small.order_steps) * 9 / 2 + 2_000);
    assert!(
        large_time < std::time::Duration::from_secs(10) && large_time < small_time * 40 + std::time::Duration::from_millis(500),
        "{small_time:?} then {large_time:?}"
    );
}

/// Each paragraph a save puts in is checked for the paragraph style the writer named for it, in the part made (the mutation of the plumbing in `splice_part` and
/// `apply_shape`: the check never ran when its list stayed empty): the counts follow the paragraphs put in, and a save that puts none in checks none.
#[test]
fn each_paragraph_put_in_is_checked_for_its_style() {
    let inserted = |inner: &str, parts: Vec<TextPart>, rows: &[(u32, &[(&str, f64)])]| {
        let mut o = open(&drawing_of(inner));
        for (ix, cells) in rows {
            set_row(&mut o, "Paragraph", *ix, cells);
        }
        set_parts(&mut o, parts);
        counted(&o).0
    };
    // Three paragraphs put in after a plain one: two of the row of a list (a new list), one plain.
    let work = inserted(
        "<text:p>zero</text:p>",
        vec![chars("zero\n"), TextPart::Pp(1), chars("b1\nb2\n"), TextPart::Pp(2), chars("plain")],
        &[(1, &[("Bullet", 1.0)]), (2, &[("HorzAlign", 2.0)])],
    );
    assert_eq!((work.checked_paragraphs, work.checked_inserted), (3, 3), "the three paragraphs put in");
    // A paragraph put in and one retyped: both are checked for their text, and one for its style.
    let work = inserted("<text:p>abc</text:p><text:p>def</text:p>", vec![chars("abcX\nnew\ndef")], &[]);
    assert_eq!((work.checked_paragraphs, work.checked_inserted), (2, 1));
    // Only characters typed: no paragraph is put in.
    let work = inserted("<text:p>abc</text:p>", vec![chars("abcd")], &[]);
    assert_eq!((work.checked_paragraphs, work.checked_inserted), (1, 0));
}
