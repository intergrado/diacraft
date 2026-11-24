//! Item 3: the kind of a shape that ODF has no element for. A guide is a page shape of kind `Guide` in the model; the writer writes it as a
//! shape of no size, and `diacraft:type="Guide"` on its own automatic style says what it is, so that the file reads back with the kind it had. The attribute is the extension's
//! (the name is proposed in the report), read and written like the others of tier 2.

use diacraft_doc::ShapeType;

use crate::drawing::{Drawing, only};
use crate::write_core::{open, part, real};
use crate::write_new::{push_shape, rectangle};

fn style(attrs: &str) -> String {
    format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#)
}

fn rect(extra: &str) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" {extra} svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#)
}

/// A style that says `Guide` gives the shape the kind `Guide` and nothing else of the shape changes; with no such attribute the shape has the kind the reader gives.
#[test]
fn a_style_that_says_guide_gives_the_shape_that_kind() {
    let (plain, _) = Drawing::a4().auto(&style("")).page(&rect("")).read();
    assert_eq!(only(&plain).kind, ShapeType::Shape);
    let attrs = r#"diacraft:version="1" diacraft:id="4" diacraft:type="Guide""#;
    let (doc, report) = Drawing::a4().auto(&style(attrs)).page(&rect("")).read();
    let s = only(&doc);
    assert_eq!((s.kind.clone(), s.id), (ShapeType::Guide, 4));
    assert_eq!(report.warning_counts.get("diacraft_tier2_unreadable"), None);
    assert!(s.sheet.cell("PinX").is_some(), "the frame is still the markup's");
}

/// Only a guide is carried: any other value is left out with one warning and the shape has the kind its markup gives. A value is compared as the text it is.
#[test]
fn any_other_kind_is_left_out_with_a_warning() {
    for value in ["Group", "Foreign", "Shape", "guide", "", "Guide "] {
        let attrs = format!(r#"diacraft:version="1" diacraft:id="4" diacraft:type="{value}""#);
        let (doc, report) = Drawing::a4().auto(&style(&attrs)).page(&rect("")).read();
        assert_eq!(only(&doc).kind, ShapeType::Shape, "type {value:?}");
        assert_eq!(report.warning_counts.get("diacraft_tier2_unreadable"), Some(&1), "type {value:?}");
    }
}

/// A new shape of kind `Guide` is written with the attribute and nothing else that is the kind's, reads back as a guide, and a second save is no edit; a shape of any
/// other kind has no such attribute.
#[test]
fn a_new_guide_is_written_with_its_kind_and_reads_back_as_one() {
    let mut o = open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut guide = rectangle(id, "Guide.1", (2.0, 3.0, 4.0, 2.0));
    guide.kind = ShapeType::Guide;
    let other = rectangle(id + 1, "Shape.2", (2.0, 8.0, 4.0, 2.0));
    push_shape(&mut o.doc, guide);
    push_shape(&mut o.doc, other);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("saved: {e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("diacraft:type=\"Guide\"").count(), 1, "the guide says what it is and the other shape does not: {content}");
    let (back, report) = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    let kinds: Vec<ShapeType> = back.pages.get(0).unwrap().shapes.iter().map(|s| s.kind.clone()).collect();
    assert_eq!(kinds[1..], [ShapeType::Guide, ShapeType::Shape], "{kinds:?}");
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}
