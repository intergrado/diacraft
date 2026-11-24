//! The document part and what hangs from it: tables, styles, the document sheet, themes, the DiaCraft solution and the unknown children.

use diacraft_color::Rgba;
use diacraft_doc::{FormulaSlot, RawNode};
use diacraft_testkit::package::{VsdxBuilder, shape};
use diacraft_vsdx::DiacraftPart;

use crate::read_ok;

fn doc(b: VsdxBuilder) -> (diacraft_doc::Document, diacraft_vsdx::ReadReport) {
    read_ok(&b.build())
}

#[test]
fn a_color_index_to_the_fixed_palette_and_to_the_table() {
    let (d, _) = doc(VsdxBuilder::new().colors(r##"<ColorEntry IX="24" RGB="#123456"/><ColorEntry IX="30" RGB="#00FF00"/>"##));
    assert_eq!(d.color(2), Some(Rgba { r: 255, g: 0, b: 0, a: 255 }));
    assert_eq!(d.color(23), Some(Rgba { r: 0x1A, g: 0x1A, b: 0x1A, a: 255 }));
    assert_eq!(d.color(24), Some(Rgba { r: 0x12, g: 0x34, b: 0x56, a: 255 }));
    assert_eq!(d.color(25), None, "no color is invented");
}

#[test]
fn a_document_with_no_colors_element_has_no_table() {
    let (d, _) = doc(VsdxBuilder::new());
    assert!(d.colors.is_empty());
    assert_eq!(d.color(24), None);
}

#[test]
fn a_color_entry_that_cannot_be_modelled_is_skipped_and_the_rest_are_read_and_a_foreign_attribute_keeps_the_row() {
    let (d, report) = doc(VsdxBuilder::new().colors(r##"<ColorEntry IX="x" RGB="#000000"/><ColorEntry RGB="#111111"/><ColorEntry IX="26"/><ColorEntry IX="27" RGB="#222222" Foo="1"/><ColorEntry IX="28" RGB="#333333"/><Other/>"##));
    // An entry with no number or no colour, and an element that is no entry, are not in the table; an entry that carries an attribute the
    // model has no slot for stays in it (and the first review: it took the colour out of the table), and the attribute is counted.
    assert_eq!(d.colors.iter().map(|e| e.ix).collect::<Vec<_>>(), [27, 28]);
    assert_eq!(d.color(27), Some(Rgba { r: 0x22, g: 0x22, b: 0x22, a: 255 }));
    // Nothing is skipped: the four children that are no entries are held in place (each after the entries before it), and the attribute is held under its entry.
    assert_eq!(report.skipped_counts.len(), 0);
    let ext = &d.ext.get().expect("the document's slot").vsdx;
    let colors = crate::held::dump(crate::held::rest(ext, "Colors")[0]);
    for held in [
        "[after 0] <n><ColorEntry IX=\"x\" RGB=\"#000000\"/></n>",
        "[after 0] <n><ColorEntry RGB=\"#111111\"/></n>",
        "[after 0] <n><ColorEntry IX=\"26\"/></n>",
        "[after 2] <n><Other/></n>",
    ] {
        assert!(colors.contains(held), "{held} in {colors}");
    }
    assert_eq!(crate::held::dump(crate::held::rest(ext, "ColorEntry/27")[0]), " Foo=\"1\"");
}

#[test]
fn the_panose_of_a_face_name_is_two_and_its_attributes_keep_their_order() {
    let (d, report) = doc(VsdxBuilder::new().face_names(r#"<FaceName NameU="Noto Sans" Panos="1" Panose="2"/><FaceName UnicodeRanges="1"/>"#));
    assert_eq!(d.fonts.len(), 1);
    assert_eq!(&*d.fonts[0].name_u, "Noto Sans");
    // V7 of the document spec: given `Panos="1" Panose="2"`, the PANOSE value is 2.
    assert_eq!(d.fonts[0].raw.get("Panose"), Some("2"));
    assert_eq!(d.fonts[0].raw.get("Panos"), Some("1"));
    let raw: Vec<_> = d.fonts[0].raw.iter().collect();
    assert_eq!(raw, [("Panos", "1"), ("Panose", "2")]);
    // The face with no `NameU` is held in place, after the face before it.
    assert_eq!(report.skipped_counts.len(), 0);
    let faces = crate::held::dump(crate::held::rest(&d.ext.get().expect("the document's slot").vsdx, "FaceNames")[0]);
    assert!(faces.contains("[after 1] <n><FaceName UnicodeRanges=\"1\"/></n>"), "{faces}");
}

#[test]
fn the_root_style_and_the_links_between_styles() {
    let styles = r#"<StyleSheet ID="1" NameU="Normal" Name="Normal" LineStyle="0" FillStyle="0" TextStyle="0" IsCustomName="1"><Cell N="LineWeight" V="0.01"/></StyleSheet><StyleSheet ID="2" NameU="Child" LineStyle="1" FillStyle="" TextStyle="9"/>"#;
    let (d, report) = doc(VsdxBuilder::new().more_styles(styles));
    assert_eq!(d.styles.len(), 3);
    let root = d.style(0).unwrap();
    assert_eq!((root.name_u.as_deref(), root.sheet.links.line), (Some("No Style"), None));
    let normal = d.style(1).unwrap();
    assert_eq!((normal.sheet.links.line, normal.sheet.links.fill, normal.sheet.links.text), (Some(0), Some(0), Some(0)));
    assert_eq!(normal.raw.get("IsCustomName"), Some("1"));
    assert!(normal.sheet.cell("LineWeight").is_some());
    let child = d.style(2).unwrap();
    assert_eq!(child.sheet.links.fill, None);
    assert_eq!(child.sheet.raw.get("FillStyle"), Some(""), "an empty link is no link and stays an attribute");
    assert_eq!(report.warning_count("style_link_not_found"), 1, "the text link names style 9");
    assert_eq!(report.stats.styles, 3);
}

#[test]
fn a_style_with_no_id_is_skipped_and_a_style_id_used_twice_keeps_both() {
    let styles = r#"<StyleSheet NameU="Nameless"/><StyleSheet ID="3" NameU="A"/><StyleSheet ID="3" NameU="B"/>"#;
    let (d, report) = doc(VsdxBuilder::new().more_styles(styles));
    assert_eq!(d.styles.len(), 3);
    assert_eq!(d.style(3).unwrap().name_u.as_deref(), Some("A"));
    // The style with no `ID` is held in place, after the root style, which came before it.
    assert_eq!(report.skipped_counts.len(), 0);
    let styles = crate::held::dump(crate::held::rest(&d.ext.get().expect("the document's slot").vsdx, "StyleSheets")[0]);
    assert!(styles.contains("[after 1] <n><StyleSheet NameU=\"Nameless\"/></n>"), "{styles}");
    assert_eq!(report.warning_count("duplicate_style_id"), 1);
}

#[test]
fn a_document_with_no_root_style_is_warned_of() {
    let (_, report) = doc(VsdxBuilder::new().styles(r#"<StyleSheet ID="1" NameU="X"/>"#));
    assert_eq!(report.warning_count("root_style_missing"), 1);
}

#[test]
fn a_cycle_of_style_links_is_read_found_once_and_warned_of() {
    // Style 1 -> 2 -> 1 for line, and 3 -> 3 for fill, and a long chain that ends: only the circles are warned of.
    let styles = r#"<StyleSheet ID="1" LineStyle="2"/><StyleSheet ID="2" LineStyle="1"/><StyleSheet ID="3" FillStyle="3"/><StyleSheet ID="4" TextStyle="5"/><StyleSheet ID="5" TextStyle="0"/>"#;
    let (d, report) = doc(VsdxBuilder::new().more_styles(styles));
    assert_eq!(report.warning_count("style_link_cycle"), 2);
    assert_eq!(d.style(1).unwrap().sheet.links.line, Some(2), "the links are kept as the file has them");
}

#[test]
fn the_roots_link_to_itself_is_where_a_chain_ends_and_no_circle_but_another_styles_is() {
    // A root style that names itself in all three families (an app may write it so) ends every chain; a style that names itself is a circle.
    let root = r#"<StyleSheet ID="0" NameU="No Style" LineStyle="0" FillStyle="0" TextStyle="0"/>"#;
    let (_, report) = doc(VsdxBuilder::new().styles(root));
    assert_eq!(report.warning_count("style_link_cycle"), 0);
    assert_eq!(report.warning_count("style_link_not_found"), 0);
    let (_, report) = doc(VsdxBuilder::new().styles(&format!("{root}<StyleSheet ID=\"1\" LineStyle=\"1\"/>")));
    assert_eq!(report.warning_count("style_link_cycle"), 1);
}

#[test]
fn the_document_sheet_is_read_with_its_attributes() {
    let sheet = r#"<Cell N="DocLangID" V="1033"/>"#;
    let doc_with = VsdxBuilder::new().document_sheet(sheet).build();
    let (d, _) = read_ok(&doc_with);
    assert!(d.sheet.cell("DocLangID").is_some());
}

#[test]
fn a_foreign_element_in_the_document_stays_where_it_was_and_the_known_ones_are_not_in_the_passthrough() {
    let extra = r#"<DocumentSettings TopPage="0"/><PublishSettings/><x:Mine xmlns:x="urn:example"/><EventList/>"#;
    let (d, report) = doc(VsdxBuilder::new().document_extra(extra));
    let kept: Vec<_> = d
        .passthrough
        .iter()
        .map(|p| {
            (
                p.after,
                match &p.node {
                    RawNode::Element(e) => e.name.to_string(),
                    _ => String::new(),
                },
            )
        })
        .collect();
    // FaceNames, StyleSheets and DocumentSheet are typed (three of them) and the four elements after them are not.
    assert_eq!(kept, [(3, "DocumentSettings".to_owned()), (3, "PublishSettings".to_owned()), (3, "x:Mine".to_owned()), (3, "EventList".to_owned())]);
    assert!(report.warnings.is_empty());
}

#[test]
fn the_color_and_font_tables_may_come_in_any_order_and_a_second_one_is_kept_as_an_element() {
    let xml = r##"<VisioDocument xmlns="http://schemas.microsoft.com/office/visio/2012/main"><StyleSheets><StyleSheet ID="0"/></StyleSheets><Colors><ColorEntry IX="24" RGB="#010203"/></Colors><Colors/><FaceNames/><DocumentSheet/></VisioDocument>"##;
    let pkg = diacraft_testkit::package::Pkg::new().part("/visio/document.xml", diacraft_testkit::package::CT_DRAWING, xml).rel(
        "/",
        "rId1",
        &format!("{}document", diacraft_testkit::package::VIS),
        "visio/document.xml",
    );
    let (d, _) = read_ok(&pkg.build());
    assert_eq!(d.colors.len(), 1);
    assert_eq!(d.passthrough.len(), 1);
}

// ------------------------------------------------------------------------------------------------------------------- themes

const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="111111"/></a:dk2><a:lt2><a:srgbClr val="222222"/></a:lt2><a:accent1><a:srgbClr val="AA0000"/></a:accent1><a:accent2><a:srgbClr val="00AA00"/></a:accent2><a:accent3><a:srgbClr val="0000AA"/></a:accent3><a:accent4><a:srgbClr val="AAAA00"/></a:accent4><a:accent5><a:srgbClr val="00AAAA"/></a:accent5><a:accent6><a:srgbClr val="AA00AA"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink><a:extLst><a:ext uri="{2703A3B3-D2E1-43D9-8057-6E9D74E0F44A}"><vt:schemeID xmlns:vt="http://schemas.microsoft.com/office/visio/2012/theme" schemeEnum="33" schemeGUID="{11111111-1111-1111-1111-111111111111}"/></a:ext></a:extLst></a:clrScheme><a:fontScheme name="F"><a:majorFont><a:latin typeface="Major Sans"/></a:majorFont><a:minorFont><a:latin typeface="Minor Sans"/></a:minorFont></a:fontScheme></a:themeElements><a:objectDefaults/></a:theme>"#;

#[test]
fn a_theme_part_is_kept_byte_for_byte_and_its_colours_and_fonts_are_read() {
    let (d, report) = doc(VsdxBuilder::new().theme(THEME.as_bytes()));
    assert_eq!(d.themes.len(), 1);
    let t = d.themes.get(0).unwrap();
    assert_eq!(&*t.bytes, THEME.as_bytes());
    assert_eq!(&*t.part, "/visio/theme/theme1.xml");
    let scheme = t.colors.as_ref().unwrap();
    assert_eq!(scheme.name, "Office");
    assert_eq!(scheme.colors[0], Rgba { r: 0, g: 0, b: 0, a: 255 });
    assert_eq!(scheme.colors[1], Rgba { r: 255, g: 255, b: 255, a: 255 });
    assert_eq!(scheme.colors[4], Rgba { r: 0xAA, g: 0, b: 0, a: 255 });
    assert_eq!(scheme.colors[11], Rgba { r: 0x95, g: 0x4F, b: 0x72, a: 255 });
    assert_eq!((t.major_font.as_deref(), t.minor_font.as_deref()), (Some("Major Sans"), Some("Minor Sans")));
    assert_eq!(report.stats.themes, 1);
}

#[test]
fn a_theme_with_no_colour_scheme_has_none_and_its_bytes_are_kept() {
    let bytes = br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements/></a:theme>"#;
    let (d, report) = doc(VsdxBuilder::new().theme(bytes));
    let t = d.themes.get(0).unwrap();
    assert!(t.colors.is_none() && t.major_font.is_none());
    assert_eq!(&*t.bytes, bytes.as_slice());
    assert!(report.warnings.is_empty());
}

#[test]
fn a_colour_scheme_with_a_colour_missing_has_no_colours_since_none_is_invented() {
    let bytes = THEME.replace(r#"<a:hlink><a:srgbClr val="0563C1"/></a:hlink>"#, "");
    let (d, _) = doc(VsdxBuilder::new().theme(bytes.as_bytes()));
    assert!(d.themes.get(0).unwrap().colors.is_none());
    assert_eq!(d.themes.get(0).unwrap().minor_font.as_deref(), Some("Minor Sans"));
}

#[test]
fn a_theme_part_that_is_not_xml_is_kept_and_warned_of() {
    let (d, report) = doc(VsdxBuilder::new().theme(b"\x00not xml"));
    assert_eq!(d.themes.len(), 1);
    assert_eq!(&*d.themes.get(0).unwrap().bytes, b"\x00not xml".as_slice());
    assert_eq!(report.warning_count("theme_unreadable"), 1);
}

#[test]
fn several_theme_parts_are_all_read_in_order_and_one_part_named_twice_is_one_theme() {
    let (d, _) = doc(VsdxBuilder::new().theme(THEME.as_bytes()).theme(b"<a:theme xmlns:a=\"urn:x\"/>"));
    assert_eq!(d.themes.len(), 2);
    let twice = diacraft_testkit::package::Pkg::new().rel(
        "/visio/document.xml",
        "rId88",
        &format!("{}theme", diacraft_testkit::package::OD),
        "theme/theme1.xml",
    );
    let (d, _) = doc(VsdxBuilder::new().theme(THEME.as_bytes()).with(twice));
    assert_eq!(d.themes.len(), 1);
}

#[test]
fn themes_past_the_count_or_the_byte_budget_are_left_out_and_warned_of() {
    let limits = diacraft_vsdx::Limits { max_themes: 1, ..diacraft_vsdx::Limits::default() };
    let bytes = VsdxBuilder::new().theme(THEME.as_bytes()).theme(THEME.as_bytes()).build();
    let (d, report) = crate::read_with(&bytes, &limits);
    assert_eq!(d.themes.len(), 1);
    assert_eq!(report.warning_count("themes_over_budget"), 1);
    let limits = diacraft_vsdx::Limits { max_theme_bytes: 100, ..diacraft_vsdx::Limits::default() };
    let (d, report) = crate::read_with(&bytes, &limits);
    assert_eq!(d.themes.len(), 0);
    assert_eq!(report.warning_count("themes_over_budget"), 2);
}

// ------------------------------------------------------------------------------------------------------ the DiaCraft solution

const DIACRAFT: &str =
    r#"<SolutionXML Name="DiaCraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"><dc:Item/><dc:Item/></dc:Data></SolutionXML>"#;

#[test]
fn the_diacraft_solution_is_read_when_present() {
    let (_, report) = doc(VsdxBuilder::new().solution("DiaCraft", DIACRAFT));
    assert_eq!(report.diacraft, Some(DiacraftPart::Loaded { version: "1".into(), children: 2 }));
}

#[test]
fn a_file_with_no_solutions_has_no_diacraft_data() {
    let (_, report) = doc(VsdxBuilder::new());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
}

#[test]
fn another_writers_solution_is_not_diacraft_s_and_the_name_is_case_sensitive() {
    for name in ["Acme", "diacraft", "DIACRAFT", "DiaCraft "] {
        let (_, report) = doc(VsdxBuilder::new().solution(name, DIACRAFT));
        assert_eq!(report.diacraft, Some(DiacraftPart::Absent), "{name}");
    }
}

#[test]
fn a_solution_named_diacraft_that_is_not_ours_is_kept_unread_and_the_file_reads() {
    for (xml, why) in [
        ("<SolutionXML Name=\"Other\"/>", "solution part is not SolutionXML named DiaCraft"),
        ("<Nope Name=\"DiaCraft\"/>", "solution part is not SolutionXML named DiaCraft"),
        ("<SolutionXML Name=\"DiaCraft\"><Data xmlns=\"urn:wrong\"/></SolutionXML>", "no Data in the DiaCraft namespace"),
        ("<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\"/></SolutionXML>", "no version"),
        ("<SolutionXML Name=\"DiaCraft\">", "solution part not XML"),
    ] {
        let (d, report) =
            doc(VsdxBuilder::new().solution("DiaCraft", xml).page(diacraft_testkit::package::PageSpec::new(1, "P").shapes(&shape(1, ""))));
        assert_eq!(report.diacraft, Some(DiacraftPart::Unreadable(why)), "{xml}");
        assert_eq!(d.pages.len(), 1);
    }
}

#[test]
fn the_diacraft_part_is_unreadable_not_truncated_when_it_is_over_the_budget() {
    let big = format!(
        "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\">{}</dc:Data></SolutionXML>",
        "<dc:Item/>".repeat(100)
    );
    // The index is a few hundred bytes and the part a thousand and more: a budget between them reads the one and not the other. The part has a cap of its own
    // (the solution part carries the data of the other format), and the index keeps the small part's.
    let limits = diacraft_vsdx::Limits { max_solution_part_bytes: 700, ..diacraft_vsdx::Limits::default() };
    let (_, report) = crate::read_with(&VsdxBuilder::new().solution("DiaCraft", &big).build(), &limits);
    assert_eq!(report.diacraft, Some(DiacraftPart::Unreadable("solution part too large")));
}

#[test]
fn a_formula_in_a_style_is_parsed_and_counted_like_any_other() {
    let styles = r#"<StyleSheet ID="1"><Cell N="LineWeight" V="0.01" F="THEMEVAL()"/></StyleSheet>"#;
    let (d, report) = doc(VsdxBuilder::new().more_styles(styles));
    let cell = d.style(1).unwrap().sheet.cell("LineWeight").unwrap().clone();
    assert!(matches!(&cell.formula, FormulaSlot::Local(f) if f.is_parsed()));
    assert_eq!((report.stats.cells, report.stats.formula_cells), (4, 1), "three flags of the root style and one more");
}
