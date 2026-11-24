//! Every limit the reader applies, as a named constant with the id of the rule that sets it, and the [`Limits`] that carry them.
//!
//! Two kinds, kept apart on purpose:
//!
//! * **The spec's**: what a real package is allowed to be. The spec calls its numbers
//!   "suggested caps", a project choice made so that no real file is rejected: they sit well above the largest file the census
//!   saw (its column is quoted beside each constant).
//! * **Labelled choices**: the spec states no number, so this crate picked one, said why, and made it a field of [`Limits`].
//!
//! The caps of the archive and of an XML tree are the container crate's (`diacraft_container::limits`, re-exported here under the names
//! they always had), since an OpenDocument package has the same ones; the caps of the content types stream and of
//! relationships are this crate's. A limit is checked *before* anything is allocated for the thing it bounds, and the archive's own claims
//! about its entries (sizes, counts) are never trusted.

pub use diacraft_container::limits::{
    MAX_PACKAGE_BYTES, MAX_PART_BYTES, MAX_PART_NAME_BYTES, MAX_PARTS, MAX_RATIO, MAX_XML_ATTR_BYTES, MAX_XML_DEPTH, MAX_XML_NODES,
    XML_DEPTH_CEILING, XmlLimits,
};

/// Labelled choice: the largest `[Content_Types].xml` or `.rels` part. The spec states none for these two kinds of part, which
/// this crate parses itself on every open. **The largest the sample-file run found are 190,513 bytes (content types) and 221,863 bytes
/// (a relationships part)**, so 4 MiB is eighteen times that (it was 32 MiB).
pub const MAX_PACKAGE_XML_BYTES: u64 = 4 * 1024 * 1024;

/// Labelled choice: the most items in the XML tree of `[Content_Types].xml` or of one `.rels` part, counted as [`MAX_XML_NODES`] counts.
/// The spec states none. A row of either is an element, two attributes and the white space after it, so 100,000 rows (the most
/// [`MAX_CONTENT_TYPE_ENTRIES`] and [`MAX_RELATIONSHIPS`] allow) are 400,000 items; this is two and a half times that, and a quarter of
/// what the tree of any other part may be, so that the one tree this crate builds at every open is not the size of the largest.
pub const MAX_PACKAGE_XML_NODES: usize = 1_000_000;

/// Labelled choice: relationships in one relationships part. The spec states none (the census's maximum is under 2,000).
pub const MAX_RELATIONSHIPS: usize = 100_000;

/// Labelled choice: `Default` and `Override` rows in the content types stream. The spec allows rows with no part, so this is not
/// the part count; it is twice [`MAX_PARTS`].
pub const MAX_CONTENT_TYPE_ENTRIES: usize = 100_000;

/// What the reader accepts. [`Limits::default`] holds the constants above, so `Limits { max_parts: 10, ..Limits::default() }`
/// changes one and keeps the rest. No field is ever raised by the reader, and a field larger than the machine can hold is not
/// an error until memory runs out, which is reported as [`crate::OpcError::OutOfMemory`] and not as an abort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Most parts in the package: [`MAX_PARTS`].
    pub max_parts: usize,
    /// Most uncompressed bytes over the whole package: [`MAX_PACKAGE_BYTES`] (1 GiB; R23 suggests 8).
    pub max_package_bytes: u64,
    /// Most uncompressed bytes in one part: [`MAX_PART_BYTES`].
    pub max_part_bytes: u64,
    /// Largest ratio of one part's uncompressed size to its compressed size: [`MAX_RATIO`].
    pub max_ratio: u64,
    /// Longest part name in bytes: [`MAX_PART_NAME_BYTES`] (labelled choice).
    pub max_part_name_bytes: usize,
    /// Largest `[Content_Types].xml` or `.rels` part in bytes: [`MAX_PACKAGE_XML_BYTES`] (labelled choice).
    pub max_package_xml_bytes: u64,
    /// Most items in the XML tree of `[Content_Types].xml` or of a relationships part: [`MAX_PACKAGE_XML_NODES`] (labelled choice). The
    /// least of this and `xml.max_nodes` holds.
    pub max_package_xml_nodes: usize,
    /// Most relationships in one relationships part: [`MAX_RELATIONSHIPS`] (labelled choice).
    pub max_relationships: usize,
    /// Most `Default` and `Override` rows in the content types stream: [`MAX_CONTENT_TYPE_ENTRIES`] (labelled choice).
    pub max_content_type_entries: usize,
    /// The limits of every XML tree this crate builds.
    pub xml: XmlLimits,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_parts: MAX_PARTS,
            max_package_bytes: MAX_PACKAGE_BYTES,
            max_part_bytes: MAX_PART_BYTES,
            max_ratio: MAX_RATIO,
            max_part_name_bytes: MAX_PART_NAME_BYTES,
            max_package_xml_bytes: MAX_PACKAGE_XML_BYTES,
            max_package_xml_nodes: MAX_PACKAGE_XML_NODES,
            max_relationships: MAX_RELATIONSHIPS,
            max_content_type_entries: MAX_CONTENT_TYPE_ENTRIES,
            xml: XmlLimits::default(),
        }
    }
}

impl Limits {
    /// The limits of the tree of `[Content_Types].xml` or of a relationships part: `xml`, with the item limit the smaller of the two.
    pub(crate) fn package_xml(&self) -> XmlLimits {
        XmlLimits { max_nodes: self.xml.max_nodes.min(self.max_package_xml_nodes), ..self.xml.clone() }
    }

    /// The limits the archive is read with: the caps that are the container's, as they are here.
    pub(crate) fn container(&self) -> diacraft_container::Limits {
        diacraft_container::Limits {
            max_parts: self.max_parts,
            max_package_bytes: self.max_package_bytes,
            max_part_bytes: self.max_part_bytes,
            max_ratio: self.max_ratio,
            max_part_name_bytes: self.max_part_name_bytes,
            xml: self.xml.clone(),
        }
    }
}
