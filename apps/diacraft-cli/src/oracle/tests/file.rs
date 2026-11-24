//! One file's report: the hash, the category, the names, the refusals, the sizes that confirm the caps.

use diacraft_calc::Native;
use diacraft_testkit::package::{
    CT_DRAWING, CT_PAGE, CT_PAGES, MasterSpec, NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder, ZipItem, cell, shape, zip,
};
use diacraft_vsdx::Limits;
use serde_json::Value;

use super::super::file::{SCHEMA, is_colour, kind_of, report};
use super::super::refusal::zip_detail;
use super::super::sha256::sha256_hex;
use super::{native_control, one_page, one_shape, reference_app_props, run};

// ------------------------------------------------------------------------------------------------------------------------ hash

#[test]
fn the_report_names_the_file_by_its_sha256_and_size() {
    let bytes = one_page(&shape(1, ""));
    let r = run(&bytes);
    assert_eq!(r.sha256, sha256_hex(&bytes));
    assert_eq!(r.to_json()["sha256"], Value::String(sha256_hex(&bytes)));
    assert_eq!(r.to_json()["bytes"], bytes.len());
    assert_eq!(r.to_json()["schema"], SCHEMA);
    assert_eq!(SCHEMA, 7);
}

// -------------------------------------------------------------------------------------------------------------------- category

#[test]
fn the_category_follows_the_censuss_terms() {
    // A file the reference app wrote, with a cell that holds a formula and a cached value: oracle_usable.
    let r = run(&one_shape(r#"<Cell N="Width" V="4" F="Height*2"/>"#));
    let c = r.category.clone().unwrap();
    assert_eq!((c.writer, c.oracle_usable, c.routing_usable), ("reference_app", true, false));
    assert_eq!(c.variant, Some(("drawing", false)));
    let category = &r.to_json()["category"];
    assert_eq!(category["package_traits"], "not_seen");
    assert_eq!(category["variant"], "drawing");
    // Another writer: not usable, however many formulas.
    let other = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, r#"<Cell N="Width" V="4" F="Height*2"/>"#))).build();
    let c = run(&other).category.unwrap();
    assert_eq!((c.writer, c.oracle_usable), ("other", false));
    // No app.xml at all: unknown.
    let bare =
        Pkg::new().part("/visio/document.xml", CT_DRAWING, "<VisioDocument/>").rel("/", "rId1", &format!("{VIS}document"), "visio/document.xml");
    assert_eq!(run(&bare.build()).category.unwrap().writer, "unknown");
}

#[test]
fn the_writer_is_the_reference_app_only_on_an_exact_match_of_its_name() {
    // The census: "an exact match for the product name, with or without the company prefix, so a tool whose name merely contains the word
    // does not count". No trimming, no folding of case (the review's M7).
    let app = |name: &str| {
        format!(
            "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>{name}</Application></Properties>"
        )
    };
    for (name, writer) in [
        ("Microsoft Visio", "reference_app"),
        ("Visio", "reference_app"),
        ("microsoft visio", "other"),
        ("VISIO", "other"),
        ("Microsoft Visio ", "other"),
        (" Visio", "other"),
        ("Visio Viewer", "other"),
        ("My Visio Tool", "other"),
    ] {
        let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, ""))).app(&app(name)).build();
        assert_eq!(run(&bytes).category.unwrap().writer, writer, "{name:?}");
    }
}

#[test]
fn a_formula_with_no_cached_value_or_a_value_that_is_not_one_does_not_make_a_file_oracle_usable() {
    // The census: an empty value, the themed placeholder and a cell carrying an error are not cached values.
    for cells in [
        r#"<Cell N="A" V="" F="1+1"/>"#,
        r#"<Cell N="A" V="Themed" F="THEMEVAL()"/>"#,
        r##"<Cell N="A" V="1" F="1/0" E="#DIV/0!"/>"##,
        r#"<Cell N="A" V="1" F="Inh"/>"#,
        r#"<Cell N="A" V="1"/>"#,
    ] {
        assert!(!run(&one_shape(cells)).category.unwrap().oracle_usable, "{cells}");
    }
}

#[test]
fn a_connector_glued_at_an_end_with_saved_geometry_makes_a_file_routing_usable() {
    let connector = r#"<Cell N="BeginX" V="1"/><Cell N="EndX" V="2"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row></Section>"#;
    let connect = r#"<Connect FromSheet="7" FromCell="BeginX" FromPart="9" ToSheet="5" ToCell="PinX" ToPart="3"/>"#;
    let page = PageSpec::new(1, "P").shapes(&(shape(5, "") + &shape(7, connector))).connects(connect);
    let r = run(&VsdxBuilder::new().page(page.clone()).app(reference_app_props()).build());
    assert!(r.category.unwrap().routing_usable);
    // Another writer's connector is not an oracle for routing.
    let r = run(&VsdxBuilder::new().page(page).build());
    let c = r.category.unwrap();
    assert_eq!((c.writer, c.routing_usable), ("other", false));
    // The same glue to a shape with no geometry is not.
    let page = PageSpec::new(1, "P").shapes(&(shape(5, "") + &shape(7, ""))).connects(connect);
    assert!(!run(&VsdxBuilder::new().page(page).app(reference_app_props()).build()).category.unwrap().routing_usable);
}

// ---------------------------------------------------------------------------------------------------------------------- names

#[test]
fn cells_and_functions_are_named_by_the_spec_s_lists_with_their_counts() {
    let cells = r#"<Cell N="Width" V="4" F="SQRT(16)+SQRT(4)"/><Cell N="Height" V="2"/><Cell N="PinX" V="1" F="Inh"/><Cell N="LocPinX" V="0" F="No Formula"/><Section N="Geometry" IX="0"><Row IX="1" T="MoveTo"><Cell N="X" V="1" F="Width*0.5"/></Row></Section>"#;
    let json = run(&one_shape(cells)).to_json().clone();
    let names = &json["names"];
    assert_eq!(names["cells"]["Width"]["formula"], 1);
    assert_eq!(names["cells"]["Height"]["value_only"], 1);
    assert_eq!(names["cells"]["PinX"]["inherited"], 1);
    assert_eq!(names["cells"]["LocPinX"]["no_formula"], 1);
    assert_eq!(names["cells"]["Geometry.X"]["formula"], 1);
    assert_eq!(names["functions"]["SQRT"], 2);
    assert_eq!(names["functions"]["_ADD"], 1);
    assert_eq!(names["functions"]["_MUL"], 1);
    // Five numbers on the shape, and the three flags of the root style the builder gives every document.
    assert_eq!(json["saved"]["value_kinds"]["number"], 8);
}

#[test]
fn a_name_the_spec_does_not_list_is_counted_and_never_named() {
    let cells = r#"<Cell N="MyOwnCell" V="1" F="MYFUNC(2)"/><Section N="User"><Row N="MyRow"><Cell N="Value" V="1"/><Cell N="MyOwnCellToo" V="2"/></Row></Section><Section N="MySection"><Row N="r"><Cell N="X" V="1"/></Row></Section>"#;
    let json = run(&one_shape(cells)).to_json().clone();
    let text = json.to_string();
    for secret in ["MyOwnCell", "MYFUNC", "MyRow", "MySection"] {
        assert!(!text.contains(secret), "{secret} crossed");
    }
    assert_eq!(json["names"]["cells_unlisted"]["formula"], 1);
    assert!(json["names"]["cells_unlisted"]["value_only"].as_u64().unwrap() >= 1);
    assert_eq!(json["names"]["functions_unlisted_calls"], 1);
    // A listed cell name in a section the file made up is named by the spec's name and the fixed word for such a section (three real cell names
    // sat in sections outside the 17 and were counted as unlisted: 608 cells); the section's own name never crosses.
    assert!(json["names"]["cells"].get("<other section>.X").is_some());
}

#[test]
fn error_unit_and_value_kinds_are_counted_by_their_listed_words() {
    let cells = r##"<Cell N="A" V="0" F="1/0" E="#DIV/0!" U="MM"/><Cell N="B" V="0" F="1/0" E="#DIV/0!" U="WEIRD"/><Cell N="C" V="1" F="x" E="#BOGUS!"/><Cell N="D" V="Themed"/><Cell N="E" V=""/><Cell N="F" V="#FF0000"/><Cell N="G" V="hello"/><Cell N="H" V="1.#INF"/>"##;
    let json = run(&one_shape(cells)).to_json().clone();
    assert_eq!(json["saved"]["errors"]["#DIV/0!"], 2);
    assert_eq!(json["saved"]["errors"]["other"], 1);
    assert_eq!(json["saved"]["units"]["MM"], 1);
    assert_eq!(json["saved"]["units"]["other"], 1);
    let kinds = &json["saved"]["value_kinds"];
    assert_eq!(
        (kinds["themed"].as_u64(), kinds["empty"].as_u64(), kinds["colour"].as_u64(), kinds["text"].as_u64(), kinds["infinite"].as_u64()),
        (Some(1), Some(1), Some(1), Some(1), Some(1))
    );
    assert!(!json.to_string().contains("BOGUS"));
}

#[test]
fn only_seven_characters_of_a_hash_and_six_hex_digits_are_a_colour() {
    // The review's I4: any `#hex` counted as a colour. A colour is `#` and exactly six hexadecimal digits.
    for yes in ["#FF0000", "#ffffff", "#0a1B2c", "#000000"] {
        assert!(is_colour(yes), "{yes}");
        assert_eq!(kind_of(yes), "colour", "{yes}");
    }
    for no in ["#FF000", "#FF00000", "#GGGGGG", "FF0000", "#FF 000", "#", "", "#12345", "#1234567", "##FF000", "#ff000é"] {
        assert!(!is_colour(no), "{no}");
        assert_eq!(kind_of(no), if no.is_empty() { "empty" } else { "text" }, "{no}");
    }
    let json = run(&one_shape(r##"<Cell N="A" V="#12345"/><Cell N="B" V="#GGGGGG"/><Cell N="C" V="#1234567"/><Cell N="D" V="#123456"/>"##))
        .to_json()
        .clone();
    assert_eq!(json["saved"]["value_kinds"]["colour"], 1);
    assert_eq!(json["saved"]["value_kinds"]["text"], 3);
}

#[test]
fn triggers_and_reserved_user_rows_are_counted_by_name() {
    let cells = r#"<Trigger N="RecalcColor"><RefBy T="Page" ID="0"/></Trigger><Trigger N="MyTrigger"/><Section N="User"><Row N="msvThemeColors"><Cell N="Value" V="1"/></Row><Row N="Mine"><Cell N="Value" V="1"/></Row></Section>"#;
    let json = run(&one_shape(cells)).to_json().clone();
    assert_eq!(json["saved"]["triggers"]["RecalcColor"], 1);
    assert!(!json.to_string().contains("MyTrigger"));
    assert_eq!(json["names"]["reserved_user_rows"]["msvThemeColors"], 1);
    assert!(!json.to_string().contains("Mine"));
}

// ------------------------------------------------------------------------------------------------------------------- refusals

#[test]
fn a_refusal_is_a_class_a_kind_and_a_fixed_word_and_never_a_name() {
    let r = run(b"<html>not a zip</html>");
    let refusal = r.refusal.clone().unwrap();
    assert_eq!((refusal.class, refusal.kind), ("not_zip", "not_a_package"));
    assert_eq!(r.to_json()["refusal"]["class"], "not_zip");
    assert_eq!(r.to_json()["read"], false);
    assert!(r.to_json().get("category").is_none());
    // A refused file is a report with its hash, no cells, no score, and the control's result beside it.
    assert_eq!(r.to_json()["score"], Value::Null);
    assert_eq!(r.to_json()["calc"]["control"]["passed"], true);
    assert!(r.to_json()["calc"].get("compared").is_none());
    assert!(r.calc.is_none());

    let secret = zip(&[ZipItem::stored("SECRETDIR/../evil.xml", b"x")]);
    let r = run(&secret);
    assert_eq!(r.refusal.clone().unwrap().class, "path_traversal");
    assert!(!r.to_json().to_string().contains("SECRET"));

    let none = run(&Pkg::new().build());
    assert_eq!(none.refusal.clone().unwrap().class, "no_drawing_parts");
    assert_eq!(none.to_json()["refusal"]["detail"], "no relationship leads to a document part");
}

#[test]
fn the_rule_behind_a_zip_corrupt_is_a_fixed_word() {
    // A valid archive with bytes in front of it: the reader refuses an archive whose offsets are shifted.
    let good = Pkg::new().build();
    let mut shifted = b"stub".to_vec();
    shifted.extend_from_slice(&good);
    let r = run(&shifted);
    let refusal = r.refusal.clone().unwrap();
    assert_eq!(refusal.class, "zip_corrupt");
    assert!(refusal.detail.is_some());
    // A checksum that is wrong, naming an item: the word, not the name.
    let mut bad = good.clone();
    let at = bad.windows(4).position(|w| w == b"Type").unwrap_or(40);
    bad[at] ^= 0x20;
    let r = run(&bad);
    if let Some(refusal) = &r.refusal {
        assert!(!r.to_json().to_string().contains("Content_Types"), "{refusal:?}");
    }
    assert_eq!(zip_detail(&diacraft_opc::OpcError::NotZip), None);
    assert_eq!(
        zip_detail(&diacraft_opc::OpcError::ZipCorrupt { detail: "the checksum of x is not the one the archive states".into() }),
        Some("checksum_differs")
    );
    assert_eq!(zip_detail(&diacraft_opc::OpcError::ZipCorrupt { detail: "something new".into() }), Some("other"));
}

#[test]
fn a_cap_refusal_names_its_class_and_the_report_still_has_the_hash() {
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "P")).page(PageSpec::new(2, "Q")).build();
    let limits = Limits { max_pages: 1, ..Limits::default() };
    let r = report(&bytes, &limits, Some("owner/repo"), &mut Native, &native_control());
    assert_eq!(r.refusal.as_ref().unwrap().class, "too_many_pages");
    assert_eq!(r.to_json()["sha256"], sha256_hex(&bytes));
    assert_eq!(r.to_json()["source"], "owner/repo");
}

#[test]
fn every_rule_of_the_zip_reader_has_its_own_word() {
    for (needle, word) in [
        ("no end record", "no_end_record"),
        ("ZIP64 end record is not before", "zip64_locator_order"),
        ("no ZIP64 end record where", "zip64_record_missing"),
        ("ZIP64 end record is too short", "zip64_record_short"),
        ("single-disk", "multi_disk"),
        ("directory does not end before", "directory_overlaps_end"),
        ("which do not fit in the", "row_count_exceeds_directory"),
        ("no central directory row", "directory_row_missing"),
        ("longer than 65,535", "row_over_65535"),
        ("leaves a value to its ZIP64", "zip64_field_missing"),
        ("ZIP64 field is too short", "zip64_field_short"),
        ("on another disk", "entry_on_another_disk"),
        ("more rows than the end record", "more_rows_than_counted"),
        ("flags as UTF-8", "bad_utf8_name"),
        ("no local header", "local_header_missing"),
        ("cut short", "record_cut_short"),
        ("past anything this machine", "offset_past_address_space"),
        ("entries overlap", "entries_overlap"),
        ("runs into the central directory", "data_runs_into_directory"),
        ("a folder has none", "folder_with_data"),
        ("is not in the file", "data_missing"),
        ("says it is", "size_differs"),
        ("checksum of", "checksum_differs"),
        ("cannot be read", "inflate_failed"),
    ] {
        let error = diacraft_opc::OpcError::ZipCorrupt { detail: format!("the item x {needle} y") };
        assert_eq!(zip_detail(&error), Some(word), "{needle}");
    }
}

fn refusal_of(bytes: &[u8], limits: &Limits) -> Value {
    report(bytes, limits, None, &mut Native, &native_control()).to_json().clone()
}

#[test]
fn an_xml_refusal_names_the_fault_and_the_part_when_the_part_is_package_structure() {
    let mut limits = Limits::default();
    limits.opc.xml.max_nodes = 30;
    let shapes: String = (0..40).map(|i| format!("<Shape ID=\"{i}\"/>")).collect();
    let json = refusal_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shapes)).build(), &limits);
    assert_eq!(json["refusal"]["class"], "xml_too_complex");
    let detail = json["refusal"]["detail"].as_str().unwrap();
    assert!(detail.contains("more than 30 items") && detail.contains("/visio/pages/page1.xml"), "{detail}");
    // The reader's own figures for a file it refused are in the report too.
    assert!(json["read_report"]["stats"]["file_bytes"].as_u64().unwrap() > 0);
    assert!(json["read_report"]["stats"]["zip_rows"].as_u64().unwrap() >= 8);
    // Malformed XML: the byte, and the part.
    let json = refusal_of(&VsdxBuilder::new().page(PageSpec::new(1, "P").part("<PageContents><Shapes></PageContents>")).build(), &Limits::default());
    let detail = json["refusal"]["detail"].as_str().unwrap();
    assert!(detail.contains("not well-formed at byte") && detail.contains("/visio/pages/page1.xml"), "{detail}");
}

fn page_part_named(name: &str) -> Vec<u8> {
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes></PageContents>");
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page></Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part(&format!("/visio/pages/{name}.xml"), CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), &format!("{}.xml", name.replace(' ', "%20")));
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn a_long_part_name_is_called_by_its_role_whatever_it_is_made_of() {
    // Eighty-one bytes of letters is not a name this report gives: package structure is short.
    let detail = |name: &str| refusal_of(&page_part_named(name), &Limits::default())["refusal"]["detail"].as_str().unwrap().to_owned();
    assert!(detail(&"p".repeat(60)).contains(&"p".repeat(60)), "a plain name of 60 bytes is given");
    let long_detail = detail(&"p".repeat(81));
    assert!(long_detail.contains("an XML part") && !long_detail.contains("pppp"), "{long_detail}");
}

#[test]
fn a_part_a_person_could_have_named_is_called_by_its_role_and_not_by_its_name() {
    // A part that reaches the reader by a relationship may be named anything: a page part called `Customer Acme.xml` is content.
    let json = refusal_of(&page_part_named("Customer Acme"), &Limits::default());
    let text = json.to_string();
    assert!(!text.contains("Acme"), "the name crossed: {text}");
    assert!(json["refusal"]["detail"].as_str().unwrap().contains("an XML part"), "{text}");
}

#[test]
fn a_limit_of_the_reader_names_the_part_and_the_section_it_was_reading() {
    let limits = Limits { max_cells: 5, ..Limits::default() };
    let in_section = |name: &str| {
        VsdxBuilder::new()
            .page(PageSpec::new(1, "P").shapes(&format!(
                r#"<Shape ID="1"><Section N="{name}"><Row N="a"><Cell N="X" V="1"/><Cell N="Y" V="1"/><Cell N="Z" V="1"/></Row></Section></Shape>"#
            )))
            .build()
    };
    let json = refusal_of(&in_section("User"), &limits);
    assert_eq!(json["refusal"]["class"], "too_many_cells");
    let detail = json["refusal"]["detail"].as_str().unwrap();
    assert!(detail.contains("more than") && detail.contains("in /visio/pages/page1.xml, section User"), "{detail}");
    // A section the file made up is `<other section>`, never its name.
    let json = refusal_of(&in_section("PrivateName"), &limits);
    assert!(json["refusal"]["detail"].as_str().unwrap().contains("section <other section>"));
    assert!(!json.to_string().contains("PrivateName"));
}

#[test]
fn the_rule_of_an_invalid_part_name_is_in_the_detail_and_the_name_is_not() {
    let secret = zip(&[ZipItem::stored("visio//secret.xml", b"x")]);
    let json = refusal_of(&secret, &Limits::default());
    assert_eq!(json["refusal"]["class"], "invalid_part_name");
    assert!(json["refusal"]["detail"].as_str().unwrap().contains("empty segment"));
    assert!(!json.to_string().contains("secret"));
}

// -------------------------------------------------------------------------------------------------------- the stats for the caps

#[test]
fn the_report_carries_the_sizes_that_confirm_the_caps() {
    let nested = format!("<Shape ID=\"1\"><Shapes>{}</Shapes></Shape>", shape(2, ""));
    let bytes = VsdxBuilder::new()
        .more_styles("<StyleSheet ID=\"1\"/>")
        .master(MasterSpec::new(1, "M").shapes(&shape(4, "")))
        .page(PageSpec::new(1, "A").shapes(&nested))
        .page(PageSpec::new(2, "B").shapes(&shape(1, "")))
        .build();
    let json = run(&bytes).to_json().clone();
    let stats = &json["read_report"]["stats"];
    assert_eq!((stats["styles"].as_u64(), stats["masters"].as_u64(), stats["pages"].as_u64()), (Some(2), Some(1), Some(2)));
    assert_eq!(stats["shapes_per_page"], serde_json::json!([2, 1]));
    assert_eq!(
        (stats["max_shape_depth"].as_u64(), stats["max_shapes_in_a_page"].as_u64(), stats["max_shapes_in_a_master"].as_u64()),
        (Some(2), Some(2), Some(1))
    );
    assert!(stats["items"].as_u64().unwrap() > 0 && stats["xml_bytes"].as_u64().unwrap() > 0);
}

// ------------------------------------------------------------------------------------- names, as the sample-file run found them wrong

#[test]
fn a_call_used_as_a_sheet_qualifier_is_a_call_and_is_named() {
    // `CONTAINERSHEETREF(1)!Width` calls `CONTAINERSHEETREF`: it was never named (288 files to 0), nor `LISTSHEETREF` (160 to 56).
    let cells = r#"<Cell N="Width" V="1" F="CONTAINERSHEETREF(1)!Width"/><Cell N="Height" V="1" F="LISTSHEETREF(2)!Height+CALLOUTTARGETREF()!Width"/><Cell N="PinX" V="1" F="LISTSHEETREF(LISTSHEETREF(1))!PinX"/>"#;
    let json = run(&one_shape(cells)).to_json().clone();
    let named = &json["names"]["functions"];
    assert_eq!(named["CONTAINERSHEETREF"], 1);
    assert_eq!(named["LISTSHEETREF"], 3, "one in Height and two in PinX: a qualifier call inside another's arguments is named too");
    assert_eq!(named["CALLOUTTARGETREF"], 1);
}

#[test]
fn the_power_operator_is_not_the_function_pow_and_the_function_is() {
    let json =
        run(&one_shape(r#"<Cell N="Width" V="8" F="2^3"/><Cell N="Height" V="8" F="POW(2,3)"/><Cell N="PinX" V="1" F="2^2^2"/>"#)).to_json().clone();
    assert_eq!(json["names"]["functions"]["POW"], 1, "only the call");
    assert_eq!(json["names"]["functions_unlisted_calls"], 0, "and the operator is not a call to an unlisted function either");
}

#[test]
fn the_numbered_cells_of_a_tabs_row_are_named_by_the_specs_pattern() {
    let tabs = r#"<Section N="Tabs"><Row IX="0"><Cell N="Position0" V="1"/><Cell N="Alignment0" V="0"/><Cell N="Position7" V="2"/><Cell N="Alignment7" V="0"/><Cell N="Position12" V="3"/></Row></Section>"#;
    let json = run(&one_shape(&format!("{tabs}<Cell N=\"Width\" V=\"1\"/><Cell N=\"Positionx\" V=\"1\"/><Cell N=\"Position1234\" V=\"1\"/>")))
        .to_json()
        .clone();
    let cells = &json["names"]["cells"];
    assert_eq!(cells["Tabs.Position<j>"]["value_only"], 3);
    assert_eq!(cells["Tabs.Alignment<j>"]["value_only"], 2);
    assert!(cells.get("Width").is_some());
    // A word with something other than digits after it, or too many digits, is not the pattern's.
    assert_eq!(json["names"]["cells_unlisted"]["value_only"], 2);
    let text = json.to_string();
    for private in ["Position7", "Position12", "Positionx", "Position1234", "Alignment7"] {
        assert!(!text.contains(private), "{private} crossed");
    }
}

#[test]
fn a_cell_that_is_only_the_word_of_a_pattern_is_not_the_pattern_s_and_a_call_with_a_qualifier_is_named() {
    // `Position` and `Alignment` alone are no cell the spec lists (`Position<j>` is a number after the word).
    let json = run(&one_shape(r#"<Cell N="Position" V="1"/><Cell N="Alignment" V="1"/><Cell N="Width" V="1" F="LISTSHEETREF(1)!SUM(2)"/>"#))
        .to_json()
        .clone();
    assert_eq!(json["names"]["cells_unlisted"]["value_only"], 2);
    assert!(json["names"]["cells"].get("Position<j>").is_none() && json["names"]["cells"].get("Alignment<j>").is_none());
    // The call that carries a qualifier is named, and so is the qualifier's.
    assert_eq!(json["names"]["functions"]["SUM"], 1);
    assert_eq!(json["names"]["functions"]["LISTSHEETREF"], 1);
}

#[test]
fn a_listed_cell_in_a_section_the_file_made_up_is_named_by_the_fixed_word_for_such_a_section() {
    let json = run(&one_shape(r#"<Section N="Secret"><Row N="r"><Cell N="X" V="1"/><Cell N="Mine" V="2"/></Row></Section>"#)).to_json().clone();
    assert_eq!(json["names"]["cells"]["<other section>.X"]["value_only"], 1);
    assert!(!json.to_string().contains("Secret"));
}

// ---------------------------------------------------------------------------------------------------------------- shared parts

/// A pages index of `n` rows that all name one page part of `shapes` shapes with two cells each.
pub(super) fn pages_sharing_one_part(n: usize, shapes: usize) -> Vec<u8> {
    let one: String = (0..shapes).map(|i| shape(i as u32, &(cell("Width", "1") + &cell("Height", "2")))).collect();
    let page = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes>{one}</Shapes></PageContents>");
    let mut rows = String::new();
    for i in 0..n {
        rows.push_str(&format!("<Page ID=\"{i}\" Name=\"P{i}\"><Rel r:id=\"rId1\"/></Page>"));
    }
    let pages = format!("<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\">{rows}</Pages>");
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, page)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    VsdxBuilder::new().without_pages_part().with(pkg).build()
}

#[test]
fn a_part_that_pages_share_is_named_once_in_the_tallies() {
    // Two pages name one part of one shape with two cells: the file says it once in the tallies of names, however many pages show it.
    let r = run(&pages_sharing_one_part(2, 1));
    assert_eq!(r.to_json()["names"]["cells"]["Width"]["value_only"], 1);
}

#[test]
fn a_connector_needs_two_rows_with_coordinates_and_a_shared_part_is_counted_once() {
    let connect = r#"<Connect FromSheet="7" FromCell="BeginX" FromPart="9" ToSheet="5" ToCell="PinX" ToPart="3"/>"#;
    let ends = r#"<Cell N="BeginX" V="1"/><Cell N="EndX" V="2"/>"#;
    let one_row = format!(r#"{ends}<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#);
    let two_rows = format!(
        r#"{ends}<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="Y" V="1"/></Row></Section>"#
    );
    let glued = |connector: &str| -> u64 {
        let page = PageSpec::new(1, "P").shapes(&(shape(5, "") + &shape(7, connector))).connects(connect);
        let r = run(&VsdxBuilder::new().page(page).app(reference_app_props()).build());
        r.to_json()["category"]["glued_connectors"].as_u64().unwrap()
    };
    assert_eq!(glued(&one_row), 0, "one row with coordinates is not a route");
    assert_eq!(glued(&two_rows), 1);
    // A connector glued at both ends has two `Connect` rows and is one connector.
    let both = format!("{connect}<Connect FromSheet=\"7\" FromCell=\"EndX\" FromPart=\"12\" ToSheet=\"5\" ToCell=\"PinX\" ToPart=\"3\"/>");
    let page = PageSpec::new(1, "P").shapes(&(shape(5, "") + &shape(7, &two_rows))).connects(&both);
    let r = run(&VsdxBuilder::new().page(page).app(reference_app_props()).build());
    assert_eq!(r.to_json()["category"]["glued_connectors"], 1);
    // Two pages that name one part (its shapes and its connects) are one connector.
    let part = format!(
        "<PageContents xmlns=\"{NS_2012}\"><Shapes>{}{}</Shapes><Connects>{connect}</Connects></PageContents>",
        shape(5, ""),
        shape(7, &two_rows)
    );
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><Rel r:id=\"rId1\"/></Page></Pages>"
    );
    let pkg = Pkg::new()
        .part("/visio/pages/pages.xml", CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{VIS}pages"), "pages/pages.xml")
        .part("/visio/pages/page1.xml", CT_PAGE, part)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{VIS}page"), "page1.xml");
    let r = run(&VsdxBuilder::new().without_pages_part().with(pkg).app(reference_app_props()).build());
    assert_eq!(r.to_json()["category"]["glued_connectors"], 1);
}
