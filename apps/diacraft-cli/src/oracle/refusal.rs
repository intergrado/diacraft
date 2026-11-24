//! Why a file would not read, in words a report may carry: a class, a kind, and a fixed phrase for the rule behind it. A name from the file
//! never appears: the reader's own message holds an item's name when it is about one, which would be content.

use diacraft_opc::{ErrorKind, OpcError, XmlError};
use diacraft_vsdx::{RefusedAt, VsdxError, part_label};

/// Why a file would not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub class: &'static str,
    pub kind: &'static str,
    /// Which rule fired or where the reader was, in words from a fixed set and numbers: for `zip_corrupt` the rule of the ZIP reader; for
    /// `invalid_part_name` the rule; for `no_drawing_parts` which case; for an XML class which limit or fault and in which part; for a limit of
    /// the reader the part and the section it was reading. **A part is named only when its name is package structure** (letters, digits and
    /// `/ . _ -`, short: `diacraft_vsdx::part_label`), otherwise by its role: a name a person typed is content.
    pub detail: Option<String>,
}

fn kind_word(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::NotAPackage => "not_a_package",
        ErrorKind::TooBig => "too_big",
        ErrorKind::Damaged => "damaged",
    }
}

/// A fixed word for the rule of the ZIP reader that refused a file as `zip_corrupt`: the reader's own message holds an item's name when it is
/// about one, which would be content, so the report says which rule and never the text.
pub fn zip_detail(error: &OpcError) -> Option<&'static str> {
    let OpcError::ZipCorrupt { detail } = error else { return None };
    const RULES: &[(&str, &str)] = &[
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
    ];
    Some(RULES.iter().find(|(needle, _)| detail.contains(needle)).map_or("other", |(_, word)| word))
}

/// The refusal of a read error.
pub fn refusal_of(error: &VsdxError, at: Option<&RefusedAt>) -> Refusal {
    let detail = match error {
        VsdxError::Package(e) => zip_detail(e).or_else(|| package_detail(e)).map(str::to_owned),
        VsdxError::NoDrawingParts(why) => Some((*why).to_owned()),
        VsdxError::Xml { part, error } => Some(format!("{} in {}", xml_word(error), part_label("an XML part", part))),
        VsdxError::Limit { kind, max } => {
            let wherever = match at {
                Some(at) if !at.part.is_empty() && !at.section.is_empty() => format!(" in {}, section {}", at.part, at.section),
                Some(at) if !at.part.is_empty() => format!(" in {}", at.part),
                _ => String::new(),
            };
            Some(format!("more than {max} {}{wherever}", kind.what()))
        }
        VsdxError::Model(e) => Some(e.to_string()),
        VsdxError::Worker(why) => Some((*why).to_owned()),
    };
    Refusal { class: error.class(), kind: kind_word(error.kind()), detail }
}

/// The rule of `opc` behind a refusal that is not a `zip_corrupt`, when it is a fixed word.
fn package_detail(error: &OpcError) -> Option<&'static str> {
    match error {
        OpcError::InvalidPartName { reason, .. } => Some(reason),
        _ => None,
    }
}

/// A fixed phrase for what was wrong with an XML part, with a number where `opc` has one.
fn xml_word(error: &XmlError) -> String {
    match error {
        XmlError::Malformed { offset, .. } => format!("not well-formed at byte {offset}"),
        XmlError::EntityRefused { offset, .. } => format!("a DTD or an entity at byte {offset}"),
        XmlError::TooDeep { limit } => format!("nested deeper than {limit}"),
        XmlError::TooManyNodes { limit } => format!("more than {limit} items"),
        XmlError::AttributeTooLarge { limit } => format!("an attribute longer than {limit} bytes"),
    }
}
