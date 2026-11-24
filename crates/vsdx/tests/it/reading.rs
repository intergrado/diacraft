//! What the reader makes of the forms the first review and the sample-file run found it did not handle: rows with no name, an `ID` used twice, a
//! relationship of the wrong type, a picture of a format no viewer draws, a theme read slot by slot, the DiaCraft version's own namespace,
//! the dates in the core properties, the sizes a report carries, and what a refused file still reports.

use std::sync::Arc;

use diacraft_doc::{MAX_COLOR_ENTRIES, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET, SchemeId};
use diacraft_testkit::package::{CT_PAGE, CT_PAGES, MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VIS, VsdxBuilder, shape};
use diacraft_vsdx::{LimitKind, Limits, VsdxError, read, read_partial};

use crate::{doc_of, first_shape, read_err, read_ok, read_with};

// ------------------------------------------------------------------------------------------------------------------ rows with no name

#[test]
fn a_row_of_a_name_keyed_section_with_no_n_is_kept_and_warned_of_and_its_local_name_is_not_its_name() {
    let (doc, report) = doc_of(
        r#"<Shape ID="1"><Section N="User"><Row LocalName="Nur lokal"/><Row N="a"/><Row IX="3"/></Section><Section N="Geometry" IX="0"><Row T="LineTo" IX="1"/></Section></Shape>"#,
    );
    let user = &first_shape(&doc).sheet.sections[0];
    assert_eq!(user.rows.len(), 3, "every row is kept");
    assert_eq!(user.rows[0].name, None, "the LocalName is not made the name");
    assert_eq!(user.rows[0].local_name.as_deref(), Some("Nur lokal"));
    // The rows with no `N` of a section keyed by name: the one with a `LocalName` and the one with only an `IX`. A section keyed by index is
    // not held to a name.
    assert_eq!(report.warning_count("row_without_name"), 2);
}

#[test]
fn an_empty_n_on_a_row_is_no_name_and_is_kept_as_written_and_does_not_refuse_the_file() {
    // It refused the whole file as `model_limit` (the model's own check, which the reader's is meant to keep ahead of).
    let (doc, report) = doc_of(
        r#"<Shape ID="1"><Section N="User"><Row N=""><Cell N="Value" V="1"/></Row></Section><Section N="Scratch"><Row N="" IX="0"/></Section></Shape>"#,
    );
    let s = first_shape(&doc);
    let row = &s.sheet.sections[0].rows[0];
    assert_eq!(row.name, None);
    assert_eq!(row.raw.get("N"), Some(""), "the attribute is kept, and wins when the row is written");
    assert_eq!(s.sheet.sections[1].rows[0].ix, Some(0));
    assert_eq!(report.warning_count("row_without_name"), 1, "the section keyed by name; the one keyed by index is not held to one");
}

// -------------------------------------------------------------------------------------------------------------- shape IDs used twice

#[test]
fn a_shape_id_used_twice_in_a_part_is_warned_of_and_a_reference_finds_the_first() {
    let (doc, report) = doc_of(&format!(
        "{}{}{}",
        r#"<Shape ID="7" Name="first"/>"#,
        r#"<Shape ID="8"><Shapes><Shape ID="7" Name="nested"/></Shapes></Shape>"#,
        r#"<Shape ID="7" Name="third"/>"#
    ));
    assert_eq!(report.warning_count("duplicate_shape_id"), 2);
    assert_eq!(report.warning_count("duplicate_shape_id_deleted"), 0);
    // The document's own lookup is the first in document order: the model keeps all three.
    let found = doc.find_shape(diacraft_doc::Owner::Page(1), 7).unwrap();
    assert_eq!(doc.shape(&found).unwrap().name.as_deref(), Some("first"));
}

#[test]
fn a_shape_id_used_twice_with_one_of_them_deleted_has_a_class_of_its_own() {
    // 487 of the sample files' 495 repeated IDs are between shapes of which one is deleted: they are said apart.
    let (_, report) = doc_of(r#"<Shape ID="7"/><Shape ID="7" Del="1"/><Shape ID="9" Del="1"/><Shape ID="9"/><Shape ID="7"/>"#);
    assert_eq!(report.warning_count("duplicate_shape_id_deleted"), 2);
    assert_eq!(report.warning_count("duplicate_shape_id"), 1, "the third 7 follows two shapes of that ID");
    let (_, parts) = {
        // The same ID in two parts is not a repeat.
        let b = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(r#"<Shape ID="7"/>"#)).page(PageSpec::new(2, "Q").shapes(r#"<Shape ID="7"/>"#));
        read_ok(&b.build())
    };
    assert_eq!(parts.warning_count("duplicate_shape_id"), 0);
}

// ---------------------------------------------------------------------------------------------- relationships of the wrong type

fn pages_with_rel(ty: &str) -> Vec<u8> {
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"5\"/></Shapes></PageContents>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", ty, "page1.xml");
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn a_page_row_reads_its_part_only_through_a_relationship_of_the_page_type() {
    let (doc, report) = read_ok(&pages_with_rel(&format!("{VIS}page")));
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 1);
    assert_eq!(report.warning_count("relationship_wrong_type"), 0);
    // `.../Page` is not `.../page` (a type is compared exactly, V5 of the package spec), and an `image` is no page.
    for wrong in [format!("{VIS}Page"), format!("{OD}image"), format!("{VIS}master")] {
        let (doc, report) = read_ok(&pages_with_rel(&wrong));
        assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 0, "{wrong}");
        assert_eq!(report.warning_count("relationship_wrong_type"), 1, "{wrong}");
    }
}

#[test]
fn a_master_row_reads_its_part_only_through_a_relationship_of_the_master_type() {
    let masters = format!("<Masters xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Master ID=\"1\" NameU=\"M\"><Rel r:id=\"rId1\"/></Master></Masters>");
    let part = format!("<MasterContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"5\"/></Shapes></MasterContents>");
    let build = |ty: &str| {
        let pkg = Pkg::new()
            .part("/visio/masters/masters.xml", diacraft_testkit::package::CT_MASTERS, masters.clone())
            .rel("/visio/document.xml", "rId60", &format!("{VIS}masters"), "masters/masters.xml")
            .part("/visio/masters/master1.xml", diacraft_testkit::package::CT_MASTER, part.clone())
            .rel("/visio/masters/masters.xml", "rId1", ty, "master1.xml");
        VsdxBuilder::new().without_masters_part().with(pkg).build()
    };
    assert_eq!(read_ok(&build(&format!("{VIS}master"))).0.masters.get(0).unwrap().shapes.len(), 1);
    let (doc, report) = read_ok(&build(&format!("{VIS}page")));
    assert_eq!(doc.masters.get(0).unwrap().shapes.len(), 0);
    assert_eq!(report.warning_count("relationship_wrong_type"), 1);
}

#[test]
fn two_relationships_to_a_document_part_are_warned_of_and_the_first_is_the_document() {
    let bytes = VsdxBuilder::new().with(Pkg::new().rel("/", "rId77", &format!("{VIS}document"), "visio/document.xml")).build();
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.pages.len(), 0);
    assert_eq!(report.warning_count("several_relationships_of_one_type"), 1);
}

#[test]
fn a_document_part_in_a_namespace_the_reader_does_not_read_is_said_to_be_that_and_not_a_wrong_root() {
    let doc = r#"<VisioDocument xmlns="http://schemas.microsoft.com/visio/2003/core"/>"#;
    let bytes = VsdxBuilder::new()
        .pkg()
        .map_parts(|name, b| {
            if name.ends_with("document.xml") {
                *b = doc.as_bytes().to_vec()
            }
        })
        .build();
    let VsdxError::NoDrawingParts(why) = read_err(&bytes, &Limits::default()) else { panic!("expected no_drawing_parts") };
    assert!(why.contains("a VisioDocument in a namespace this reader does not read"), "{why}");
    let other = r#"<Other xmlns="http://schemas.microsoft.com/office/visio/2012/main"/>"#;
    let bytes = VsdxBuilder::new()
        .pkg()
        .map_parts(|name, b| {
            if name.ends_with("document.xml") {
                *b = other.as_bytes().to_vec()
            }
        })
        .build();
    let VsdxError::NoDrawingParts(why) = read_err(&bytes, &Limits::default()) else { panic!("expected no_drawing_parts") };
    assert!(why.contains("is not VisioDocument"), "{why}");
}

// ----------------------------------------------------------------------------------------------- markup compatibility

#[test]
fn must_understand_on_a_root_opens_the_file_with_one_warning_and_a_clean_root_has_none() {
    const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    let part = format!(
        "<PageContents xmlns=\"{NS_2012}\" xmlns:mc=\"{MC}\" xmlns:x=\"urn:example\" mc:MustUnderstand=\"x\"><Shapes><Shape ID=\"1\"/></Shapes></PageContents>"
    );
    let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build());
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 1, "the file opens and the part is read");
    assert_eq!(report.warning_count("must_understand"), 1);
    // `MustUnderstand` with a prefix bound to something else is no markup-compatibility attribute.
    let other = format!("<PageContents xmlns=\"{NS_2012}\" xmlns:mc=\"urn:other\" mc:MustUnderstand=\"x\"/>");
    assert_eq!(read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(&other)).build()).1.warning_count("must_understand"), 0);
    assert_eq!(doc_of(r#"<Shape ID="1"/>"#).1.warning_count("must_understand"), 0);
}

#[test]
fn an_alternate_content_element_under_a_shape_is_kept_whole_with_its_fallback_in_it() {
    // V11 of the preservation spec, the half that is reading: the element is an unknown one, and its fallback is inside it.
    let xml = r#"<Shape ID="1"><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x" xmlns:x="urn:example"/><mc:Fallback><Cell N="PinX" V="1"/></mc:Fallback></mc:AlternateContent></Shape>"#;
    let (doc, _) = doc_of(xml);
    let s = first_shape(&doc);
    assert!(s.sheet.cell("PinX").is_none(), "the fallback's cell is not the shape's cell");
    let diacraft_doc::RawNode::Element(e) = &s.passthrough.iter().next().unwrap().node else { panic!("an element") };
    assert_eq!(&*e.name, "mc:AlternateContent");
    assert_eq!(e.children.len(), 2);
}

// -------------------------------------------------------------------------------------------------------------------- pictures

fn picture_page(ty: &str, content_type: &str) -> VsdxBuilder {
    let shape = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape).rel("rId1", ty, "../media/p.bin")).with(Pkg::new().part(
        "/visio/media/p.bin",
        content_type,
        [1u8, 2, 3],
    ))
}

#[test]
fn only_the_six_formats_a_viewer_draws_are_pictures() {
    for ct in ["image/bmp", "image/x-emf", "image/gif", "image/jpeg", "image/png", "image/tiff", "IMAGE/PNG", "image/x-ms-bmp"] {
        let (doc, _) = read_ok(&picture_page(&format!("{OD}image"), ct).build());
        assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_some(), "{ct}");
    }
    for ct in ["image/x-wmf", "application/octet-stream", "image/svg+xml", "application/vnd.ms-visio.drawing.main+xml", "text/plain"] {
        let (doc, report) = read_ok(&picture_page(&format!("{OD}image"), ct).build());
        assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none(), "{ct}");
        assert_eq!(report.warning_count("picture_unresolved"), 1, "{ct}");
    }
}

#[test]
fn an_image_relationship_to_the_document_part_is_no_picture() {
    let shape = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let builder = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape).rel("rId1", &format!("{OD}image"), "../document.xml"));
    let (doc, report) = read_ok(&builder.build());
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_none());
    assert_eq!(report.stats.media_bytes, 0, "no copy of the document part was made");
}

#[test]
fn the_fallback_of_an_object_is_the_supported_image_among_its_relationships_and_not_the_first_of_them() {
    let shape = r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Object"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    let builder = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shape).rel("rId1", &format!("{OD}oleObject"), "../embeddings/o.bin")).with(
        Pkg::new()
            .part("/visio/embeddings/o.bin", "application/vnd.openxmlformats-officedocument.oleObject", [9u8])
            .part("/visio/media/a.wmf", "image/x-wmf", [1u8])
            .part("/visio/media/b.emf", "image/x-emf", [2u8, 2])
            .part("/visio/media/c.png", "image/png", [3u8, 3, 3])
            // The first relationship is a hyperlink (to a PNG: it is no image relationship), the second an image of a format no viewer draws, the
            // third the one to take.
            .rel("/visio/embeddings/o.bin", "rId1", &format!("{OD}hyperlink"), "../media/c.png")
            .rel("/visio/embeddings/o.bin", "rId2", &format!("{OD}image"), "../media/a.wmf")
            .rel("/visio/embeddings/o.bin", "rId3", &format!("{OD}image"), "../media/b.emf")
            .rel("/visio/embeddings/o.bin", "rId4", &format!("{OD}image"), "../media/c.png"),
    );
    let (doc, report) = read_ok(&builder.build());
    let picture = first_shape(&doc).foreign.as_ref().unwrap().picture.clone().unwrap();
    assert_eq!(&*picture.part, "/visio/media/b.emf");
    assert_eq!(report.stats.media_bytes, 2);
}

#[test]
fn a_foreign_data_is_read_only_on_a_shape_of_type_foreign() {
    // On any other shape it is an unknown element, kept, and no picture is loaded for it.
    let inner = r#"<ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData>"#;
    let b = |ty: &str| {
        VsdxBuilder::new()
            .page(PageSpec::new(1, "P").shapes(&format!("<Shape ID=\"1\" Type=\"{ty}\">{inner}</Shape>")).rel(
                "rId1",
                &format!("{OD}image"),
                "../media/i.png",
            ))
            .with(Pkg::new().part("/visio/media/i.png", "image/png", [1u8, 2]))
            .build()
    };
    let (doc, report) = read_ok(&b("Foreign"));
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_some());
    assert_eq!(report.stats.media_bytes, 2);
    for ty in ["Shape", "Group", "Guide"] {
        let (doc, report) = read_ok(&b(ty));
        let s = first_shape(&doc);
        assert!(s.foreign.is_none(), "{ty}");
        assert_eq!(s.passthrough.iter().count(), 1, "{ty}: the element is kept whole");
        assert_eq!(report.stats.media_bytes, 0, "{ty}: no picture is loaded");
    }
}

// ----------------------------------------------------------------------------------------------------------------------- themes

const THEME_HEAD: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:vt="http://schemas.microsoft.com/office/visio/2012/theme" name="T">"#;

fn scheme(slots: [&str; 12]) -> String {
    let names = ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];
    let inner: String = names.iter().zip(slots).map(|(n, c)| format!("<a:{n}>{c}</a:{n}>")).collect();
    format!("<a:clrScheme name=\"S\">{inner}</a:clrScheme>")
}

fn rgb(hex: &str) -> String {
    format!("<a:srgbClr val=\"{hex}\"/>")
}

const COLOURS: [&str; 12] = ["000001", "000002", "000003", "000004", "000005", "000006", "000007", "000008", "000009", "00000A", "00000B", "00000C"];

fn theme_xml(clr: &str, extra_elements: &str) -> String {
    format!("{THEME_HEAD}<a:themeElements>{clr}{extra_elements}</a:themeElements></a:theme>")
}

fn read_theme_of(xml: &str) -> Arc<diacraft_doc::Theme> {
    let (doc, _) = read_ok(&VsdxBuilder::new().theme(xml.as_bytes()).build());
    Arc::clone(doc.themes.get(0).unwrap())
}

#[test]
fn each_of_the_twelve_slots_of_a_colour_scheme_is_read_into_its_own_place() {
    let slots: Vec<String> = COLOURS.iter().map(|c| rgb(c)).collect();
    let theme = read_theme_of(&theme_xml(&scheme(std::array::from_fn(|i| slots[i].as_str())), ""));
    for (i, hex) in COLOURS.iter().enumerate() {
        assert_eq!(theme.slots[i], diacraft_color::Rgba::from_hex(hex), "slot {i}");
    }
    let colors = theme.colors.as_ref().unwrap();
    assert_eq!(colors.name, "S");
    // `dk2` is the third and `lt2` the fourth, not the other way about, and `hlink` and `folHlink` are the last two.
    assert_eq!(colors.colors[2], diacraft_color::Rgba::from_hex("000003").unwrap());
    assert_eq!(colors.colors[3], diacraft_color::Rgba::from_hex("000004").unwrap());
    assert_eq!(colors.colors[10], diacraft_color::Rgba::from_hex("00000B").unwrap());
    assert_eq!(colors.colors[11], diacraft_color::Rgba::from_hex("00000C").unwrap());
}

#[test]
fn one_slot_that_is_not_a_colour_the_reader_reads_costs_that_slot_and_not_the_other_eleven() {
    let mut slots: Vec<String> = COLOURS.iter().map(|c| rgb(c)).collect();
    slots[3] = "<a:schemeClr val=\"bg2\"/>".to_owned(); // lt2: not a colour we read
    let theme = read_theme_of(&theme_xml(&scheme(std::array::from_fn(|i| slots[i].as_str())), ""));
    assert_eq!(theme.slots.iter().filter(|s| s.is_none()).count(), 1);
    assert!(theme.slots[3].is_none());
    assert!(theme.slots[2].is_some() && theme.slots[4].is_some());
    assert!(theme.colors.is_none(), "no colour is invented: the scheme as a whole needs all twelve");
    // A slot missing altogether is the same.
    let mut slots: Vec<String> = COLOURS.iter().map(|c| rgb(c)).collect();
    slots[11] = String::new();
    let theme = read_theme_of(&theme_xml(&scheme(std::array::from_fn(|i| slots[i].as_str())), ""));
    assert!(theme.slots[11].is_none() && theme.slots[10].is_some());
}

#[test]
fn a_system_colour_is_its_last_colour_else_one_of_the_two_system_names() {
    let slots = [
        r#"<a:sysClr val="windowText" lastClr="123456"/>"#,
        r#"<a:sysClr val="window"/>"#,
        r#"<a:sysClr val="windowText"/>"#,
        r#"<a:sysClr val="window" lastClr="not-hex"/>"#,
        r#"<a:sysClr val="other"/>"#,
        r#"<a:sysClr lastClr="ABCDEF"/>"#,
        r#"<a:srgbClr val="12345"/>"#,
        r#"<a:srgbClr/>"#,
        r#"<a:srgbClr val="GGGGGG"/>"#,
        r#"<a:srgbClr val="00FF00"/>"#,
        r#"<a:sysClr/>"#,
        "",
    ];
    let theme = read_theme_of(&theme_xml(&scheme(slots), ""));
    let want = |hex: &str| diacraft_color::Rgba::from_hex(hex);
    assert_eq!(theme.slots[0], want("123456"), "lastClr wins over val");
    assert_eq!(theme.slots[1], want("FFFFFF"), "window is white");
    assert_eq!(theme.slots[2], want("000000"), "windowText is black");
    assert_eq!(theme.slots[3], want("FFFFFF"), "a lastClr that is not hex falls to val");
    assert_eq!(theme.slots[4], None, "a system colour that is neither");
    assert_eq!(theme.slots[5], want("ABCDEF"), "lastClr with no val");
    assert_eq!(&theme.slots[6..9], [None, None, None]);
    assert_eq!(theme.slots[9], want("00FF00"));
    assert_eq!(&theme.slots[10..], [None, None]);
}

fn ext(inner: &str) -> String {
    format!("<a:extLst><a:ext uri=\"{{00000000-0000-0000-0000-000000000000}}\">{inner}</a:ext></a:extLst>")
}

#[test]
fn the_scheme_id_of_each_component_is_found_by_the_name_of_the_element_and_not_by_the_uri() {
    // V2 of the themes spec: under `a:fontScheme`, an extension with another URI, `schemeEnum` 5.
    let font = format!(
        "<a:fontScheme name=\"F\"><a:majorFont><a:latin typeface=\"Major\"/></a:majorFont><a:minorFont><a:latin typeface=\"Minor\"/></a:minorFont>{}</a:fontScheme>",
        ext(r#"<vt:schemeID schemeEnum="5"/>"#)
    );
    let slots: Vec<String> = COLOURS.iter().map(|c| rgb(c)).collect();
    let clr = scheme(std::array::from_fn(|i| slots[i].as_str())).replace(
        "</a:clrScheme>",
        &format!("{}</a:clrScheme>", ext(r#"<vt:schemeID schemeEnum="33" schemeGUID="{11111111-1111-1111-1111-111111111111}"/>"#)),
    );
    let elements = format!(
        "{font}{}",
        ext(concat!(
            r#"<vt:fmtSchemeEx><vt:schemeID schemeEnum="7"/></vt:fmtSchemeEx>"#,
            r#"<vt:fmtConnectorSchemeEx><vt:schemeID schemeEnum="8"/></vt:fmtConnectorSchemeEx>"#,
            r#"<vt:themeScheme><vt:schemeID schemeEnum="65535" schemeGUID="{G}"/></vt:themeScheme>"#
        ))
    );
    let theme = read_theme_of(&theme_xml(&clr, &elements));
    let id = |n: u32, guid: Option<&str>| Some(SchemeId { scheme_enum: n, scheme_guid: guid.map(Arc::from) });
    assert_eq!(theme.font_scheme_id, id(5, None));
    assert_eq!(theme.color_scheme_id, id(33, Some("{11111111-1111-1111-1111-111111111111}")), "The GUID is kept as the file had it");
    assert_eq!(theme.effect_scheme_id, id(7, None));
    assert_eq!(theme.connector_scheme_id, id(8, None));
    assert_eq!(theme.theme_scheme_id, id(65535, Some("{G}")));
    assert_eq!(theme.major_font.as_deref(), Some("Major"));
    assert_eq!(theme.minor_font.as_deref(), Some("Minor"));
}

#[test]
fn a_scheme_id_in_another_namespace_or_with_a_schemeenum_that_is_not_a_number_is_not_one() {
    let f = |inner: &str| format!("<a:fontScheme name=\"F\">{}</a:fontScheme>", ext(inner));
    let theme = read_theme_of(&theme_xml("", &f(r#"<w:schemeID xmlns:w="urn:other" schemeEnum="5"/>"#)));
    assert_eq!(theme.font_scheme_id, None, "a vocabulary that is not the `vt:`");
    for bad in ["x", "-1", "+3", "4294967296", ""] {
        let theme = read_theme_of(&theme_xml("", &f(&format!("<vt:schemeID schemeEnum=\"{bad}\"/>"))));
        assert_eq!(theme.font_scheme_id, None, "{bad:?}");
    }
    // MS-VSDX's placeholder namespace is accepted beside the real one.
    let theme = read_theme_of(&theme_xml("", &f(r#"<v:schemeID xmlns:v="http://visThemeSchemaUri" schemeEnum="9"/>"#)));
    assert_eq!(theme.font_scheme_id, Some(SchemeId { scheme_enum: 9, scheme_guid: None }));
    // The first usable one wins when a part holds two.
    let theme = read_theme_of(&theme_xml("", &f(r#"<vt:schemeID schemeEnum="x"/><vt:schemeID schemeEnum="1"/><vt:schemeID schemeEnum="2"/>"#)));
    assert_eq!(theme.font_scheme_id.as_ref().map(|i| i.scheme_enum), Some(1));
}

#[test]
fn a_theme_is_chosen_by_its_scheme_enum_and_not_by_its_place() {
    // The second part has `schemeEnum` 33, and `ThemeIndex` 33 selects it.
    let part = |n: u32| theme_xml("", &ext(&format!("<vt:themeScheme><vt:schemeID schemeEnum=\"{n}\"/></vt:themeScheme>")));
    let bytes = VsdxBuilder::new().theme(part(7).as_bytes()).theme(part(33).as_bytes()).theme(theme_xml("", "").as_bytes()).build();
    let (doc, _) = read_ok(&bytes);
    assert_eq!(doc.themes.len(), 3);
    assert_eq!(doc.theme_by_index(33).map(|t| &*t.part), Some("/visio/theme/theme2.xml"));
    assert_eq!(doc.theme_by_index(7).map(|t| &*t.part), Some("/visio/theme/theme1.xml"));
    assert_eq!(doc.theme_by_index(8), None);
}

#[test]
fn a_themed_cell_with_no_theme_part_reads_and_keeps_its_value_and_formula() {
    // V7 of the themes spec, the half that is reading.
    let (doc, _) = doc_of(r#"<Shape ID="1"><Cell N="LineColor" V="Themed" F="THEMEVAL()"/></Shape>"#);
    assert!(doc.themes.is_empty());
    let s = first_shape(&doc);
    let c = s.sheet.cell("LineColor").unwrap();
    assert_eq!(c.value.as_ref().unwrap().text(), "Themed");
    assert_eq!(c.formula.text(), Some("THEMEVAL()"));
}

// ---------------------------------------------------------------------------------------------- the DiaCraft part, the properties

#[test]
fn the_diacraft_version_is_the_version_of_the_diacraft_namespace_and_no_other_vocabularys() {
    let data = |attrs: &str| {
        format!(
            "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" xmlns:q=\"urn:other\" {attrs}><dc:Item/></dc:Data></SolutionXML>"
        )
    };
    let (_, report) = read_ok(&VsdxBuilder::new().solution("DiaCraft", &data(r#"dc:version="7""#)).build());
    assert_eq!(report.diacraft, Some(diacraft_vsdx::DiacraftPart::Loaded { version: "7".to_owned(), children: 1 }));
    // `q:version`, in another namespace, is not DiaCraft's version; neither is `version` with no prefix.
    for attrs in [r#"q:version="7""#, r#"version="7""#, ""] {
        let (_, report) = read_ok(&VsdxBuilder::new().solution("DiaCraft", &data(attrs)).build());
        assert_eq!(report.diacraft, Some(diacraft_vsdx::DiacraftPart::Unreadable("no version")), "{attrs:?}");
    }
    // And the first of two is the one when both are there.
    let (_, report) = read_ok(&VsdxBuilder::new().solution("DiaCraft", &data(r#"q:version="1" dc:version="2""#)).build());
    assert_eq!(report.diacraft, Some(diacraft_vsdx::DiacraftPart::Loaded { version: "2".to_owned(), children: 1 }));
}

#[test]
fn the_dates_and_the_language_of_the_core_properties_are_read_as_text() {
    let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>T</dc:title><dcterms:created>2031-03-07T20:05:09Z</dcterms:created><dcterms:modified>2031-04-08T01:02:03Z</dcterms:modified><cp:lastPrinted>2031-05-09T04:05:06Z</cp:lastPrinted><dc:language>de-DE</dc:language></cp:coreProperties>"#;
    let (doc, _) = read_ok(&VsdxBuilder::new().core(core).build());
    assert_eq!(doc.props.created.as_deref(), Some("2031-03-07T20:05:09Z"));
    assert_eq!(doc.props.modified.as_deref(), Some("2031-04-08T01:02:03Z"));
    assert_eq!(doc.props.last_printed.as_deref(), Some("2031-05-09T04:05:06Z"));
    assert_eq!(doc.props.language.as_deref(), Some("de-DE"));
    // Missing is `None`, present and empty is `Some("")`.
    let bare = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dcterms="http://purl.org/dc/terms/"><dcterms:created></dcterms:created></cp:coreProperties>"#;
    let (doc, _) = read_ok(&VsdxBuilder::new().core(bare).build());
    assert_eq!((doc.props.created.as_deref(), doc.props.modified.as_deref()), (Some(""), None));
}

// -------------------------------------------------------------------------------------------------------------- the colour table

#[test]
fn the_colour_table_is_capped_and_an_entry_with_a_number_that_is_not_plain_is_counted() {
    assert_eq!(MAX_COLOR_ENTRIES, 4_096);
    let table = |n: usize| (0..n).map(|i| format!("<ColorEntry IX=\"{}\" RGB=\"#000001\"/>", 24 + i)).collect::<String>();
    let (doc, _) = read_ok(&VsdxBuilder::new().colors(&table(MAX_COLOR_ENTRIES)).build());
    assert_eq!(doc.colors.len(), MAX_COLOR_ENTRIES);
    let e = read_err(&VsdxBuilder::new().colors(&table(MAX_COLOR_ENTRIES + 1)).build(), &Limits::default());
    assert!(matches!(e, VsdxError::Limit { kind: LimitKind::Colors, max: 4_096 }), "{e}");
    assert_eq!(e.class(), "too_many_colors");
    // `IX="024"` is the number 24 and its text is not given back: the row stays, and the attribute is held under the entry.
    let (doc, report) = read_ok(&VsdxBuilder::new().colors(r##"<ColorEntry IX="024" RGB="#123456"/>"##).build());
    assert_eq!(doc.color(24), diacraft_color::Rgba::from_hex("123456"));
    assert_eq!(report.skipped_counts.len(), 0);
    assert_eq!(crate::held::dump(crate::held::rest(&doc.ext.get().expect("the document's slot").vsdx, "ColorEntry/24")[0]), " IX=\"024\"");
}

// ------------------------------------------------------------------------------------------------------ a page named a space

#[test]
fn a_page_named_one_space_keeps_its_name_with_a_warning_and_the_file_opens() {
    let b = VsdxBuilder::new().page(PageSpec::new(1, " ").shapes(r#"<Shape ID="1"/>"#));
    let (doc, report) = read_ok(&b.build());
    assert_eq!(&*doc.pages.get(0).unwrap().name, " ");
    assert_eq!(report.warning_count("page_name_not_valid"), 1);
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 1);
}

// ----------------------------------------------------------------------------------------------------------------- caps and rows

fn rows(n: usize) -> String {
    let body: String = (0..n).map(|i| format!("<Row IX=\"{i}\" T=\"RelLineTo\"/>")).collect();
    format!("<Shape ID=\"1\"><Section N=\"Geometry\" IX=\"0\">{body}</Section></Shape>")
}

#[test]
fn a_section_of_16_385_rows_and_of_320_000_reads_and_one_of_320_001_is_refused() {
    // The sample files have a real section of 31,273 rows, which the old cap of 16,384 refused.
    for n in [16_385, MAX_ROWS_PER_SECTION] {
        let (doc, report) = doc_of(&rows(n));
        assert_eq!(first_shape(&doc).sheet.sections[0].rows.len(), n);
        assert_eq!(report.stats.max_rows_in_a_section, n);
    }
    let e = read_err(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&rows(MAX_ROWS_PER_SECTION + 1))).build(), &Limits::default());
    assert_eq!(e.class(), "too_many_rows_in_a_section");
}

#[test]
fn a_sheet_of_8_192_sections_reads_and_one_of_8_193_is_refused() {
    let sections = |n: usize| (0..n).map(|i| format!("<Section N=\"Geometry\" IX=\"{i}\"/>")).collect::<String>();
    let (doc, _) = doc_of(&format!("<Shape ID=\"1\">{}</Shape>", sections(MAX_SECTIONS_PER_SHEET)));
    assert_eq!(first_shape(&doc).sheet.sections.len(), MAX_SECTIONS_PER_SHEET);
    let e = read_err(
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("<Shape ID=\"1\">{}</Shape>", sections(MAX_SECTIONS_PER_SHEET + 1)))).build(),
        &Limits::default(),
    );
    assert_eq!(e.class(), "too_many_sections_in_a_sheet");
}

#[test]
fn a_sheet_of_4_096_cells_in_one_place_reads_and_one_of_4_097_is_refused() {
    let cells = |n: usize| (0..n).map(|i| format!("<Cell N=\"C{i}\" V=\"1\"/>")).collect::<String>();
    let (doc, _) = doc_of(&format!("<Shape ID=\"1\">{}</Shape>", cells(4_096)));
    assert_eq!(first_shape(&doc).sheet.cells.len(), 4_096);
    let e = read_err(
        &VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("<Shape ID=\"1\">{}</Shape>", cells(4_097)))).build(),
        &Limits::default(),
    );
    assert_eq!(e.class(), "too_many_cells_in_one_place");
}

// ------------------------------------------------------------------------------------------------------------------- the report

#[test]
fn a_report_carries_the_sizes_that_confirm_the_container_caps_and_the_xml_limits() {
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("<Shape ID=\"1\"><Cell N=\"X\" V=\"{}\"/><a><b><c/></b></a></Shape>", "9".repeat(300))))
        .master(MasterSpec::new(1, "M"))
        .build();
    let (_, report) = read_ok(&bytes);
    let s = &report.stats;
    assert_eq!(s.file_bytes, bytes.len() as u64);
    let package = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    assert_eq!(s.zip_rows, package.rows());
    assert_eq!(s.parts, package.parts().count());
    assert_eq!(s.max_part_name_bytes, package.parts().map(str::len).max().unwrap());
    assert_eq!(s.max_part_bytes, package.parts().map(|p| package.part(p).unwrap().len() as u64).max().unwrap());
    assert_eq!(s.content_type_rows, package.content_types().defaults.len() + package.content_types().overrides.len());
    assert_eq!(s.max_xml_attr_bytes, 300, "the longest attribute value read");
    assert!(s.max_xml_depth >= 6, "PageContents, Shapes, Shape, a, b, c: {}", s.max_xml_depth);
    assert!(s.max_xml_items > 10);
    let rels_rows = package.rels("/visio/pages/pages.xml").unwrap().items.len();
    assert!(s.max_rels_rows >= rels_rows.max(1));
    assert!(s.max_rels_bytes > 0);
}

#[test]
fn a_refused_file_reports_what_the_reader_knew_when_it_refused_it() {
    // A page part past the XML item cap: refused as `xml_too_complex` with the part named (a part name that is package structure is).
    let mut limits = Limits::default();
    limits.opc.xml.max_nodes = 50;
    let shapes: String = (0..40).map(|i| format!("<Shape ID=\"{i}\"/>")).collect();
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build();
    let (result, report) = read_partial(&bytes, &limits);
    assert_eq!(result.unwrap_err().class(), "xml_too_complex");
    assert_eq!(report.stats.file_bytes, bytes.len() as u64);
    assert!(report.stats.zip_rows >= 8, "the archive was read: {}", report.stats.zip_rows);
    assert!(report.stats.parts >= 8);
    assert_eq!(report.stats.styles, 1, "the styles had been read");
    // A limit of the reader: where it was.
    // The root style holds three cells, so five leaves two for the page's three.
    let limits = Limits { max_cells: 5, ..Limits::default() };
    let bytes = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(
            r#"<Shape ID="1"><Section N="User"><Row N="a"><Cell N="X" V="1"/><Cell N="Y" V="1"/><Cell N="Z" V="1"/></Row></Section></Shape>"#,
        ))
        .build();
    let (result, report) = read_partial(&bytes, &limits);
    assert_eq!(result.unwrap_err().class(), "too_many_cells");
    let at = report.refused_at.unwrap();
    assert_eq!(at.part, "/visio/pages/page1.xml");
    assert_eq!(at.section, "User");
    assert!(report.stats.max_xml_items > 0);
    // A file that is not a package: its size and nothing else.
    let (result, report) = read_partial(b"<html/>", &Limits::default());
    assert_eq!(result.unwrap_err().class(), "not_zip");
    assert_eq!(report.stats.file_bytes, 7);
    assert_eq!(report.stats.zip_rows, 0);
    // And `read` is the same call with the report dropped for an error.
    assert!(read(b"<html/>", &Limits::default()).is_err());
    assert!(read_with(&VsdxBuilder::new().build(), &Limits::default()).1.refused_at.is_none());
}

// ---------------------------------------------------------------------------------------------------- the review's survivors

#[test]
fn a_section_name_used_again_in_one_sheet_is_warned_of_each_time_and_a_new_one_is_not() {
    let w = |xml: &str| doc_of(&format!("<Shape ID=\"1\">{xml}</Shape>")).1.warning_count("duplicate_section");
    assert_eq!(w(r#"<Section N="Same"/><Section N="Same"/><Section N="Same"/><Section N="Other"/>"#), 2);
    assert_eq!(w(r#"<Section N="A"/><Section N="B"/>"#), 0);
    // Two sections of Geometry are numbered and are not a repeat.
    assert_eq!(w(r#"<Section N="Geometry"/><Section N="Geometry"/>"#), 0);
    // The same name in two sheets is not a repeat.
    let (_, report) = doc_of(r#"<Shape ID="1"><Section N="Same"/></Shape><Shape ID="2"><Section N="Same"/></Shape>"#);
    assert_eq!(report.warning_count("duplicate_section"), 0);
}

#[test]
fn a_root_declares_the_standard_two_for_nothing_and_anything_else_is_held() {
    // The attributes of the root of a page part the model has no field for, by name: the standard declarations are not among them.
    let part = |root_attrs: &str| -> Vec<String> {
        let part = format!("<PageContents {root_attrs}><Shapes><Shape ID=\"1\"/></Shapes></PageContents>");
        let (doc, report) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build());
        assert_eq!(report.skipped_counts.len(), 0);
        let page = doc.pages.get(0).unwrap();
        let held = page.ext.get().map(|e| crate::held::rest(&e.vsdx, "PageContents")).unwrap_or_default();
        held.iter().flat_map(|k| k.raw.iter().map(|(name, _)| name.to_owned())).collect()
    };
    assert!(part(&format!("xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"")).is_empty());
    assert!(part(&format!("xmlns=\"{NS_2012}\"")).is_empty(), "a root need not declare r");
    assert_eq!(part(&format!("xmlns=\"{NS_2012}\" xmlns:r=\"urn:not-relationships\"")), ["xmlns:r"], "r bound to something else");
    assert_eq!(part(&format!("xmlns=\"{NS_2012}\" xmlns:x=\"{NS_R}\"")), ["xmlns:x"], "the relationships namespace under another prefix");
    assert_eq!(part("xmlns=\"\""), ["xmlns"], "a default namespace that is not the format's");
    // Every other attribute of a root is held, `xml:space` among them: the builder's roots carry it, and the model holds it nowhere else.
    assert_eq!(part(&format!("xmlns=\"{NS_2012}\" id=\"x\" xml:space=\"preserve\"")), ["id", "xml:space"]);
}

#[test]
fn the_report_keeps_the_most_of_each_xml_figure_and_not_the_last_tree_s() {
    // The document part is the deepest, the fullest and has the longest attribute name; the parts read after it are small.
    let deep = format!("{}<leaf/>{}", "<a>".repeat(40), "</a>".repeat(40));
    let many = "<c/>".repeat(3_000);
    let long_name = format!("<x {}=\"1\"/>", "n".repeat(400));
    let builder = VsdxBuilder::new().document_extra(&format!("{deep}{many}{long_name}")).page(PageSpec::new(1, "P").shapes(&shape(1, "")));
    let (_, report) = read_ok(&builder.build());
    assert_eq!(report.stats.max_xml_depth, 42, "the root, forty elements and a leaf");
    assert!(report.stats.max_xml_items > 3_040, "{}", report.stats.max_xml_items);
    assert_eq!(report.stats.max_xml_attr_bytes, 400, "the longest attribute name counts, not only values");
}

fn pages_with_fillers(fillers: usize) -> Vec<u8> {
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"5\"/></Shapes></PageContents>");
    let mut pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    for i in 0..fillers {
        pkg = pkg.rel("/visio/pages/pages.xml", &format!("f{i}"), "urn:filler", "x.bin");
    }
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn the_rows_of_a_relationships_part_are_charged_to_the_items_one_each() {
    let (a, b) = (read_ok(&pages_with_fillers(0)).1, read_ok(&pages_with_fillers(500)).1);
    assert_eq!(b.stats.items - a.stats.items, 500);
    assert_eq!(b.stats.max_rels_rows, 501);
    assert!(b.stats.xml_bytes > a.stats.xml_bytes, "and the bytes of the part are charged to the XML budget");
}

#[test]
fn an_external_relationship_leads_to_no_part_for_a_page_a_master_a_theme_or_the_document() {
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .rel_external("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "/visio/pages/page1.xml");
    let builder = VsdxBuilder::new()
        .without_pages_part()
        .with(pkg)
        .with(Pkg::new().part(
            "/visio/pages/page1.xml",
            CT_PAGE,
            format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"5\"/></Shapes></PageContents>"),
        ))
        .with(Pkg::new().rel_external("/visio/document.xml", "rIdT", &format!("{OD}theme"), "/visio/theme/theme1.xml"));
    let (doc, report) = read_ok(&builder.build());
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 0, "the part is there and the relationship is to a URL");
    assert_eq!(report.warning_count("page_part_unresolved"), 1);
    assert_eq!(doc.themes.len(), 0);
    assert_eq!(report.warning_count("part_missing"), 0, "an external theme is not a theme part that is missing");
}

#[test]
fn a_package_with_bytes_after_its_end_record_reads_and_counts_them_under_a_class_of_its_own() {
    let mut bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).build();
    bytes.extend_from_slice(&[0u8; 26]);
    let (doc, report) = read_ok(&bytes);
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(report.warning_count("opc_bytes_after_end_record"), 1);
    assert_eq!(report.warning_count("opc_directory_item"), 0);
}

#[test]
fn a_dot_folder_row_reads_and_is_counted_with_the_directory_items() {
    // Two real files from one source have a folder called `./`.
    let mut pkg = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).pkg();
    pkg = pkg.item(diacraft_testkit::package::ZipItem::stored("./", b""));
    let (doc, report) = read_ok(&pkg.build());
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(report.warning_count("opc_directory_item"), 1);
}

#[test]
fn an_id_that_is_a_number_with_its_text_kept_is_a_repeat_and_one_that_is_no_number_is_not_tracked() {
    let (_, report) = doc_of(r#"<Shape ID="7"/><Shape ID="007"/><Shape ID="+7"/>"#);
    assert_eq!(report.warning_count("duplicate_shape_id"), 2, "007 and +7 are the number 7 again");
    assert_eq!(report.warning_count("shape_id_not_a_number"), 2);
    let (_, report) = doc_of(r#"<Shape ID="x"/><Shape ID="x"/><Shape ID=""/>"#);
    assert_eq!(report.warning_count("duplicate_shape_id"), 0, "no number, no repeat");
    assert_eq!(report.warning_count("shape_id_not_a_number"), 3);
}

#[test]
fn a_refused_file_has_counted_the_masters_and_pages_it_had_read() {
    let b = VsdxBuilder::new()
        .master(MasterSpec::new(1, "M"))
        .master(MasterSpec::new(2, "N"))
        .page(PageSpec::new(1, "A"))
        .page(PageSpec::new(2, "B"))
        .page(PageSpec::new(3, "C").part("<PageContents><Shapes></PageContents>"));
    let (result, report) = read_partial(&b.build(), &Limits::default());
    assert_eq!(result.unwrap_err().class(), "xml_malformed");
    assert_eq!((report.stats.styles, report.stats.masters, report.stats.pages), (1, 2, 2));
}
