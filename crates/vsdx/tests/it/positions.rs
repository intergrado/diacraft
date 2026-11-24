//! Where an unknown node sits (unknown elements, comments and instructions "at their position among siblings").
//!
//! Every element the reader reads into typed children keeps what it does not know in a `Passthrough`, each node with `after`: how many of the
//! element's typed children came before it. This asserts the number at every place the reader computes one, with an unknown node before the
//! first typed child, between two, and after the last, so that a `typed` that is off by one, or not advanced where it should be, shows as a
//! different list. (The review's mutants of `after` survived five in six: the signature of the preservation test hid position, and the
//! columns, rows and indexes were not looked at.)

use diacraft_doc::{Passthrough, RawNode};
use diacraft_testkit::package::{MasterSpec, NS_2012, NS_R, OD, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::Limits;

use crate::{first_shape, read_with};

/// The unknown nodes of a passthrough as `(after, what)`: the name of an element, or `comment`, `pi` or `text`.
fn placed(pass: &Passthrough) -> Vec<(u32, String)> {
    pass.iter()
        .map(|p| {
            let what = match &p.node {
                RawNode::Element(e) => e.name.to_string(),
                RawNode::Comment(_) => "comment".to_owned(),
                RawNode::Pi { .. } => "pi".to_owned(),
                RawNode::Text(_) => "text".to_owned(),
            };
            (p.after, what)
        })
        .collect()
}

fn expect(pass: &Passthrough, wanted: &[(u32, &str)]) {
    let got = placed(pass);
    let wanted: Vec<(u32, String)> = wanted.iter().map(|(a, w)| (*a, (*w).to_owned())).collect();
    assert_eq!(got, wanted);
}

fn shape_doc(xml: &str) -> diacraft_doc::Document {
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(xml)).build_doc()
}

trait BuildDoc {
    fn build_doc(&self) -> diacraft_doc::Document;
}

impl BuildDoc for VsdxBuilder {
    fn build_doc(&self) -> diacraft_doc::Document {
        read_with(&self.build(), &Limits::default()).0
    }
}

/// The package of `builder` with the bytes of `part` replaced.
fn with_part(builder: &VsdxBuilder, part: &str, xml: &str) -> Vec<u8> {
    builder
        .pkg()
        .map_parts(|name, bytes| {
            if name.eq_ignore_ascii_case(part) {
                *bytes = xml.as_bytes().to_vec();
            }
        })
        .build()
}

fn root(name: &str, inner: &str) -> String {
    format!("<{name} xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{inner}</{name}>")
}

#[test]
fn a_shape_counts_cells_sections_text_data_and_shapes_as_its_typed_children() {
    let doc = shape_doc(
        r#"<Shape ID="1"><U0/><Cell N="A" V="1"/><U1/><Section N="User"/><U2/><Text>t</Text><U3/><Data1>d</Data1><U4/><Shapes/><U5/></Shape>"#,
    );
    expect(&first_shape(&doc).passthrough, &[(0, "U0"), (1, "U1"), (2, "U2"), (3, "U3"), (4, "U4"), (5, "U5")]);
}

#[test]
fn a_shape_counts_a_foreign_data_and_a_second_text_is_not_a_typed_child() {
    let doc = shape_doc(
        r#"<Shape ID="1" Type="Foreign"><U0/><ForeignData ForeignType="Bitmap"/><U1/><Text>a</Text><Text>b</Text><Data1>x</Data1><Data1>y</Data1><ForeignData/></Shape>"#,
    );
    // The second `Text`, the second `Data1` and the second `ForeignData` are unknown elements, after the typed ones before them: the foreign data
    // (1), then the first text (2) and the first `Data1` (3).
    expect(&first_shape(&doc).passthrough, &[(0, "U0"), (1, "U1"), (2, "Text"), (3, "Data1"), (3, "ForeignData")]);
}

#[test]
fn comments_instructions_and_text_between_typed_children_keep_their_place_too() {
    let doc = shape_doc(r#"<Shape ID="1"><!--c0--><Cell N="A" V="1"/><?p d?>words<Cell N="B" V="2"/><!--c2--></Shape>"#);
    expect(&first_shape(&doc).passthrough, &[(0, "comment"), (1, "pi"), (1, "text"), (2, "comment")]);
}

#[test]
fn a_trigger_and_an_unnamed_cell_are_kept_in_the_sheets_list_after_the_typed_children_before_them() {
    let doc = shape_doc(
        r#"<Shape ID="1"><Trigger N="T0"/><Cell N="A" V="1"/><Trigger N="T1"/><Cell V="2"/><Section N="User"/><Cell N="A" V="3"/></Shape>"#,
    );
    let s = first_shape(&doc);
    // The second `A` is a cell of a name already taken, and the unnamed one has no name: both are unknown elements.
    expect(&s.sheet.passthrough, &[(0, "Trigger"), (1, "Trigger"), (1, "Cell"), (2, "Cell")]);
}

#[test]
fn a_section_counts_its_cells_and_rows() {
    let doc = shape_doc(r#"<Shape ID="1"><Section N="User"><U0/><Cell N="A" V="1"/><U1/><Row N="r"/><U2/><Row N="s"/><U3/></Section></Shape>"#);
    let s = first_shape(&doc);
    expect(&s.sheet.sections[0].passthrough, &[(0, "U0"), (1, "U1"), (2, "U2"), (3, "U3")]);
}

#[test]
fn a_row_counts_its_cells() {
    let doc =
        shape_doc(r#"<Shape ID="1"><Section N="User"><Row N="r"><U0/><Cell N="A" V="1"/><U1/><Cell N="B" V="2"/><!--c--></Row></Section></Shape>"#);
    let s = first_shape(&doc);
    expect(&s.sheet.sections[0].rows[0].passthrough, &[(0, "U0"), (1, "U1"), (2, "comment")]);
}

#[test]
fn a_mixed_cell_keeps_all_its_content_in_order_with_no_typed_children() {
    let doc = shape_doc(r#"<Shape ID="1"><Cell N="A">a<RefBy ID="1"/>b<?NewValue V?></Cell></Shape>"#);
    let s = first_shape(&doc);
    let cell = s.sheet.cell("A").unwrap();
    let extra = cell.extra.as_ref().unwrap();
    expect(&extra.passthrough, &[(0, "text"), (0, "RefBy"), (0, "text"), (0, "pi")]);
}

#[test]
fn a_foreign_data_counts_its_rel() {
    let doc =
        shape_doc(r#"<Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><U0/><Rel r:id="rId1"/><U1/><!--c--></ForeignData></Shape>"#);
    let s = first_shape(&doc);
    expect(&s.foreign.as_ref().unwrap().passthrough, &[(0, "U0"), (1, "U1"), (1, "comment")]);
}

#[test]
fn a_style_counts_its_cells_and_sections() {
    let builder = VsdxBuilder::new().more_styles(
        r#"<StyleSheet ID="1"><U0/><Cell N="A" V="1"/><U1/><Section N="User"><Row N="a"/></Section><!--c--><Cell N="B" V="2"/><U3/></StyleSheet>"#,
    );
    let doc = read_with(&builder.build(), &Limits::default()).0;
    let style = doc.style(1).unwrap();
    expect(&style.passthrough, &[(0, "U0"), (1, "U1"), (2, "comment"), (3, "U3")]);
    // The sheet's list holds triggers and the cells that cannot be cells: nothing here.
    assert!(style.sheet.passthrough.is_empty());
}

#[test]
fn a_page_sheet_and_a_document_sheet_count_their_cells_and_sections() {
    let builder = VsdxBuilder::new()
        .document_sheet(r#"<U0/><Cell N="A" V="1"/><U1/><Section N="User"/><U2/>"#)
        .page(PageSpec::new(1, "P").sheet(r#"<U0/><Cell N="PageWidth" V="1"/><U1/><Trigger N="T"/><Section N="Layer"/><U2/>"#));
    let doc = read_with(&builder.build(), &Limits::default()).0;
    expect(&doc.sheet.passthrough, &[(0, "U0"), (1, "U1"), (2, "U2")]);
    // A trigger is kept and is not a typed child, so the section is the second typed child and `U2` is after two.
    expect(&doc.pages.get(0).unwrap().sheet.passthrough, &[(0, "U0"), (1, "U1"), (1, "Trigger"), (2, "U2")]);
}

#[test]
fn the_document_part_counts_the_colour_table_the_font_table_the_styles_and_the_document_sheet() {
    let builder = VsdxBuilder::new();
    let xml = root(
        "VisioDocument",
        "<U0/><Colors><ColorEntry IX=\"24\" RGB=\"#000000\"/></Colors><!--c--><FaceNames><FaceName NameU=\"A\"/></FaceNames><U2/><StyleSheets><StyleSheet ID=\"0\"/></StyleSheets><DocumentSheet/><U4/><Colors/><U4b/>",
    );
    let doc = read_with(&with_part(&builder, "/visio/document.xml", &xml), &Limits::default()).0;
    // A second `Colors` is an unknown element, after the four typed ones.
    expect(&doc.passthrough, &[(0, "U0"), (1, "comment"), (2, "U2"), (4, "U4"), (4, "Colors"), (4, "U4b")]);
}

#[test]
fn a_page_row_counts_its_page_sheet_and_its_rel() {
    let builder = VsdxBuilder::new().page(PageSpec::new(1, "P"));
    let xml = root("Pages", r#"<Page ID="1" Name="A"><U0/><PageSheet/><U1/><Rel r:id="rId1"/><U2/><PageSheet/><U3/></Page>"#);
    let doc = read_with(&with_part(&builder, "/visio/pages/pages.xml", &xml), &Limits::default()).0;
    let page = doc.pages.get(0).unwrap();
    // The second `PageSheet` is an unknown element after the two typed children.
    expect(&page.passthrough, &[(0, "U0"), (1, "U1"), (2, "U2"), (2, "PageSheet"), (2, "U3")]);
}

#[test]
fn a_master_row_counts_its_page_sheet_its_rel_and_its_icon() {
    let builder = VsdxBuilder::new().master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4"/>"#));
    let xml =
        root("Masters", r#"<Master ID="1" NameU="M"><U0/><PageSheet/><U1/><Rel r:id="rId1"/><U2/><Icon>QUJD</Icon><U3/><Icon>x</Icon></Master>"#);
    let doc = read_with(&with_part(&builder, "/visio/masters/masters.xml", &xml), &Limits::default()).0;
    let master = doc.masters.get(0).unwrap();
    assert_eq!(master.icon.as_deref(), Some("QUJD"));
    expect(&master.passthrough, &[(0, "U0"), (1, "U1"), (2, "U2"), (3, "U3"), (3, "Icon")]);
}

#[test]
fn a_part_that_a_page_reaches_by_a_picture_relationship_is_read_with_the_rels_of_its_own() {
    // A guard for the fixture: the picture of the page is read through the page part's own relationships.
    let part = root(
        "PageContents",
        r#"<Shapes><Shape ID="1" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape></Shapes>"#,
    );
    let builder = VsdxBuilder::new()
        .page(PageSpec::new(1, "P").part(&part).rel("rId1", &format!("{OD}image"), "../media/i.png"))
        .with(Pkg::new().part("/visio/media/i.png", "image/png", [1u8, 2]));
    let doc = read_with(&builder.build(), &Limits::default()).0;
    assert!(first_shape(&doc).foreign.as_ref().unwrap().picture.is_some());
}

#[test]
fn a_section_with_no_name_is_kept_in_the_sheets_list_after_the_typed_children_before_it() {
    let doc = shape_doc(r#"<Shape ID="1"><Cell N="A" V="1"/><Section/><Cell N="B" V="2"/><Section N=""/></Shape>"#);
    expect(&first_shape(&doc).sheet.passthrough, &[(1, "Section"), (2, "Section")]);
}

#[test]
fn comments_instructions_and_text_in_a_master_row_and_a_page_row_keep_their_place() {
    let builder = VsdxBuilder::new().master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4"/>"#)).page(PageSpec::new(1, "P"));
    let masters = root("Masters", r#"<Master ID="1" NameU="M"><!--c0--><PageSheet/><?p d?>words<Rel r:id="rId1"/><!--c3--></Master>"#);
    let pages = root("Pages", r#"<Page ID="1" Name="A"><!--c0--><PageSheet/><?p d?>words<Rel r:id="rId1"/><!--c3--></Page>"#);
    let bytes = builder
        .pkg()
        .map_parts(|name, b| {
            if name.eq_ignore_ascii_case("/visio/masters/masters.xml") {
                *b = masters.as_bytes().to_vec();
            } else if name.eq_ignore_ascii_case("/visio/pages/pages.xml") {
                *b = pages.as_bytes().to_vec();
            }
        })
        .build();
    let doc = read_with(&bytes, &Limits::default()).0;
    // The page sheet is the first typed child and the `Rel` the second.
    expect(&doc.masters.get(0).unwrap().passthrough, &[(0, "comment"), (1, "pi"), (1, "text"), (2, "comment")]);
    expect(&doc.pages.get(0).unwrap().passthrough, &[(0, "comment"), (1, "pi"), (1, "text"), (2, "comment")]);
}
