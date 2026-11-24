//! The edges of the retext that mutation testing found unpinned: which
//! edits are spliced into the paragraph's own bytes and which write the paragraph again, which paragraphs a list item lets go or takes, which style a run is
//! given when the file holds several of one look, and which styles of the file may stand for a row.
//!
//! A bookmark in a paragraph tells the two ways of writing apart: an edit spliced in place keeps it where it stands, and a paragraph written again from
//! the model puts it back by the offset it has in the model's text (`write_zero.rs` holds the vectors of R10).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, TextPart, Value};
use diacraft_odg::WriteError;

use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real, shape_mut};
use crate::write_runs::{chars, id_of, row_cell, set_parts, set_row, shape, the_shape};

const STYLES: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"#;

fn drawing(styles: &str, inner: &str) -> Drawing {
    Drawing::a4().auto(styles).page(&shape("x", "1cm", inner))
}

/// The words of the refusal, when the save is one.
fn refusal<T>(result: Result<T, WriteError>) -> String {
    match result {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another error: {other}"),
        Ok(_) => panic!("the save was not refused"),
    }
}

/// The paragraphs of a list item that holds more than one, or of a list header, are not taken out; the list item that holds one only goes with its paragraph.
#[test]
fn a_paragraph_taken_out_of_a_list_item_that_holds_more_or_out_of_a_header_is_refused() {
    let two = r#"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p><text:p>two</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing(STYLES, two));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("one")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("list item that holds more than one paragraph"), "{words}");
    let header = r#"<text:list text:style-name="L1"><text:list-header><text:p>head</text:p></text:list-header><text:list-item><text:p>one</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing(STYLES, header));
    set_parts(&mut o, vec![TextPart::Pp(1), chars("one")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("list header"), "{words}");
    // An item that holds the paragraph alone goes with it.
    let alone = r#"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing(STYLES, alone));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("one")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert_eq!(after.matches("<text:list-item>").count(), 1, "{after}");
    assert!(after.contains("<text:list-item><text:p>one</text:p></text:list-item></text:list>") && !after.contains("two"), "{after}");
}

/// A paragraph put beside one of a list item that holds more than it, or of a header, is refused: a new item of that list would split the item.
#[test]
fn a_paragraph_put_beside_a_list_item_that_holds_more_or_beside_a_header_is_refused() {
    let two = r#"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p><text:p>two</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing(STYLES, two));
    set_parts(&mut o, vec![TextPart::Pp(0), chars("one\ntwo\nthree")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("list item that holds more than one paragraph"), "{words}");
    let header = r#"<text:list text:style-name="L1"><text:list-header><text:p>head</text:p></text:list-header><text:list-item><text:p>one</text:p></text:list-item></text:list>"#;
    let mut o = open(&drawing(STYLES, header));
    set_parts(&mut o, vec![chars("head\nnew\n"), TextPart::Pp(1), chars("one")]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("list header"), "{words}");
}

/// The attributes a `Character` row of `Style` 1 writes (the bold bit, with the others off).
const BOLD: &str = r#"fo:font-weight="bold" fo:font-style="normal" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal" style:text-underline-style="none""#;

/// A text style of the look of the bold row, called `name`.
fn bold_style(name: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties {BOLD}/></style:style>"#)
}

/// Three styles of the one look.
fn three_bold() -> String {
    ["T1", "T2", "T3"].iter().map(|n| bold_style(n)).collect()
}

/// The style a bold run of a paragraph put after the paragraph `neighbour` is given, when the file holds three styles of that look.
fn style_of_the_run_beside(neighbour: &str) -> String {
    let mut o = open(&drawing(&three_bold(), &format!("<text:p>{neighbour}</text:p>")));
    let shape = the_shape(&o.doc);
    let mut parts: Vec<TextPart> = shape.text.as_ref().expect("a text").parts.clone();
    // The row of the bold run: the row of the model whose `Style` is the bold bit (row 0 when the whole paragraph is the run).
    let bold = (0..8u32).find(|ix| row_cell(shape, "Character", *ix, "Style") == Some(1.0)).expect("a bold row");
    parts.extend([chars("\n"), TextPart::Cp(bold), chars("new")]);
    set_parts(&mut o, parts);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let marker = "\">new</text:span>";
    let end = after.find(marker).unwrap_or_else(|| panic!("no span of the new paragraph: {after}"));
    let head = &after[..end];
    head[head.rfind('"').unwrap() + 1..].to_owned()
}

/// A paragraph put beside another is given the style the other's span names for a run of that look; where the span says nothing of it (a run in two spans, a
/// run that is not wholly inside one span), the first style of the part that holds that look and nothing else.
#[test]
fn a_run_put_beside_a_paragraph_takes_the_style_the_spans_of_that_paragraph_name_for_its_look() {
    // One span: its style, and not the first of the same look.
    assert_eq!(style_of_the_run_beside(r#"<text:span text:style-name="T2">bb</text:span>"#), "T2");
    // A counted space inside the one span is part of the run.
    assert_eq!(style_of_the_run_beside(r#"<text:span text:style-name="T2">b<text:s/>b</text:span>"#), "T2");
    // The plain text that follows the span is no part of the run.
    assert_eq!(style_of_the_run_beside(r#"<text:span text:style-name="T2">bb</text:span>cc"#), "T2");
    assert_eq!(style_of_the_run_beside(r#"cc<text:span text:style-name="T2">bb</text:span>"#), "T2");
    // Two spans, one inside the other: no one style is the run's.
    assert_eq!(style_of_the_run_beside(r#"<text:span text:style-name="T2"><text:span text:style-name="T3">bb</text:span></text:span>"#), "T1");
    assert_eq!(
        style_of_the_run_beside(r#"<text:span text:style-name="T2">b<text:span text:style-name="T2"><text:s/></text:span>b</text:span>"#),
        "T1"
    );
}

/// A save of the model whose first paragraph has `inner` as its content and whose text is made `parts`; the part saved.
fn saved_paragraph(inner: &str, parts: Vec<TextPart>) -> Result<String, WriteError> {
    let mut o = open(&drawing(STYLES, &format!("<text:p>{inner}</text:p>")));
    set_parts(&mut o, parts);
    real(&o.doc, &o.bytes, &o.index).map(|w| part(&w.bytes, "content.xml"))
}

const MARKED: &str = r#"ab<text:span text:style-name="T1">cd</text:span><text:bookmark text:name="m"/>ef"#;

/// An edit that lies inside one raw text of the paragraph is written in place, the bookmark and the span kept; the first of them, a character typed at the very start of
/// the paragraph, is one the writer maps one to one when the paragraph starts with text and has other things after it.
#[test]
fn an_edit_inside_one_run_of_raw_text_is_written_in_place_and_keeps_the_bookmark_and_the_span() {
    let rest = [TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")];
    let with = |first: &str| -> Vec<TextPart> { std::iter::once(chars(first)).chain(rest.iter().cloned()).collect() };
    let span = r#"<text:span text:style-name="T1">cd</text:span><text:bookmark text:name="m"/>ef"#;
    for (typed, written) in [("Zab", "Zab"), ("aXb", "aXb"), ("aX", "aX")] {
        let after = saved_paragraph(MARKED, with(typed)).unwrap_or_else(|e| panic!("{typed}: {e}"));
        assert!(after.contains(&format!("<text:p>{written}{span}</text:p>")), "{typed}: {after}");
    }
    // At the boundary of the span the character joins the text before it, so the paragraph is written again whole (R10 item 3): the bookmark was after `cd`, at 4
    // of `abcdef`; the prefix is `ab`, the suffix `cdef`, and it moves by the one character typed, to 5: between the bold run and `ef`.
    let after = saved_paragraph(MARKED, with("abY")).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>abY<text:span text:style-name="T1">cd</text:span><text:bookmark text:name="m"/>ef</text:p>"#), "{after}");
    // After the span's text, in the raw text that follows the bookmark.
    let parts = vec![chars("ab"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("eXf")];
    let after = saved_paragraph(MARKED, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:bookmark text:name="m"/>eXf</text:p>"#), "{after}");
}

/// An edit spliced into a text that holds a space (not at its edges) is still in place: the bookmark that follows stays where it stands, after the typed character
/// (R10 item 2; the paragraph written whole would put it before the character: `ab cd` then `X` at the end, the mark at the end, a place the prefix keeps).
#[test]
fn an_edit_in_a_text_with_a_space_in_it_is_written_in_place_and_keeps_the_bookmark_after_it() {
    let after = saved_paragraph(r#"ab cd<text:bookmark text:name="m"/>"#, vec![chars("ab cdX")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>ab cdX<text:bookmark text:name="m"/></text:p>"#), "{after}");
}

/// An edit in the second text of a paragraph is spliced there and not in the first: with two bookmarks the typed character at the end of the later text goes
/// before the bookmark that follows it (the paragraph written whole would put that bookmark first), and one in the first text leaves the second text alone.
#[test]
fn an_edit_in_a_later_text_of_the_paragraph_is_written_in_that_text() {
    let marked = r#"ab<text:bookmark text:name="m"/>cd<text:bookmark text:name="n"/>"#;
    let after = saved_paragraph(marked, vec![chars("abcdX")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>ab<text:bookmark text:name="m"/>cdX<text:bookmark text:name="n"/></text:p>"#), "{after}");
    let after = saved_paragraph(marked, vec![chars("aXbcd")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>aXb<text:bookmark text:name="m"/>cd<text:bookmark text:name="n"/></text:p>"#), "{after}");
    // A character replaced in the later text, and one taken out of it.
    let after = saved_paragraph(marked, vec![chars("abcX")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"cX<text:bookmark text:name="n"/></text:p>"#), "{after}");
    let after = saved_paragraph(marked, vec![chars("abc")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:bookmark text:name="m"/>c<text:bookmark text:name="n"/></text:p>"#), "{after}");
}

/// A character typed where it repeats the ones beside it is the edit of the characters between the common head and the common tail, and no more of the tail than is left.
#[test]
fn a_character_typed_beside_the_same_character_is_one_insertion() {
    let marked = r#"aa<text:bookmark text:name="m"/>b"#;
    let after = saved_paragraph(marked, vec![chars("aaab")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>aaa<text:bookmark text:name="m"/>b</text:p>"#), "{after}");
    // A character taken out of a repeat.
    let after = saved_paragraph(r#"aaa<text:bookmark text:name="m"/>b"#, vec![chars("aab")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>aa<text:bookmark text:name="m"/>b</text:p>"#), "{after}");
}

/// A counted space before the edit is counted in the characters that come before it.
#[test]
fn an_edit_after_a_counted_space_lands_in_the_text_after_it() {
    let marked = r#"a<text:s/>b<text:bookmark text:name="m"/>c"#;
    let after = saved_paragraph(marked, vec![chars("a Xc")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>a<text:s/>X<text:bookmark text:name="m"/>c</text:p>"#), "{after}");
}

/// A text that begins or ends with a space is not mapped one to one where another node abuts it: written again whole, and the bookmark is put back at the offset the
/// model's text gives it (R10 item 3), with the space after it a plain space, since a plain one is read back as one.
#[test]
fn an_edit_in_a_text_that_ends_with_a_space_beside_a_bookmark_is_written_again_and_the_bookmark_is_put_back() {
    // `a b` with the bookmark after the space (offset 2); `X` typed after the `a`: the suffix ` b` holds the bookmark, which moves to 3.
    let marked = r#"a <text:bookmark text:name="m"/>b"#;
    let after = saved_paragraph(marked, vec![chars("aX b")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>aX <text:bookmark text:name="m"/>b</text:p>"#), "{after}");
    // The bookmark before the space (offset 1): the prefix `a ` holds it, and it stays at 1, before the space.
    let marked = r#"a<text:bookmark text:name="m"/> b"#;
    let after = saved_paragraph(marked, vec![chars("a Xb")]).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(r#"<text:p>a<text:bookmark text:name="m"/> Xb</text:p>"#), "{after}");
}

/// The text style of a paragraph put in a file that holds `styles` for a bold run of a row of the model with no span of the file's to take it from.
fn style_of_a_new_bold_run(styles: &str) -> String {
    let mut o = open(&drawing(styles, "<text:p>ab</text:p>"));
    set_row(&mut o, "Character", 1, &[("Style", 1.0)]);
    set_parts(&mut o, vec![chars("a"), TextPart::Cp(1), chars("b")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let at = after.find("<text:span text:style-name=\"").unwrap_or_else(|| panic!("no span: {after}")) + "<text:span text:style-name=\"".len();
    after[at..at + after[at..].find('"').unwrap()].to_owned()
}

/// A style of the file stands for a row only when it holds exactly the row's attributes: no parent, one properties element of the kind and nothing else beside it.
#[test]
fn a_style_of_the_file_stands_for_a_row_only_when_it_holds_exactly_its_attributes() {
    assert_eq!(style_of_a_new_bold_run(&bold_style("T7")), "T7", "the clean style is used");
    let style = |attributes: &str, inside: &str, before: &str, after: &str| {
        format!(r#"<style:style style:name="T7" style:family="text"{attributes}>{before}<style:text-properties {BOLD}{inside}{after}</style:style>"#)
    };
    for (what, decoy) in [
        ("a child in the properties", style("", "><style:x/></style:text-properties>", "", "")),
        ("another properties element", style("", "/>", "", r#"<style:paragraph-properties fo:text-align="center"/>"#)),
        ("an element before the properties", style("", "/>", r#"<style:map style:condition="x" style:apply-style-name="T1"/>"#, "")),
        ("an element after the properties", style("", "/>", "", r#"<style:map style:condition="x" style:apply-style-name="T1"/>"#)),
        ("a parent", style(r#" style:parent-style-name="T1""#, "/>", "", "")),
        ("another attribute of the properties", style("", r#" fo:letter-spacing="1pt"/>"#, "", "")),
        ("another attribute of the style", style(r#" style:display-name="Seven""#, "/>", "", "")),
    ] {
        assert_eq!(style_of_a_new_bold_run(&decoy), "T8", "{what}: not used, and the new one is named after the largest number: {decoy}");
    }
    // A namespace declaration is no attribute of the style, on the style or on its properties.
    let declared = style(r#" xmlns:foo="urn:x-foo""#, "/>", "", "");
    assert_eq!(style_of_a_new_bold_run(&declared), "T7", "a declaration on the style");
    let declared = style("", r#" xmlns:foo="urn:x-foo"/>"#, "", "");
    assert_eq!(style_of_a_new_bold_run(&declared), "T7", "a declaration on the properties");
}

/// A face the model names with quotes round it (as a style sheet writes a name with a space) is the face the part declares without them.
#[test]
fn a_font_the_model_names_with_quotes_is_the_declared_face() {
    let mut d = Drawing::a4().page(&shape("x", "1cm", "<text:p>plain</text:p>"));
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="DejaVu Sans Mono"/>"#.to_owned();
    for named in ["'DejaVu Sans Mono'", "\"DejaVu Sans Mono\"", "  'DejaVu Sans Mono' "] {
        let mut o = open(&d);
        let id = id_of(&o.doc);
        {
            let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
            let mut section = Section::new("Character", None);
            let mut row = Row { ix: Some(1), ..Row::default() };
            row.cells.insert(CellName::new("Font"), Cell::constant(Value::from_text(named)));
            section.rows.push(Arc::new(row));
            sheet.sections.push(Arc::new(section));
        }
        set_parts(&mut o, vec![TextPart::Cp(1), chars("face")]);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{named}: {e}"));
        assert!(part(&saved.bytes, "content.xml").contains("style:font-name=\"Mono\""), "{named}");
        let _ = open_bytes(saved.bytes);
    }
}

/// A text style of the look of the italic row (`Style` 2), called `name`.
fn italic_style(name: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="text"><style:text-properties fo:font-weight="normal" fo:font-style="italic" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal" style:text-underline-style="none"/></style:style>"#
    )
}

/// The style the italic run of a paragraph put after `neighbour` is given, the file holding the styles `styles`.
fn italic_run_beside(styles: &str, neighbour: &str) -> String {
    let mut o = open(&drawing(styles, &format!("<text:p>{neighbour}</text:p>")));
    let shape = the_shape(&o.doc);
    let mut parts: Vec<TextPart> = shape.text.as_ref().expect("a text").parts.clone();
    let italic = (0..8u32).find(|ix| row_cell(shape, "Character", *ix, "Style") == Some(2.0)).expect("an italic row");
    parts.extend([chars("\n"), TextPart::Cp(italic), chars("new")]);
    set_parts(&mut o, parts);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let end = after.find("\">new</text:span>").unwrap_or_else(|| panic!("no span of the new paragraph: {after}"));
    let head = &after[..end];
    head[head.rfind('"').unwrap() + 1..].to_owned()
}

/// A span that holds white space the reader collapses is not one whose characters the tokens count as the runs do: the paragraph's runs are not matched with its
/// spans, and an italic run is not given the style of the bold span that the offsets of the spans would put it in (the review).
#[test]
fn a_paragraph_whose_white_space_collapses_names_no_style_for_a_run_of_another_look() {
    let styles = format!("{}{}", bold_style("T1"), italic_style("T2"));
    // `a  b` (two spaces, read as three characters) in a bold span, then an italic span of one character.
    let style = italic_run_beside(&styles, r#"<text:span text:style-name="T1">a  b</text:span><text:span text:style-name="T2">c</text:span>"#);
    assert_eq!(
        style, "T3",
        "the italic run is given a style of its own look (the row holds the size too), and not the bold one it lies over in the offsets of the spans"
    );
    // The same where the spans do not collapse anything: the span's own style.
    let style = italic_run_beside(&styles, r#"<text:span text:style-name="T1">a b</text:span><text:span text:style-name="T2">c</text:span>"#);
    assert_eq!(style, "T2");
}

/// A style the neighbour's span names that is a common style, and not an automatic one the part can count references to, is not the style a run is given: the run
/// is given an automatic style of its row's attributes (the review).
#[test]
fn a_run_is_not_given_a_common_style_a_span_beside_it_names() {
    let common = italic_style("Emphasis");
    let mut d = Drawing::a4().common(&common).page(&shape("x", "1cm", r#"<text:p><text:span text:style-name="Emphasis">ab</text:span></text:p>"#));
    d.auto.clear();
    let mut o = open(&d);
    let shape = the_shape(&o.doc);
    let mut parts: Vec<TextPart> = shape.text.as_ref().expect("a text").parts.clone();
    let italic = (0..8u32).find(|ix| row_cell(shape, "Character", *ix, "Style") == Some(2.0)).expect("an italic row");
    parts.extend([chars("\n"), TextPart::Cp(italic), chars("new")]);
    set_parts(&mut o, parts);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<text:span text:style-name=\"T1\">new</text:span>"), "an automatic style, named after the file's numbers: {after}");
    assert!(after.contains("<style:style style:name=\"T1\" style:family=\"text\">"), "{after}");
}

/// A format set where no character is (an empty shape: the row has no run to be written on) is refused in words, and the rows of a text that are only numbered anew
/// are no change (the review).
#[test]
fn a_format_on_a_shape_with_no_character_is_refused_and_rows_numbered_anew_are_no_edit() {
    let mut o = open(&drawing(STYLES, "<text:p/>"));
    set_row(&mut o, "Character", 0, &[("Style", 1.0), ("Size", 0.25)]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("no character"), "{words}");
    let mut o = open(&drawing(STYLES, "<text:p/>"));
    set_row(&mut o, "Paragraph", 0, &[("HorzAlign", 2.0)]);
    assert!(refusal(real(&o.doc, &o.bytes, &o.index)).contains("no character"));
    // A text with a bold span: its row is renumbered (1 to 5) with the marker that names it, and nothing differs but the number.
    let mut o = open(&drawing(STYLES, r#"<text:p>ab<text:span text:style-name="T1">cd</text:span>ef</text:p>"#));
    let id = id_of(&o.doc);
    {
        let shape = shape_mut(&mut o.doc, 0, id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        for section in sheet.sections.iter_mut().filter(|s| s.name.as_str() == "Character") {
            for row in Arc::make_mut(section).rows.iter_mut().filter(|r| r.ix == Some(1)) {
                Arc::make_mut(row).ix = Some(5);
            }
        }
        let parts: Vec<TextPart> =
            shape.text.as_ref().unwrap().parts.iter().map(|p| if *p == TextPart::Cp(1) { TextPart::Cp(5) } else { p.clone() }).collect();
        shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!saved.edited, "the rows were numbered anew and are the same: {}", part(&saved.bytes, "content.xml"));
}

const WITH_ID: &str = r#"ab<text:span text:style-name="T1" xml:id="s1" text:class-names="c">cd</text:span>ef"#;

/// A paragraph written again keeps the `text:span` of each run that is unchanged, with the `xml:id` and the other attributes the span holds (ownership R8: an
/// unchanged run keeps its span; the review, LOW): in the head the two paragraphs have in common, in the tail, and not a run whose text changed.
#[test]
fn a_paragraph_written_again_keeps_the_span_of_each_run_that_is_unchanged() {
    let kept = r#"<text:span text:style-name="T1" xml:id="s1" text:class-names="c">cd</text:span>"#;
    // A character typed at the boundary before the span: the span is in the tail.
    let parts = vec![chars("abQ"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")];
    let after = saved_paragraph(WITH_ID, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(&format!("<text:p>abQ{kept}ef</text:p>")), "{after}");
    // A character typed at the boundary after it: the span is in the head.
    let parts = vec![chars("ab"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("Zef")];
    let after = saved_paragraph(WITH_ID, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(&format!("<text:p>ab{kept}Zef</text:p>")), "{after}");
    // The span's text changed as well: that run is written anew, from the model, and has none of the span's attributes.
    let parts = vec![chars("abQ"), TextPart::Cp(1), chars("cX"), TextPart::Cp(0), chars("ef")];
    let after = saved_paragraph(WITH_ID, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains("<text:p>abQ<text:span text:style-name=\"T1\">cX</text:span>ef</text:p>") && !after.contains("xml:id"), "{after}");
    // Saved again, it is no edit; and the style the span names is still named, so it is still there.
    let mut o = open(&drawing(STYLES, &format!("<text:p>{WITH_ID}</text:p>")));
    set_parts(&mut o, vec![chars("abQ"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")]);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(part(&first.bytes, "content.xml").contains("style:name=\"T1\""));
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited && again.bytes == first.bytes);
}

/// What a span needs to be kept (the mutation of `kept_runs` and `plain_spans`): the tail is shifted by all that was added, a run whose text changed is not kept
/// when the characters after it are the tail, a run whose row changed is not kept when its text is the same, and a span that holds another is no plain span.
#[test]
fn a_span_is_kept_for_a_run_with_its_text_in_the_tail_and_its_row_and_for_no_other() {
    let kept = r#"<text:span text:style-name="T1" xml:id="s1" text:class-names="c">cd</text:span>"#;
    // Three characters typed before the span: the tail is shifted by three.
    let parts = vec![chars("abQQQ"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")];
    let after = saved_paragraph(WITH_ID, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains(&format!("<text:p>abQQQ{kept}ef</text:p>")), "{after}");
    // The first character of the span changed, as one before it did (two places, so the paragraph is written again) and the rest of it is the tail: the span's
    // run is written anew.
    let parts = vec![chars("aQ"), TextPart::Cp(1), chars("Xd"), TextPart::Cp(0), chars("ef")];
    let after = saved_paragraph(WITH_ID, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(after.contains("<text:p>aQ<text:span text:style-name=\"T1\">Xd</text:span>ef</text:p>") && !after.contains("xml:id"), "{after}");
    // The characters are the same and the run has another row (italic, not bold): the span is written anew and has none of the old span's attributes, and the style
    // it names, which nothing else names, says the new row: it is edited where it stands (no style is renamed).
    let mut o = open(&drawing(STYLES, &format!("<text:p>{WITH_ID}</text:p>")));
    set_row(&mut o, "Character", 5, &[("Style", 2.0)]);
    set_parts(&mut o, vec![chars("aQb"), TextPart::Cp(5), chars("cd"), TextPart::Cp(0), chars("ef")]);
    let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}")).bytes, "content.xml");
    assert!(!after.contains("xml:id"), "{after}");
    assert!(after.contains("<text:p>aQb<text:span text:style-name=\"T1\">cd</text:span>ef</text:p>"), "{after}");
    assert!(after.contains(r#"fo:font-style="italic""#) && after.contains(r#"fo:font-weight="normal""#) && !after.contains("bold"), "{after}");
    // A span inside a span: neither is a plain span, and neither is kept (the inner one's id is not written again).
    let nested = r#"ab<text:span text:style-name="T1"><text:span text:style-name="T1" xml:id="in">cd</text:span></text:span>ef"#;
    let parts = vec![chars("abQ"), TextPart::Cp(1), chars("cd"), TextPart::Cp(0), chars("ef")];
    let after = saved_paragraph(nested, parts).unwrap_or_else(|e| panic!("{e}"));
    assert!(!after.contains("xml:id"), "{after}");
}

/// A format on a paragraph with no character is lost whatever else the edit is (the re-review, parked 7, probe EMPTY_PARA_BOLD): a bold
/// row held by an empty paragraph the edit adds, or left at an empty last paragraph when the characters that used it are deleted. It is refused in words, as the
/// same row is when nothing else changes. A row that no marker holds in force, or that holds only a stretch of no character amid characters (the engine leaves
/// one when a formatted run is deleted), is no format a person sees and is saved; a row that a character of the text uses is saved.
#[test]
fn a_format_on_an_empty_paragraph_is_refused_beside_another_edit_of_the_text() {
    let bold = |o: &mut crate::write_core::Open| set_row(o, "Character", 5, &[("Style", 1.0), ("Size", 0.25)]);
    // The row held by an empty paragraph that the edit adds (a marker and no character).
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![chars("abc\n"), TextPart::Cp(5)]);
    let words = refusal(real(&o.doc, &o.bytes, &o.index));
    assert!(words.contains("no character"), "{words}");
    // The same with a character typed in the paragraph before it: a retype and an empty paragraph.
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![chars("abcd\n"), TextPart::Cp(5)]);
    assert!(refusal(real(&o.doc, &o.bytes, &o.index)).contains("no character"));
    // The row held by an empty paragraph in the middle of the text, carried on from the one before (a marker holds until the next marker, across paragraphs).
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![chars("abc\n"), TextPart::Cp(5), chars("\n"), TextPart::Cp(0), chars("def")]);
    assert!(refusal(real(&o.doc, &o.bytes, &o.index)).contains("no character"));
    // A row the text has and no marker holds, beside a typed character, and a paragraph row that no paragraph uses: no format is seen, and the save is made.
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_row(&mut o, "Paragraph", 5, &[("HorzAlign", 2.0)]);
    set_parts(&mut o, vec![chars("abcd")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "saved again it is no edit");
    // A stretch of no character amid characters (a run whose characters were deleted, its marker left): saved.
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![chars("a"), TextPart::Cp(5), TextPart::Cp(0), chars("bcd")]);
    assert!(real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}")).edited);
    // The same row, used by a character of the text: saved, and saved again as no edit.
    let mut o = open(&drawing(STYLES, "<text:p>abc</text:p>"));
    bold(&mut o);
    set_parts(&mut o, vec![chars("abc"), TextPart::Cp(5), chars("d")]);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(first.edited);
    let again = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == first.bytes);
    // A row that the text had before (the file's own bold row) and that a later edit leaves unused is no new row, and is not refused.
    let mut o = open(&drawing(STYLES, r#"<text:p>ab<text:span text:style-name="T1">cd</text:span>ef</text:p>"#));
    set_parts(&mut o, vec![chars("abef")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
}
