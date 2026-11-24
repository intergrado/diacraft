//! What the `.odg` reader holds of the document's own data (the named definitions, the layer set, the style elements no style sheet is made of, the style
//! sheets' own slots, the pages' master pages and layouts), of each shape (its held values and their stamps, its style name, its units, a picture's attributes, its glue points)
//! and of each text (its paragraphs and their styles, the nodes it keeps). The inputs are written by hand from the OASIS ODF 1.4 standard.

use std::sync::Arc;

use diacraft_doc::{DefFamily, Document, OdfDoc, Page, stamp_cells};

use crate::drawing::Drawing;

/// The document's `.odg` data.
fn odf_doc(doc: &Document) -> OdfDoc {
    doc.ext.get().map(|d| d.odf.clone()).unwrap_or_default()
}

fn page0(doc: &Document) -> &Page {
    doc.pages.get(0).expect("a page")
}

/// A one-pixel PNG's signature and a few bytes: no decoder reads it, the reader holds the bytes.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-fill-image-bytes-";

/// The six families of definitions in `office:styles`, each as written.
const DEFINITIONS: &str = concat!(
    r##"<draw:marker draw:name="Arrow" draw:display-name="An arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"##,
    r##"<draw:gradient draw:name="G1" draw:display-name="Blue" draw:style="linear" draw:start-color="#ff0000" draw:end-color="#0000ff" draw:angle="0.5rad"/>"##,
    r##"<draw:opacity draw:name="O1" draw:style="linear" draw:start="100%" draw:end="0%"/>"##,
    r##"<draw:hatch draw:name="H1" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##,
    r##"<draw:stroke-dash draw:name="D1" draw:style="rect" draw:dots1="1" draw:dots1-length="0.2cm" draw:distance="0.2cm"/>"##,
    r##"<draw:fill-image draw:name="Img" xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"##,
    r##"<draw:marker draw:name="Arrow" svg:viewBox="0 0 1 1" svg:d="M0 0"/>"##,
);

fn with_definitions() -> Drawing {
    let mut d = Drawing::a4().common(DEFINITIONS);
    d.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    d
}

/// Each named definition of `office:styles` is held as written, in file order, whatever its family; a repeated name is held too; and a fill image's bytes are the picture
/// its href names, shared and not copied.
#[test]
fn named_definitions_are_held_exactly_in_file_order() {
    let (doc, _) = with_definitions().read();
    let odf = odf_doc(&doc);
    let held: Vec<(DefFamily, &str)> = odf.definitions.iter().map(|d| (d.family, &*d.name)).collect();
    assert_eq!(
        held,
        [
            (DefFamily::Marker, "Arrow"),
            (DefFamily::Gradient, "G1"),
            (DefFamily::Opacity, "O1"),
            (DefFamily::Hatch, "H1"),
            (DefFamily::StrokeDash, "D1"),
            (DefFamily::FillImage, "Img"),
            (DefFamily::Marker, "Arrow"),
        ]
    );
    let marker = &odf.definitions[0].element;
    assert_eq!(&*marker.name, "draw:marker");
    assert_eq!(marker.attrs.get("draw:display-name"), Some("An arrow"));
    assert_eq!(marker.attrs.get("svg:d"), Some("M10 0l-10 30h20z"));
    assert_eq!(odf.definitions[1].element.attrs.get("draw:angle"), Some("0.5rad"), "an attribute as written, not as read");
    assert_eq!(odf.definitions[3].element.attrs.get("draw:rotation"), Some("450"));
    assert!(odf.definitions[..5].iter().all(|d| d.bytes.is_none()), "only a fill image holds bytes");
    assert_eq!(odf.definitions[5].bytes.as_deref(), Some(PNG), "the picture the href names");
}

/// A fill image's picture is the definition's bytes and **no part of the model besides** (keeps every package entry no field reads as a part, and leaves out the
/// pictures the model holds): the entry that a definition takes is not in `Document.parts`, and an entry nothing names is.
#[test]
fn a_fill_image_entry_is_the_definition_s_bytes_and_not_also_a_part() {
    let mut d = with_definitions();
    d.files.push(("Pictures/unnamed.png".to_owned(), PNG.to_vec()));
    let (doc, _) = d.read();
    let odf = odf_doc(&doc);
    assert_eq!(odf.definitions[5].bytes.as_deref(), Some(PNG), "the definition holds the picture");
    let parts: Vec<&str> = doc.parts.iter().map(|p| &*p.name).collect();
    assert!(!parts.contains(&"Pictures/a.png"), "the picture a definition holds is held once: {parts:?}");
    assert!(parts.contains(&"Pictures/unnamed.png"), "an entry nothing names is a part: {parts:?}");
}

/// A definition with no `draw:name` is no definition: it is kept whole as a style element.
#[test]
fn a_definition_with_no_name_is_a_kept_style_element() {
    let d = Drawing::a4().common(r##"<draw:hatch draw:style="single" draw:color="#000000"/>"##);
    let (doc, _) = d.read();
    let odf = odf_doc(&doc);
    assert!(odf.definitions.is_empty());
    let kept = odf.styles.iter().find(|s| &*s.family == "common:hatch").expect("held whole");
    assert_eq!(kept.element.attrs.get("draw:color"), Some("#000000"));
}

/// The document's layer set is held whole and in order, LibreOffice's five included; the style elements no style sheet is made of are held whole by container, family and
/// name: a text style, a list style, a default style, a page layout, an automatic graphic style and a drawing-page style.
#[test]
fn the_layer_set_and_the_other_style_families_are_held() {
    let mut d = Drawing::a4();
    d.masters = r##"<draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="mine" draw:display="screen" draw:protected="true"><svg:title>t</svg:title></draw:layer></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"##.to_owned();
    d = d.common(concat!(
        r##"<style:style style:name="Emph" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>"##,
        r##"<style:default-style style:family="graphic"><style:graphic-properties draw:fill="none"/></style:default-style>"##,
        r##"<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style>"##,
        r##"<style:style style:name="Std" style:family="graphic"><style:graphic-properties draw:fill="solid"/></style:style>"##,
    ));
    d = d.auto(concat!(
        r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Std"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H1"/></style:style>"##,
        r##"<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties draw:background-size="border"/></style:style>"##,
    ));
    d = d.page("");
    d.pages = d.pages.replacen("draw:master-page-name=\"Default\"", "draw:master-page-name=\"Default\" draw:style-name=\"dp1\"", 1);
    let (doc, _) = d.read();
    let odf = odf_doc(&doc);
    let layers: Vec<(&str, Option<&str>)> = odf.layers.iter().map(|l| (l.attrs.get("draw:name").unwrap(), l.attrs.get("draw:display"))).collect();
    assert_eq!(layers, [("layout", None), ("mine", Some("screen"))]);
    assert_eq!(odf.layers[1].attrs.get("draw:protected"), Some("true"));
    assert_eq!(odf.layers[1].children.len(), 1, "the layer's own child is kept");
    let held: Vec<(&str, &str, &str)> = odf.styles.iter().map(|s| (&*s.family, &*s.name, &*s.element.name)).collect();
    for expected in [
        ("common:text", "Emph", "style:style"),
        ("common:graphic", "", "style:default-style"),
        ("common:list-style", "L1", "text:list-style"),
        ("auto:graphic", "gr1", "style:style"),
        ("auto:drawing-page", "dp1", "style:style"),
        ("auto:page-layout", "PM1", "style:page-layout"),
    ] {
        assert!(held.contains(&expected), "{expected:?} is held whole: {held:?}");
    }
    assert!(
        !held.iter().any(|(family, name, _)| *family == "common:graphic" && *name == "Std"),
        "a common graphic style is a style sheet, not a kept element"
    );
    // A page that names a drawing-page style holds the element the document holds.
    let style = page0(&doc).ext.get().and_then(|e| e.odf.drawing_page_style.clone()).expect("the page's drawing-page style");
    assert_eq!(&*style.name, "style:style");
    let kept = odf.styles.iter().find(|s| &*s.name == "dp1").expect("dp1");
    assert!(Arc::ptr_eq(&style, &kept.element), "the page and the document share one element");
}

/// An automatic style of the graphic, paragraph and text families is kept as **what no cell reads**: its attributes, and for each properties element the attributes and children no cell
/// reads (a mapped property is in the cells, and a value the style gives its shapes and paragraphs is held with them), and none is kept for a style that holds nothing of the kind.
#[test]
fn an_automatic_style_is_kept_as_what_no_cell_reads() {
    let d = Drawing::a4().auto(concat!(
        r##"<style:style style:name="only" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" svg:stroke-width="1pt"/></style:style>"##,
        r##"<style:style style:name="more" style:family="graphic" style:class="c"><style:graphic-properties draw:fill="solid" draw:textarea-horizontal-align="center" draw:auto-grow-height="false"><x:kid/></style:graphic-properties>"##,
        r##"<style:paragraph-properties fo:text-align="center" fo:border="1pt"/><style:map style:condition="c"/></style:style>"##,
        r##"<style:style style:name="T9" style:family="text"><style:text-properties fo:font-size="9pt" style:text-blinking="true"/></style:style>"##,
    ));
    let mut d = d;
    d.ns = d.ns.replace("xmlns:diacraft", "xmlns:x=\"urn:example:extension\" xmlns:diacraft");
    let (doc, _) = d.read();
    let odf = odf_doc(&doc);
    let kept = |name: &str| odf.styles.iter().find(|s| &*s.name == name && s.family.starts_with("auto:"));
    assert!(kept("only").is_none(), "a style of cells alone is kept as nothing");
    let more = kept("more").expect("a style with more");
    assert_eq!(&*more.family, "auto:graphic");
    let names = |e: &diacraft_doc::RawElement| -> Vec<String> {
        e.children
            .iter()
            .filter_map(|c| match c {
                diacraft_doc::RawNode::Element(k) => Some(k.name.to_string()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(names(&more.element), ["style:graphic-properties", "style:paragraph-properties", "style:map"]);
    assert_eq!(more.element.attrs.get("style:class"), Some("c"), "the style's own attributes are kept");
    let diacraft_doc::RawNode::Element(graphic) = &more.element.children[0] else { panic!("an element") };
    let attrs: Vec<&str> = graphic.attrs.iter().map(|(n, _)| n).collect();
    assert_eq!(attrs, ["draw:textarea-horizontal-align", "draw:auto-grow-height"], "the attributes no cell reads, and not draw:fill");
    assert_eq!(names(graphic), ["x:kid"], "and the child nothing reads");
    let diacraft_doc::RawNode::Element(paragraph) = &more.element.children[1] else { panic!("an element") };
    assert_eq!(paragraph.attrs.iter().map(|(n, _)| n).collect::<Vec<_>>(), ["fo:border"]);
    let text = kept("T9").expect("a text style with an attribute no cell reads");
    assert_eq!(&*text.family, "auto:text");
}

/// A style sheet's own slot holds the names, the family, the attributes of its properties no cell reads (keyed by the properties element), the lengths in `px` that no cell
/// spells in their unit, and what its element holds besides.
#[test]
fn a_style_sheet_holds_what_no_cell_reads() {
    let d = Drawing::a4().common(concat!(
        r##"<style:style style:name="Std" style:display-name="Standard" style:family="graphic" style:parent-style-name="Base" style:class="x">"##,
        r##"<style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:fill-hatch-name="H1" svg:stroke-width="3px" loext:odd="1" draw:shadow-blur="2px">"##,
        r##"<x:kid k="1"/></style:graphic-properties>"##,
        r##"<style:paragraph-properties fo:text-align="center" fo:line-height="12px" fo:border="1pt solid #000000"/>"##,
        r##"<style:text-properties fo:font-size="12pt" style:text-blinking="true"/>"##,
        r##"<style:map style:condition="true"/>"##,
        r##"</style:style>"##,
        r##"<style:style style:name="Base" style:family="graphic"/>"##,
    ));
    let mut d = d;
    d.ns = d.ns.replace("xmlns:diacraft", "xmlns:loext=\"urn:example:loext\" xmlns:x=\"urn:example:extension\" xmlns:diacraft");
    let (doc, _) = d.read();
    let style = doc.styles.iter().find(|s| s.name_u.as_deref() == Some("Std")).expect("the style sheet");
    let odf = style.ext.get().expect("the sheet's slot").odf.clone();
    assert_eq!(odf.name.as_deref(), Some("Std"));
    assert_eq!(odf.display_name.as_deref(), Some("Standard"));
    assert_eq!(odf.parent.as_deref(), Some("Base"));
    assert_eq!(odf.family.as_deref(), Some("graphic"));
    assert_eq!(odf.unmapped.get("style:graphic-properties/draw:fill-hatch-name"), Some("H1"));
    assert_eq!(odf.unmapped.get("style:graphic-properties/loext:odd"), Some("1"));
    assert_eq!(odf.unmapped.get("style:graphic-properties/draw:shadow-blur"), Some("2px"));
    assert_eq!(odf.unmapped.get("style:paragraph-properties/fo:border"), Some("1pt solid #000000"));
    assert_eq!(odf.unmapped.get("style:text-properties/style:text-blinking"), Some("true"));
    assert!(odf.unmapped.get("style:graphic-properties/draw:fill").is_none(), "an attribute a cell reads is not held here");
    assert!(odf.unmapped.get("style:text-properties/fo:font-size").is_none());
    assert_eq!(odf.units.get("svg:stroke-width"), Some("px"), "a px length has no unit in a cell");
    assert_eq!(odf.units.get("fo:line-height"), Some("px"));
    assert!(odf.units.get("fo:font-size").is_none(), "12pt is the cell's own unit");
    assert_eq!(odf.kept.raw.get("style:class"), Some("x"));
    assert!(odf.kept.raw.get("style:name").is_none() && odf.kept.raw.get("style:parent-style-name").is_none());
    let kept: Vec<&str> = odf
        .kept
        .kept
        .iter()
        .map(|p| match &p.node {
            diacraft_doc::RawNode::Element(e) => &*e.name,
            _ => "?",
        })
        .collect();
    assert_eq!(
        kept,
        ["style:map", "style:graphic-properties"],
        "the style's own child, and the child of its properties no cell reads (under the properties' name)"
    );
}

/// A page holds its master page's name and attributes, its layout's name and the layout's properties no cell reads, the units of the layout's lengths and the stamp of its
/// cells.
#[test]
fn a_page_holds_its_master_page_and_its_layout() {
    let mut d = Drawing::a4();
    d.styles_auto = r##"<style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" fo:margin-right="96px" fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait" style:writing-mode="lr-tb" style:footnote-max-height="0cm"/><style:header-style/></style:page-layout>"##.to_owned();
    d.masters = r##"<draw:layer-set><draw:layer draw:name="layout"/></draw:layer-set><style:master-page style:name="Default" style:display-name="Def" style:page-layout-name="PM1" draw:style-name="mp1"/>"##.to_owned();
    let (doc, _) = d.page("").read();
    let page = page0(&doc);
    let odf = page.ext.get().expect("the page's slot").odf.clone();
    assert_eq!(odf.master_page.as_deref(), Some("Default"));
    assert_eq!(odf.layout.as_deref(), Some("PM1"));
    assert_eq!(odf.master_page_attrs.get("style:display-name"), Some("Def"));
    assert_eq!(odf.master_page_attrs.get("draw:style-name"), Some("mp1"));
    assert_eq!(odf.layout_props.get("style:writing-mode"), Some("lr-tb"));
    assert_eq!(odf.layout_props.get("style:footnote-max-height"), Some("0cm"));
    assert!(odf.layout_props.get("fo:page-width").is_none() && odf.layout_props.get("style:print-orientation").is_none(), "the cells read these");
    assert_eq!(odf.units.get("fo:margin-right"), Some("px"));
    let cells = ["PageWidth", "PageHeight", "PageLeftMargin", "PageRightMargin", "PageTopMargin", "PageBottomMargin", "PrintPageOrientation"];
    assert_eq!(odf.stamp, Some(stamp_cells(&page.sheet, &cells)), "the stamp of the page's cells as the reader set them");
    // A cell edit makes the stamp stale, and putting the cell back makes it good again.
    let mut edited = (*page.sheet).clone();
    let before = edited.cells.get("PageWidth").cloned().expect("PageWidth");
    edited.cells.insert(diacraft_doc::CellName::new("PageWidth"), diacraft_doc::Cell::new());
    assert_ne!(odf.stamp, Some(stamp_cells(&edited, &cells)));
    edited.cells.insert(diacraft_doc::CellName::new("PageWidth"), before);
    assert_eq!(odf.stamp, Some(stamp_cells(&edited, &cells)));
    // The layout is also held whole.
    assert!(odf_doc(&doc).styles.iter().any(|s| &*s.family == "auto:page-layout" && &*s.name == "PM1" && s.element.children.len() == 2));
}
