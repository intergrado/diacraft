//! `[Content_Types].xml`: the `Default` rows that give a type to an extension and the `Override` rows that give one to a part.
//!
//! Every row is kept as written, in file order, including rows for extensions and parts that are not in the package and rows that
//! repeat one before them (a save carries each over). Lookups follow R7: an `Override` by part name,
//! ASCII case-insensitive; else the `Default` for the text after the last dot of the last segment, also case-insensitive. Where the
//! "one of each" is broken, the first row wins and a [`Warning`] says so: a reader that took the last would let a row at the end of
//! the stream change the type of a part named at the start.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::error::{OpcError, Warning, XmlError};
use crate::limits::Limits;
use crate::name::{fold, lenient_decode};
use crate::xml;

/// The name of the stream, which is a ZIP item and not a part.
pub const CONTENT_TYPES_ITEM: &str = "[Content_Types].xml";

/// The rows of a content types stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentTypes {
    /// The `Default` rows as `(Extension, ContentType)`, as written, in file order.
    pub defaults: Vec<(String, String)>,
    /// The `Override` rows as `(PartName, ContentType)`, as written, in file order.
    pub overrides: Vec<(String, String)>,
    /// Folded extension to the index of the first `Default` row for it.
    default_index: HashMap<String, usize>,
    /// Folded part name to the index of the first `Override` row for it.
    override_index: HashMap<String, usize>,
}

impl ContentTypes {
    /// The content type of the part named `part`, or `None` when no row gives it one. The part need not be in the package: the
    /// answer is about the name.
    pub fn lookup(&self, part: &str) -> Option<&str> {
        let key = fold(part);
        if let Some(&row) = self.override_index.get(&key) {
            return self.overrides.get(row).map(|(_, content_type)| content_type.as_str());
        }
        let last_segment = key.rsplit('/').next()?;
        let (_, extension) = last_segment.rsplit_once('.')?;
        let row = *self.default_index.get(extension)?;
        self.defaults.get(row).map(|(_, content_type)| content_type.as_str())
    }
}

/// Reads the stream. `Err` for XML that is not well-formed, whose root is not `Types`, or that has more rows than the limit.
pub(crate) fn parse(bytes: &[u8], limits: &Limits) -> Result<(ContentTypes, Vec<Warning>), OpcError> {
    let part = || CONTENT_TYPES_ITEM.to_owned();
    let root = xml::parse_with(bytes, &limits.package_xml()).map_err(|error| OpcError::Xml { part: part(), error })?;
    if !root.is("Types") {
        let message = format!("the root element is {:?}, not Types", root.name);
        return Err(OpcError::Xml { part: part(), error: XmlError::Malformed { offset: 0, message } });
    }
    let mut types = ContentTypes::default();
    let mut warnings = Vec::new();
    let mut rows = 0usize;
    for row in root.elements() {
        let is_default = row.is("Default");
        if !is_default && !row.is("Override") {
            continue;
        }
        rows += 1;
        if rows > limits.max_content_type_entries {
            return Err(OpcError::TooManyContentTypeEntries { limit: limits.max_content_type_entries });
        }
        let (key_attr, key_kind) = if is_default { ("Extension", "Default") } else { ("PartName", "Override") };
        let (Some(key), Some(content_type)) = (row.attr(key_attr), row.attr("ContentType")) else {
            warnings.push(Warning::IncompleteContentTypeEntry { element: key_kind.to_owned() });
            continue;
        };
        if is_default {
            let index = types.defaults.len();
            match types.default_index.entry(key.to_ascii_lowercase()) {
                Entry::Vacant(slot) => {
                    slot.insert(index);
                }
                Entry::Occupied(_) => warnings.push(Warning::DuplicateDefault { extension: key.to_owned() }),
            }
            types.defaults.push((key.to_owned(), content_type.to_owned()));
        } else {
            let index = types.overrides.len();
            let folded = fold(&lenient_decode(key));
            match types.override_index.entry(folded) {
                Entry::Vacant(slot) => {
                    slot.insert(index);
                }
                Entry::Occupied(_) => warnings.push(Warning::DuplicateOverride { part_name: key.to_owned() }),
            }
            types.overrides.push((key.to_owned(), content_type.to_owned()));
        }
    }
    Ok((types, warnings))
}
