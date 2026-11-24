//! The writer's retext (retext; plan Review Focus 5): only the
//! edited paragraphs are rewritten, and the paragraph's style, its list and its other paragraphs are kept.

use std::sync::Arc;

use diacraft_doc::{Document, Shape, TextPart};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{Saver, compare, content_changes, open, open_bytes, part, real, shape_mut};

const STYLES: &str = r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="•"/></text:list-style>"##;

/// A custom shape holding `inner` as its text, with an enhanced geometry after it.
fn shape(name: &str, x: &str, inner: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{inner}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    )
}

/// Right-to-left and CJK text, a tab, a line break and a counted space, in two bulleted paragraphs of one list.
const LIST: &str = "<text:list text:style-name=\"L1\"><text:list-item><text:p text:style-name=\"P1\">one<text:s text:c=\"2\"/>two</text:p></text:list-item><text:list-item><text:p text:style-name=\"P1\">\u{645}\u{631}\u{62d}\u{628}\u{627}<text:tab/>\u{4f60}\u{597d}<text:line-break/>end</text:p></text:list-item></text:list>";

fn drawing(a: &str, b: &str) -> Drawing {
    Drawing::a4().auto(STYLES).page(&format!("{}{}", shape("x", "1cm", a), shape("y", "6cm", b)))
}

fn id_of(doc: &Document, name: &str) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap().id
}

/// The model's text of a shape, parts and all, as characters with the markers named.
fn show(shape: &Shape) -> String {
    shape.text.as_ref().map_or(String::new(), |t| {
        t.parts
            .iter()
            .map(|p| match p {
                TextPart::Chars(c) => c.to_string(),
                TextPart::Cp(i) => format!("<cp{i}>"),
                TextPart::Pp(i) => format!("<pp{i}>"),
                _ => "?".to_owned(),
            })
            .collect()
    })
}

/// Replaces `from` by `to` in the characters of the shape's text.
fn retype(shape: &mut Shape, from: &str, to: &str) {
    let text = shape.text.as_ref().expect("a shape with text");
    let mut parts = text.parts.clone();
    let mut done = false;
    for p in parts.iter_mut() {
        if let TextPart::Chars(c) = p
            && c.contains(from)
        {
            *p = TextPart::Chars(Arc::from(c.replacen(from, to, 1)));
            done = true;
            break;
        }
    }
    assert!(done, "no {from:?} in {}", show(shape));
    shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

/// What a retext of a custom shape that has no style adds (the retext row): a new style whose
/// only attribute is `draw:auto-grow-height="false"`, so that LibreOffice keeps the frame, and the shape's `draw:style-name` naming it.
const GROW_STYLE: &str =
    r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:auto-grow-height="false"/></style:style>"#;

/// The part without what [`GROW_STYLE`] adds: the style and the first `draw:style-name="gr1"`, so that what is asserted of the rest of the part holds
/// as it did.
fn strip_grow(content: &str) -> String {
    content.replacen(GROW_STYLE, "", 1).replacen(" draw:style-name=\"gr1\"", "", 1)
}

/// Review Focus 5: a retext of a shape whose text has a list, right-to-left and CJK runs, `text:s`, `text:tab` and `text:line-break`, and a retext of
/// the other shape: the paragraph is rewritten between its own tags, which keep their style and the list around them; the other paragraph and the
/// other shape are byte for byte as they were.
fn retext_case(save: &Saver) {
    let mut o = open(&drawing(LIST, LIST));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "two", "three");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // The first paragraph of the first shape: the same style and list, the text as R24 writes it (a first space as itself, a counted one for the rest).
    let expected = before.replacen("one<text:s text:c=\"2\"/>two</text:p>", "one <text:s/>three</text:p>", 1);
    assert_ne!(expected, before);
    assert_eq!(strip_grow(&after), expected, "the paragraph's content, and nothing else of the part (but the shape's frame held: see `strip_grow`)");
    assert!(after.contains(GROW_STYLE) && after.contains(" draw:style-name=\"gr1\""), "{after}");
    assert!(
        after.contains("<text:list text:style-name=\"L1\"><text:list-item><text:p text:style-name=\"P1\">one <text:s/>three</text:p>"),
        "{after}"
    );
    // The second paragraph with its tab and line break and the other shape's whole text are as they were.
    let second = "\u{645}\u{631}\u{62d}\u{628}\u{627}<text:tab/>\u{4f60}\u{597d}<text:line-break/>end</text:p>";
    assert_eq!(after.matches(second).count(), 2, "both shapes keep that paragraph");
    assert_eq!(after.matches(LIST).count(), 1, "the other shape's whole list is byte for byte");
    // By element: only inside the first shape.
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert!(changed.iter().all(|(path, _)| path.contains("draw:custom-shape[1]")), "{diff:?}");
    assert_eq!((added, removed), (1, 0), "the style that holds the frame: {diff:?}");
    // Read back: the text is the model's, the other shape's is as it was.
    let back = open_bytes(saved.bytes);
    assert_eq!(show(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap()), show(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap()));
    assert_eq!(show(back.doc.pages.get(0).unwrap().shapes.get(1).unwrap()), show(o.doc.pages.get(0).unwrap().shapes.get(1).unwrap()));
    // The other shape retyped alone leaves the first byte for byte.
    let mut o = open(&drawing(LIST, LIST));
    let y = id_of(&o.doc, "y");
    retype(shape_mut(&mut o.doc, 0, y), "end", "fin");
    let after = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    let before = part(&o.bytes, "content.xml");
    let (cut, _) = before.split_once("draw:name=\"y\"").unwrap();
    assert!(strip_grow(&after).starts_with(cut), "the first shape and all before the second are byte for byte");
    assert!(after.contains("<text:line-break/>fin</text:p>") && after.matches("<text:line-break/>end</text:p>").count() == 1, "{after}");
}

/// V11 (R9 retext): a span of bold `ab` and plain `cd` retyped as `ce`: the span with its style kept byte for byte, only the text after it changed.
fn a_span_is_kept_and_only_the_text_after_it_changes(save: &Saver) {
    let body = "<text:p text:style-name=\"P1\"><text:span text:style-name=\"T1\">ab</text:span>cd</text:p>";
    let mut o = open(&drawing(body, body));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "cd", "ce");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(strip_grow(&after), before.replacen("</text:span>cd</text:p>", "</text:span>ce</text:p>", 1));
    // A character typed inside the span's run is written inside the span.
    let mut o = open(&drawing(body, body));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "ab", "aXb");
    let after = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("<text:span text:style-name=\"T1\">aXb</text:span>cd</text:p>"), "{after}");
    // White space elements before the edit stand for the characters they read as: the edit is found after them, and they and the span are kept.
    let with_space = "<text:p text:style-name=\"P1\">a<text:tab/>b<text:s text:c=\"2\"/>c<text:span text:style-name=\"T1\">d</text:span>ef</text:p>";
    let mut o = open(&drawing(with_space, with_space));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "ef", "eg");
    let after = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("c<text:span text:style-name=\"T1\">d</text:span>eg</text:p>"), "{after}");
    assert!(after.contains("a<text:tab/>b<text:s text:c=\"2\"/>c<text:span"), "{after}");
    // The first character of the run after the span replaced: the boundary is where the edit begins, and the edit is in that run.
    let mut o = open(&drawing(body, body));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "cd", "Xd");
    let after = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("<text:span text:style-name=\"T1\">ab</text:span>Xd</text:p>"), "{after}");
    // The last character of the span replaced: the span's end is where the edit ends, and the edit is inside the span.
    let mut o = open(&drawing(body, body));
    let x = id_of(&o.doc, "x");
    retype(shape_mut(&mut o.doc, 0, x), "ab", "aX");
    let after = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(after.contains("<text:span text:style-name=\"T1\">aX</text:span>cd</text:p>"), "{after}");
}

/// A retext of a custom shape that holds text writes `draw:auto-grow-height="false"` into the shape's graphic style when it has none
/// (the LibreOffice probe: the retexted 4 cm frame became 8.198 cm wide), by the restyle rules: in the style where the shape alone names it, in a
/// copy where others name it too, in a new style on its common style or none; a style that has the attribute is kept as it is, and neither a
/// retext to no text nor a retext of a shape that is no custom shape writes it. Each is checked with `diff_packages`, and saved again it is no edit.
#[test]
fn a_retext_of_a_custom_shape_writes_auto_grow_height_false_into_its_style_by_the_restyle_rules() {
    let custom = |name: &str, style: &str| {
        let attr = if style.is_empty() { String::new() } else { format!(" draw:style-name=\"{style}\"") };
        format!(
            "<draw:custom-shape draw:name=\"{name}\"{attr} svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p>old</text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
        )
    };
    let autos = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style><style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:auto-grow-height="true"/></style:style>"##;
    let run = |body: &str, edit: &dyn Fn(&mut crate::write_core::Open)| {
        let d = Drawing::a4().common(r#"<style:style style:name="Box" style:family="graphic"/>"#).auto(autos).page(body);
        let mut o = open(&d);
        edit(&mut o);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
        let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
        assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
        (o, saved)
    };
    let retype_first = |o: &mut crate::write_core::Open, text: &str| {
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        shape_mut(&mut o.doc, 0, id).text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![TextPart::Chars(Arc::from(text))], ..Default::default() }));
    };
    // The shape alone names gr1, which has no such attribute: the style has it, where it stands.
    let (o, saved) = run(&custom("a", "gr1"), &|o| retype_first(o, "new"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r##"<style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:auto-grow-height="false"/>"##), "{after}");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((added, removed), (0, 0), "{diff:?}");
    assert_eq!(changed.len(), 2, "the shape and the style: {diff:?}");
    assert!(changed.iter().any(|(_, attrs)| attrs.iter().any(|a| a == "draw:auto-grow-height")), "{diff:?}");
    // gr1 named by two shapes: the retyped one has a copy that has it, and the other shape's style is as it was.
    let (o, saved) = run(&format!("{}{}", custom("a", "gr1"), custom("b", "gr1")), &|o| retype_first(o, "new"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr3""#) && after.contains(r#"draw:name="b" draw:style-name="gr1""#), "{after}");
    assert!(after.contains(r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:auto-grow-height="false"/>"##), "{after}");
    let (changed, added, _) = content_changes(&compare(&o.bytes, &saved.bytes));
    assert_eq!((changed.len(), added), (2, 1), "the shape (its style name), its paragraph, and the new style");
    // A style that has the attribute, whatever it says: kept. The edit is the paragraph and nothing else.
    let (o, saved) = run(&custom("a", "gr2"), &|o| retype_first(o, "new"));
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(after, before.replacen("<text:p>old</text:p>", "<text:p>new</text:p>", 1));
    // A common style: a new automatic style that has it and names the common one as its parent.
    let (_, saved) = run(&custom("a", "Box"), &|o| retype_first(o, "new"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="Box"><style:graphic-properties draw:auto-grow-height="false"/></style:style>"#), "{after}");
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr3""#), "{after}");
    // No style at all: a new one with no parent.
    let (_, saved) = run(&custom("a", ""), &|o| retype_first(o, "new"));
    assert!(
        part(&saved.bytes, "content.xml").contains(
            r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:auto-grow-height="false"/></style:style>"#
        )
    );
    // A retext to no text, and the retext of a shape that is no custom shape, write no style.
    let (o, saved) = run(&custom("a", "gr1"), &|o| retype_first(o, ""));
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml").replacen("<text:p>old</text:p>", "<text:p></text:p>", 1));
    let rect = r#"<draw:rect draw:name="r" draw:style-name="gr1" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p>old</text:p></draw:rect>"#;
    let (o, saved) = run(rect, &|o| retype_first(o, "new"));
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml").replacen("<text:p>old</text:p>", "<text:p>new</text:p>", 1));
}

/// A paragraph the file lays out with indentation (it reads as nothing), a counted space whose count is in single quotes, and a
/// paragraph of tens of megabytes of white space were refused as `write_mismatch`, "the file on record is not the one this drawing was opened from",
/// when the file is the one read. They are written: the paragraph's text is rewritten as R24 writes it, and reads back as the model has it.
#[test]
fn an_indented_paragraph_a_count_in_single_quotes_and_a_huge_run_of_white_space_are_written_and_not_refused_as_a_mismatch() {
    let one = |inner: &str, from: &str, to: &str| {
        let mut o = open(&Drawing::a4().page(&shape("x", "1cm", inner)));
        let x = id_of(&o.doc, "x");
        retype(shape_mut(&mut o.doc, 0, x), from, to);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{inner:.60}: {} ({e})", e.class()));
        let back = open_bytes(saved.bytes.clone());
        assert_eq!(
            show(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap()),
            show(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap()),
            "{inner:.60}"
        );
        part(&saved.bytes, "content.xml")
    };
    let after = one("<text:p>\n          hello\n        </text:p>", "hello", "jello");
    assert!(after.contains("<text:p>jello</text:p>"), "the indentation is not part of the text: {after}");
    let after = one("<text:p text:style-name=\"P1\">\n  hello world\n</text:p>", "world", "there");
    assert!(after.contains("<text:p text:style-name=\"P1\">hello there</text:p>"), "{after}");
    let after = one("<text:p>a<text:s text:c='3'/>b end</text:p>", "end", "fin");
    assert!(after.contains("<text:p>a <text:s text:c=\"2\"/>b fin</text:p>"), "the count in single quotes is three spaces: {after}");
    let after = one(&format!("<text:p>a<text:s text:c = '3' />b{}</text:p>", " ".repeat(40 << 20)), "b", "c");
    assert!(after.contains("<text:p>a <text:s text:c=\"2\"/>c</text:p>"), "{}", &after[..after.len().min(400)]);
}

/// An edit in a paragraph with spans whose raw text node holds a reference, a tab, a line feed or two spaces in a row has no byte range of its own: the
/// paragraph is written again whole from the model, and the text reads back as the model has it.
#[test]
fn an_edit_in_a_paragraph_with_spans_where_a_text_node_holds_a_reference_a_tab_or_two_spaces_is_written_whole() {
    for node in ["&amp;x", "a\tb", "a\nb", "a  b"] {
        let body = format!("<text:p text:style-name=\"P1\"><text:span text:style-name=\"T1\">ab</text:span>{node}cd</text:p>");
        let o = open(&drawing(&body, &body));
        let x = id_of(&o.doc, "x");
        let mut doc = o.doc.clone();
        retype(shape_mut(&mut doc, 0, x), "cd", "ce");
        let saved = real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{node:?}: {e}"));
        let back = open_bytes(saved.bytes);
        let chars = |d: &Document| -> String {
            d.pages
                .get(0)
                .unwrap()
                .shapes
                .get(0)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .parts
                .iter()
                .filter_map(|p| if let TextPart::Chars(c) = p { Some(c.to_string()) } else { None })
                .collect()
        };
        assert_eq!(chars(&back.doc), chars(&doc), "{node:?}");
    }
}

/// Review LOW: a paragraph's own style marker at offset 0 no longer refuses a character typed at the paragraph's start, which follows the
/// marker; with a span after it, the character goes into the first run of raw text; a run's marker at the start is still a boundary (refused above).
#[test]
fn typing_at_the_start_of_a_paragraph_follows_the_paragraph_s_own_marker() {
    for (body, from, to, expected) in [
        ("<text:p text:style-name=\"P1\">one</text:p>", "one", "Xone", "<text:p text:style-name=\"P1\">Xone</text:p>"),
        (
            "<text:p text:style-name=\"P1\">ab<text:span text:style-name=\"T1\">cd</text:span></text:p>",
            "ab",
            "Xab",
            "<text:p text:style-name=\"P1\">Xab<text:span text:style-name=\"T1\">cd</text:span></text:p>",
        ),
        (
            "<text:p>ab<text:span text:style-name=\"T1\">cd</text:span></text:p>",
            "ab",
            "Xab",
            "<text:p>Xab<text:span text:style-name=\"T1\">cd</text:span></text:p>",
        ),
    ] {
        let mut o = open(&drawing(body, body));
        let x = id_of(&o.doc, "x");
        retype(shape_mut(&mut o.doc, 0, x), from, to);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{body}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains(expected), "{body}: {after}");
        let back = open_bytes(saved.bytes);
        assert_eq!(show(back.doc.pages.get(0).unwrap().shapes.get(0).unwrap()), show(o.doc.pages.get(0).unwrap().shapes.get(0).unwrap()), "{body}");
    }
}

/// A paragraph written as one empty tag is opened and closed by its own name, under the prefix the file binds (here `t` and `text` for one namespace).
#[test]
fn an_empty_paragraph_written_as_one_tag_gets_its_text_between_a_start_and_an_end_tag_of_its_own_name() {
    let mut d = Drawing::a4().auto(STYLES).page(&format!("{}{}", shape("x", "1cm", "<t:p/>"), shape("y", "6cm", "<text:p/>")));
    d.ns.push_str(r#" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#);
    let mut o = open(&d);
    for name in ["x", "y"] {
        let id = id_of(&o.doc, name);
        shape_mut(&mut o.doc, 0, id).text =
            Some(Arc::new(diacraft_doc::Text { parts: vec![TextPart::Chars(Arc::from("hi & <you>"))], ..Default::default() }));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<t:p>hi &amp; &lt;you&gt;</t:p><draw:enhanced-geometry"), "{after}");
    assert!(after.contains("<text:p>hi &amp; &lt;you&gt;</text:p><draw:enhanced-geometry"), "{after}");
    let back = open_bytes(saved.bytes);
    for at in 0..2 {
        assert_eq!(show(back.doc.pages.get(0).unwrap().shapes.get(at).unwrap()), "hi & <you>");
    }
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 2] = [
    ("retext_keeps_lists_and_whitespace_elements", retext_case),
    ("a_span_is_kept_and_only_the_text_after_it_changes", a_span_is_kept_and_only_the_text_after_it_changes),
];

#[test]
fn retext_keeps_lists_and_whitespace_elements() {
    retext_case(&real);
}

#[test]
fn a_span_is_kept_and_only_the_text_after_it_changes_test() {
    a_span_is_kept_and_only_the_text_after_it_changes(&real);
}

#[test]
fn echo_control_fails_every_retext_case() {
    crate::write_core::echo_control(&CASES);
}

#[test]
fn a_retext_the_writer_does_not_write_is_refused_in_words_and_nothing_is_written() {
    // The zero-width elements a paragraph may hold (a bookmark, its start and end, a soft page break) are kept where they stand.
    for element in [
        "<text:soft-page-break/>",
        "<text:bookmark text:name=\"m\"/>",
        "<text:bookmark-start text:name=\"m\"/>",
        "<text:bookmark-end text:name=\"m\"/>",
    ] {
        let body = format!("<text:p text:style-name=\"P1\">ab{element}cd</text:p>");
        let mut o = open(&drawing(&body, &body));
        let x = id_of(&o.doc, "x");
        retype(shape_mut(&mut o.doc, 0, x), "cd", "ce");
        let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{element}: {e}")).bytes, "content.xml");
        assert!(after.contains(&format!("ab{element}ce</text:p>")), "{element}: {after}");
    }
    // A character typed before a zero-width element that stands first (R10 item 3, the prefix first): the paragraph is written whole, and the element stays before
    // the character at its offset, which is now the typed one.
    let bookmarked = "<text:p><text:bookmark text:name=\"m\"/>ab<text:span text:style-name=\"T1\">cd</text:span></text:p>";
    let o = open(&drawing(bookmarked, bookmarked));
    let x = id_of(&o.doc, "x");
    let mut doc = o.doc.clone();
    retype(shape_mut(&mut doc, 0, x), "ab", "Xab");
    let after = part(&real(&doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}")).bytes, "content.xml");
    assert!(after.contains("<text:p><text:bookmark text:name=\"m\"/>Xab<text:span text:style-name=\"T1\">cd</text:span></text:p>"), "{after}");
    // A paragraph holding a link is refused where it is written, in words that name the link (the placing does not cover one).
    let link = "<text:p text:style-name=\"P1\">go <text:a xlink:href=\"http://example.com\">here</text:a></text:p>";
    let o = open(&drawing(link, link));
    let x = id_of(&o.doc, "x");
    let mut doc = o.doc.clone();
    retype(shape_mut(&mut doc, 0, x), "go ", "come ");
    match real(&doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains("link") && what.contains("`text:a` element") && what.contains("Q4"), "{what}"),
        other => panic!("expected a refusal of the link: {other:?}"),
    }
}
