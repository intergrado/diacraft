//! The words a report may use: names from the spec's own lists, the error words and the units, and the fixed words
//! for what is not listed. **A name crosses only when the spec lists it**; every other name in a file (a row's, a section's, a cell the spec
//! does not list, a function a file made up) is content, and is counted without being named.

use diacraft_doc::{SECTION_NAMES, Section};

use super::names::CELLS;

/// The `U` values.
pub(super) const UNITS: &[&str] = &[
    "AC", "AD", "BOOL", "C", "CM", "COLOR", "CY", "C_D", "D", "DA", "DATE", "DE", "DEG", "DL", "DP", "DT", "ED", "EH", "EM", "ES", "EW", "F_I", "FT",
    "HA", "IN", "IN_F", "KM", "M", "MI", "MI_F", "MM", "NM", "P", "P_PT", "PER", "PNT", "PT", "RAD", "STR", "YD",
];

/// The six error words, sorted.
pub(super) const ERRORS: &[&str] = &["#DIM!", "#DIV/0!", "#N/A", "#NUM!", "#REF!", "#VALUE!"];

/// What a name that is not in the spec's lists is called in a count by name.
pub(super) const UNLISTED: &str = "(unlisted)";

/// The word for a section a file made up.
pub(super) const OTHER_SECTION: &str = "<other section>";

/// The spec's own copy of a name in a sorted table, so a name crosses only when the spec lists it.
pub(super) fn listed(table: &'static [&'static str], name: &str) -> Option<&'static str> {
    table.binary_search(&name).ok().and_then(|i| table.get(i)).copied()
}

/// As [`listed`] for a cell name, which the spec may list as a pattern: `Alignment<j>` and `Position<j>` (the cells of a `Tabs` row, numbered
/// from 0). A name is the pattern's when it is the word and a number (`Alignment3`); the pattern's own spelling crosses, never the name.
pub(super) fn listed_cell(name: &str) -> Option<&'static str> {
    if let Some(found) = listed(CELLS, name) {
        return Some(found);
    }
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if stem.len() == name.len() || name.len() - stem.len() > 3 {
        return None;
    }
    CELLS.iter().copied().find(|pattern| pattern.strip_suffix("<j>") == Some(stem))
}

/// A name in a short table that is not sorted.
pub(super) fn find_in(table: &'static [&'static str], name: &str) -> Option<&'static str> {
    table.iter().find(|n| **n == name).copied()
}

/// The 17 predefined sections by name, `""` for no section, and a fixed word for any other (the name of a section a file defines is content).
pub(super) fn section_word(section: Option<&Section>) -> &'static str {
    match section {
        None => "",
        Some(s) => section_name_word(s.name.as_str()),
    }
}

/// As [`section_word`] for a section's name.
pub(super) fn section_name_word(name: &str) -> &'static str {
    SECTION_NAMES.iter().find(|n| **n == name).copied().unwrap_or(OTHER_SECTION)
}

/// An error word as a report gives it: one of the six, else `other`.
pub(super) fn error_word(text: &str) -> &'static str {
    find_in(ERRORS, text).unwrap_or("other")
}
