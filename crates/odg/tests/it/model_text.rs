//! The text of a model written from nothing (`write_with(doc, None, ..)`): carriage returns, runs, paragraph formatting, lists, headings and fields, as the markup an `.odg` has for them,
//! with the names the file gave while the stamps hold, and what the file cannot say carried. Every read-back goes through the real reader (`read`); the inputs are built by code or written by hand from the
//! ODF standard (Part 3, chapters 5 to 7 and 16).

use std::sync::Arc;

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Cell, CellName, Document, OdfRun, RawNode, Row, Section, Shape, Text, TextPart, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, SaveNote, WriteOptions, compare_content, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;
use crate::model_write::blank;

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ helpers

pub(crate) fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

pub(crate) fn number(n: f64) -> Cell {
    Cell::constant(Value::from_number(n).unwrap())
}

/// A row of `section` with the cells `(name, number)`.
pub(crate) fn row(ix: u32, cells: &[(&str, f64)]) -> Row {
    let mut r = Row { ix: Some(ix), ..Row::default() };
    for (name, n) in cells {
        r.cells.insert(CellName::new(name), number(*n));
    }
    r
}

pub(crate) fn section(name: &str, rows: Vec<Row>) -> Section {
    let mut s = Section::new(name, None);
    s.rows = rows.into_iter().map(Arc::new).collect();
    s.rows.iter().for_each(|_| ());
    s
}

/// A drawing of one rectangle whose text is `parts`, with the sections the rows are in.
pub(crate) fn doc_with(parts: Vec<TextPart>, sections: Vec<Section>) -> Document {
    let mut doc = blank(1);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    shape.text = Some(Arc::new(Text { parts, ..Default::default() }));
    let sheet = Arc::make_mut(&mut shape.sheet);
    for s in sections {
        sheet.sections.push(Arc::new(s));
    }
    doc
}

pub(crate) fn first_shape(doc: &Document) -> &Shape {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap()
}

/// `content.xml` from its first container on (the root's declarations are long, and the same in every file).
pub(crate) fn content_of(bytes: &[u8]) -> String {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let text = String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap();
    let at = text.find("<office:font-face-decls").or_else(|| text.find("<office:automatic-styles")).unwrap_or(0);
    text[at..].to_owned()
}

/// The model written from nothing and read again: the bytes, the model read, and the classes of the notes.
fn round(doc: &Document) -> (Vec<u8>, Document, Vec<&'static str>) {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
    let notes = written.notes.iter().map(SaveNote::class).collect();
    (written.bytes, back, notes)
}

/// The package with DiaCraft's data taken off every element of `content.xml`: what a program that reads only the standard's markup is given.
fn without_extension(bytes: &[u8]) -> Vec<u8> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    for entry in package.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        let mut data = package.part(&entry.name).unwrap().to_vec();
        if entry.name == "content.xml" {
            let mut text = String::from_utf8(data).unwrap();
            while let Some(at) = text.find(" diacraft:") {
                // `diacraft:name="value"`: the value has no quote in it (the writer escapes it).
                let open = text[at..].find('"').unwrap() + at;
                let close = text[open + 1..].find('"').unwrap() + open + 1;
                text.replace_range(at..=close, "");
            }
            data = text.into_bytes();
        }
        let method = if entry.name == "mimetype" { Method::Stored } else { Method::Deflated };
        writer.add(&entry.name, &data, method).unwrap();
    }
    writer.finish().unwrap()
}

/// The cells of a row of the shape's `section`, as `name=value|unit` in the order of the names.
fn cells_of(shape: &Shape, section: &str, ix: u32) -> Vec<String> {
    shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == section)
        .flat_map(|s| s.rows.iter())
        .find(|r| r.ix == Some(ix))
        .map(|r| {
            r.cells
                .iter()
                .map(|(n, c)| {
                    format!(
                        "{}={}|{}",
                        n.as_str(),
                        c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
                        c.unit.clone().unwrap_or_default()
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A paragraph of a text as a person sees it: its characters, the cells of its paragraph row, and each run's characters range and the cells of its row. **The `IX` of a row is no part of it.**
#[derive(Debug, PartialEq, Clone)]
struct Seen {
    chars: String,
    paragraph: Vec<String>,
    runs: Vec<(usize, usize, Vec<String>)>,
}

fn seen(shape: &Shape) -> Vec<Seen> {
    let Some(text) = &shape.text else { return Vec::new() };
    let (mut pp, mut cp) = (0u32, 0u32);
    let mut out: Vec<Seen> = Vec::new();
    let mut current = Seen { chars: String::new(), paragraph: Vec::new(), runs: Vec::new() };
    let mut run_start = 0usize;
    let mut count = 0usize;
    let close_run = |current: &mut Seen, run_start: usize, count: usize, cp: u32| {
        if count > run_start {
            let cells = cells_of(shape, "Character", cp);
            match current.runs.last_mut() {
                Some(last) if last.1 == run_start && last.2 == cells => last.1 = count,
                _ => current.runs.push((run_start, count, cells)),
            }
        }
    };
    for part in &text.parts {
        match part {
            TextPart::Chars(c) => {
                for (i, piece) in c.split('\n').enumerate() {
                    if i > 0 {
                        close_run(&mut current, run_start, count, cp);
                        current.paragraph = cells_of(shape, "Paragraph", pp);
                        out.push(std::mem::replace(&mut current, Seen { chars: String::new(), paragraph: Vec::new(), runs: Vec::new() }));
                        run_start = 0;
                        count = 0;
                    }
                    current.chars.push_str(piece);
                    count += piece.chars().count();
                }
            }
            TextPart::Fld { text, .. } => {
                current.chars.push_str(text);
                count += text.chars().count();
            }
            TextPart::Cp(ix) => {
                close_run(&mut current, run_start, count, cp);
                run_start = count;
                cp = *ix;
            }
            TextPart::Pp(ix) => pp = *ix,
            _ => {}
        }
    }
    close_run(&mut current, run_start, count, cp);
    current.paragraph = cells_of(shape, "Paragraph", pp);
    out.push(current);
    out
}

/// The formulas and shown texts of the fields of a text, in order.
fn fields_of(shape: &Shape) -> Vec<(String, String)> {
    let Some(text) = &shape.text else { return Vec::new() };
    text.parts
        .iter()
        .filter_map(|p| match p {
            TextPart::Fld { ix, text } => {
                let formula = shape
                    .sheet
                    .sections
                    .iter()
                    .filter(|s| s.name.as_str() == "Field")
                    .flat_map(|s| s.rows.iter())
                    .find(|r| r.ix == Some(*ix))
                    .and_then(|r| r.cells.get("Value"))
                    .and_then(|c| c.formula.text().map(str::to_owned))
                    .unwrap_or_default();
                Some((formula, text.to_string()))
            }
            _ => None,
        })
        .collect()
}

/// The characters of a text, the paragraphs between U+000A, every field's shown text among them.
fn characters(shape: &Shape) -> String {
    shape
        .text
        .as_ref()
        .map(|t| {
            t.parts
                .iter()
                .map(|p| match p {
                    TextPart::Chars(c) => c.to_string(),
                    TextPart::Fld { text, .. } => text.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn model_of(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap().0
}

/// A custom shape holding `inner` as its text.
pub(crate) fn shape_of(inner: &str) -> String {
    format!(
        "<draw:custom-shape draw:name=\"x\" svg:x=\"1cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\">{inner}<draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>"
    )
}

pub(crate) fn drawing_with(auto: &str, inner: &str) -> Drawing {
    Drawing::a4().auto(auto).page(&shape_of(inner))
}

/// The styles the vectors use: a centred paragraph, a bold run, a bullet list, a numbered list, and an alignment spelled `start`.
pub(crate) const STYLES: &str = concat!(
    r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"##,
    r##"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="start" style:line-spacing="0.2cm"/></style:style>"##,
    r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"##,
    r##"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic" style:text-underline-style="wave" style:text-underline-color="#ff0000"/></style:style>"##,
    r##"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="&#8226;"/></text:list-style>"##,
    r##"<text:list-style style:name="L2"><text:list-level-style-number text:level="1" style:num-suffix="." style:num-format="1"/></text:list-style>"##,
);

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ carriage returns

#[test]
fn a_text_with_a_carriage_return_survives_a_model_save() {
    let doc = doc_with(vec![chars("a\r\nb\rc")], Vec::new());
    let (bytes, back, _) = round(&doc);
    let xml = content_of(&bytes);
    // The file shows three paragraphs, a `CR LF` being one break.
    assert_eq!(xml.matches("<text:p>").count(), 3, "{xml}");
    assert!(xml.contains("<text:p>a</text:p><text:p>b</text:p><text:p>c</text:p>"), "{xml}");
    // DiaCraft's data carries the exact text, carriage returns and all.
    assert_eq!(characters(first_shape(&back)), "a\r\nb\rc");
    // A program that reads only the standard's markup reads three paragraphs.
    let (plain, _) = read(&without_extension(&bytes), &Limits::default()).unwrap();
    assert_eq!(characters(first_shape(&plain)), "a\nb\nc");
}

#[test]
fn a_carriage_return_that_two_parts_of_the_text_divide_is_one_break() {
    let doc = doc_with(vec![chars("a\r"), chars("\nb")], Vec::new());
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("<text:p>a</text:p><text:p>b</text:p>"));
    assert_eq!(characters(first_shape(&back)), "a\r\nb");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ runs

#[test]
fn a_text_with_runs_survives_a_model_save() {
    let parts = vec![chars("plain "), TextPart::Cp(1), chars("bold"), TextPart::Cp(2), chars(" and italic"), TextPart::Cp(0), chars(" end")];
    let rows = section("Character", vec![row(1, &[("Style", 1.0)]), row(2, &[("Style", 2.0), ("Size", 14.0 / 72.0)])]);
    let doc = doc_with(parts, vec![rows]);
    let (bytes, back, _) = round(&doc);
    let xml = content_of(&bytes);
    // Each run that writes an attribute is a span of an automatic text style that holds it.
    assert!(
        xml.contains("plain <text:span text:style-name=\"T1\">bold</text:span><text:span text:style-name=\"T2\"> and italic</text:span> end"),
        "{xml}"
    );
    assert!(xml.contains("<style:style style:name=\"T1\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\""), "{xml}");
    assert!(xml.contains("fo:font-style=\"italic\"") && xml.contains("fo:font-size=\"14pt\""), "{xml}");
    // DiaCraft reads its own rows back, the very cells.
    assert_eq!(seen(first_shape(&back)), seen(first_shape(&doc)));
    // A program that reads the standard's markup only sees the run bold.
    let (plain, _) = read(&without_extension(&bytes), &Limits::default()).unwrap();
    let runs = seen(first_shape(&plain));
    assert_eq!(runs[0].chars, "plain bold and italic end");
    let bold = runs[0].runs.iter().find(|r| (r.0, r.1) == (6, 10)).expect("a run of `bold`");
    assert!(bold.2.contains(&"Style=1|".to_owned()), "{bold:?}");
}

#[test]
fn runs_are_written_with_white_space_as_the_standard_has_it() {
    let parts = vec![TextPart::Cp(1), chars("a  b\tc\u{2028}d"), TextPart::Cp(0), chars(" e  ")];
    let doc = doc_with(parts, vec![section("Character", vec![row(1, &[("Style", 1.0)])])]);
    let (bytes, back, _) = round(&doc);
    let xml = content_of(&bytes);
    assert!(
        xml.contains("<text:span text:style-name=\"T1\">a <text:s/>b<text:tab/>c<text:line-break/>d</text:span> e<text:s text:c=\"2\"/>"),
        "{xml}"
    );
    assert_eq!(characters(first_shape(&back)), "a  b\tc\u{2028}d e  ");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ paragraphs

#[test]
fn paragraph_formatting_survives_a_model_save() {
    let parts = vec![TextPart::Pp(1), chars("one\ntwo\n"), TextPart::Pp(0), chars("three")];
    let rows = section("Paragraph", vec![row(1, &[("HorzAlign", 2.0), ("SpBefore", 6.0 / 72.0), ("IndLeft", 0.5)])]);
    let doc = doc_with(parts, vec![rows]);
    let (bytes, back, _) = round(&doc);
    let xml = content_of(&bytes);
    assert!(
        xml.contains("fo:text-align=\"right\"") && xml.contains("fo:margin-top=\"0.083333333333in\"") && xml.contains("fo:margin-left=\"0.5in\""),
        "{xml}"
    );
    assert_eq!(xml.matches("<text:p text:style-name=\"P1\">").count(), 2, "{xml}");
    assert!(xml.contains("<text:p>three</text:p>"), "{xml}");
    assert_eq!(seen(first_shape(&back)), seen(first_shape(&doc)));
    let (plain, _) = read(&without_extension(&bytes), &Limits::default()).unwrap();
    let paragraphs = seen(first_shape(&plain));
    assert!(paragraphs[0].paragraph.iter().any(|c| c.starts_with("HorzAlign=2")), "{paragraphs:?}");
    assert!(paragraphs[2].paragraph.is_empty() || !paragraphs[2].paragraph.iter().any(|c| c.starts_with("HorzAlign=2")), "{paragraphs:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ lists

#[test]
fn a_bulleted_list_survives_a_model_save() {
    let parts = vec![TextPart::Pp(1), chars("one\ntwo\n"), TextPart::Pp(0), chars("three")];
    let rows = section("Paragraph", vec![row(1, &[("Bullet", 3.0)])]);
    let doc = doc_with(parts, vec![rows]);
    let (bytes, back, _) = round(&doc);
    let xml = content_of(&bytes);
    // The two paragraphs of one row are the items of one list, with an automatic list style of the bullet the row says.
    assert!(xml.contains("<text:list text:style-name=\"L1\"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list><text:p>three</text:p>"), "{xml}");
    assert!(xml.contains("<text:list-style style:name=\"L1\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"\u{25AA}\""), "{xml}");
    assert_eq!(seen(first_shape(&back)), seen(first_shape(&doc)));
    let (plain, _) = read(&without_extension(&bytes), &Limits::default()).unwrap();
    assert!(seen(first_shape(&plain))[0].paragraph.iter().any(|c| c.starts_with("Bullet=3")));
}

/// The markup of the lists the shape's text holds, as the names of the elements in the order they open and close.
fn list_shape(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<text:") {
        let tail = &rest[at..];
        let end = tail.find([' ', '>', '/']).unwrap_or(tail.len());
        let name = &tail[1..end];
        if matches!(name, "text:list" | "text:list-item" | "text:list-header" | "text:p" | "text:h") {
            out.push_str(&format!("<{}>", name.trim_start_matches("text:")));
        }
        rest = &tail[end..];
    }
    let mut closing = String::new();
    for name in ["list-item", "list"] {
        closing.push_str(&format!("</{name}>"));
    }
    let _ = closing;
    out
}

#[test]
fn a_numbered_list_survives_a_model_save() {
    let d = drawing_with(
        STYLES,
        r##"<text:list text:style-name="L2"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"##,
    );
    let (bytes, back, _) = round(&model_of(&d));
    let xml = content_of(&bytes);
    assert!(xml.contains("<text:list text:style-name=\"L2\"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"), "{xml}");
    assert!(xml.contains("text:list-level-style-number"), "the held list style is written: {xml}");
    assert_eq!(characters(first_shape(&back)), "one\ntwo");
    // The file says all of it: DiaCraft's data carries none of the text.
    assert!(!xml.contains("diacraft:text"), "{xml}");
}

#[test]
fn a_nested_list_survives_a_model_save() {
    let d = drawing_with(
        STYLES,
        concat!(
            r##"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>inner</text:p></text:list-item><text:list-item><text:p>inner two</text:p></text:list-item></text:list></text:list-item>"##,
            r##"<text:list-item><text:p>two</text:p></text:list-item></text:list>"##
        ),
    );
    let before = model_of(&d);
    let (bytes, back, _) = round(&before);
    let xml = content_of(&bytes);
    assert_eq!(characters(first_shape(&back)), "one\ninner\ninner two\ntwo");
    let shape_markup = &xml[xml.find("<draw:custom-shape").unwrap()..];
    assert!(
        shape_markup.contains("<text:list text:style-name=\"L1\"><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>inner</text:p></text:list-item><text:list-item><text:p>inner two</text:p></text:list-item></text:list></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"),
        "{shape_markup}"
    );
    let _ = list_shape(&xml);
    assert_eq!(held_levels(first_shape(&back)), held_levels(first_shape(&before)));
}

/// The lists each held paragraph is in: how many levels, and the name of each list and item it opens.
fn held_levels(shape: &Shape) -> Vec<Vec<(Option<String>, Option<String>)>> {
    let Some(t) = shape.text.as_ref().and_then(|t| t.ext.get()) else { return Vec::new() };
    t.odf
        .paragraphs
        .iter()
        .map(|p| p.levels.iter().map(|l| (l.list.as_ref().map(|e| e.name.to_string()), l.item.as_ref().map(|e| e.name.to_string()))).collect())
        .collect()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ headings

#[test]
fn a_heading_survives_a_model_save() {
    let d = drawing_with(STYLES, r##"<text:h text:outline-level="2" text:style-name="P1">Title</text:h><text:p>body</text:p>"##);
    let before = model_of(&d);
    let (bytes, back, _) = round(&before);
    let xml = content_of(&bytes);
    assert!(xml.contains("<text:h text:outline-level=\"2\" text:style-name=\"P1\">Title</text:h><text:p>body</text:p>"), "{xml}");
    assert_eq!(seen(first_shape(&back)), seen(first_shape(&before)));
    let elements = |d: &Document| -> Vec<Option<String>> {
        first_shape(d)
            .text
            .as_ref()
            .and_then(|t| t.ext.get())
            .map(|t| t.odf.paragraphs.iter().map(|p| p.element.as_ref().map(|e| e.name.to_string())).collect())
            .unwrap_or_default()
    };
    assert_eq!(elements(&back), [Some("text:h".to_owned()), None]);
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ fields

/// A text of one paragraph `p <field> q` with a field row of `formula` showing `shown`.
fn field_doc(formula: &str, shown: &str) -> Document {
    let mut field = Row { ix: Some(0), ..Row::default() };
    field.cells.insert(CellName::new("Value"), Cell::read(Some(formula), Some(shown), None, None, &mut 1_000));
    let parts = vec![chars("p "), TextPart::Fld { ix: 0, text: Arc::from(shown) }, chars(" q")];
    doc_with(parts, vec![section("Field", vec![field])])
}

fn field_survives(formula: &str, element: &str, shown: &str) {
    let doc = field_doc(formula, shown);
    let (bytes, back, notes) = round(&doc);
    let xml = content_of(&bytes);
    assert!(xml.contains(&format!("p <text:{element}>{shown}</text:{element}> q")), "{formula}: {xml}");
    assert!(!notes.contains(&"field_as_text"), "{notes:?}");
    assert_eq!(fields_of(first_shape(&back)), [(formula.to_owned(), shown.to_owned())], "{formula}");
    assert_eq!(characters(first_shape(&back)), format!("p {shown} q"));
    // The file says all of it.
    assert!(!xml.contains("diacraft:text") && !xml.contains("diacraft:s-Field"), "{xml}");
}

#[test]
fn a_page_number_field_survives_a_model_save() {
    field_survives("PAGENUMBER()", "page-number", "3");
}

#[test]
fn a_page_count_field_survives_a_model_save() {
    field_survives("PAGECOUNT()", "page-count", "9");
}

#[test]
fn a_file_name_field_survives_a_model_save() {
    field_survives("FILENAME()", "file-name", "plan.odg");
}

#[test]
fn a_date_or_time_field_survives_a_model_save() {
    field_survives("NOW()", "date", "2026-11-22");
}

#[test]
fn a_time_field_of_a_file_survives_a_model_save_as_the_element_it_was() {
    let d = drawing_with(STYLES, r##"<text:p>at <text:time>12:30</text:time> o</text:p>"##);
    let before = model_of(&d);
    assert_eq!(fields_of(first_shape(&before)), [("NOW()".to_owned(), "12:30".to_owned())]);
    let (bytes, back, _) = round(&before);
    assert!(content_of(&bytes).contains("at <text:time>12:30</text:time> o"), "{}", content_of(&bytes));
    assert_eq!(fields_of(first_shape(&back)), fields_of(first_shape(&before)));
}

#[test]
fn a_field_with_no_element_is_written_as_its_shown_text_and_said() {
    for (formula, shown) in [("PAGENAME()", "Page-1"), ("Prop.Row_1", "42"), ("DOCCREATION()", "2026")] {
        let doc = field_doc(formula, shown);
        let (bytes, back, notes) = round(&doc);
        let xml = content_of(&bytes);
        assert!(xml.contains(&format!("<text:p>p {shown} q</text:p>")), "{formula}: {xml}");
        assert!(notes.contains(&"field_as_text"), "{formula}: {notes:?}");
        // The field rides in DiaCraft's data, which reads it back.
        assert_eq!(fields_of(first_shape(&back)), [(formula.to_owned(), shown.to_owned())], "{formula}");
    }
}

#[test]
fn a_fixed_date_a_link_and_a_bookmark_a_file_held_are_written_where_they_stood() {
    let d = drawing_with(
        STYLES,
        concat!(
            r##"<text:p>a <text:date text:fixed="true">2020-01-01</text:date> b <text:a xlink:type="simple" xlink:href="http://example.org/">link</text:a> c<text:bookmark text:name="mark"/> d</text:p>"##,
            r##"<text:p>x<text:bookmark-start text:name="r"/>y<text:bookmark-end text:name="r"/>z</text:p>"##
        ),
    );
    let before = model_of(&d);
    let (bytes, back, _) = round(&before);
    let xml = content_of(&bytes);
    assert!(xml.contains("a <text:date text:fixed=\"true\">2020-01-01</text:date> b "), "{xml}");
    assert!(
        xml.contains("<text:a xlink:type=\"simple\" xlink:href=\"http://example.org/\">link</text:a> c<text:bookmark text:name=\"mark\"/> d"),
        "{xml}"
    );
    assert!(xml.contains("x<text:bookmark-start text:name=\"r\"/>y<text:bookmark-end text:name=\"r\"/>z"), "{xml}");
    assert_eq!(characters(first_shape(&back)), characters(first_shape(&before)));
    // Read again, the marks are in the text and the link is held.
    let marks = |d: &Document| first_shape(d).text.as_ref().unwrap().parts.iter().filter(|p| matches!(p, TextPart::OdfRaw(_))).count();
    assert_eq!(marks(&back), 3);
    assert_eq!(marks(&back), marks(&before));
    assert!(!xml.contains("diacraft:text"), "{xml}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ held names

/// The paragraph and run style names every held paragraph of the shape names, in order.
fn held_names(shape: &Shape) -> Vec<(Option<String>, Vec<Option<String>>)> {
    let Some(t) = shape.text.as_ref().and_then(|t| t.ext.get()) else { return Vec::new() };
    t.odf
        .paragraphs
        .iter()
        .map(|p| {
            (p.style.name.as_ref().map(|n| n.to_string()), p.runs.iter().map(|r: &OdfRun| r.style.name.as_ref().map(|n| n.to_string())).collect())
        })
        .collect()
}

const HELD: &str = concat!(
    r##"<text:p text:style-name="P1">ab<text:span text:style-name="T1">cd</text:span> <text:span text:style-name="T2">ef</text:span></text:p>"##,
    r##"<text:p text:style-name="P3">plain</text:p>"##,
    r##"<text:list text:style-name="L1"><text:list-item><text:p text:style-name="P1">one</text:p></text:list-item></text:list>"##,
    r##"<text:h text:outline-level="1">Head</text:h>"##,
    r##"<text:p><text:page-number>3</text:page-number> <text:a xlink:href="#a" xlink:type="simple">go</text:a><text:bookmark text:name="b"/></text:p>"##,
);

#[test]
fn the_names_a_file_gave_its_styles_are_written_while_their_stamps_hold() {
    let before = model_of(&drawing_with(STYLES, HELD));
    let names_before = held_names(first_shape(&before));
    assert_eq!(names_before[0], (Some("P1".to_owned()), vec![Some("T1".to_owned()), Some("T2".to_owned())]));
    let (bytes, back, _) = round(&before);
    let xml = content_of(&bytes);
    // The names, and what each held: the alignment as the file spelled it, the underline's colour.
    assert!(xml.contains("<text:p text:style-name=\"P1\">ab<text:span text:style-name=\"T1\">cd</text:span> <text:span text:style-name=\"T2\">ef</text:span></text:p>"), "{xml}");
    assert!(xml.contains("<style:style style:name=\"P3\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"start\" style:line-spacing=\"0.2cm\"/></style:style>"), "{xml}");
    assert!(xml.contains("style:text-underline-style=\"wave\"") && xml.contains("style:text-underline-color=\"#ff0000\""), "{xml}");
    assert_eq!(held_names(first_shape(&back)), names_before);
    // The file says all of its text.
    assert!(!xml.contains("diacraft:text") && !xml.contains("diacraft:s-Character") && !xml.contains("diacraft:s-Paragraph"), "{xml}");
}

#[test]
fn a_name_the_file_gave_is_no_name_the_writer_makes_again() {
    // The file's paragraph style is `P2`, which is the name the writer's own paragraph style for the shape takes first.
    let d = drawing_with(
        r##"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"##,
        r##"<text:p text:style-name="P2">centred</text:p>"##,
    );
    let (bytes, _, _) = round(&model_of(&d));
    let xml = content_of(&bytes);
    let names: Vec<&str> = xml.match_indices("style:name=\"").map(|(at, _)| xml[at + 12..].split('"').next().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names.len(), sorted.len(), "no two styles of one name: {names:?}");
    assert!(xml.contains("<text:p text:style-name=\"P2\">centred</text:p>"), "{xml}");
}

#[test]
fn a_held_value_is_not_written_once_the_row_it_stood_beside_changed() {
    let before = model_of(&drawing_with(STYLES, r##"<text:p text:style-name="P3">plain</text:p>"##));
    let mut doc = before.clone();
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut shape.sheet);
        for s in sheet.sections.iter_mut() {
            if s.name.as_str() == "Paragraph" {
                let s = Arc::make_mut(s);
                for r in s.rows.iter_mut() {
                    Arc::make_mut(r).cells.insert(CellName::new("HorzAlign"), number(2.0));
                }
            }
        }
    }
    let (bytes, _, _) = round(&doc);
    let xml = content_of(&bytes);
    assert!(xml.contains("fo:text-align=\"right\"") && !xml.contains("fo:text-align=\"start\""), "{xml}");
    assert!(!xml.contains("style:name=\"P3\""), "the name is a name of the row as it was: {xml}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the visit and the second save

/// The vectors: each a text a file holds, by hand from the standard (Part 3 chapters 5 to 7).
fn vectors() -> Vec<(&'static str, Drawing)> {
    let plain = drawing_with(STYLES, r##"<text:p>one</text:p><text:p>two  three<text:tab/>four<text:line-break/>five</text:p>"##);
    let styled = drawing_with(
        STYLES,
        r##"<text:p text:style-name="P1">ab<text:span text:style-name="T1">cd</text:span><text:span text:style-name="T2">ef</text:span></text:p>"##,
    );
    let centred = drawing_with(STYLES, r##"<text:p text:style-name="P1">a</text:p><text:p text:style-name="P3">b</text:p><text:p>c</text:p>"##);
    let bullets = drawing_with(
        STYLES,
        r##"<text:list text:style-name="L1"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list><text:p>after</text:p>"##,
    );
    let numbered = drawing_with(
        STYLES,
        r##"<text:list text:style-name="L2"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list>"##,
    );
    let nested = drawing_with(
        STYLES,
        r##"<text:list text:style-name="L1"><text:list-item><text:p>a</text:p><text:list><text:list-item><text:p>b</text:p></text:list-item></text:list></text:list-item><text:list-item><text:p>c</text:p></text:list-item></text:list>"##,
    );
    let heading = drawing_with(STYLES, r##"<text:h text:outline-level="1">T</text:h><text:p>body</text:p>"##);
    let fields = drawing_with(
        STYLES,
        r##"<text:p>p <text:page-number>3</text:page-number> of <text:page-count>9</text:page-count> in <text:file-name>f.odg</text:file-name> <text:date>2026-01-01</text:date> <text:time>12:00</text:time></text:p>"##,
    );
    let kept = drawing_with(
        STYLES,
        r##"<text:p>a <text:date text:fixed="true">2020</text:date> <text:a xlink:href="#x" xlink:type="simple">link</text:a><text:bookmark text:name="m"/></text:p>"##,
    );
    let held = drawing_with(STYLES, HELD);
    vec![
        ("plain text with white space", plain),
        ("runs", styled),
        ("paragraph formatting", centred),
        ("a bulleted list", bullets),
        ("a numbered list", numbered),
        ("a nested list", nested),
        ("a heading", heading),
        ("every field", fields),
        ("a fixed date, a link and a bookmark", kept),
        ("every held text group", held),
    ]
}

/// What a text holds of the file that no cell says, as the reader held it: the element of each paragraph, the names it and its runs named, and the lists it was in.
fn held_view(shape: &Shape) -> Vec<String> {
    let Some(t) = shape.text.as_ref().and_then(|t| t.ext.get()) else { return Vec::new() };
    t.odf
        .paragraphs
        .iter()
        .map(|p| {
            format!(
                "{:?} {:?} {:?} {:?} {:?}",
                p.element.as_ref().map(|e| (e.name.to_string(), e.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>())),
                p.style.name,
                p.runs.iter().map(|r| (r.from, r.to, r.style.name.clone(), r.element.as_ref().map(|e| e.name.to_string()))).collect::<Vec<_>>(),
                p.levels
                    .iter()
                    .map(|l| (l.list.as_ref().map(|e| e.name.to_string()), l.item.as_ref().map(|e| e.name.to_string())))
                    .collect::<Vec<_>>(),
                p.style.held.iter().map(|h| (h.group, h.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>())).collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// **An `.odg` visit through DiaCraft has the same content** for the text rows of the design's §3.4 (hand-built vectors): a drawing read, written from the model alone and read again holds the same paragraphs,
/// the same cells in each paragraph's and run's row, the same fields, the same style names, lists, headings and elements, and **DiaCraft's data carries none of the text**.
#[test]
fn an_odg_visit_through_diacraft_has_the_same_content() {
    for (name, drawing) in vectors() {
        let before = model_of(&drawing);
        let written = write_with(&before, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() })
            .unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let (after, _) = read(&written.bytes, &Limits::default()).unwrap();
        let (a, b) = (first_shape(&before), first_shape(&after));
        assert_eq!(characters(b), characters(a), "{name}: the characters");
        assert_eq!(seen(b), seen(a), "{name}: the paragraphs, runs and the cells of their rows");
        assert_eq!(fields_of(b), fields_of(a), "{name}: the fields");
        assert_eq!(held_view(b), held_view(a), "{name}: the names, elements, lists and held values");
        let xml = content_of(&written.bytes);
        for carried in ["diacraft:text", "diacraft:s-Character", "diacraft:s-Paragraph", "diacraft:s-Field"] {
            assert!(!xml.contains(carried), "{name}: the text is the file's, and {carried} is in it: {xml}");
        }
        // The markup of the text is the file's: the same elements in the same order.
        let tags = |x: &str| -> Vec<String> {
            let shape = &x[x.find("<draw:custom-shape").unwrap()..x.find("<draw:enhanced-geometry").unwrap()];
            shape.match_indices("<text:").map(|(at, _)| shape[at..].split([' ', '>', '/']).next().unwrap().to_owned()).collect()
        };
        let original = String::from_utf8(
            Package::read(
                &package(DRAWING, {
                    let mut d = drawing.clone();
                    d.version = "1.4".to_owned();
                    d.files()
                }),
                &Limits::default(),
            )
            .unwrap()
            .part("content.xml")
            .unwrap()
            .to_vec(),
        )
        .unwrap();
        assert_eq!(tags(&xml), tags(&original), "{name}: the elements of the text");
    }
}

/// **A second model save is no edit** for a drawing that holds every held text group: the file written, read again and written again has the same content.
#[test]
fn a_second_model_save_is_no_edit() {
    for (name, drawing) in vectors() {
        let before = model_of(&drawing);
        let options = WriteOptions { content_unchanged: true, ..WriteOptions::default() };
        let first = write_with(&before, None, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
        let second = write_with(&again, None, &options).unwrap_or_else(|e| panic!("{name}: the second save: {e}"));
        let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
        let shown: Vec<String> = diff.shown.iter().map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
        assert!(diff.same(), "{name}: the second save differs: {shown:?}");
    }
    // The held names are still written the second time.
    let first = write_with(&model_of(&drawing_with(STYLES, HELD)), None, &WriteOptions::default()).unwrap();
    let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
    let second = write_with(&again, None, &WriteOptions::default()).unwrap();
    let xml = content_of(&second.bytes);
    for name in ["P1", "P3", "T1", "T2", "L1"] {
        assert!(xml.contains(&format!("style:name=\"{name}\"")), "{name}: {xml}");
    }
}

#[test]
fn a_second_model_save_of_a_text_made_by_the_engine_is_no_edit() {
    let parts =
        vec![TextPart::Pp(1), chars("one\ntwo\n"), TextPart::Pp(0), TextPart::Cp(1), chars("bold"), TextPart::Cp(0), chars(" and a\r\nreturn")];
    let sections = vec![section("Paragraph", vec![row(1, &[("Bullet", 1.0)])]), section("Character", vec![row(1, &[("Style", 1.0)])])];
    let doc = doc_with(parts, sections);
    let options = WriteOptions::default();
    let first = write_with(&doc, None, &options).unwrap();
    let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
    assert_eq!(seen(first_shape(&again)), seen(first_shape(&doc)), "the first save keeps what the model holds, the carriage return aside");
    let second = write_with(&again, None, &options).unwrap();
    let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
    let shown: Vec<String> = diff.shown.iter().map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    assert!(diff.same(), "{shown:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the refusals closed

#[test]
fn a_text_with_a_field_is_written() {
    let doc = field_doc("PAGENUMBER()", "1");
    assert!(write_with(&doc, None, &WriteOptions::default()).is_ok());
}

#[test]
fn a_text_with_a_tab_stop_marker_is_written_and_carried() {
    let doc = doc_with(vec![chars("a"), TextPart::Tp(0), chars("b")], Vec::new());
    let (_, back, _) = round(&doc);
    assert_eq!(characters(first_shape(&back)), "ab");
    assert!(first_shape(&back).text.as_ref().unwrap().parts.iter().any(|p| matches!(p, TextPart::Tp(0))), "the marker comes back");
}

#[test]
fn a_text_with_an_element_the_model_keeps_among_its_parts_is_written() {
    let raw = diacraft_doc::RawElement {
        name: Arc::from("text:bookmark"),
        attrs: diacraft_doc::RawAttrs::from_pairs([("text:name", "kept")]),
        children: Vec::new(),
    };
    let doc = doc_with(vec![chars("a"), TextPart::OdfRaw(Box::new(RawNode::Element(raw))), chars("b")], Vec::new());
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("<text:p>a<text:bookmark text:name=\"kept\"/>b</text:p>"), "{}", content_of(&bytes));
    assert_eq!(characters(first_shape(&back)), "ab");
}

#[test]
fn a_run_s_cell_that_has_no_attribute_of_a_text_style_here_is_carried() {
    let parts = vec![TextPart::Cp(1), chars("x")];
    let doc = doc_with(parts, vec![section("Character", vec![row(1, &[("Style", 1.0), ("Perpendicular", 1.0)])])]);
    let (bytes, back, _) = round(&doc);
    // The bold is in the file, and the cell the file has no attribute for is in DiaCraft's data.
    assert!(content_of(&bytes).contains("fo:font-weight=\"bold\""), "{}", content_of(&bytes));
    assert_eq!(cells_of(first_shape(&back), "Character", 1), cells_of(first_shape(&doc), "Character", 1));
}

#[test]
fn a_paragraph_s_cell_that_has_no_attribute_of_a_paragraph_style_here_is_carried() {
    let parts = vec![TextPart::Pp(1), chars("x")];
    let doc = doc_with(parts, vec![section("Paragraph", vec![row(1, &[("HorzAlign", 1.0), ("Perpendicular", 1.0)])])]);
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("fo:text-align=\"center\""), "{}", content_of(&bytes));
    assert_eq!(cells_of(first_shape(&back), "Paragraph", 1), cells_of(first_shape(&doc), "Paragraph", 1));
}

#[test]
fn a_run_s_font_that_is_a_number_of_the_document_s_table_of_faces_other_than_0_is_carried() {
    let parts = vec![TextPart::Cp(1), chars("x")];
    let doc = doc_with(parts, vec![section("Character", vec![row(1, &[("Font", 7.0), ("Style", 1.0)])])]);
    let (_, back, _) = round(&doc);
    assert_eq!(cells_of(first_shape(&back), "Character", 1), cells_of(first_shape(&doc), "Character", 1));
}

#[test]
fn a_bullet_s_font_that_is_a_number_of_the_document_s_table_of_faces_other_than_0_is_carried() {
    let parts = vec![TextPart::Pp(1), chars("x")];
    let doc = doc_with(parts, vec![section("Paragraph", vec![row(1, &[("Bullet", 1.0), ("BulletFont", 5.0)])])]);
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("text:list-level-style-bullet"), "{}", content_of(&bytes));
    assert_eq!(cells_of(first_shape(&back), "Paragraph", 1), cells_of(first_shape(&doc), "Paragraph", 1));
}

#[test]
fn a_run_s_or_a_paragraph_s_cell_with_a_value_its_attribute_cannot_hold_is_carried() {
    let parts = vec![TextPart::Cp(1), chars("x")];
    let doc = doc_with(parts, vec![section("Character", vec![row(1, &[("Case", 9.0), ("Style", 1.0)])])]);
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("fo:font-weight=\"bold\""));
    assert_eq!(cells_of(first_shape(&back), "Character", 1), cells_of(first_shape(&doc), "Character", 1));
}

#[test]
fn a_run_s_or_a_paragraph_s_cell_that_holds_a_formula_is_written_by_its_value_and_carried_by_its_formula() {
    let mut r = row(1, &[("Style", 1.0)]);
    r.cells
        .insert(CellName::new("Size"), Cell::read(Some("THEMEVAL(\"FontSize\")"), Some(&(14.0f64 / 72.0).to_string()), Some("PT"), None, &mut 1_000));
    let doc = doc_with(vec![TextPart::Cp(1), chars("x")], vec![section("Character", vec![r])]);
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("fo:font-size=\"14pt\""), "the cached value is in the file: {}", content_of(&bytes));
    let formula = |d: &Document| {
        first_shape(d)
            .sheet
            .sections
            .iter()
            .find(|s| s.name.as_str() == "Character")
            .and_then(|s| s.rows.iter().find(|r| r.ix == Some(1)))
            .and_then(|r| r.cells.get("Size"))
            .and_then(|c| c.formula.text().map(str::to_owned))
    };
    assert_eq!(formula(&back), Some("THEMEVAL(\"FontSize\")".to_owned()));
}

#[test]
fn a_bullet_that_is_more_than_one_character_is_carried() {
    let mut r = row(1, &[("Bullet", 1.0)]);
    r.cells.insert(CellName::new("BulletStr"), Cell::constant(Value::from_text("ab")));
    let doc = doc_with(vec![TextPart::Pp(1), chars("x")], vec![section("Paragraph", vec![r])]);
    let (bytes, back, _) = round(&doc);
    assert!(content_of(&bytes).contains("text:bullet-char=\"a\""), "{}", content_of(&bytes));
    assert_eq!(cells_of(first_shape(&back), "Paragraph", 1), cells_of(first_shape(&doc), "Paragraph", 1));
}

#[test]
fn a_text_format_set_on_a_shape_that_has_no_character_to_carry_it_is_carried() {
    // A row no paragraph or run of the text uses: the file would read the shape as having none.
    let doc = doc_with(vec![chars("x")], vec![section("Character", vec![row(3, &[("Style", 1.0)])])]);
    let (_, back, _) = round(&doc);
    assert_eq!(cells_of(first_shape(&back), "Character", 3), cells_of(first_shape(&doc), "Character", 3));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ hostile input

#[test]
fn a_list_nested_deeper_than_the_files_diacraft_reads_allow_is_refused_in_words() {
    fn nested(depth: usize) -> Drawing {
        let open = r##"<text:list><text:list-item>"##.repeat(depth);
        let close = r##"</text:list-item></text:list>"##.repeat(depth);
        let mut d = Drawing::a4().page(&shape_of(&format!("{open}<text:p>deep</text:p>{close}")));
        d.version = "1.4".to_owned();
        d
    }
    // The deepest a file of the reader's default depth can hold is read, written and read again.
    let at_limit = model_of(&nested(90));
    let (bytes, back, _) = round(&at_limit);
    assert_eq!(characters(first_shape(&back)), "deep");
    assert_eq!(content_of(&bytes).matches("<text:list>").count(), 90);
    // One list past what the writer allows is refused, in words, and writes nothing.
    let mut past = at_limit.clone();
    {
        let page = Arc::make_mut(past.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let text = Arc::make_mut(shape.text.as_mut().unwrap());
        let odf = &mut text.ext.make_mut().odf;
        let first = odf.paragraphs[0].levels[0].clone();
        odf.paragraphs[0].levels.extend(std::iter::repeat_n(first, 300));
    }
    let Err(refused) = write_with(&past, None, &WriteOptions::default()) else { panic!("the list is refused") };
    assert!(refused.to_string().contains("list"), "{refused}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ the measurement

/// The text of every kind this task writes, as models, for the report: for each, whether the model writer refuses it, and whether DiaCraft's data carries any of its text. Run by hand.
#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure_text() {
    let mut cases: Vec<(String, Document)> = vectors().into_iter().map(|(n, d)| (format!("file: {n}"), model_of(&d))).collect();
    let rows = |name: &str, r: Vec<Row>| vec![section(name, r)];
    cases.push(("made: a carriage return".to_owned(), doc_with(vec![chars("a\r\nb")], Vec::new())));
    cases.push(("made: runs".to_owned(), doc_with(vec![TextPart::Cp(1), chars("b")], rows("Character", vec![row(1, &[("Style", 1.0)])]))));
    cases.push((
        "made: paragraph formatting".to_owned(),
        doc_with(vec![TextPart::Pp(1), chars("b")], rows("Paragraph", vec![row(1, &[("HorzAlign", 1.0)])])),
    ));
    cases
        .push(("made: a bulleted list".to_owned(), doc_with(vec![TextPart::Pp(1), chars("b")], rows("Paragraph", vec![row(1, &[("Bullet", 1.0)])]))));
    cases.push(("made: a page number field".to_owned(), field_doc("PAGENUMBER()", "1")));
    cases.push(("made: a page name field".to_owned(), field_doc("PAGENAME()", "Page-1")));
    cases.push(("made: a tab stop marker".to_owned(), doc_with(vec![chars("a"), TextPart::Tp(0), chars("b")], Vec::new())));
    let (mut refused, mut carried, mut native) = (0, 0, 0);
    for (name, doc) in &cases {
        match write_with(doc, None, &WriteOptions::default()) {
            Err(e) => {
                refused += 1;
                println!("TEXT {name:<44} refused: {e}");
            }
            Ok(w) => {
                let xml = content_of(&w.bytes);
                let tier2 = xml.contains("diacraft:text")
                    || xml.contains("diacraft:s-Character")
                    || xml.contains("diacraft:s-Paragraph")
                    || xml.contains("diacraft:s-Field");
                if tier2 {
                    carried += 1
                } else {
                    native += 1
                }
                println!(
                    "TEXT {name:<44} written, {} {:?}",
                    if tier2 { "text carried in DiaCraft's data" } else { "text said by the file" },
                    w.notes.iter().map(SaveNote::class).collect::<Vec<_>>()
                );
            }
        }
    }
    println!(
        "TEXTS {} cases: {refused} refused, {carried} written with the text carried, {native} written with the text said by the file",
        cases.len()
    );
}

/// A black-box check against LibreOffice, run by hand (ignored): writes each vector as the file holds it (`orig_NN.odg`, the hand-built package) and as the model writer writes it after a visit
/// (`new_NN.odg`), and the models DiaCraft makes (`made_NN.odg`), into `$ODG_OUT_DIR/in`. A script then converts each with
/// `soffice -env:UserInstallation=file://$ODG_OUT_DIR/profile --headless --convert-to fodg` and compares the text LibreOffice reads from the two.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_OUT_DIR set to an empty folder"]
fn lo_model_text() {
    let dir = std::path::PathBuf::from(std::env::var("ODG_OUT_DIR").expect("set ODG_OUT_DIR to an empty folder")).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, (_, drawing)) in vectors().into_iter().enumerate() {
        let mut drawing = drawing;
        drawing.version = "1.3".to_owned();
        // LibreOffice 26.8.2.2 does not load a drawing whose shape text holds a bookmark (measured on the hand-built original, whoever writes it): the bookmarks are left out of this check.
        while let Some(at) = drawing.pages.find("<text:bookmark") {
            let end = drawing.pages[at..].find("/>").map(|e| at + e + 2).unwrap();
            drawing.pages.replace_range(at..end, "");
        }
        let bytes = package(DRAWING, drawing.files());
        std::fs::write(dir.join(format!("orig_{n:02}.odg")), &bytes).unwrap();
        let (doc, _) = read(&bytes, &Limits::default()).unwrap();
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
        std::fs::write(dir.join(format!("new_{n:02}.odg")), &written.bytes).unwrap();
    }
    let rows = |name: &str, r: Vec<Row>| vec![section(name, r)];
    let made = [
        doc_with(vec![chars("a\r\nb\rc")], Vec::new()),
        doc_with(
            vec![chars("plain "), TextPart::Cp(1), chars("bold"), TextPart::Cp(0), chars(" end")],
            rows("Character", vec![row(1, &[("Style", 1.0)])]),
        ),
        doc_with(
            vec![TextPart::Pp(1), chars("one\ntwo\n"), TextPart::Pp(0), chars("three")],
            rows("Paragraph", vec![row(1, &[("HorzAlign", 1.0), ("Bullet", 1.0)])]),
        ),
        field_doc("PAGENUMBER()", "3"),
        field_doc("PAGENAME()", "Page-1"),
    ];
    for (n, doc) in made.iter().enumerate() {
        let written = write_with(doc, None, &WriteOptions::default()).unwrap();
        std::fs::write(dir.join(format!("made_{n:02}.odg")), &written.bytes).unwrap();
    }
}
