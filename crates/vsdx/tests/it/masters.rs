//! Masters, stencils and how shapes use them.

use diacraft_doc::RawNode;
use diacraft_testkit::package::{MasterSpec, PageSpec, Pkg, VIS, VsdxBuilder, shape, shape_with};

use crate::{first_shape, read_ok};

fn read_builder(b: VsdxBuilder) -> (diacraft_doc::Document, diacraft_vsdx::ReadReport) {
    read_ok(&b.build())
}

#[test]
fn a_master_with_id_zero_is_one_master() {
    let b = VsdxBuilder::new().master(MasterSpec::new(0, "Box").attrs(r#" UniqueID="{00000000-0000-0000-0000-000000000001}""#));
    let (doc, report) = read_builder(b);
    assert_eq!(doc.masters.len(), 1);
    let m = doc.master(0).unwrap();
    assert_eq!((m.id, m.name.as_deref(), m.unique_id.as_deref()), (0, Some("Box"), Some("{00000000-0000-0000-0000-000000000001}")));
    assert!(m.rel_id.is_some());
    assert_eq!(report.stats.masters, 1);
}

#[test]
fn a_master_with_no_rel_has_no_shapes_and_its_row_is_kept() {
    let (doc, _) = read_builder(VsdxBuilder::new().master(MasterSpec::new(3, "Empty").without_part()));
    let m = doc.master(3).unwrap();
    assert!(m.shapes.is_empty());
    assert_eq!(m.rel_id, None);
}

#[test]
fn a_master_shortcut_is_not_modelled_and_is_never_fetched() {
    let masters_xml = r#"<Masters xmlns="http://schemas.microsoft.com/office/visio/2012/main"><MasterShortcut ID="9" ShortcutURL="https://example.invalid/x"/></Masters>"#;
    let pkg = Pkg::new().part("/visio/masters/masters.xml", "application/vnd.ms-visio.masters+xml", masters_xml).rel(
        "/visio/document.xml",
        "rId60",
        &format!("{VIS}masters"),
        "masters/masters.xml",
    );
    let (doc, report) = read_builder(VsdxBuilder::new().without_masters_part().with(pkg));
    assert_eq!(doc.masters.len(), 0, "A Masters with only shortcut rows is normal");
    // The shortcut is held in place, as the row it is, and never fetched.
    assert_eq!(report.skipped_counts.len(), 0);
    let held = crate::held::dump(crate::held::rest(&doc.ext.get().expect("the document's slot").vsdx, "Masters")[0]);
    assert!(held.contains("[after 0] <n><MasterShortcut ID=\"9\" ShortcutURL=\"https://example.invalid/x\"/></n>"), "{held}");
}

#[test]
fn pattern_flags_are_kept_as_numbers_and_the_other_attributes_in_order() {
    let attrs = r#" PatternFlags="65" BaseID="{B}" MatchByName="0" IconSize="1" Hidden="1" MasterType="8197""#;
    let (doc, _) = read_builder(
        VsdxBuilder::new()
            .master(MasterSpec::new(1, "A").attrs(attrs))
            .master(MasterSpec::new(2, "B").attrs(r#" PatternFlags="0""#))
            .master(MasterSpec::new(3, "C").attrs(r#" PatternFlags="33""#))
            .master(MasterSpec::new(4, "D").attrs(r#" PatternFlags="033""#)),
    );
    let flags: Vec<_> = doc.masters.iter().map(|m| m.pattern_flags).collect();
    assert_eq!(flags, [Some(65), Some(0), Some(33), Some(33)]);
    let raw: Vec<_> = doc.master(1).unwrap().raw.iter().collect();
    assert_eq!(raw, [("BaseID", "{B}"), ("MatchByName", "0"), ("IconSize", "1"), ("Hidden", "1"), ("MasterType", "8197")]);
    assert_eq!(doc.master(4).unwrap().raw.get("PatternFlags"), Some("033"), "a number that gives no text back stays an attribute");
    assert!(doc.master(3).unwrap().raw.is_empty());
}

#[test]
fn a_master_part_may_hold_two_top_level_shapes() {
    let (doc, report) = read_builder(VsdxBuilder::new().master(MasterSpec::new(2, "M").shapes(&format!("{}{}", shape(4, ""), shape(5, "")))));
    assert_eq!(doc.master(2).unwrap().shapes.len(), 2);
    assert_eq!(report.stats.max_shapes_in_a_master, 2);
}

#[test]
fn a_member_names_a_shape_of_the_master_of_its_group() {
    // Page shape 10 is an instance of master 2, and its member 11 stands for shape 6 of that master.
    let master = MasterSpec::new(2, "M").shapes(&shape(6, ""));
    let page = shape_with(10, r#" Master="2""#, &format!("<Shapes>{}</Shapes>", shape_with(11, r#" MasterShape="6""#, "")));
    let (doc, report) = read_builder(VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&page)));
    let group = first_shape(&doc);
    assert_eq!((group.master, group.children.get(0).unwrap().master_shape), (Some(2), Some(6)));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_nested_master_starts_a_new_scope_for_its_descendants() {
    // Shape 12's MasterShape 6 is looked up in master 3, the master of its nearest ancestor that has one, and not in master 2.
    let m2 = MasterSpec::new(2, "M2").shapes(&shape(5, ""));
    let m3 = MasterSpec::new(3, "M3").shapes(&shape(6, ""));
    let inner = shape_with(12, r#" MasterShape="6""#, "");
    let mid = shape_with(11, r#" Master="3""#, &format!("<Shapes>{inner}</Shapes>"));
    let page = shape_with(10, r#" Master="2""#, &format!("<Shapes>{mid}</Shapes>"));
    let (_, report) = read_builder(VsdxBuilder::new().master(m2).master(m3).page(PageSpec::new(1, "P").shapes(&page)));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_master_that_names_nothing_is_a_warning_and_the_attribute_is_kept() {
    let (doc, report) = read_builder(VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape_with(1, r#" Master="9""#, ""))));
    assert_eq!(first_shape(&doc).master, Some(9));
    assert_eq!(report.warning_count("master_not_found"), 1);
}

#[test]
fn a_master_shape_that_does_not_resolve_is_a_warning_and_the_attribute_is_kept() {
    let master = MasterSpec::new(2, "M").shapes(&shape(6, ""));
    let page = shape_with(
        10,
        r#" Master="2""#,
        &format!("<Shapes>{}{}</Shapes>", shape_with(11, r#" MasterShape="77""#, ""), shape_with(12, r#" MasterShape="6""#, "")),
    );
    let (doc, report) = read_builder(VsdxBuilder::new().master(master).page(PageSpec::new(1, "P").shapes(&page)));
    assert_eq!(first_shape(&doc).children.get(0).unwrap().master_shape, Some(77));
    assert_eq!(report.warning_count("master_shape_not_found"), 1);
}

#[test]
fn a_master_shape_with_no_master_above_it_is_a_warning() {
    let (_, report) = read_builder(VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape_with(1, r#" MasterShape="6""#, ""))));
    assert_eq!(report.warning_count("master_shape_without_master"), 1);
}

#[test]
fn a_master_shape_in_a_master_part_cannot_name_a_master_so_no_cycle_of_masters_exists() {
    // Shapes in master parts never have `Master` or `MasterShape`. A hostile one that does keeps them as attributes and reads no tie.
    let hostile = shape_with(4, r#" Master="1" MasterShape="4""#, "");
    let (doc, report) = read_builder(VsdxBuilder::new().master(MasterSpec::new(1, "Loop").shapes(&hostile)));
    let s = doc.master(1).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!((s.master, s.master_shape), (None, None));
    assert_eq!((s.raw.get("Master"), s.raw.get("MasterShape")), (Some("1"), Some("4")));
    assert!(report.warnings.is_empty());
}

#[test]
fn two_masters_with_one_unique_id_are_both_read_and_warned_of() {
    let attrs = r#" UniqueID="{U}""#;
    let (doc, report) = read_builder(VsdxBuilder::new().master(MasterSpec::new(1, "A").attrs(attrs)).master(MasterSpec::new(2, "B").attrs(attrs)));
    assert_eq!(doc.masters.len(), 2);
    assert_eq!(report.warning_count("duplicate_master_unique_id"), 1);
}

#[test]
fn a_master_id_used_twice_keeps_both_and_the_first_is_the_one_found() {
    let (doc, report) = read_builder(VsdxBuilder::new().master(MasterSpec::new(1, "A")).master(MasterSpec::new(1, "B")));
    assert_eq!(doc.masters.len(), 2);
    assert_eq!(doc.master(1).unwrap().name.as_deref(), Some("A"));
    assert_eq!(report.warning_count("duplicate_master_id"), 1);
}

#[test]
fn a_master_with_no_id_is_skipped_and_reported() {
    let masters_xml = r#"<Masters xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Master Name="A"/><Master ID="2" Name="B"/></Masters>"#;
    let pkg = Pkg::new().part("/visio/masters/masters.xml", "application/vnd.ms-visio.masters+xml", masters_xml).rel(
        "/visio/document.xml",
        "rId60",
        &format!("{VIS}masters"),
        "masters/masters.xml",
    );
    let (doc, report) = read_builder(VsdxBuilder::new().without_masters_part().with(pkg));
    assert_eq!(doc.masters.iter().map(|m| m.id).collect::<Vec<_>>(), [2]);
    assert_eq!(report.skipped_counts.len(), 0);
    let held = crate::held::dump(crate::held::rest(&doc.ext.get().expect("the document's slot").vsdx, "Masters")[0]);
    assert!(held.contains("[after 0] <n><Master Name=\"A\"/></n>"), "the row with no `ID` is held in place: {held}");
}

#[test]
fn the_icon_is_kept_as_text_and_an_icon_with_attributes_is_kept_whole() {
    let masters_xml = r#"<Masters xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Master ID="1"><Icon>AAAB</Icon></Master><Master ID="2"><Icon IconSize="1">AAAB</Icon></Master></Masters>"#;
    let pkg = Pkg::new().part("/visio/masters/masters.xml", "application/vnd.ms-visio.masters+xml", masters_xml).rel(
        "/visio/document.xml",
        "rId60",
        &format!("{VIS}masters"),
        "masters/masters.xml",
    );
    let (doc, _) = read_builder(VsdxBuilder::new().without_masters_part().with(pkg));
    assert_eq!(doc.master(1).unwrap().icon.as_deref(), Some("AAAB"));
    let odd = doc.master(2).unwrap();
    assert_eq!(odd.icon, None);
    assert!(matches!(&odd.passthrough.iter().next().unwrap().node, RawNode::Element(e) if &*e.name == "Icon"));
}

#[test]
fn a_package_with_no_masters_index_has_no_masters_and_reads() {
    let (doc, _) = read_builder(VsdxBuilder::new().without_masters_part().page(PageSpec::new(1, "P")));
    assert_eq!(doc.masters.len(), 0);
}

#[test]
fn a_masters_index_whose_root_is_not_masters_is_warned_of_and_read_as_unknown() {
    let pkg = Pkg::new()
        .part(
            "/visio/masters/masters.xml",
            "application/vnd.ms-visio.masters+xml",
            r#"<Pages xmlns="http://schemas.microsoft.com/office/visio/2012/main"/>"#,
        )
        .rel("/visio/document.xml", "rId60", &format!("{VIS}masters"), "masters/masters.xml");
    let (doc, report) = read_builder(VsdxBuilder::new().without_masters_part().with(pkg));
    assert_eq!(doc.masters.len(), 0);
    assert_eq!(report.warning_count("part_unexpected_root"), 1);
}

#[test]
fn a_master_page_sheet_is_read_with_its_layers() {
    let sheet = r#"<Cell N="PageWidth" V="1"/><Section N="Layer"><Row IX="0"><Cell N="Name" V="Connector"/></Row></Section>"#;
    let masters_xml = format!(
        r#"<Masters xmlns="http://schemas.microsoft.com/office/visio/2012/main"><Master ID="1"><PageSheet UniqueID="{{P}}">{sheet}</PageSheet></Master></Masters>"#
    );
    let pkg = Pkg::new().part("/visio/masters/masters.xml", "application/vnd.ms-visio.masters+xml", masters_xml).rel(
        "/visio/document.xml",
        "rId60",
        &format!("{VIS}masters"),
        "masters/masters.xml",
    );
    let (doc, _) = read_builder(VsdxBuilder::new().without_masters_part().with(pkg));
    let s = &doc.master(1).unwrap().sheet;
    assert!(s.cell("PageWidth").is_some());
    assert_eq!(s.sections[0].rows.len(), 1);
    assert_eq!(s.raw.get("UniqueID"), Some("{P}"));
}

#[test]
fn a_shape_in_a_master_part_reads_its_own_style_links_and_ids_without_a_master() {
    let master = MasterSpec::new(1, "M").shapes(&shape_with(4, r#" LineStyle="0" Type="Shape""#, ""));
    let (doc, report) = read_builder(VsdxBuilder::new().master(master));
    let s = doc.master(1).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!(s.sheet.links.line, Some(0));
    assert_eq!(report.warning_count("style_not_found"), 0, "style 0 is the root style the builder gives");
}
