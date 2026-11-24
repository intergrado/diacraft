//! An arrow size beside no line end (the review): a size says how big the end is, and a
//! file with no end says nothing of it, so the size is no cell the file's own markup gives back. It is carried by DiaCraft's extension, as every cell the markup cannot say is, and was
//! dropped without a word from every new shape (a size alone, a size beside the end 0, a size of 0 alone).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape};
use diacraft_odg::{Limits, read};

use crate::flat_write::small_png;
use crate::write_core::{Open, custom, open_bytes};
use crate::write_new::{connector, push_shape, rectangle};
use crate::write_picture::{named, picture, save};

/// The cells that make a line end, as a case sets them: the name, and the number.
type Ends<'a> = &'a [(&'a str, &'a str)];

/// Every combination the review names, and the ones beside them: a size with no end at either side, a size beside the end 0, a size of 0, a size beside an end, and an end alone.
const CASES: [&[(&str, &str)]; 12] = [
    &[("BeginArrowSize", "4")],
    &[("BeginArrow", "0"), ("BeginArrowSize", "4")],
    &[("EndArrowSize", "0")],
    &[("EndArrowSize", "3")],
    &[("EndArrow", "0"), ("EndArrowSize", "5")],
    &[("EndArrow", "4"), ("EndArrowSize", "3")],
    &[("BeginArrow", "4"), ("BeginArrowSize", "2")],
    &[("EndArrow", "4"), ("BeginArrowSize", "4")],
    &[("BeginArrow", "4"), ("EndArrowSize", "1")],
    &[("EndArrow", "4")],
    &[("BeginArrow", "0")],
    &[("BeginArrow", "1"), ("BeginArrowSize", "2"), ("EndArrow", "0"), ("EndArrowSize", "6")],
];

fn with(mut shape: Shape, ends: Ends<'_>) -> Shape {
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (name, value) in ends {
        sheet.cells.insert(CellName::new(name), Cell::read(None, Some(value), None, None, &mut 1_000));
    }
    shape
}

/// The number a cell of `shape` holds, `None` for no cell.
fn cell(shape: &Shape, name: &str) -> Option<f64> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number())
}

/// Every cell the case set is in the shape the file reads back as, with the number it was given.
#[track_caller]
fn assert_read_back(model: &Shape, back: &Shape, ends: Ends<'_>, what: &str) {
    for (name, _) in ends {
        assert_eq!(cell(model, name), cell(back, name), "{what}: {name} of {ends:?} (a cell the model holds is not lost)");
        assert!(cell(back, name).is_some(), "{what}: {name} of {ends:?} is read back");
    }
}

/// The shape the save wrote, read back, found by name.
fn back_of(saved: &diacraft_odg::Written, name: &str) -> Shape {
    let (doc, _) = read(&saved.bytes, &Limits::default()).unwrap();
    named(&doc, name).clone()
}

fn page() -> Open {
    open_bytes(crate::drawing::Drawing::a4().page(&custom("s1", "1cm")).bytes())
}

/// A new rectangle (a custom shape) with the cells of a case is saved with every one of them, and a second save is no edit.
#[test]
fn an_arrow_size_beside_no_end_on_a_new_custom_shape_is_carried_and_not_dropped() {
    for ends in CASES {
        let mut o = page();
        let id = o.index.next_id(0).unwrap();
        let model = with(rectangle(id, "R", (2.0, 8.0, 4.0, 2.0)), ends);
        push_shape(&mut o.doc, model.clone());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{ends:?}: {e}"));
        assert_read_back(&model, &back_of(&saved, "R"), ends, "a custom shape");
        assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "{ends:?}: a second save is no edit");
    }
}

/// The same for a new connector, which takes its ends from the same cells.
#[test]
fn an_arrow_size_beside_no_end_on_a_new_connector_is_carried_and_not_dropped() {
    for ends in CASES {
        let mut o = page();
        let id = o.index.next_id(0).unwrap();
        let model = with(connector(id, (2.0, 3.0, 9.0, 12.0)), ends);
        push_shape(&mut o.doc, model.clone());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{ends:?}: {e}"));
        assert_read_back(&model, &back_of(&saved, "Connector.1"), ends, "a connector");
    }
}

/// The same for a new picture (a frame), in a package.
#[test]
fn an_arrow_size_beside_no_end_on_a_new_picture_is_carried_and_not_dropped() {
    for ends in CASES {
        let mut o = page();
        let id = o.index.next_id(0).unwrap();
        let model = with(picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()), ends);
        push_shape(&mut o.doc, model.clone());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{ends:?}: {e}"));
        assert_read_back(&model, &back_of(&saved, "Picture.1"), ends, "a picture");
    }
}

/// The same in a flat drawing, whose new shapes are written by the same code.
#[test]
fn an_arrow_size_beside_no_end_on_a_new_shape_of_a_flat_drawing_is_carried_and_not_dropped() {
    for ends in CASES {
        let mut d = crate::write_core::drawing_of(3);
        d.meta = Some(crate::write_core::full_meta());
        let mut o = open_bytes(crate::flat::Flat::of(&d).bytes());
        let id = o.index.next_id(0).unwrap();
        let model = with(rectangle(id, "R", (2.0, 8.0, 4.0, 2.0)), ends);
        push_shape(&mut o.doc, model.clone());
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{ends:?}: {e}"));
        assert_read_back(&model, &back_of(&saved, "R"), ends, "a flat drawing's shape");
    }
}

/// A size that stands beside an end is written natively, as it always was: the file's own markers say it, and the extension carries nothing of it. (The fix takes a size out of the
/// cells written natively only where no end is.)
#[test]
fn a_size_beside_an_end_is_still_written_in_the_files_own_markup_and_needs_no_extension() {
    let mut o = page();
    let id = o.index.next_id(0).unwrap();
    let model = with(rectangle(id, "R", (2.0, 8.0, 4.0, 2.0)), &[("EndArrow", "4"), ("EndArrowSize", "3")]);
    push_shape(&mut o.doc, model.clone());
    let before = crate::write_core::part(&o.bytes, "content.xml").matches("diacraft:").count();
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let content = crate::write_core::part(&saved.bytes, "content.xml");
    assert!(content.contains("draw:marker-end=\"DiaCraft_20_end_20_4"), "{content}");
    assert_eq!(content.matches("diacraft:").count(), before, "no tier 2 for what the markers say: {content}");
    assert_read_back(&model, &back_of(&saved, "R"), &[("EndArrow", "4"), ("EndArrowSize", "3")], "an end with its size");
}

/// A line end's size is read back from the width of its marker and the line's weight *the file* gives: a line weight that is not written natively (a formula) is tier 2, so the
/// file's line has the weight its style gives, and the marker is made for that: the size is what the model had (the property test found a size of 4 read back as 2).
#[test]
fn a_size_beside_an_end_is_read_back_whole_whatever_becomes_of_the_line_weight() {
    for (weight_formula, weight) in [(Some("THEMEVAL()"), "0.02"), (Some("1*1"), "0.3"), (None, "0.02"), (None, "0.5")] {
        for size in ["0", "1", "3", "4", "6"] {
            let mut o = page();
            let id = o.index.next_id(0).unwrap();
            let mut model = with(rectangle(id, "R", (2.0, 8.0, 4.0, 2.0)), &[("BeginArrow", "1"), ("BeginArrowSize", size)]);
            Arc::make_mut(&mut model.sheet)
                .cells
                .insert(CellName::new("LineWeight"), Cell::read(weight_formula, Some(weight), Some("IN"), None, &mut 1_000));
            push_shape(&mut o.doc, model.clone());
            let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{weight_formula:?} {weight} size {size}: {e}"));
            let back = back_of(&saved, "R");
            assert_eq!(cell(&model, "BeginArrowSize"), cell(&back, "BeginArrowSize"), "{weight_formula:?} {weight} size {size}");
            assert_eq!(cell(&model, "BeginArrow"), cell(&back, "BeginArrow"), "{weight_formula:?} {weight} size {size}");
            assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "a second save is no edit");
        }
    }
}
