//! The styles of an `.odg` come back as they were from a visit through `.diacraft`.
//!
//! A visit is the crate-level one of PL5: the drawing is read, the model goes through the whole-model codec (the `.diacraft` path: `encode`, then `decode`), the model is written from
//! the model alone with the content stamp, and `compare_content` judges the file written against the file read, in the two parts that say what the drawing is (`content.xml` and
//! `styles.xml`). A second visit of what came back is the same bytes. Every read-back goes through the real reader, and every input is built in code from the ODF 1.4 Part 3 vocabulary.
//! Each class has a test named for what comes back, and a control that an edited style is still written from its cells.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_odg::{Limits, WriteOptions, compare_content, read, write_with};
use diacraft_sheetxml::model::{Budget, MemBlobs, decode, encode};

use crate::drawing::Drawing;
use crate::flat::Flat;

/// The parts a visit is judged by.
const DRAWING_PARTS: [&str; 2] = ["content.xml", "styles.xml"];

/// The model through the whole-model codec, as a `.diacraft` file holds it.
pub(crate) fn through_codec(doc: &Document) -> Document {
    let mut blobs = MemBlobs::new();
    let encoded = encode(doc, &mut blobs).unwrap_or_else(|e| panic!("encode: {e:?}"));
    let pages: Vec<&diacraft_container::xml::El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&diacraft_container::xml::El> = encoded.masters.iter().map(|(_, e)| e).collect();
    let mut budget = Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4);
    let mut notes = Vec::new();
    let back = decode(&encoded.document, &pages, &masters, &blobs, &mut budget, &mut notes).unwrap_or_else(|e| panic!("decode: {e:?}"));
    assert!(notes.is_empty(), "{notes:?}");
    back
}

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The package of `drawing` written as ODF 1.4, the version the model writer writes (the version is no class of this task).
pub(crate) fn source_of(drawing: &Drawing) -> Vec<u8> {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    drawing.bytes()
}

/// The differences of content between `source` and `written`, in the drawing's two parts.
fn differences(source: &[u8], written: &[u8]) -> Vec<String> {
    let diff = compare_content(source, written, &Limits::default()).unwrap_or_else(|e| panic!("compare: {e}"));
    diff.shown.iter().filter(|d| DRAWING_PARTS.contains(&d.part.as_str())).map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect()
}

/// The visit of `source`: the file written, and the differences of content from the source in the drawing's two parts. `Err` is a refusal or a failed read, in words.
pub(crate) fn visit(source: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (doc, _) = read(source, &Limits::default()).map_err(|e| format!("does not read: {e}"))?;
    let doc = through_codec(&doc);
    let written = write_with(&doc, None, &unedited()).map_err(|e| format!("refused: {e}"))?;
    let found = differences(source, &written.bytes);
    Ok((written.bytes, found))
}

/// The visit of `drawing` is the same content, and a second visit of its output is the same bytes (PL5). Gives the file written.
pub(crate) fn assert_same(what: &str, drawing: &Drawing) -> Vec<u8> {
    let source = source_of(drawing);
    let (written, found) = visit(&source).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(found.is_empty(), "{what}: the visit differs from the file: {found:#?}");
    let (again, found) = visit(&written).unwrap_or_else(|e| panic!("{what}: second visit: {e}"));
    assert!(found.is_empty(), "{what}: the second visit differs from the file: {found:#?}");
    assert_eq!(written, again, "{what}: a second visit is the same bytes");
    written
}

/// The text of a part of a package.
pub(crate) fn part(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap().to_vec()).unwrap()
}

/// A rectangle on the style `style`.
pub(crate) fn rect(style: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

pub(crate) const GR1: &str =
    r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;

/// A rectangle on an automatic style `gr1` whose parent is the common style `parent` (how a drawing program names a common style), and that style.
fn on_parent(parent: &str) -> (String, String) {
    (format!(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="{parent}"/>"#), rect("gr1"))
}

/// A drawing whose one rectangle is on the automatic style `gr1`, and whose common styles are `common`.
fn with_common(common: &str, parent: &str) -> Drawing {
    let (auto, page) = on_parent(parent);
    Drawing::a4().common(common).auto(&auto).page(&page)
}

fn text_box(para: &str) -> String {
    format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box>{para}</draw:text-box></draw:frame>"#)
}

fn edit_cell(doc: &mut Document, style: usize, section: Option<&str>, name: &str, value: Value) {
    let style = Arc::make_mut(doc.styles.get_mut(style).expect("a style sheet"));
    let sheet = Arc::make_mut(&mut style.sheet);
    let addr = match section {
        Some(section) => CellAddr::in_row(section, None, RowKey::Ix(0), name),
        None => CellAddr::direct(name),
    };
    sheet.set_cell(&addr, Cell::constant(value)).unwrap();
}

fn edit_shape_fill(doc: &mut Document, colour: &str) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).set_cell(&CellAddr::direct("FillForegnd"), Cell::constant(Value::from_text(colour))).unwrap();
}

/// The kept styles of a model that nothing names, as `family/name` (the page layouts and master pages the drawing holds are no such styles).
fn kept_styles(doc: &Document) -> Vec<String> {
    doc.ext
        .get()
        .map(|d| d.odf.styles.iter().filter(|s| s.family.starts_with("unused")).map(|s| format!("{}/{}", s.family, s.name)).collect())
        .unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 1: an automatic style nothing names

const UNUSED_GRAPHIC: &str =
    r##"<style:style style:name="gr9" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##;
const UNUSED_PARAGRAPH: &str =
    r#"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
const UNUSED_TEXT: &str = r##"<style:style style:name="T9" style:family="text"><style:text-properties fo:color="#ff0000"/></style:style>"##;

#[test]
fn an_automatic_style_nothing_names_comes_back() {
    let written = assert_same("a graphic style", &Drawing::a4().auto(GR1).auto(UNUSED_GRAPHIC).page(&rect("gr1")));
    assert!(part(&written, "content.xml").contains(r#"style:name="gr9""#), "under its name");
    let written =
        assert_same("a paragraph style and a text style", &Drawing::a4().auto(GR1).auto(UNUSED_PARAGRAPH).auto(UNUSED_TEXT).page(&rect("gr1")));
    let content = part(&written, "content.xml");
    assert!(content.contains(r#"style:name="P9""#) && content.contains(r#"style:name="T9""#), "{content}");
    assert_same(
        "a style with nothing in it",
        &Drawing::a4().auto(GR1).auto(r#"<style:style style:name="gr9" style:family="graphic"/>"#).page(&rect("gr1")),
    );
    assert_same("a drawing no shape of which has a style", &Drawing::a4().auto(UNUSED_GRAPHIC).page(&rect("")));
}

#[test]
fn an_automatic_style_of_styles_xml_nothing_names_comes_back_in_styles_xml() {
    let mut drawing = Drawing::a4().auto(GR1).page(&rect("gr1"));
    drawing.styles_auto.push_str(r##"<style:style style:name="gr8" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#0000ff"/></style:style>"##);
    let written = assert_same("a style of styles.xml", &drawing);
    assert!(part(&written, "styles.xml").contains(r#"style:name="gr8""#));
    assert!(!part(&written, "content.xml").contains(r#"style:name="gr8""#), "not in the other part");
}

/// A style a shape names is not held twice: the shape's style is written with the shape, and a style no shape names is written once, from what the model holds of it.
#[test]
fn a_style_a_shape_names_is_written_once_and_a_style_nothing_names_once() {
    let written = assert_same("both", &Drawing::a4().auto(GR1).auto(UNUSED_GRAPHIC).page(&format!("{}{}", rect("gr1"), rect("gr1"))));
    let content = part(&written, "content.xml");
    assert_eq!(content.matches(r#"style:name="gr1""#).count(), 1, "{content}");
    assert_eq!(content.matches(r#"style:name="gr9""#).count(), 1, "{content}");
    let (doc, _) = read(&source_of(&Drawing::a4().auto(GR1).auto(UNUSED_GRAPHIC).page(&rect("gr1"))), &Limits::default()).unwrap();
    assert_eq!(
        kept_styles(&doc),
        ["unused:graphic/gr9"],
        "the style no shape names is held whole, once, and the one a shape names holds nothing no cell reads"
    );
}

/// A style a text names is named: a paragraph and a run find theirs, and a style beside them is not.
#[test]
fn the_styles_of_a_text_are_named_and_a_style_beside_them_is_not() {
    let drawing = Drawing::a4()
        .auto(r##"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"##)
        .auto(r##"<style:style style:name="T1" style:family="text"><style:text-properties fo:color="#ff0000"/></style:style>"##)
        .auto(UNUSED_TEXT)
        .page(&text_box(r#"<text:p text:style-name="P1">Hello <text:span text:style-name="T1">world</text:span></text:p>"#));
    let (doc, _) = read(&source_of(&drawing), &Limits::default()).unwrap();
    assert_eq!(kept_styles(&doc), ["unused:text/T9"]);
}

/// The control: the style of a shape edited in DiaCraft is written from its cells, and the style nothing names is as it was.
#[test]
fn an_edited_shape_is_written_from_its_cells_and_the_style_nothing_names_stays() {
    let source = source_of(&Drawing::a4().auto(GR1).auto(UNUSED_GRAPHIC).page(&rect("gr1")));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    edit_shape_fill(&mut doc, "#ff00ff");
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains("#ff00ff"), "the shape's new fill: {content}");
    assert!(content.contains(r#"style:name="gr9""#) && content.contains("#00ff00"), "the style nothing names, as it was: {content}");
    assert!(!content.contains("#336699"), "the shape's old fill is gone: {content}");
}

/// A flat drawing comes back as the package that holds the same elements, and the styles nothing names with it.
#[test]
fn an_automatic_style_of_a_flat_drawing_nothing_names_comes_back() {
    let mut drawing = Drawing::a4().auto(GR1).auto(UNUSED_GRAPHIC).auto(UNUSED_PARAGRAPH).page(&rect("gr1"));
    drawing.version = "1.4".to_owned();
    let flat = Flat::of(&drawing).bytes();
    let (doc, _) = read(&flat, &Limits::default()).unwrap();
    assert_eq!(kept_styles(&doc), ["unused:graphic/gr9", "unused:paragraph/P9"]);
    let written = write_with(&through_codec(&doc), None, &unedited()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(content.contains(r#"style:name="gr9""#) && content.contains(r#"style:name="P9""#), "{content}");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 3: a style or a master page with no name

#[test]
fn a_style_with_no_name_comes_back_unnamed() {
    // An automatic style no name can ask for.
    let written = assert_same(
        "an automatic graphic style",
        &Drawing::a4()
            .auto(r##"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##)
            .page(&rect("")),
    );
    assert!(!part(&written, "content.xml").contains("style:name"), "no name is made up");
    // An automatic style of each of the other families.
    assert_same(
        "paragraph and text styles",
        &Drawing::a4()
            .auto(r#"<style:style style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style><style:style style:family="text"><style:text-properties fo:font-size="9pt"/></style:style>"#)
            .page(&rect("")),
    );
    // A common style with no name (closed before this task: its test is kept).
    let written = assert_same(
        "a common graphic style",
        &Drawing::a4()
            .common(r##"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##)
            .page(&rect("")),
    );
    assert!(!part(&written, "styles.xml").contains(r#"style:name="""#));
}

#[test]
fn a_master_page_with_no_name_comes_back_unnamed_and_a_reference_to_it_stays_a_reference() {
    // A master page nothing names (closed before this task: its test is kept).
    let mut unnamed = Drawing::a4().page(&rect(""));
    unnamed.masters.push_str(r#"<style:master-page style:page-layout-name="PM1"/>"#);
    assert_same("a master page no page names", &unnamed);
    // A page that names a master page that has no name.
    let mut named_by_a_page = Drawing::a4().page(&rect(""));
    named_by_a_page.pages = named_by_a_page.pages.replace(r#"draw:master-page-name="Default""#, r#"draw:master-page-name="""#);
    named_by_a_page.masters = named_by_a_page.masters.replace(r#"style:name="Default""#, "");
    let written = assert_same("a page on a master page with no name", &named_by_a_page);
    assert!(part(&written, "content.xml").contains(r#"draw:master-page-name="""#), "the reference is still a reference");
    assert!(!part(&written, "styles.xml").contains(r#"style:name="""#), "the master page has still no name: {}", part(&written, "styles.xml"));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 4: a style of attributes no field reads

#[test]
fn a_graphic_style_of_attributes_no_cell_reads_comes_back() {
    // Closed before this task: the style is written, and its test is kept.
    assert_same(
        "an automatic style",
        &Drawing::a4().auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:textarea-horizontal-align="justify"/></style:style>"#).page(&rect("gr1")),
    );
    let (auto, page) = on_parent("Plain");
    assert_same(
        "a common style",
        &Drawing::a4()
            .common(r#"<style:style style:name="Plain" style:family="graphic"><style:graphic-properties draw:textarea-horizontal-align="justify"/></style:style>"#)
            .auto(&auto)
            .page(&page),
    );
}

/// The text half of the class (handed, which closed it: the cause was `held_style` in `write/text/model.rs`, which returned no style when a run's or a paragraph's cells and
/// held values write nothing, and with it the span and the style name): a run or a paragraph whose automatic style holds only attributes no cell reads (`fo:language`, `fo:orphans`) comes back as
/// it was. `visit_text.rs` has the rest of the class.
#[test]
fn a_text_style_of_attributes_no_cell_reads_comes_back() {
    assert_same(
        "a text style",
        &Drawing::a4()
            .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:language="en" fo:country="US"/></style:style>"#)
            .page(&text_box(r#"<text:p>Hello <text:span text:style-name="T1">world</text:span></text:p>"#)),
    );
    assert_same(
        "a paragraph style",
        &Drawing::a4()
            .auto(r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:orphans="2"/></style:style>"#)
            .page(&text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#)),
    );
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 5: a text group written whole

#[test]
fn a_bold_style_keeps_only_the_attribute_it_had() {
    let bold = |text_properties: &str| {
        with_common(
            &format!(r#"<style:style style:name="Bold" style:family="graphic"><style:text-properties {text_properties}/></style:style>"#),
            "Bold",
        )
    };
    let written = assert_same("bold", &bold(r#"fo:font-weight="bold""#));
    let styles = part(&written, "styles.xml");
    assert!(styles.contains(r#"<style:text-properties fo:font-weight="bold"/>"#), "the weight alone: {styles}");
    assert!(!styles.contains("font-variant") && !styles.contains("font-style") && !styles.contains("-complex"), "{styles}");
    assert_same("italic", &bold(r#"fo:font-style="italic""#));
    assert_same("a numeric weight", &bold(r#"fo:font-weight="700""#));
    assert_same("an oblique style", &bold(r#"fo:font-style="oblique""#));
    assert_same("the complex script's weight alone", &bold(r#"style:font-weight-complex="bold""#));
    assert_same("small caps", &bold(r#"fo:font-variant="small-caps""#));
    // The five attributes, in the forms the writer uses, are what the cells write.
    assert_same(
        "the whole group",
        &bold(
            r#"fo:font-weight="bold" fo:font-style="normal" fo:font-variant="normal" style:font-weight-complex="normal" style:font-style-complex="normal""#,
        ),
    );
    // A paragraph style's text group.
    assert_same(
        "a paragraph style",
        &Drawing::a4()
            .common(r#"<style:style style:name="Pb" style:family="paragraph"><style:text-properties fo:font-weight="bold"/></style:style>"#)
            .page(&rect("")),
    );
}

/// A style that underlines and says nothing of the weight and the style does not say them (the `Style` cell holds the underline's bit too).
#[test]
fn an_underlined_style_does_not_say_the_weight_and_the_style() {
    let source = source_of(&with_common(
        r#"<style:style style:name="U1" style:family="graphic"><style:text-properties style:text-underline-style="solid" style:text-underline-type="single"/></style:style>"#,
        "U1",
    ));
    let (written, found) = visit(&source).unwrap();
    assert!(found.is_empty(), "{found:?}");
    let styles = part(&written, "styles.xml");
    assert!(!styles.contains("font-weight") && !styles.contains("font-style") && !styles.contains("font-variant"), "{styles}");
}

/// The control: a group an edit changed is written from the cells, all five of it, and not as the file spelled it.
#[test]
fn an_edited_group_is_written_from_the_cells() {
    let source = source_of(&with_common(
        r#"<style:style style:name="Bold" style:family="graphic"><style:text-properties fo:font-weight="bold"/></style:style>"#,
        "Bold",
    ));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let mut doc = through_codec(&doc);
    let at = doc.styles.iter().position(|s| s.name_u.as_deref() == Some("Bold")).unwrap();
    // Bold and italic: the group's cell changes, and the held spelling is no longer the cells'.
    edit_cell(&mut doc, at, Some("Character"), "Style", Value::from_number(3.0).unwrap());
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let styles = part(&written.bytes, "styles.xml");
    assert!(
        styles.contains(r#"fo:font-weight="bold""#) && styles.contains(r#"fo:font-style="italic""#) && styles.contains(r#"fo:font-variant="normal""#),
        "written from the cells: {styles}"
    );
    // And one nobody edited is not.
    let (again, _) = read(&source, &Limits::default()).unwrap();
    let kept = write_with(&through_codec(&again), None, &unedited()).unwrap();
    assert!(!part(&kept.bytes, "styles.xml").contains("font-variant"));
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 6: a name held twice

#[test]
fn two_common_styles_of_one_name_come_back() {
    let two = r##"<style:style style:name="Dup" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style><style:style style:name="Dup" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let written = assert_same("two common styles of a name", &with_common(two, "Dup"));
    let styles = part(&written, "styles.xml");
    assert_eq!(styles.matches(r#"style:name="Dup""#).count(), 2, "{styles}");
    // A style that follows them is numbered as it was (the reader numbers the common styles by their places).
    let after = format!(
        r##"{two}<style:style style:name="After" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##
    );
    assert_same("a style after the two", &with_common(&after, "After"));
}

/// The second automatic style of a name is found by no reference. It is kept under a name of its own, and nothing is refused: a style the save made is there once.
#[test]
fn the_second_automatic_style_of_a_name_is_kept_under_a_name_of_its_own() {
    let repeat = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;
    let source = source_of(&Drawing::a4().auto(GR1).auto(repeat).page(&rect("gr1")));
    let (written, _) = visit(&source).unwrap();
    let content = part(&written, "content.xml");
    assert!(content.contains("#336699") && content.contains("#ff0000"), "both styles are written: {content}");
    assert_eq!(content.matches(r#"style:name="gr1""#).count(), 1, "{content}");
    let (again, _) = visit(&written).unwrap();
    assert_eq!(written, again, "a second visit is the same bytes");
    // The name a repeat is given is not one the file holds.
    let green = r##"<style:style style:name="gr1_2" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##;
    let crowded = Drawing::a4().auto(GR1).auto(repeat).auto(green).page(&format!("{}{}", rect("gr1"), rect("gr1_2")));
    let (written, _) = visit(&source_of(&crowded)).unwrap();
    let content = part(&written, "content.xml");
    for fill in ["#336699", "#ff0000", "#00ff00"] {
        assert_eq!(content.matches(fill).count(), 1, "{fill} once: {content}");
    }
    assert!(content.contains(r#"style:name="gr1_3""#), "the repeat takes the next number the file does not hold: {content}");
}

/// **Not closed here**: the repeat of an automatic style's name is written under the name the file wrote it with, so that a name two definitions of one family hold is compared as written. The
/// model writer's check of the styles a save made (`check_styles` in `write/mod.rs`, which no stream owns) asks each style it made to be there once, and the repeat is there too.
#[test]
#[ignore = "write/mod.rs check_styles counts the file's own repeat of a name as a second style the save made"]
fn the_second_automatic_style_of_a_name_is_written_under_the_name_it_had() {
    assert_same(
        "a repeat",
        &Drawing::a4()
            .auto(GR1)
            .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##)
            .page(&rect("gr1")),
    );
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ class 2: kept style data

/// A graphic automatic style's kept data beyond the attributes of its graphic properties in the five namespaces (its own attributes, its paragraph and text properties, its other children,
/// the children of its graphic properties, an attribute in another namespace) comes back where it stood. The writer of the style is `new_style_with` (`write/styles.rs`), which the caller that
/// knows the style's name hands the style's kept data to (`remainder_of` in `write/shapes/asread.rs`). Written back by that task: this test was red on its base.
#[test]
fn the_kept_data_of_a_graphic_style_comes_back_where_it_stood() {
    let loext = |mut d: Drawing| {
        d.ns.push_str(r#" xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0""#);
        d
    };
    for (what, style) in [
        (
            "its own attribute",
            r##"<style:style style:name="gr1" style:family="graphic" style:class="extra"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##,
        ),
        (
            "paragraph properties",
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:paragraph-properties fo:orphans="2" fo:widows="2"/></style:style>"##,
        ),
        (
            "text properties",
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:text-properties fo:language="en" fo:country="US"/></style:style>"##,
        ),
        (
            "a child of the graphic properties",
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"><text:list-style style:name="L9"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style></style:graphic-properties></style:style>"##,
        ),
        (
            "an attribute of another namespace",
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699" loext:decorative="true"/></style:style>"##,
        ),
        (
            "a child that is no properties element",
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:map style:condition="x" style:apply-style-name="gr2"/></style:style>"##,
        ),
    ] {
        assert_same(what, &loext(Drawing::a4().auto(style).page(&rect("gr1"))));
    }
}

/// **Handed to ** (`read/styles.rs`, which the style stream owns): `style:writing-mode` in the paragraph properties of a graphic style is a property the reader's table maps
/// (`P::WritingMode`) and then keeps in no cell and no held value, so the attribute is lost on the way in and no writer can give it back. Red on the base and on this branch.
#[test]
#[ignore = "The reader maps style:writing-mode of a graphic style's paragraph properties and keeps it nowhere"]
fn the_writing_mode_of_the_paragraph_properties_of_a_graphic_style_comes_back() {
    assert_same(
        "paragraph properties",
        &Drawing::a4()
            .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:paragraph-properties style:writing-mode="lr-tb"/></style:style>"##)
            .page(&rect("gr1")),
    );
}

/// **Not closed here**: a style that underlines or strikes says its `type` where the file stated the style alone (the cells write both). The group `Style` is held by its spelling (class 5);
/// the groups `Underline` and `Strike` hold the attributes *beyond* the cells, so the absence of a `type` has no place to be held.
#[test]
#[ignore = "The underline's and the strike-through's type has no place to be held absent"]
fn an_underlined_style_that_says_no_type_comes_back() {
    for text_properties in [r#"style:text-underline-style="solid""#, r#"style:text-line-through-style="solid""#] {
        assert_same(
            text_properties,
            &with_common(
                &format!(r#"<style:style style:name="U1" style:family="graphic"><style:text-properties {text_properties}/></style:style>"#),
                "U1",
            ),
        );
    }
}
