//! Cells, sections, rows and shapes them: each test names its vector or requirement.

use diacraft_doc::{CellAddr, FormulaSlot, Lookup, RawNode, RowKey, SectionKey, ShapeType, TextPart};
use diacraft_testkit::package::{cell, cell_f, cell_u, shape};

use crate::{direct, doc_of, first_shape};

fn shape_of(inner: &str) -> std::sync::Arc<diacraft_doc::Shape> {
    first_shape(&doc_of(&shape(5, inner)).0)
}

#[test]
fn a_cell_keeps_its_formula_value_and_unit_as_the_file_gave_them() {
    let s = shape_of(r#"<Cell N="Width" V="1" F="Height*2" U="IN"/>"#);
    let c = direct(&s.sheet, "Width");
    assert_eq!(c.value.as_ref().unwrap().text(), "1");
    assert_eq!(c.formula.text(), Some("Height*2"));
    assert!(matches!(c.formula, FormulaSlot::Local(_)));
    assert_eq!(c.unit.as_deref(), Some("IN"));
    assert_eq!(c.error, None);
}

#[test]
fn a_value_is_not_normalised() {
    // R7 of the preservation spec: `V="0.10"` stays as written, and so does a long number and `1.#INF`.
    let s = shape_of(r#"<Cell N="A" V="0.10"/><Cell N="B" V="1.#INF"/><Cell N="C" V="00012.50e0"/><Cell N="D" V=""/>"#);
    for (name, text) in [("A", "0.10"), ("B", "1.#INF"), ("C", "00012.50e0"), ("D", "")] {
        assert_eq!(direct(&s.sheet, name).value.as_ref().unwrap().text(), text, "{name}");
    }
}

#[test]
fn e_is_read_beside_v_and_f() {
    let s = shape_of(r##"<Cell N="Width" V="2" F="1/0" E="#DIV/0!"/>"##);
    let c = direct(&s.sheet, "Width");
    assert_eq!(c.error.as_deref(), Some("#DIV/0!"));
    assert_eq!(c.value.as_ref().unwrap().text(), "2");
}

#[test]
fn inh_and_no_formula_are_markers_and_not_formulas() {
    let s = shape_of(r#"<Cell N="LineColor" V="0" F="Inh"/><Cell N="X" V="2" F="No Formula"/><Cell N="Y" V="3" F="Inhx"/>"#);
    assert!(matches!(direct(&s.sheet, "LineColor").formula, FormulaSlot::Inherit));
    assert!(matches!(direct(&s.sheet, "X").formula, FormulaSlot::NoFormula));
    // Only the exact words are markers: a formula that merely starts with them is a formula.
    assert!(matches!(direct(&s.sheet, "Y").formula, FormulaSlot::Local(_)));
}

#[test]
fn an_unknown_unit_is_kept() {
    let s = shape_of(r#"<Cell N="X" V="1" U="NURBS"/>"#);
    assert_eq!(direct(&s.sheet, "X").unit.as_deref(), Some("NURBS"));
}

#[test]
fn the_themed_marker_is_read_in_either_case_and_keeps_its_spelling() {
    let s = shape_of(r#"<Cell N="LineColor" V="Themed"/><Cell N="FillForegnd" V="themed"/>"#);
    for (name, text) in [("LineColor", "Themed"), ("FillForegnd", "themed")] {
        let v = direct(&s.sheet, name).value.as_ref().unwrap();
        assert!(v.is_themed(), "{name}");
        assert_eq!(v.text(), text);
    }
}

#[test]
fn text_in_a_cell_is_kept() {
    let s = shape_of(r#"<Cell N="Copyright">Some text</Cell>"#);
    let c = direct(&s.sheet, "Copyright");
    assert_eq!(c.text(), Some("Some text"));
    assert!(c.value.is_none());
}

#[test]
fn of_preservation_a_processing_instruction_in_a_cell_is_kept_and_does_not_fail() {
    let s = shape_of(r#"<Cell N="ThemeIndex" V="33"><?NewValue V?></Cell>"#);
    let extra = direct(&s.sheet, "ThemeIndex").extra.clone().unwrap();
    let kept: Vec<_> = extra.passthrough.iter().collect();
    assert_eq!(kept.len(), 1);
    assert!(matches!(&kept[0].node, RawNode::Pi { target, data } if &**target == "NewValue" && &**data == "V"));
}

#[test]
fn a_cell_with_text_and_a_child_keeps_its_content_in_order() {
    // Mixed content: text before and after an element is one passthrough in order, not a text and a list.
    let s = shape_of(r#"<Cell N="X" V="1">one<RefBy T="Page" ID="2"/>two</Cell>"#);
    let extra = direct(&s.sheet, "X").extra.clone().unwrap();
    assert!(extra.text.is_none());
    let nodes: Vec<_> = extra.passthrough.iter().map(|p| &p.node).collect();
    assert!(matches!(nodes[0], RawNode::Text(t) if &**t == "one"));
    assert!(matches!(nodes[1], RawNode::Element(e) if &*e.name == "RefBy"));
    assert!(matches!(nodes[2], RawNode::Text(t) if &**t == "two"));
}

#[test]
fn cell_names_match_by_exact_case() {
    // `pinx` is an unknown cell, kept verbatim, and `PinX` is not there.
    let s = shape_of(r#"<Cell N="pinx" V="1"/>"#);
    assert!(s.sheet.cell("pinx").is_some());
    assert!(s.sheet.cell("PinX").is_none());
}

#[test]
fn an_attribute_the_model_has_no_field_for_is_kept_in_order() {
    let s = shape_of(r#"<Cell N="X" V="1" Foo="bar" x:note="n" xmlns:x="urn:z" Baz="q"/>"#);
    let raw: Vec<_> = direct(&s.sheet, "X").raw.iter().collect();
    assert_eq!(raw, [("Foo", "bar"), ("x:note", "n"), ("xmlns:x", "urn:z"), ("Baz", "q")]);
}

#[test]
fn a_cell_with_no_name_or_an_empty_one_is_kept_as_an_unknown_element() {
    let (doc, report) = doc_of(&shape(5, r#"<Cell V="1"/><Cell N="" V="2"/><Cell N="Ok" V="3"/>"#));
    let s = first_shape(&doc);
    assert_eq!(s.sheet.cells.len(), 1);
    assert_eq!(s.sheet.passthrough.len(), 2);
    assert_eq!(report.warning_count("cell_without_name"), 2);
}

#[test]
fn a_cell_named_twice_in_one_place_keeps_the_first_and_the_second_as_an_element() {
    let (doc, report) = doc_of(&shape(5, r#"<Cell N="X" V="1"/><Cell N="X" V="2"/>"#));
    let s = first_shape(&doc);
    assert_eq!(direct(&s.sheet, "X").value.as_ref().unwrap().text(), "1");
    let kept: Vec<_> = s.sheet.passthrough.iter().collect();
    assert_eq!(kept.len(), 1);
    assert!(matches!(&kept[0].node, RawNode::Element(e) if &*e.name == "Cell" && e.attrs.get("V") == Some("2")));
    assert_eq!(report.warning_count("duplicate_cell"), 1);
}

#[test]
fn geometry_sections_with_no_ix_are_numbered_by_how_many_come_before() {
    let s = shape_of(r#"<Section N="Geometry"/><Section N="Geometry"/>"#);
    let ix: Vec<_> = s.sheet.sections.iter().map(|x| x.ix).collect();
    assert_eq!(ix, [Some(0), Some(1)]);
}

#[test]
fn an_ix_of_3_and_then_none_reads_as_3_and_1_with_a_warning() {
    let (doc, report) = doc_of(&shape(5, r#"<Section N="Geometry" IX="3"/><Section N="Geometry"/>"#));
    let s = first_shape(&doc);
    let ix: Vec<_> = s.sheet.sections.iter().map(|x| x.ix).collect();
    assert_eq!(ix, [Some(3), Some(1)]);
    assert_eq!(report.warning_count("geometry_ix_not_rising"), 1);
}

#[test]
fn ix_on_a_section_that_is_not_geometry_is_no_key() {
    // IX appears only on Geometry. On another section it stays an attribute, and the section is found by its name alone.
    let s = shape_of(r#"<Section N="User" IX="2"><Row N="a"/></Section>"#);
    let section = s.sheet.section(&SectionKey::new("User", None)).expect("the User section is found without an ix");
    assert_eq!(section.ix, None);
    assert_eq!(section.raw.get("IX"), Some("2"));
}

#[test]
fn a_row_is_keyed_by_its_ix_and_never_renumbered() {
    let s = shape_of(r#"<Section N="Geometry" IX="0"><Row T="LineTo" IX="2"><Cell N="X" V="1"/></Row></Section>"#);
    let section = s.sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap();
    assert_eq!(section.rows.len(), 1);
    assert_eq!(section.rows[0].ix, Some(2));
    assert_eq!(section.rows[0].row_type.as_deref(), Some("LineTo"));
    assert!(section.row(&RowKey::Ix(2)).is_some());
    assert!(section.row(&RowKey::Ix(0)).is_none());
    let found = s.sheet.lookup(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X"));
    assert!(matches!(found, Lookup::Found(c) if c.value.as_ref().unwrap().text() == "1"));
}

#[test]
fn an_absent_row_ix_is_the_count_of_earlier_rows_in_a_section_keyed_by_index() {
    let s = shape_of(r#"<Section N="Character"><Row IX="0"/><Row/><Row IX="5"/><Row/></Section>"#);
    let section = s.sheet.section(&SectionKey::new("Character", None)).unwrap();
    let ix: Vec<_> = section.rows.iter().map(|r| r.ix).collect();
    assert_eq!(ix, [Some(0), Some(1), Some(5), Some(3)]);
}

#[test]
fn a_section_keyed_by_name_gives_its_rows_no_number() {
    let s = shape_of(r#"<Section N="User"><Row N="a"/><Row N="b"/></Section>"#);
    let section = s.sheet.section(&SectionKey::new("User", None)).unwrap();
    assert!(section.rows.iter().all(|r| r.ix.is_none()));
    assert!(section.row(&RowKey::name("b")).is_some());
}

#[test]
fn a_row_with_both_n_and_ix_keeps_both() {
    let s = shape_of(r#"<Section N="Scratch"><Row N="n" IX="4"/></Section>"#);
    let row = &s.sheet.section(&SectionKey::new("Scratch", None)).unwrap().rows[0];
    assert_eq!((row.name.as_deref(), row.ix), (Some("n"), Some(4)));
}

#[test]
fn a_deleted_row_is_kept_and_marked() {
    let s = shape_of(r#"<Section N="Property"><Row N="Prop1" Del="1"/></Section>"#);
    let row = &s.sheet.section(&SectionKey::new("Property", None)).unwrap().rows[0];
    assert!(row.del);
    assert!(row.raw.is_empty());
}

#[test]
fn a_flag_that_is_not_the_word_one_keeps_its_text() {
    // Del="0" and Del="true" are not what a bool gives back, so the attribute stays, and wins.
    let s = shape_of(r#"<Section N="User"><Row N="a" Del="0"/><Row N="b" Del="true"/></Section>"#);
    let rows = &s.sheet.section(&SectionKey::new("User", None)).unwrap().rows;
    assert!(!rows[0].del);
    assert_eq!(rows[0].raw.get("Del"), Some("0"));
    assert!(rows[1].del);
    assert_eq!(rows[1].raw.get("Del"), Some("true"));
}

#[test]
fn a_number_that_is_not_plain_keeps_its_text_and_the_nearest_number() {
    let (doc, report) = doc_of(r#"<Shape ID="4294967296" Master="007"/>"#);
    let s = first_shape(&doc);
    // The ID is past u32: the shape reads as the nearest number, and the attribute is the truth.
    assert_eq!(s.id, u32::MAX);
    assert_eq!(s.raw.get("ID"), Some("4294967296"));
    assert_eq!(s.master, Some(7));
    assert_eq!(s.raw.get("Master"), Some("007"));
    assert_eq!(report.warning_count("shape_id_not_a_number"), 1);
}

#[test]
fn ids_are_any_unsigned_number_and_kept_exactly() {
    for id in [0u32, 1, 3, 4, u32::MAX] {
        let (doc, report) = doc_of(&shape(id, ""));
        let s = first_shape(&doc);
        assert_eq!(s.id, id);
        assert!(s.raw.is_empty());
        assert_eq!(report.warnings, Vec::<String>::new(), "id {id}");
    }
}

#[test]
fn a_shape_with_no_id_is_read_as_id_zero_and_warned_of() {
    let (doc, report) = doc_of("<Shape/>");
    assert_eq!(first_shape(&doc).id, 0);
    assert_eq!(report.warning_count("shape_without_id"), 1);
}

#[test]
fn the_attributes_the_model_has_no_field_for_are_kept_in_order() {
    let (doc, _) = doc_of(
        r#"<Shape ID="5" OriginalID="9" UniqueID="{X}" IsCustomName="1" Foo="bar" Name="N" NameU="U" Type="Group" LineStyle="3" FillStyle="" TextStyle="x"/>"#,
    );
    let s = first_shape(&doc);
    let raw: Vec<_> = s.raw.iter().collect();
    assert_eq!(raw, [("OriginalID", "9"), ("UniqueID", "{X}"), ("IsCustomName", "1"), ("Foo", "bar")]);
    assert_eq!((s.name.as_deref(), s.name_u.as_deref()), (Some("N"), Some("U")));
    assert_eq!(s.kind, ShapeType::Group);
    // The style links are the sheet's: a number is typed; an empty link and a word are no link, and the attribute is kept in the sheet.
    assert_eq!(s.sheet.links.line, Some(3));
    assert_eq!(s.sheet.links.fill, None);
    assert_eq!(s.sheet.links.text, None);
    let sheet_raw: Vec<_> = s.sheet.raw.iter().collect();
    assert_eq!(sheet_raw, [("FillStyle", ""), ("TextStyle", "x")]);
}

#[test]
fn a_shape_type_nobody_lists_is_kept() {
    let (doc, _) = doc_of(r#"<Shape ID="1" Type="Weird"/><Shape ID="2"/>"#);
    let list = &doc.pages.get(0).unwrap().shapes;
    assert_eq!(list.get(0).unwrap().kind, ShapeType::Other("Weird".into()));
    assert_eq!(list.get(1).unwrap().kind, ShapeType::Unspecified);
}

#[test]
fn a_shape_takes_its_children_in_any_order_and_keeps_a_foreign_one_where_it_sat() {
    // An element in a third party's namespace sits between others, and stays there: after one typed child.
    let (doc, _) = doc_of(
        r#"<Shape ID="5"><Section N="Geometry"/><Cell N="X" V="1"/><x:note xmlns:x="urn:z" a="1">hi</x:note><Cell N="Y" V="2"/><Text>t</Text></Shape>"#,
    );
    let s = first_shape(&doc);
    assert!(s.sheet.cell("X").is_some() && s.sheet.cell("Y").is_some());
    assert_eq!(s.sheet.sections.len(), 1);
    let kept: Vec<_> = s.passthrough.iter().collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].after, 2, "the Section and the first Cell came before it");
    assert!(matches!(&kept[0].node, RawNode::Element(e) if &*e.name == "x:note" && e.attrs.get("xmlns:x") == Some("urn:z")));
}

#[test]
fn an_element_in_another_namespace_is_never_read_as_a_vsdx_one() {
    // A foreign `Cell` and a foreign `Shape`: kept, and not read, whatever their local names.
    let (doc, _) =
        doc_of(r#"<Shape ID="5"><x:Cell xmlns:x="urn:z" N="X" V="1"/><Cell xmlns="urn:z" N="Y" V="2"/></Shape><x:Shape xmlns:x="urn:z" ID="6"/>"#);
    let list = &doc.pages.get(0).unwrap().shapes;
    assert_eq!(list.len(), 1, "the foreign Shape is no shape");
    let s = first_shape(&doc);
    assert!(s.sheet.cells.is_empty());
    assert_eq!(s.passthrough.len(), 2);
}

#[test]
fn an_older_xform1d_element_is_an_unknown_element_with_its_cells() {
    let s = shape_of(r#"<XForm1D><Cell N="BeginX" V="1"/></XForm1D><Cell N="PinX" V="2"/>"#);
    assert!(s.sheet.cell("BeginX").is_none(), "the cells inside an unknown element are not the shape's");
    let kept: Vec<_> = s.passthrough.iter().collect();
    assert_eq!(kept.len(), 1);
    assert!(matches!(&kept[0].node, RawNode::Element(e) if &*e.name == "XForm1D" && e.children.len() == 1));
}

#[test]
fn a_trigger_is_kept_in_the_sheets_passthrough() {
    let s = shape_of(r#"<Cell N="X" V="1"/><Trigger N="RecalcColor"><RefBy T="Page" ID="0"/></Trigger>"#);
    let kept: Vec<_> = s.sheet.passthrough.iter().collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].after, 1);
    assert!(matches!(&kept[0].node, RawNode::Element(e) if &*e.name == "Trigger"));
}

#[test]
fn data1_data2_and_data3_are_read_and_an_odd_one_is_kept_whole() {
    let s = shape_of(r#"<Data1>one</Data1><Data2/><Data3 foo="x">three</Data3>"#);
    assert_eq!(s.data1.as_deref(), Some("one"));
    assert_eq!(s.data2.as_deref(), Some(""));
    assert_eq!(s.data3, None);
    assert_eq!(s.passthrough.len(), 1);
}

#[test]
fn text_is_kept_in_pieces_with_its_markers() {
    let s = shape_of(r#"<Text><cp IX="0"/>ab<cp IX="1"/>cd<fld IX="0">3</fld></Text>"#);
    let parts = &s.text.as_ref().unwrap().parts;
    assert_eq!(
        parts.as_slice(),
        [TextPart::Cp(0), TextPart::Chars("ab".into()), TextPart::Cp(1), TextPart::Chars("cd".into()), TextPart::Fld { ix: 0, text: "3".into() }]
    );
}

#[test]
fn of_the_document_spec_whitespace_in_text_is_exact() {
    let s = shape_of("<Text>  two  spaces </Text>");
    assert_eq!(s.text.as_ref().unwrap().parts.as_slice(), [TextPart::Chars("  two  spaces ".into())]);
}

#[test]
fn a_text_marker_that_is_not_plain_is_kept_whole() {
    let s = shape_of(r#"<Text><cp IX="x"/><pp IX="1" a="b"/><tp IX="2"/><zz/>x<!--c--><?pi d?></Text>"#);
    let parts = &s.text.as_ref().unwrap().parts;
    assert!(matches!(&parts[0], TextPart::Raw(RawNode::Element(e)) if &*e.name == "cp"));
    assert!(matches!(&parts[1], TextPart::Raw(RawNode::Element(e)) if &*e.name == "pp"));
    assert_eq!(parts[2], TextPart::Tp(2));
    assert!(matches!(&parts[3], TextPart::Raw(RawNode::Element(e)) if &*e.name == "zz"));
    assert_eq!(parts[4], TextPart::Chars("x".into()));
    assert!(matches!(&parts[5], TextPart::Raw(RawNode::Comment(_))));
    assert!(matches!(&parts[6], TextPart::Raw(RawNode::Pi { .. })));
}

#[test]
fn an_empty_text_element_is_a_text_with_no_parts() {
    let s = shape_of("<Text/>");
    assert!(s.text.as_ref().unwrap().parts.is_empty());
    assert!(shape_of("").text.is_none());
}

#[test]
fn a_geometry_section_that_starts_later_is_not_renumbered() {
    let s = shape_of(r#"<Section N="Geometry" IX="0"><Row T="LineTo" IX="2"/><Row T="LineTo" IX="3"/></Section>"#);
    let rows = &s.sheet.sections[0].rows;
    assert_eq!(rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(2), Some(3)]);
}

#[test]
fn a_cell_under_a_section_and_a_cell_in_a_row_are_told_apart() {
    let s = shape_of(&format!(
        r#"<Section N="Geometry" IX="0">{}<Row T="MoveTo" IX="1">{}</Row></Section>"#,
        cell("NoFill", "1"),
        cell_u("X", "2", "MM")
    ));
    let section = &s.sheet.sections[0];
    assert!(section.cells.contains_key("NoFill"));
    assert_eq!(section.rows[0].cells.get("X").unwrap().unit.as_deref(), Some("MM"));
    let f = cell_f("Y", "0", "X*2");
    assert!(f.contains("F=\"X*2\""));
}

#[test]
fn a_tab_element_in_a_tabs_row_is_kept_as_an_unknown_element() {
    // The Learn schema's `Tab` elements are not what real files hold, and they are kept all the same.
    let s = shape_of(r#"<Section N="Tabs"><Row IX="0"><Cell N="Position0" V="1"/><Tab><Cell N="Position" V="2"/></Tab></Row></Section>"#);
    let row = &s.sheet.sections[0].rows[0];
    assert!(row.cells.contains_key("Position0"));
    assert_eq!(row.passthrough.len(), 1);
}
