//! A retext of a shape whose text holds a field the reader maps to a `Field` row (`text:page-number`, `text:page-count`, `text:file-name`) keeps the
//! field where the file has it. The splice writer refused such a text whole ("a text with a field, a tab stop marker or an element the model keeps among its parts"): the field's shown
//! text is now among the paragraph's characters, and its element is kept by its bytes as any field's is.

use std::sync::Arc;

use diacraft_doc::TextPart;
use diacraft_odg::SaveNote;

use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real, shape_mut};
use crate::write_runs::{id_of, shape, the_shape};

const FIELDS: &str = r#"<text:p>page <text:page-number>3</text:page-number> of <text:page-count>9</text:page-count></text:p><text:p>second</text:p>"#;

fn drawing(inner: &str) -> Drawing {
    Drawing::a4().page(&shape("x", "1cm", inner))
}

/// The model's text with `from` in a run of characters replaced by `to`.
fn retype(o: &mut crate::write_core::Open, from: &str, to: &str) {
    let id = id_of(&o.doc);
    let parts: Vec<TextPart> = the_shape(&o.doc)
        .text
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(c) if c.contains(from) => TextPart::Chars(Arc::from(c.replace(from, to))),
            other => other.clone(),
        })
        .collect();
    shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

#[test]
fn a_text_with_a_field_is_retyped_and_the_field_stays_where_the_file_has_it() {
    let mut o = open(&drawing(FIELDS));
    retype(&mut o, "second", "third");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let xml = part(&saved.bytes, "content.xml");
    assert!(
        xml.contains("<text:p>page <text:page-number>3</text:page-number> of <text:page-count>9</text:page-count></text:p><text:p>third</text:p>"),
        "{xml}"
    );
    // The file reads as the model: the same fields, and the new text.
    let back = open_bytes(saved.bytes.clone());
    let fields = |d: &diacraft_doc::Document| -> Vec<String> {
        the_shape(d)
            .text
            .as_ref()
            .unwrap()
            .parts
            .iter()
            .filter_map(|p| if let TextPart::Fld { text, .. } = p { Some(text.to_string()) } else { None })
            .collect()
    };
    assert_eq!(fields(&back.doc), ["3", "9"]);
    assert!(saved.notes.iter().all(|n| !matches!(n, SaveNote::FieldWritten { .. })), "{:?}", saved.notes);
    // A second save is no edit.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
}

#[test]
fn a_retype_of_the_paragraph_that_holds_the_fields_keeps_them_too() {
    let mut o = open(&drawing(FIELDS));
    retype(&mut o, "page ", "sheet ");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let xml = part(&saved.bytes, "content.xml");
    assert!(xml.contains("sheet <text:page-number>3</text:page-number> of <text:page-count>9</text:page-count>"), "{xml}");
}

#[test]
fn a_file_name_field_survives_a_retext_of_its_shape() {
    let mut o = open(&drawing(r#"<text:p>in <text:file-name>plan.odg</text:file-name></text:p><text:p>x</text:p>"#));
    retype(&mut o, "x", "y");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    assert!(part(&saved.bytes, "content.xml").contains("<text:p>in <text:file-name>plan.odg</text:file-name></text:p><text:p>y</text:p>"));
}

/// The model's text with the field of row `ix` showing `shown`, as a refresh of the fields leaves it.
fn refreshed(o: &mut crate::write_core::Open, ix: u32, shown: &str) {
    let id = id_of(&o.doc);
    let parts: Vec<TextPart> = the_shape(&o.doc)
        .text
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|p| match p {
            TextPart::Fld { ix: i, .. } if *i == ix => TextPart::Fld { ix: *i, text: Arc::from(shown) },
            other => other.clone(),
        })
        .collect();
    shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

/// The `IX` of the fields of the shape, in order.
fn field_rows(d: &diacraft_doc::Document) -> Vec<(u32, String)> {
    the_shape(d)
        .text
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .filter_map(|p| if let TextPart::Fld { ix, text } = p { Some((*ix, text.to_string())) } else { None })
        .collect()
}

/// Fix 2: a field whose characters a refresh changed is still a field the model holds (a `fld` and its row), so the save writes its element again with the
/// new characters, its attributes as the file had them, and says nothing: a field never loses its element in a save.
#[test]
fn a_field_a_refresh_changed_keeps_its_element_with_the_new_characters() {
    let inner = r#"<text:p>page <text:page-number text:select-page="current">3</text:page-number> of <text:page-count>9</text:page-count></text:p><text:p>second</text:p>"#;
    let mut o = open(&drawing(inner));
    let rows = field_rows(&o.doc);
    assert_eq!(rows.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["3", "9"]);
    refreshed(&mut o, rows[0].0, "12");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let xml = part(&saved.bytes, "content.xml");
    assert!(
        // The space after the field is `text:s`, as a paragraph written whole writes it (R10 item 6).
        xml.contains(r#"<text:p>page <text:page-number text:select-page="current">12</text:page-number><text:s/>of <text:page-count>9</text:page-count></text:p><text:p>second</text:p>"#),
        "{xml}"
    );
    assert!(saved.notes.iter().all(|n| !matches!(n, SaveNote::FieldWritten { .. })), "{:?}", saved.notes);
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(field_rows(&back.doc).iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["12", "9"]);
    // And with the rest of the paragraph retyped in the same edit: the field keeps its element too.
    let mut o = open(&drawing(inner));
    refreshed(&mut o, rows[0].0, "1");
    retype(&mut o, "page ", "sheet ");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let xml = part(&saved.bytes, "content.xml");
    assert!(
        xml.contains(r#"sheet <text:page-number text:select-page="current">1</text:page-number><text:s/>of <text:page-count>9</text:page-count>"#),
        "{xml}"
    );
    assert!(saved.notes.iter().all(|n| !matches!(n, SaveNote::FieldWritten { .. })), "{:?}", saved.notes);
}

/// The same inside a span, and for a field that shows characters that need escaping: the new characters are written as text.
#[test]
fn a_refreshed_field_inside_a_span_is_written_with_escaped_characters() {
    let inner = r#"<text:p><text:span text:style-name="T1">in <text:file-name>plan.odg</text:file-name></text:span></text:p>"#;
    let d = Drawing::a4()
        .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .page(&shape("x", "1cm", inner));
    let mut o = open(&d);
    let rows = field_rows(&o.doc);
    assert_eq!(rows.len(), 1, "{rows:?}");
    refreshed(&mut o, rows[0].0, "a<b&c.odg");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let xml = part(&saved.bytes, "content.xml");
    assert!(xml.contains(r#"in <text:file-name>a&lt;b&amp;c.odg</text:file-name></text:span>"#), "{xml}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(field_rows(&back.doc).iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["a<b&c.odg"]);
}
