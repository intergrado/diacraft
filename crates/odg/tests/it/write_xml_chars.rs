//! What the writer puts into a file that must stay a file: a character XML 1.0 does not allow, written as it was,
//! made a file our own reader refuses; a carriage return was refused by the check of the bytes made and not in words, and in a new shape's text it
//! became a space; a number past what a coordinate holds was written as `inf`. Every case here goes through `read` of the bytes saved, not through
//! the writer's own tokenizer, which passed the files the reader refuses.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape, Text, TextPart};
use diacraft_odg::{Limits, WriteError, read};

use crate::drawing::Drawing;
use crate::write_core::{custom, open, open_bytes, part, real, shape_mut};
use crate::write_new::{push_shape, rectangle};

/// The characters that go through the tests: the controls, the edges of the ranges XML 1.0 allows, the noncharacters and the ends of the planes.
fn characters() -> Vec<char> {
    let mut all: Vec<char> = (0u32..0x100).filter_map(char::from_u32).filter(|c| *c != '\n').collect();
    all.extend(['\u{2028}', '\u{D7FF}', '\u{E000}', '\u{FEFF}', '\u{FFFD}', '\u{FFFE}', '\u{FFFF}', '\u{10000}', '\u{10FFFF}']);
    all
}

/// Whether the writer must refuse `c` in a text: what XML 1.0 forbids, and the carriage return.
fn refused_in_text(c: char) -> bool {
    (c < ' ' && !matches!(c, '\t' | '\n')) || c == '\u{FFFE}' || c == '\u{FFFF}'
}

/// Whether the writer must refuse `c` in a name: what XML 1.0 forbids (a carriage return in an attribute is written as a reference).
fn refused_in_name(c: char) -> bool {
    (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || c == '\u{FFFE}' || c == '\u{FFFF}'
}

fn text_of(shape: &Shape) -> String {
    shape.text.as_ref().map_or(String::new(), |t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect())
}

fn set_text(doc: &mut Document, id: u32, text: &str) {
    shape_mut(doc, 0, id).text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
}

/// A refusal is in words (`Unsupported`), never the check of the bytes made; a save is a file our reader reads.
#[track_caller]
fn judge(what: &str, c: char, refused: bool, saved: Result<diacraft_odg::Written, WriteError>) -> Option<Vec<u8>> {
    match saved {
        Ok(w) => {
            assert!(!refused, "{what}: U+{:04X} was written, and XML 1.0 or the reader does not allow it", u32::from(c));
            if let Err(e) = read(&w.bytes, &Limits::default()) {
                panic!("{what}: U+{:04X} was written and our own reader refuses the file: {e}", u32::from(c));
            }
            Some(w.bytes)
        }
        Err(WriteError::Unsupported(words)) => {
            assert!(refused, "{what}: U+{:04X} was refused ({words}) and the reader reads it", u32::from(c));
            assert!(!words.is_empty());
            None
        }
        Err(other) => panic!("{what}: U+{:04X} was refused as {other:?}, which is not a refusal in words", u32::from(c)),
    }
}

/// A retext of an existing shape to `a<c>b`.
#[test]
fn every_character_in_a_retext_is_refused_in_words_when_xml_forbids_it_and_reads_back_when_it_does_not() {
    let d = Drawing::a4().page(r#"<draw:custom-shape draw:name="t" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:p>old</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#);
    for c in characters() {
        let mut o = open(&d);
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        let text = format!("a{c}b");
        set_text(&mut o.doc, id, &text);
        let Some(bytes) = judge("retext", c, refused_in_text(c), real(&o.doc, &o.bytes, &o.index)) else { continue };
        let (doc, _) = read(&bytes, &Limits::default()).unwrap();
        let back = text_of(doc.pages.get(0).unwrap().shapes.get(0).unwrap());
        // White space the format collapses is read as the one space it is; every other character is read as it was.
        let expected = if c.is_ascii_whitespace() && c != '\t' { "a b".to_owned() } else { text };
        assert_eq!(back, expected, "retext: U+{:04X}", u32::from(c));
    }
}

/// A new shape with the text `a<c>b`, and one with the name `N<c>`.
#[test]
fn every_character_in_a_new_shape_s_text_and_name_is_refused_in_words_when_xml_forbids_it_and_reads_back_when_it_does_not() {
    for c in characters() {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        let text = format!("a{c}b");
        r.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text.as_str()))], ..Default::default() }));
        push_shape(&mut o.doc, r);
        if let Some(bytes) = judge("new text", c, refused_in_text(c), real(&o.doc, &o.bytes, &o.index)) {
            let (doc, _) = read(&bytes, &Limits::default()).unwrap();
            let back = text_of(doc.pages.get(0).unwrap().shapes.get(1).unwrap());
            let expected = if c.is_ascii_whitespace() && c != '\t' { "a b".to_owned() } else { text };
            assert_eq!(back, expected, "new text: U+{:04X}", u32::from(c));
        }
        // The name goes in an attribute: a carriage return, a line feed and a tab are written as references and read back.
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        let name = format!("N{c}");
        r.name_u = Some(Arc::from(name.as_str()));
        push_shape(&mut o.doc, r);
        if let Some(bytes) = judge("new name", c, refused_in_name(c), real(&o.doc, &o.bytes, &o.index)) {
            let (doc, _) = read(&bytes, &Limits::default()).unwrap();
            assert_eq!(doc.pages.get(0).unwrap().shapes.get(1).unwrap().name_u.as_deref(), Some(name.as_str()), "new name: U+{:04X}", u32::from(c));
        }
        // The shape's `Name`, when it is not its `NameU`, is an attribute of DiaCraft's extension: the same characters are refused in words there, by the
        // guard of the writer and, were it not there, by the encoding of the extension (the mutation run found the guard's two halves tied together by `||`).
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        r.name = Some(Arc::from(name.as_str()));
        push_shape(&mut o.doc, r);
        if let Some(bytes) = judge("new Name", c, refused_in_name(c), real(&o.doc, &o.bytes, &o.index)) {
            let (doc, _) = read(&bytes, &Limits::default()).unwrap();
            assert_eq!(doc.pages.get(0).unwrap().shapes.get(1).unwrap().name.as_deref(), Some(name.as_str()), "new Name: U+{:04X}", u32::from(c));
        }
    }
}

/// The text of a new shape is read back by the check of the bytes made as the text of an edited one is: a carriage return in it is refused in words,
/// and does not become a space.
#[test]
fn a_carriage_return_in_a_text_is_refused_in_words_in_a_retext_and_in_a_new_shape() {
    let words = |saved: Result<diacraft_odg::Written, WriteError>| match saved {
        Err(WriteError::Unsupported(words)) => words,
        other => panic!("expected a refusal in words: {other:?}"),
    };
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    let id = o.index.next_id(0).unwrap();
    let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
    r.text = Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("one\r\ntwo"))], ..Default::default() }));
    push_shape(&mut o.doc, r);
    assert!(words(real(&o.doc, &o.bytes, &o.index)).contains("carriage return"));
    let d = Drawing::a4().page(r#"<draw:custom-shape draw:name="t" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:p>old</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#);
    let mut o = open(&d);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    set_text(&mut o.doc, id, "one\r\ntwo");
    assert!(words(real(&o.doc, &o.bytes, &o.index)).contains("carriage return"));
}

/// A row of `Width*1e300` was written as `draw:enhanced-path="M 0 21600 L inf ..."`. A coordinate or a modifier past 1e15, or not a
/// number, is refused in words, in a new shape and in an edit; a share the path holds is written whole, and `-0` and a share too small to show are 0.
#[test]
fn a_number_past_what_a_coordinate_holds_is_refused_in_words_and_never_written_as_inf() {
    let new_with_share = |k: &str| {
        let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
        let id = o.index.next_id(0).unwrap();
        let mut r = rectangle(id, "R", (2.0, 3.0, 4.0, 2.0));
        {
            let sheet = Arc::make_mut(&mut r.sheet);
            let g = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap();
            let row = Arc::make_mut(Arc::make_mut(g).rows.get_mut(1).unwrap());
            row.cells.insert(CellName::new("X"), Cell::read(Some(&format!("Width*{k}")), Some("1"), Some("IN"), None, &mut 1000));
        }
        push_shape(&mut o.doc, r);
        real(&o.doc, &o.bytes, &o.index)
    };
    // A row the path cannot hold is tier 2: the file holds a placeholder rectangle, the row is carried as the text it is, and nothing is written as
    // `inf`.
    for k in ["1e300", "1e20", "1e12"] {
        let saved = new_with_share(k).unwrap_or_else(|e| panic!("{k}: {e}"));
        assert!(saved.notes.iter().any(|n| n.class() == "outline_approximated"), "{k}: {:?}", saved.notes);
        let content = part(&saved.bytes, "content.xml");
        let path = content.split("draw:enhanced-path=\"").last().unwrap().split('"').next().unwrap();
        assert!(!path.contains("inf") && !path.contains("NaN") && !path.contains('e'), "{k}: {path}");
        let back = open_bytes(saved.bytes);
        let read = back.doc.pages.get(0).unwrap().shapes.get(1).unwrap().clone();
        let section = read.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap();
        let formula = section.rows.get(1).and_then(|r| r.cells.get("X")).and_then(|c| c.formula.text()).unwrap_or_default().to_owned();
        assert_eq!(formula, format!("Width*{k}"), "the row reads back as it was");
    }
    for (k, shown) in [("1e10", "216000000000000"), ("-0", " 0 "), ("1e-300", " 0 ")] {
        let saved = new_with_share(k).unwrap_or_else(|e| panic!("{k}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        let path = content.split("draw:enhanced-path=\"").last().unwrap().split('"').next().unwrap();
        assert!(!path.contains("inf") && !path.contains("NaN") && !path.contains('e'), "{k}: {path}");
        assert!(path.contains(shown), "{k}: {path}");
        read(&saved.bytes, &Limits::default()).unwrap_or_else(|e| panic!("{k}: the file does not read: {e}"));
    }
}
