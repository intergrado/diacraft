//! Why a package is refused, and what was odd about one that was read, as data with a class word a harness compares instead of wording.
//!
//! The classes are: the container's (the VSDX reader's `not_zip`, `zip_corrupt`, `path_traversal`,
//! `invalid_part_name`, `zip_ratio_exceeded`, `part_too_large`, `too_many_parts`, `package_too_large`, `out_of_memory`,
//! `unsupported_compression`, `duplicate_part`, `xml_malformed`, `xml_entity_refused`, `xml_too_complex`), and ODF's own (`missing_manifest`,
//! `manifest_malformed`, `not_a_drawing`, `no_drawing_content`, `encrypted_package`). A cap of this crate's passed has a class of its own
//! ([`LimitKind`]). The warning classes `mimetype_irregular`, `manifest_incomplete` and `manifest_lists_missing_file` are the spec's;
//! the others are this crate's and say so.
//!
//! Names, media types and messages come from the file, which is hostile: [`Display`](fmt::Display) escapes them and cuts them short, so an
//! error shown in a log or a dialog cannot carry control characters or terminal escapes, nor be as long as the file.

use std::fmt;

use diacraft_container::{ContainerError, XmlError};

/// The longest piece of the file a message repeats, in characters.
const MOST_SHOWN: usize = 64;

/// `text` cut to [`MOST_SHOWN`] characters, with `…` when it was cut.
pub(crate) fn shown(text: &str) -> String {
    let mut out: String = text.chars().take(MOST_SHOWN).collect();
    if text.chars().nth(MOST_SHOWN).is_some() {
        out.push('…');
    }
    out
}

/// A cap of this crate's that a package passed. Each has a class word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// Rows of the manifest ([`crate::Limits::max_manifest_entries`]).
    ManifestEntries,
    /// Bytes of XML parsed by one read or one side of a comparison ([`crate::Limits::max_xml_bytes`]).
    XmlBytes,
    /// Pages of the drawing, background pages included ([`crate::Limits::max_pages`]). The VSDX reader's class word.
    Pages,
    /// Style sheets ([`crate::Limits::max_styles`]). The VSDX reader's class word.
    Styles,
    /// Shapes on one page, those kept unmodelled included ([`crate::Limits::max_shapes_per_page`]).
    ShapesInAPage,
    /// Shapes in the whole document ([`crate::Limits::max_shapes`]). The VSDX reader's class word.
    Shapes,
    /// Cells in the whole document ([`crate::Limits::max_cells`]). The VSDX reader's class word.
    Cells,
    /// Items kept in the model, over the whole document ([`crate::Limits::max_items`]). The VSDX reader's class word.
    Items,
    /// Bytes of text in the shapes, over the whole document ([`crate::Limits::max_text_bytes`]).
    TextBytes,
}

impl LimitKind {
    /// The class word.
    pub fn class(self) -> &'static str {
        match self {
            LimitKind::ManifestEntries => "too_many_manifest_entries",
            LimitKind::XmlBytes => "too_much_xml",
            LimitKind::Pages => "too_many_pages",
            LimitKind::Styles => "too_many_styles",
            LimitKind::ShapesInAPage => "too_many_shapes_in_a_page",
            LimitKind::Shapes => "too_many_shapes",
            LimitKind::Cells => "too_many_cells",
            LimitKind::Items => "too_many_items",
            LimitKind::TextBytes => "too_much_text",
        }
    }

    /// What the cap counts, in words.
    pub fn what(self) -> &'static str {
        match self {
            LimitKind::ManifestEntries => "rows in the manifest",
            LimitKind::XmlBytes => "bytes of XML in the parts read",
            LimitKind::Pages => "pages",
            LimitKind::Styles => "styles",
            LimitKind::ShapesInAPage => "shapes on one page",
            LimitKind::Shapes => "shapes",
            LimitKind::Cells => "cells",
            // A path point is a geometry row and its two cells, 29 items (`read::cost::ROW` and `ITEMS_PER_CELL`; a test holds the figure; it was 28
            // until the model's rows gained a slot for their extension data), so the default cap of 20,000,000 holds about 690,000 of
            // them (`limits::MAX_ITEMS`).
            LimitKind::Items => {
                "items kept in the document (shapes, cells and text count, and each path point counts as 29, so about 690000 path points fill the default limit)"
            }
            LimitKind::TextBytes => "bytes of text in the shapes",
        }
    }
}

/// What a caller shows a person, in three values, as the VSDX reader's `ErrorKind` does: [`OdgError::class`] is for harnesses that compare
/// words; this is for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Not something this reader reads as a drawing: not a ZIP, not an OpenDocument package, not a drawing, protected by a password, or
    /// compressed in a way the format does not allow.
    NotAPackage,
    /// A package, but past a limit. A larger [`Limits`](crate::Limits) may open it.
    TooBig,
    /// A package that contradicts itself or the format: damaged, truncated, or holding a name or XML that is refused.
    Damaged,
}

/// What a package that holds no drawing lacks ([`OdgError::NoDrawingContent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// The package has no `content.xml`.
    ContentPart,
    /// `content.xml` has no `office:body`, or its body holds no `office:drawing`.
    DrawingBody,
    /// A flat drawing has no `office:body`, or its body holds no `office:drawing`.
    FlatBody,
}

/// Why a package was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdgError {
    /// The archive: not a ZIP, damaged, a refused name, or past one of the container's limits. The class is the container's.
    Container(ContainerError),
    /// No `META-INF/manifest.xml`. Class `missing_manifest`.
    MissingManifest,
    /// A manifest that is not well-formed, or whose root is not `manifest:manifest` in the manifest's namespace. Class
    /// `manifest_malformed`. A manifest with a DTD or past an XML limit is [`OdgError::Xml`], with the XML class.
    ManifestMalformed { detail: String },
    /// The package's media type (the manifest's root row, else `mimetype`) is not a drawing's or a drawing template's. Class
    /// `not_a_drawing`. `media_type` is what the package says, cut short.
    NotADrawing { media_type: String },
    /// No `content.xml`, or a `content.xml` whose body holds no `office:drawing`, so no pages. Class
    /// `no_drawing_content` for both; [`Missing`] says which, and the message fits it. The package reader keeps every
    /// part as bytes and does not give this; the drawing's reader does.
    NoDrawingContent { missing: Missing },
    /// The manifest marks a file as encrypted or holds keys for recipients, or an entry is flagged encrypted in the archive. Class
    /// `encrypted_package`. Refused before any entry but the manifest is inflated.
    EncryptedPackage,
    /// An XML part this crate parses could not be read: a DTD or an entity, or past an XML limit. The class is the XML one.
    Xml { part: String, error: XmlError },
    /// A cap of this crate's passed: the cap and its number.
    Limit { kind: LimitKind, max: u64 },
    /// An XML file that is read as a flat drawing and whose root is not `office:document` in the office namespace. Class `xml_malformed`, as R5
    /// says of a root that is not the expected one. `root` is the name the file gives it, cut short.
    UnexpectedFlatRoot { root: String },
    /// One of the four XML parts has a root other than its fixed one (`office:document-content`, `office:document-styles`,
    /// `office:document-meta`, `office:document-settings`, matched by namespace). Class `xml_malformed`, as R5 says.
    UnexpectedRoot { part: &'static str },
    /// A comparison of two drawings would have to make more than `max` bytes of copies on one side to put the drawing in the form of the other (`compare_content`):
    /// a picture a package names, once for each frame that names it (counted as the base64 it makes), the declarations of a prefix given to each
    /// element that leaves the root of its part, the picture data a flat file holds, and the attributes of a part's root put on the joined one. Class
    /// `too_much_xml`. Refused before any copy is made.
    CompareCopies { max: u64 },
    /// More of something than allows in one place: path commands in a page, or pairs in one `draw:points`. Class
    /// `xml_too_complex`, as E7 says. `what` says what, `max` is the cap.
    TooComplex { what: &'static str, max: u64 },
    /// The document the reader built is past a cap of the model: a write the model refused, or `Document::check_limits` afterwards. The
    /// reader checks each of the model's caps before it stores what the cap counts (sections, rows, cells in one place by way of tab stops,
    /// pages, styles, shapes), so this answer means a cap it missed. Class `model_limit`, the VSDX reader's word.
    Model(diacraft_doc::DocError),
}

impl OdgError {
    /// The class word: the container's for the archive, the spec's ODF words, the XML ones, or the cap's.
    pub fn class(&self) -> &'static str {
        match self {
            OdgError::Container(e) => e.class(),
            OdgError::MissingManifest => "missing_manifest",
            OdgError::ManifestMalformed { .. } => "manifest_malformed",
            OdgError::NotADrawing { .. } => "not_a_drawing",
            OdgError::NoDrawingContent { .. } => "no_drawing_content",
            OdgError::EncryptedPackage => "encrypted_package",
            OdgError::Xml { error, .. } => error.class(),
            OdgError::Limit { kind, .. } => kind.class(),
            OdgError::CompareCopies { .. } => "too_much_xml",
            OdgError::UnexpectedRoot { .. } | OdgError::UnexpectedFlatRoot { .. } => "xml_malformed",
            OdgError::TooComplex { .. } => "xml_too_complex",
            OdgError::Model(_) => "model_limit",
        }
    }

    /// Which of three things a person is told ([`ErrorKind`]).
    pub fn kind(&self) -> ErrorKind {
        match self {
            OdgError::Container(e) => match e {
                ContainerError::NotZip | ContainerError::UnsupportedCompression { .. } | ContainerError::EncryptedPackage { .. } => {
                    ErrorKind::NotAPackage
                }
                ContainerError::ZipRatioExceeded { .. }
                | ContainerError::PartTooLarge { .. }
                | ContainerError::PackageTooLarge { .. }
                | ContainerError::TooManyParts { .. }
                | ContainerError::OutOfMemory { .. } => ErrorKind::TooBig,
                ContainerError::Xml { error, .. } => xml_kind(error),
                ContainerError::ZipCorrupt { .. }
                | ContainerError::PathTraversal { .. }
                | ContainerError::InvalidPartName { .. }
                | ContainerError::DuplicatePart { .. } => ErrorKind::Damaged,
            },
            OdgError::MissingManifest | OdgError::NotADrawing { .. } | OdgError::EncryptedPackage | OdgError::UnexpectedFlatRoot { .. } => {
                ErrorKind::NotAPackage
            }
            OdgError::ManifestMalformed { .. } | OdgError::NoDrawingContent { .. } | OdgError::UnexpectedRoot { .. } => ErrorKind::Damaged,
            OdgError::Xml { error, .. } => xml_kind(error),
            OdgError::Limit { .. } | OdgError::CompareCopies { .. } | OdgError::TooComplex { .. } | OdgError::Model(_) => ErrorKind::TooBig,
        }
    }
}

fn xml_kind(error: &XmlError) -> ErrorKind {
    match error {
        XmlError::Malformed { .. } | XmlError::EntityRefused { .. } => ErrorKind::Damaged,
        XmlError::TooDeep { .. } | XmlError::TooManyNodes { .. } | XmlError::AttributeTooLarge { .. } => ErrorKind::TooBig,
    }
}

impl From<ContainerError> for OdgError {
    /// The archive's error as this crate's. An entry the archive flags as encrypted is the same refusal as a manifest that says so: a person
    /// is told the drawing is protected, whichever said it.
    fn from(error: ContainerError) -> Self {
        match error {
            ContainerError::EncryptedPackage { .. } => OdgError::EncryptedPackage,
            other => OdgError::Container(other),
        }
    }
}

impl fmt::Display for OdgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OdgError::Container(e) => write!(f, "{e}"),
            OdgError::MissingManifest => write!(f, "this is not an OpenDocument package: it has no META-INF/manifest.xml"),
            OdgError::ManifestMalformed { detail } => write!(f, "the package's manifest cannot be read: {}", shown(detail).escape_debug()),
            OdgError::NotADrawing { media_type } => {
                write!(f, "this is not a drawing: the package says it is \"{}\"", shown(media_type).escape_debug())
            }
            OdgError::NoDrawingContent { missing: Missing::ContentPart } => write!(f, "the package holds no drawing: it has no content.xml"),
            OdgError::NoDrawingContent { missing: Missing::DrawingBody } => {
                write!(f, "the package holds no drawing: the body of its content.xml holds no office:drawing")
            }
            OdgError::NoDrawingContent { missing: Missing::FlatBody } => {
                write!(f, "the document holds no drawing: it has no office:body, or its body holds no office:drawing")
            }
            OdgError::UnexpectedFlatRoot { root } => {
                write!(f, "this XML file is not an OpenDocument drawing: its root is \"{}\", not office:document", shown(root).escape_debug())
            }
            OdgError::EncryptedPackage => {
                write!(f, "the drawing is password-protected, and DiaCraft cannot open protected drawings yet")
            }
            OdgError::Xml { part, error } => write!(f, "\"{}\": {error}", shown(part).escape_debug()),
            OdgError::Limit { kind, max } => write!(f, "the package holds more than {max} {}", kind.what()),
            OdgError::CompareCopies { max } => write!(
                f,
                "comparing these drawings would have to copy more than {max} bytes (a picture named by many frames, a long address given to many elements, or picture data) to put one in the form of the other"
            ),
            OdgError::UnexpectedRoot { part } => write!(f, "\"{part}\" does not hold what an OpenDocument package's {part} holds"),
            OdgError::TooComplex { what, max } => write!(f, "the drawing holds more than {max} {what}"),
            OdgError::Model(e) => write!(f, "the drawing is past a limit of the document: {e}"),
        }
    }
}

impl std::error::Error for OdgError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OdgError::Container(e) => Some(e),
            OdgError::Xml { error, .. } => Some(error),
            OdgError::Model(e) => Some(e),
            _ => None,
        }
    }
}

/// Why a package's `mimetype` is not what R2 asks of it. A package with any of these is read, with one [`Warning::MimetypeIrregular`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimetypeFault {
    /// There is no `mimetype` entry.
    Absent,
    /// It is not the archive's first entry (its local header is not at byte 0).
    NotFirst,
    /// It is deflated, not stored.
    Compressed,
    /// Its local header has an extra field, so its bytes are not at byte 38.
    ExtraField,
    /// It is longer than [`MAX_MIMETYPE_BYTES`](crate::limits::MAX_MIMETYPE_BYTES), or not printable ASCII: it is not read as a media type.
    Unreadable,
    /// It names another media type than the manifest's root row, which wins.
    DisagreesWithManifest,
}

impl MimetypeFault {
    /// A word for the fault.
    pub fn word(self) -> &'static str {
        match self {
            MimetypeFault::Absent => "absent",
            MimetypeFault::NotFirst => "not_first",
            MimetypeFault::Compressed => "compressed",
            MimetypeFault::ExtraField => "extra_field",
            MimetypeFault::Unreadable => "unreadable",
            MimetypeFault::DisagreesWithManifest => "disagrees_with_manifest",
        }
    }
}

/// A thing found while reading that is not an error. The package is read whole all the same, and the reader says what it saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Bytes after the archive's end record and its comment, ignored (the container's R31 rule): how many. Class `bytes_after_end_record`
    /// (this crate's).
    BytesAfterEndRecord { count: usize },
    /// `mimetype` is not what R2 asks: each fault found, in the order of [`MimetypeFault`]. Class `mimetype_irregular`. One warning,
    /// whatever the number of faults.
    MimetypeIrregular { faults: Vec<MimetypeFault> },
    /// A file of the package the manifest does not list: `mimetype` and files under `META-INF/` need no row. Class
    /// `manifest_incomplete`. The first [`Limits::max_report_entries`](crate::Limits) are named, and the rest counted in
    /// [`Warning::MoreManifestIncomplete`].
    ManifestIncomplete { name: String },
    /// More files the manifest does not list than are named one by one. Class `manifest_incomplete`.
    MoreManifestIncomplete { count: usize },
    /// A manifest row for a file, or for a folder, that the package does not have. Class `manifest_lists_missing_file`. Named and
    /// counted as [`Warning::ManifestIncomplete`] is.
    ManifestListsMissingFile { path: String },
    /// More rows for missing files than are named one by one. Class `manifest_lists_missing_file`.
    MoreManifestListsMissingFile { count: usize },
    /// Two manifest rows with one `manifest:full-path`: the first is used. Class `manifest_duplicate_entry` (this crate's; R8 says each
    /// file is listed once).
    ManifestDuplicateEntry { path: String },
    /// More rows listed twice than are named one by one: how many. Class `manifest_duplicate_entry`.
    MoreManifestDuplicateEntries { count: usize },
    /// Manifest rows with no `manifest:full-path` (in the manifest's namespace): skipped, and how many. Class `manifest_entry_incomplete`
    /// (this crate's; R9 requires the attribute).
    ManifestEntryIncomplete { count: usize },
    /// The manifest has no root row (`/`) though `mimetype` exists, which R8 requires. Class `manifest_no_root_entry` (this crate's).
    ManifestNoRootEntry,
    /// Neither the manifest's root row nor `mimetype` says what the package is: it is read, and what its content says decides. Class
    /// `media_type_unknown` (this crate's).
    MediaTypeUnknown,
    /// A part whose tree a read keeps (`content.xml`, `styles.xml`) did not parse, so it was not looked through for macros. Its bytes are
    /// kept all the same, and its error is `Package::tree_error`; the drawing's reader refuses it. Class `part_not_scanned` (this crate's).
    PartNotScanned { part: String, class: &'static str },
}

impl Warning {
    /// The class word.
    pub fn class(&self) -> &'static str {
        match self {
            Warning::BytesAfterEndRecord { .. } => "bytes_after_end_record",
            Warning::MimetypeIrregular { .. } => "mimetype_irregular",
            Warning::ManifestIncomplete { .. } | Warning::MoreManifestIncomplete { .. } => "manifest_incomplete",
            Warning::ManifestListsMissingFile { .. } | Warning::MoreManifestListsMissingFile { .. } => "manifest_lists_missing_file",
            Warning::ManifestDuplicateEntry { .. } | Warning::MoreManifestDuplicateEntries { .. } => "manifest_duplicate_entry",
            Warning::ManifestEntryIncomplete { .. } => "manifest_entry_incomplete",
            Warning::ManifestNoRootEntry => "manifest_no_root_entry",
            Warning::MediaTypeUnknown => "media_type_unknown",
            Warning::PartNotScanned { .. } => "part_not_scanned",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::BytesAfterEndRecord { count } => write!(f, "{count} bytes after the end record of the archive were ignored"),
            Warning::MimetypeIrregular { faults } => {
                let words: Vec<&str> = faults.iter().map(|fault| fault.word()).collect();
                write!(f, "the package's mimetype entry is not first, stored and plain as it should be ({})", words.join(", "))
            }
            Warning::ManifestIncomplete { name } => write!(f, "the manifest does not list \"{}\"", shown(name).escape_debug()),
            Warning::MoreManifestIncomplete { count } => write!(f, "the manifest does not list {count} more files"),
            Warning::ManifestListsMissingFile { path } => {
                write!(f, "the manifest lists \"{}\", which the package does not have", shown(path).escape_debug())
            }
            Warning::MoreManifestListsMissingFile { count } => write!(f, "the manifest lists {count} more files the package does not have"),
            Warning::ManifestDuplicateEntry { path } => {
                write!(f, "the manifest lists \"{}\" twice: the first row is used", shown(path).escape_debug())
            }
            Warning::MoreManifestDuplicateEntries { count } => write!(f, "the manifest lists {count} more files twice"),
            Warning::ManifestEntryIncomplete { count } => write!(f, "{count} rows of the manifest name no file and were skipped"),
            Warning::ManifestNoRootEntry => write!(f, "the manifest has no row for the package itself"),
            Warning::MediaTypeUnknown => write!(f, "the package does not say what kind of document it is"),
            Warning::PartNotScanned { part, class } => {
                write!(f, "\"{}\" was not looked through for macros: {class}", shown(part).escape_debug())
            }
        }
    }
}
