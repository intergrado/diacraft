//! The text of an `.odg` comes back as it was from a visit through `.diacraft`.
//!
//! A visit is the crate-level one of PL5, as `visit_styles.rs` makes it: the drawing is read, the model goes through the whole-model codec, the model is written from the model alone with the content
//! stamp, and `compare_content` judges the file written against the file read, in `content.xml` and `styles.xml`. A second visit of what came back is the same bytes. Every read-back goes through
//! the real reader and every input is built in code from the ODF 1.4 Part 3 vocabulary. Each class has a test named for what comes back, and a control that an edit is still written from its cells.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{assert_same, part, source_of, through_codec, visit};

fn text_box(paragraphs: &str) -> String {
    format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box>{paragraphs}</draw:text-box></draw:frame>"#)
}

/// An automatic text style `name` whose text properties are `attrs`.
fn text_style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties {attrs}/></style:style>"#)
}

/// An automatic paragraph style `name` whose paragraph properties are `attrs`.
fn paragraph_style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties {attrs}/></style:style>"#)
}

/// An automatic paragraph style `name` whose properties are `paragraph` and, in the properties element of the text, `text`.
fn paragraph_style_with_text(name: &str, paragraph: &str, text: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties {paragraph}/><style:text-properties {text}/></style:style>"#
    )
}

fn span(style: &str, text: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{text}</text:span>"#)
}

/// The text of the one start tag of `content` that holds `style:name="name"`, to the end of its element.
fn style_of(content: &str, name: &str) -> String {
    let at = content.find(&format!(r#"style:name="{name}""#)).unwrap_or_else(|| panic!("no style {name}: {content}"));
    let from = content[..at].rfind("<style:style").unwrap_or(0);
    let end = content[at..].find("</style:style>").map(|e| at + e + "</style:style>".len()).unwrap_or(content.len());
    content[from..end].to_owned()
}

/// A visit that came back with no DiaCraft data for the text: the file says all of it.
fn assert_native(written: &[u8]) {
    let content = part(written, "content.xml");
    assert!(!content.contains("diacraft:text"), "the text rides in DiaCraft's data: {content}");
}

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The first shape's `Character` row `ix`, edited: the cell `name` set to the number `value`.
fn edit_run_cell(doc: &mut Document, ix: u32, name: &str, value: f64) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(ix), name), Cell::constant(Value::from_number(value).unwrap())).unwrap();
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 3: a style of attributes no cell reads

/// The text half of `visit_styles::a_graphic_style_of_attributes_no_cell_reads_comes_back`'s class (the cause was `held_style`): a run or a paragraph whose automatic style holds only attributes no
/// cell reads comes back with its span, its name and its style.
#[test]
fn a_text_style_of_attributes_no_cell_reads_comes_back() {
    let written = assert_same(
        "a span of a language",
        &Drawing::a4()
            .auto(&text_style("T1", r#"fo:language="en" fo:country="US""#))
            .page(&text_box(&format!("<text:p>Hello {}</text:p>", span("T1", "world")))),
    );
    assert_native(&written);
    let content = part(&written, "content.xml");
    assert!(content.contains(r#"<text:span text:style-name="T1">world</text:span>"#), "{content}");
    assert!(style_of(&content, "T1").contains(r#"fo:language="en""#), "{content}");
    assert_same(
        "a paragraph that is all one span",
        &Drawing::a4().auto(&text_style("T1", r#"fo:language="en""#)).page(&text_box(&format!("<text:p>{}</text:p>", span("T1", "world")))),
    );
    let written = assert_same(
        "a paragraph style",
        &Drawing::a4().auto(&paragraph_style("P1", r#"fo:orphans="2""#)).page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#)),
    );
    assert_native(&written);
    assert!(part(&written, "content.xml").contains(r#"<text:p text:style-name="P1">Hello</text:p>"#));
}

/// A span of unread attributes between two others, inside a paragraph whose other spans write theirs, and two paragraphs of one such style.
#[test]
fn spans_of_unread_attributes_among_spans_that_write_theirs_come_back() {
    let auto = format!(
        "{}{}{}{}",
        text_style("T1", r#"fo:language="en""#),
        text_style("T2", r#"fo:font-weight="bold""#),
        text_style("T3", r#"fo:hyphenate="true""#),
        paragraph_style("P1", r#"fo:text-align="center" fo:orphans="2""#)
    );
    let paras = format!(
        r#"<text:p text:style-name="P1">a {} b {} c {} d</text:p><text:p text:style-name="P1">{}</text:p>"#,
        span("T1", "one"),
        span("T2", "two"),
        span("T3", "three"),
        span("T1", "again")
    );
    let written = assert_same("mixed", &Drawing::a4().auto(&auto).page(&text_box(&paras)));
    assert_native(&written);
    let content = part(&written, "content.xml");
    assert_eq!(content.matches(r#"style:name="T1""#).count(), 1, "one style for the name: {content}");
    assert_eq!(content.matches(r#"text:style-name="T1""#).count(), 2, "{content}");
}

/// The span has the cells of its neighbours (its style gives nothing a cell says), and the neighbours are on a row that writes: the span is not a stretch of its own in the model.
#[test]
fn a_span_of_unread_attributes_inside_text_the_paragraph_style_makes_bold_comes_back() {
    let auto = format!(
        "{}{}",
        paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-weight="bold""#),
        text_style("T1", r#"fo:language="en""#)
    );
    let written = assert_same(
        "a span in bold text",
        &Drawing::a4().auto(&auto).page(&text_box(&format!(r#"<text:p text:style-name="P1">Hello {} !</text:p>"#, span("T1", "world")))),
    );
    assert_native(&written);
}

/// A span that names a common style that says nothing a cell reads.
#[test]
fn a_span_of_a_common_style_of_unread_attributes_comes_back() {
    let common = r#"<style:style style:name="Lang" style:family="text"><style:text-properties fo:language="de"/></style:style>"#;
    let written = assert_same(
        "a common text style",
        &Drawing::a4().common(common).page(&text_box(&format!("<text:p>Hello {}</text:p>", span("Lang", "Welt")))),
    );
    assert_native(&written);
    assert!(part(&written, "content.xml").contains(r#"<text:span text:style-name="Lang">Welt</text:span>"#));
}

/// The control: a span whose style writes nothing a cell reads, and whose row an edit changed, is written from its cells; the paragraph's other spans keep their styles.
#[test]
fn a_span_of_unread_attributes_whose_row_was_edited_is_written_from_its_cells() {
    let auto = format!("{}{}", text_style("T1", r#"fo:language="en""#), text_style("T2", r#"fo:font-weight="bold""#));
    let source =
        source_of(&Drawing::a4().auto(&auto).page(&text_box(&format!("<text:p>a {} b {} c</text:p>", span("T1", "one"), span("T2", "two")))));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    // The bold run's size is edited (its row is the one with a `Style` cell).
    let ix = {
        let sheet = &doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet;
        let section = sheet.section(&diacraft_doc::SectionKey::new("Character", None)).unwrap();
        section.rows.iter().find(|r| r.cells.contains_key("Style")).and_then(|r| r.ix).unwrap()
    };
    edit_run_cell(&mut doc, ix, "Size", 0.5);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains(r#"style:name="T1""#) && content.contains(r#"fo:language="en""#), "the span nothing edited keeps its style: {content}");
    assert!(content.contains("36pt"), "the edited run's size is written from the cell: {content}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 1: a run style writes what the file had

/// A bold span writes the weight alone, as the file had it, and not the size and the four attributes its cells also say.
#[test]
fn a_bold_span_comes_back_with_the_attributes_its_style_had() {
    let source =
        |attrs: &str| Drawing::a4().auto(&text_style("T1", attrs)).page(&text_box(&format!("<text:p>Hello {}</text:p>", span("T1", "world"))));
    let written = assert_same("bold", &source(r#"fo:font-weight="bold""#));
    assert_native(&written);
    let content = part(&written, "content.xml");
    assert!(style_of(&content, "T1").contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "the weight alone: {content}");
    for (what, attrs) in [
        ("italic", r#"fo:font-style="italic""#),
        ("a numeric weight", r#"fo:font-weight="700""#),
        ("an oblique style", r#"fo:font-style="oblique""#),
        ("the complex script's weight alone", r#"style:font-weight-complex="bold""#),
        ("small caps", r#"fo:font-variant="small-caps""#),
        ("bold italic", r#"fo:font-weight="bold" fo:font-style="italic""#),
        ("a size and a weight", r#"fo:font-size="18pt" fo:font-weight="bold""#),
        ("a weight and a language", r#"fo:font-weight="bold" fo:language="en""#),
        (
            "the whole group in the forms the writer uses",
            r#"fo:font-weight="bold" fo:font-style="normal" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal""#,
        ),
    ] {
        let written = assert_same(what, &source(attrs));
        assert_native(&written);
    }
}

/// The size a span's style does not state is not written from the row's `Size`, which every row of a run holds; a size it states in another form than the writer's comes back as stated.
#[test]
fn the_size_a_span_style_states_comes_back_as_it_was_and_none_is_made_up() {
    let source =
        |attrs: &str| Drawing::a4().auto(&text_style("T1", attrs)).page(&text_box(&format!("<text:p>Hello {}</text:p>", span("T1", "world"))));
    let written = assert_same("a size in points", &source(r#"fo:font-size="18pt""#));
    assert!(style_of(&part(&written, "content.xml"), "T1").contains(r#"fo:font-size="18pt""#));
    let written = assert_same("a size in inches", &source(r#"fo:font-size="0.25in""#));
    assert!(style_of(&part(&written, "content.xml"), "T1").contains(r#"fo:font-size="0.25in""#), "as stated");
    let written = assert_same("a size in percent", &source(r#"fo:font-size="150%""#));
    assert!(style_of(&part(&written, "content.xml"), "T1").contains(r#"fo:font-size="150%""#), "as stated");
    let written = assert_same("no size", &source(r#"fo:font-style="italic""#));
    assert!(!style_of(&part(&written, "content.xml"), "T1").contains("font-size"), "none made up");
}

/// A paragraph with a bold span and an italic one and text between, and a second paragraph of the same styles.
#[test]
fn spans_of_two_styles_in_two_paragraphs_come_back() {
    let auto = format!("{}{}", text_style("T1", r#"fo:font-weight="bold""#), text_style("T2", r#"fo:font-style="italic""#));
    let paras =
        format!("<text:p>a {} b {} c</text:p><text:p>{}{}</text:p>", span("T1", "bold"), span("T2", "italic"), span("T2", "x"), span("T1", "y"));
    assert_native(&assert_same("two", &Drawing::a4().auto(&auto).page(&text_box(&paras))));
}

/// The control: a run the user changed is written from its cells, all of the attributes a group says, and the run beside it keeps its style as the file had it.
#[test]
fn a_run_an_edit_changed_is_written_from_its_cells_and_the_run_beside_it_is_as_it_was() {
    let auto = format!("{}{}", text_style("T1", r#"fo:font-weight="bold""#), text_style("T2", r#"fo:font-style="italic""#));
    let source =
        source_of(&Drawing::a4().auto(&auto).page(&text_box(&format!("<text:p>a {} b {} c</text:p>", span("T1", "one"), span("T2", "two")))));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    let bold = {
        let sheet = &doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet;
        let section = sheet.section(&diacraft_doc::SectionKey::new("Character", None)).unwrap();
        section
            .rows
            .iter()
            .find(|r| r.cells.get("Style").and_then(|c| c.value.as_ref()).and_then(Value::number) == Some(1.0))
            .and_then(|r| r.ix)
            .unwrap()
    };
    edit_run_cell(&mut doc, bold, "Style", 3.0);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    // The edited run says the weight and the style of its cell, the whole group.
    assert!(
        content.contains(r#"fo:font-weight="bold" fo:font-style="italic""#) || content.matches(r#"fo:font-style="italic""#).count() >= 2,
        "{content}"
    );
    // The italic run, which nothing edited, says the style alone.
    assert!(style_of(&content, "T2").contains(r#"<style:text-properties fo:font-style="italic"/>"#), "{content}");
}

/// The row of a run holds every cell its chain resolves, the face, the colour and the size among them: a span whose style stated none of them is not given them, and a drawing whose shape style
/// gives them writes them once, in the style that gives them.
#[test]
fn what_a_span_reads_through_its_chain_is_not_written_into_its_style() {
    let common = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt" fo:color="#112233" fo:font-weight="normal" fo:language="en"/></style:style>"##;
    let mut drawing = Drawing::a4()
        .common(common)
        .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"/>"#)
        .auto(&text_style("T1", r#"fo:font-weight="bold""#))
        .auto(&text_style("T2", r##"fo:font-style="italic" fo:color="#445566""##))
        .page(&format!(
            r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>Hello {} and {}</text:p><text:p>second</text:p></draw:rect>"#,
            span("T1", "bold"),
            span("T2", "italic")
        ));
    drawing.fonts = r#"<style:font-face style:name="Liberation Sans" svg:font-family="&apos;Liberation Sans&apos;"/>"#.to_owned();
    let written = assert_same("a chain that gives the face, the colour and the size", &drawing);
    let content = part(&written, "content.xml");
    assert_eq!(style_of(&content, "T1").matches("style:text-properties").count(), 1);
    assert!(style_of(&content, "T1").contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "the weight alone: {content}");
    assert!(style_of(&content, "T2").contains(r##"fo:color="#445566""##) && !style_of(&content, "T2").contains("font-name"), "{content}");
    assert_eq!(content.matches("font-name").count(), 0, "the face is the shape style's: {content}");
}

/// A colour in capitals, and a size in the units the writer does not use, come back as the file spelled them.
#[test]
fn a_colour_and_a_scale_come_back_as_the_file_spelled_them() {
    let source =
        |attrs: &str| Drawing::a4().auto(&text_style("T1", attrs)).page(&text_box(&format!("<text:p>Hello {}</text:p>", span("T1", "world"))));
    let written = assert_same("a colour in capitals", &source(r##"fo:color="#FF0000""##));
    assert!(style_of(&part(&written, "content.xml"), "T1").contains(r##"fo:color="#FF0000""##));
    assert_same("a scale", &source(r#"style:text-scale="150%""#));
    assert_same(
        "an underline",
        &source(r##"style:text-underline-style="solid" style:text-underline-type="single" style:text-underline-color="#ff0000""##),
    );
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 2: a paragraph style that holds text properties

/// The class "written differently": a paragraph whose automatic style holds text properties came back as a text carried in DiaCraft's data and a span of a new style round it. Now the paragraph
/// names its style, which holds the properties as the file had them.
#[test]
fn a_paragraph_style_that_holds_text_properties_comes_back_as_the_paragraph_style() {
    for (what, text) in [
        ("a size", r#"fo:font-size="20pt""#),
        ("a weight", r#"fo:font-weight="bold""#),
        ("a size and a weight", r#"fo:font-size="20pt" fo:font-weight="bold""#),
        ("a colour", r##"fo:color="#ff0000""##),
        ("a language", r#"fo:language="en""#),
    ] {
        let auto = paragraph_style_with_text("P1", r#"fo:text-align="center""#, text);
        let written = assert_same(what, &Drawing::a4().auto(&auto).page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#)));
        assert_native(&written);
        let content = part(&written, "content.xml");
        assert!(content.contains(r#"<text:p text:style-name="P1">Hello</text:p>"#), "{what}: no span round the text: {content}");
        assert!(!content.contains("<text:span"), "{what}: {content}");
    }
    // Only text properties: the style has no paragraph properties element.
    let auto = r#"<style:style style:name="P1" style:family="paragraph"><style:text-properties fo:font-size="20pt"/></style:style>"#;
    assert_native(&assert_same("text properties alone", &Drawing::a4().auto(auto).page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#))));
}

/// The paragraph has spans of its own, which read through the paragraph's properties too (their rows hold the size the paragraph's style gives).
#[test]
fn a_paragraph_style_of_text_properties_and_the_spans_inside_it_come_back() {
    let auto = format!(
        "{}{}{}",
        paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-size="20pt""#),
        text_style("T1", r#"fo:font-weight="bold""#),
        text_style("T2", r#"fo:font-size="30pt""#)
    );
    let paras = format!(
        r#"<text:p text:style-name="P1">Hello {} and {} done</text:p><text:p text:style-name="P1">{}</text:p>"#,
        span("T1", "bold"),
        span("T2", "big"),
        span("T1", "b")
    );
    let written = assert_same("spans in a paragraph with text properties", &Drawing::a4().auto(&auto).page(&text_box(&paras)));
    assert_native(&written);
    let content = part(&written, "content.xml");
    assert!(!style_of(&content, "T1").contains("font-size"), "the bold span does not state the size its paragraph gives it: {content}");
}

/// Two paragraphs on two paragraph styles with text properties, and a third with none.
#[test]
fn paragraphs_of_different_styles_come_back_each_with_its_own() {
    let auto = format!(
        "{}{}{}",
        paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-size="20pt""#),
        paragraph_style_with_text("P2", r#"fo:text-align="end""#, r#"fo:font-weight="bold""#),
        paragraph_style("P3", r#"fo:text-align="start""#)
    );
    let paras = r#"<text:p text:style-name="P1">one</text:p><text:p text:style-name="P2">two</text:p><text:p text:style-name="P3">three</text:p><text:p>four</text:p>"#;
    assert_native(&assert_same("three styles", &Drawing::a4().auto(&auto).page(&text_box(paras))));
}

/// A paragraph style that names a face the part declares, and the colour and the underline of a paragraph.
#[test]
fn a_paragraph_style_that_names_a_declared_face_comes_back() {
    let mut drawing = Drawing::a4().auto(&paragraph_style_with_text(
        "P1",
        r#"fo:text-align="center""#,
        r##"style:font-name="Liberation Sans" fo:font-size="20pt" fo:color="#FF0000" style:text-underline-style="solid" style:text-underline-type="single""##,
    ));
    drawing.fonts = r#"<style:font-face style:name="Liberation Sans" svg:font-family="&apos;Liberation Sans&apos;"/>"#.to_owned();
    let written = assert_same("a face", &drawing.page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#)));
    assert_native(&written);
    assert!(!part(&written, "content.xml").contains("<text:span"));
}

/// A weight ODF 1.4 lists no value for (an earlier version took any text) is written as the value it lists nearest, and the text the file had rides in DiaCraft's own attribute beside it, as the
/// held values of do: in a span's style that states it, and in the text properties of a paragraph style. A second visit gives the same bytes.
#[test]
fn a_weight_odf_1_4_lists_no_value_for_is_written_as_one_it_lists_and_comes_back() {
    let drawing = Drawing::a4()
        .auto(&text_style("T1", r#"fo:font-weight="650""#))
        .auto(&paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-weight="250""#))
        .page(&text_box(&format!(r#"<text:p text:style-name="P1">Hello {}</text:p>"#, span("T1", "world"))));
    let source = source_of(&drawing);
    let (written, _) = visit(&source).unwrap();
    let content = part(&written, "content.xml");
    assert!(!content.contains(r#"fo:font-weight="650""#) && !content.contains(r#"fo:font-weight="250""#), "{content}");
    assert!(
        content.contains(r#"diacraft:older-font-weight="650""#) && content.contains(r#"diacraft:older-font-weight="250""#),
        "the weights ride: {content}"
    );
    let (again, _) = visit(&written).unwrap();
    assert_eq!(again, written, "a second visit is the same bytes");
    let (doc, _) = read(&written, &Limits::default()).unwrap();
    let held: Vec<String> = doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .filter_map(|s| s.text.as_ref())
        .flat_map(|t| t.ext.get().map(|e| e.odf.clone()))
        .flat_map(|o| o.paragraphs)
        .flat_map(|p| p.style.held.into_iter().chain(p.runs.into_iter().flat_map(|r| r.style.held)))
        .filter_map(|h| h.attrs.get("fo:font-weight").map(str::to_owned))
        .collect();
    assert_eq!(held, ["250", "650"], "a read gives the weights back as the file had them");
}

/// A size ODF 1.4 refuses (an earlier version took 0) is not said back as the file spelled it: the writer writes the least size and carries the 0, in a span's style and in a
/// paragraph style's text properties.
#[test]
fn a_font_size_of_zero_is_not_written_back_as_it_was() {
    let drawing = Drawing::a4()
        .auto(&text_style("T1", r#"fo:font-size="0pt" fo:font-weight="bold""#))
        .auto(&paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-size="0pt""#))
        .page(&text_box(&format!(r#"<text:p text:style-name="P1">Hello {}</text:p>"#, span("T1", "world"))));
    let (written, _) = visit(&source_of(&drawing)).unwrap();
    let content = part(&written, "content.xml");
    assert!(!content.contains(r#"fo:font-size="0pt""#) && !content.contains(r#"fo:font-size="0in""#), "{content}");
}

/// The control: the text of a paragraph whose style holds text properties, formatted by an edit, is written from its cells (a span with the cells' attributes), and the paragraph keeps its style.
#[test]
fn text_an_edit_formatted_in_a_paragraph_with_text_properties_is_written_from_its_cells() {
    let auto = paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-size="20pt""#);
    let source = source_of(&Drawing::a4().auto(&auto).page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#)));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    let ix = {
        let sheet = &doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet;
        let section = sheet.section(&diacraft_doc::SectionKey::new("Character", None)).unwrap();
        section.rows.iter().find(|r| r.cells.contains_key("Size")).and_then(|r| r.ix).unwrap()
    };
    edit_run_cell(&mut doc, ix, "Size", 0.5);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains("36pt"), "the edited size is written: {content}");
    assert!(content.contains(r#"text:style-name="P1""#), "{content}");
}

/// The second visit of a visit is the same bytes for the vectors above that are not asserted by `assert_same`.
#[test]
fn a_second_visit_of_the_text_vectors_is_the_same_bytes() {
    let auto = format!(
        "{}{}{}",
        paragraph_style_with_text("P1", r#"fo:text-align="center""#, r#"fo:font-size="20pt""#),
        text_style("T1", r#"fo:font-weight="bold" fo:language="en""#),
        text_style("T2", r#"fo:hyphenate="true""#)
    );
    let paras = format!(r#"<text:p text:style-name="P1">Hello {} and {}</text:p>"#, span("T1", "bold"), span("T2", "x"));
    let source = source_of(&Drawing::a4().auto(&auto).page(&text_box(&paras)));
    let (first, found) = visit(&source).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let (second, found) = visit(&first).unwrap();
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(first, second);
    let (doc, _) = read(&first, &Limits::default()).unwrap();
    let again = write_with(&through_codec(&doc), None, &unedited()).unwrap();
    assert_eq!(again.bytes, first, "a write of the model read back is the same bytes");
}
