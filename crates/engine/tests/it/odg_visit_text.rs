//! Through the engine (PL5's second leg): an `.odg` is opened, Save As `.diacraft`, a new session opens that file, Save As `.odg`, and the content check judges the file that
//! came back against the file that was opened, in the two parts that say what the drawing is (`content.xml` and `styles.xml`). A second visit of what came back is the same bytes (clock
//! pinned). The crate-level tests of the same classes are `crates/odg/tests/it/visit_text.rs`.

use serde_json::json;

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{GR1, GR2, P1, part_of};
use crate::run;

/// The parts a visit is judged by.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// A drawing as ODF 1.4, the version the model writer writes, whose automatic styles are `auto` beside the ones the builder gives it, whose common styles are `common` and whose page holds a text frame
/// of `paragraphs`.
fn source(common: &str, auto: &str, paragraphs: &str) -> Vec<u8> {
    source_on("", common, auto, paragraphs)
}

/// [`source`] with the frame on the automatic style `style` (none when empty).
fn source_on(style: &str, common: &str, auto: &str, paragraphs: &str) -> Vec<u8> {
    let named = if style.is_empty() { String::new() } else { format!(r#"draw:style-name="{style}" "#) };
    let frame = format!(
        r#"<draw:frame {named}draw:layer="layout" svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box>{paragraphs}</draw:text-box></draw:frame>"#
    );
    let mut drawing = crate::odg_build::Drawing::new().page(&frame);
    drawing.auto = format!("{GR1}{GR2}{P1}{auto}");
    drawing.common = common.to_owned();
    let (content, styles) = (
        drawing.content_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
        drawing.styles_xml().replace("office:version=\"1.3\"", "office:version=\"1.4\""),
    );
    let files = vec![
        ("content.xml".to_owned(), content.into_bytes()),
        ("styles.xml".to_owned(), styles.into_bytes()),
        ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
    ];
    crate::odg_build::package(crate::odg_build::DRAWING, files)
}

/// What the content check says of the drawing that went through `.diacraft` and came back, in the drawing's two parts, and the file that came back.
fn visit(dir: &Dir, source: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    if let Err(e) = t.execute("file.save_as", &json!({ "path": dir.path("w.odg") })) {
        return Err(format!("{e}"));
    }
    let written = dir.read("w.odg");
    let diff = diacraft_odg::compare_content(source, &written, &diacraft_odg::Limits::default()).unwrap();
    let shown =
        diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    Ok((written, shown))
}

/// The visit is the same content, and a visit of what came back is the same bytes.
fn assert_same(what: &str, source: &[u8]) -> Vec<u8> {
    let (written, found) = visit(&Dir::new(), source).unwrap_or_else(|e| panic!("{what}: refused: {e}"));
    assert!(found.is_empty(), "{what}: the visit differs from the file: {found:#?}");
    let (again, found) = visit(&Dir::new(), &written).unwrap_or_else(|e| panic!("{what}: the second visit is refused: {e}"));
    assert!(found.is_empty(), "{what}: the second visit differs from the file: {found:#?}");
    assert_eq!(again, written, "{what}: a visit of what came back is the same bytes (clock pinned)");
    written
}

fn text_style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="text"><style:text-properties {attrs}/></style:style>"#)
}

fn paragraph_style(name: &str, paragraph: &str, text: &str) -> String {
    format!(
        r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties {paragraph}/><style:text-properties {text}/></style:style>"#
    )
}

fn span(style: &str, text: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{text}</text:span>"#)
}

/// A bold span comes back with the weight alone, and the text is the file's own, not DiaCraft's data.
#[test]
fn a_bold_span_comes_back_as_the_file_had_it() {
    let auto = format!("{}{}", text_style("T1", r#"fo:font-weight="bold""#), text_style("T2", r#"fo:font-style="italic" fo:font-size="18pt""#));
    let written =
        assert_same("bold and italic", &source("", &auto, &format!("<text:p>a {} b {} c</text:p>", span("T1", "bold"), span("T2", "italic"))));
    let content = part_of(&written, "content.xml");
    assert!(content.contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "the weight alone: {content}");
    assert!(!content.contains("diacraft:text"), "the text is the file's: {content}");
}

/// A span and a paragraph whose styles hold only attributes no cell reads keep their spans and names.
#[test]
fn a_style_of_attributes_no_cell_reads_comes_back() {
    let auto = format!(
        "{}{}",
        text_style("T1", r#"fo:language="en" fo:country="US""#),
        r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:orphans="2"/></style:style>"#
    );
    let written = assert_same(
        "unread attributes",
        &source("", &auto, &format!(r#"<text:p>Hello {}</text:p><text:p text:style-name="P2">second</text:p>"#, span("T1", "world"))),
    );
    let content = part_of(&written, "content.xml");
    assert!(
        content.contains(r#"<text:span text:style-name="T1">world</text:span>"#)
            && content.contains(r#"<text:p text:style-name="P2">second</text:p>"#),
        "{content}"
    );
}

/// A paragraph style that holds text properties comes back as the paragraph style, with no span round its text and none of the text carried.
#[test]
fn a_paragraph_style_of_text_properties_comes_back_as_the_paragraph_style() {
    let auto =
        format!("{}{}", paragraph_style("P2", r#"fo:text-align="center""#, r#"fo:font-size="20pt""#), text_style("T1", r#"fo:font-weight="bold""#));
    let written = assert_same(
        "a paragraph with text properties",
        &source("", &auto, &format!(r#"<text:p text:style-name="P2">Hello {}</text:p><text:p>plain</text:p>"#, span("T1", "bold"))),
    );
    let content = part_of(&written, "content.xml");
    assert!(!content.contains("diacraft:text"), "{content}");
    assert_eq!(content.matches("<text:span").count(), 1, "the bold span only: {content}");
}

/// The face, the colour and the size a span reads through the shape's common style are not written into the span's style.
#[test]
fn what_a_span_reads_through_its_chain_is_not_written_into_its_style() {
    let common = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:text-properties fo:font-size="18pt" fo:color="#112233"/></style:style>"##;
    let auto = format!(
        "{}{}",
        r#"<style:style style:name="gr9" style:family="graphic" style:parent-style-name="standard"/>"#,
        text_style("T1", r#"fo:font-weight="bold""#)
    );
    let written = assert_same("a chain", &source_on("gr9", common, &auto, &format!("<text:p>Hello {}</text:p>", span("T1", "bold"))));
    let content = part_of(&written, "content.xml");
    assert!(content.contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "{content}");
}
