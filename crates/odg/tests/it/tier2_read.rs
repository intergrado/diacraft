//! Tier 2 on a shape's own automatic style, read (the proposal of for the names).
//!
//! Every input is written by hand from the specs: a rectangle and the automatic styles that carry DiaCraft's attributes in their
//! `style:graphic-properties` (the one place a LibreOffice save keeps them).

use diacraft_doc::{CellAddr, RowKey};

use crate::drawing::{Drawing, cell, formula, only, shape, text};

/// An automatic graphic style `name` whose graphic properties carry `attrs`.
fn style(name: &str, attrs: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#)
}

/// A rectangle 2 cm by 1 cm at (1 cm, 1 cm) on the style `name`.
fn rect(name: &str, extra: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{name}" {extra} svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#)
}

/// `xml` as the value of an attribute: the quotes escaped.
fn value(xml: &str) -> String {
    xml.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

#[test]
fn a_shape_whose_style_carries_cells_and_a_section_reads_them_over_what_its_markup_gave() {
    let cells = value(r#"<Sheet><Cell N="Note" V="kept" U="X"/><Cell N="Width" V="0.7874015748031495" F="Height*2"/></Sheet>"#);
    let section = value(r#"<Sheet><Section N="User"><Row N="a" LocalName="A"><Cell N="Value" V="3" F="Width*2"/></Row></Section></Sheet>"#);
    let attrs = format!(r#"diacraft:version="1" diacraft:id="7" diacraft:cells="{cells}" diacraft:s-User="{section}""#);
    let (doc, report) = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", "")).read();
    let s = only(&doc);
    assert_eq!(s.id, 7, "the shape takes its DiaCraft id");
    assert_eq!(text(cell(s, "Note")), "kept");
    // A cell that the native markup also gave is replaced by the carried one (its value is the markup's own width: a carried size that is not is stale, tier 3).
    assert_eq!(formula(cell(s, "Width")), Some("Height*2"));
    let row = s.sheet.section(&diacraft_doc::SectionKey::new("User", None)).unwrap().row(&RowKey::name("a")).unwrap();
    assert_eq!(row.local_name.as_deref(), Some("A"));
    assert_eq!(formula(row.cells.get("Value")), Some("Width*2"));
    // The other cells are what the native markup gave: the frame is still read from `svg:*`.
    assert!(cell(s, "PinX").is_some() && cell(s, "LocPinX").is_some());
    assert_eq!(report.warning_counts.get("diacraft_tier2_unreadable"), None);
    let _ = CellAddr::direct("x");
}

/// SECNAME: an earlier build's tier 2 that names a section by the word a formula uses for it (`Prop`, `Controls`) is **no alias** of the model's `Property` and `Control`:
/// no file of ours is public, and a name read as another would let two sections answer to one reference. It is read as the extension section it says it is and kept as
/// it is, and the model has no `Property` or `Control` from it.
#[test]
fn a_section_named_by_the_word_a_formula_uses_is_an_extension_section_and_no_alias_of_the_models() {
    let section = |name: &str| value(&format!(r#"<Sheet><Section N="{name}"><Row N="Cost"><Cell N="Value" V="3"/></Row></Section></Sheet>"#));
    let attrs =
        format!(r#"diacraft:version="1" diacraft:id="7" diacraft:s-Prop="{}" diacraft:s-Controls="{}""#, section("Prop"), section("Controls"));
    let (doc, report) = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", "")).read();
    let s = only(&doc);
    let held = |name: &str| s.sheet.section(&diacraft_doc::SectionKey::new(name, None)).map(|x| x.rows.len());
    assert_eq!((held("Prop"), held("Controls")), (Some(1), Some(1)), "read as the sections they say they are");
    assert_eq!((held("Property"), held("Control")), (None, None), "and not as the model's");
    assert_eq!(report.warning_counts.get("diacraft_tier2_unreadable"), None);
}

#[test]
fn a_cell_the_style_says_is_gone_is_taken_out_and_so_is_a_section_and_a_name() {
    let gone = "c.LocPinX s.Connection._ name name-u";
    let attrs = format!(r#"diacraft:version="1" diacraft:id="3" diacraft:gone="{gone}""#);
    let (plain, _) = Drawing::a4().page(&rect("none", r#"draw:name="Box""#)).read();
    assert!(cell(only(&plain), "LocPinX").is_some());
    assert!(only(&plain).sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_some(), "the rebuild has the four glue rows");
    assert_eq!(only(&plain).name.as_deref(), Some("Box"));
    let (doc, _) = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", r#"draw:name="Box""#)).read();
    let s = only(&doc);
    assert!(cell(s, "LocPinX").is_none());
    assert!(s.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_none());
    assert!(s.name.is_none() && s.name_u.is_none());
}

#[test]
fn the_names_a_dropped_shape_needs_are_carried_and_nothing_else_of_the_shape_changes() {
    let attrs =
        r#"diacraft:version="1" diacraft:id="4" diacraft:name="Process" diacraft:name-u="Process.4" diacraft:master="2" diacraft:master-shape="5""#;
    let (doc, _) = Drawing::a4().auto(&style("gr1", attrs)).page(&rect("gr1", r#"draw:name="Box""#)).read();
    let s = only(&doc);
    assert_eq!((s.name.as_deref(), s.name_u.as_deref()), (Some("Process"), Some("Process.4")));
    assert_eq!((s.master, s.master_shape), (Some(2), Some(5)));
    assert_eq!(s.id, 4);
}

#[test]
fn a_text_that_is_carried_is_the_shapes_text_with_its_markers() {
    let text_item = value(r#"<Text><cp IX="1"/>ab<pp IX="2"/>c<fld IX="0">3</fld><!--k--></Text>"#);
    let attrs = format!(r#"diacraft:version="1" diacraft:id="4" diacraft:text="{text_item}""#);
    let (doc, _) = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", "")).read();
    let parts = &only(&doc).text.as_ref().unwrap().parts;
    use diacraft_doc::TextPart::{Chars, Cp, Fld, Pp, Raw};
    assert!(matches!(parts.as_slice(), [Cp(1), Chars(a), Pp(2), Chars(b), Fld { ix: 0, .. }, Raw(_)] if &**a == "ab" && &**b == "c"), "{parts:?}");
}

#[test]
fn what_does_not_read_is_left_out_with_a_warning_and_the_shape_reads_from_its_markup() {
    let attrs = concat!(
        r#"diacraft:version="1" diacraft:id="5" diacraft:cells="not xml &lt;" diacraft:s-User="&lt;Other/&gt;" "#,
        r#"diacraft:s-Property="&lt;Sheet&gt;&lt;Section N=&quot;Hyperlink&quot;/&gt;&lt;/Sheet&gt;" diacraft:text="&lt;Cells/&gt;" "#,
        r#"diacraft:master="x" diacraft:gone="zz c.a_ s.User" diacraft:future="1" diacraft:s-="1""#
    );
    let (doc, report) = Drawing::a4().auto(&style("gr1", attrs)).page(&rect("gr1", "")).read();
    let s = only(&doc);
    assert_eq!(s.id, 5);
    assert!(cell(s, "Width").is_some());
    assert_eq!(s.master, None, "a number that is not one is no link");
    // The cells, the `User` section, the `Property` section that holds another, the text, and three tokens of `gone`.
    assert!(report.warning_counts.get("diacraft_tier2_unreadable").copied().unwrap_or(0) >= 7, "{:?}", report.warning_counts);
}

#[test]
fn two_shapes_that_carry_one_id_the_first_keeps_its_data_and_the_second_is_read_from_its_markup_alone() {
    let cells = value(r#"<Sheet><Cell N="Note" V="mine"/></Sheet>"#);
    let attrs = format!(r#"diacraft:version="1" diacraft:id="7" diacraft:cells="{cells}""#);
    let both = format!("{}{}", rect("gr1", ""), rect("gr2", ""));
    let (doc, report) = Drawing::a4().auto(&format!("{}{}", style("gr1", &attrs), style("gr2", &attrs))).page(&both).read();
    let (first, second) = (shape(&doc, 0, 0), shape(&doc, 0, 1));
    assert_eq!(first.id, 7);
    assert_eq!(text(cell(first, "Note")), "mine");
    assert_ne!(second.id, 7, "the later shape is given the next number nothing holds");
    assert!(cell(second, "Note").is_none(), "and none of the data");
    assert_eq!(report.warning_counts.get("diacraft_id_repeated"), Some(&1));
    // The high-water mark counts the id, whoever holds it.
    assert!(report.index.high_water(0).unwrap() >= 7);
}

/// Whether the shape of the page that has `id` was read as one that carries tier 2.
fn carries(report: &diacraft_odg::ReadReport, id: u32) -> bool {
    report.index.page(0).is_some_and(|p| p.has_tier2(id))
}

/// A shape carries tier 2 when its own automatic style holds an attribute of the namespace that the writer owns, and in no other case: not a style with no
/// attribute of the namespace, nor one with the version alone, nor one that holds names this build does not know, nor a style that is a common one, and not a
/// shape that is a copy.
#[test]
fn a_shape_carries_tier_2_only_when_its_own_automatic_style_holds_an_owned_attribute() {
    let cells = value(r#"<Sheet><Cell N="Note" V="mine"/></Sheet>"#);
    let owned = format!(r#"diacraft:version="1" diacraft:id="7" diacraft:cells="{cells}""#);
    let (_, report) = Drawing::a4().auto(&style("gr1", &owned)).page(&rect("gr1", "")).read();
    assert!(carries(&report, 7), "an owned attribute");
    // The version alone, an id alone, a style with none, and names that are not the writer's: no tier 2, and the unknown names are kept and counted.
    for (attrs, kept) in [
        ("", 0),
        (r#"diacraft:version="1""#, 0),
        (r#"diacraft:version="1" diacraft:id="7""#, 0),
        (r#"diacraft:version="1" diacraft:id="7" diacraft:future="1" diacraft:fp-9="x" diacraft:filled="1""#, 3),
    ] {
        let (doc, report) = Drawing::a4().auto(&style("gr1", attrs)).page(&rect("gr1", "")).read();
        assert!(!carries(&report, only(&doc).id), "no tier 2 in {attrs:?}");
        assert_eq!(report.kept.get("diacraft_attribute").copied().unwrap_or(0), kept, "{attrs:?}");
    }
    // A common style is no shape's own: its attributes are not read as the shape's.
    let on_common = format!(r#"diacraft:version="1" diacraft:id="9" diacraft:name="Common" diacraft:cells="{cells}""#);
    let (doc, report) = Drawing::a4().common(&style("c1", &on_common)).page(&rect("c1", r#"draw:name="Box""#)).read();
    let s = only(&doc);
    assert_ne!(s.id, 9, "the id of a common style is not the shape's");
    assert!(cell(s, "Note").is_none() && s.name.as_deref() == Some("Box"), "{:?}", s.name);
    assert!(!carries(&report, s.id));
    // A copy is read from its markup alone and does not carry it.
    let both = format!("{}{}", rect("gr1", ""), rect("gr2", ""));
    let (doc, report) = Drawing::a4().auto(&format!("{}{}", style("gr1", &owned), style("gr2", &owned))).page(&both).read();
    assert!(carries(&report, 7) && !carries(&report, shape(&doc, 0, 1).id));
}

/// A style holds at most as many attributes of the extension as a sheet may hold sections twice (a section, and the fingerprint of its outline) and thirty-two: the ones past it are kept as the file has them and
/// counted, and none is read.
#[test]
fn a_style_is_read_with_at_most_the_attributes_the_cap_allows_and_the_rest_are_kept() {
    let most = 2 * diacraft_doc::MAX_SECTIONS_PER_SHEET + 32;
    let many: String = (0..most + 3).map(|k| format!(r#"diacraft:s-a{k}="x" "#)).collect();
    let (_, report) = Drawing::a4().auto(&style("gr1", &many)).page(&rect("gr1", "")).read();
    // Each one read is a value that is no section, one warning each; the three past the cap are kept.
    assert_eq!(report.warning_counts.get("diacraft_tier2_unreadable").copied(), Some(most as u64), "{:?}", report.warning_counts);
    assert_eq!(report.kept.get("diacraft_attribute").copied(), Some(3));
}

/// The bytes of a style's attributes of the extension are charged by their sum, the length of the name and of the value: a value of 1,280 bytes costs 22
/// items more than a value of none (1,440 bytes of text at nine eighths, with the style's few other bytes in the sum), a product would cost four times that.
#[test]
fn the_bytes_of_the_attributes_of_a_style_are_charged_as_the_sum_of_their_names_and_values() {
    let items = |value: &str| {
        let attrs = format!(r#"diacraft:version="1" diacraft:id="7" diacraft:name="{value}""#);
        let (_, report) = Drawing::a4().auto(&style("gr1", &attrs)).page(&rect("gr1", "")).read();
        report.work.items
    };
    let (empty, long) = (items(""), items(&"x".repeat(1280)));
    // (DiaCraft's own attributes are no part of what an automatic style is kept as.)
    assert_eq!(long - empty, 22, "1,280 bytes of a name's value are 1,440 bytes of text, 22 items of 64 bytes beside the style's other bytes");
}

/// **Of the review.** A file of carries `PinX` with a place `LocPinX` of `Width*0.25` (the shape was 4 cm wide, its left edge at 2 cm: the pin at 3 cm, the place
/// 1 cm), and LibreOffice widened the shape to 8 cm with the left edge kept. The pin is the left edge plus the place the formula gives now (0.25 of 8 cm: 4 cm), not 3 cm,
/// which would put the left edge at 1 cm.
#[test]
fn a_carried_pin_of_a_shape_widened_in_another_program_is_the_left_edge_plus_the_place_now() {
    let cells =
        value(r#"<Sheet><Cell N="PinX" V="1.1811023622047243" U="IN"/><Cell N="LocPinX" V="0.39370078740157477" U="IN" F="Width*0.25"/></Sheet>"#);
    let attrs = format!(r#"diacraft:version="1" diacraft:id="7" diacraft:cells="{cells}""#);
    let shape = |width: &str| {
        format!(r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="1cm" svg:width="{width}" svg:height="1cm"><text:p/></draw:rect>"#)
    };
    let inches = |cm: f64| cm / 2.54;
    let pin = |width: &str| {
        let (doc, _) = Drawing::a4().auto(&style("gr1", &attrs)).page(&shape(width)).read();
        let s = only(&doc);
        (
            cell(s, "PinX").and_then(|c| c.value.as_ref()).and_then(diacraft_doc::Value::number).unwrap(),
            formula(cell(s, "LocPinX")).map(str::to_owned),
        )
    };
    let (widened, place) = pin("8cm");
    assert!((widened - inches(4.0)).abs() < 1e-3, "the pin is at 4 cm ({}), and not at 3 cm: {widened}", inches(4.0));
    assert_eq!(place.as_deref(), Some("Width*0.25"), "the place is the formula's");
    // The width as it was: the pin stands where the file had it.
    let (same, _) = pin("4cm");
    assert!((same - inches(3.0)).abs() < 1e-3, "{same}");
}
