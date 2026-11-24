//! The held graphic values of a shape (a hatch, a dash, a foreign line end, a shadow) are written into its automatic style while their stamps match the cells, and are
//! not written once the cells changed (the native-format design's §3.3). Every read-back goes through the real reader.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Value};
use diacraft_odg::{Limits, WriteOptions, compare_content, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const RECT: &str = r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
const LINE: &str = r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#;

fn hatch() -> Drawing {
    Drawing::a4()
        .common(r##"<draw:hatch draw:name="H1" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H1"/></style:style>"#)
        .page(RECT)
}

fn dash() -> Drawing {
    Drawing::a4()
        .common(r#"<draw:stroke-dash draw:name="D1" draw:style="rect" draw:dots1="1" draw:dots1-length="0.2cm" draw:distance="0.2cm"/>"#)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D1"/></style:style>"#)
        .page(LINE)
}

fn arrow() -> Drawing {
    Drawing::a4()
        .common(r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0L0 30H20z"/>"#)
        .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:marker-end="Arrow" draw:marker-end-width="0.3cm"/></style:style>"#)
        .page(LINE)
}

fn shadow() -> Drawing {
    Drawing::a4()
        .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:shadow="visible" draw:shadow-offset-x="0.2cm" draw:shadow-offset-y="0.2cm" draw:shadow-color="#808080"/></style:style>"##)
        .page(RECT)
}

fn model_of(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap().0
}

fn written(doc: &Document) -> (String, String) {
    let bytes = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}")).bytes;
    let package = diacraft_odg::package::Package::read(&bytes, &Limits::default()).unwrap();
    let part = |name: &str| String::from_utf8(package.part(name).unwrap().to_vec()).unwrap();
    (part("content.xml"), part("styles.xml"))
}

/// The text of the first automatic style of `content.xml`.
fn first_style(content: &str) -> &str {
    let at = content.find("<style:style ").expect("a style");
    &content[at..content[at..].find("</style:style>").map(|n| at + n).unwrap()]
}

fn edit_first(doc: &mut Document, change: impl FnOnce(&mut diacraft_doc::Shape)) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    change(Arc::make_mut(page.shapes.get_mut(0).unwrap()));
}

#[test]
fn a_hatch_the_file_named_is_written_with_its_name_and_its_definition() {
    let (content, styles) = written(&model_of(&hatch()));
    let style = first_style(&content);
    assert!(style.contains("draw:fill=\"hatch\"") && style.contains("draw:fill-hatch-name=\"H1\""), "{style}");
    assert!(styles.contains("<draw:hatch draw:name=\"H1\""), "the definition is written: {styles}");
    // Read again, the shape holds the hatch.
    let bytes = write_with(&model_of(&hatch()), None, &WriteOptions::default()).unwrap().bytes;
    let (back, _) = read(&bytes, &Limits::default()).unwrap();
    let held = back.pages.get(0).unwrap().shapes.get(0).unwrap().ext.get().map(|d| d.odf.held.len()).unwrap_or(0);
    assert!(held >= 1, "the reader holds the hatch again");
}

#[test]
fn a_hatch_is_not_written_once_the_cells_it_stands_beside_changed_and_is_again_when_they_come_back() {
    let mut doc = model_of(&hatch());
    let before = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("FillPattern").cloned();
    edit_first(&mut doc, |shape| {
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("FillPattern"), Cell::constant(Value::from_number(1.0).unwrap()));
    });
    let (content, _) = written(&doc);
    assert!(!first_style(&content).contains("fill-hatch-name"), "the cells changed: the hatch is stale: {}", first_style(&content));
    // An undo puts the cell back, and the value is good again.
    edit_first(&mut doc, |shape| {
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("FillPattern"), before.clone().unwrap());
    });
    let (content, _) = written(&doc);
    assert!(first_style(&content).contains("draw:fill-hatch-name=\"H1\""), "{}", first_style(&content));
}

#[test]
fn a_dash_the_file_named_is_written_as_held() {
    let (content, styles) = written(&model_of(&dash()));
    let style = first_style(&content);
    assert!(style.contains("draw:stroke-dash=\"D1\""), "{style}");
    assert!(styles.contains("<draw:stroke-dash draw:name=\"D1\""), "{styles}");
}

#[test]
fn a_line_end_marker_the_file_named_is_written_with_its_name_and_its_width() {
    let (content, styles) = written(&model_of(&arrow()));
    let style = first_style(&content);
    assert!(style.contains("draw:marker-end=\"Arrow\"") && style.contains("draw:marker-end-width=\"0.3cm\""), "{style}");
    assert!(styles.contains("<draw:marker draw:name=\"Arrow\""), "{styles}");
}

#[test]
fn a_shadow_the_file_held_is_written_as_held() {
    let (content, _) = written(&model_of(&shadow()));
    let style = first_style(&content);
    for part in ["draw:shadow=\"visible\"", "draw:shadow-offset-x=\"0.2cm\"", "draw:shadow-offset-y=\"0.2cm\"", "draw:shadow-color=\"#808080\""] {
        assert!(style.contains(part), "{part}: {style}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the visit

/// The shapes of a custom shape on the style `gr1` (a hatch and a shadow) and a line on `gr2` (a dash and a line end that DiaCraft did not draw).
fn every_held_group() -> Drawing {
    let custom = r#"<draw:custom-shape draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#;
    let line = r#"<draw:line draw:style-name="gr2" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#;
    Drawing::a4()
        .common(r##"<draw:hatch draw:name="H1" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##)
        .common(r#"<draw:stroke-dash draw:name="D1" draw:style="rect" draw:dots1="1" draw:dots1-length="0.2cm" draw:distance="0.2cm"/>"#)
        .common(r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0L0 30H20z"/>"#)
        .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H1" draw:shadow="visible" draw:shadow-offset-x="0.2cm" draw:shadow-offset-y="0.2cm" draw:shadow-color="#808080"/></style:style>"##)
        .auto(r#"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D1" draw:marker-end="Arrow" draw:marker-end-width="0.3cm"/></style:style>"#)
        .page(&format!("{custom}{line}"))
}

/// What a shape holds of the four groups a style's graphic properties are: group, attributes and stamp of each held value.
type HeldValues = Vec<(String, Vec<(String, String)>, Option<diacraft_doc::Stamp>)>;

/// The held values of the shapes of the first page, as the reader holds them.
fn held_of(doc: &Document) -> Vec<HeldValues> {
    let graphic = |g: diacraft_doc::HeldGroup| {
        matches!(
            g,
            diacraft_doc::HeldGroup::Fill | diacraft_doc::HeldGroup::Line | diacraft_doc::HeldGroup::LineEnds | diacraft_doc::HeldGroup::Shadow
        )
    };
    doc.pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .map(|s| {
            let Some(d) = s.ext.get() else { return Vec::new() };
            d.odf
                .held
                .iter()
                .filter(|h| graphic(h.group))
                .map(|h| (format!("{:?}", h.group), h.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(), h.stamp))
                .collect()
        })
        .collect()
}

/// The document's `odf` data of a model.
fn odf_of(doc: &Document) -> diacraft_doc::OdfDoc {
    doc.ext.get().map(|d| d.odf.clone()).unwrap_or_default()
}

/// **An `.odg` visit through DiaCraft has the same content** for the rows of the design's section 3.4 that this task writes: a drawing read, written from the model alone and read
/// again holds the same definitions, layer set, other style families, font faces and master pages, the same held graphic values with the same stamps, and `styles.xml` has the same content
/// as the file's (`compare_content`).
#[test]
fn an_odg_visit_through_diacraft_has_the_same_content() {
    let mut extra_layer = Drawing::a4().page(RECT);
    extra_layer.masters = r#"<draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="Notes" draw:display="true"/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#.to_owned();
    let mut font = Drawing::a4().page(RECT);
    font.fonts =
        r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'" style:font-family-generic="swiss"/>"#.to_owned();
    let mut two_masters = Drawing::a4().page(RECT).page(RECT);
    // The second page is on the second master page: the model holds a master page by the pages that name it, so the master pages come in the order the pages first name them (a master
    // page no page names is not held, and the order of the file's own is not either).
    let second = two_masters.pages.rfind("draw:master-page-name=\"Default\"").unwrap();
    two_masters.pages.replace_range(second..second + "draw:master-page-name=\"Default\"".len(), "draw:master-page-name=\"Other\"");
    two_masters.masters.push_str(r#"<style:master-page style:name="Other" style:page-layout-name="PM1" style:display-name="Other"/>"#);
    let styled = Drawing::a4()
        .common(r#"<style:style style:name="Strong" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
        .common(r#"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="-"/></text:list-style>"#)
        .page(RECT);
    let vectors = [
        ("every held group", every_held_group()),
        ("a hatch", hatch()),
        ("a dash", dash()),
        ("a foreign line end", arrow()),
        ("a shadow", shadow()),
        ("an extra layer", extra_layer),
        ("a font face", font),
        ("pages on two master pages", two_masters),
        ("other style families", styled),
    ];
    for (name, drawing) in vectors {
        let mut drawing = drawing;
        drawing.version = "1.4".to_owned();
        let bytes = package(DRAWING, drawing.files());
        let (before, _) = read(&bytes, &Limits::default()).unwrap();
        let options = WriteOptions { content_unchanged: true, ..WriteOptions::default() };
        let written = write_with(&before, None, &options).unwrap_or_else(|e| panic!("{name}: refused: {e}"));
        let (after, _) = read(&written.bytes, &Limits::default()).unwrap();
        let (b, a) = (odf_of(&before), odf_of(&after));
        let definitions = |o: &diacraft_doc::OdfDoc| o.definitions.iter().map(|d| (d.family, d.name.clone(), d.element.clone())).collect::<Vec<_>>();
        assert_eq!(definitions(&b), definitions(&a), "{name}: the definitions");
        assert_eq!(b.layers, a.layers, "{name}: the layer set");
        let common = |o: &diacraft_doc::OdfDoc| o.styles.iter().filter(|s| s.family.starts_with("common:")).cloned().collect::<Vec<_>>();
        assert_eq!(common(&b), common(&a), "{name}: the other style families");
        let faces = |d: &Document| d.fonts.iter().map(|f| (f.name_u.clone(), f.ext.get().map(|e| e.odf.clone()))).collect::<Vec<_>>();
        assert_eq!(faces(&before), faces(&after), "{name}: the font faces");
        let masters = |d: &Document| d.pages.iter().map(|p| p.ext.get().and_then(|e| e.odf.master_page.clone())).collect::<Vec<_>>();
        assert_eq!(masters(&before), masters(&after), "{name}: the master page of each page");
        assert_eq!(held_of(&before), held_of(&after), "{name}: the held graphic values and their stamps");
        let diff = compare_content(&bytes, &written.bytes, &Limits::default()).unwrap();
        let styles: Vec<String> = diff.shown.iter().filter(|d| d.part == "styles.xml").map(|d| format!("{} {:?}", d.path, d.found)).collect();
        assert!(styles.is_empty(), "{name}: styles.xml differs from the file's: {styles:?}");
    }
}

/// **A second model save is no edit** for a drawing that holds every held group (a hatch and a shadow, a dash and a foreign line end): the drawing written, read again and written again has
/// the same content.
#[test]
fn a_second_model_save_is_no_edit() {
    let mut drawing = every_held_group();
    drawing.version = "1.4".to_owned();
    let (doc, _) = read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap();
    let options = WriteOptions { content_unchanged: true, ..WriteOptions::default() };
    let first = write_with(&doc, None, &options).unwrap();
    let (again, _) = read(&first.bytes, &Limits::default()).unwrap();
    let second = write_with(&again, None, &options).unwrap();
    let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
    let ours: Vec<String> = diff.shown.iter().map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
    assert!(diff.same(), "the second save differs: {ours:?}");
    // And what was held is still written: the hatch, the shadow, the dash and the marker.
    let (content, _) = written(&again);
    for want in ["draw:fill-hatch-name=\"H1\"", "draw:shadow=\"visible\"", "draw:stroke-dash=\"D1\"", "draw:marker-end=\"Arrow\""] {
        assert!(content.contains(want), "{want}");
    }
}
