//! The language of a run and of a drawing in an `.odg`, and right-to-left alignment:
//! a run's `LangID` is read from the triplet of the script type most of its letters belong to and written into that
//! triplet when it changes, `zxx` and `none` are no language, `dc:language` is the drawing's language, and `start` and `end` follow the writing direction of a paragraph.
//!
//! Every input is written by hand from the OASIS ODF 1.4 standard (Part 3 §20.188, §20.195, §20.202, §20.223, §20.343 to §20.358) and the vectors; no file made by another program is read.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, Shape, TextPart, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::{Drawing, meta_with, row_cell, text};
use crate::model_text::{chars, doc_with, row, section};
use crate::write_core::{Open, open, open_bytes, part, real, shape_mut};
use crate::write_runs::{id_of, set_parts, set_row, shape, the_shape};

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ helpers

/// A text style `name` whose text properties are `props`.
fn text_style(name: &str, props: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties {props}/></style:style>"#)
}

fn span(style: &str, inner: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{inner}</text:span>"#)
}

/// The shape `x` holding one paragraph with `inner`.
fn labelled(inner: &str) -> String {
    shape("x", "1cm", &format!("<text:p>{inner}</text:p>"))
}

/// The `LangID` the row `ix` of the shape's `Character` section holds, as text.
fn lang_of_row(shape: &Shape, ix: u32) -> Option<String> {
    row_cell(shape, "Character", None, ix, "LangID").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// What each run of the shape's text is in: its characters and the `LangID` of the row in force (the first row before any marker).
fn langs(shape: &Shape) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut row = 0u32;
    for p in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match p {
            TextPart::Cp(ix) => row = *ix,
            TextPart::Chars(c) => out.push((c.to_string(), lang_of_row(shape, row))),
            _ => {}
        }
    }
    out
}

/// The `LangID` of the first run of a one-run text whose span has the text properties `props`.
fn read_lang(props: &str, run: &str) -> Option<String> {
    let o = open(&Drawing::a4().auto(&text_style("T1", props)).page(&labelled(&span("T1", run))));
    langs(the_shape(&o.doc)).into_iter().next().and_then(|(_, lang)| lang)
}

/// The row of the last `pp` marker of the shape's text (0 when it has none): the paragraph row of a text of one paragraph.
fn last_pp(shape: &Shape) -> u32 {
    shape.text.iter().flat_map(|t| t.parts.iter()).filter_map(|p| if let TextPart::Pp(ix) = p { Some(*ix) } else { None }).next_back().unwrap_or(0)
}

/// The rows of the shape's text that a run is on, in order, one for each run: the `Cp` markers.
fn markers(shape: &Shape) -> Vec<u32> {
    shape.text.iter().flat_map(|t| t.parts.iter()).filter_map(|p| if let TextPart::Cp(ix) = p { Some(*ix) } else { None }).collect()
}

/// Gives the cell `name` of row `ix` of the shape's `section` the text `value`, keeping the row's other cells.
fn set_text(o: &mut Open, section: &str, ix: u32, name: &str, value: &str) {
    let id = id_of(&o.doc);
    let sheet = Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet);
    let at = sheet.sections.iter().position(|s| s.name.as_str() == section).unwrap_or_else(|| panic!("no section {section}"));
    let sec = Arc::make_mut(sheet.sections.get_mut(at).unwrap());
    let at = sec.rows.iter().position(|r| r.ix == Some(ix)).unwrap_or_else(|| panic!("no row {ix} in {section}"));
    let mut row = Row::clone(&sec.rows[at]);
    row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    sec.rows[at] = Arc::new(row);
}

const LATIN: &str = r#"fo:language="en" fo:country="US""#;
const ASIAN: &str = r#"style:language-asian="ja" style:country-asian="JP""#;
const COMPLEX: &str = r#"style:language-complex="ar" style:country-complex="EG""#;
/// U+65E5 U+672C, then U+8A9E: Japanese letters, of the Asian script type.
const NIHON: &str = "\u{65E5}\u{672C}";
const NIHONGO: &str = "\u{65E5}\u{672C}\u{8A9E}";
/// U+0639: an Arabic letter, of the complex script type.
const AIN: &str = "\u{0639}";

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the reader

/// The triplet of the Latin type is the language of Latin text.
#[test]
fn a_latin_run_is_in_the_language_of_the_latin_triplet() {
    assert_eq!(read_lang(r#"fo:language="de" fo:country="CH""#, "Hallo").as_deref(), Some("de-CH"));
    assert_eq!(read_lang(r#"fo:language="fr""#, "Salut").as_deref(), Some("fr"), "a language alone is a tag");
    assert_eq!(read_lang(r#"fo:language="DE" fo:country="ch""#, "Hallo").as_deref(), Some("de-CH"), "in canonical case");
    assert_eq!(read_lang(r#"fo:language="sr" fo:script="Latn" fo:country="RS""#, "Zdravo").as_deref(), Some("sr-Latn-RS"));
}

/// Text of the Asian type is in the language of the Asian triplet; the type most of the letters belong to decides; a tie goes to the Latin type, then the Asian one
/// A run with no letter is read through the Latin triplet.
#[test]
fn the_script_type_most_of_a_run_s_letters_belong_to_picks_the_triplet() {
    let props = format!("{LATIN} {ASIAN} {COMPLEX}");
    // One Japanese letter (U+65E5) among Arabic ones, and a tie of Asian and complex letters.
    let ri = "\u{65E5}";
    let cases: [(&str, &str); 9] = [
        (NIHON, "ja-JP"),
        (&format!("ab{NIHONGO}"), "ja-JP"),
        (&format!("ab{NIHON}"), "en-US"),
        ("123", "en-US"),
        (&format!("{AIN}{AIN}{AIN}{ri}"), "ar-EG"),
        (&format!("{ri}{AIN}{AIN}"), "ar-EG"),
        (&format!("{NIHON}{AIN}"), "ja-JP"),
        (&format!("{ri}{AIN}"), "ja-JP"),
        (&format!("a{ri}{AIN}"), "en-US"),
    ];
    for (run, expected) in cases {
        assert_eq!(read_lang(&props, run).as_deref(), Some(expected), "{run:?}");
    }
    // No Latin triplet and no letters: no language is read, and the run keeps what it inherits.
    assert_eq!(read_lang(ASIAN, "123"), None);
    assert_eq!(read_lang(ASIAN, "abc"), None);
}

/// `zxx` and `none` are no language, in any of the three triplets.
#[test]
fn zxx_and_none_read_as_no_language() {
    assert_eq!(read_lang(r#"fo:language="zxx" fo:country="none""#, "text").as_deref(), Some("x-none"));
    assert_eq!(read_lang(r#"fo:language="none""#, "text").as_deref(), Some("x-none"));
    assert_eq!(read_lang(r#"fo:language="ZXX""#, "text").as_deref(), Some("x-none"));
    let props = format!(r#"{LATIN} style:language-asian="none""#);
    assert_eq!(read_lang(&props, NIHON).as_deref(), Some("x-none"), "Asian text, Asian triplet none");
    assert_eq!(read_lang(&props, "text").as_deref(), Some("en-US"), "Latin text is still in English");
    let complex = r#"style:language-complex="zxx" style:country-complex="none""#;
    assert_eq!(read_lang(complex, AIN).as_deref(), Some("x-none"));
}

/// A tag the three attributes cannot say is `style:rfc-language-tag`, and it is the language when it is there.
#[test]
fn an_rfc_language_tag_is_the_run_s_language() {
    let props = r#"fo:language="de" fo:country="CH" style:rfc-language-tag="de-CH-1996""#;
    assert_eq!(read_lang(props, "Hallo").as_deref(), Some("de-CH-1996"));
    let asian = r#"style:language-asian="zh" style:country-asian="TW" style:rfc-language-tag-asian="zh-Hant-TW""#;
    assert_eq!(read_lang(asian, NIHON).as_deref(), Some("zh-Hant-TW"));
    // A tag that is no tag: no language is read.
    assert_eq!(read_lang(r#"fo:language="d3 4""#, "text"), None);
    assert_eq!(read_lang(r#"fo:country="US""#, "text"), None, "a country alone");
}

/// The language of a paragraph style reaches the paragraph's own text, and the default styles' reaches the root.
#[test]
fn the_languages_of_a_paragraph_style_and_of_the_default_style_are_read() {
    let styles = r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties fo:language="fr" fo:country="FR"/></style:style>"#;
    let o = open(&Drawing::a4().auto(styles).page(&shape("x", "1cm", r#"<text:p text:style-name="P1">Salut</text:p>"#)));
    assert_eq!(langs(the_shape(&o.doc)), [("Salut".to_owned(), Some("fr-FR".to_owned()))]);
    let defaults = r#"<style:default-style style:family="graphic"><style:text-properties fo:language="es" fo:country="ES"/></style:default-style>"#;
    let o = open(&Drawing::a4().common(defaults).page(&labelled("Hola")));
    let root = o.doc.styles.iter().find(|s| s.id == 0).expect("the root style");
    let cell = diacraft_doc::CellAddr::in_row("Character", None, diacraft_doc::RowKey::Ix(0), "LangID");
    assert_eq!(text(crate::drawing::cell_at(&root.sheet, &cell)), "es-ES", "the root's language is the default style's");
    assert_eq!(langs(the_shape(&o.doc)), [("Hola".to_owned(), None)], "the run inherits it: no cell of its own");
}

/// `dc:language` is the drawing's language, read into the core language and into `DocLangID`.
#[test]
fn dc_language_is_the_core_language_and_the_document_s_language() {
    let mut d = Drawing::a4().page("");
    d.meta = Some(meta_with("Gen/1", "<dc:language>de-DE</dc:language>"));
    let (doc, _) = d.read();
    assert_eq!(doc.props.language.as_deref(), Some("de-DE"));
    assert_eq!(text(doc.sheet.cell("DocLangID")), "de-DE");
    assert_eq!(diacraft_doc::lang::document_language(&doc, None).as_str(), "de-DE");
    // A value that is no language tag stays the core language as written and is no `DocLangID`.
    let mut d = Drawing::a4().page("");
    d.meta = Some(meta_with("Gen/1", "<dc:language>de_DE</dc:language>"));
    let (doc, _) = d.read();
    assert_eq!(doc.props.language.as_deref(), Some("de_DE"));
    assert!(doc.sheet.cell("DocLangID").is_none());
}

/// V77, the reader's half: `start` and `end` are 0 and 2 in every writing mode; `left` and `right` are read against the writing mode.
#[test]
fn start_and_end_follow_the_writing_direction_and_left_and_right_are_read_against_it() {
    let read_align = |mode: &str, align: &str| {
        let style = format!(
            r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="{align}" style:writing-mode="{mode}"/></style:style>"#
        );
        let o = open(&Drawing::a4().auto(&style).page(&shape("x", "1cm", r#"<text:p text:style-name="P1">x</text:p>"#)));
        let s = the_shape(&o.doc);
        let cell = |name: &str| row_cell(s, "Paragraph", None, 0, name).and_then(|c| c.value.as_ref()).and_then(Value::number);
        (cell("HorzAlign"), cell("Flags"))
    };
    for (mode, align, expected) in [
        ("rl-tb", "start", 0.0),
        ("rl-tb", "end", 2.0),
        ("rl-tb", "left", 2.0),
        ("rl-tb", "right", 0.0),
        ("rl-tb", "center", 1.0),
        ("lr-tb", "start", 0.0),
        ("lr-tb", "end", 2.0),
        ("lr-tb", "left", 0.0),
        ("lr-tb", "right", 2.0),
    ] {
        let flags = if mode == "rl-tb" { 1.0 } else { 0.0 };
        assert_eq!(read_align(mode, align), (Some(expected), Some(flags)), "{mode} {align}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the splice writer

/// A custom shape of two runs: German (Switzerland) text, with the Asian and the complex triplets as well, and a bold one.
fn two_runs() -> Drawing {
    let t1 = text_style(
        "T1",
        r#"fo:language="de" fo:country="CH" style:language-asian="ja" style:country-asian="JP" style:language-complex="hi" style:country-complex="IN""#,
    );
    let t2 = r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;
    Drawing::a4().auto(&t1).auto(t2).page(&labelled(&format!("{}{}", span("T1", "Hallo"), span("T2", "x"))))
}

const T1_AS_READ: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:language="de" fo:country="CH" style:language-asian="ja" style:country-asian="JP" style:language-complex="hi" style:country-complex="IN"/></style:style>"#;

/// An unedited run's attributes are as read, when another run of the shape is edited.
#[test]
fn a_run_whose_language_is_unchanged_keeps_its_attributes() {
    let mut o = open(&two_runs());
    let s = the_shape(&o.doc);
    assert_eq!(langs(s).first().and_then(|(_, l)| l.as_deref()), Some("de-CH"));
    let rows = markers(s);
    assert_eq!(rows.len(), 2, "two runs, each opened by its marker: {rows:?}");
    set_row(&mut o, "Character", rows[1], &[("Style", 1.0), ("Size", 20.0 / 72.0)]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(T1_AS_READ), "T1 as read: {after}");
    assert!(after.contains("fo:font-size=\"20pt\""), "{after}");
}

/// Saves the first run of [`two_runs`] with its `LangID` set to `tag`, and the content part made and the model read back from it.
fn save_with_lang(drawing: &Drawing, tag: &str) -> (String, Document) {
    let mut o = open(drawing);
    let rows = markers(the_shape(&o.doc));
    assert!(!rows.is_empty(), "the run has a row of its own: {:?}", the_shape(&o.doc).text);
    set_text(&mut o, "Character", rows[0], "LangID", tag);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{tag}: {e}"));
    (part(&saved.bytes, "content.xml"), open_bytes(saved.bytes).doc)
}

/// A changed `LangID` is written into the triplet of the run's script type, and the other two triplets stay as read.
#[test]
fn a_changed_language_is_written_into_the_latin_triplet() {
    let (after, back) = save_with_lang(&two_runs(), "fr-FR");
    for kept in [r#"style:language-asian="ja""#, r#"style:country-asian="JP""#, r#"style:language-complex="hi""#, r#"style:country-complex="IN""#] {
        assert!(after.contains(kept), "{kept} as read: {after}");
    }
    assert!(after.contains(r#"fo:language="fr""#) && after.contains(r#"fo:country="FR""#), "{after}");
    assert!(!after.contains(r#"fo:language="de""#) && !after.contains(r#"fo:country="CH""#), "{after}");
    assert_eq!(langs(the_shape(&back)).first().and_then(|(_, l)| l.as_deref()), Some("fr-FR"));
}

#[test]
fn a_script_subtag_is_written_as_fo_script_and_any_other_tag_as_an_rfc_tag() {
    let (after, back) = save_with_lang(&two_runs(), "sr-Latn-RS");
    assert!(after.contains(r#"fo:language="sr""#) && after.contains(r#"fo:script="Latn""#) && after.contains(r#"fo:country="RS""#), "{after}");
    assert!(!after.contains("rfc-language-tag"), "no rfc tag for a tag the three say: {after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("sr-Latn-RS"));
    // The nearest triplet and the tag.
    let (after, back) = save_with_lang(&two_runs(), "de-CH-1996");
    assert!(after.contains(r#"fo:language="de""#) && after.contains(r#"fo:country="CH""#), "{after}");
    assert!(after.contains(r#"style:rfc-language-tag="de-CH-1996""#), "{after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("de-CH-1996"));
    // And a tag that needs none after one that did: the rfc tag goes.
    let again = Drawing::a4()
        .auto(&text_style("T1", r#"fo:language="de" fo:country="CH" style:rfc-language-tag="de-CH-1996""#))
        .page(&labelled(&span("T1", "Hallo")));
    let (after, back) = save_with_lang(&again, "fr-FR");
    assert!(!after.contains("rfc-language-tag") && after.contains(r#"fo:language="fr""#), "{after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("fr-FR"));
}

/// `x-none` is `zxx` with `none`, as LibreOffice writes it.
#[test]
fn no_language_is_written_as_zxx_and_none() {
    let (after, back) = save_with_lang(&two_runs(), "x-none");
    assert!(after.contains(r#"fo:language="zxx""#) && after.contains(r#"fo:country="none""#), "{after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("x-none"));
}

/// The triplet written is the one of the run's script type: an Asian run's goes into the Asian attributes, and the Latin ones stay as read.
#[test]
fn a_run_of_asian_letters_is_written_into_the_asian_triplet() {
    let both = text_style("T1", &format!("{LATIN} {ASIAN}"));
    let d = Drawing::a4().auto(&both).page(&labelled(&span("T1", NIHONGO)));
    let (after, back) = save_with_lang(&d, "ko-KR");
    assert!(after.contains(r#"style:language-asian="ko""#) && after.contains(r#"style:country-asian="KR""#), "{after}");
    assert!(after.contains(r#"fo:language="en""#) && after.contains(r#"fo:country="US""#), "the Latin triplet as read: {after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("ko-KR"));
    // The same style over Latin letters: the Latin triplet.
    let d = Drawing::a4().auto(&both).page(&labelled(&span("T1", "abc")));
    let (after, back) = save_with_lang(&d, "de-DE");
    assert!(after.contains(r#"fo:language="de""#) && after.contains(r#"style:language-asian="ja""#), "{after}");
    assert_eq!(langs(the_shape(&back))[0].1.as_deref(), Some("de-DE"));
}

/// A retext that turns the letters of a run into letters of another script type writes the run's language into the other triplet, and the run reads back in the language it was in.
#[test]
fn a_retext_that_changes_the_script_type_of_a_run_writes_the_language_into_the_other_triplet() {
    let mut o = open(&two_runs());
    let rows = markers(the_shape(&o.doc));
    assert_eq!(rows.len(), 2, "{rows:?}");
    set_parts(&mut o, vec![TextPart::Cp(rows[0]), chars(NIHONGO), TextPart::Cp(rows[1]), chars("x")]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(r#"style:language-asian="de""#) && after.contains(r#"style:country-asian="CH""#),
        "the Asian triplet says German (Switzerland): {after}"
    );
    assert!(!after.contains(r#"fo:language="de""#), "the Latin triplet no longer does: {after}");
    let back = open_bytes(saved.bytes);
    assert_eq!(langs(the_shape(&back.doc)).first().map(|(c, l)| (c.as_str(), l.as_deref())), Some((NIHONGO, Some("de-CH"))));
}

/// V77, the splice writer's half, for a new paragraph row: in a paragraph with `Flags` 1, `HorzAlign` 0 is `start` and 2 is `end`; with `Flags` 0 they are `left` and `right`.
#[test]
fn a_right_to_left_paragraph_is_aligned_by_start_and_end() {
    let styles = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
    type Case<'a> = (&'a [(&'a str, f64)], &'a str, &'a str);
    let cases: [Case; 6] = [
        (&[("HorzAlign", 0.0), ("Flags", 1.0)], r#"fo:text-align="start""#, r#"style:writing-mode="rl-tb""#),
        (&[("HorzAlign", 2.0), ("Flags", 1.0)], r#"fo:text-align="end""#, r#"style:writing-mode="rl-tb""#),
        (&[("HorzAlign", 1.0), ("Flags", 1.0)], r#"fo:text-align="center""#, r#"style:writing-mode="rl-tb""#),
        (&[("HorzAlign", 0.0), ("Flags", 0.0)], r#"fo:text-align="left""#, r#"style:writing-mode="lr-tb""#),
        (&[("HorzAlign", 2.0), ("Flags", 0.0)], r#"fo:text-align="right""#, r#"style:writing-mode="lr-tb""#),
        (&[("HorzAlign", 2.0)], r#"fo:text-align="right""#, r#"fo:text-align="right""#),
    ];
    for (cells, align, mode) in cases {
        let mut o = open(&Drawing::a4().auto(styles).page(&labelled("plain")));
        set_row(&mut o, "Paragraph", 1, cells);
        set_parts(&mut o, vec![TextPart::Pp(1), chars("text")]);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{cells:?}: {e}"));
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains(align) && after.contains(mode), "{cells:?}: {align} and {mode} in {after}");
        // Read back: the same cells.
        let back = open_bytes(saved.bytes);
        let s = the_shape(&back.doc);
        let pp = last_pp(s);
        for (name, expected) in cells {
            let got = row_cell(s, "Paragraph", None, pp, name).and_then(|c| c.value.as_ref()).and_then(Value::number);
            assert_eq!(got, Some(*expected), "{cells:?}: {name} read back");
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the model writer

fn with_language(tag: &str, doc_lang_id: bool, core: bool) -> Document {
    let mut doc = crate::model_write::blank(1);
    if core {
        Arc::make_mut(&mut doc.props).language = Some(Arc::from(tag));
    }
    if doc_lang_id {
        Arc::make_mut(&mut doc.sheet).cells.insert(CellName::new("DocLangID"), Cell::constant(Value::from_text(tag)));
    }
    doc
}

fn meta_of(bytes: &[u8]) -> String {
    part(bytes, "meta.xml")
}

/// A new drawing in `fr-FR` writes `dc:language` and the default graphic style's Latin triplet; one whose language is `DocLangID` alone is as well in `dc:language`.
#[test]
fn a_new_drawing_writes_its_language_in_dc_language_and_on_the_default_graphic_style() {
    let doc = with_language("fr-FR", true, true);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(meta_of(&written.bytes).contains("<dc:language>fr-FR</dc:language>"), "{}", meta_of(&written.bytes));
    let styles = part(&written.bytes, "styles.xml");
    assert!(
        styles.contains(
            r#"<style:default-style style:family="graphic"><style:text-properties fo:language="fr" fo:country="FR"/></style:default-style>"#
        ),
        "{styles}"
    );
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(back.props.language.as_deref(), Some("fr-FR"));
    assert_eq!(diacraft_doc::lang::document_language(&back, None).as_str(), "fr-FR");
    // `DocLangID` alone (no core language) is the drawing's language in `meta.xml`.
    let doc = with_language("de-DE", true, false);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(meta_of(&written.bytes).contains("<dc:language>de-DE</dc:language>"), "{}", meta_of(&written.bytes));
    // A drawing in no language writes none, as before.
    let written = write_with(&crate::model_write::blank(1), None, &WriteOptions::default()).unwrap();
    assert!(!meta_of(&written.bytes).contains("dc:language") && !part(&written.bytes, "styles.xml").contains("default-style"));
}

/// A drawing that has a default graphic style of its own keeps it: no second one is written for the drawing's language.
#[test]
fn a_default_style_the_drawing_holds_is_not_written_twice() {
    let defaults = r#"<style:default-style style:family="graphic"><style:text-properties fo:language="es" fo:country="ES"/></style:default-style>"#;
    let mut d = Drawing::a4().common(defaults).page(&labelled("Hola"));
    d.meta = Some(meta_with("Gen/1", "<dc:language>es-ES</dc:language>"));
    let (doc, _) = d.read();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let styles = part(&written.bytes, "styles.xml");
    assert_eq!(styles.matches("<style:default-style").count(), 1, "{styles}");
    assert!(styles.contains(r#"fo:language="es""#), "{styles}");
}

/// V77, the model writer's half: the same words from the cells of a text written from nothing.
#[test]
fn the_model_writer_aligns_a_right_to_left_paragraph_by_start_and_end() {
    for (align, flags, word) in [(0.0, 1.0, "start"), (2.0, 1.0, "end"), (1.0, 1.0, "center"), (0.0, 0.0, "left"), (2.0, 0.0, "right")] {
        let doc = doc_with(vec![TextPart::Pp(1), chars("text")], vec![section("Paragraph", vec![row(1, &[("HorzAlign", align), ("Flags", flags)])])]);
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
        let content = part(&written.bytes, "content.xml");
        assert!(content.contains(&format!(r#"fo:text-align="{word}""#)), "{align} {flags}: {content}");
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        let s = back.pages.get(0).unwrap().shapes.get(0).unwrap();
        let pp = last_pp(s);
        let got = |name: &str| row_cell(s, "Paragraph", None, pp, name).and_then(|c| c.value.as_ref()).and_then(Value::number);
        assert_eq!((got("HorzAlign"), got("Flags")), (Some(align), Some(flags)), "{word}");
    }
}

/// A run's language set in the model of a text written from nothing is written into the triplet of its script type, and reads back the same.
#[test]
fn the_model_writer_writes_a_run_s_language_into_its_triplet() {
    for (run, tag, attrs) in [
        ("Hallo", "de-CH", [r#"fo:language="de""#, r#"fo:country="CH""#]),
        (NIHON, "ja-JP", [r#"style:language-asian="ja""#, r#"style:country-asian="JP""#]),
        (AIN, "ar-EG", [r#"style:language-complex="ar""#, r#"style:country-complex="EG""#]),
    ] {
        let mut character = Row { ix: Some(1), ..Row::default() };
        character.cells.insert(CellName::new("LangID"), Cell::constant(Value::from_text(tag)));
        let mut sec = Section::new("Character", None);
        sec.rows.push(Arc::new(character));
        let doc = doc_with(vec![TextPart::Cp(1), chars(run)], vec![sec]);
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{tag}: {e}"));
        let content = part(&written.bytes, "content.xml");
        for attr in attrs {
            assert!(content.contains(attr), "{tag}: {attr} in {content}");
        }
        let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
        let s = back.pages.get(0).unwrap().shapes.get(0).unwrap();
        assert_eq!(langs(s).first().and_then(|(_, l)| l.as_deref()), Some(tag), "{tag}: read back");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ NoProofing

/// `NoProofing` has no OpenDocument form, so it rides in DiaCraft's own data on the shape's style (tier 2): the cell is read back as it was set,
/// from a drawing written from nothing and from an edit of a file's shape, and the runs keep their languages.
#[test]
fn no_proofing_rides_in_diacraft_s_data_and_is_read_back() {
    let no_proofing = |shape: &Shape| shape.sheet.cell("NoProofing").and_then(|c| c.value.as_ref()).and_then(Value::number);
    // From nothing.
    let mut doc = crate::model_write::blank(1);
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("NoProofing"), Cell::constant(Value::from_number(1.0).unwrap()));
    }
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(no_proofing(back.pages.get(0).unwrap().shapes.get(0).unwrap()), Some(1.0), "written from nothing");
    // An edit of a file's shape, whose runs have a language.
    let mut o = open(&two_runs());
    let id = id_of(&o.doc);
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet)
        .cells
        .insert(CellName::new("NoProofing"), Cell::constant(Value::from_number(1.0).unwrap()));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let back = open_bytes(saved.bytes);
    let s = the_shape(&back.doc);
    assert_eq!(no_proofing(s), Some(1.0), "an edit of a file's shape");
    assert_eq!(langs(s).first().and_then(|(_, l)| l.as_deref()), Some("de-CH"), "the runs keep their languages");
    // And cleared again, it is 0.
    let mut o = open_bytes(back.bytes);
    let id = id_of(&o.doc);
    Arc::make_mut(&mut shape_mut(&mut o.doc, 0, id).sheet)
        .cells
        .insert(CellName::new("NoProofing"), Cell::constant(Value::from_number(0.0).unwrap()));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(no_proofing(the_shape(&open_bytes(saved.bytes).doc)), Some(0.0));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ hostile inputs

/// A tag longer than a cell's own short text (64 bytes) is read whole, and a language of a megabyte is no tag: it is not joined into one, and costs a constant for each run that reads it.
#[test]
fn a_long_tag_is_read_whole_and_a_language_of_a_megabyte_costs_a_constant_for_each_run() {
    let long = format!("de-CH-1996-x-{}", "abcdefgh-".repeat(9).trim_end_matches('-'));
    assert!(long.len() > 64 && long.len() <= 255, "{}", long.len());
    assert_eq!(read_lang(&format!(r#"style:rfc-language-tag="{long}""#), "Hallo").as_deref(), Some(long.as_str()));
    // A tag past the bound of a tag is none, from a full tag as from a triplet.
    let too_long = format!("de-x-{}", "a".repeat(260));
    assert_eq!(
        read_lang(&format!(r#"style:rfc-language-tag="{too_long}" fo:language="de""#), "Hallo").as_deref(),
        Some("de"),
        "the triplet is the language when the full tag is no tag"
    );
    assert_eq!(read_lang(&format!(r#"fo:language="{}" fo:country="CH""#, "a".repeat(300)), "Hallo"), None);
    // A megabyte stated for each of the three, read by `runs` spans.
    let work = |runs: usize| {
        let big = "a".repeat(1 << 20);
        let props = format!(r#"fo:language="{big}" fo:script="{big}" fo:country="{big}""#);
        let spans: String = (0..runs).map(|k| span("T1", &format!("w{k}"))).collect();
        let (doc, report) = Drawing::a4().auto(&text_style("T1", &props)).page(&labelled(&spans)).read();
        assert!(langs(the_shape(&doc)).iter().all(|(_, lang)| lang.is_none()), "no tag is made of them");
        (report.work.style_steps, report.work.text_units, report.work.value_parses)
    };
    let (small, large) = (work(100), work(2_000));
    assert!(large.0 <= 25 * small.0 + 1_000, "style steps: {small:?} {large:?}");
    assert!(large.1 <= 25 * small.1 + 1_000, "text units: {small:?} {large:?}");
    assert_eq!(small.2, large.2, "no value is parsed for a run: {small:?} {large:?}");
}

/// Every language the reader makes is a language tag in canonical case or `x-none`, whatever the attributes hold (a property test over the words the three triplets are given).
#[test]
fn a_language_that_is_read_is_always_a_tag_or_no_language() {
    let words = [
        "",
        "de",
        "DE",
        "zxx",
        "none",
        "x-none",
        "CH",
        "ch",
        "419",
        "Latn",
        "latn",
        "d3 4",
        "de-CH-1996",
        "x-private",
        "i-klingon",
        "a",
        "abcdefghi",
        "12",
        "-",
        "de_DE",
    ];
    let mut checked = 0usize;
    for (language, script, country, rfc) in itertools_like(&words) {
        let props = format!(r#"fo:language="{language}" fo:script="{script}" fo:country="{country}" style:rfc-language-tag="{rfc}""#);
        if let Some(tag) = read_lang(&props, "Hello") {
            assert!(tag == "x-none" || diacraft_doc::lang::parse(&tag).is_ok_and(|t| t.as_str() == tag), "{props}: {tag:?}");
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked} of the combinations read a language");
}

/// The combinations of `words` taken four at a time, a stride apart so that the test is a few hundred runs and not 160,000.
fn itertools_like<'a>(words: &[&'a str]) -> Vec<(&'a str, &'a str, &'a str, &'a str)> {
    let n = words.len();
    let mut out = Vec::new();
    for a in 0..n {
        for b in (0..n).step_by(3) {
            for c in (0..n).step_by(4) {
                for d in (0..n).step_by(5) {
                    out.push((words[a], words[(b + a) % n], words[(c + a) % n], words[(d + b) % n]));
                }
            }
        }
    }
    out
}
