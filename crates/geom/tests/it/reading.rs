//! The readings in `rules`: row numbers of references, and the constants a render may flip.

use diacraft_geom::rules;

/// Given sections `IX` 0 and `IX` 1, the second with rows `IX` 1 to 4, `Geometry2.X4` is section `IX` 1, row
/// `IX` 4. Adding one to the row gives row `IX` 3, and not adding one to the section gives section `IX` 2, which does not exist.
/// `Geometry1.Path` names section `IX` 0.
#[test]
fn geo_a_reference_names_section_ix_plus_one_and_the_row_ix_itself() {
    assert_eq!(rules::geometry_reference(2, 4), Some((1, 4)));
    assert_ne!(rules::geometry_reference(2, 4), Some((1, 3)), "naive: row number is IX plus one");
    assert_ne!(rules::geometry_reference(2, 4), Some((2, 4)), "naive: section number is IX");
    assert_eq!(rules::geometry_section_ix(1), Some(0), "Geometry1.Path names section IX 0");
    assert_eq!(rules::geometry_section_ix(2), Some(1));
}

/// Given an `Ellipse` alone in section `IX` 0, with `IX` 1, `Geometry1.C1` reads its `C`. A reader
/// numbering from 0 reads no row.
#[test]
fn geo_geometry1_c1_is_section_ix_0_row_ix_1() {
    assert_eq!(rules::geometry_reference(1, 1), Some((0, 1)));
    assert_ne!(rules::geometry_reference(1, 1), Some((0, 0)), "naive: rows numbered from 0");
}

/// formula-language V25: `Geometry2.X4` is the Geometry section with `IX` 1, row `IX` 4. Naive: MS-VSDX's
/// `IX` plus one gives row `IX` 3 for the second.
#[test]
fn of_the_formula_language() {
    assert_eq!(rules::geometry_reference(2, 4), Some((1, 4)));
}

/// There is no `Geometry0`: the section's number is `IX` plus one, so 0 would be `IX` -1. No panic at the limits.
#[test]
fn a_number_no_ix_has_names_nothing() {
    assert_eq!(rules::geometry_reference(0, 1), None);
    assert_eq!(rules::geometry_section_ix(0), None);
    assert_eq!(rules::geometry_reference(u32::MAX, u32::MAX), Some((u32::MAX - 1, u32::MAX)));
    assert_eq!(rules::geometry_reference(1, 0), Some((0, 0)), "row 0 is the section's property row");
}

/// The readings are single named values, so a render that disagrees changes one line: pin what they are.
#[test]
fn the_named_readings_are_the_specs() {
    assert_eq!(rules::SECTION_NUMBER_OVER_IX, 1, "Section number is IX plus one");
    assert_eq!(rules::ROW_NUMBER_OVER_IX, 0, "Row number is the IX itself, against MS-VSDX's plus one");
    assert_eq!(rules::ARC_BOW_SIGN, 1.0, "Positive bows to the right of travel");
    assert_eq!(rules::effective_bow(0.25), 0.25);
    assert_eq!(rules::effective_bow(-3.0), -3.0);
    assert_eq!(rules::AXIS_TURN_SIGN, -1.0, "Turn by -C");
    assert_eq!(rules::ratio_scale(4.0), 0.25, "Divide x by D");
    assert_eq!(rules::CLOSED_TOLERANCE, 1e-5, "D3");
    assert_eq!(rules::MAX_ELLIPTICAL_RATIO, 1000.0, "EARC");
    assert_eq!(rules::ARC_CHORD_RATIO, 1e-9, "The share of the larger of Width and Height");
}
