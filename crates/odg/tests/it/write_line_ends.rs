//! Line ends 1 to 45 written as DiaCraft's own markers in every size class, and read back by number and class (
//! on the writer's side). The reader's vectors are in
//! `read_line_ends.rs`.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{Case, Saver, echo_control, num_of, open, open_bytes, part, real, shape_mut};
use crate::write_markers::{arrowed, attr, base, markers, points, set};
use crate::write_new::push_shape;

/// A new connector with an end of `number` and `size` at `weight` points, saved onto an empty page.
fn saved_end(save: &Saver, number: f64, size: f64, weight: f64) -> Vec<u8> {
    let mut o = open(&base());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((number, size)), weight));
    save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{number} at size {size} and {weight} pt: {e}")).bytes
}

/// The only marker of `styles.xml` and its attributes.
fn the_marker(bytes: &[u8]) -> String {
    let held = markers(bytes);
    assert_eq!(held.len(), 1, "{held:?}");
    held[0].clone()
}

/// The number and the size class the first shape of the saved file reads back as, for its end.
fn read_end(bytes: &[u8]) -> (f64, f64) {
    let back = open_bytes(bytes.to_vec());
    let s = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
    (num_of(s, "EndArrow"), num_of(s, "EndArrowSize"))
}

/// The band class a mark at size class `class` and `weight` points has, by the rule worked out here from the bases: round(100 w / S), between 1 and 99.
fn band_class(class: usize, weight: f64) -> u32 {
    let base = [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0][class];
    let w = if weight > 0.0 { weight } else { 0.25 };
    ((100.0 * w / (base + weight)).round() as i64).clamp(1, 99) as u32
}

const UNBANDED: [u32; 10] = [2, 4, 5, 6, 8, 10, 11, 13, 39, 42];

/// `EndArrow` 16, size 2, 0.75 pt: S 3.27 pt, band class 23, the marker named for it with its display name, 6.54 pt wide, not centred, drawn as the outer
/// triangle and the hole against it; read back as 16 and 2, and a second save is no edit.
fn case_32(save: &Saver) {
    let bytes = saved_end(save, 16.0, 2.0, 0.75);
    let after = part(&bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_16_20_t23"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some("6.54pt"), "{after}");
    assert!(!after.contains("marker-end-center"), "{after}");
    let held = the_marker(&bytes);
    assert!(
        held.contains(r#"draw:name="DiaCraft_20_end_20_16_20_t23" draw:display-name="DiaCraft end 16 t23" svg:viewBox="0 0 2000 2000""#),
        "{held}"
    );
    assert!(held.contains(r#"svg:d="M 1000 0 L 2000 2000 L 0 2000 Z M 1000 514 L 372 1770 L 1628 1770 Z""#), "{held}");
    assert_eq!(read_end(&bytes), (16.0, 2.0));
    let o = open_bytes(bytes.clone());
    let again = real(&o.doc, &bytes, &o.index).unwrap();
    assert!(!again.edited && again.bytes == bytes);
}

/// The same at 5 pt: S 7.52 pt, band class 66, 15.04 pt wide, drawn as the whole triangle (the band is wider than the inradius).
fn case_33(save: &Saver) {
    let bytes = saved_end(save, 16.0, 2.0, 5.0);
    let after = part(&bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_16_20_t66"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some("15.04pt"), "{after}");
    let held = the_marker(&bytes);
    assert!(held.contains(r#"svg:viewBox="0 0 2000 2000" svg:d="M 1000 0 L 2000 2000 L 0 2000 Z""#), "{held}");
    assert_eq!(read_end(&bytes), (16.0, 2.0));
}

/// `EndArrow` 24, size 2, 0.75 pt: 1.5 x 3.27 = 4.905 pt wide, the box 1500 wide and 1615 high, the spine and the bar one contour.
fn case_34(save: &Saver) {
    let bytes = saved_end(save, 24.0, 2.0, 0.75);
    let after = part(&bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_24_20_t23"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some("4.905pt"), "{after}");
    let held = the_marker(&bytes);
    assert!(held.contains(r#"svg:viewBox="0 0 1500 1615""#), "{held}");
    assert!(held.contains(r#"svg:d="M 635 0 L 865 0 L 865 1385 L 1500 1385 L 1500 1615 L 0 1615 L 0 1385 L 635 1385 Z""#), "{held}");
    assert_eq!(read_end(&bytes), (24.0, 2.0));
}

/// `EndArrow` 9, size 2, 0.75 pt: centred, band class 23, the box 2000 by 2326 with the vertex at its centre.
fn case_35(save: &Saver) {
    let bytes = saved_end(save, 9.0, 2.0, 0.75);
    let after = part(&bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_9_20_t23"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-center").as_deref(), Some("true"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some("6.54pt"), "{after}");
    assert!(the_marker(&bytes).contains(r#"svg:viewBox="0 0 2000 2326""#));
    assert_eq!(read_end(&bytes), (9.0, 2.0));
}

/// The same hollow triangle on a line of weight 0: the band is 0.25 pt over the class's base, 0.25 / 2.52, class 10, and the width is 2 x 2.52 = 5.04 pt.
fn case_36(save: &Saver) {
    let bytes = saved_end(save, 16.0, 2.0, 0.0);
    let after = part(&bytes, "content.xml");
    assert_eq!(attr(&after, "draw:marker-end").as_deref(), Some("DiaCraft_20_end_20_16_20_t10"), "{after}");
    assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some("5.04pt"), "{after}");
    assert_eq!(read_end(&bytes), (16.0, 2.0));
}

/// A new marker for 14, size 2, 0.75 pt has its display name written (a program that saves the file builds the name from it).
fn case_38(save: &Saver) {
    let held = the_marker(&saved_end(save, 14.0, 2.0, 0.75));
    assert!(held.contains(r#"draw:name="DiaCraft_20_end_20_14_20_t23" draw:display-name="DiaCraft end 14 t23""#), "{held}");
}

/// The numbers no source measures are written in the box of DiaCraft's rule: 34 at size 2 is 2.24 x 3.27 = 7.3248 pt and at size 3 2.24 x 3.99 = 8.9376 pt,
/// 35 is 1.5 x 3.27 = 4.905 pt, and each reads back as its number and size; 44 has a box 2865 high; 8 is a solid head with no band, and 19 its hollow form.
fn case_40(save: &Saver) {
    for (n, size, width) in [(34.0, 2.0, "7.3248pt"), (35.0, 2.0, "4.905pt"), (34.0, 3.0, "8.9376pt")] {
        let bytes = saved_end(save, n, size, 0.75);
        let after = part(&bytes, "content.xml");
        assert_eq!(attr(&after, "draw:marker-end-width").as_deref(), Some(width), "{n} at {size}: {after}");
        assert_eq!(read_end(&bytes), (n, size), "{n} at {size}");
    }
    let held = the_marker(&saved_end(save, 44.0, 2.0, 0.75));
    assert!(held.contains(r#"draw:name="DiaCraft_20_end_20_44_20_t23""#) && held.contains(r#"svg:viewBox="0 0 2000 2865""#), "{held}");
    let bytes = saved_end(save, 8.0, 2.0, 0.75);
    let held = the_marker(&bytes);
    assert!(held.contains(r#"draw:name="DiaCraft_20_end_20_8" draw:display-name="DiaCraft end 8" svg:viewBox="0 0 2000 2000""#), "{held}");
    assert!(held.contains(r#"svg:d="M 1000 0 L 2000 2000 L 1000 1500 L 0 2000 Z""#), "{held}");
    assert_eq!(attr(&part(&bytes, "content.xml"), "draw:marker-end-width").as_deref(), Some("6.54pt"));
    assert_eq!(read_end(&bytes), (8.0, 2.0));
    let bytes = saved_end(save, 19.0, 2.0, 0.75);
    let held = the_marker(&bytes);
    assert!(held.contains(r#"draw:name="DiaCraft_20_end_20_19_20_t23""#) && held.contains(r#"svg:viewBox="0 0 2000 2000""#), "{held}");
    assert!(held.contains(r#"svg:d="M 1000 0 L 2000 2000 L 1000 1500 L 0 2000 Z M 1000 514 L 514 1486 L 1000 1243 L 1486 1486 Z""#), "{held}");
    assert_eq!(read_end(&bytes), (19.0, 2.0));
}

/// Class 6 has a base of 18 pt: `EndArrow` 13 at size 6 and 1 pt is 2 x (18 + 1) = 38 pt wide and reads back as size 6.
fn case_41(save: &Saver) {
    let bytes = saved_end(save, 13.0, 6.0, 1.0);
    assert_eq!(attr(&part(&bytes, "content.xml"), "draw:marker-end-width").as_deref(), Some("38pt"));
    assert_eq!(read_end(&bytes), (13.0, 6.0));
}

const CASES: [Case; 9] = [
    ("v32", case_32),
    ("v33", case_33),
    ("v34", case_34),
    ("v35", case_35),
    ("v36", case_36),
    ("v38", case_38),
    ("v40", case_40),
    ("v41", case_41),
    ("v39", case_39),
];

#[test]
fn odg_styles_a_hollow_triangle_is_named_by_its_band_class_and_drawn_as_a_triangle_and_the_hole_against_it() {
    case_32(&real);
}

#[test]
fn odg_styles_a_band_wider_than_the_inradius_writes_the_whole_triangle() {
    case_33(&real);
}

#[test]
fn odg_styles_a_crossbar_and_the_spine_are_one_contour_in_a_box_1500_by_1615() {
    case_34(&real);
}

#[test]
fn odg_styles_the_slanted_stroke_through_the_vertex_is_centred() {
    case_35(&real);
}

#[test]
fn odg_styles_a_line_of_weight_0_has_a_band_of_a_quarter_of_a_point() {
    case_36(&real);
}

#[test]
fn odg_styles_a_new_marker_has_its_display_name_written() {
    case_38(&real);
}

#[test]
fn odg_styles_the_numbers_no_source_measures_are_written_in_the_box_of_dia_craft_s_rule() {
    case_40(&real);
}

/// Class 6 is written at a base of 18 pt; a class past 6 has no art (the file would read it back as class 2): no marker is written for it and its cells are
/// carried by DiaCraft's extension, so the model reads back as it was.
#[test]
fn odg_styles_class_6_is_written_at_a_base_of_18_pt_and_class_7_is_carried_with_no_marker() {
    case_41(&real);
    let o = open(&base());
    let id = o.index.next_id(0).unwrap();
    for size in [7.0, 8.0, 100.0] {
        let mut doc = o.doc.clone();
        push_shape(&mut doc, arrowed(id, None, Some((13.0, size)), 1.0));
        let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("size {size}: {e}"));
        assert!(!part(&saved.bytes, "content.xml").contains("draw:marker-end="), "size {size}: no marker is written");
        assert!(markers(&saved.bytes).is_empty(), "size {size}: no marker of ours in `styles.xml`");
        assert_eq!(read_end(&saved.bytes), (13.0, size), "size {size}: the cells read back as the model has them");
    }
}

/// Once tier 2 is written: a new line whose `EndArrow` is 254 by the formula `USE("Fancy end")`, with `EndArrowSize` 2 and 0.75 pt, is written
/// with `draw:marker-end="DiaCraft_20_end_20_4"` at 6.54 pt, the cell's formula and its value 254 as tier 2 (number 4's marker alone would read back as 4 without a
/// word), and reads back `EndArrow` 254 with that formula; a second save is no edit. The same for the begin, and for a plain 254. (The vector's third sentence, a program
/// that re-marks the end so that the 254 is dropped, is tier 3's.)
#[test]
fn odg_styles_a_custom_line_end_is_written_as_number_4_s_marker_and_its_cell_is_carried() {
    let o = open(&base());
    let id = o.index.next_id(0).unwrap();
    for begin in [false, true] {
        for formula in [Some(r#"USE("Fancy end")"#), None] {
            let mut c = arrowed(id, None, None, 0.75);
            let (arrow, size, local) = if begin { ("BeginArrow", "BeginArrowSize", "start") } else { ("EndArrow", "EndArrowSize", "end") };
            Arc::make_mut(&mut c.sheet).cells.insert(CellName::new(arrow), Cell::read(formula, Some("254"), None, None, &mut 1_000));
            set(&mut c, size, 2.0);
            let mut doc = o.doc.clone();
            push_shape(&mut doc, c);
            let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("begin {begin} {formula:?}: {e}"));
            let content = part(&saved.bytes, "content.xml");
            let what = format!("begin {begin} {formula:?}: {content}");
            assert_eq!(attr(&content, &format!("draw:marker-{local}")).as_deref(), Some("DiaCraft_20_end_20_4"), "{what}");
            assert_eq!(attr(&content, &format!("draw:marker-{local}-width")).as_deref(), Some("6.54pt"), "{what}");
            assert_eq!(markers(&saved.bytes).len(), 1, "number 4's marker is in `styles.xml`: {what}");
            let back = open_bytes(saved.bytes.clone());
            let cell = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cell(arrow).cloned().expect("the cell reads back");
            assert_eq!(cell.value.as_ref().and_then(diacraft_doc::Value::number), Some(254.0), "{what}");
            assert_eq!(cell.formula.text(), formula, "{what}: the formula, verbatim");
            let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
            assert!(!again.edited && again.bytes == saved.bytes, "{what}: a second save is no edit");
        }
    }
}

/// The words of the refusal of a weight no marker is as wide as: a save that fails with them, and with nothing written.
fn assert_weight_refused(result: Result<diacraft_odg::Written, WriteError>, what: &str) {
    match result {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("line end") && words.contains("a million points"), "{what}: {words}"),
        other => panic!("{what}: expected the refusal of the weight, got {:?}", other.map(|w| String::from_utf8_lossy(&w.bytes).len())),
    }
}

/// As the tier 2 carries a cell the table has no art for: a new line whose weight is a length no marker can be as wide as is written with
/// **no marker** and its line-end cells carried (the model reads back as it was), for a number with a band (16) and one without (13), at either end: no width of `inf`
/// or of three hundred digits, and no band class that is not a number, is written. The weight is the cell's own, in inches: 1e5 inches is 7.2 million points and 1e14
/// inches is 7.2e15, stroke widths the writer says; 1e300, 3e307 and `f64::MAX / 72` are lengths it cannot say, which the save refuses as it does for a line with no end.
/// The cap itself, 1e6 points, is written with its marker, and a point over it is not.
#[test]
fn a_new_line_whose_weight_no_marker_can_be_as_wide_as_has_its_end_carried_with_no_marker_for_a_number_with_a_band_and_one_without() {
    let o = open(&base());
    let id = o.index.next_id(0).unwrap();
    let carried = |c: Shape, what: &str| {
        let mut doc = o.doc.clone();
        push_shape(&mut doc, c);
        let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{what}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert!(!after.contains("draw:marker-end") && !after.contains("draw:marker-start"), "{what}: no marker is named: {after}");
        assert!(markers(&saved.bytes).is_empty(), "{what}: no marker of ours in `styles.xml`");
        assert!(!after.contains("inf") && !after.contains("e+"), "{what}: no width of `inf` or with an exponent: {after}");
        saved.bytes
    };
    for inches in [1e5, 1e14] {
        for number in [16.0, 13.0] {
            let mut c = arrowed(id, None, Some((number, 2.0)), 0.75);
            set(&mut c, "LineWeight", inches);
            let bytes = carried(c, &format!("end {number} at {inches} in"));
            assert_eq!(read_end(&bytes), (number, 2.0), "end {number} at {inches} in: the cells read back");
            let mut c = arrowed(id, Some((number, 2.0)), None, 0.75);
            set(&mut c, "LineWeight", inches);
            let bytes = carried(c, &format!("begin {number} at {inches} in"));
            let back = open_bytes(bytes);
            let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap();
            assert_eq!((num_of(read, "BeginArrow"), num_of(read, "BeginArrowSize")), (number, 2.0), "begin {number} at {inches} in");
        }
    }
    // A length the writer cannot say: the stroke width is refused whatever the line's ends are.
    for inches in [1e300, 3e307, f64::MAX / 72.0] {
        for number in [16.0, 13.0] {
            let mut c = arrowed(id, None, Some((number, 2.0)), 0.75);
            set(&mut c, "LineWeight", inches);
            let mut doc = o.doc.clone();
            push_shape(&mut doc, c);
            match real(&doc, &o.bytes, &o.index) {
                Err(WriteError::Unsupported(words)) => assert!(words.contains("too large"), "{inches} in: {words}"),
                other => panic!("{inches} in: expected a refusal of the length, got {:?}", other.map(|w| w.bytes.len())),
            }
        }
    }
    // The heaviest line whose end is written: 1e6 points is the cap and has its marker, and a point over it has none.
    for number in [16.0, 13.0] {
        let mut c = arrowed(id, None, Some((number, 2.0)), 1_000_000.0);
        set(&mut c, "LineWeight", 1_000_000.0 / 72.0);
        let mut doc = o.doc.clone();
        push_shape(&mut doc, c);
        let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{number}: the cap itself is written: {e}"));
        let width = attr(&part(&saved.bytes, "content.xml"), "draw:marker-end-width").unwrap();
        assert!(points(&width) > 2_000_000.0 && points(&width) < 2_240_100.0, "{number}: {width}");
        let mut c = arrowed(id, None, Some((number, 2.0)), 0.75);
        set(&mut c, "LineWeight", 1_000_001.0 / 72.0);
        let bytes = carried(c, &format!("{number} a point over the cap"));
        assert_eq!(read_end(&bytes), (number, 2.0));
    }
}

/// The weight of a line the file holds, whose style names a marker of DiaCraft's own, is set to a length no marker can be as wide as:
/// the save is refused in the words of the weight, and not written with a width of millions of points (1e5 inches is 7.2 million points, a stroke width the file
/// holds) or of no width at all.
#[test]
fn a_weight_edit_that_no_marker_can_be_as_wide_as_is_refused_in_words_and_not_dropped() {
    for inches in [1e5, 1e300, 3e307] {
        let mut o = two_lines(&format!("{MARKER_T23}{MARKER_T44}"));
        delete_second(&mut o);
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        set(shape_mut(&mut o.doc, 0, id), "LineWeight", inches);
        assert_weight_refused(real(&o.doc, &o.bytes, &o.index), &format!("an edit to {inches} in"));
    }
}

/// A drawing of two lines whose style `gr1` has the stroke width `weight` (a length as the file writes it) and no line end.
fn heavy_lines(weight: &str) -> crate::write_core::Open {
    let style = format!(
        r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties svg:stroke-width="{weight}" draw:stroke="solid"/></style:style>"#
    );
    open(&Drawing::a4().auto(&style).page(&two_connectors().replace("gr2", "gr1")))
}

/// A line the file holds with a weight past what a marker can be as wide as, given an end (the weight is the file's own and not changed, so
/// no stroke width is written to refuse it first): refused in words, for a number with a band and one without, whatever the unit of the file's length.
#[test]
fn an_end_set_on_a_line_the_file_holds_with_a_hostile_weight_is_refused_in_words() {
    let weights = [
        format!("1{}pt", "0".repeat(300)),
        format!("3{}in", "0".repeat(307)),
        format!("2{}in", "0".repeat(306)),
        format!("1{}in", "0".repeat(306)),
        "20000000pt".to_owned(),
    ];
    for weight in &weights {
        for number in [16.0, 13.0] {
            let mut o = heavy_lines(weight);
            let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
            let line = shape_mut(&mut o.doc, 0, id);
            set(line, "EndArrow", number);
            set(line, "EndArrowSize", 2.0);
            assert_weight_refused(real(&o.doc, &o.bytes, &o.index), &format!("end {number} on a line of {}", &weight[..weight.len().min(12)]));
        }
    }
}

/// V44 on a line the file holds, whose end is set to the custom one (by a formula and plain): number 4's marker takes the place of the one it had, which no style
/// names after the save and so is taken out, the cell is carried, the model reads back as it was, and a second save is no edit.
#[test]
fn odg_styles_a_line_of_the_file_set_to_the_custom_end_is_written_as_number_4_s_marker_and_its_cell_is_carried() {
    for formula in [Some(r#"USE("Fancy end")"#), None] {
        let mut o = two_lines(MARKER_T23);
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let cell = Cell::read(formula, Some("254"), None, None, &mut 1_000);
        Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet).cells.insert(CellName::new("EndArrow"), cell);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{formula:?}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(
            content.contains(r#"draw:marker-end="DiaCraft_20_end_20_4" draw:marker-end-width="6.54pt""#) || content.contains("DiaCraft_20_end_20_4"),
            "{formula:?}: {content}"
        );
        let names = marker_names(&saved.bytes);
        assert!(names.contains(&"DiaCraft_20_end_20_4".to_owned()), "{formula:?}: number 4's marker is added: {names:?}");
        assert!(!names.contains(&"DiaCraft_20_end_20_14_20_t23".to_owned()), "{formula:?}: the marker no style names is taken out: {names:?}");
        let back = open_bytes(saved.bytes.clone());
        let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("EndArrow").cloned().expect("the cell reads back");
        assert_eq!(read.value.as_ref().and_then(diacraft_doc::Value::number), Some(254.0), "{formula:?}");
        assert_eq!(read.formula.text(), formula, "{formula:?}: the formula as the model has it");
        let again = real(&back.doc, &saved.bytes, &back.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "{formula:?}: a second save is no edit");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// Upkeep, reuse by name and edits of lines the file has.

pub(crate) const MARKER_T23: &str = r#"<draw:marker draw:name="DiaCraft_20_end_20_14_20_t23" draw:display-name="DiaCraft end 14 t23" svg:viewBox="0 0 2000 3000" svg:d="M 1000 0 L 2000 3000 L 0 3000 Z M 1000 800 L 500 2800 L 1500 2800 Z"/>"#;
pub(crate) const MARKER_T44: &str = r#"<draw:marker draw:name="DiaCraft_20_end_20_14_20_t44" draw:display-name="DiaCraft end 14 t44" svg:viewBox="0 0 2000 3000" svg:d="M 1000 0 L 2000 3000 L 0 3000 Z M 1000 1300 L 700 2700 L 1300 2700 Z"/>"#;
pub(crate) const FOREIGN_ARROW: &str = r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="m10 0-10 30h20z"/>"#;

/// Two styles: `gr1` at 0.75 pt naming the `t23` marker of number 14 (6.54 pt wide at size 2) and `gr2` at 2 pt naming the `t44` one (9.04 pt).
pub(crate) const TWO_STYLES: &str = concat!(
    r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:marker-end="DiaCraft_20_end_20_14_20_t23" draw:marker-end-width="6.54pt" svg:stroke-width="0.75pt" draw:stroke="solid"/></style:style>"#,
    r#"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:marker-end="DiaCraft_20_end_20_14_20_t44" draw:marker-end-width="9.04pt" svg:stroke-width="2pt" draw:stroke="solid"/></style:style>"#
);

/// Two connectors, the first of style `gr1` and the second of style `gr2`.
pub(crate) fn two_connectors() -> String {
    (1..=2)
        .map(|k| {
            format!(
                "<draw:connector draw:name=\"c{k}\" draw:style-name=\"gr{k}\" draw:layer=\"layout\" svg:x1=\"1cm\" svg:y1=\"{k}cm\" svg:x2=\"6cm\" svg:y2=\"{k}cm\" svg:d=\"M 0 0 L 5000 0\" svg:viewBox=\"0 0 5000 1\"><text:p/></draw:connector>"
            )
        })
        .collect()
}

fn two_lines(common: &str) -> crate::write_core::Open {
    open(&Drawing::a4().common(common).auto(TWO_STYLES).page(&two_connectors()))
}

fn delete_second(o: &mut crate::write_core::Open) {
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(1);
}

/// The names of the markers `styles.xml` holds, in order.
fn marker_names(bytes: &[u8]) -> Vec<String> {
    markers(bytes).iter().map(|m| attr(m, "draw:name").unwrap()).collect()
}

/// Two lines with `EndArrow` 14, size 2, one at 0.75 pt and one at 2 pt, have two markers, `t23` and `t44`. After the 2 pt line is deleted and the drawing saved,
/// the `t44` marker is gone and `t23` stays; a marker named `Arrow` that no style names is not removed.
fn case_39(save: &Saver) {
    // The two lines as the writer makes them.
    let mut o = open(&base());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((14.0, 2.0)), 0.75));
    push_shape(&mut o.doc, arrowed(id + 1, None, Some((14.0, 2.0)), 2.0));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t23", "DiaCraft_20_end_20_14_20_t44"]);
    // The same two lines in a file that also holds a marker no style names: the deletion takes out `t44` and nothing else.
    let mut o = two_lines(&format!("{FOREIGN_ARROW}{MARKER_T23}{MARKER_T44}"));
    delete_second(&mut o);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(marker_names(&saved.bytes), ["Arrow", "DiaCraft_20_end_20_14_20_t23"], "{}", part(&saved.bytes, "styles.xml"));
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(FOREIGN_ARROW) && styles.contains(MARKER_T23), "the others are as they were: {styles}");
    assert!(part(&saved.bytes, "content.xml").contains("draw:marker-end=\"DiaCraft_20_end_20_14_20_t23\""));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

#[test]
fn odg_styles_a_marker_no_style_names_after_the_save_is_taken_out_and_another_producer_s_is_not() {
    case_39(&real);
}

/// A marker taken out goes whole: the element that is not empty, the white space before it, and the one beside it is left as it was.
#[test]
fn a_marker_taken_out_goes_whole_when_it_is_not_an_empty_element_and_is_set_off_by_white_space() {
    let open_form = MARKER_T44.replace("/>", "><!-- a program left this --></draw:marker>");
    let common = format!("\n    {MARKER_T23}\n    {open_form}\n    {FOREIGN_ARROW}\n  ");
    let mut o = two_lines(&common);
    delete_second(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(&format!("<office:styles>\n    {MARKER_T23}\n    {FOREIGN_ARROW}\n  </office:styles>")), "{styles}");
    assert!(!styles.contains("a program left this") && !styles.contains("draw:marker>"), "{styles}");
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t23", "Arrow"]);
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// A marker no style names that the save did not orphan is left where it is: only what the edit made unnamed goes.
#[test]
fn a_marker_that_no_style_named_before_the_edit_is_left_alone() {
    let mut o = two_lines(&format!("{MARKER_T23}{MARKER_T44}"));
    // A save that touches the first line only: the second line, `gr2` and its marker are as they were.
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    set(shape_mut(&mut o.doc, 0, id), "LinePattern", 0.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"));
    // A marker nothing names, in a file where an edit takes a style out: the edit's own orphan goes, the one that was never named stays.
    let mut o = open(
        &Drawing::a4()
            .common(&format!("{MARKER_T23}{MARKER_T44}{}", MARKER_T23.replace("14_20_t23", "12_20_t23").replace("end 14 t23", "end 12 t23")))
            .auto(TWO_STYLES)
            .page(&two_connectors()),
    );
    delete_second(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t23", "DiaCraft_20_end_20_12_20_t23"]);
}

/// A marker a common style of `styles.xml` names is not taken out when the last shape that named it is deleted.
#[test]
fn a_marker_a_common_style_names_stays_when_the_last_line_that_named_it_is_deleted() {
    let keep = r#"<style:style style:name="Keep" style:family="graphic"><style:graphic-properties draw:marker-end="DiaCraft_20_end_20_14_20_t44"/></style:style>"#;
    let mut o = two_lines(&format!("{MARKER_T23}{MARKER_T44}{keep}"));
    delete_second(&mut o);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t23", "DiaCraft_20_end_20_14_20_t44"]);
}

/// A marker the file holds under DiaCraft's name, with whatever path another program left, is the marker a line names: used as it is, and `styles.xml` is
/// not touched, for a band class and a number alike; a second save is no edit.
#[test]
fn a_marker_of_a_band_class_the_file_holds_is_reused_by_name_and_a_second_save_is_no_edit() {
    let rewritten = r#"<draw:marker draw:name="DiaCraft_20_end_20_16_20_t23" draw:display-name="DiaCraft end 16 t23" svg:viewBox="0 0 2000 2000" svg:d="M1000 0l1000 2000h-2000zM1000 514l-628 1256h1256z"/>"#;
    let mut o = open(&base().common(rewritten));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, arrowed(id, None, Some((16.0, 2.0)), 0.75));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "the marker stands as the file has it");
    assert!(part(&saved.bytes, "content.xml").contains("draw:marker-end=\"DiaCraft_20_end_20_16_20_t23\""));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // Another band class of the same number is another marker, written beside it; the first is still named by the first line.
    let mut doc = o.doc.clone();
    push_shape(&mut doc, arrowed(id + 1, None, Some((16.0, 2.0)), 2.0));
    let second = real(&doc, &saved.bytes, &saved.index).unwrap();
    assert_eq!(marker_names(&second.bytes), ["DiaCraft_20_end_20_16_20_t23", "DiaCraft_20_end_20_16_20_t44"]);
    assert!(part(&second.bytes, "styles.xml").contains(rewritten));
}

/// The weight of a line the file holds changes: the band class follows it, so the style names another marker, which the save writes, and the one nothing names
/// any more goes (the marker is the number and the band class); the other line that named the first marker keeps it.
#[test]
fn a_weight_edit_names_the_marker_of_the_new_band_class_and_takes_out_the_one_no_line_names() {
    let mut o = two_lines(&format!("{MARKER_T23}{MARKER_T44}"));
    delete_second(&mut o);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    // 3 pt at size 2: S 5.52 pt, 3 / 5.52 = 0.5435, band class 54, and the width 2 x 5.52 = 11.04 pt.
    set(shape_mut(&mut o.doc, 0, id), "LineWeight", 3.0 / 72.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("draw:marker-end=\"DiaCraft_20_end_20_14_20_t54\"") && after.contains("draw:marker-end-width=\"11.04pt\""), "{after}");
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t54"], "the new marker is there, and the two no line names are gone");
    let back = open_bytes(saved.bytes.clone());
    let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!((num_of(read, "EndArrow"), num_of(read, "EndArrowSize")), (14.0, 2.0));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // With two lines on the style, the one edited is given a copy of it, and the other keeps `t23`.
    let mut o = open(&Drawing::a4().common(MARKER_T23).auto(TWO_STYLES).page(&two_connectors().replace("gr2", "gr1")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    set(shape_mut(&mut o.doc, 0, id), "LineWeight", 3.0 / 72.0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(marker_names(&saved.bytes), ["DiaCraft_20_end_20_14_20_t23", "DiaCraft_20_end_20_14_20_t54"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// Every number, every size, two weights.

/// A drawing of one connector for each of 45 numbers, 7 size classes and the weights `weights`, each with the number at both ends: begin at the class and end at
/// the class counted from the other side (6 less it).
fn every_end(weights: &[f64]) -> (crate::write_core::Open, Vec<(u32, usize, f64)>) {
    let mut o = open(&base());
    let first = o.index.next_id(0).unwrap();
    let mut made = Vec::new();
    for n in 1..=45u32 {
        for class in 0..7usize {
            for &weight in weights {
                let id = first + u32::try_from(made.len()).unwrap();
                push_shape(&mut o.doc, arrowed(id, Some((f64::from(n), class as f64)), Some((f64::from(n), (6 - class) as f64)), weight));
                made.push((n, class, weight));
            }
        }
    }
    (o, made)
}

/// The writer's drawing of every end at every size on two weights, saved and read back: each number and size comes back right; a second save is no edit; and the
/// markers are exactly one for each number and band class the lines use.
#[test]
fn every_end_at_every_size_class_on_two_weights_is_written_and_read_back_by_number_and_class() {
    let (o, made) = every_end(&[0.75, 3.0]);
    assert_eq!(made.len(), 45 * 7 * 2);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let back = open_bytes(saved.bytes.clone());
    let shapes = &back.doc.pages.get(0).unwrap().shapes;
    assert_eq!(shapes.len(), made.len() + 1);
    let mut wrong = Vec::new();
    for (k, (n, class, weight)) in made.iter().enumerate() {
        let s = shapes.get(k + 1).unwrap();
        let got = (num_of(s, "BeginArrow"), num_of(s, "BeginArrowSize"), num_of(s, "EndArrow"), num_of(s, "EndArrowSize"));
        let want = (f64::from(*n), *class as f64, f64::from(*n), (6 - class) as f64);
        if got != want {
            wrong.push((n, class, weight, got));
        }
    }
    assert!(wrong.is_empty(), "{} of {} came back wrong, the first: {:?}", wrong.len(), made.len(), wrong.first());
    // One marker for each number and band class used, and no other.
    let mut want: BTreeSet<String> = BTreeSet::new();
    for (n, class, weight) in &made {
        for c in [*class, 6 - class] {
            let name = if UNBANDED.contains(n) {
                format!("DiaCraft_20_end_20_{n}")
            } else {
                format!("DiaCraft_20_end_20_{n}_20_t{}", band_class(c, *weight))
            };
            want.insert(name);
        }
    }
    let got: BTreeSet<String> = marker_names(&saved.bytes).into_iter().collect();
    assert_eq!(got.len(), marker_names(&saved.bytes).len(), "no marker is written twice");
    assert_eq!(got, want);
    // Each width is the spread times S: read the widths back, in points, and the class they say is the class written.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // The widest band class a 0.75 pt and a 3 pt line reach is under 99 and over 1 on every class.
    for class in 0..7 {
        for weight in [0.75, 3.0] {
            assert!((1..=99).contains(&band_class(class, weight)));
        }
    }
}

/// Every width written, for every number, size and weight, is the spread times the base plus the weight to the four places the attribute is written in.
#[test]
fn every_width_written_is_the_spread_times_s_for_the_class_and_weight() {
    let spreads = |n: u32| -> f64 {
        match n {
            1..=4 | 8 | 9 | 12..=17 | 19 | 23 | 39 | 40 | 43..=45 => 2.0,
            5 => 1.99,
            6 | 18 => 2.02,
            7 => 1.92,
            22 | 34 | 38 => 2.24,
            _ => 1.5,
        }
    };
    let bases = [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0];
    let (o, made) = every_end(&[0.75, 3.0]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let content = part(&saved.bytes, "content.xml");
    // The widths of the styles in the order of the shapes: a style for each, the begin's width before the end's in the attribute list.
    let widths: Vec<f64> = content.split("draw:marker-start-width=\"").skip(1).map(|t| points(&t[..t.find('"').unwrap()])).collect();
    assert_eq!(widths.len(), made.len());
    for ((n, class, weight), got) in made.iter().zip(&widths) {
        let want = spreads(*n) * (bases[*class] + weight);
        assert!((got - want).abs() < 0.0001, "number {n} class {class} weight {weight}: {got} against {want}");
    }
}

#[test]
fn echo_control_fails_every_line_end_case() {
    echo_control(&CASES);
}

/// What a save of every end at every size class costs, counted: the markers made, the work of drawing them (a marker is drawn once to write it and once to check
/// it, so the work follows the markers and is twice the work of building them), and the text written.
#[test]
fn the_work_of_writing_every_end_at_every_class_is_counted_and_follows_the_markers() {
    let cost = |weights: &[f64]| {
        let (o, _) = every_end(weights);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
        (marker_names(&saved.bytes).len() as u64, saved.work)
    };
    let (one, w1) = cost(&[0.75]);
    let (two, w2) = cost(&[0.75, 3.0]);
    // 35 numbers with a band at 7 size classes and ten with none: 255 markers on one weight; on two, 500 (no two classes at one weight make one band class).
    assert_eq!((one, two), (255, 500));
    // Measured: 244,496 steps for 255 markers and 483,856 for 500, about 960 each.
    for (n, w) in [(one, &w1), (two, &w2)] {
        let each = w.marker_art_steps / n;
        assert!((900..=1_050).contains(&each), "{n} markers cost {} steps, {each} each", w.marker_art_steps);
    }
    assert!(w2.marker_art_steps > w1.marker_art_steps * 19 / 10 && w2.marker_art_steps < w1.marker_art_steps * 21 / 10, "{w1:?} {w2:?}");
    // The text the markers add is bounded by their paths: under 600 bytes of marker text each, and the whole save under 600 bytes of text for each line.
    assert!(w2.bytes_written < 600 * two + 900 * 630, "{} bytes written", w2.bytes_written);
}

/// The most markers a drawing can need, every one of them: each banded number at each of the 99 band classes, and the ten with no band, 3,475 in all, from lines
/// whose weights are chosen for the band classes (class 2: a weight of `base x k / (100 - k)`). The save writes them all, once each, reads them back, and its work
/// is the work of building them twice.
#[test]
fn a_drawing_that_needs_every_marker_there_is_writes_3475_of_them_in_counted_work() {
    let mut o = open(&base());
    let first = o.index.next_id(0).unwrap();
    let mut made = 0u32;
    for n in 1..=45u32 {
        let weights: Vec<f64> = if UNBANDED.contains(&n) { vec![0.75] } else { (1..=99).map(|k| 2.52 * f64::from(k) / f64::from(100 - k)).collect() };
        for weight in weights {
            push_shape(&mut o.doc, arrowed(first + made, None, Some((f64::from(n), 2.0)), weight));
            made += 1;
        }
    }
    assert_eq!(made, 35 * 99 + 10);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let names = marker_names(&saved.bytes);
    assert_eq!(names.len(), 3475);
    assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 3475, "each once");
    // Measured: 3,308,354 steps, twice the 1,654,177 of building them, and every path under 500 bytes.
    let steps = saved.work.marker_art_steps;
    assert!((3_200_000..=3_450_000).contains(&steps), "{steps} steps");
    for m in markers(&saved.bytes) {
        assert!(m.len() < 1_100, "{} bytes of marker: {m}", m.len());
    }
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.len() < 1_500_000, "{} bytes of styles.xml", styles.len());
    // Read back, every line has its number and its class.
    let back = open_bytes(saved.bytes.clone());
    let shapes = &back.doc.pages.get(0).unwrap().shapes;
    assert_eq!(shapes.len(), made as usize + 1);
    assert!(shapes.iter().skip(1).all(|s| num_of(s, "EndArrowSize") == 2.0 && (1.0..=45.0).contains(&num_of(s, "EndArrow"))));
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

/// The same drawing with every line deleted: the 3,475 markers no style names any more all go, in one save, in work that follows them. The yardstick is the same
/// drawing with lines that have no line end, deleted the same way: what the markers add is a removal each, and nothing walks all the markers for each.
#[test]
fn a_drawing_whose_every_line_is_deleted_takes_out_all_3475_markers_in_counted_work() {
    // The work of deleting every line of a drawing of 3,475 lines (with their line ends or without), and the markers that are left.
    let deleted = |with_ends: bool| {
        let mut o = open(&base());
        let first = o.index.next_id(0).unwrap();
        let mut made = 0u32;
        for n in 1..=45u32 {
            let weights: Vec<f64> =
                if UNBANDED.contains(&n) { vec![0.75] } else { (1..=99).map(|k| 2.52 * f64::from(k) / f64::from(100 - k)).collect() };
            for weight in weights {
                let shape = if with_ends {
                    arrowed(first + made, None, Some((f64::from(n), 2.0)), weight)
                } else {
                    let mut c = crate::write_new::connector(first + made, (1.0, 10.0, 8.0, 12.0));
                    set(&mut c, "LineWeight", weight / 72.0);
                    c
                };
                push_shape(&mut o.doc, shape);
                made += 1;
            }
        }
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
        let held = marker_names(&saved.bytes).len();
        // Open the saved file and delete every line (the page keeps its own shape).
        let mut back = open_bytes(saved.bytes.clone());
        let page = Arc::make_mut(back.doc.pages.get_mut(0).unwrap());
        while page.shapes.len() > 1 {
            page.shapes.remove(page.shapes.len() - 1);
        }
        let mut work = diacraft_odg::WriteWork::default();
        let after = diacraft_odg::write_counted(
            &back.doc,
            Some(&diacraft_odg::OriginalPackage { bytes: &back.bytes, index: &back.index }),
            &diacraft_odg::WriteOptions::default(),
            &mut work,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        // A second save of the same model is no edit.
        let again = real(&back.doc, &after.bytes, &after.index).unwrap();
        assert!(!again.edited && again.bytes == after.bytes);
        (held, marker_names(&after.bytes).len(), work)
    };
    let (held, left, with) = deleted(true);
    let (none_held, none_left, without) = deleted(false);
    assert_eq!((held, left, none_held, none_left), (3475, 0, 0, 0));
    // A marker is taken out, not drawn.
    assert_eq!(with.marker_art_steps, 0);
    // What the markers add to the edits is one removal each, and to the steps of looking things up, a walk of the held markers once (and of the markers of the part
    // made once, none): under a few for each marker.
    let (extra_edits, extra_steps) = (with.edits - without.edits, with.order_steps - without.order_steps);
    assert!((3475..=2 * 3475).contains(&extra_edits), "{extra_edits} more edits for 3,475 markers ({} and {})", with.edits, without.edits);
    assert!(extra_steps <= 3 * 3475, "{extra_steps} more steps for 3,475 markers ({} and {})", with.order_steps, without.order_steps);
}
