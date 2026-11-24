//! Every limit the reader applies, as a named constant with the id of the rule that sets it, and the [`Limits`] that carry them.
//!
//! The numbers are those of the VSDX package reader, and they hold for every package this crate
//! reads: gives an OpenDocument package the same caps and the same error classes. Two kinds, kept apart
//! on purpose:
//!
//! * **The spec's**: what a real package is allowed to be. The spec calls its numbers "suggested caps", a project choice made so
//!   that no real file is rejected: they sit well above the largest file the census saw (its column is quoted beside each constant).
//! * **Labelled choices**: the spec states no number, so this crate picked one, said why, and made it a field of [`Limits`].
//!
//! A limit is checked *before* anything is allocated for the thing it bounds, and the archive's own claims about its entries
//! (sizes, counts) are never trusted: see `zipread.rs`.

/// Parts in a package (the census's maximum is 1,925). A package with more is refused as `too_many_parts`. A part is a file of
/// the package that the caller counts: every file for [`crate::zip::Plain`], every file but the content types stream for OPC.
pub const MAX_PARTS: usize = 50_000;

/// Uncompressed bytes in a package, summed over all parts: **1 GiB, until R23 is amended.** R23 suggests 8 GiB; the
/// largest package in the census is 173.4 MB, so 1 GiB is 5.8 times that, and 8 GiB let a file of 8 MB ask for 8 GiB, which a web
/// build cannot address and most desktops cannot spare. A caller that wants more passes a larger [`Limits::max_package_bytes`]: a
/// file of about 1 MiB (1 GiB over [`MAX_RATIO`]) can still ask for all of this one.
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;

/// Uncompressed bytes in one part (maximum seen 44.1 MB; 15.6 MB for a page part).
pub const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

/// Compression ratio of one part, uncompressed over compressed (maximum seen 196). Deflate cannot go past 1,032, so this
/// refuses what is at or beyond the format's own limit, and the per-part and package caps are what bound the rest.
pub const MAX_RATIO: u64 = 1000;

/// Element depth of an XML part (maximum seen 26). The depth of nested `Shape` elements (maximum seen 11, cap 200) is
/// inside this one: an element in a `Shape` is deeper than the shape.
pub const MAX_XML_DEPTH: usize = 200;

/// Labelled choice: the longest part name, in bytes after percent-decoding. The spec states none; a zip name may be 65,535 bytes
/// and 50,000 of those would be 3 GB of names. Real names are under 100 bytes.
pub const MAX_PART_NAME_BYTES: usize = 1024;

/// Labelled choice: the most items in one XML tree, counting each element, text run, comment, processing instruction and
/// attribute as one. The spec states none. **The most the sample-file run found in one part is 1,481,648** (a master part of 17 MB), so
/// this is four times that (real files first; 8 million was a choice before any file had been read, and a
/// tree costs about 150 bytes per item, see the OPC crate's documentation).
pub const MAX_XML_NODES: usize = 6_000_000;

/// Labelled choice: the longest single attribute value or name, in bytes as written. The spec states none. **The longest the sample-file run
/// found is 43,350 bytes**, so this is twenty-four times that (it was 16 MiB). Formulas and values are a few hundred
/// bytes; the part's own cap is what bounds the rest, and this keeps one value from being the whole part.
pub const MAX_XML_ATTR_BYTES: usize = 1024 * 1024;

/// Labelled choice, and not a field of [`XmlLimits`]: no depth a caller asks for goes past this, because dropping or cloning a
/// tree recurses once per level. The spec's own cap is 200.
pub const XML_DEPTH_CEILING: usize = 512;

/// What the archive reader accepts. [`Limits::default`] holds the constants above, so `Limits { max_parts: 10, ..Limits::default() }`
/// changes one and keeps the rest. No field is ever raised by the reader, and a field larger than the machine can hold is not an error
/// until memory runs out, which is reported as [`crate::ContainerError::OutOfMemory`] and not as an abort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Most parts in the package: [`MAX_PARTS`]. The central directory may count at most twice this and sixteen rows (rows are
    /// parts, folders and the entries a caller does not count), and a count past that is refused before a row is read.
    pub max_parts: usize,
    /// Most uncompressed bytes over the whole package: [`MAX_PACKAGE_BYTES`] (1 GiB; R23 suggests 8).
    pub max_package_bytes: u64,
    /// Most uncompressed bytes in one part: [`MAX_PART_BYTES`]. A caller's judge may give an entry a smaller cap.
    pub max_part_bytes: u64,
    /// Largest ratio of one part's uncompressed size to its compressed size: [`MAX_RATIO`].
    pub max_ratio: u64,
    /// Longest part name in bytes: [`MAX_PART_NAME_BYTES`] (labelled choice). A name more than three times this as written is refused
    /// before it is copied.
    pub max_part_name_bytes: usize,
    /// The limits of every XML tree a caller builds from the package.
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
            xml: XmlLimits::default(),
        }
    }
}

/// What an XML tree may be. [`XmlLimits::default`] holds the constants above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlLimits {
    /// Deepest element nesting: [`MAX_XML_DEPTH`], and never more than [`XML_DEPTH_CEILING`] whatever this says.
    pub max_depth: usize,
    /// Most items in the tree: [`MAX_XML_NODES`] (labelled choice).
    pub max_nodes: usize,
    /// Longest attribute name or value in bytes: [`MAX_XML_ATTR_BYTES`] (labelled choice).
    pub max_attr_bytes: usize,
}

impl Default for XmlLimits {
    fn default() -> Self {
        XmlLimits { max_depth: MAX_XML_DEPTH, max_nodes: MAX_XML_NODES, max_attr_bytes: MAX_XML_ATTR_BYTES }
    }
}
