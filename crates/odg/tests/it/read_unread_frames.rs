//! A coordinate, a size or a transform that is no value the reader reads is read as absent, said once in the report, and not held as text (found by the hostile-file sweeps).
//!
//! **The bug.** A shape holds the text of its frame (`svg:x1` to `svg:y2` of a segment, `svg:x`, `svg:y`, `svg:width`, `svg:height` and `draw:transform` of a shape placed by a transform,
//! the centre and radii of a circle) beside the cells that stand for it, and the writer of a drawing from the model gives the text back while the cells are as the read made them. A
//! `svg:y2` of `infcm`, `NaNcm`, `1e999cm` or `1e308cm` is no length (a length is a decimal number and a unit: ODF 1.4, 18.3.18), so the reader read the end as absent and still held the
//! text, and the save wrote it back; the check of the bytes made then refused it ("the reader does not read what the save wrote as y2"), so a drawing that opened could not be saved as an
//! `.odg`. The text the reader does not read is no longer held: what is saved is what the cells say, and the report says that an attribute was read as absent.

use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::{Drawing, close, cm};

/// What is no length: the four of the sweep, and a plain number, a comma and an exponent.
const NOT_LENGTHS: [&str; 7] = ["infcm", "NaNcm", "1e999cm", "1e308cm", "5", "1,5cm", "1e2cm"];

fn line(y2: &str) -> Drawing {
    Drawing::a4().page(&format!(r#"<draw:line draw:name="l" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="{y2}"/>"#))
}

fn transformed(width: &str) -> Drawing {
    Drawing::a4().page(&format!(
        r#"<draw:rect draw:name="r" svg:x="1cm" svg:y="1cm" svg:width="{width}" svg:height="2cm" draw:transform="translate (1cm 1cm)"/>"#
    ))
}

fn circle(r: &str) -> Drawing {
    Drawing::a4().page(&format!(r#"<draw:circle draw:name="c" svg:cx="5cm" svg:cy="4cm" svg:r="{r}"/>"#))
}

/// The drawing read, and what the report says of attributes read as absent.
fn read_with_notes(d: &Drawing) -> (diacraft_doc::Document, u64, Vec<String>) {
    let (doc, report) = read(&d.bytes(), &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()));
    let count = report.warning_counts.get("frame_attribute_unread").copied().unwrap_or(0);
    let words = report.warnings.iter().filter(|w| w.contains("read as absent")).cloned().collect();
    (doc, count, words)
}

/// The drawing written from the model alone (what `file.save_as` an `.odg` of a drawing from another file does): it must be made, read again, and hold no attribute valued `unread` (the text
/// it was not read from; `None` for none to look for).
#[track_caller]
fn saved_from_the_model(what: &str, doc: &diacraft_doc::Document, unread: Option<&str>) -> Vec<u8> {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{what}: the save was refused: {e}"));
    let text = String::from_utf8_lossy(&crate::write_core::part_bytes(&written.bytes, "content.xml")).into_owned();
    if let Some(unread) = unread {
        assert!(!text.contains(&format!("=\"{unread}\"")), "{what}: the text that is no value is not written back: {text}");
    }
    read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{what}: the file made does not read: {} ({e})", e.class()));
    written.bytes
}

#[test]
fn an_end_of_a_line_that_is_no_length_is_read_as_absent_said_once_and_the_drawing_is_saved_as_an_odg() {
    for bad in NOT_LENGTHS {
        let (doc, count, words) = read_with_notes(&line(bad));
        assert_eq!(count, 1, "{bad}: one attribute read as absent");
        assert_eq!(words.len(), 1, "{bad}: said once: {words:?}");
        assert!(!words[0].contains(bad), "{bad}: the words hold none of the file's text: {}", words[0]);
        let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
        close(crate::write_core::num_of(shape, "EndX"), cm(5.0));
        saved_from_the_model(bad, &doc, Some(bad));
    }
    // The control: a line whose ends are all lengths says nothing, and is held as it was.
    let (doc, count, words) = read_with_notes(&line("13cm"));
    assert_eq!((count, words.len()), (0, 0));
    assert!(saved_from_the_model("13cm", &doc, None).len() > 100);
}

#[test]
fn a_size_of_a_shape_placed_by_a_transform_that_is_no_length_is_read_as_absent_and_the_drawing_is_saved() {
    for bad in NOT_LENGTHS {
        let (doc, count, _) = read_with_notes(&transformed(bad));
        assert_eq!(count, 1, "{bad}");
        saved_from_the_model(bad, &doc, Some(bad));
    }
    assert_eq!(read_with_notes(&transformed("3cm")).1, 0);
}

/// A circle given by its centre whose radius is no length is saved as it was before (the attribute the reader does not read stays in the markup the shape keeps, and is no frame value): a
/// guard that the change to what is held does not make the save of such a drawing worse.
#[test]
fn a_radius_of_a_circle_that_is_no_length_is_saved_as_an_odg() {
    for bad in NOT_LENGTHS {
        let (doc, _, _) = read_with_notes(&circle(bad));
        saved_from_the_model(bad, &doc, None);
    }
}

/// A transform that does not close is no frame value either: the reader leaves it among the attributes it did not read (which a save keeps, as it keeps every attribute it does not know),
/// and the shape is saved all the same.
#[test]
fn a_transform_the_reader_does_not_read_is_kept_with_the_attributes_it_did_not_read_and_the_drawing_is_saved() {
    let d = Drawing::a4().page(r#"<draw:rect draw:name="r" svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:transform="rotate (0.5"/>"#);
    let (doc, count, _) = read_with_notes(&d);
    assert_eq!(count, 0, "no frame value was held and dropped");
    saved_from_the_model("rotate (0.5", &doc, None);
}
