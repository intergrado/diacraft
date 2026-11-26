//! What a sheet window needs to know about sections: the display group of a cell, the order of the sections, which sections take rows and
//! which can be inserted or deleted, the row numbers of a reference, and the spelling of a section in one. The tables are checked against the
//! spec's own tables in `catalogue_spec.rs`; the tests here hold the behaviour of the functions, and tie the order of the sections and of the
//! cells to the rows of the window's spec (for the order of the sections, R12 for the order of the cells, R14 for what a shape inserts).

use std::collections::BTreeSet;

use diacraft_doc::{
    DISPLAY_ORDER, Owner, SECTION_SPELLINGS, SectionKind, ShapeRef, SheetKind, SheetRef, display_group, display_section, ix_to_row_number,
    next_row_number, row_number_to_ix, section_for_spelling, section_rules, section_spelling,
};
use proptest::prelude::*;

// ---- the display group of a cell ------------------------------------------------------------------------------------------

#[test]
fn a_cell_directly_under_a_sheet_has_the_display_group_the_index_gives_it() {
    assert_eq!(display_group("PinX"), Some("Shape Transform"));
    assert_eq!(display_group("Width"), Some("Shape Transform"));
    assert_eq!(display_group("LockWidth"), Some("Protection"));
    assert_eq!(display_group("BeginX"), Some("1-D Endpoints"));
    assert_eq!(display_group("PageWidth"), Some("Page Properties"));
    assert_eq!(display_group("Ruler & Grid"), None, "a section's label is not a cell");
}

#[test]
fn a_cell_with_no_group_in_the_index_has_none() {
    // The cells seen only in samples sit directly under a sheet and belong to no section (the index's D13).
    for name in ["TextColor", "PageColor", "LockNoSelect", "ContentLayout", "DefaultFillStyle", "DefaultLineStyle", "DefaultTextStyle"] {
        assert_eq!(display_group(name), None, "{name}");
    }
    // A cell of a row is not directly under a sheet, and an unknown name is no cell.
    for name in ["X", "Y", "GradientStopColor", "Row_1", "", "Nonsense"] {
        assert_eq!(display_group(name), None, "{name:?}");
    }
}

#[test]
fn a_display_group_is_looked_up_by_the_exact_name() {
    // Names match in case.
    assert_eq!(display_group("pinx"), None);
    assert_eq!(display_group("PINX"), None);
    assert_eq!(display_group("PinX "), None);
}

#[test]
fn a_name_that_a_row_also_has_is_a_direct_cell_of_its_group() {
    // `LangID`, `Calendar` and `Comment` are cells of Miscellaneous and cells in rows of other sections; asked for as a cell directly under a
    // sheet, they are Miscellaneous's. `DisplayMode` is Group Properties' and an Action Tag row cell.
    assert_eq!(display_group("LangID"), Some("Miscellaneous"));
    assert_eq!(display_group("Calendar"), Some("Miscellaneous"));
    assert_eq!(display_group("Comment"), Some("Miscellaneous"));
    assert_eq!(display_group("DisplayMode"), Some("Group Properties"));
}

#[test]
fn the_cells_of_a_group_come_in_its_own_list_and_each_names_its_group_back() {
    let mut seen = BTreeSet::new();
    for section in DISPLAY_ORDER.iter().filter(|s| s.kind == SectionKind::Cells) {
        assert!(!section.cells.is_empty(), "{} lists no cell", section.label);
        for cell in section.cells {
            assert_eq!(display_group(cell), Some(section.label), "{cell}");
            assert!(seen.insert(*cell), "{cell} is in two groups");
        }
    }
    let first = display_section("Shape Transform").expect("the table has Shape Transform");
    assert!(first.cells.contains(&"PinX") && first.cells.contains(&"Width"));
}

// ---- the order of the sections ---------------------------------------------------------------------------------------------
//
// One table serves every kind of sheet: the orders the window's spec gives (R2 for a shape, R3 for what R2 leaves out, R4 for a page, R5 for a
// style, R6 for the document) agree wherever they overlap, so a single total order satisfies all of them and a per-kind order is not needed. Each
// rule is held below as a chain of labels that must come in that order in the table, with other sections allowed between them.

/// The sections of a shape's sheet in the order of the published window images. `Geometry` stands for every Geometry section, `Action Tag` for
/// the section the 2010 window called Smart tags, and `Line Gradient` and `Fill Gradient` are the index's names for the two gradient stops sections.
const R2_SHAPE: &[&str] = &[
    "1-D Endpoints",
    "Shape Transform",
    "User-defined Cells",
    "Shape Data",
    "Hyperlinks",
    "Connection Points",
    "Actions",
    "Controls",
    "Geometry",
    "Scratch",
    "Protection",
    "Miscellaneous",
    "Group Properties",
    "Line Format",
    "Fill Format",
    "Text Fields",
    "Character",
    "Paragraph",
    "Tabs",
    "Annotation",
    "Text Block Format",
    "Text Transform",
    "Layer Membership",
    "Events",
    "Foreign Image Info",
    "Image Properties",
    "Glue Info",
    "Alignment",
    "Shape Layout",
    "Action Tag",
    "Bevel Properties",
    "3-D Rotation Properties",
    "Line Gradient",
    "Fill Gradient",
    "Theme Properties",
    "Quick Style",
];

/// R3 (a project rule where no image shows the place): what follows Action Tags on a shape, then the sections a file defines itself.
const R3_TAIL: &[&str] = &[
    "Action Tag",
    "Gradient Properties",
    "Additional Effect Properties",
    "Bevel Properties",
    "3-D Rotation Properties",
    "Line Gradient",
    "Fill Gradient",
    "Theme Properties",
    "Quick Style",
    "Change Shape Behavior",
    "MasterSelectors",
];

/// A page's sheet: its own groups first, the sections of rows (the order, with Layers after them), then Print Properties and Theme Properties.
const R4_PAGE: &[&str] = &[
    "Page Properties",
    "Page Layout",
    "Ruler & Grid",
    "User-defined Cells",
    "Shape Data",
    "Hyperlinks",
    "Actions",
    "Scratch",
    "Layers",
    "Print Properties",
    "Theme Properties",
];

/// What a style's sheet shows of the groups of a shape, in order, after Style Properties (a project rule: first).
const R5_STYLE: &[&str] = &["Style Properties", "Miscellaneous", "Group Properties", "Line Format", "Fill Format", "Character", "Paragraph", "Tabs"];

/// The position of the section labelled `label` in the order.
fn position(label: &str) -> usize {
    DISPLAY_ORDER.iter().position(|s| s.label == label).unwrap_or_else(|| panic!("{label} is not in the order"))
}

/// Asserts that the labels of `chain` come in this order in the table.
fn assert_chain(rule: &str, chain: &[&str]) {
    for pair in chain.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(position(a) < position(b), "{rule}: {a} (place {}) must come before {b} (place {})", position(a), position(b));
    }
}

#[test]
fn r2_the_sections_of_a_shape_come_in_the_order_of_the_window() {
    assert_chain("R2", R2_SHAPE);
}

#[test]
fn r2_1d_endpoints_come_above_shape_transform_and_not_below_it() {
    // The index lists Shape Transform first; the window shows 1-D Endpoints above it (a 2024 image).
    assert!(position("1-D Endpoints") < position("Shape Transform"));
    // The rows of a shape sit between its transform and its Protection: Controls above Geometry above Scratch (images of 2022 and 2024).
    assert_chain("R2", &["Shape Transform", "Controls", "Geometry", "Scratch", "Protection"]);
}

#[test]
fn r3_what_the_images_do_not_place_follows_action_tags_and_a_files_own_sections_come_after() {
    assert_chain("R3", R3_TAIL);
}

#[test]
fn r4_the_sections_of_a_page_come_in_the_order_of_its_sheet() {
    assert_chain("R4", R4_PAGE);
    // Print Properties comes after the sections of rows, not with the page's own groups.
    assert!(position("Print Properties") > position("Layers") && position("Print Properties") > position("Ruler & Grid"));
}

#[test]
fn r5_a_style_shows_its_sections_in_the_order_of_a_shapes_and_style_properties_first() {
    assert_chain("R5", R5_STYLE);
    assert!(position("Style Properties") < position(R2_SHAPE[0]), "Style Properties comes before every section of a shape");
}

#[test]
fn r6_the_document_has_document_properties_first_and_reviewer_last() {
    assert!(position("Document Properties") < position(R2_SHAPE[0]), "Document Properties comes before every section of R2");
    let reviewer = position("Reviewer");
    assert_eq!(reviewer + 1, DISPLAY_ORDER.len(), "Reviewer is the last section");
    for label in R2_SHAPE.iter().chain(R3_TAIL).chain(R4_PAGE) {
        assert!(position(label) < reviewer, "{label} comes before Reviewer");
    }
}

#[test]
fn v1_to_v3_of_the_window_spec_come_out_in_the_order_given() {
    // A 1-D connector with Controls, a Geometry section, User-defined Cells and the usual groups.
    assert_chain(
        "V1",
        &[
            "1-D Endpoints",
            "Shape Transform",
            "User-defined Cells",
            "Controls",
            "Geometry",
            "Protection",
            "Miscellaneous",
            "Line Format",
            "Fill Format",
        ],
    );
    // A group holding User-defined Cells, Shape Data and Actions.
    assert_chain(
        "V2",
        &[
            "Shape Transform",
            "User-defined Cells",
            "Shape Data",
            "Actions",
            "Protection",
            "Miscellaneous",
            "Group Properties",
            "Line Format",
            "Fill Format",
        ],
    );
    // A page holding User-defined Cells and Layers.
    assert_chain("V3", &["Page Properties", "Page Layout", "Ruler & Grid", "User-defined Cells", "Layers", "Print Properties"]);
}

#[test]
fn every_section_is_in_the_order_once() {
    let labels: Vec<&str> = DISPLAY_ORDER.iter().map(|s| s.label).collect();
    let unique: BTreeSet<&str> = labels.iter().copied().collect();
    assert_eq!(unique.len(), labels.len(), "a label is listed twice");
    // The sections a chain names are all in the table (a name that is not would panic in `position`), and every kind of sheet is covered.
    for label in R2_SHAPE.iter().chain(R3_TAIL).chain(R4_PAGE).chain(R5_STYLE) {
        assert!(labels.contains(label), "{label}");
    }
}

// ---- the order of the cells of a section -----------------------------------------------------------------------------------

/// The cells a section lists first, in the order the window lays them out, by the name a file gives each (the titles `Ask` and `Tip` are the
/// cells `Verify` and `Prompt`, and `X Dynamics` and `X Behavior` are `XDyn` and `XCon`). A cell R12 does not name follows these, in the index's order
/// (`catalogue_spec.rs` checks that). Shape Transform fills its three columns down; Geometry shows its five own cells above the columns X, Y, A to E.
pub const R12_FIRST: &[(&str, &[&str])] = &[
    ("Shape Transform", &["Width", "Height", "Angle", "PinX", "PinY", "LocPinX", "LocPinY", "FlipX", "FlipY", "ResizeMode"]),
    ("Geometry", &["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag", "X", "Y", "A", "B", "C", "D", "E"]),
    ("Shape Data", &["Label", "Prompt", "Type", "Format", "Value", "SortKey", "Invisible", "Verify", "LangID", "Calendar"]),
    ("User-defined Cells", &["Value", "Prompt"]),
    ("Controls", &["X", "Y", "XDyn", "YDyn", "XCon", "YCon", "CanGlue", "Prompt"]),
    ("Actions", &["Action", "Menu", "TagName", "ButtonFace", "SortKey", "Checked", "Disabled", "ReadOnly", "Invisible", "BeginGroup", "FlyoutChild"]),
    ("Text Fields", &["Format", "Value", "Calendar", "ObjectKind"]),
];

#[test]
fn r12_shape_transform_starts_with_width_height_and_angle() {
    let cells = display_section("Shape Transform").expect("Shape Transform").cells;
    // The first column of the window, then the second and the third.
    assert_eq!(&cells[..3], &["Width", "Height", "Angle"]);
    assert_eq!(&cells[3..7], &["PinX", "PinY", "LocPinX", "LocPinY"]);
    assert_eq!(&cells[7..], &["FlipX", "FlipY", "ResizeMode"]);
}

#[test]
fn r12_geometry_lists_its_own_cells_then_the_columns_x_y_and_a_to_e() {
    let cells = display_section("Geometry").expect("Geometry").cells;
    assert_eq!(&cells[..5], &["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"]);
    assert_eq!(&cells[5..12], &["X", "Y", "A", "B", "C", "D", "E"]);
}

#[test]
fn r12_a_section_of_rows_lists_its_column_titles_first() {
    for (label, first) in R12_FIRST {
        let section = display_section(label).unwrap_or_else(|| panic!("{label} is not in the order"));
        assert!(section.cells.len() >= first.len(), "{label} lists fewer cells than R12 names");
        assert_eq!(&section.cells[..first.len()], *first, "{label}");
    }
}

// ---- the lookup of a section -----------------------------------------------------------------------------------------------

#[test]
fn a_section_is_found_by_its_label_or_by_its_element_name() {
    let by_label = display_section("Shape Data").expect("Shape Data");
    let by_element = display_section("Property").expect("Property is the element of Shape Data");
    assert!(std::ptr::eq(by_label, by_element));
    assert_eq!(by_label.kind, SectionKind::Rows);
    assert_eq!(by_label.element, Some("Property"));
    let transform = display_section("Shape Transform").expect("Shape Transform");
    assert_eq!((transform.kind, transform.element), (SectionKind::Cells, None));
    assert_eq!(display_section("Geometry").map(|s| s.label), Some("Geometry"));
    assert_eq!(display_section("Connection").map(|s| s.label), Some("Connection Points"));
    assert!(display_section("Nonsense").is_none() && display_section("").is_none());
}

#[test]
fn no_label_is_the_element_name_of_another_section() {
    for a in DISPLAY_ORDER {
        for b in DISPLAY_ORDER {
            if !std::ptr::eq(a, b) {
                assert_ne!(Some(a.label), b.element, "{} is the element of {}", a.label, b.label);
            }
        }
    }
}

// ---- the rules of a section ------------------------------------------------------------------------------------------------

#[test]
fn shape_transform_cannot_be_deleted() {
    // V6 of the shape sheet window's check list.
    let rules = section_rules("Shape Transform");
    assert!(!rules.deletable && !rules.insertable && !rules.rows_addable);
    for kind in [SheetKind::Shape, SheetKind::Page, SheetKind::Document, SheetKind::Style] {
        assert!(!rules.can_insert_on(kind) && !rules.can_delete_on(kind), "{kind:?}");
    }
}

#[test]
fn rows_made_by_the_text_or_owned_by_another_command_are_not_added_by_hand() {
    for name in ["Character", "Paragraph", "Tabs", "Text Fields", "Field", "Layers", "Layer", "Reviewer", "Annotation"] {
        let rules = section_rules(name);
        assert!(!rules.rows_addable, "{name}");
        assert!(!rules.insertable && !rules.deletable, "{name}");
    }
}

#[test]
fn a_section_of_rows_that_takes_rows_can_be_inserted_on_the_sheets_the_design_names() {
    let shape_only = [SheetKind::Shape];
    let everywhere = [SheetKind::Shape, SheetKind::Page, SheetKind::Document, SheetKind::Style];
    for (name, kinds) in [
        ("Geometry", &shape_only[..]),
        ("Connection Points", &shape_only),
        ("Controls", &shape_only),
        ("Scratch", &everywhere),
        ("Shape Data", &everywhere),
        ("User-defined Cells", &everywhere),
        ("Hyperlinks", &everywhere),
        ("Actions", &everywhere),
        ("Action Tag", &everywhere),
    ] {
        let rules = section_rules(name);
        assert!(rules.rows_addable && rules.insertable && rules.deletable, "{name}");
        for kind in everywhere {
            let expected = kinds.contains(&kind);
            assert_eq!((rules.can_insert_on(kind), rules.can_delete_on(kind)), (expected, expected), "{name} on {kind:?}");
        }
        assert_eq!(rules.row_limit, None, "{name}");
    }
}

#[test]
fn the_gradient_stops_take_rows_and_ignore_those_past_the_tenth() {
    for name in ["Fill Gradient", "Line Gradient"] {
        let rules = section_rules(name);
        assert!(rules.rows_addable, "{name}");
        assert!(!rules.insertable && !rules.deletable, "{name}: it belongs to the fill or the line");
        assert_eq!(rules.row_limit, Some(10), "{name}");
    }
}

#[test]
fn text_transform_is_inserted_and_deleted_on_a_shape_and_has_no_rows_to_add() {
    let rules = section_rules("Text Transform");
    assert!(!rules.rows_addable && rules.insertable && rules.deletable);
    assert!(rules.can_insert_on(SheetKind::Shape) && rules.can_delete_on(SheetKind::Shape));
    for kind in [SheetKind::Page, SheetKind::Document, SheetKind::Style] {
        assert!(!rules.can_insert_on(kind) && !rules.can_delete_on(kind), "{kind:?}");
    }
}

#[test]
fn r14_layer_membership_is_inserted_and_deleted_on_a_shape_like_text_transform() {
    let rules = section_rules("Layer Membership");
    assert!(!rules.rows_addable, "one cell, no rows");
    assert!(rules.insertable && rules.deletable, "Insert Section offers it on a shape; Delete Section takes its cell away");
    assert_eq!(rules.row_limit, None);
    assert!(rules.can_insert_on(SheetKind::Shape) && rules.can_delete_on(SheetKind::Shape));
    // What the dialog lists on the other sheets is not known, so the other kinds keep the design's lists, which have no Layer Membership.
    for kind in [SheetKind::Page, SheetKind::Document, SheetKind::Style] {
        assert!(!rules.can_insert_on(kind) && !rules.can_delete_on(kind), "{kind:?}");
    }
}

#[test]
fn r14_a_shape_can_insert_the_sections_the_dialog_offers_and_no_others() {
    // The dialog on a shape's sheet: User-defined cells, Shape Data, Hyperlinks, Connection points, Actions, Controls, Geometry (with Ellipse and
    // Infinite line, two kinds of Geometry section), Scratch, Text transform, Layer membership and Smart tags (Action Tag). Reviewer, Annotation and
    // Layers are greyed on a shape.
    let offered: BTreeSet<&str> = DISPLAY_ORDER.iter().filter(|s| s.rules.can_insert_on(SheetKind::Shape)).map(|s| s.label).collect();
    let expected: BTreeSet<&str> = [
        "User-defined Cells",
        "Shape Data",
        "Hyperlinks",
        "Connection Points",
        "Actions",
        "Controls",
        "Geometry",
        "Scratch",
        "Text Transform",
        "Layer Membership",
        "Action Tag",
    ]
    .into_iter()
    .collect();
    assert_eq!(offered, expected);
    for greyed in ["Reviewer", "Annotation", "Layers"] {
        assert!(!section_rules(greyed).can_insert_on(SheetKind::Shape), "{greyed}");
    }
}

#[test]
fn every_other_display_group_and_every_section_no_spec_lists_allows_nothing() {
    let exceptions = ["Text Transform", "Layer Membership"];
    for section in DISPLAY_ORDER.iter().filter(|s| s.kind == SectionKind::Cells && !exceptions.contains(&s.label)) {
        let rules = section_rules(section.label);
        assert!(!rules.rows_addable && !rules.insertable && !rules.deletable, "{}", section.label);
    }
    for name in ["MasterSelectors", "Nonsense", "", "Geometry1", "geometry"] {
        let rules = section_rules(name);
        assert!(!rules.rows_addable && !rules.insertable && !rules.deletable && rules.row_limit.is_none(), "{name:?}");
    }
}

#[test]
fn rules_are_the_same_by_label_and_by_element_name() {
    for section in DISPLAY_ORDER {
        if let Some(element) = section.element {
            assert_eq!(section_rules(element), section_rules(section.label), "{}", section.label);
        }
    }
}

#[test]
fn a_sheet_ref_has_a_kind() {
    assert_eq!(SheetKind::of(&SheetRef::Document), SheetKind::Document);
    assert_eq!(SheetKind::of(&SheetRef::Style(3)), SheetKind::Style);
    assert_eq!(SheetKind::of(&SheetRef::Page(1)), SheetKind::Page);
    // A master's page sheet holds what a page's does.
    assert_eq!(SheetKind::of(&SheetRef::Master(1)), SheetKind::Page);
    assert_eq!(SheetKind::of(&SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0))), SheetKind::Shape);
    assert_eq!(SheetKind::of(&SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0))), SheetKind::Shape);
}

// ---- the row numbers of a reference ----------------------------------------------------------------------------------------

#[test]
fn a_geometry_row_number_is_the_ix_and_any_other_is_the_ix_plus_one() {
    assert_eq!(row_number_to_ix(true, 3), Some(3));
    assert_eq!(row_number_to_ix(true, 0), Some(0));
    assert_eq!(row_number_to_ix(false, 1), Some(0));
    assert_eq!(row_number_to_ix(false, 4), Some(3));
    assert_eq!(ix_to_row_number(true, 3), Some(3));
    assert_eq!(ix_to_row_number(false, 0), Some(1));
    assert_eq!(ix_to_row_number(false, 3), Some(4));
}

#[test]
fn row_zero_outside_geometry_and_a_number_past_the_end_name_no_row() {
    assert_eq!(row_number_to_ix(false, 0), None);
    assert_eq!(ix_to_row_number(false, u32::MAX), None);
    assert_eq!(row_number_to_ix(false, u32::MAX), Some(u32::MAX - 1));
    assert_eq!(ix_to_row_number(false, u32::MAX - 1), Some(u32::MAX));
    assert_eq!(row_number_to_ix(true, u32::MAX), Some(u32::MAX));
    assert_eq!(ix_to_row_number(true, u32::MAX), Some(u32::MAX));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn row_numbers_and_ix_values_are_inverses_where_both_exist(geometry in any::<bool>(), n in any::<u32>()) {
        if let Some(ix) = row_number_to_ix(geometry, n) {
            prop_assert_eq!(ix_to_row_number(geometry, ix), Some(n));
        }
        if let Some(number) = ix_to_row_number(geometry, n) {
            prop_assert_eq!(row_number_to_ix(geometry, number), Some(n));
        }
    }
}

// ---- the next free number for a row ----------------------------------------------------------------------------------------

#[test]
fn the_next_row_number_is_the_least_one_that_is_not_taken() {
    assert_eq!(next_row_number(&[]), Some(1));
    assert_eq!(next_row_number(&[1]), Some(2));
    assert_eq!(next_row_number(&[1, 2, 3]), Some(4));
    assert_eq!(next_row_number(&[2, 3]), Some(1));
    assert_eq!(next_row_number(&[1, 3]), Some(2));
    // Unsorted, repeated, and a zero (which is no row number) do not change the answer.
    assert_eq!(next_row_number(&[3, 1, 2, 2, 1]), Some(4));
    assert_eq!(next_row_number(&[0]), Some(1));
    assert_eq!(next_row_number(&[0, 1, 5]), Some(2));
    assert_eq!(next_row_number(&[u32::MAX]), Some(1));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_next_row_number_agrees_with_a_search_of_the_set(taken in proptest::collection::vec(0u32..40, 0..60)) {
        let set: BTreeSet<u32> = taken.iter().copied().collect();
        let expected = (1u32..).find(|n| !set.contains(n));
        prop_assert_eq!(next_row_number(&taken), expected);
    }
}

// ---- the spelling of a section in a reference ------------------------------------------------------------------------------

#[test]
fn a_section_is_spelled_in_a_reference_as_formula_language_r40_says() {
    for (element, spelling) in [
        ("ActionTag", "SmartTags"),
        ("Character", "Char"),
        ("Connection", "Connections"),
        ("Control", "Controls"),
        ("Field", "Fields"),
        ("FillGradient", "FillGradientStops"),
        ("Layer", "Layers"),
        ("LineGradient", "LineGradientStops"),
        ("Paragraph", "Para"),
        ("Property", "Prop"),
        ("Scratch", "Scratch"),
        ("User", "User"),
    ] {
        assert_eq!(section_spelling(element), Some(spelling), "{element}");
        assert_eq!(section_for_spelling(spelling), Some(element), "{spelling}");
    }
}

#[test]
fn geometry_is_spelled_with_its_number_and_so_is_in_no_table() {
    assert_eq!(section_spelling("Geometry"), None);
    assert_eq!(section_for_spelling("Geometry"), None);
    assert_eq!(section_for_spelling("Geometry1"), None);
    assert_eq!(section_spelling("Nonsense"), None);
    assert_eq!(section_for_spelling("Nonsense"), None);
    assert_eq!(section_for_spelling("char"), None, "names match in case");
}

#[test]
fn the_spelling_table_is_a_bijection() {
    let spellings: BTreeSet<&str> = SECTION_SPELLINGS.iter().map(|(s, _)| *s).collect();
    let elements: BTreeSet<&str> = SECTION_SPELLINGS.iter().map(|(_, e)| *e).collect();
    assert_eq!(spellings.len(), SECTION_SPELLINGS.len());
    assert_eq!(elements.len(), SECTION_SPELLINGS.len());
    for (spelling, element) in SECTION_SPELLINGS {
        assert_eq!(section_for_spelling(spelling), Some(*element));
        assert_eq!(section_spelling(element), Some(*spelling));
    }
}
