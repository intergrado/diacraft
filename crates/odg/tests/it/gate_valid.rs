//! What a visit writes back of the styles of an ODF 1.0 file is valid ODF 1.4, and what it could not say there is carried.
//!
//! The full gate's schema check found two classes that the styles gave the output:
//!
//! * **HELD-WEIGHT**: a common graphic style whose text properties say `fo:font-weight="250"`, written as spelled (ODF 1.4 takes `normal`, `bold` and 100 to 900 in steps of 100);
//! * **HELD-NUMBERING**: an automatic paragraph style that nothing names, with `text:enable-numbering="false"`, written whole (no version of the schema names the attribute).
//!
//! A visit is the crate-level one of PL5, as `visit_styles.rs` makes it. Every read-back goes through the real reader, and every part written is held to the ODF 1.4 RelaxNG schema when
//! `ODF_SCHEMA_RNG` names it (`model_safe::schema_errors`); the tests also assert the values of the attributes themselves, so that they are red without the schema. Every input is built in
//! code from the ODF 1.4 Part 3 vocabulary.

use diacraft_container::xml::{El, Node};
use diacraft_doc::Document;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::model_safe::{model_same, schema_errors};
use crate::visit_styles::{GR1, part, rect, source_of, through_codec, visit};

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The package of `drawing` as ODF 1.0, the version whose schema takes what 1.4's refuses.
pub(crate) fn old(drawing: &Drawing) -> Vec<u8> {
    let mut drawing = drawing.clone();
    drawing.version = "1.0".to_owned();
    drawing.bytes()
}

/// Every value of the attribute `name` in `el` and below it.
fn values(el: &El, name: &str, out: &mut Vec<String>) {
    out.extend(el.attrs.iter().filter(|(k, _)| k == name).map(|(_, v)| v.clone()));
    for child in el.children.iter() {
        if let Node::El(e) = child {
            values(e, name, out);
        }
    }
}

/// The values of the attribute `name` in the part `part_name` of `bytes`.
fn attribute_values(bytes: &[u8], part_name: &str, name: &str) -> Vec<String> {
    let root = diacraft_container::xml::parse(part(bytes, part_name).as_bytes()).unwrap();
    let mut out = Vec::new();
    values(&root, name, &mut out);
    out
}

/// The model of the file `bytes`.
fn model(bytes: &[u8]) -> Document {
    read(bytes, &Limits::default()).unwrap().0
}

/// What the ODF 1.4 schema says of every part of `bytes` (nothing when no schema is named).
fn assert_valid(bytes: &[u8], what: &str) {
    if let Some(checked) = schema_errors(bytes, what) {
        checked.unwrap_or_else(|e| panic!("{what}: not valid ODF 1.4:\n{e}"));
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ HELD-WEIGHT

const LIGHT: &str = r#"<style:style style:name="Light" style:family="graphic"><style:text-properties fo:font-weight="250"/></style:style>"#;

const ON_LIGHT: &str = r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Light"/>"#;

/// The weight vectors: a common graphic style that says 250, used by a shape and unused.
fn weights() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("used by a shape", old(&Drawing::a4().common(LIGHT).auto(ON_LIGHT).page(&rect("gr1")))),
        ("unused", old(&Drawing::a4().common(LIGHT).page(&rect("")))),
    ]
}

/// **The weight of a style that ODF 1.4 has no value for is written as the value it has nearest, and the weight itself rides in DiaCraft's own attribute** on the same element, so that a read
/// of the file written gives the weight back: the model read from the file written is the one read from the file visited (the style's held spelling and its property no cell reads).
#[test]
fn a_weight_odf_1_4_has_no_value_for_is_written_as_one_it_has_and_comes_back() {
    for (what, source) in weights() {
        let (written, _) = visit(&source).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(
            attribute_values(&written, "styles.xml", "fo:font-weight"),
            ["200"],
            "{what}: the weight the schema takes nearest to 250 (a tie goes to the lighter one)"
        );
        let styles = part(&written, "styles.xml");
        assert!(styles.contains(r#"diacraft:older-font-weight="250""#), "{what}: the weight it had rides on the same element: {styles}");
        assert_valid(&written, what);
        // A DiaCraft read gives the weight back: the same model as the file the visit began with.
        assert_eq!(model_same(&model(&source), &model(&written)), Ok(()), "{what}");
        let light = model(&written).styles.iter().find(|s| s.name_u.as_deref() == Some("Light")).cloned().expect("the style sheet");
        let odf = &light.ext.get().expect("its own slot").odf;
        assert_eq!(odf.unmapped.get("style:text-properties/fo:font-weight"), Some("250"), "{what}: the property no cell reads");
        assert!(odf.text_held.iter().any(|h| h.attrs.get("fo:font-weight") == Some("250")), "{what}: the held spelling of the group");
        // A second visit is the same bytes.
        let (again, _) = visit(&written).unwrap_or_else(|e| panic!("{what}: second visit: {e}"));
        assert_eq!(again, written, "{what}: a second visit is the same bytes");
    }
}

/// The other attributes of the group a style may spell in a form ODF 1.4 has no value for (the style's slant, the small caps, and the complex script's weight and slant) are written the same way,
/// each as the value it has nearest and each carried, and a read gives each back.
#[test]
fn the_other_attributes_of_the_group_are_written_as_values_odf_1_4_has_and_come_back() {
    let style = r#"<style:style style:name="Odd" style:family="graphic"><style:text-properties fo:font-weight="bolder" fo:font-style="slanted" fo:font-variant="SMALL-CAPS" style:font-weight-complex="650" style:font-style-complex="Italic"/></style:style>"#;
    let source = old(&Drawing::a4().common(style).page(&rect("")));
    let (written, _) = visit(&source).unwrap();
    let get = |name: &str| attribute_values(&written, "styles.xml", name);
    assert_eq!(get("fo:font-weight"), ["bold"]);
    assert_eq!(get("fo:font-style"), ["normal"]);
    assert_eq!(get("fo:font-variant"), ["small-caps"]);
    assert_eq!(get("style:font-weight-complex"), ["600"], "650 is a bold weight (600 to 900), so the nearest bold one");
    assert_eq!(get("style:font-style-complex"), ["italic"]);
    assert_valid(&written, "the group");
    // A read of either file gives the style the same values: the held spelling holds the five as the file spelled them, and the two weights, which no cell can show, are properties no cell reads.
    let spelled = |bytes: &[u8]| {
        let doc = model(bytes);
        let odd = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Odd")).cloned().expect("the style sheet");
        let odf = odd.ext.get().expect("its own slot").odf.clone();
        let mut held: Vec<(String, String)> =
            odf.text_held.iter().flat_map(|h| h.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<Vec<_>>()).collect();
        held.sort();
        let mut unmapped: Vec<(String, String)> = odf.unmapped.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        unmapped.sort();
        (held, unmapped)
    };
    let (held, unmapped) = spelled(&written);
    assert_eq!(spelled(&source), (held.clone(), unmapped.clone()), "the file written gives what the file visited gave");
    let pairs = |list: &[(&str, &str)]| list.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect::<Vec<_>>();
    assert_eq!(
        held,
        pairs(&[
            ("fo:font-style", "slanted"),
            ("fo:font-variant", "SMALL-CAPS"),
            ("fo:font-weight", "bolder"),
            ("style:font-style-complex", "Italic"),
            ("style:font-weight-complex", "650"),
        ])
    );
    assert_eq!(unmapped, pairs(&[("style:text-properties/fo:font-weight", "bolder"), ("style:text-properties/style:font-weight-complex", "650")]));
    let (again, _) = visit(&written).unwrap();
    assert_eq!(again, written, "a second visit is the same bytes");
}

/// A text box whose one paragraph holds `para`.
fn text_box(para: &str) -> String {
    format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box>{para}</draw:text-box></draw:frame>"#)
}

/// **A paragraph style's text properties are written the same way**: a paragraph style named by a paragraph, whose text properties say a weight ODF 1.4 lists no value for, is written
/// with the values it lists nearest, the values it had ride in DiaCraft's own attributes beside them (as `text:enable-numbering` of such a style does), and a read gives them back. (The text
/// properties of a style a *run* names are the run's cells': the `Style` cell writes the group.)
#[test]
fn the_text_properties_of_a_paragraph_style_a_paragraph_names_are_written_in_the_form_odf_1_4_takes_and_come_back() {
    let styles = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-weight="650"/></style:style>"#;
    let page = text_box(r#"<text:p text:style-name="P1">Hello</text:p>"#);
    let source = old(&Drawing::a4().auto(GR1).auto(styles).page(&page));
    let (written, _) = visit(&source).unwrap();
    // (The paragraph's run is given a style of its own, which says `normal` of each.)
    let (weights, slants) = (attribute_values(&written, "content.xml", "fo:font-weight"), attribute_values(&written, "content.xml", "fo:font-style"));
    assert!(
        weights.contains(&"600".to_owned()) && weights.iter().all(|w| w == "600" || w == "normal"),
        "{weights:?}: {}",
        part(&written, "content.xml")
    );
    assert!(slants.iter().all(|w| w == "normal"), "{slants:?}");
    assert_eq!(attribute_values(&written, "content.xml", "diacraft:older-font-weight"), ["650"]);
    assert_valid(&written, "a paragraph style");
    // What the model holds of the style is what the file visited gave (the page's own text is another task's: the run is given a style of its own).
    let kept = |bytes: &[u8]| {
        let doc = model(bytes);
        doc.ext
            .get()
            .map(|d| d.odf.styles.iter().filter(|k| &*k.name == "P1").map(|k| format!("{} {:?}", k.family, k.element)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    assert_eq!(kept(&written), kept(&source));
    assert_eq!(kept(&source).len(), 1, "the style is kept: its weight is a property no cell reads");
    // (A second visit of the file written is not asserted here: the text of a frame the first visit wrote rides in DiaCraft's data, and the second visit writes its paragraph's style anew, without
    // the weight no cell reads, on the base as well; the text writer's.)
}

/// A value ODF 1.4 takes is written as it is, with no carrier, and a weight that another program changed after the file was written is the program's: the carrier is used only while the
/// value beside it is the one the writer would write for it.
#[test]
fn a_weight_that_is_valid_has_no_carrier_and_a_carrier_that_another_program_outdated_is_not_used() {
    let valid = r#"<style:style style:name="Strong" style:family="graphic"><style:text-properties fo:font-weight="700"/></style:style>"#;
    let (written, _) = visit(&old(&Drawing::a4().common(valid).page(&rect("")))).unwrap();
    assert!(!part(&written, "styles.xml").contains("older-"), "{}", part(&written, "styles.xml"));
    // The file the visit wrote reads as it did; the same file with the weight set to bold by another program, which left the carrier as it was, reads as bold.
    let carried = |weight: &str| {
        format!(
            r#"<style:style style:name="Light" style:family="graphic"><style:text-properties fo:font-weight="{weight}" diacraft:older-font-weight="250"/></style:style>"#
        )
    };
    let spelled = |weight: &str| {
        let doc = model(&source_of(&Drawing::a4().common(&carried(weight)).page(&rect(""))));
        let light = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Light")).cloned().expect("the style sheet");
        let odf = light.ext.get().expect("its own slot").odf.clone();
        (
            odf.unmapped.get("style:text-properties/fo:font-weight").map(str::to_owned),
            odf.text_held.iter().filter_map(|h| h.attrs.get("fo:font-weight").map(str::to_owned)).collect::<Vec<_>>(),
        )
    };
    assert_eq!(spelled("200"), (Some("250".to_owned()), vec!["250".to_owned()]), "the weight the writer wrote 250 as: the carrier is the weight");
    assert_eq!(
        spelled("bold"),
        (None, vec!["bold".to_owned()]),
        "the weight another program set: the carrier is outdated, and the file's own value is read"
    );
    // A weight 1.4 takes that is not the one the carrier stands for is the file's own, whatever the carrier says: nothing is read from it.
    assert_eq!(
        spelled("300"),
        (None, vec!["300".to_owned()]),
        "a weight the schema takes is the file's own, and a carrier that does not stand for it is no spelling"
    );
}

/// **A default style is a style written whole**: its text properties and paragraph properties are written in the form ODF 1.4 takes the same way, and a read gives the file's back.
#[test]
fn a_default_style_with_the_forms_of_an_earlier_version_is_written_in_the_form_odf_1_4_takes_and_comes_back() {
    let defaults = r#"<style:default-style style:family="graphic"><style:paragraph-properties text:enable-numbering="true" fo:text-align="center"/><style:text-properties fo:font-weight="250"/></style:default-style>"#;
    let source = old(&Drawing::a4().common(defaults).page(&rect("")));
    let (written, _) = visit(&source).unwrap();
    assert_eq!(attribute_values(&written, "styles.xml", "fo:font-weight"), ["200"], "{}", part(&written, "styles.xml"));
    assert!(attribute_values(&written, "styles.xml", "text:enable-numbering").is_empty());
    assert_eq!(attribute_values(&written, "styles.xml", "diacraft:older-font-weight"), ["250"]);
    assert_eq!(attribute_values(&written, "styles.xml", "diacraft:older-enable-numbering"), ["true"]);
    assert_valid(&written, "a default style");
    assert_eq!(model_same(&model(&source), &model(&written)), Ok(()));
    let (again, _) = visit(&written).unwrap();
    assert_eq!(again, written, "a second visit is the same bytes");
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ HELD-NUMBERING

const NUMBERED: &str = r#"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties fo:text-align="center" text:enable-numbering="false"/></style:style>"#;

/// The kept styles of a model that nothing names, as `family/name`.
fn unused(doc: &Document) -> Vec<String> {
    doc.ext
        .get()
        .map(|d| d.odf.styles.iter().filter(|s| s.family.starts_with("unused")).map(|s| format!("{}/{}", s.family, s.name)).collect())
        .unwrap_or_default()
}

/// The properties element of the kept style `family/name` of `doc`, as the reader keeps it, whose local name is `holder`.
fn kept_props(doc: &Document, family: &str, name: &str, holder: &str) -> Option<diacraft_doc::RawElement> {
    let kept = doc.ext.get()?.odf.styles.iter().find(|s| &*s.family == family && &*s.name == name)?;
    kept.element.children.iter().find_map(|c| match c {
        diacraft_doc::RawNode::Element(e) if e.name.rsplit(':').next() == Some(holder) => Some(e.clone()),
        _ => None,
    })
}

/// **An automatic style that nothing names, kept whole, is written as a style a paragraph names is** (`write/text/model.rs`): the attribute the schemas of 1.2 and 1.4 have no place for on its
/// paragraph properties moves into DiaCraft's own attribute, and the read of the file written gives it back where it stood.
#[test]
fn an_unused_style_with_text_enable_numbering_is_written_in_the_form_odf_1_4_takes_and_comes_back() {
    let in_content = old(&Drawing::a4().auto(GR1).auto(NUMBERED).page(&rect("gr1")));
    let mut in_styles = Drawing::a4().auto(GR1).page(&rect("gr1"));
    in_styles.styles_auto.push_str(&NUMBERED.replace("P9", "P8"));
    let in_styles = old(&in_styles);
    for (what, source, part_name, family, name) in [
        ("in content.xml", in_content, "content.xml", "unused:paragraph", "P9"),
        ("in styles.xml", in_styles, "styles.xml", "unused-styles:paragraph", "P8"),
    ] {
        let (written, _) = visit(&source).unwrap_or_else(|e| panic!("{what}: {e}"));
        let text = part(&written, part_name);
        assert!(!text.contains("text:enable-numbering"), "{what}: the attribute is in no part of 1.4: {text}");
        assert!(text.contains(r#"diacraft:older-enable-numbering="false""#), "{what}: it rides in DiaCraft's own attribute: {text}");
        assert_valid(&written, what);
        // The read gives it back where it stood: the model is the one the file visited gave, and the carrier is no attribute of the style.
        let (before, after) = (model(&source), model(&written));
        assert_eq!(unused(&after), [format!("{family}/{name}")], "{what}");
        assert_eq!(model_same(&before, &after), Ok(()), "{what}");
        let props = kept_props(&after, family, name, "paragraph-properties").expect("the properties");
        assert_eq!(props.attrs.get("text:enable-numbering"), Some("false"), "{what}: {props:?}");
        assert!(
            props.attrs.iter().all(|(k, _)| !k.contains("older-") && !k.starts_with("xmlns")),
            "{what}: the carrier and its declaration are no attributes of the style: {props:?}"
        );
        let (again, _) = visit(&written).unwrap_or_else(|e| panic!("{what}: second visit: {e}"));
        assert_eq!(again, written, "{what}: a second visit is the same bytes");
    }
}

/// The same treatment for the weights an unused style spells, and for a text style and a graphic style that hold text properties: every automatic style nothing names is held whole.
#[test]
fn an_unused_style_of_any_family_is_written_in_the_form_odf_1_4_takes() {
    let styles = concat!(
        r##"<style:style style:name="T9" style:family="text"><style:text-properties fo:font-weight="250" fo:color="#ff0000"/></style:style>"##,
        r#"<style:style style:name="gr9" style:family="graphic"><style:paragraph-properties text:enable-numbering="true"/><style:text-properties fo:font-style="slanted"/></style:style>"#
    );
    let source = old(&Drawing::a4().auto(GR1).auto(styles).page(&rect("gr1")));
    let (written, _) = visit(&source).unwrap();
    assert_eq!(attribute_values(&written, "content.xml", "fo:font-weight"), ["200"]);
    assert_eq!(attribute_values(&written, "content.xml", "fo:font-style"), ["normal"]);
    assert!(attribute_values(&written, "content.xml", "text:enable-numbering").is_empty());
    assert_valid(&written, "unused styles of every family");
    let (before, after) = (model(&source), model(&written));
    assert_eq!(model_same(&before, &after), Ok(()));
    assert_eq!(kept_props(&after, "unused:text", "T9", "text-properties").unwrap().attrs.get("fo:font-weight"), Some("250"));
    assert_eq!(kept_props(&after, "unused:graphic", "gr9", "paragraph-properties").unwrap().attrs.get("text:enable-numbering"), Some("true"));
    let (again, _) = visit(&written).unwrap();
    assert_eq!(again, written, "a second visit is the same bytes");
}

/// An attribute of the standard's own namespaces on the properties of an unused style that the schemas have no place for and no carrier is made for is left out and counted, as the used path
/// leaves it out (`style_attributes_not_written`): the note says so, and the file is valid.
#[test]
fn an_attribute_with_no_carrier_on_an_unused_style_is_left_out_and_counted() {
    let style = r##"<style:style style:name="P9" style:family="paragraph"><style:paragraph-properties fo:text-align="center" draw:fill-color="#ff0000"/></style:style>"##;
    let source = old(&Drawing::a4().auto(GR1).auto(style).page(&rect("gr1")));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let written = write_with(&through_codec(&doc), None, &unedited()).unwrap();
    let content = part(&written.bytes, "content.xml");
    assert!(!content.contains("#ff0000"), "the attribute the schema has no place for is left out: {content}");
    assert!(content.contains(r#"fo:text-align="center""#), "what the schema takes stays: {content}");
    assert!(written.notes.iter().any(|n| n.class() == "document_data_unwritten"), "the save says what it left out: {:?}", written.notes);
    assert_valid(&written.bytes, "an attribute with no carrier");
}
