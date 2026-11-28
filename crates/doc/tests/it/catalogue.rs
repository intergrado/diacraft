//! The catalogue of cells: the class and kind each batch spec gives a cell, checked against the spec tables themselves.

use std::collections::BTreeSet;

use diacraft_doc::{CellClass, CellName, Family, ValueKind, cell_info, listed_cells, section_class};

fn class_of(section: Option<&str>, name: &str) -> CellClass {
    cell_info(section, name).class
}

#[test]
fn the_classes_of_inheritance_are_what_the_batches_say() {
    use CellClass::*;
    // One or two of each, from the "At a glance" tables of the batches.
    for (section, name, class) in [
        (None, "PinX", ShapeOnly),
        (None, "Width", ShapeOnly),
        (None, "BeginX", ShapeOnly),
        (None, "SelectMode", StyleAny),
        (None, "GlueType", StyleAny),
        (None, "Blur", StyleAny),
        (None, "FillForegnd", FillFamily),
        (None, "ShapeShdwOffsetX", FillFamily),
        (None, "RotationXAngle", FillFamily),
        (None, "GlowSize", FillFamily),
        (None, "LineWeight", LineFamily),
        (None, "LineGradientEnabled", LineFamily),
        (None, "FillGradientEnabled", FillFamily),
        (None, "LeftMargin", TextFamily),
        (None, "TxtWidth", ShapeOnly),
        (None, "ImgWidth", ShapeOnly),
        (None, "PageWidth", PageSheet),
        (None, "PageLeftMargin", PageSheet),
        (None, "EnableLineProps", StyleFlag(Family::Line)),
        (None, "EnableFillProps", StyleFlag(Family::Fill)),
        (None, "EnableTextProps", StyleFlag(Family::Text)),
        (None, "HideForApply", StyleLocal),
        (Some("Geometry"), "X", ShapeOnly),
        (Some("Geometry"), "NoFill", ShapeOnly),
        (Some("Connection"), "DirX", ShapeOnly),
        (Some("Control"), "XCon", ShapeOnly),
        (Some("Scratch"), "A", ShapeOnly),
        (Some("Field"), "Value", ShapeOnly),
        (Some("Character"), "Size", TextFamily),
        (Some("Paragraph"), "IndFirst", TextFamily),
        (Some("Tabs"), "Position0", TextFamily),
        (Some("Tabs"), "Alignment3", TextFamily),
        (Some("FillGradient"), "GradientStopColor", FillFamily),
        (Some("LineGradient"), "GradientStopColor", LineFamily),
        // The page layout, ruler and grid, action, layer and layer membership batches.
        (None, "RouteStyle", PageSheet),
        (None, "LineToLineX", PageSheet),
        (None, "XGridOrigin", PageSheet),
        (None, "YRulerDensity", PageSheet),
        (None, "LayerMember", StyleAny),
        (Some("Actions"), "Action", ShapeOnly),
        (Some("Actions"), "Menu", ShapeOnly),
        (Some("Actions"), "Prompt", ShapeOnly),
        (Some("ActionTag"), "XJustify", ShapeOnly),
        (Some("ActionTag"), "X", ShapeOnly),
        (Some("ActionTag"), "DisplayMode", ShapeOnly),
        (Some("Layer"), "Visible", PageSheet),
        (Some("Layer"), "ColorTrans", PageSheet),
        (Some("Layer"), "NameUniv", PageSheet),
    ] {
        assert_eq!(class_of(section, name), class, "{section:?} {name}");
        assert!(cell_info(section, name).listed, "{section:?} {name}");
    }
}

#[test]
fn a_cell_in_a_section_is_not_a_cell_directly_under_a_sheet() {
    // `X` in Geometry and `X` in Scratch are listed; `X` directly under a sheet is not a cell any batch lists.
    assert!(!cell_info(None, "X").listed);
    assert!(cell_info(Some("Geometry"), "X").listed);
    // And a direct cell is not found in a row.
    assert!(!cell_info(Some("Geometry"), "PinX").listed);
}

#[test]
fn a_cell_of_a_layer_or_action_row_is_found_by_the_section_element_and_the_file_n() {
    // The index's `Layers` is the display group; the `Section` element is `Layer`, and the cell `Transparency` is `ColorTrans` in a file.
    assert!(cell_info(Some("Layer"), "Visible").listed);
    assert!(!cell_info(Some("Layers"), "Visible").listed);
    assert!(!cell_info(None, "Visible").listed, "a layer's cell is not a cell under a sheet");
    assert!(cell_info(Some("Layer"), "ColorTrans").listed);
    assert!(!cell_info(Some("Layer"), "Transparency").listed);
    // `Lock` is the file's `N` and `Locked` the spelling in a reference.
    assert!(cell_info(Some("Layer"), "Lock").listed);
    assert!(!cell_info(Some("Layer"), "Locked").listed);
    // `Action Tag` is the display group and `ActionTag` the element; its `X Justify` is `XJustify`.
    assert!(cell_info(Some("ActionTag"), "XJustify").listed);
    assert!(!cell_info(Some("ActionTag"), "X Justify").listed);
    assert!(!cell_info(Some("Action Tag"), "X").listed);
    // The same name in two sections is two cells, each with its own kind: `X` of an action tag is a length, `TagName` of both is text.
    assert_eq!(cell_info(Some("ActionTag"), "X").kind, ValueKind::Length);
    assert_eq!(cell_info(Some("Actions"), "TagName").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("ActionTag"), "TagName").kind, ValueKind::Text);
    // `DisplayMode` of an action tag is a row cell. The group cell of that name under a sheet is another cell, in another class.
    assert_eq!(cell_info(Some("ActionTag"), "DisplayMode").class, CellClass::ShapeOnly);
    assert_eq!(cell_info(None, "DisplayMode").class, CellClass::StyleAny);
}

#[test]
fn names_are_exact_and_a_case_variant_is_unlisted() {
    // `pinx`, `PINX` are unknown cells. `DefaultTabStop` is the `N`; the label's spelling, `DefaultTabstop`, is not.
    for name in ["pinx", "PINX", "Pinx", "defaulttabstop", "DefaultTabstop"] {
        assert!(!cell_info(None, name).listed, "{name}");
        assert_eq!(class_of(None, name), CellClass::Unlisted);
    }
    assert!(cell_info(None, "DefaultTabStop").listed);
    assert!(!cell_info(Some("Tabs"), "position0").listed);
}

#[test]
fn an_unlisted_cell_is_style_any_and_defaults_to_a_plain_zero() {
    let info = cell_info(None, "SomethingAFileInvented");
    assert_eq!((info.class, info.kind, info.default_formula, info.listed), (CellClass::Unlisted, ValueKind::Scalar, None, false));
    assert_eq!(section_class("SectionAFileInvented"), CellClass::Unlisted);
    assert_eq!(section_class("Reviewer"), CellClass::Unlisted, "the review batch is not catalogued yet");
}

#[test]
fn the_class_of_a_section_is_the_class_of_its_cells() {
    assert_eq!(section_class("Geometry"), CellClass::ShapeOnly);
    assert_eq!(section_class("Connection"), CellClass::ShapeOnly);
    assert_eq!(section_class("Character"), CellClass::TextFamily);
    assert_eq!(section_class("Paragraph"), CellClass::TextFamily);
    assert_eq!(section_class("Tabs"), CellClass::TextFamily);
    assert_eq!(section_class("FillGradient"), CellClass::FillFamily);
    assert_eq!(section_class("LineGradient"), CellClass::LineFamily);
    // The action tag, action and layer batches: the rows of `Actions` and `ActionTag` merge with the master's, and a page's `Layer` rows
    // have no master step.
    assert_eq!(section_class("Actions"), CellClass::ShapeOnly);
    assert_eq!(section_class("ActionTag"), CellClass::ShapeOnly);
    assert_eq!(section_class("Layer"), CellClass::PageSheet);
    // The data and links batch: `style-any` by project rule, how the sections resolved while no batch listed them.
    for section in ["Property", "User", "Hyperlink", "MasterSelectors"] {
        assert_eq!(section_class(section), CellClass::StyleAny, "{section}");
    }
}

#[test]
fn the_data_and_links_cells_are_listed_by_their_file_names_with_the_kinds_of_their_types() {
    // The Ask on drop cell is `Verify` in a file and in a formula, so `Ask` is no cell of the section.
    assert!(cell_info(Some("Property"), "Verify").listed);
    assert!(!cell_info(Some("Property"), "Ask").listed);
    // `vAny` reads 0 days, a string empty, a truth value, a number, a language name and a calendar 0.
    for (section, name, kind) in [
        ("Property", "Value", ValueKind::Days),
        ("User", "Value", ValueKind::Days),
        ("Property", "Label", ValueKind::Text),
        ("Property", "Format", ValueKind::Text),
        ("Property", "SortKey", ValueKind::Text),
        ("User", "Prompt", ValueKind::Text),
        ("Hyperlink", "Address", ValueKind::Text),
        ("Hyperlink", "SortKey", ValueKind::Text),
        ("Property", "Type", ValueKind::Scalar),
        ("Property", "Invisible", ValueKind::Scalar),
        ("Property", "LangID", ValueKind::Scalar),
        ("Property", "Calendar", ValueKind::Scalar),
        ("Hyperlink", "NewWindow", ValueKind::Scalar),
        ("MasterSelectors", "MasterColumn", ValueKind::Scalar),
    ] {
        let info = cell_info(Some(section), name);
        assert_eq!((info.listed, info.kind, info.class), (true, kind, CellClass::StyleAny), "{section} {name}");
    }
    assert!(cell_info(None, "ContentLayout").listed, "a cell a file defines for itself, directly under a shape");
    assert!(!cell_info(None, "Verify").listed, "a row's cell is not a cell under a sheet");
}

#[test]
fn kinds_give_the_defaults_of() {
    assert_eq!(cell_info(None, "PinX").kind, ValueKind::Length);
    assert_eq!(cell_info(None, "Angle").kind, ValueKind::Angle);
    assert_eq!(cell_info(None, "FlipX").kind, ValueKind::Scalar);
    assert_eq!(cell_info(None, "FillForegnd").kind, ValueKind::Color);
    assert_eq!(cell_info(None, "LineColor").kind, ValueKind::Color);
    assert_eq!(cell_info(Some("Character"), "Font").kind, ValueKind::Font);
    assert_eq!(cell_info(Some("Paragraph"), "BulletStr").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("Scratch"), "A").kind, ValueKind::Days, "vAny reads 0 days");
    // The layer list is a string: a missing one reads empty, not 0.
    assert_eq!(cell_info(None, "LayerMember").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("Actions"), "Menu").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("Actions"), "SortKey").kind, ValueKind::Text, "PtgString, even where it holds digits");
    assert_eq!(cell_info(Some("Actions"), "ButtonFace").kind, ValueKind::Text, "an icon number held as a string");
    assert_eq!(cell_info(Some("Actions"), "Action").kind, ValueKind::Scalar);
    assert_eq!(cell_info(Some("Actions"), "Checked").kind, ValueKind::Scalar);
    assert_eq!(cell_info(Some("Layer"), "Name").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("Layer"), "NameUniv").kind, ValueKind::Text);
    assert_eq!(cell_info(Some("Layer"), "Color").kind, ValueKind::Color);
    assert_eq!(cell_info(Some("Layer"), "ColorTrans").kind, ValueKind::Scalar);
    assert_eq!(cell_info(Some("Layer"), "Status").kind, ValueKind::Scalar);
    assert_eq!(cell_info(None, "XGridSpacing").kind, ValueKind::Length);
    assert_eq!(cell_info(None, "YRulerOrigin").kind, ValueKind::Length);
    assert_eq!(cell_info(None, "XGridDensity").kind, ValueKind::Scalar);
    // The eight spacing cells keep MS-VSDX's kind, `vScalar`, though the spec reads them as lengths.
    assert_eq!(cell_info(None, "AvenueSizeX").kind, ValueKind::Scalar);
}

#[test]
fn only_the_text_transform_cells_that_print_one_and_the_picture_sizes_read_a_printed_formula() {
    let with_formula: BTreeSet<&str> =
        listed_cells().filter(|(s, n, _)| s.is_none() && cell_info(*s, n).default_formula.is_some()).map(|(_, n, _)| n).collect();
    assert_eq!(with_formula, BTreeSet::from(["ImgHeight", "ImgWidth", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtPinX", "TxtPinY", "TxtWidth"]));
}

#[test]
fn the_tables_are_sorted_and_have_no_name_twice() {
    let direct: Vec<&str> = listed_cells().filter(|(s, _, _)| s.is_none()).map(|(_, n, _)| n).collect();
    let rows: Vec<(&str, &str)> = listed_cells().filter_map(|(s, n, _)| s.map(|s| (s, n))).collect();
    assert!(direct.windows(2).all(|w| w[0] < w[1]), "direct cells sorted and unique");
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "row cells sorted and unique");
}

#[test]
fn a_known_name_shares_the_catalogues_allocation() {
    // Hundreds of thousands of `PinX` cells hold one string between them: the name is `Static`, and equal to any other spelling of it.
    let a = CellName::new("PinX");
    let b = CellName::new(&(String::from("Pin") + "X"));
    assert_eq!(a, b);
    assert_eq!(a.as_str().as_ptr(), b.as_str().as_ptr(), "the same bytes in memory");
    // An unknown name is allocated, once per name.
    let u = CellName::new("Invented");
    assert_eq!(u.as_str(), "Invented");
}

#[test]
fn every_listed_cell_name_is_one_the_catalogue_shares() {
    // The list of shared names is kept by hand beside the tables: a cell added to a table and not to it would be allocated per cell.
    let not_shared: Vec<&str> = listed_cells()
        .map(|(_, name, _)| name)
        .filter(|name| {
            // Two strings of their own, so that equal pointers can only come from the catalogue.
            let own: String = name.chars().collect();
            let other: String = name.chars().collect();
            CellName::new(&own).as_str().as_ptr() != CellName::new(&other).as_str().as_ptr()
        })
        .collect();
    assert!(not_shared.is_empty(), "{not_shared:?} are in the tables and not in the list of shared names");
}

#[test]
fn a_direct_cell_is_found_by_its_name_in_any_letter_case() {
    use diacraft_doc::direct_cell_spelling;
    // The spelling the table lists, whatever the case asked for, and the exact name as it is.
    for (asked, listed) in [
        ("Width", "Width"),
        ("WIDTH", "Width"),
        ("width", "Width"),
        ("wIdTh", "Width"),
        ("PINX", "PinX"),
        ("pinx", "PinX"),
        ("BEGINARROW", "BeginArrow"),
    ] {
        assert_eq!(direct_cell_spelling(asked), Some(listed), "{asked}");
    }
    // A name that is none of them in any case, an empty one, one with other letters, and one longer than any the table lists.
    let (long_a, long_b) = ("w".repeat(1024), "Width".repeat(200));
    for asked in ["", "Mine", "Widt", "Widthh", "Wi dth", "Wïdth", "Geometry1", "User", long_a.as_str(), long_b.as_str()] {
        assert_eq!(direct_cell_spelling(asked), None, "{asked}");
    }
}

#[test]
fn no_two_direct_cells_differ_only_by_case_so_a_name_in_another_case_is_one_cell() {
    let mut seen = BTreeSet::new();
    for (section, name, _) in listed_cells() {
        if section.is_none() {
            assert!(seen.insert(name.to_ascii_lowercase()), "{name} and another listed cell differ only by case");
            assert_eq!(diacraft_doc::direct_cell_spelling(&name.to_ascii_uppercase()), Some(name), "{name}");
        }
    }
    assert!(seen.len() > 100);
}
