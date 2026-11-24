//! What the writer's new edits cost (the lessons of the rounds: count the units each loop walks, with chained sizes, and let a ratio of
//! 4 for 4 times the work be the proof of linear): restyles that clone, shapes taken out with their styles, retexts, new shapes and connectors, and a
//! page resized with many shapes on it. Each scenario is run at two sizes, 4 times apart, and the counts the save keeps ([`diacraft_odg::WriteWork`])
//! are compared: the edits made, the bytes copied and written, and the steps of the lookups.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Value};
use diacraft_odg::{OriginalPackage, WriteOptions, WriteWork, write_counted};

use crate::drawing::{Drawing, cm};
use crate::write_core::{Open, open};

/// What a save counted, by name.
#[derive(Clone, Copy, Debug)]
struct Counts {
    edits: u64,
    bytes_copied: u64,
    bytes_written: u64,
    scan_bytes: u64,
    steps: u64,
}

fn save_counts(o: &Open) -> Counts {
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let saved = saved.unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()));
    assert!(saved.edited);
    Counts {
        edits: work.edits,
        bytes_copied: work.bytes_copied,
        bytes_written: work.bytes_written,
        scan_bytes: work.scan_bytes,
        steps: work.steps_used() + work.order_steps,
    }
}

/// `large` is at most 4.5 times `small` (4 for the work, and a little for the names, which gain a digit), where quadratic would be 16.
#[track_caller]
fn linear(what: &str, small: u64, large: u64) {
    assert!(small > 0, "{what}: nothing counted");
    assert!(large <= small * 9 / 2 + 400, "{what}: {small} then {large} for 4 times the work");
}

fn assert_linear(scenario: &str, small: Counts, large: Counts) {
    linear(&format!("{scenario} edits"), small.edits, large.edits);
    linear(&format!("{scenario} bytes copied"), small.bytes_copied, large.bytes_copied);
    linear(&format!("{scenario} bytes written"), small.bytes_written, large.bytes_written);
    linear(&format!("{scenario} bytes scanned"), small.scan_bytes, large.scan_bytes);
    linear(&format!("{scenario} steps"), small.steps, large.steps);
    println!("COST {scenario}: {small:?} then {large:?}");
}

fn rect(name: &str, style: &str, k: usize) -> String {
    format!(
        "\n    <draw:rect draw:name=\"{name}\" draw:style-name=\"{style}\" svg:x=\"{}cm\" svg:y=\"3cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p>t{k}</text:p></draw:rect>",
        k % 20
    )
}

fn gr(name: &str) -> String {
    format!(
        "<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\"/></style:style>"
    )
}

fn set_text(doc: &mut Document, at: usize, cell: &str, text: &str) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

/// `n` shapes on one shared style, each restyled: `n` clones.
fn restyle_each(n: usize) -> Counts {
    let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr1", k)).collect();
    let mut o = open(&Drawing::a4().auto(&gr("gr1")).page(&body));
    for k in 0..n {
        set_text(&mut o.doc, k, "FillForegnd", "#336699");
    }
    save_counts(&o)
}

/// `n` shapes, each on a style of its own, all deleted: `n` styles are orphans.
fn delete_all(n: usize) -> Counts {
    let styles: String = (0..n).map(|k| gr(&format!("gr{k}"))).collect();
    let body: String = (0..n).map(|k| rect(&format!("s{k}"), &format!("gr{k}"), k)).collect();
    let mut o = open(&Drawing::a4().auto(&styles).page(&body));
    // One shape of another style stays, so that the page and the styles are not empty.
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    while !page.shapes.is_empty() {
        page.shapes.remove(0);
    }
    save_counts(&o)
}

/// `n` shapes whose text is retyped.
fn retext_each(n: usize) -> Counts {
    let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr1", k)).collect();
    let mut o = open(&Drawing::a4().auto(&gr("gr1")).page(&body));
    for k in 0..n {
        let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(k).unwrap());
        shape.text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::from(format!("u{k}")))], ..Default::default() }));
    }
    save_counts(&o)
}

/// A page of 100 shapes made taller: every shape's `svg:y` is written anew.
fn resize_page(n: usize) -> Counts {
    let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr1", k)).collect();
    let mut o = open(&Drawing::a4().auto(&gr("gr1")).page(&body));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut page.sheet);
    sheet.cells.get_mut("PageHeight").unwrap().value = Value::from_number(cm(42.0));
    save_counts(&o)
}

#[test]
fn a_restyle_that_clones_for_each_of_n_shapes_costs_work_that_follows_n() {
    assert_linear("restyle", restyle_each(250), restyle_each(1_000));
}

#[test]
fn taking_out_n_shapes_and_the_n_styles_they_alone_named_costs_work_that_follows_n() {
    assert_linear("delete", delete_all(250), delete_all(1_000));
}

#[test]
fn a_retext_of_n_shapes_costs_work_that_follows_n() {
    assert_linear("retext", retext_each(250), retext_each(1_000));
}

#[test]
fn a_resize_of_a_page_of_n_shapes_costs_work_that_follows_n() {
    assert_linear("resize", resize_page(250), resize_page(1_000));
}

#[test]
fn a_style_shared_by_n_shapes_and_edited_for_one_costs_the_style_and_not_n_times_it() {
    // One shape of 2,000 restyled: the references are counted in one pass and the clone is one style, whatever the others are.
    let one = |n: usize| {
        let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr1", k)).collect();
        let mut o = open(&Drawing::a4().auto(&gr("gr1")).page(&body));
        set_text(&mut o.doc, 0, "FillForegnd", "#336699");
        save_counts(&o)
    };
    let (small, large) = (one(500), one(2_000));
    assert!(
        large.edits <= small.edits + 4,
        "the edits are the shape's and the style's, however many shapes name the style: {small:?} then {large:?}"
    );
    linear("scanned", small.scan_bytes, large.scan_bytes);
    linear("copied", small.bytes_copied, large.bytes_copied);
}

#[test]
fn n_new_shapes_on_a_page_of_n_cost_work_that_follows_n() {
    let new = |n: usize| {
        let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr1", k)).collect();
        let mut o = open(&Drawing::a4().auto(&gr("gr1")).page(&body));
        let first = o.index.next_id(0).unwrap();
        for k in 0..n {
            crate::write_new::push_shape(&mut o.doc, crate::write_new::rectangle(first + k as u32, &format!("R{k}"), (2.0, 3.0, 4.0, 2.0)));
        }
        save_counts(&o)
    };
    assert_linear("new shapes", new(250), new(1_000));
}

/// Timed: one shape of 320,000 paragraphs, each with a span, every one retyped. A walk of all the markers for each edited paragraph
/// made this save take 69.7 s against a read of 1.1 s; the markers are now passed once by a cursor. The yardstick is the read of the same
/// file in the same process, which scales with the machine's load as the save does: the save is at most ten times it, and a second over.
#[test]
fn a_retext_of_320000_paragraphs_with_spans_takes_a_few_times_the_read_of_the_file() {
    let paras: String =
        (0..320_000).map(|k| format!(r#"<text:p text:style-name="P1"><text:span text:style-name="T1">a</text:span>b{k}</text:p>"#)).collect();
    let styles = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"##;
    let d = Drawing::a4().auto(styles).page(&format!(
        r#"<draw:custom-shape draw:name="t" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm">{paras}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#
    ));
    let bytes = d.bytes();
    let started = std::time::Instant::now();
    let mut o = crate::write_core::open_bytes(bytes);
    let read = started.elapsed();
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    let shape = crate::write_core::shape_mut(&mut o.doc, 0, id);
    let markers = shape.text.as_ref().unwrap().parts.iter().filter(|p| !matches!(p, diacraft_doc::TextPart::Chars(_))).count();
    assert!(markers >= 320_000, "a marker or more for each paragraph: {markers}");
    let parts: Vec<diacraft_doc::TextPart> = shape
        .text
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .cloned()
        .map(|p| if let diacraft_doc::TextPart::Chars(c) = p { diacraft_doc::TextPart::Chars(Arc::from(c.replace('b', "c"))) } else { p })
        .collect();
    shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
    let mut work = WriteWork::default();
    let started = std::time::Instant::now();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
    let save = started.elapsed();
    println!("COST retext of 320,000 paragraphs with spans: read {read:?}, save {save:?}, steps {}", work.steps_used());
    assert!(saved.is_ok_and(|w| w.edited));
    assert!(save.as_secs_f64() < 10.0 * read.as_secs_f64() + 1.0, "the save took {save:?} against a read of {read:?}");
}

/// A colour set on shapes whose styles state no fill asks the style sheets of the drawing for the fill pattern the model draws them
/// with. The sheets are indexed **once** for the whole save and each sheet costs a step, and each shape costs a few steps for the chain it looks through: the
/// same recolouring of shapes whose styles do state the fill costs the same without them, so the difference is the lookup: 1,500 common styles whether 50 or
/// 200 shapes ask, and a few steps for each of 150 more.
#[test]
fn the_fill_pattern_a_colour_asks_for_costs_one_index_of_the_style_sheets_and_a_few_steps_for_each_shape() {
    let common: String = (0..1_500)
        .map(|k| format!("<style:style style:name=\"C{k}\" style:family=\"graphic\"><style:graphic-properties draw:stroke=\"none\"/></style:style>"))
        .collect();
    let stated = "<style:style style:name=\"gr9\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#ff0000\"/></style:style>";
    let unstated = "<style:style style:name=\"gr9\" style:family=\"graphic\"><style:graphic-properties draw:fill-color=\"#ff0000\"/></style:style>";
    let steps = |auto: &str, n: usize| {
        let body: String = (0..n).map(|k| rect(&format!("s{k}"), "gr9", k)).collect();
        let mut o = open(&Drawing::a4().common(&common).auto(auto).page(&body));
        for k in 0..n {
            set_text(&mut o.doc, k, "FillForegnd", "#336699");
        }
        save_counts(&o).steps
    };
    let lookup = |n: usize| steps(unstated, n).saturating_sub(steps(stated, n));
    let (small, large) = (lookup(50), lookup(200));
    println!("COST fill pattern of a colour with 1,500 common styles, the lookup alone: {small} on 50 shapes and {large} on 200");
    assert!(small >= 1_500, "the index of the sheets is counted, a step each: {small}");
    assert!(large > small && large - small <= 150 * 4, "150 more shapes cost {} steps: the index is built once", large.saturating_sub(small));
}
