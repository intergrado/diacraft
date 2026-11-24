//! The fixed colour palette of a colour index.
//! This part is new in DiaCraft: it is not from the sibling crate.

use diacraft_color::{FIXED_PALETTE, Rgba, fixed_color};

/// An integer from 0 to 23 is a fixed palette, as the spec lists it.
const R12: [&str; 24] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "800000", "008000", "000080", "808000", "800080", "008080",
    "C0C0C0", "E6E6E6", "CDCDCD", "B3B3B3", "9A9A9A", "808080", "666666", "4D4D4D", "333333", "1A1A1A",
];

#[test]
fn every_index_from_0_to_23_is_the_specs_colour() {
    for (i, hex) in R12.iter().enumerate() {
        assert_eq!(fixed_color(i as i64).map(|c| c.hex()).as_deref(), Some(*hex), "index {i}");
        assert_eq!(FIXED_PALETTE.get(i).copied(), Rgba::from_hex(hex), "index {i}");
    }
    assert_eq!(FIXED_PALETTE.len(), 24);
}

/// vsdx-document V6: colour integer 2 gives `#FF0000`, 23 gives `#1A1A1A`, and 24 is not in the fixed palette: it looks up
/// the document's colour table (`ColorEntry` with `IX` 24), which only the document holds.
#[test]
fn an_index_above_23_is_for_the_documents_colour_table() {
    assert_eq!(fixed_color(2), Some(Rgba::rgb(255, 0, 0)));
    assert_eq!(fixed_color(23), Some(Rgba::rgb(0x1A, 0x1A, 0x1A)));
    assert_eq!(fixed_color(24), None);
    // Naive: wrapping the index around the palette, or clamping it to the last entry.
    assert_ne!(fixed_color(24), fixed_color(0));
    assert_ne!(fixed_color(24), fixed_color(23));
}

/// Hostile indexes: no panic, and none has a colour here (an invalid index is colour 0 by the caller's rule).
#[test]
fn an_index_out_of_range_has_no_fixed_colour() {
    for i in [-1, i64::MIN, i64::MAX, 253, 254, 1 << 40] {
        assert_eq!(fixed_color(i), None, "{i}");
    }
}
