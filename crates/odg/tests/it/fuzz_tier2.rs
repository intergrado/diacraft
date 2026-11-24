//! The crash of `odg_write` that the run found, composed in code (the artifact `crash-fde9eaaa...`: 192 bytes, two edits of the model and a body of one
//! custom shape).
//!
//! The model after the edits is not one the engine makes: **`Width` was set and `LocPinX`, which is the formula `Width*0.5`, kept the value it had** (the fuzz target sets a cell
//! and does not calculate). The shape's pin is then not the centre of the frame its size and place give, and the writer carried the pin as tier 2 and wrote the frame; the
//! reader keeps a carried pin only where it puts the frame's edge where the markup has it, takes this one out, and the file read back had the frame's centre as the
//! pin: the check of the bytes made refused the save with `SelfCheck`, a class that says the writer wrote what it cannot read. The writer refuses such a shape in words now.

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Page, Shape, Value};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{Open, open_bytes, real};

/// The body of the artifact: a custom shape that says no width (`swidth` is no attribute of the format), 2 cm high, at 5 cm.
const BODY: &str = r#"<draw:custom-shape draw:name="b" svg:x="5cm" svg:y="1cm" swidth="2cm" svg:height="2cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;

/// The drawing of the artifact as the reader gives it: one page with one shape of no width.
fn artifact() -> (Vec<u8>, Open) {
    let bytes = Drawing::a4().page(BODY).bytes();
    let open = open_bytes(bytes.clone());
    (bytes, open)
}

/// A page `id` named `name` with the sheet of the first page and a copy of `shape` as its one shape, `ID` 1 (the page edit of the fuzz target that adds a page with a copy
/// of the first shape).
fn page_with_copy_of(doc: &Document, id: u32, name: &str, shape: &Shape) -> Arc<Page> {
    let mut page = Page::new(id, name);
    page.sheet = Arc::clone(&doc.pages.get(0).expect("a page").sheet);
    let mut copy = shape.clone();
    copy.id = 1;
    page.shapes.push(Arc::new(copy));
    Arc::new(page)
}

/// Op 112 of the artifact: the `Width` of the first shape of the first page is set to what it was and 62 / 7, with nothing calculated again.
fn set_width_only(doc: &mut Document, extra: f64) {
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
    let sheet = Arc::make_mut(&mut shape.sheet);
    let cell = sheet.cells.get_mut("Width").expect("a width");
    let v = cell.value.as_ref().and_then(Value::number).expect("a number");
    cell.value = Value::from_number(v + extra);
}

/// The artifact, in code. A page is added with a copy of the shape (op 247), the width of the first shape is set (op 112), and the model is saved; then, from the saved bytes
/// and the index of the save, another page is added with a copy of the first shape of the first page, the one whose width was set (the second round of the fuzz target).
/// The first save is made; the second is **refused in words** (`write_unsupported`) and not by the check of the bytes (`write_self_check`: red on main).
#[test]
fn a_shape_whose_width_was_set_and_whose_pin_place_was_not_calculated_again_is_refused_in_words_when_it_is_written_new() {
    let (bytes, mut open) = artifact();
    let first = Arc::clone(open.doc.pages.get(0).unwrap());
    let shape = Arc::clone(first.shapes.get(0).unwrap());
    let added = page_with_copy_of(&open.doc, 2, "a", &shape);
    open.doc.pages.push(added);
    set_width_only(&mut open.doc, 62.0 / 7.0);
    let saved = real(&open.doc, &bytes, &open.index)
        .expect("the first save: the copy has the cells it was read with, and the edited shape is a shape of the file");
    // The second round.
    let mut next = open.doc.clone();
    let first = Arc::clone(next.pages.get(0).unwrap());
    let added = page_with_copy_of(&next, 3, "p", first.shapes.get(0).unwrap());
    next.pages.push(added);
    let error = real(&next, &saved.bytes, &saved.index).expect_err("a new shape whose cells disagree");
    assert_eq!(error.class(), "write_unsupported", "{error}");
    let WriteError::Unsupported(words) = &error else { panic!("{error:?}") };
    assert!(words.contains("pin") && words.contains("LocPinX"), "{words}");
}

/// **The input minimised:** one save. The width of the shape is set and nothing calculated again, a page is added with a copy of the shape, and the model is saved: the copy is
/// a new shape whose cells disagree, and it is refused in words (the artifact needed a first save and a second round only because the fuzz target made the copy before it set
/// the width).
#[test]
fn the_minimal_model_one_page_added_with_a_copy_of_a_shape_whose_width_was_set_alone_is_refused_in_words() {
    let (bytes, mut open) = artifact();
    set_width_only(&mut open.doc, 1.0);
    let shape = Arc::clone(open.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
    let added = page_with_copy_of(&open.doc, 2, "a", &shape);
    open.doc.pages.push(added);
    let error = real(&open.doc, &bytes, &open.index).expect_err("a new shape whose cells disagree");
    assert_eq!(error.class(), "write_unsupported", "{error}");
    // The same page with a copy of the shape as it was read is written.
    let (bytes, mut open) = artifact();
    let shape = Arc::clone(open.doc.pages.get(0).unwrap().shapes.get(0).unwrap());
    let added = page_with_copy_of(&open.doc, 2, "a", &shape);
    open.doc.pages.push(added);
    real(&open.doc, &bytes, &open.index).expect("the copy of a shape as it was read agrees with itself");
}

/// The same edit with the place calculated again (`LocPinX` is `Width*0.5` of the width it has): the model agrees with itself, the new shape is written, and the file
/// reads back with the shape where the model has it, whatever the pin is.
#[test]
fn the_same_shape_with_its_cells_calculated_again_is_written_new_and_reads_back() {
    let (bytes, mut open) = artifact();
    let first = Arc::clone(open.doc.pages.get(0).unwrap());
    let shape = Arc::clone(first.shapes.get(0).unwrap());
    let added = page_with_copy_of(&open.doc, 2, "a", &shape);
    open.doc.pages.push(added);
    set_width_only(&mut open.doc, 62.0 / 7.0);
    {
        let page = Arc::make_mut(open.doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        let width = sheet.cells.get("Width").and_then(|c| c.value.as_ref()).and_then(Value::number).unwrap();
        let place = sheet.cells.get_mut("LocPinX").expect("a place");
        place.value = Value::from_number(width / 2.0);
        // The pin is the centre of a frame that has its place: it moves to where the frame the file has puts it.
        let pin = sheet.cells.get_mut("PinX").expect("a pin");
        let was = pin.value.as_ref().and_then(Value::number).unwrap();
        pin.value = Value::from_number(was + width / 2.0);
    }
    let saved = real(&open.doc, &bytes, &open.index).expect("the first save");
    let mut next = open.doc.clone();
    let first = Arc::clone(next.pages.get(0).unwrap());
    let added = page_with_copy_of(&next, 3, "p", first.shapes.get(0).unwrap());
    next.pages.push(added);
    let second = real(&next, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("a model that agrees with itself is written: {e}"));
    let (back, _) = diacraft_odg::read(&second.bytes, &diacraft_odg::Limits::default()).expect("the bytes read");
    assert_eq!(back.pages.len(), 3);
    let pin = |doc: &Document, page: usize| -> f64 {
        doc.pages.get(page).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c: &Cell| c.value.as_ref()?.number()).unwrap()
    };
    assert!((pin(&back, 2) - pin(&next, 2)).abs() < 1e-6, "the copy reads back with the pin the model has: {} and {}", pin(&back, 2), pin(&next, 2));
}
