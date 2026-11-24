//! What makes no content for VSDX packages (the classes F1 and F2): an empty container and an absent one, an empty
//! relationships part and none, and the order of a shape's sections, text and foreign data among themselves. Each case that the check now calls the same
//! has a control beside it that real content in the same place still differs. Every package is built in code (`diacraft-testkit`). Holds
//! the rules to the words: only a relationships part (by its name) is let go when it holds nothing, and a container only under its known parent.

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_testkit::package::{CT_RELS, MasterSpec, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::{Limits, compare_content};

/// The folder of the format's parts, written in pieces.
const PAGE1_RELS: &str = concat!("/vi", "sio", "/pages/_rels/page1.xml.rels");
const DOCUMENT: &str = concat!("/vi", "sio", "/document.xml");
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn diff(a: &[u8], b: &[u8]) -> ContentDiff {
    let (ab, ba) = (compare_content(a, b, &Limits::default()).unwrap(), compare_content(b, a, &Limits::default()).unwrap());
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab
}

#[track_caller]
fn same(a: &[u8], b: &[u8], what: &str) {
    let d = diff(a, b);
    assert!(d.same(), "{what}: {:?}", d.shown);
}

#[track_caller]
fn differs(a: &[u8], b: &[u8], what: &str) -> ContentDiff {
    let d = diff(a, b);
    assert!(!d.same(), "{what}: the check is blind to it");
    d
}

/// `builder`'s package with `from` replaced by `to` in the part `part` (once, and it must be there).
fn edited(builder: &VsdxBuilder, part: &str, from: &str, to: &str) -> Vec<u8> {
    builder
        .pkg()
        .map_parts(|name, bytes| {
            if name == part {
                let text = String::from_utf8(bytes.clone()).unwrap();
                assert_eq!(text.matches(from).count(), 1, "{from} in {name}");
                *bytes = text.replacen(from, to, 1).into_bytes();
            }
        })
        .build()
}

fn one_page(shapes: &str) -> VsdxBuilder {
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes))
}

const BOX: &str = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/></Shape>"#;

// ------------------------------------------------------------------------------------------------------------ an empty relationships part (F1a)

#[test]
fn an_empty_relationships_part_is_the_same_as_none() {
    let base = one_page(BOX);
    let without = base.build();
    for spelling in [
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"{RELS_NS}\"/>"),
        format!("<Relationships xmlns=\"{RELS_NS}\"></Relationships>"),
        format!("<Relationships xmlns=\"{RELS_NS}\">\r\n</Relationships>"),
    ] {
        let with = base.clone().with(Pkg::new().part_without_override(PAGE1_RELS, spelling.as_bytes())).build();
        same(&with, &without, &spelling);
    }
}

#[test]
fn a_relationships_part_with_a_row_or_anything_else_is_still_a_part() {
    let base = one_page(BOX);
    let without = base.build();
    for (spelling, what) in [
        (format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"x.xml\"/></Relationships>"), "a row"),
        (format!("<Relationships xmlns=\"{RELS_NS}\" a=\"1\"/>"), "an attribute"),
        ("<Relationships xmlns=\"urn:not-opc\"/>".to_owned(), "another namespace"),
        (format!("<Relationships xmlns=\"{RELS_NS}\"><?x y?></Relationships>"), "an instruction"),
    ] {
        let with = base.clone().with(Pkg::new().part_without_override(PAGE1_RELS, spelling.as_bytes())).build();
        let d = differs(&with, &without, what);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart), "{what}");
    }
    // The page's relationships part typed by an `Override` in the one package: the part is no difference, and the row of the content types still is.
    let typed = base.clone().with(Pkg::new().part(PAGE1_RELS, CT_RELS, format!("<Relationships xmlns=\"{RELS_NS}\"/>"))).build();
    let d = differs(&typed, &without, "an Override row");
    assert!(d.shown.iter().all(|x| x.part == "[Content_Types].xml"), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ empty containers (F1b, F1c)

#[test]
fn an_empty_container_of_the_document_part_is_the_same_as_an_absent_one() {
    let base = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(BOX));
    let plain = base.build();
    for (from, what) in [("<FaceNames></FaceNames>", "FaceNames"), ("<DocumentSheet></DocumentSheet>", "DocumentSheet")] {
        same(&edited(&base, DOCUMENT, from, ""), &plain, what);
        same(&edited(&base, DOCUMENT, from, &format!("<{what}/>")), &plain, what);
    }
    // No styles at all: an empty `StyleSheets` and none.
    let bare = VsdxBuilder::new().styles("").page(PageSpec::new(1, "P").shapes(BOX));
    same(&edited(&bare, DOCUMENT, "<StyleSheets></StyleSheets>", ""), &bare.build(), "StyleSheets");
    // An empty `Colors` and none.
    let coloured = base.clone().colors("");
    same(&coloured.build(), &plain, "Colors");
}

#[test]
fn a_container_of_the_document_part_that_holds_anything_still_differs_from_none() {
    let base = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(BOX));
    let plain = base.build();
    differs(&base.clone().colors(r##"<ColorEntry IX="0" RGB="#000000"/>"##).build(), &plain, "a colour");
    differs(&base.clone().face_names(r#"<FaceName NameU="Sans" Name="Sans"/>"#).build(), &plain, "a face");
    differs(&base.clone().document_sheet(r#"<Cell N="OutputFormat" V="0"/>"#).build(), &plain, "a cell of the document sheet");
    differs(
        &edited(&base, DOCUMENT, "<StyleSheets>", "<StyleSheets a=\"1\">"),
        &edited(&base, DOCUMENT, "<StyleSheets>", "<StyleSheets>"),
        "an attribute",
    );
    // The root style taken out is a style less, whatever the container.
    let bare = VsdxBuilder::new().styles("").page(PageSpec::new(1, "P").shapes(BOX));
    differs(&bare.build(), &plain, "the root style");
    // An element of the document part that is not on the list, empty, is still content (`EventList`).
    differs(&base.clone().document_extra("<EventList/>").build(), &plain, "an empty EventList");
}

#[test]
fn an_empty_connects_or_shapes_of_a_page_or_a_group_is_the_same_as_none() {
    let root = concat!("<PageContents xmlns=\"http://schemas.microsoft.com/office/vi", "sio", "/2012/main\">");
    let part = |inner: &str| format!("<?xml version=\"1.0\"?>{root}{inner}</PageContents>");
    let page = |inner: &str| VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part(inner))).build();
    same(&page(&format!("<Shapes>{BOX}</Shapes><Connects/>")), &page(&format!("<Shapes>{BOX}</Shapes>")), "Connects");
    same(&page("<Shapes></Shapes>"), &page(""), "Shapes");
    same(&page("<Shapes/><Connects> </Connects>"), &page(""), "both");
    let group = |inner: &str| page(&format!("<Shapes><Shape ID=\"1\" Type=\"Group\"><Cell N=\"PinX\" V=\"1\"/>{inner}</Shape></Shapes>"));
    same(&group("<Shapes/>"), &group(""), "a group's Shapes");
    // Controls: a shape in it, a row in it, an attribute on it.
    differs(&group(&format!("<Shapes>{}</Shapes>", BOX.replace("ID=\"1\"", "ID=\"2\""))), &group(""), "a member");
    differs(
        &page(&format!("<Shapes>{BOX}</Shapes><Connects><Connect FromSheet=\"1\" ToSheet=\"1\"/></Connects>")),
        &page(&format!("<Shapes>{BOX}</Shapes>")),
        "a row",
    );
    differs(&page("<Shapes a=\"1\"/>"), &page(""), "an attribute");
}

// ------------------------------------------------------------------------------------------------------------ a shape's children (F2)

/// A page with one shape whose children are the cells, then `inner`.
fn shape_with(inner: &str) -> Vec<u8> {
    one_page(&format!(r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="1"/>{inner}</Shape>"#)).build()
}

const GEOMETRY: &str = r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row></Section>"#;
const PROPERTY: &str = r#"<Section N="Property"><Row N="Cost"><Cell N="Value" V="3"/></Row></Section>"#;
const TEXT: &str = "<Text>Hello</Text>";
const FOREIGN: &str = r#"<ForeignData ForeignType="Bitmap"/>"#;

#[test]
fn a_shapes_sections_text_and_foreign_data_in_another_order_are_the_same() {
    let written = shape_with(&format!("{GEOMETRY}{PROPERTY}{TEXT}{FOREIGN}"));
    for (order, what) in [
        (format!("{TEXT}{GEOMETRY}{PROPERTY}{FOREIGN}"), "the text before the sections"),
        (format!("{GEOMETRY}{PROPERTY}{FOREIGN}{TEXT}"), "the foreign data before the text"),
        (format!("{GEOMETRY}{TEXT}{PROPERTY}{FOREIGN}"), "the text before the property section"),
        (format!("{FOREIGN}{TEXT}{GEOMETRY}{PROPERTY}"), "all of them the other way"),
    ] {
        same(&shape_with(&order), &written, what);
    }
    // With the cells after them, and with `Shapes` where it was.
    let members = r#"<Shapes><Shape ID="2" Type="Shape"/></Shapes>"#;
    same(
        &one_page(&format!(r#"<Shape ID="1" Type="Group">{TEXT}{GEOMETRY}<Cell N="PinX" V="1"/>{members}</Shape>"#)).build(),
        &one_page(&format!(r#"<Shape ID="1" Type="Group"><Cell N="PinX" V="1"/>{GEOMETRY}{TEXT}{members}</Shape>"#)).build(),
        "a group",
    );
}

#[test]
fn the_order_of_the_sections_and_every_other_child_of_a_shape_is_still_content() {
    let written = shape_with(&format!("{GEOMETRY}{PROPERTY}{TEXT}"));
    differs(&shape_with(&format!("{PROPERTY}{GEOMETRY}{TEXT}")), &written, "two sections swapped");
    let d = differs(&shape_with(&format!("{GEOMETRY}{PROPERTY}<Text>Hullo</Text>")), &written, "another text");
    assert_eq!(d.count, 1, "{:?}", d.shown);
    // `Shapes` and `Data1` are not of the set: their place against the text is content.
    let members = r#"<Shapes><Shape ID="2" Type="Shape"/></Shapes>"#;
    differs(&shape_with(&format!("{members}{TEXT}")), &shape_with(&format!("{TEXT}{members}")), "Shapes before the text");
    differs(&shape_with(&format!("<Data1>a</Data1>{TEXT}")), &shape_with(&format!("{TEXT}<Data1>a</Data1>")), "Data1 before the text");
    // The order of the children of a section is not the shape's.
    let rows_a = r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"/><Row T="LineTo" IX="2"/></Section>"#;
    let rows_b = r#"<Section N="Geometry" IX="0"><Row T="LineTo" IX="2"/><Row T="MoveTo" IX="1"/></Section>"#;
    differs(&shape_with(rows_a), &shape_with(rows_b), "two rows swapped");
}

// ------------------------------------------------------------------------------------------------------------ only what names

/// The root of a page part, in the format's namespace.
const PAGE_ROOT: &str = concat!("<PageContents xmlns=\"http://schemas.microsoft.com/office/vi", "sio", "/2012/main\">");

/// A one-page package whose page part holds `inner`.
fn page_part(inner: &str) -> Vec<u8> {
    let part = format!("<?xml version=\"1.0\"?>{PAGE_ROOT}{inner}</PageContents>");
    VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build()
}

#[test]
fn a_part_that_is_not_a_relationships_part_is_a_part_whatever_its_root_holds() {
    // The full gate's fuzz crash, in VSDX words: a part that is not a relationships part, with a root that holds nothing, dropped. A save must keep a part it
    // does not know, so dropping it is a difference, whatever its root: an empty `Relationships` of OPC included.
    let base = one_page(BOX);
    let without = base.build();
    let empty_rels = format!("<Relationships xmlns=\"{RELS_NS}\"/>");
    for (name, bytes, what) in [
        ("/extra/data.xml", empty_rels.as_str(), "an XML part whose root is an empty Relationships"),
        ("/extra/rels/data.xml.rels", empty_rels.as_str(), "a .rels part outside a _rels folder"),
        ("/extra/_rels.xml", empty_rels.as_str(), "a part named like the folder"),
        ("/extra/data.xml", "<Empty/>", "an XML part whose root is an empty element of no list"),
        (
            "/extra/data.xml",
            concat!("<Colors xmlns=\"http://schemas.microsoft.com/office/vi", "sio", "/2012/main\"/>"),
            "a part whose root is a listed container",
        ),
    ] {
        let with = base.clone().with(Pkg::new().part_without_override(name, bytes.as_bytes())).build();
        let d = differs(&with, &without, what);
        assert_eq!(d.count, 1, "{what}: {:?}", d.shown);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart), "{what}");
    }
}

#[test]
fn an_empty_relationships_part_named_in_any_case_is_the_same_as_none() {
    // OPC folds a part name by ASCII case: `_RELS` and `.RELS` name a relationships part too (the own case, the control of the test above).
    let base = one_page(BOX);
    let without = base.build();
    for name in [PAGE1_RELS, concat!("/vi", "sio", "/pages/_RELS/page1.xml.RELS"), "/_rels/extra.xml.rels"] {
        let with = base.clone().with(Pkg::new().part_without_override(name, format!("<Relationships xmlns=\"{RELS_NS}\"/>"))).build();
        same(&with, &without, name);
    }
}

#[test]
fn a_listed_container_outside_its_known_parent_is_content() {
    // Names each container with its parent: the document part's four under `VisioDocument`, `Connects` and `Shapes` under a page's or a master's root,
    // and a group's `Shapes` under its `Shape`. Anywhere else an empty one is an element of the part, which a save keeps.
    let base = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(BOX));
    let plain = base.build();
    differs(&base.clone().document_extra("<Connects/>").build(), &plain, "Connects in the document part");
    differs(&base.clone().document_extra("<Shapes/>").build(), &plain, "Shapes in the document part");
    differs(
        &base.clone().document_extra("<EventList><Colors/></EventList>").build(),
        &base.clone().document_extra("<EventList/>").build(),
        "Colors in another element",
    );
    let members = format!("<Shapes>{BOX}</Shapes>");
    differs(&page_part(&format!("{members}<Colors/>")), &page_part(&members), "Colors in a page");
    differs(&page_part(&format!("{members}<FaceNames/>")), &page_part(&members), "FaceNames in a page");
    differs(&page_part(&format!("{members}<Extra><Shapes/></Extra>")), &page_part(&format!("{members}<Extra/>")), "Shapes in an unknown element");
    differs(&page_part(&format!("{members}<Extra><Connects/></Extra>")), &page_part(&format!("{members}<Extra/>")), "Connects in an unknown element");
    differs(&shape_with("<Connects/>"), &shape_with(""), "Connects in a shape");
    differs(&shape_with("<DocumentSheet/>"), &shape_with(""), "DocumentSheet in a shape");
    differs(
        &shape_with(r#"<Section N="Geometry" IX="0"><Shapes/></Section>"#),
        &shape_with(r#"<Section N="Geometry" IX="0"></Section>"#),
        "Shapes in a section",
    );
}

#[test]
fn an_empty_connects_or_shapes_of_a_master_is_the_same_as_none() {
    // The master case, beside the page's and the group's above.
    let root = concat!("<MasterContents xmlns=\"http://schemas.microsoft.com/office/vi", "sio", "/2012/main\">");
    let master = |inner: &str| {
        let part = format!("<?xml version=\"1.0\"?>{root}{inner}</MasterContents>");
        VsdxBuilder::new().master(MasterSpec::new(2, "M").part(&part)).page(PageSpec::new(1, "P").shapes(BOX)).build()
    };
    same(&master(&format!("<Shapes>{BOX}</Shapes><Connects/>")), &master(&format!("<Shapes>{BOX}</Shapes>")), "a master's Connects");
    same(&master("<Shapes></Shapes>"), &master(""), "a master's Shapes");
    differs(&master("<Extra><Shapes/></Extra>"), &master("<Extra/>"), "Shapes in an unknown element of a master");
}
