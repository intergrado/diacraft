//! The texts: each paragraph of a shape's text holds the style it named (`None` is held too) with the stamp of its row, a run for each `text:span`, the values of
//! its automatic style that lists ("Paragraph and run styles") with a stamp of their own, and a stamp of its characters; and a bookmark in a paragraph is a
//! zero-width part of the text, which the splice writer leaves out of what it writes. The inputs are written by hand from ODF 1.4 Part 3.

use std::sync::Arc;

use diacraft_doc::{Document, RawNode, RowKey, SectionKey, Shape, TextGroup, TextPart, stamp_text_group, stamp_text_row};

use crate::drawing::Drawing;
use crate::write_core::{open, part, real, shape_mut};

const STYLES: &str = concat!(
    r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="start" style:line-spacing="0.2cm" fo:margin-left="1cm"/></style:style>"##,
    r##"<style:style style:name="T1" style:family="text"><style:text-properties style:text-underline-style="dotted" style:text-underline-color="#ff0000" style:text-underline-width="bold" fo:text-transform="lowercase" fo:font-weight="bold"/></style:style>"##,
    r##"<style:style style:name="T2" style:family="text"><style:text-properties style:text-underline-style="solid" fo:font-style="italic"/></style:style>"##,
);

const TEXT: &str = concat!(
    r##"<text:p text:style-name="P1">a<text:span text:style-name="T1">bc</text:span>d<text:bookmark text:name="b1"/>e<text:span text:style-name="T2">f</text:span></text:p>"##,
    r##"<text:p>second</text:p>"##,
);

fn shape_of(text: &str) -> Drawing {
    Drawing::a4().auto(STYLES).page(&format!(
        r##"<draw:custom-shape draw:name="S" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm">{text}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"##
    ))
}

fn the_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("S")).expect("the shape").as_ref()
}

/// Each paragraph holds the style it named and the stamp of its row; a run for each span with its own; the values of the automatic style, each stamped by its group of the row.
#[test]
fn a_paragraph_and_a_run_hold_the_style_they_named() {
    let (doc, _) = shape_of(TEXT).read();
    let shape = the_shape(&doc);
    let text = shape.text.as_ref().expect("the text");
    let odf = text.ext.get().expect("the text's slot").odf.clone();
    assert_eq!(odf.paragraphs.len(), 2, "a paragraph for each text:p of the file");
    let (p0, p1) = (&odf.paragraphs[0], &odf.paragraphs[1]);
    assert_eq!(p0.style.name.as_deref(), Some("P1"));
    assert_eq!(p1.style.name, None, "that a paragraph named no style is held too");
    assert!(p1.style.stamp.is_some() && p1.characters.is_some() && p0.characters != p1.characters, "a stamp of each paragraph's characters");
    // The paragraph's `Paragraph` row, and the stamps R22 gives its held values: HorzAlign (with Flags) for the alignment, SpLine for the line spacing.
    let rows = shape.sheet.section(&SectionKey::new("Paragraph", None)).expect("Paragraph rows");
    let empty = diacraft_doc::Row::default();
    let row = |ix: u32| rows.row(&RowKey::Ix(ix)).map_or(&empty, |r| &**r);
    let stamps: Vec<(TextGroup, Option<diacraft_doc::Stamp>)> = p0.style.held.iter().map(|h| (h.group, h.stamp)).collect();
    assert_eq!(stamps.iter().map(|(g, _)| *g).collect::<Vec<_>>(), [TextGroup::HorzAlign, TextGroup::SpLine]);
    let held_row = row(0);
    for held in &p0.style.held {
        assert_eq!(held.stamp, Some(stamp_text_group(held_row, held.group)), "{:?}", held.group);
    }
    let align = &p0.style.held[0];
    assert_eq!(align.attrs.get("fo:text-align"), Some("start"));
    assert_eq!(p0.style.held[1].attrs.get("style:line-spacing"), Some("0.2cm"));
    assert!(p0.style.held.iter().all(|h| h.attrs.get("fo:margin-left").is_none()), "a margin is a cell's");
    assert_eq!(p0.style.stamp, Some(stamp_text_row(held_row)));
    // The runs, by where they lie in the paragraph's characters: `a|bc|d|e|f`.
    let runs: Vec<(u32, u32, Option<&str>)> = p0.runs.iter().map(|r| (r.from, r.to, r.style.name.as_deref())).collect();
    assert_eq!(runs, [(1, 3, Some("T1")), (5, 6, Some("T2"))]);
    let t1 = &p0.runs[0].style;
    let groups: Vec<TextGroup> = t1.held.iter().map(|h| h.group).collect();
    assert_eq!(groups, [TextGroup::Underline, TextGroup::Case]);
    assert_eq!(t1.held[0].attrs.get("style:text-underline-style"), Some("dotted"));
    assert_eq!(t1.held[0].attrs.get("style:text-underline-color"), Some("#ff0000"));
    assert_eq!(t1.held[0].attrs.get("style:text-underline-width"), Some("bold"));
    assert_eq!(t1.held[1].attrs.get("fo:text-transform"), Some("lowercase"));
    assert!(t1.held.iter().all(|h| h.attrs.get("fo:font-weight").is_none()), "a weight is a cell's");
    // A solid underline is the cell's alone (R22 holds a style other than `solid`): nothing is held for T2's.
    assert!(p0.runs[1].style.held.is_empty());
    assert!(p0.runs[1].style.stamp.is_some());
}

/// A bookmark is held whole, where it stands, as a zero-width part of the text: the characters before and after it are parts of their own, and the text's characters are as they were.
#[test]
fn a_bookmark_is_a_zero_width_part_of_the_text() {
    let (doc, _) = shape_of(TEXT).read();
    let text = the_shape(&doc).text.as_ref().expect("the text").clone();
    let mut before = String::new();
    let mut after = String::new();
    let mut seen = false;
    let mut marks = 0;
    for part in &text.parts {
        match part {
            TextPart::OdfRaw(node) => {
                marks += 1;
                seen = true;
                match &**node {
                    RawNode::Element(e) => {
                        assert_eq!(&*e.name, "text:bookmark");
                        assert_eq!(e.attrs.get("text:name"), Some("b1"));
                    }
                    other => panic!("{other:?}"),
                }
            }
            TextPart::Chars(c) if seen => after.push_str(c),
            TextPart::Chars(c) => before.push_str(c),
            _ => {}
        }
    }
    assert_eq!((marks, before.as_str(), after.as_str()), (1, "abcd", "ef\nsecond"));
    // A text of bookmarks alone is empty text: nothing is held.
    let (doc, _) = shape_of(r##"<text:p><text:bookmark text:name="only"/></text:p>"##).read();
    assert!(the_shape(&doc).text.is_none());
}

/// The splice writer is not turned back by the bookmark: a shape whose text has one is retyped, and the file keeps its bookmark's paragraph's style.
#[test]
fn a_text_with_a_bookmark_is_retyped_by_the_splice_writer() {
    let d = shape_of(TEXT);
    let o = open(&d);
    let mut doc = o.doc.clone();
    let id = the_shape(&doc).id;
    let shape = shape_mut(&mut doc, 0, id);
    let text = shape.text.as_ref().expect("text").clone();
    let mut parts = text.parts.clone();
    for p in &mut parts {
        if let TextPart::Chars(c) = p
            && c.contains("second")
        {
            *p = TextPart::Chars(Arc::from(c.replacen("second", "2nd", 1)));
        }
    }
    shape.text = Some(Arc::new(diacraft_doc::Text { parts, ext: text.ext.clone() }));
    let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("2nd") && !content.contains("second"), "{content}");
    assert!(content.contains(r##"<text:bookmark text:name="b1"/>"##), "the first paragraph is as the file had it: {content}");
}
