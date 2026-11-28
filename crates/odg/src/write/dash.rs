//! The dashes of DiaCraft's own: the `draw:stroke-dash` elements a save adds to
//! `office:styles` of `styles.xml`, and the name a style gives to name one.
//!
//! * **Names**: DiaCraft's own dash for `LinePattern` *n* (2 to 23) is `DiaCraft_20_dash_20_<n>`, and `DiaCraft_20_dash_20_<n>_20_flat` for the form a flat cap (`LineCap` 1) draws. The number has
//!   no sign and no leading zero. The display name has spaces (`DiaCraft dash 5 flat`) and is always written, since LibreOffice builds a definition's name from it again. The reader knows a dash
//!   by its name and its lengths ([`crate::read::props`], `dash_pattern`), so a definition the file holds under the name is used as it is.
//! * **Lengths**: percentages of the line's weight, so the dash follows the weight as the table's marks and gaps do, and no length is in points. Every pattern of the table has at most
//!   two lengths of mark and one of gap, so it is the form ODF says: `draw:dots1` marks of `draw:dots1-length`, then `draw:dots2` marks of `draw:dots2-length`, each followed by a gap of
//!   `draw:distance` ([`Form`]). A mark of 0 is a dot: its length is `0%`, and the cap draws it (a round cap a round dot, a square one a square dot).
//! * **The flat form**: the table's lists are for a round or projecting cap, which adds half a weight past each end of a mark. Under a flat cap each mark is a weight longer and each gap a
//!   weight shorter, so that the mark as seen is the same under all three caps: `DiaCraft_20_dash_20_3_20_flat` is a mark of `100%` and a gap of `400%`.
//! * **`draw:style`** is `rect` always: the cap is the style's `svg:stroke-linecap`, as LibreOffice writes its own dashes. (Measured, LibreOffice 26.8.2.2: `rect` and `round` draw alike under an
//!   explicit cap.)
//! * **Only the patterns of the table** have a dash. A pattern past 23, a custom one (254, a master as the pattern) and a fraction have none: the cell is carried by DiaCraft's
//!   data and the style says nothing of the stroke's pattern.

use crate::read::props::{DASH_PREFIX, FLAT_SUFFIX, PATTERNS, own_dash};

/// A dash of DiaCraft's own: the pattern (2 to 23) and whether it is the form of a flat cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DashKey {
    pub pattern: u8,
    pub flat: bool,
}

/// What a dash says, in ODF's terms: the lengths and the distance in percent of the line's weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Form {
    /// How many marks of the first length, and their length. At least one.
    pub dots1: u32,
    pub length1: u32,
    /// How many marks of the second length and their length: 0 and 0 when the pattern has one length of mark.
    pub dots2: u32,
    pub length2: u32,
    /// The gap after each mark.
    pub distance: u32,
}

/// A length in weights as a percentage, whole: `None` for one that is no number or is past a thousand weights (the table's longest is 39).
fn percent(weights: f64) -> Option<u32> {
    let p = (weights * 100.0).round();
    (p.is_finite() && (0.0..=100_000.0).contains(&p)).then_some(p as u32)
}

impl DashKey {
    /// The dash for the pattern `value` under a cap of the form `flat`: `None` unless `value` is a whole number from 2 to 23 (a number outside the table has no dash).
    pub(crate) fn of_pattern(value: f64, flat: bool) -> Option<DashKey> {
        if !(value.is_finite() && value.fract() == 0.0 && (2.0..=23.0).contains(&value)) {
            return None;
        }
        Some(DashKey { pattern: value as u8, flat })
    }

    /// The name of the definition (`draw:name`).
    pub(crate) fn name(self) -> String {
        if self.flat { format!("{DASH_PREFIX}{}{FLAT_SUFFIX}", self.pattern) } else { format!("{DASH_PREFIX}{}", self.pattern) }
    }

    /// The display name: spaces for the underscores, which LibreOffice builds the name from again.
    pub(crate) fn display_name(self) -> String {
        self.name().replace("_20_", " ")
    }

    /// The dash a name is, when it is the one this module makes for a key (the number plain and in the table, the flat suffix last): the reader's rule.
    pub(crate) fn of_name(name: &str) -> Option<DashKey> {
        own_dash(name).map(|(pattern, flat)| DashKey { pattern, flat })
    }

    /// The form of the dash: the table's marks and gaps of the pattern in percent of the weight, each mark a weight longer and each gap a weight shorter for the flat form,
    /// and the marks put as ODF says them, the first run of equal marks then the rest. `None` for a pattern the table has no row for, and for a row that is not at most two lengths of mark
    /// and one of gap (none is).
    pub(crate) fn form(self) -> Option<Form> {
        let (_, table) = PATTERNS.iter().find(|(n, _)| *n == self.pattern)?;
        let extra = if self.flat { 1.0 } else { 0.0 };
        let gap = percent(table.get(1)? - extra)?;
        if !table.iter().skip(1).step_by(2).all(|g| percent(g - extra) == Some(gap)) {
            return None;
        }
        let marks: Vec<u32> = table.iter().step_by(2).map(|m| percent(m + extra)).collect::<Option<_>>()?;
        let first = *marks.first()?;
        let dots1 = marks.iter().take_while(|m| **m == first).count();
        let rest = marks.get(dots1..)?;
        let (dots2, length2) = match rest.first() {
            None => (0, 0),
            Some(&second) if rest.iter().all(|m| *m == second) => (rest.len(), second),
            Some(_) => return None,
        };
        Some(Form { dots1: u32::try_from(dots1).ok()?, length1: first, dots2: u32::try_from(dots2).ok()?, length2, distance: gap })
    }

    /// The attributes of the element, each by its local name in the `draw` namespace and in the order they are written: the name, the display name, the style, the first kind of mark and its
    /// length, the second kind and its length when the pattern has one, and the distance. `None` for a key that has no form.
    pub(crate) fn attributes(self) -> Option<Vec<(&'static str, String)>> {
        let f = self.form()?;
        let mut out = vec![
            ("name", self.name()),
            ("display-name", self.display_name()),
            ("style", "rect".to_owned()),
            ("dots1", f.dots1.to_string()),
            ("dots1-length", format!("{}%", f.length1)),
        ];
        if f.dots2 > 0 {
            out.push(("dots2", f.dots2.to_string()));
            out.push(("dots2-length", format!("{}%", f.length2)));
        }
        out.push(("distance", format!("{}%", f.distance)));
        Some(out)
    }

    /// The element as it is written in `office:styles`, under the prefix `draw` (`declare` is the declaration of the prefix, where the part does not bind it): empty for a key that has no form.
    pub(crate) fn element(self, draw: &str, declare: &str) -> String {
        let Some(attributes) = self.attributes() else { return String::new() };
        let mut text = format!("<{draw}:stroke-dash{declare}");
        for (local, value) in attributes {
            text.push_str(&format!(" {draw}:{local}=\"{value}\""));
        }
        text.push_str("/>");
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pattern of the table as ODF says it for a round or projecting cap (from the table in the specification, not from the code's own): `(pattern, dots1, length1, dots2,
    /// length2, distance)`.
    const ROUND: [(u8, u32, u32, u32, u32, u32); 22] = [
        (2, 1, 700, 0, 0, 500),
        (3, 1, 0, 0, 0, 500),
        (4, 1, 700, 1, 0, 500),
        (5, 1, 700, 2, 0, 500),
        (6, 2, 700, 1, 0, 500),
        (7, 1, 1900, 1, 700, 500),
        (8, 1, 1900, 2, 700, 500),
        (9, 1, 300, 0, 0, 300),
        (10, 1, 0, 0, 0, 300),
        (11, 1, 300, 1, 0, 300),
        (12, 1, 300, 2, 0, 300),
        (13, 2, 300, 1, 0, 300),
        (14, 1, 900, 1, 300, 300),
        (15, 1, 900, 2, 300, 300),
        (16, 1, 1500, 0, 0, 900),
        (17, 1, 0, 0, 0, 900),
        (18, 1, 1500, 1, 0, 900),
        (19, 1, 1500, 2, 0, 900),
        (20, 2, 1500, 1, 0, 900),
        (21, 1, 3900, 1, 1500, 900),
        (22, 1, 3900, 2, 1500, 900),
        (23, 1, 100, 0, 0, 200),
    ];

    fn form(pattern: u8, flat: bool) -> (u32, u32, u32, u32, u32) {
        let f = DashKey { pattern, flat }.form().unwrap_or_else(|| panic!("no form for {pattern}"));
        (f.dots1, f.length1, f.dots2, f.length2, f.distance)
    }

    #[test]
    fn every_pattern_of_the_table_is_its_runs_in_percent_of_the_weight() {
        for (n, d1, l1, d2, l2, gap) in ROUND {
            assert_eq!(form(n, false), (d1, l1, d2, l2, gap), "pattern {n}");
        }
    }

    #[test]
    fn the_flat_form_makes_each_mark_a_weight_longer_and_each_gap_a_weight_shorter() {
        for (n, d1, l1, d2, l2, gap) in ROUND {
            // A second kind of mark is only there when the pattern has one; a dot of 0 is one weight long under a flat cap.
            let want = (d1, l1 + 100, d2, if d2 > 0 { l2 + 100 } else { 0 }, gap - 100);
            assert_eq!(form(n, true), want, "pattern {n}, flat");
        }
        // The cases prints: 8, 4 for 7, 5; 4, 2 for 3, 3; 1, 4 for 0, 5; 2, 1 for 1, 2.
        assert_eq!(form(2, true), (1, 800, 0, 0, 400));
        assert_eq!(form(9, true), (1, 400, 0, 0, 200));
        assert_eq!(form(3, true), (1, 100, 0, 0, 400));
        assert_eq!(form(23, true), (1, 200, 0, 0, 100));
    }

    #[test]
    fn a_form_read_back_as_runs_is_the_row_of_the_table() {
        // The marks in the order ODF draws them (the first kind, then the second) and the gap after each are the table's list, read around its cycle.
        for (n, table) in PATTERNS {
            for flat in [false, true] {
                let f = DashKey { pattern: n, flat }.form().unwrap();
                let mut runs = Vec::new();
                for _ in 0..f.dots1 {
                    runs.extend([f64::from(f.length1) / 100.0, f64::from(f.distance) / 100.0]);
                }
                for _ in 0..f.dots2 {
                    runs.extend([f64::from(f.length2) / 100.0, f64::from(f.distance) / 100.0]);
                }
                let back = |i: usize, v: f64| {
                    if !flat {
                        v
                    } else if i.is_multiple_of(2) {
                        v - 1.0
                    } else {
                        v + 1.0
                    }
                };
                assert_eq!(runs.len(), table.len(), "pattern {n}");
                for (i, (got, want)) in runs.iter().zip(table.iter()).enumerate() {
                    assert!((back(i, *got) - want).abs() < 1e-9, "pattern {n} flat {flat} run {i}: {got} for {want}");
                }
            }
        }
    }

    #[test]
    fn a_name_is_made_from_the_key_and_the_key_from_the_name() {
        for pattern in 2..=23u8 {
            for flat in [false, true] {
                let key = DashKey { pattern, flat };
                assert_eq!(DashKey::of_name(&key.name()), Some(key), "{}", key.name());
                assert_eq!(key.display_name(), key.name().replace("_20_", " "));
            }
        }
        assert_eq!(DashKey { pattern: 5, flat: false }.name(), "DiaCraft_20_dash_20_5");
        assert_eq!(DashKey { pattern: 5, flat: true }.display_name(), "DiaCraft dash 5 flat");
        for bad in [
            "DiaCraft_20_dash_20_1",
            "DiaCraft_20_dash_20_24",
            "DiaCraft_20_dash_20_05",
            "DiaCraft_20_dash_20_+5",
            "DiaCraft_20_dash_20_5_20_flat_20_flat",
            "DiaCraft_20_dash_20_",
            "DiaCraft_20_dash_20_5 ",
            "Dash_20_5",
            "",
        ] {
            assert_eq!(DashKey::of_name(bad), None, "{bad:?}");
        }
        // A name of a file may be as long as the file: a long run of digits, or of the flat suffix, is no dash and costs one pass over it.
        let long = "0".repeat(1 << 20);
        for bad in [
            format!("{DASH_PREFIX}{long}5"),
            format!("{DASH_PREFIX}5{}", FLAT_SUFFIX.repeat(1 << 16)),
            format!("{DASH_PREFIX}{}", "9".repeat(1 << 20)),
        ] {
            assert_eq!(DashKey::of_name(&bad), None, "a name of {} bytes", bad.len());
        }
    }

    #[test]
    fn only_a_whole_number_of_the_table_has_a_dash() {
        for ok in [2.0, 3.0, 22.0, 23.0] {
            assert!(DashKey::of_pattern(ok, false).is_some(), "{ok}");
        }
        for bad in [0.0, 1.0, 1.5, 2.5, 23.5, 24.0, 254.0, -3.0, f64::NAN, f64::INFINITY, 1e300] {
            assert_eq!(DashKey::of_pattern(bad, false), None, "{bad}");
        }
        assert_eq!(DashKey::of_pattern(7.0, true), Some(DashKey { pattern: 7, flat: true }));
    }

    #[test]
    fn the_element_says_the_name_the_lengths_and_no_second_kind_for_a_pattern_that_has_one() {
        let one = DashKey { pattern: 2, flat: false }.element("draw", "");
        assert_eq!(
            one,
            "<draw:stroke-dash draw:name=\"DiaCraft_20_dash_20_2\" draw:display-name=\"DiaCraft dash 2\" draw:style=\"rect\" draw:dots1=\"1\" draw:dots1-length=\"700%\" draw:distance=\"500%\"/>"
        );
        let two = DashKey { pattern: 5, flat: true }.element("d", " xmlns:d=\"urn:x\"");
        assert_eq!(
            two,
            "<d:stroke-dash xmlns:d=\"urn:x\" d:name=\"DiaCraft_20_dash_20_5_20_flat\" d:display-name=\"DiaCraft dash 5 flat\" d:style=\"rect\" d:dots1=\"1\" d:dots1-length=\"800%\" d:dots2=\"2\" d:dots2-length=\"100%\" d:distance=\"400%\"/>"
        );
        assert_eq!(DashKey { pattern: 99, flat: false }.element("draw", ""), "", "a key outside the table has no element");
    }
}
