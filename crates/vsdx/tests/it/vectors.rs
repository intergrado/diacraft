//! Vectors of the format specs that are the reader's to run and had no test of their own (the first review: 84 `-V` lines, 38 named). Each test
//! names its vector, `V<n>` of the spec it is in. The vectors that are another crate's (the writer's, the calculator's, the renderer's) are not
//! here: `docs/file-format-parity.md` lists them by owner.

use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
use diacraft_vsdx::{DiacraftPart, Limits, Variant};

use crate::{read_ok, read_with};

#[test]
fn document_a_connection_string_to_a_share_is_read_and_kept_and_nothing_is_opened() {
    // The reader opens no file and no connection: the element is kept whole in the document's own list, and the open makes no access.
    let extra = r#"<DataConnections><DataConnection ConnectionString="Data Source=\\host\share\x" Command="select 1"/></DataConnections>"#;
    let (doc, report) = read_ok(&VsdxBuilder::new().document_extra(extra).build());
    let diacraft_doc::RawNode::Element(e) = &doc.passthrough.iter().next().unwrap().node else { panic!("an element") };
    assert_eq!(&*e.name, "DataConnections");
    let diacraft_doc::RawNode::Element(c) = &e.children[0] else { panic!("an element") };
    assert_eq!(c.attrs.get("ConnectionString"), Some(r"Data Source=\\host\share\x"));
    assert_eq!(report.warnings, Vec::<String>::new());
}

#[test]
fn document_a_cell_def_and_a_section_def_are_read_with_no_error_and_both_kept() {
    let extra = r#"<CellDefs><CellDef N="Mine" T="STRING"/></CellDefs><SectionDefs><SectionDef N="MineSection"><CellDef N="Inner"/></SectionDef></SectionDefs>"#;
    let shape_xml = shape(1, r#"<Section N="MineSection"><Row N="r"><Cell N="Inner" V="1"/></Row></Section><Cell N="Mine" V="x"/>"#);
    let (doc, _) = read_ok(&VsdxBuilder::new().document_extra(extra).page(PageSpec::new(1, "P").shapes(&shape_xml)).build());
    let names: Vec<String> = doc
        .passthrough
        .iter()
        .map(|p| match &p.node {
            diacraft_doc::RawNode::Element(e) => e.name.to_string(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names, ["CellDefs", "SectionDefs"]);
    let s = crate::first_shape(&doc);
    assert_eq!(s.sheet.sections[0].name.as_str(), "MineSection", "a section nobody listed is kept as data");
    assert!(s.sheet.cell("Mine").is_some());
}

#[test]
fn masters_a_stencil_with_no_page_part_and_one_page_without_a_rel_is_a_stencil_with_no_drawing_pages() {
    let b = VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml").page(PageSpec::new(0, "P").without_part());
    let (doc, report) = read_ok(&b.build());
    assert_eq!(report.variant, Some(Variant { kind: diacraft_vsdx::Kind::Stencil, macro_enabled: false }));
    assert_eq!(doc.pages.len(), 1);
    assert!(doc.pages.get(0).unwrap().shapes.is_empty() && doc.pages.get(0).unwrap().rel_id.is_none());
    assert_eq!(report.stats.shapes_per_page, [0]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn preservation_another_writers_solution_is_not_diacrafts_and_the_name_is_case_sensitive() {
    // `Name="Acme"` and `Name="diacraft"` are no DiaCraft data: the report says there is none, and the solution's part is not read as one.
    let acme = r#"<SolutionXML Name="Acme"><Data/></SolutionXML>"#;
    let (_, report) = read_ok(&VsdxBuilder::new().solution("Acme", acme).build());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent));
    let lower = r#"<SolutionXML Name="diacraft"><dc:Data xmlns:dc="urn:app.diacraft:extension" dc:version="1"/></SolutionXML>"#;
    let (_, report) = read_ok(&VsdxBuilder::new().solution("diacraft", lower).build());
    assert_eq!(report.diacraft, Some(DiacraftPart::Absent), "the API treats the name as case-sensitive");
}

#[test]
fn themes_a_theme_with_an_empty_theme_elements_is_read_with_no_extensions_and_no_error() {
    let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements/></a:theme>"#;
    let (doc, report) = read_ok(&VsdxBuilder::new().theme(xml.as_bytes()).build());
    let t = doc.themes.get(0).unwrap();
    assert_eq!(&*t.bytes, xml.as_bytes(), "the bytes are the part's");
    assert!(t.colors.is_none() && t.theme_scheme_id.is_none() && t.color_scheme_id.is_none() && t.slots.iter().all(Option::is_none));
    assert_eq!(report.warning_count("theme_unreadable"), 0);
}

#[test]
fn themes_a_line_ex_keeps_all_its_attributes_in_the_bytes_the_model_holds() {
    // The reader holds the part's bytes whole; "arrowhead 4 at the line's end" is the renderer's. What is the reader's is that nothing is lost.
    let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:vt="http://schemas.microsoft.com/office/visio/2012/theme" name="T"><a:themeElements><a:extLst><a:ext uri="{6CAB99AB-0A78-4BAB-B597-526D62367CB4}"><vt:lineStyles><vt:fmtSchemeLineStyles><vt:lineStyle><vt:lineEx rndg="0" start="0" startSize="2" end="4" endSize="2" pattern="1"/></vt:lineStyle></vt:fmtSchemeLineStyles></vt:lineStyles></a:ext></a:extLst></a:themeElements></a:theme>"#;
    let (doc, _) = read_ok(&VsdxBuilder::new().theme(xml.as_bytes()).build());
    let text = String::from_utf8(doc.themes.get(0).unwrap().bytes.to_vec()).unwrap();
    for attr in [r#"rndg="0""#, r#"start="0""#, r#"startSize="2""#, r#"end="4""#, r#"endSize="2""#, r#"pattern="1""#] {
        assert!(text.contains(attr), "{attr}");
    }
}

#[test]
fn themes_a_theme_with_no_variant_lists_reads_and_has_nothing_to_select_among() {
    let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="S"/></a:themeElements></a:theme>"#;
    let (doc, report) = read_ok(&VsdxBuilder::new().theme(xml.as_bytes()).build());
    assert_eq!(doc.themes.len(), 1);
    assert_eq!(report.warning_count("theme_unreadable"), 0);
}

#[test]
fn package_the_extension_that_disagrees_with_the_content_type_is_told_to_the_caller_that_has_the_name() {
    // The bytes carry no name, so the reader gives the variant and the caller (`file.open`) compares it with the extension.
    let b = VsdxBuilder::new().document_type("application/vnd.ms-visio.stencil.main+xml");
    let (_, report) = read_with(&b.build(), &Limits::default());
    let why = diacraft_vsdx::check_extension(report.variant, "vsdm").unwrap();
    assert!(why.contains("extension") && why.contains("stencil"), "{why}");
    assert_eq!(diacraft_vsdx::check_extension(report.variant, "vssx"), None);
}

#[test]
fn masters_a_master_row_with_no_rel_has_no_shapes_and_no_part_is_made() {
    let (doc, _) = read_ok(&VsdxBuilder::new().master(MasterSpec::new(3, "Empty").without_part()).build());
    assert_eq!(doc.masters.len(), 1);
    assert!(doc.masters.get(0).unwrap().shapes.is_empty() && doc.masters.get(0).unwrap().rel_id.is_none());
}
