//! The fixed palette of a colour index.
//!
//! A colour given as an integer
//! from 0 to 23 is a fixed colour; an integer above 23 looks up the document's colour table, which only the document
//! holds; and an invalid index is colour 0, which is the caller's rule (this module answers `None` and the caller decides).
//! Written for DiaCraft from the spec's list: nothing here is from the sibling crate.

use crate::Rgba;

/// The 24 colours of R12, by index: 0 black, 1 white, 2 red, 3 green, 4 blue, 5 yellow, 6 magenta, 7 cyan, then the dark
/// primaries and secondaries, the silver, and the greys from light to dark.
pub const FIXED_PALETTE: [Rgba; 24] = [
    Rgba::rgb(0x00, 0x00, 0x00),
    Rgba::rgb(0xFF, 0xFF, 0xFF),
    Rgba::rgb(0xFF, 0x00, 0x00),
    Rgba::rgb(0x00, 0xFF, 0x00),
    Rgba::rgb(0x00, 0x00, 0xFF),
    Rgba::rgb(0xFF, 0xFF, 0x00),
    Rgba::rgb(0xFF, 0x00, 0xFF),
    Rgba::rgb(0x00, 0xFF, 0xFF),
    Rgba::rgb(0x80, 0x00, 0x00),
    Rgba::rgb(0x00, 0x80, 0x00),
    Rgba::rgb(0x00, 0x00, 0x80),
    Rgba::rgb(0x80, 0x80, 0x00),
    Rgba::rgb(0x80, 0x00, 0x80),
    Rgba::rgb(0x00, 0x80, 0x80),
    Rgba::rgb(0xC0, 0xC0, 0xC0),
    Rgba::rgb(0xE6, 0xE6, 0xE6),
    Rgba::rgb(0xCD, 0xCD, 0xCD),
    Rgba::rgb(0xB3, 0xB3, 0xB3),
    Rgba::rgb(0x9A, 0x9A, 0x9A),
    Rgba::rgb(0x80, 0x80, 0x80),
    Rgba::rgb(0x66, 0x66, 0x66),
    Rgba::rgb(0x4D, 0x4D, 0x4D),
    Rgba::rgb(0x33, 0x33, 0x33),
    Rgba::rgb(0x1A, 0x1A, 0x1A),
];

/// The colour of a fixed palette index (0 to 23), or `None` for any other integer: above 23 the document's colour table is
/// asked, and a negative or enormous index is invalid.
pub fn fixed_color(index: i64) -> Option<Rgba> {
    usize::try_from(index).ok().and_then(|i| FIXED_PALETTE.get(i)).copied()
}
