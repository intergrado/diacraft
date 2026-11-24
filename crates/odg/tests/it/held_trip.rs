//! **Nothing an `.odg` holds is lost by the trip to `.diacraft`**. A `.diacraft` file keeps no original, so what the model does not hold is gone the day a drawing
//! opened from an `.odg` is saved as one. For each construct the reader once kept only in the file's bytes, a drawing that holds it is read, the model is encoded by the
//! whole-model codec (the `.diacraft` path) and the **encoded parts are searched for what the construct says**: an attribute by its name and value (`Attr`), or any string of
//! the parts, an element's name, an attribute's name or value or a text, that contains a token only that construct holds (`Any`). A construct that the model holds comes through
//! the codec too (`decode(encode(m)) == m` is the codec's contract and is checked for each drawing). The drawings are written by hand from ODF 1.4 Part 3.

use std::collections::HashSet;

use diacraft_container::xml::{El, Node, to_bytes};
use diacraft_doc::Document;
use diacraft_sheetxml::model::{Budget, MemBlobs, decode, encode};

use crate::build::replace;
use crate::drawing::Drawing;

/// What the encoded parts say: the strings of every element and attribute, and the pairs of each `attr` element.
#[derive(Default)]
struct Parts {
    strings: Vec<String>,
    pairs: HashSet<(String, String)>,
    xml: String,
}

fn walk(root: &El, out: &mut Parts) {
    let mut stack: Vec<&El> = vec![root];
    while let Some(el) = stack.pop() {
        out.strings.push(el.name.clone());
        for (k, v) in &el.attrs {
            out.strings.push(k.clone());
            out.strings.push(v.clone());
        }
        if el.name.ends_with("attr") || el.name == "attr" {
            let name = el.attrs.iter().find(|(k, _)| k == "name" || k == "n").map(|(_, v)| v.clone());
            let value = el.attrs.iter().find(|(k, _)| k == "value" || k == "v").map(|(_, v)| v.clone());
            if let (Some(n), Some(v)) = (name, value) {
                out.pairs.insert((n, v));
            }
        }
        for node in &el.children {
            match node {
                Node::El(child) => stack.push(child),
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => out.strings.push(t.clone()),
                Node::Pi { target, data } => {
                    out.strings.push(target.clone());
                    out.strings.push(data.clone());
                }
            }
        }
    }
}

/// The model through the codec: the parts as XML text, and the model decoded again.
fn through_codec(doc: &Document) -> (Parts, Document) {
    let mut blobs = MemBlobs::new();
    let encoded = encode(doc, &mut blobs).unwrap_or_else(|e| panic!("{e:?}"));
    let mut parts = Parts::default();
    let mut all: Vec<&El> = vec![&encoded.document];
    all.extend(encoded.pages.iter().map(|(_, e)| e));
    all.extend(encoded.masters.iter().map(|(_, e)| e));
    for el in all {
        walk(el, &mut parts);
        parts.xml.push_str(&String::from_utf8_lossy(&to_bytes(el, &[], &[], false).unwrap_or_default()));
    }
    let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&El> = encoded.masters.iter().map(|(_, e)| e).collect();
    let mut budget = Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4);
    let mut notes = Vec::new();
    let back = decode(&encoded.document, &pages, &masters, &blobs, &mut budget, &mut notes).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(notes.is_empty(), "{notes:?}");
    (parts, back)
}

/// What a construct says, to be found in the encoded parts.
enum Probe {
    /// An `attr` element with this name and value.
    Attr(&'static str, &'static str),
    /// A string (an element's or attribute's name or value, a text) that contains this.
    Any(&'static str),
}

/// Reads `drawing`, sends the model through the codec and looks for each probe. The model must come back the same, and each probe must be found.
#[track_caller]
fn survives(drawing: &Drawing, probes: &[Probe]) {
    survives_bytes(&drawing.bytes(), probes);
}

#[track_caller]
fn survives_bytes(bytes: &[u8], probes: &[Probe]) {
    let (doc, _) =
        diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()));
    let (parts, back) = through_codec(&doc);
    assert!(doc == back, "the model comes back from the codec the same");
    let mut lost: Vec<String> = Vec::new();
    for probe in probes {
        let found = match probe {
            // A style's properties are keyed by their element (`style:graphic-properties/draw:fill-hatch-name`): the name is the key's end.
            Probe::Attr(n, v) => parts.pairs.iter().any(|(k, w)| w == v && (k == n || k.ends_with(&format!("/{n}")))),
            Probe::Any(token) => parts.xml.contains(token) || parts.strings.iter().any(|s| s.contains(token)),
        };
        if !found {
            lost.push(match probe {
                Probe::Attr(n, v) => format!("{n}=\"{v}\""),
                Probe::Any(t) => (*t).to_owned(),
            });
        }
    }
    assert!(lost.is_empty(), "the model does not hold, and the codec does not write: {lost:?}");
}

const NS_X: &str = r#" xmlns:x="urn:example:extension""#;

fn with_x(mut d: Drawing) -> Drawing {
    d.ns.push_str(NS_X);
    d
}

fn common_style(props: &str) -> Drawing {
    Drawing::a4().common(&format!(r##"<style:style style:name="Cs" style:family="graphic"><style:graphic-properties {props}/></style:style>"##))
}

fn shape_with(text: &str) -> Drawing {
    Drawing::a4().page(&format!(
        r##"<draw:custom-shape draw:name="S" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm">{text}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"##
    ))
}

// ---------------------------------------------------------------------------------------------------------------------------------- a common style

#[test]
fn a_common_style_s_hatch_fill_comes_through_the_codec() {
    survives(
        &common_style(r##"draw:fill="hatch" draw:fill-hatch-name="RefHatch1" draw:fill-hatch-solid="true" draw:fill-color="#ff0000""##),
        &[Probe::Attr("draw:fill", "hatch"), Probe::Attr("draw:fill-hatch-name", "RefHatch1")],
    );
}

#[test]
fn a_common_style_s_foreign_dash_comes_through_the_codec() {
    survives(
        &common_style(r##"draw:stroke="dash" draw:stroke-dash="RefDash1" svg:stroke-width="0.1cm""##),
        &[Probe::Attr("draw:stroke-dash", "RefDash1")],
    );
}

#[test]
fn a_common_style_s_foreign_marker_comes_through_the_codec() {
    survives(
        &common_style(r##"draw:marker-start="RefMarker1" draw:marker-start-width="0.77cm" draw:marker-start-center="true""##),
        &[Probe::Attr("draw:marker-start", "RefMarker1"), Probe::Attr("draw:marker-start-width", "0.77cm")],
    );
}

#[test]
fn a_common_style_s_shadow_comes_through_the_codec() {
    survives(
        &common_style(r##"draw:shadow="visible" draw:shadow-opacity="37%" draw:shadow-offset-x="0.31cm" draw:shadow-color="#333333""##),
        &[Probe::Attr("draw:shadow-opacity", "37%"), Probe::Attr("draw:shadow-offset-x", "0.31cm")],
    );
}

#[test]
fn a_common_style_s_gradient_name_comes_through_the_codec() {
    survives(
        &common_style(r##"draw:fill="gradient" draw:fill-gradient-name="RefGrad1""##),
        &[Probe::Attr("draw:fill", "gradient"), Probe::Attr("draw:fill-gradient-name", "RefGrad1")],
    );
}

#[test]
fn a_common_style_s_writing_mode_and_padding_come_through_the_codec() {
    survives(
        &common_style(r##"style:writing-mode="tb-rl" fo:padding="0.17cm" draw:textarea-vertical-align="justify""##),
        &[Probe::Attr("style:writing-mode", "tb-rl"), Probe::Attr("fo:padding", "0.17cm")],
    );
}

#[test]
fn a_property_value_no_cell_can_read_comes_through_the_codec() {
    survives(
        &common_style(r##"svg:stroke-width="one parsec" draw:fill-color="not-a-colour""##),
        &[Probe::Attr("svg:stroke-width", "one parsec"), Probe::Attr("draw:fill-color", "not-a-colour")],
    );
}

#[test]
fn a_common_paragraph_style_s_approximated_properties_come_through_the_codec() {
    let d = Drawing::a4().common(
        r##"<style:style style:name="Ps" style:family="paragraph"><style:paragraph-properties fo:text-align="start" style:line-spacing="0.45cm"><style:tab-stops><style:tab-stop style:position="3.3cm" style:leader-text="LeaderTok"/></style:tab-stops></style:paragraph-properties><style:text-properties style:text-underline-style="wave" style:text-underline-color="#123456"/></style:style>"##,
    );
    survives(&d, &[Probe::Attr("fo:text-align", "start"), Probe::Attr("style:text-underline-style", "wave"), Probe::Any("LeaderTok")]);
}

#[test]
fn an_automatic_paragraph_style_s_tab_stops_come_through_the_codec() {
    let d = Drawing::a4().auto(
        r##"<style:style style:name="Pa" style:family="paragraph"><style:paragraph-properties><style:tab-stops><style:tab-stop style:position="2cm" style:leader-text="AutoLeaderTok"/></style:tab-stops></style:paragraph-properties></style:style>"##,
    );
    let d = d.page(r##"<draw:rect svg:width="1cm" svg:height="1cm"><text:p text:style-name="Pa">a</text:p></draw:rect>"##);
    survives(&d, &[Probe::Any("AutoLeaderTok")]);
}

// ---------------------------------------------------------------------------------------------------------------------------------- a text

#[test]
fn a_heading_comes_through_the_codec() {
    survives(
        &shape_with(r##"<text:h text:outline-level="2" text:style-name="Heading_20_2">Title</text:h><text:p>body</text:p>"##),
        &[Probe::Any("text:h"), Probe::Any("outline-level")],
    );
}

#[test]
fn a_list_comes_through_the_codec() {
    let d = Drawing::a4()
        .auto(r##"<text:list-style style:name="ListTok"><text:list-level-style-bullet text:level="1" text:bullet-char="-"/></text:list-style>"##)
        .page(concat!(
            r##"<draw:custom-shape svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="2cm"><text:list text:style-name="ListTok"><text:list-item><text:p>one</text:p></text:list-item>"##,
            r##"<text:list-item><text:p>two</text:p><text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item></text:list>"##,
            r##"<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"##
        ));
    survives(&d, &[Probe::Any("text:list-item"), Probe::Any("ListTok")]);
}

#[test]
fn a_hyperlink_in_a_text_comes_through_the_codec() {
    survives(
        &shape_with(r##"<text:p>see <text:a xlink:href="http://example.org/tok" xlink:type="simple">the link</text:a> here</text:p>"##),
        &[Probe::Any("example.org/tok")],
    );
}

#[test]
fn a_field_the_text_has_no_cell_for_comes_through_the_codec() {
    survives(
        &shape_with(
            r##"<text:p><text:title>TitleTok</text:title> and <text:date style:data-style-name="N10" text:fixed="true">2026-01-01</text:date></text:p>"##,
        ),
        &[Probe::Any("text:title"), Probe::Any("N10")],
    );
}

#[test]
fn a_bookmark_in_an_otherwise_empty_text_comes_through_the_codec() {
    survives(&shape_with(r##"<text:p><text:bookmark text:name="BmTok"/></text:p>"##), &[Probe::Any("BmTok")]);
}

#[test]
fn a_paragraph_s_own_attributes_come_through_the_codec() {
    survives(&shape_with(r##"<text:p text:class-names="ClassTok" xml:id="pid1">a</text:p>"##), &[Probe::Any("ClassTok")]);
}

// ---------------------------------------------------------------------------------------------------------------------------------- the document

/// The package of `d` with `content` and `styles` replaced by the texts made of its own, with the replacements `(from, to)` done on each.
fn package_with(d: &Drawing, content: &[(&str, &str)], styles: &[(&str, &str)]) -> Vec<u8> {
    let mut c = d.content_xml();
    for (from, to) in content {
        assert!(c.contains(from), "{from}");
        c = c.replacen(from, to, 1);
    }
    let mut st = d.styles_xml();
    for (from, to) in styles {
        assert!(st.contains(from), "{from}");
        st = st.replacen(from, to, 1);
    }
    let files = replace(replace(d.files(), "content.xml", c.into_bytes()), "styles.xml", st.into_bytes());
    crate::build::package(crate::build::DRAWING, files)
}

fn plain() -> Drawing {
    with_x(Drawing::a4().page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##))
}

#[test]
fn the_scripts_of_the_content_part_come_through_the_codec() {
    let bytes = package_with(&plain(), &[("<office:body>", r##"<office:scripts><x:s a="ScriptTok"/></office:scripts><office:body>"##)], &[]);
    survives_bytes(&bytes, &[Probe::Any("ScriptTok")]);
}

#[test]
fn a_foreign_child_of_the_body_and_of_the_drawing_come_through_the_codec() {
    let bytes = package_with(
        &plain(),
        &[
            ("<office:drawing>", r##"<office:drawing x:da="DrawAttrTok"><x:drawchild a="DrawTok"/>"##),
            ("<office:body>", r##"<office:body x:ba="BodyAttrTok"><x:bodychild a="BodyTok"/>"##),
        ],
        &[],
    );
    survives_bytes(&bytes, &[Probe::Any("DrawTok"), Probe::Any("BodyTok"), Probe::Any("DrawAttrTok"), Probe::Any("BodyAttrTok")]);
}

#[test]
fn a_foreign_child_of_the_styles_part_and_of_the_master_styles_come_through_the_codec() {
    let bytes = package_with(
        &plain(),
        &[],
        &[
            ("<office:master-styles>", r##"<office:master-styles><x:mschild a="MsTok"/>"##),
            ("</office:document-styles>", r##"<x:rootchild a="RootTok"/></office:document-styles>"##),
        ],
    );
    survives_bytes(&bytes, &[Probe::Any("MsTok"), Probe::Any("RootTok")]);
}

#[test]
fn a_foreign_child_of_a_layer_set_and_a_master_page_come_through_the_codec() {
    let bytes = package_with(
        &plain(),
        &[],
        &[
            ("<draw:layer-set>", r##"<draw:layer-set x:la="LayerSetAttrTok"><x:lschild a="LsTok"/>"##),
            (
                r##"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"##,
                r##"<style:master-page style:name="Default" style:page-layout-name="PM1"><x:mpchild a="MpTok"/></style:master-page>"##,
            ),
        ],
    );
    survives_bytes(&bytes, &[Probe::Any("LsTok"), Probe::Any("LayerSetAttrTok"), Probe::Any("MpTok")]);
}

/// Controls: what the model holds already comes through (a shape's own attribute and child, a text style of the common family, a list style, a page layout's child).
#[test]
fn what_the_model_holds_already_comes_through_the_codec() {
    let d = with_x(
        Drawing::a4()
            .common(concat!(
                r##"<style:style style:name="Tx" style:family="text"><style:text-properties fo:font-style="italic" x:p="TextStyleTok"/></style:style>"##,
                r##"<text:list-style style:name="L9"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style>"##
            ))
            .page(r##"<draw:rect x:cell="CellTok" svg:width="1cm" svg:height="1cm"><x:kid k="ChildTok"/><text:p>t</text:p></draw:rect>"##),
    );
    let bytes =
        package_with(&d, &[], &[("<style:page-layout-properties", r##"<style:header-style x:h="HeaderTok"/><style:page-layout-properties"##)]);
    survives_bytes(&bytes, &[Probe::Any("TextStyleTok"), Probe::Any("L9"), Probe::Any("CellTok"), Probe::Any("ChildTok"), Probe::Any("HeaderTok")]);
}

/// Controls (1): what the shapes' and the pages' own markup holds, which the reader kept in the `odf` slot.
#[test]
fn the_markup_of_a_shape_a_page_and_a_frame_comes_through_the_codec() {
    let d = with_x(Drawing::a4().page(concat!(
        r##"<draw:rect x:ra="RectAttrTok" svg:width="1cm" svg:height="1cm">stray<x:rk k="RectKidTok"/></draw:rect>"##,
        r##"<draw:frame svg:width="3cm" svg:height="1cm"><draw:text-box><x:tb k="TextBoxKidTok"/><text:p>t</text:p></draw:text-box></draw:frame>"##,
        r##"<draw:frame svg:width="3cm" svg:height="1cm"><draw:image xlink:href="http://example.org/LinkedTok.png" xlink:type="simple"/></draw:frame>"##,
        r##"<draw:custom-shape svg:width="3cm" svg:height="1cm"><draw:enhanced-geometry draw:type="PresetTok" svg:viewBox="0 0 10 10"><draw:handle draw:handle-position="HandleTok 1"/></draw:enhanced-geometry></draw:custom-shape>"##,
        r##"<draw:rect svg:width="1cm" svg:height="1cm" svg:rx="0.2cm" draw:corner-radius="0.31cm"/>"##,
    )));
    let bytes =
        package_with(&d, &[("<draw:page ", r##"<draw:page x:pa="PageAttrTok" "##), ("</draw:page>", r##"<x:pk k="PageKidTok"/></draw:page>"##)], &[]);
    survives_bytes(
        &bytes,
        &[
            Probe::Any("RectAttrTok"),
            Probe::Any("stray"),
            Probe::Any("RectKidTok"),
            Probe::Any("TextBoxKidTok"),
            Probe::Any("LinkedTok"),
            Probe::Any("PresetTok"),
            Probe::Any("HandleTok"),
            Probe::Any("0.31cm"),
            Probe::Any("PageAttrTok"),
            Probe::Any("PageKidTok"),
        ],
    );
}

/// Controls (2): the other style families, the definitions the common styles name, the generator, the settings and the roots, as and 3 hold them.
#[test]
fn the_documents_own_data_comes_through_the_codec() {
    let d = with_x(
        Drawing::a4()
            .common(concat!(
                r##"<draw:marker draw:name="MarkerDefTok" svg:viewBox="0 0 10 10" svg:d="M0 0"/>"##,
                r##"<draw:stroke-dash draw:name="DashDefTok" draw:style="rect" draw:dots1="1"/>"##,
                r##"<draw:hatch draw:name="HatchDefTok" draw:style="single" draw:color="#000000"/>"##,
                r##"<draw:gradient draw:name="GradDefTok" draw:style="linear" draw:start-color="#000000" draw:end-color="#ffffff"/>"##,
                r##"<style:style style:name="Ts" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>"##,
                r##"<text:list-style style:name="ListStyleTok"><text:list-level-style-bullet text:level="1" text:bullet-char="x"><x:lk k="ListLevelKidTok"/></text:list-level-style-bullet></text:list-style>"##,
            ))
            .page(r##"<draw:rect svg:width="1cm" svg:height="1cm"/>"##),
    );
    survives(
        &d,
        &[
            Probe::Any("MarkerDefTok"),
            Probe::Any("DashDefTok"),
            Probe::Any("HatchDefTok"),
            Probe::Any("GradDefTok"),
            Probe::Any("ListStyleTok"),
            Probe::Any("ListLevelKidTok"),
        ],
    );
}

/// The same values on an **automatic** style of a shape, which holds on the shape: the control of the common styles' tests above.
#[test]
fn the_values_of_a_shapes_own_style_come_through_the_codec() {
    let d = Drawing::a4()
        .auto(concat!(
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="AutoHatchTok" draw:stroke="dash" draw:stroke-dash="AutoDashTok" "##,
            r##"draw:marker-start="AutoMarkerTok" draw:shadow="visible" draw:shadow-opacity="37%" fo:padding="0.17cm" style:writing-mode="tb-rl"/></style:style>"##
        ))
        .page(r##"<draw:rect draw:style-name="gr1" svg:width="1cm" svg:height="1cm"/>"##);
    survives(
        &d,
        &[
            Probe::Attr("draw:fill-hatch-name", "AutoHatchTok"),
            Probe::Attr("draw:stroke-dash", "AutoDashTok"),
            Probe::Attr("draw:marker-start", "AutoMarkerTok"),
            Probe::Attr("draw:shadow-opacity", "37%"),
            Probe::Attr("fo:padding", "0.17cm"),
            Probe::Attr("style:writing-mode", "tb-rl"),
        ],
    );
}

/// What an automatic style's text properties say that the cells only approximate.
#[test]
fn the_text_properties_of_an_automatic_style_come_through_the_codec() {
    let d = Drawing::a4()
        .auto(concat!(
            r##"<style:style style:name="gr2" style:family="graphic"><style:text-properties style:text-underline-style="wave" style:text-underline-color="#112233"/></style:style>"##,
            r##"<style:style style:name="P2" style:family="paragraph"><style:text-properties style:text-line-through-style="dash"/></style:style>"##,
            r##"<style:style style:name="T2" style:family="text"><style:text-properties style:text-overline-style="wave" fo:text-transform="lowercase"/></style:style>"##,
        ))
        .page(r##"<draw:rect draw:style-name="gr2" svg:width="1cm" svg:height="1cm"><text:p text:style-name="P2">a<text:span text:style-name="T2">b</text:span></text:p></draw:rect>"##);
    survives(
        &d,
        &[
            Probe::Attr("style:text-underline-style", "wave"),
            Probe::Attr("style:text-underline-color", "#112233"),
            Probe::Attr("style:text-line-through-style", "dash"),
            Probe::Attr("style:text-overline-style", "wave"),
            Probe::Attr("fo:text-transform", "lowercase"),
        ],
    );
}

#[test]
fn a_master_page_s_own_layers_and_children_come_through_the_codec() {
    let bytes = package_with(
        &plain(),
        &[],
        &[(
            r##"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"##,
            r##"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:layer-set><draw:layer draw:name="MasterLayerTok" draw:protected="true" x:la="LayerAttrTok"/></draw:layer-set><x:mpchild a="MpKidTok"/></style:master-page>"##,
        )],
    );
    survives_bytes(&bytes, &[Probe::Any("MasterLayerTok"), Probe::Any("LayerAttrTok"), Probe::Any("MpKidTok")]);
}

#[test]
fn a_page_s_own_layer_set_comes_through_the_codec() {
    let d = plain();
    let bytes = package_with(
        &d,
        &[
            ("<draw:page ", r##"<draw:page x:pa="P" "##),
            (
                r##"<draw:rect"##,
                r##"<draw:layer-set x:sa="PageLayerSetTok"><draw:layer draw:name="PageLayerTok" x:la="PageLayerAttrTok"/><x:lk k="PageLayerKidTok"/></draw:layer-set><draw:rect"##,
            ),
        ],
        &[],
    );
    survives_bytes(
        &bytes,
        &[Probe::Any("PageLayerTok"), Probe::Any("PageLayerAttrTok"), Probe::Any("PageLayerSetTok"), Probe::Any("PageLayerKidTok")],
    );
}

/// A text that holds nothing but a paragraph with its own attributes and a heading with no characters.
#[test]
fn an_empty_paragraph_with_attributes_and_an_empty_heading_come_through_the_codec() {
    survives(
        &shape_with(r##"<text:p text:style-name="Pe" text:class-names="EmptyClassTok"/><text:h text:outline-level="3"/>"##),
        &[Probe::Any("EmptyClassTok"), Probe::Any("text:h")],
    );
}
