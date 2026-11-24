//! `manifest.xml`: the format version, the kind, and every other entry of the package with its role (the public spec's 2.4).
//!
//! The root is `manifest`, in DiaCraft's namespace, with `format`, `kind` and `written-by`; it holds one `part` element for each entry other than `mimetype` and
//! `manifest.xml`, with `path`, `role`, `required` and `media-type`, in the order of the entries (2.9). **These are the manifest's own `manifest` and `part`**: they are
//! not the model vocabulary's (3.14), which stand in `document.xml`. A reader keeps what it does not know of a row of an entry it does not know (5.2): the other
//! attributes, as they are.

use diacraft_container::ContainerError;
use diacraft_container::xml::{El, Node, XmlLimits, to_bytes};
use diacraft_sheetxml::model::{Budget, URN};

use crate::error::NativeError;
use crate::kind::{FORMAT, Kind};
use crate::names::role;
use crate::xmlns::{Declarations, attr, is_ours, is_plain_attr};

/// The prefix a writer gives the namespace.
const PREFIX: &str = "diacraft";

/// The most bytes of a path, a role word or a media type in a row: an entry's name is at most 1,024 bytes, and the rest are words.
pub(crate) const WORD_MAX: usize = 1_024;

/// One row of the manifest: an entry, its role and what the manifest says of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The entry's name.
    pub path: String,
    /// The role word, compared byte for byte.
    pub role: String,
    /// `required="1"`: a reader that does not know the role refuses the file.
    pub required: bool,
    /// The media type of the entry's bytes: written for blobs and for extra parts.
    pub media_type: Option<String>,
    /// The other attributes of the row, with no prefix, in the order they were written: kept for an entry of a role this version does not know.
    pub extra: Vec<(String, String)>,
}

impl Row {
    /// A row of a role this version knows.
    pub(crate) fn known(path: &str, role: &str, required: bool, media_type: Option<&str>) -> Row {
        Row { path: path.to_owned(), role: role.to_owned(), required, media_type: media_type.map(str::to_owned), extra: Vec::new() }
    }
}

/// What a manifest says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    /// The version, `major.minor`.
    pub format: (u32, u32),
    /// The kind of the file.
    pub kind: Kind,
    /// Who wrote the file, as free text.
    pub written_by: Option<String>,
    /// The entries, in the order listed.
    pub rows: Vec<Row>,
}

/// A whole number written in decimal digits, up to nineteen of them (what a `u64` holds of any length of digits): a version's two numbers.
fn version_number(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 19 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `major.minor`, two whole numbers with a dot between them.
fn parse_version(text: &str) -> Option<(u64, u64)> {
    let (major, minor) = text.split_once('.')?;
    Some((version_number(major)?, version_number(minor)?))
}

/// A flag (3.1): `1` and `true` are on, `0` and `false` are off, and any other text is `None`.
pub(crate) fn parse_flag(text: &str) -> Option<bool> {
    match text {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// Reads a manifest from its tree. **A major version other than 1 is refused first**, in words that name it: the rest of a manifest of another major version is not
/// read. Every string a row holds is charged to `budget` as bytes, and at most `max_rows` rows are read: a manifest that lists more is a package of more entries than a
/// package may have (the rows are counted before the 50,001st is stored).
pub(crate) fn parse(root: &El, max_rows: usize, budget: &mut Budget) -> Result<Manifest, NativeError> {
    let damaged = |class: &'static str| NativeError::damaged(class);
    let own = Declarations::of(root).ok_or_else(|| damaged("manifest_malformed"))?;
    if root.local() != "manifest" || !is_ours(root, &own, None) {
        return Err(damaged("manifest_malformed"));
    }
    let (major, minor) = attr(root, "format").and_then(parse_version).ok_or_else(|| damaged("manifest_version"))?;
    if major != u64::from(FORMAT.0) {
        return Err(NativeError::NewerMajor { major, minor });
    }
    // A higher minor opens, and a minor too large for the number the version is held in is the largest it can be.
    let format = (FORMAT.0, u32::try_from(minor).unwrap_or(u32::MAX));
    let kind = attr(root, "kind").and_then(Kind::from_word).ok_or_else(|| damaged("manifest_kind"))?;
    let written_by = attr(root, "written-by").map(str::to_owned);
    if let Some(text) = &written_by {
        budget.charge_bytes(u64::try_from(text.len()).unwrap_or(u64::MAX))?;
    }
    let mut rows: Vec<Row> = Vec::new();
    for child in root.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }) {
        let theirs = Declarations::of(child).ok_or_else(|| damaged("manifest_malformed"))?;
        if child.local() != "part" || !is_ours(child, &theirs, Some(&own)) {
            continue;
        }
        if rows.len() >= max_rows {
            return Err(ContainerError::TooManyParts { limit: max_rows }.into());
        }
        rows.push(parse_row(child, budget)?);
    }
    Ok(Manifest { format, kind, written_by, rows })
}

fn parse_row(el: &El, budget: &mut Budget) -> Result<Row, NativeError> {
    let bad = || NativeError::damaged("manifest_row");
    let mut word = |text: &str| -> Result<(), NativeError> {
        if text.len() > WORD_MAX {
            return Err(bad());
        }
        budget.charge_bytes(u64::try_from(text.len()).unwrap_or(u64::MAX)).map_err(NativeError::from)
    };
    let path = attr(el, "path").ok_or_else(bad)?;
    let role_word = attr(el, "role").ok_or_else(bad)?;
    let required = match attr(el, "required") {
        None => false,
        Some(text) => parse_flag(text).ok_or_else(bad)?,
    };
    let media_type = attr(el, "media-type");
    word(path)?;
    word(role_word)?;
    if let Some(text) = media_type {
        word(text)?;
    }
    // What a row says beyond the four words is kept only for an entry of a role this version does not know: the attributes with no prefix, in order.
    let mut extra = Vec::new();
    if !role::is_known(role_word) {
        for (name, value) in &el.attrs {
            if !is_plain_attr(name) || matches!(name.as_str(), "path" | "role" | "required" | "media-type") {
                continue;
            }
            budget.charge_bytes(u64::try_from(name.len().saturating_add(value.len())).unwrap_or(u64::MAX).saturating_add(64))?;
            extra.push((name.clone(), value.clone()));
        }
    }
    Ok(Row { path: path.to_owned(), role: role_word.to_owned(), required, media_type: media_type.map(str::to_owned), extra })
}

/// The bytes of a manifest: the declaration, and the element with no layout white space, the rows in the order given, `required` written as `1` for a row that
/// has it and left out otherwise, and the attributes of a row in the order of the spec's table (`path`, `role`, `required`, `media-type`, then the others).
pub(crate) fn write(kind: Kind, written_by: &str, rows: &[Row]) -> Result<Vec<u8>, NativeError> {
    let mut root = El { name: format!("{PREFIX}:manifest"), attrs: vec![(format!("xmlns:{PREFIX}"), URN.to_owned())], children: Vec::new() };
    root.attrs.push(("format".to_owned(), format!("{}.{}", FORMAT.0, FORMAT.1)));
    root.attrs.push(("kind".to_owned(), kind.word().to_owned()));
    if !written_by.is_empty() {
        root.attrs.push(("written-by".to_owned(), written_by.to_owned()));
    }
    root.children.reserve(rows.len());
    for row in rows {
        let mut el = El { name: format!("{PREFIX}:part"), attrs: Vec::with_capacity(4 + row.extra.len()), children: Vec::new() };
        el.attrs.push(("path".to_owned(), row.path.clone()));
        el.attrs.push(("role".to_owned(), row.role.clone()));
        if row.required {
            el.attrs.push(("required".to_owned(), "1".to_owned()));
        }
        if let Some(media_type) = &row.media_type {
            el.attrs.push(("media-type".to_owned(), media_type.clone()));
        }
        el.attrs.extend(row.extra.iter().cloned());
        root.children.push(Node::El(el));
    }
    to_bytes(&root, &[], &[], true).map_err(|_| NativeError::Unwritable("the manifest cannot be written: a name or a word in it is not XML"))
}

/// The limits of the parse of the manifest or the state: the container's, at one tree's items and the depth of 512.
pub(crate) fn xml_limits(container: &diacraft_container::Limits, tree_items: u64) -> XmlLimits {
    XmlLimits { max_nodes: usize::try_from(tree_items).unwrap_or(usize::MAX).min(container.xml.max_nodes), ..container.xml.clone() }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_parse_is_held_to_the_items_a_tree_may_have_and_to_the_containers_depth_and_attributes() {
        let mut container = diacraft_container::Limits::default();
        container.xml.max_nodes = 5_000;
        container.xml.max_depth = 77;
        container.xml.max_attr_bytes = 4_321;
        // The pool's items when they are fewer than the container's, and the container's when they are not; depth and attribute size are the container's.
        let fewer = xml_limits(&container, 1_000);
        assert_eq!((fewer.max_nodes, fewer.max_depth, fewer.max_attr_bytes), (1_000, 77, 4_321));
        let more = xml_limits(&container, 1_000_000);
        assert_eq!(more.max_nodes, 5_000);
        assert_eq!(xml_limits(&container, u64::MAX).max_nodes, 5_000);
    }

    #[test]
    fn a_flag_is_one_or_true_zero_or_false_and_nothing_else() {
        for (text, want) in [("1", Some(true)), ("true", Some(true)), ("0", Some(false)), ("false", Some(false))] {
            assert_eq!(parse_flag(text), want, "{text}");
        }
        for text in ["", "2", "yes", "True", "FALSE", " 1", "1 ", "01", "-1"] {
            assert_eq!(parse_flag(text), None, "{text:?}");
        }
    }
}
