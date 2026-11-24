//! What makes no content for VSDX packages (the classes F3, F4 and F7): a relationship row that implies and that one
//! package lacks; a part's content type given where it had none, a property part's own type for the drawing's, and the standard `rels` `Default` for an `Override`
//! of a relationships part; and a `Row` with no `IX` against one whose `IX` is the count of the rows before it. Each case the check now calls the same has controls
//! beside it that a real difference in the same place still differs. Every package is built in code (`diacraft-testkit`).

use diacraft_container::canon::{ContentDiff, Found};
use diacraft_testkit::package::{
    CT_APP, CT_CORE, CT_DRAWING, CT_PAGE, CT_RELS, MasterSpec, Namespace, OD, PK, PageSpec, VIS, VsdxBuilder, ZipItem, zip,
};
use diacraft_vsdx::{Limits, compare_content};

const CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const CT_CUSTOM: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const CT_OCTETS: &str = "application/octet-stream";

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

// ------------------------------------------------------------------------------------------------------------ F7: a Row with no IX

const BOX: &str = r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="1"/>"#;

/// A drawing in `ns` with one shape that holds `inner`.
fn shape_in(ns: Namespace, inner: &str) -> Vec<u8> {
    VsdxBuilder::new().namespace(ns).page(PageSpec::new(1, "P").shapes(&format!("{BOX}{inner}</Shape>"))).build()
}

fn section(name: &str, rows: &str) -> String {
    format!(r#"<Section N="{name}" IX="0">{rows}</Section>"#)
}

const THREE_NAMESPACES: [Namespace; 3] = [Namespace::Main2012, Namespace::Core2011, Namespace::None];

#[test]
fn a_row_with_no_ix_is_the_same_as_one_whose_ix_is_the_count_of_the_rows_before_it() {
    let bare = r#"<Row T="MoveTo"><Cell N="X" V="0"/></Row><Row T="LineTo"><Cell N="X" V="1"/></Row><Row T="LineTo"><Cell N="X" V="2"/></Row>"#;
    let said = r#"<Row T="MoveTo" IX="0"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="1"><Cell N="X" V="1"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/></Row>"#;
    let some = r#"<Row T="MoveTo" IX="0"><Cell N="X" V="0"/></Row><Row T="LineTo"><Cell N="X" V="1"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/></Row>"#;
    for ns in THREE_NAMESPACES {
        for kind in ["Geometry", "Character", "Paragraph", "Field", "FillGradient", "LineGradient", "Layer", "Reviewer", "Scratch", "Tabs"] {
            let bare_package = shape_in(ns, &section(kind, bare));
            same(&bare_package, &shape_in(ns, &section(kind, said)), kind);
            same(&bare_package, &shape_in(ns, &section(kind, some)), kind);
            same(&shape_in(ns, &section(kind, some)), &shape_in(ns, &section(kind, said)), kind);
        }
        // A kind in neither list (`Connection`): a row with no name is placed, as the reader places it.
        let (bare, said) = (r#"<Row T="Connection"><Cell N="X" V="0"/></Row>"#, r#"<Row T="Connection" IX="0"><Cell N="X" V="0"/></Row>"#);
        same(&shape_in(ns, &section("Connection", bare)), &shape_in(ns, &section("Connection", said)), "Connection");
    }
}

#[test]
fn an_ix_that_disagrees_with_the_place_of_the_row_is_still_a_difference() {
    let bare = section("Geometry", r#"<Row T="MoveTo"/><Row T="LineTo"/>"#);
    for (rows, what) in [
        (r#"<Row T="MoveTo" IX="0"/><Row T="LineTo" IX="2"/>"#, "the second row says 2"),
        (r#"<Row T="MoveTo" IX="1"/><Row T="LineTo" IX="1"/>"#, "the first row says 1"),
        (r#"<Row T="MoveTo" IX="0"/><Row T="LineTo" IX="0"/>"#, "the second row says 0"),
        (r#"<Row T="MoveTo" IX="0"/><Row T="LineTo" IX="01"/>"#, "a 1 spelt 01"),
        (r#"<Row T="MoveTo" IX="0"/><Row T="LineTo" IX=""/>"#, "a blank one"),
    ] {
        let d = differs(&shape_in(Namespace::Main2012, &bare), &shape_in(Namespace::Main2012, &section("Geometry", rows)), what);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Value), "{what}: {:?}", d.shown);
    }
    // Each row's place is the count before it, not the highest `IX` before it plus one: a row that says 7 is the first.
    let seven = section("Geometry", r#"<Row T="MoveTo" IX="7"/><Row T="LineTo"/>"#);
    same(
        &shape_in(Namespace::Main2012, &seven),
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Row T="MoveTo" IX="7"/><Row T="LineTo" IX="1"/>"#)),
        "after 7",
    );
    differs(
        &shape_in(Namespace::Main2012, &seven),
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Row T="MoveTo" IX="7"/><Row T="LineTo" IX="8"/>"#)),
        "after 7, the next is not 8",
    );
}

#[test]
fn a_row_keyed_by_name_keeps_an_absent_ix_absent() {
    for kind in ["User", "Property", "Actions", "Control", "Hyperlink", "ActionTag"] {
        let named = section(kind, r#"<Row N="Cost"><Cell N="Value" V="3"/></Row>"#);
        let said = section(kind, r#"<Row N="Cost" IX="0"><Cell N="Value" V="3"/></Row>"#);
        let d = differs(&shape_in(Namespace::Main2012, &named), &shape_in(Namespace::Main2012, &said), kind);
        assert_eq!(d.shown.first().map(|x| x.found), Some(Found::Attribute), "{kind}: {:?}", d.shown);
    }
    // A kind in neither list: a row that has a name is found by it, so its `IX` is not the count.
    let named = section("Connection", r#"<Row N="x"/>"#);
    differs(&shape_in(Namespace::Main2012, &named), &shape_in(Namespace::Main2012, &section("Connection", r#"<Row N="x" IX="0"/>"#)), "named");
    // A row of the indexed kind that has a name too is placed all the same (the reader places it).
    same(
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Row N="x"/>"#)),
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Row N="x" IX="0"/>"#)),
        "named in Geometry",
    );
    // Rows are not the cells of a section, and another element is not a row.
    differs(
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Cell N="Y" V="0"/><Other/>"#)),
        &shape_in(Namespace::Main2012, &section("Geometry", r#"<Cell N="Y" V="0"/><Other IX="0"/>"#)),
        "another element",
    );
}

#[test]
fn the_lists_of_sections_are_the_ones_the_reader_keys_rows_by() {
    use diacraft_doc::{Keying, keying};
    let placed = diacraft_vsdx::CANON_RULES.placed;
    assert_eq!(placed.len(), 3, "one entry for each namespace a part can be written in");
    for entry in placed {
        assert_eq!(
            (entry.parent.local, entry.child.local, entry.kind.local, entry.place.local, entry.named_by.local),
            ("Section", "Row", "N", "IX", "N")
        );
        for name in entry.always {
            assert_eq!(keying(name), Keying::Indexed, "{name}");
        }
        for name in entry.never {
            assert_eq!(keying(name), Keying::Named, "{name}");
        }
        // Every section the reader keys one way is listed.
        for name in ["Character", "Field", "FillGradient", "Geometry", "Layer", "LineGradient", "Paragraph", "Reviewer", "Scratch", "Tabs"] {
            assert!(entry.always.contains(&name), "{name}");
        }
        for name in ["User", "Property", "Actions", "Control", "Hyperlink", "ActionTag"] {
            assert!(entry.never.contains(&name), "{name}");
        }
        assert_eq!(keying("Connection"), Keying::Either);
    }
}

// ------------------------------------------------------------------------------------------------------------ F4: a part's content type

/// A package of the parts given as (name, bytes), with the content types stream holding `rows`. No relationship, no document: the check reads any package.
fn mini(rows: &str, parts: &[(&str, &str)]) -> Vec<u8> {
    let types = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="{CONTENT_TYPES}">{rows}</Types>"#);
    let mut items = vec![ZipItem::stored("[Content_Types].xml", types.as_bytes())];
    items.extend(parts.iter().map(|(name, bytes)| ZipItem::stored(name, bytes.as_bytes())));
    zip(&items)
}

const XML_DEFAULT: &str = r#"<Default Extension="xml" ContentType="application/xml"/>"#;
const RELS_DEFAULT: &str = r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#;

fn over(part: &str, content_type: &str) -> String {
    format!(r#"<Override PartName="{part}" ContentType="{content_type}"/>"#)
}

fn default_for(extension: &str, content_type: &str) -> String {
    format!(r#"<Default Extension="{extension}" ContentType="{content_type}"/>"#)
}

#[test]
fn a_part_that_had_no_type_and_is_given_one_is_the_same() {
    let parts = [("x/thing.dat", "0123"), ("x/other.xml", "<a/>")];
    let none = mini(XML_DEFAULT, &parts);
    // By an `Override` of its own, and by a `Default` for its extension, either way round.
    same(&none, &mini(&format!("{XML_DEFAULT}{}", over("/x/thing.dat", CT_OCTETS)), &parts), "an Override");
    same(&none, &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &parts), "a Default");
    // A `Default` that covers several such parts.
    let more = [("x/a.dat", "1"), ("x/b.dat", "2"), ("x/other.xml", "<a/>")];
    same(&mini(XML_DEFAULT, &more), &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &more), "a Default for two");
    // A part of upper-case name and extension, found as OPC finds it.
    same(&mini(XML_DEFAULT, &[("x/UP.DAT", "1")]), &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &[("x/UP.DAT", "1")]), "case");
    // The other parts keep their rows: the `Default` for `xml` was in both.
    same(&none, &none, "itself");
}

#[test]
fn a_type_given_to_a_part_that_had_one_in_the_other_package_is_still_a_difference() {
    let parts = [("x/p.xml", "<a/>"), ("x/b.dat", "2")];
    // The types differ for a part that has one in each: a page typed as a master.
    let d = differs(
        &mini(&format!("{XML_DEFAULT}{}", over("/x/p.xml", CT_PAGE)), &parts),
        &mini(&format!("{XML_DEFAULT}{}", over("/x/p.xml", "application/vnd.ms-visio.master+xml")), &parts),
        "a page typed as a master",
    );
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| (x.part.as_str(), x.found)), Some(("[Content_Types].xml", Found::Value)));
    // `Default` against `Override` for a part that has a type in both stays a difference (the choice is not recorded in the model).
    differs(
        &mini(&format!("{XML_DEFAULT}{}", over("/x/b.dat", CT_OCTETS)), &parts),
        &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &parts),
        "Override against Default",
    );
    // A `Default` that covers a part that had a type by an `Override` of its own is not an addition for a part that had none.
    let two = [("x/a.dat", "1"), ("x/b.dat", "2")];
    differs(
        &mini(&format!("{XML_DEFAULT}{}", over("/x/b.dat", CT_OCTETS)), &two),
        &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &two),
        "a Default over a typed part",
    );
    // A `Default` for an extension that the one package has a row for and the other gives another type is a value.
    differs(
        &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &two),
        &mini(&format!("{XML_DEFAULT}{}", default_for("dat", "application/x")), &two),
        "another type",
    );
}

#[test]
fn a_row_for_no_part_or_for_a_part_only_one_package_has_is_still_a_difference() {
    let parts = [("x/p.xml", "<a/>")];
    // A row that types nothing in the package: not a part that had none given one.
    differs(&mini(XML_DEFAULT, &parts), &mini(&format!("{XML_DEFAULT}{}", over("/x/ghost.dat", CT_OCTETS)), &parts), "an Override for no part");
    differs(&mini(XML_DEFAULT, &parts), &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &parts), "a Default for no part");
    // A part one package has is a part it lacks, and its row is one more.
    let more = [("x/p.xml", "<a/>"), ("x/thing.dat", "0")];
    let d = differs(&mini(XML_DEFAULT, &parts), &mini(&format!("{XML_DEFAULT}{}", over("/x/thing.dat", CT_OCTETS)), &more), "an extra part");
    assert!(d.shown.iter().any(|x| x.found == Found::ExtraPart || x.found == Found::MissingPart), "{:?}", d.shown);
    // A `Default` that covers a part only the one package has.
    differs(&mini(XML_DEFAULT, &parts), &mini(&format!("{XML_DEFAULT}{}", default_for("dat", CT_OCTETS)), &more), "a Default over an extra part");
}

/// The package relationships part with one row of `ty` to `target`.
fn root_rels(ty: &str, target: &str) -> String {
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{ty}" Target="{target}"/></Relationships>"#
    )
}

#[test]
fn a_property_part_typed_as_the_drawing_and_given_its_own_type_is_the_same() {
    for (name, ty, own) in [
        ("docProps/app.xml", format!("{OD}extended-properties"), CT_APP),
        ("docProps/custom.xml", format!("{OD}custom-properties"), CT_CUSTOM),
        ("docProps/core.xml", format!("{PK}core-properties"), CT_CORE),
    ] {
        let rels = root_rels(&ty, name);
        let parts = [("_rels/.rels", rels.as_str()), (name, "<Properties/>")];
        let rows = |content_type: &str| format!("{XML_DEFAULT}{RELS_DEFAULT}{}", over(&format!("/{name}"), content_type));
        let (drawn, owned) = (mini(&rows(CT_DRAWING), &parts), mini(&rows(own), &parts));
        same(&drawn, &owned, name);
        // Any of the six types of the drawing's document is the drawing's.
        same(&mini(&rows("application/vnd.ms-visio.stencil.macroEnabled.main+xml"), &parts), &owned, name);
        // Another type for the part is not.
        differs(&drawn, &mini(&rows(CT_OCTETS), &parts), "another type");
        differs(&drawn, &mini(&rows(CT_PAGE), &parts), "the type of a page");
    }
}

#[test]
fn a_part_typed_as_the_drawing_is_only_a_property_part_when_a_relationship_says_so_and_the_type_is_its_kind() {
    let rows = |content_type: &str| format!("{XML_DEFAULT}{RELS_DEFAULT}{}", over("/docProps/app.xml", content_type));
    let (ext, core) =
        (root_rels(&format!("{OD}extended-properties"), "docProps/app.xml"), root_rels(&format!("{OD}custom-properties"), "docProps/app.xml"));
    let with_rels = |rels: &str, content_type: &str| mini(&rows(content_type), &[("_rels/.rels", rels), ("docProps/app.xml", "<Properties/>")]);
    // The right relationship and the right type.
    same(&with_rels(&ext, CT_DRAWING), &with_rels(&ext, CT_APP), "the relationship of its kind");
    // The part reached as custom properties holds the custom type, not the extended one.
    differs(&with_rels(&core, CT_DRAWING), &with_rels(&core, CT_APP), "the type of another kind");
    same(&with_rels(&core, CT_DRAWING), &with_rels(&core, CT_CUSTOM), "the type of its own kind");
    // No relationship at all: a part named `app.xml` is not a property part by its name.
    let none = mini(&rows(CT_DRAWING), &[("docProps/app.xml", "<Properties/>")]);
    let own = mini(&rows(CT_APP), &[("docProps/app.xml", "<Properties/>")]);
    differs(&none, &own, "no relationship");
    // A relationship to another part.
    let elsewhere = root_rels(&format!("{OD}extended-properties"), "docProps/other.xml");
    differs(&with_rels(&elsewhere, CT_DRAWING), &with_rels(&elsewhere, CT_APP), "a relationship to another part");
    // A page part typed as the drawing is a page typed wrongly.
    let page = |content_type: &str| {
        mini(&format!("{XML_DEFAULT}{}", over("/visio/pages/page1.xml", content_type)), &[("visio/pages/page1.xml", "<PageContents/>")])
    };
    differs(&page(CT_DRAWING), &page(CT_PAGE), "a page");
}

#[test]
fn an_override_for_a_relationships_part_and_the_default_for_rels_are_the_same() {
    let parts = [("_rels/.rels", "<Relationships/>"), ("x/_rels/a.xml.rels", "<Relationships/>"), ("x/a.xml", "<a/>")];
    let overridden = mini(&format!("{XML_DEFAULT}{}{}", over("/_rels/.rels", CT_RELS), over("/x/_rels/a.xml.rels", CT_RELS)), &parts);
    let standard = mini(&format!("{XML_DEFAULT}{RELS_DEFAULT}"), &parts);
    same(&overridden, &standard, "the Default for rels");
    // The package that has both says no more than the standard one.
    same(&mini(&format!("{XML_DEFAULT}{RELS_DEFAULT}{}", over("/x/_rels/a.xml.rels", CT_RELS)), &parts), &standard, "both");
    // One of them overridden and the other by the `Default`, in one package.
    same(&mini(&format!("{XML_DEFAULT}{RELS_DEFAULT}{}", over("/_rels/.rels", CT_RELS)), &parts), &standard, "one of two");
    // A relationships part typed by its `Override` against none, or against another type.
    let untyped = mini(XML_DEFAULT, &parts);
    same(&overridden, &untyped, "a part that had none");
    // Controls: the `Default` for `rels` gives another type, or the `Override` does.
    differs(&overridden, &mini(&format!("{XML_DEFAULT}{}", default_for("rels", "application/xml")), &parts), "the Default says another type");
    differs(
        &mini(&format!("{XML_DEFAULT}{}{}", over("/_rels/.rels", "application/xml"), over("/x/_rels/a.xml.rels", CT_RELS)), &parts),
        &standard,
        "an Override says another type",
    );
    // A `Default` for `rels` that no part uses is still a row.
    differs(
        &mini(XML_DEFAULT, &[("x/a.xml", "<a/>")]),
        &mini(&format!("{XML_DEFAULT}{RELS_DEFAULT}"), &[("x/a.xml", "<a/>")]),
        "a Default for no part",
    );
    // An `Override` for a part that is no relationships part is not the Default.
    differs(
        &mini(&format!("{XML_DEFAULT}{}", over("/x/a.xml", CT_RELS)), &parts),
        &mini(&format!("{XML_DEFAULT}{RELS_DEFAULT}"), &parts),
        "a part that is no relationships part",
    );
}

// ------------------------------------------------------------------------------------------------------------ F3: a relationship row implies

const MASTER_ROW: &str = "../masters/master1.xml";

fn master_type() -> String {
    format!("{VIS}master")
}

fn page_type() -> String {
    format!("{VIS}page")
}

const INSTANCE: &str = r#"<Shape ID="1" Master="2" Type="Shape"><Cell N="PinX" V="1"/></Shape>"#;
const MASTER_SHAPE: &str = r#"<Shape ID="5" Type="Shape"><Cell N="Width" V="1"/></Shape>"#;

/// A drawing of masters `Box` (ID 2, `master1.xml`) and `Other` (ID 3, `master2.xml`), and one page that holds `shapes` and has the relationships `rels`.
fn drawing(shapes: &str, rels: &[(&str, String, &str)]) -> Vec<u8> {
    let mut page = PageSpec::new(1, "P").shapes(shapes);
    for (id, ty, target) in rels {
        page = page.rel(id, ty, target);
    }
    VsdxBuilder::new()
        .master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE))
        .master(MasterSpec::new(3, "Other").shapes(MASTER_SHAPE))
        .page(page)
        .build()
}

#[test]
fn a_row_to_the_master_the_shapes_use_is_the_same_as_none() {
    let with = drawing(INSTANCE, &[("rId5", master_type(), MASTER_ROW)]);
    let without = drawing(INSTANCE, &[]);
    same(&with, &without, "a page with no relationships part");
    // With another row beside it in both: a picture's, say.
    let image = ("rId1", format!("{OD}image"), "../media/image1.png");
    same(
        &drawing(INSTANCE, &[image.clone(), ("rId5", master_type(), MASTER_ROW)]),
        &drawing(INSTANCE, std::slice::from_ref(&image)),
        "a relationships part that holds another row",
    );
    // A shape inside a group that uses it.
    let nested = r#"<Shape ID="1" Type="Group"><Shapes><Shape ID="2" Master="2" Type="Shape"/></Shapes></Shape>"#;
    same(&drawing(nested, &[("rId5", master_type(), MASTER_ROW)]), &drawing(nested, &[]), "a nested shape");
    // Two masters used, one row each, either missing.
    let both = r#"<Shape ID="1" Master="2" Type="Shape"/><Shape ID="2" Master="3" Type="Shape"/>"#;
    let rows = [("rId5", master_type(), MASTER_ROW), ("rId6", master_type(), "../masters/master2.xml")];
    same(&drawing(both, &rows), &drawing(both, &[]), "two rows");
    same(&drawing(both, &rows), &drawing(both, &rows[..1]), "one of two");
    // A row that says its target another way.
    same(&drawing(INSTANCE, &[("rId5", master_type(), "/visio/masters/master1.xml")]), &drawing(INSTANCE, &[]), "an absolute target");
}

#[test]
fn a_row_that_the_shapes_do_not_imply_is_still_a_difference() {
    let without = drawing(INSTANCE, &[]);
    // A row to a master no shape uses, and the other package does not have.
    differs(&drawing(INSTANCE, &[("rId5", master_type(), "../masters/master2.xml")]), &without, "a master no shape uses");
    // The same Id with another target: the row differs.
    let d = differs(
        &drawing(INSTANCE, &[("rId5", master_type(), MASTER_ROW)]),
        &drawing(INSTANCE, &[("rId5", master_type(), "../masters/master2.xml")]),
        "another target",
    );
    assert!(d.shown.iter().any(|x| x.found == Found::Value), "{:?}", d.shown);
    // Another Id for the same row: both packages say it, and not alike.
    differs(&drawing(INSTANCE, &[("rId5", master_type(), MASTER_ROW)]), &drawing(INSTANCE, &[("rId6", master_type(), MASTER_ROW)]), "another Id");
    // A row of another type to the part of a master the shapes use is no master row.
    differs(&drawing(INSTANCE, &[("rId5", format!("{OD}image"), MASTER_ROW)]), &without, "another type");
    differs(&drawing(INSTANCE, &[("rId5", format!("{VIS}masters"), MASTER_ROW)]), &without, "a longer type");
    differs(&drawing(INSTANCE, &[("rId5", format!("{VIS}Master"), MASTER_ROW)]), &without, "a type in another case");
    // The master the shapes use is the other one.
    let other = r#"<Shape ID="1" Master="3" Type="Shape"/>"#;
    differs(&drawing(other, &[("rId5", master_type(), MASTER_ROW)]), &drawing(other, &[]), "the row names the master the shapes do not use");
    // No shape at all.
    differs(&drawing("", &[("rId5", master_type(), MASTER_ROW)]), &drawing("", &[]), "no shapes");
    // An external row is a link, and says nothing the shapes say.
    let external = VsdxBuilder::new()
        .master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE))
        .page(PageSpec::new(1, "P").shapes(INSTANCE))
        .with(diacraft_testkit::package::Pkg::new().rel_external(
            concat!("/vi", "sio", "/pages/page1.xml"),
            "rId5",
            &master_type(),
            "http://example.org/master1.xml",
        ))
        .build();
    differs(&external, &drawing(INSTANCE, &[]), "an external row");
    // A row of a master's part is not a page's.
    let mut master_rel = VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE).rel("rId5", &master_type(), "master2.xml"));
    master_rel = master_rel.master(MasterSpec::new(3, "Other").shapes(MASTER_SHAPE));
    differs(
        &master_rel.build(),
        &VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE)).master(MasterSpec::new(3, "Other").shapes(MASTER_SHAPE)).build(),
        "a master's own row",
    );
}

/// A drawing of page 1, which says `back`, and page 2, which is a background page; page 1 has the relationships `rels`.
fn with_background(back: &str, rels: &[(&str, String, &str)]) -> Vec<u8> {
    let mut page = PageSpec::new(1, "P").attrs(back).shapes(r#"<Shape ID="1" Type="Shape"/>"#);
    for (id, ty, target) in rels {
        page = page.rel(id, ty, target);
    }
    VsdxBuilder::new()
        .page(page)
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(r#"<Shape ID="1" Type="Shape"/>"#))
        .page(PageSpec::new(3, "Other").shapes(r#"<Shape ID="1" Type="Shape"/>"#))
        .build()
}

#[test]
fn a_row_to_the_background_page_is_the_same_as_none() {
    let back = r#" BackPage="2""#;
    same(&with_background(back, &[("rId9", page_type(), "page2.xml")]), &with_background(back, &[]), "the page's BackPage");
    // Not a page row for the page itself or for another: the rows below are controls.
    differs(&with_background(back, &[("rId9", page_type(), "page3.xml")]), &with_background(back, &[]), "a page that is not the background page");
    differs(&with_background(back, &[("rId9", page_type(), "page1.xml")]), &with_background(back, &[]), "the page itself");
    // No `BackPage`: R6 keeps such a row, and says nothing of why.
    differs(&with_background("", &[("rId9", page_type(), "page2.xml")]), &with_background("", &[]), "a row and no BackPage");
    // A `BackPage` that names a page the drawing does not have.
    differs(
        &with_background(r#" BackPage="9""#, &[("rId9", page_type(), "page2.xml")]),
        &with_background(r#" BackPage="9""#, &[]),
        "a BackPage that is no page",
    );
    // A row of another type to the background page's part.
    differs(&with_background(back, &[("rId9", format!("{VIS}pages"), "page2.xml")]), &with_background(back, &[]), "another type");
    // Both a master row and the page row, in one part, are both taken as they are implied.
    let both = [("rId9", page_type(), "page2.xml"), ("rId8", master_type(), MASTER_ROW)];
    differs(&with_background(back, &both), &with_background(back, &[]), "a master row for a master no shape uses");
}

#[test]
fn what_one_package_has_that_the_other_lacks_is_taken_out_whichever_package_it_is() {
    let with = drawing(INSTANCE, &[("rId5", master_type(), MASTER_ROW)]);
    let without = drawing(INSTANCE, &[]);
    // Each is the same as the other, and as itself: the count is symmetric (`diff`), and so is the verdict.
    same(&without, &with, "the other way");
    // The row sits beside one the other package lacks and is not implied: that row is the difference, and one only.
    let rows = [("rId5", master_type(), MASTER_ROW), ("rId6", master_type(), "../masters/master2.xml")];
    let d = differs(&drawing(INSTANCE, &rows), &without, "an implied row and one that is not");
    assert_eq!(d.count, 1, "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ what none of the rules lets go

#[test]
fn a_dropped_part_whose_root_is_an_empty_element_is_still_a_difference() {
    // The control (the fuzz run of the full gate on 758acd4f): a part that holds nothing but an empty root is a part, unless it is a relationships
    // part with no row. Neither of the rules of is a licence for any other empty part.
    let base = mini(XML_DEFAULT, &[("x/a.xml", "<a/>")]);
    for (name, body) in [
        ("x/unknown.xml", "<Empty/>"),
        ("x/unknown.xml", r#"<Unknown xmlns="urn:x"/>"#),
        ("x/_rels/a.xml.rels", "<Empty/>"),
        ("x/_rels/a.xml.rels", r#"<Relationships xmlns="urn:not-opc"/>"#),
        ("x/thing.dat", "<Empty/>"),
    ] {
        let with = mini(XML_DEFAULT, &[("x/a.xml", "<a/>"), (name, body)]);
        let d = differs(&with, &base, &format!("{name} holding {body}"));
        assert!(d.shown.iter().any(|x| x.found == Found::MissingPart || x.found == Found::ExtraPart), "{name}: {:?}", d.shown);
    }
    // The same part, empty, in a full drawing: a part of the drawing's package that no list names.
    let drawing = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(r#"<Shape ID="1" Type="Shape"/>"#));
    let with = drawing.clone().with(diacraft_testkit::package::Pkg::new().part("/extra/empty.xml", "application/xml", "<Empty/>")).build();
    differs(&with, &drawing.build(), "an empty part of a drawing");
}

#[test]
fn a_relationships_part_that_keeps_a_row_the_page_does_not_imply_is_still_a_part() {
    // A page's part of relationships that holds one implied row and one that is not: the implied one goes, the other is a part the other package lacks.
    let rows = [("rId5", master_type(), MASTER_ROW), ("rId6", format!("{OD}image"), "../media/image1.png")];
    let d = differs(&drawing(INSTANCE, &rows), &drawing(INSTANCE, &[]), "an implied row beside another");
    assert_eq!(d.count, 1, "{:?}", d.shown);
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ what it costs, and what it will not read

/// A drawing of `pages` pages that each use master 2, each page's relationships part holding the row to it when `rows`.
fn many_pages(pages: u32, rows: bool) -> Vec<u8> {
    let mut builder = VsdxBuilder::new().master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE));
    for id in 1..=pages {
        let mut page = PageSpec::new(id, &format!("P{id}")).shapes(INSTANCE);
        if rows {
            page = page.rel("rId5", &master_type(), MASTER_ROW);
        }
        builder = builder.page(page);
    }
    builder.build()
}

#[test]
fn the_rows_of_many_pages_cost_a_few_steps_each() {
    use diacraft_container::Work;
    let steps = |pages: u32| {
        let (with, without) = (many_pages(pages, true), many_pages(pages, false));
        let mut work = Work::default();
        let d = diacraft_vsdx::compare_content_counted(&with, &without, &Limits::default(), &mut work).unwrap();
        assert!(d.same(), "{pages} pages: {:?}", d.shown);
        work.canon_steps
    };
    let (few, many) = (steps(50), steps(400));
    // Eight times the pages, no more than ten times the steps: the index is read once, and each page's part once.
    assert!(many <= few * 10, "{few} steps for 50 pages and {many} for 400");
}

#[test]
fn a_relationships_part_past_a_mebibyte_is_compared_as_it_is() {
    let rows = |pad: usize| {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="{}" Target="{MASTER_ROW}"/><!--{}--></Relationships>"#,
            master_type(),
            "x".repeat(pad)
        )
    };
    let part = concat!("/vi", "sio", "/pages/_rels/page1.xml.rels");
    let build = |pad: usize| {
        VsdxBuilder::new()
            .master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE))
            .master(MasterSpec::new(3, "Other").shapes(MASTER_SHAPE))
            .page(PageSpec::new(1, "P").shapes(INSTANCE))
            .with(diacraft_testkit::package::Pkg::new().part_without_override(part, rows(pad)))
            .build()
    };
    let without = drawing(INSTANCE, &[]);
    // Under the size it is read, and over it is a part the other package lacks.
    same(&build(100), &without, "a small part");
    let d = differs(&build((1 << 20) + 1), &without, "a part past a mebibyte");
    assert_eq!(d.shown.first().map(|x| x.found), Some(Found::MissingPart), "{:?}", d.shown);
}

// ------------------------------------------------------------------------------------------------------------ hostile input

/// A drawing with what the three rules read: two masters, a page that uses one and has a background page, the rows between them when `rows`, and a property part typed as the drawing.
fn rich(rows: bool) -> Vec<u8> {
    let mut page = PageSpec::new(1, "P").attrs(r#" BackPage="2""#).shapes(INSTANCE);
    if rows {
        page = page.rel("rId5", &master_type(), MASTER_ROW).rel("rId6", &page_type(), "page2.xml");
    }
    let geometry =
        r#"<Shape ID="9" Type="Shape"><Section N="Geometry" IX="0"><Row T="MoveTo"><Cell N="X" V="0"/></Row><Row T="LineTo"/></Section></Shape>"#;
    VsdxBuilder::new()
        .master(MasterSpec::new(2, "Box").shapes(MASTER_SHAPE))
        .master(MasterSpec::new(3, "Other").shapes(MASTER_SHAPE))
        .page(page)
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(geometry))
        .build()
}

/// `package` written again with right checksums, each of its parts (and the content types stream) given to `change`.
fn repacked(package: &[u8], mut change: impl FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let read = diacraft_opc::Package::read(package, &diacraft_opc::Limits::default()).unwrap();
    let mut types = read.content_types_bytes().to_vec();
    change("[Content_Types].xml", &mut types);
    let mut items = vec![ZipItem::stored("[Content_Types].xml", &types)];
    for name in read.parts() {
        let mut bytes = read.part(name).unwrap().to_vec();
        change(name, &mut bytes);
        items.push(ZipItem::stored(name.trim_start_matches('/'), &bytes));
    }
    zip(&items)
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 400, failure_persistence: None, ..proptest::prelude::ProptestConfig::default() })]

    /// The parts the rules read (the content types, the relationships, the indexes, the pages), mutated and written again with right checksums: the check answers or refuses in
    /// words, its count is symmetric, a package is the same as itself, and it answers the same twice. No panic, no hang.
    #[test]
    fn mutated_parts_the_rules_read_never_panic_or_hang_and_the_count_is_symmetric(seed in proptest::prelude::any::<u64>(), rows in proptest::prelude::any::<bool>()) {
        let mut g = crate::fuzz::Gen(seed | 1);
        let base = rich(true);
        let names: Vec<String> = diacraft_opc::Package::read(&base, &diacraft_opc::Limits::default()).unwrap().parts().map(str::to_owned).collect();
        let mut chosen = vec![names[g.below(names.len())].clone()];
        if g.below(2) == 0 {
            chosen.push("[Content_Types].xml".to_owned());
        }
        let mutated = repacked(&base, |name, bytes| {
            if chosen.iter().any(|c| c == name) {
                let mut inner = crate::fuzz::Gen(g.next() | 1);
                crate::fuzz::mutate(&mut inner, bytes);
            }
        });
        let other = rich(rows);
        let started = std::time::Instant::now();
        let limits = Limits::default();
        let (ab, ba) = (compare_content(&mutated, &other, &limits), compare_content(&other, &mutated, &limits));
        proptest::prop_assert!(started.elapsed() < std::time::Duration::from_secs(60), "a comparison took {:?}", started.elapsed());
        match (ab, ba) {
            (Ok(x), Ok(y)) => {
                proptest::prop_assert_eq!(x.count, y.count);
                proptest::prop_assert_eq!(compare_content(&mutated, &other, &limits).unwrap(), x);
            }
            (Err(x), Err(y)) => {
                proptest::prop_assert_eq!(x.class(), y.class());
                proptest::prop_assert!(!x.to_string().chars().any(char::is_control), "{x:?}");
            }
            // A package that does not read is refused whichever side it is on, and the other is the same package.
            (x, y) => proptest::prop_assert!(false, "{} then {}", x.is_ok(), y.is_ok()),
        }
        if let Ok(d) = compare_content(&mutated, &mutated, &limits) {
            proptest::prop_assert!(d.same(), "{:?}", d.shown);
        }
    }
}
