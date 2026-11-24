//! Paper, page sizes and drawing scales: the tables behind page setup.
//!
//! **Sources.** The number `PaperKind` holds is a Windows paper code (which gives no list of the codes). The
//! codes and sizes below are from two public pages, both read 2026-11-07: the numbers from Microsoft's Excel VBA reference for the
//! `XlPaperSize` enumeration (<https://learn.microsoft.com/en-us/office/vba/api/excel.xlpapersize>), which uses the Windows codes, and the
//! sizes from the `DEVMODE` structure's page (<https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-devmodea>). The sizes of
//! the named page sizes are ISO 216 (A0 to A6), the North American loose sizes, ANSI engineering C to E and the ARCH sizes, as the Wikipedia
//! article <https://en.wikipedia.org/wiki/Paper_size> gives them (read 2026-11-07). Which sizes a gallery offers, their ids and their names, and
//! the drawing scales that follow, are **ours**: no specification lists them.
//!
//! Every size here is **portrait** (width at most height), in paper inches; a page or a sheet in landscape is the same size turned.

use diacraft_units::Unit;

// The paper table, the code of the paper a page prints on when its sheet names none, and the way a sheet is held are the print's own
// (`diacraft_render::print`): Page Setup and Print read the same table, so a page set to a paper prints on that paper.
pub use diacraft_render::print::{DEFAULT_PAPER, Orientation, PAPERS, Paper, paper};

const fn mm(n: f64) -> f64 {
    n / 25.4
}

/// A named page size of the Size gallery.
#[derive(Debug, PartialEq)]
pub struct SizePreset {
    pub id: &'static str,
    pub name: &'static str,
    /// What the gallery files it under.
    pub group: &'static str,
    /// Portrait, in paper inches.
    pub width: f64,
    pub height: f64,
    /// The paper this size prints on, when the table of papers has it: choosing the size then sets the paper too.
    pub paper_kind: Option<u32>,
    /// What `DrawingSizeType` is labelled with: 2 a standard size, 5 metric, 6 ANSI engineering, 7 ANSI architectural.
    pub size_type: u8,
}

const fn size(
    id: &'static str,
    name: &'static str,
    group: &'static str,
    (width, height): (f64, f64),
    paper_kind: Option<u32>,
    size_type: u8,
) -> SizePreset {
    SizePreset { id, name, group, width, height, paper_kind, size_type }
}

/// The page sizes the Size gallery offers, in its order.
pub const SIZES: &[SizePreset] = &[
    size("letter", "Letter", "North America", (8.5, 11.0), Some(1), 2),
    size("legal", "Legal", "North America", (8.5, 14.0), Some(5), 2),
    size("tabloid", "Tabloid", "North America", (11.0, 17.0), Some(3), 2),
    size("executive", "Executive", "North America", (7.25, 10.5), Some(7), 2),
    size("statement", "Statement", "North America", (5.5, 8.5), Some(6), 2),
    size("a0", "A0", "ISO", (mm(841.0), mm(1189.0)), None, 5),
    size("a1", "A1", "ISO", (mm(594.0), mm(841.0)), None, 5),
    size("a2", "A2", "ISO", (mm(420.0), mm(594.0)), None, 5),
    size("a3", "A3", "ISO", (mm(297.0), mm(420.0)), Some(8), 5),
    size("a4", "A4", "ISO", (mm(210.0), mm(297.0)), Some(9), 5),
    size("a5", "A5", "ISO", (mm(148.0), mm(210.0)), Some(11), 5),
    size("a6", "A6", "ISO", (mm(105.0), mm(148.0)), None, 5),
    size("ansi_c", "ANSI C", "ANSI engineering", (17.0, 22.0), Some(24), 6),
    size("ansi_d", "ANSI D", "ANSI engineering", (22.0, 34.0), Some(25), 6),
    size("ansi_e", "ANSI E", "ANSI engineering", (34.0, 44.0), Some(26), 6),
    size("arch_a", "Arch A", "ANSI architectural", (9.0, 12.0), None, 7),
    size("arch_b", "Arch B", "ANSI architectural", (12.0, 18.0), None, 7),
    size("arch_c", "Arch C", "ANSI architectural", (18.0, 24.0), None, 7),
    size("arch_d", "Arch D", "ANSI architectural", (24.0, 36.0), None, 7),
    size("arch_e", "Arch E", "ANSI architectural", (36.0, 48.0), None, 7),
];

/// The size named `id` (in any case).
pub fn size_preset(id: &str) -> Option<&'static SizePreset> {
    let id = id.trim();
    SIZES.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

/// How near a page's size is to a named size for it to be that size: 0.002 inch (0.05 mm), so that a file's 8.268 by 11.693 is A4.
pub const SIZE_TOLERANCE: f64 = 0.002;

/// The named size a rectangle of `width` by `height` paper inches is, in either orientation.
pub fn size_named(width: f64, height: f64) -> Option<&'static SizePreset> {
    let (short, long) = if width <= height { (width, height) } else { (height, width) };
    SIZES.iter().find(|s| (s.width - short).abs() <= SIZE_TOLERANCE && (s.height - long).abs() <= SIZE_TOLERANCE)
}

/// A drawing scale of the Scale list: a length on paper that stands for a length in the drawing.
#[derive(Debug, PartialEq)]
pub struct ScalePreset {
    pub id: &'static str,
    pub name: &'static str,
    /// Which family: `DrawingScaleType` is 0 for none, 1 architectural, 2 civil engineering, 4 metric, 5 mechanical engineering.
    pub kind: u8,
    pub paper: (f64, Unit),
    pub drawing: (f64, Unit),
}

const fn scale(id: &'static str, name: &'static str, kind: u8, paper: (f64, Unit), drawing: (f64, Unit)) -> ScalePreset {
    ScalePreset { id, name, kind, paper, drawing }
}

use Unit::{Foot as Ft, Inch as In, Millimetre as Mm};

/// The drawing scales the page setup offers, in its order. Choosing one writes `PageScale` and `DrawingScale` (`V` in inches, `U` the unit named
/// here) and `DrawingScaleType`; the shapes' own cells are not changed.
pub const SCALES: &[ScalePreset] = &[
    scale("none", "No scale (1:1)", 0, (1.0, In), (1.0, In)),
    scale("arch_1_16", "1/16 in = 1 ft", 1, (1.0 / 16.0, In), (1.0, Ft)),
    scale("arch_3_32", "3/32 in = 1 ft", 1, (3.0 / 32.0, In), (1.0, Ft)),
    scale("arch_1_8", "1/8 in = 1 ft", 1, (1.0 / 8.0, In), (1.0, Ft)),
    scale("arch_3_16", "3/16 in = 1 ft", 1, (3.0 / 16.0, In), (1.0, Ft)),
    scale("arch_1_4", "1/4 in = 1 ft", 1, (1.0 / 4.0, In), (1.0, Ft)),
    scale("arch_3_8", "3/8 in = 1 ft", 1, (3.0 / 8.0, In), (1.0, Ft)),
    scale("arch_1_2", "1/2 in = 1 ft", 1, (1.0 / 2.0, In), (1.0, Ft)),
    scale("arch_3_4", "3/4 in = 1 ft", 1, (3.0 / 4.0, In), (1.0, Ft)),
    scale("arch_1", "1 in = 1 ft", 1, (1.0, In), (1.0, Ft)),
    scale("arch_1_1_2", "1 1/2 in = 1 ft", 1, (1.5, In), (1.0, Ft)),
    scale("arch_3", "3 in = 1 ft", 1, (3.0, In), (1.0, Ft)),
    scale("civil_10", "1 in = 10 ft", 2, (1.0, In), (10.0, Ft)),
    scale("civil_20", "1 in = 20 ft", 2, (1.0, In), (20.0, Ft)),
    scale("civil_30", "1 in = 30 ft", 2, (1.0, In), (30.0, Ft)),
    scale("civil_40", "1 in = 40 ft", 2, (1.0, In), (40.0, Ft)),
    scale("civil_50", "1 in = 50 ft", 2, (1.0, In), (50.0, Ft)),
    scale("civil_60", "1 in = 60 ft", 2, (1.0, In), (60.0, Ft)),
    scale("civil_100", "1 in = 100 ft", 2, (1.0, In), (100.0, Ft)),
    scale("metric_1_2", "1:2", 4, (1.0, Mm), (2.0, Mm)),
    scale("metric_1_5", "1:5", 4, (1.0, Mm), (5.0, Mm)),
    scale("metric_1_10", "1:10", 4, (1.0, Mm), (10.0, Mm)),
    scale("metric_1_20", "1:20", 4, (1.0, Mm), (20.0, Mm)),
    scale("metric_1_50", "1:50", 4, (1.0, Mm), (50.0, Mm)),
    scale("metric_1_100", "1:100", 4, (1.0, Mm), (100.0, Mm)),
    scale("metric_1_200", "1:200", 4, (1.0, Mm), (200.0, Mm)),
    scale("metric_1_500", "1:500", 4, (1.0, Mm), (500.0, Mm)),
    scale("metric_1_1000", "1:1000", 4, (1.0, Mm), (1000.0, Mm)),
    scale("mech_2_1", "2:1", 5, (2.0, In), (1.0, In)),
    scale("mech_4_1", "4:1", 5, (4.0, In), (1.0, In)),
    scale("mech_10_1", "10:1", 5, (10.0, In), (1.0, In)),
    scale("mech_1_2", "1:2", 5, (1.0, In), (2.0, In)),
    scale("mech_1_4", "1:4", 5, (1.0, In), (4.0, In)),
    scale("mech_1_8", "1:8", 5, (1.0, In), (8.0, In)),
];

/// The scale named `id` (in any case).
pub fn scale_preset(id: &str) -> Option<&'static ScalePreset> {
    let id = id.trim();
    SCALES.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

/// The family a scale is, as a word: `DrawingScaleType`'s numbers.
pub fn scale_family(kind: u8) -> &'static str {
    match kind {
        0 => "none",
        1 => "architectural",
        2 => "civil engineering",
        3 => "custom",
        4 => "metric",
        5 => "mechanical engineering",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_named_sizes_are_the_standard_ones_in_portrait() {
        let letter = size_preset("LETTER").unwrap();
        assert_eq!((letter.width, letter.height), (8.5, 11.0));
        let a4 = size_preset("a4").unwrap();
        assert!((a4.width - 8.267_716_535).abs() < 1e-8 && (a4.height - 11.692_913_386).abs() < 1e-8, "{a4:?}");
        assert!(SIZES.iter().all(|s| s.width <= s.height && s.width > 0.0), "every size is held portrait");
        let ids: HashSet<&str> = SIZES.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), SIZES.len(), "ids are unique");
        // A size that names a paper has the paper's size.
        for s in SIZES {
            if let Some(kind) = s.paper_kind {
                let p = paper(kind).unwrap_or_else(|| panic!("{}: paper {kind} is not in the table", s.id));
                assert!((p.width - s.width).abs() < 1e-9 && (p.height - s.height).abs() < 1e-9, "{}: {s:?} against {p:?}", s.id);
            }
        }
    }

    #[test]
    fn the_papers_are_portrait_and_their_codes_are_unique() {
        assert!(PAPERS.iter().all(|p| p.width <= p.height && p.width > 0.0));
        let kinds: HashSet<u32> = PAPERS.iter().map(|p| p.kind).collect();
        assert_eq!(kinds.len(), PAPERS.len());
        assert!(paper(DEFAULT_PAPER).is_some());
        assert!(paper(32767).is_none() && paper(0).is_none(), "a user-defined paper has no size");
    }

    #[test]
    fn a_file_page_of_a_rounded_size_is_a_named_size_in_either_orientation() {
        // The sample files' commonest page sizes: 8.268 by 11.693, 8.5 by 11.
        assert_eq!(size_named(8.268, 11.693).map(|s| s.id), Some("a4"));
        assert_eq!(size_named(11.693, 8.268).map(|s| s.id), Some("a4"));
        assert_eq!(size_named(11.0, 8.5).map(|s| s.id), Some("letter"));
        assert_eq!(size_named(10.0, 7.0), None);
    }

    #[test]
    fn the_scales_hold_the_ratios_they_are_named_for() {
        let inches = |(v, u): (f64, Unit)| u.to_internal(v);
        let ratio = |id: &str| {
            let s = scale_preset(id).unwrap();
            inches(s.drawing) / inches(s.paper)
        };
        assert!((ratio("arch_1_4") - 48.0).abs() < 1e-9, "1/4 in = 1 ft is 48 to 1");
        assert!((ratio("arch_3_32") - 128.0).abs() < 1e-9, "3/32 in = 1 ft: the specifications' own example, 128 to 1");
        assert!((ratio("metric_1_100") - 100.0).abs() < 1e-9);
        assert!((ratio("mech_2_1") - 0.5).abs() < 1e-9);
        assert!((ratio("civil_10") - 120.0).abs() < 1e-9);
        assert!((ratio("none") - 1.0).abs() < 1e-12);
        let ids: HashSet<&str> = SCALES.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), SCALES.len());
        assert!(SCALES.iter().all(|s| s.paper.0 > 0.0 && s.drawing.0 > 0.0));
    }

    #[test]
    fn an_orientation_is_the_way_round_a_rectangle_is() {
        assert_eq!(Orientation::of(11.0, 8.5), Orientation::Landscape);
        assert_eq!(Orientation::of(8.5, 11.0), Orientation::Portrait);
        assert_eq!(Orientation::of(8.5, 8.5), Orientation::Portrait);
        assert_eq!(Orientation::parse(" Landscape "), Some(Orientation::Landscape));
        assert_eq!(Orientation::parse("sideways"), None);
    }
}
