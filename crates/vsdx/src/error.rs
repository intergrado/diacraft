//! Why a file is refused, as data, with a class word that a harness compares instead of wording.
//!
//! Three families, each with its class words: the container's (`opc`'s own: `not_zip`, `zip_corrupt`, `path_traversal`, ...), what this
//! crate adds (`no_drawing_parts`, a required part that is not XML we accept: `xml_malformed`, `xml_entity_refused`, `xml_too_complex`), and a
//! cap passed (`too_many_pages`, `too_many_styles`, ...). [`VsdxError::kind`] says which of three things a person is told, as `opc`'s
//! [`ErrorKind`] does for the container. A message never repeats text from the file raw: it holds numbers, part names (escaped) and words.

use std::fmt;

use diacraft_opc::{ErrorKind, OpcError, XmlError};
use diacraft_sheetxml::{LimitError, LimitKind as SheetKind};

/// A cap this reader applies to what it builds. Each has a class word and the number that was passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    Pages,
    Styles,
    Masters,
    /// Shapes in one page or master part, nested ones counted.
    ShapesInAPart,
    /// Shapes in the whole document.
    Shapes,
    /// Levels of nested `Shape` elements.
    ShapeDepth,
    /// Cells in the whole document.
    Cells,
    /// Cells directly under one sheet, section or row.
    CellsInOnePlace,
    SectionsInASheet,
    RowsInASection,
    /// A cell, section or row name past `MAX_NAME_BYTES`.
    NameBytes,
    /// XML items (elements, attributes and text nodes) modelled or kept, over the whole document.
    Items,
    /// Bytes of XML parts parsed, over the whole document.
    XmlBytes,
    /// Entries in the colour table.
    Colors,
    /// Bytes of memory that reading the file would hold at once: the file, the package inflated, the trees of the parts and the model built from them, charged
    /// by the real size of each before it is made (`Limits::max_memory`).
    Memory,
}

/// The caps the sheet codec applies are its own; the class words and the wording are written there, once.
impl From<SheetKind> for LimitKind {
    fn from(kind: SheetKind) -> LimitKind {
        match kind {
            SheetKind::Cells => LimitKind::Cells,
            SheetKind::CellsInOnePlace => LimitKind::CellsInOnePlace,
            SheetKind::SectionsInASheet => LimitKind::SectionsInASheet,
            SheetKind::RowsInASection => LimitKind::RowsInASection,
            SheetKind::NameBytes => LimitKind::NameBytes,
            SheetKind::Items => LimitKind::Items,
            SheetKind::Bytes => LimitKind::Memory,
        }
    }
}

impl LimitKind {
    /// The class word.
    pub fn class(self) -> &'static str {
        match self {
            LimitKind::Pages => "too_many_pages",
            LimitKind::Styles => "too_many_styles",
            LimitKind::Masters => "too_many_masters",
            LimitKind::ShapesInAPart => "too_many_shapes_in_a_part",
            LimitKind::Shapes => "too_many_shapes",
            LimitKind::ShapeDepth => "shapes_too_deep",
            LimitKind::Cells => SheetKind::Cells.class(),
            LimitKind::CellsInOnePlace => SheetKind::CellsInOnePlace.class(),
            LimitKind::SectionsInASheet => SheetKind::SectionsInASheet.class(),
            LimitKind::RowsInASection => SheetKind::RowsInASection.class(),
            LimitKind::NameBytes => SheetKind::NameBytes.class(),
            LimitKind::Items => SheetKind::Items.class(),
            LimitKind::XmlBytes => "too_much_xml",
            LimitKind::Colors => "too_many_colors",
            LimitKind::Memory => SheetKind::Bytes.class(),
        }
    }

    /// What the cap counts, in words: `pages`, `shapes`, `cells in one sheet, section or row`.
    pub fn what(self) -> &'static str {
        match self {
            LimitKind::Pages => "pages",
            LimitKind::Styles => "styles",
            LimitKind::Masters => "masters",
            LimitKind::ShapesInAPart => "shapes in one page or master part",
            LimitKind::Shapes => "shapes",
            LimitKind::ShapeDepth => "levels of nested shapes",
            LimitKind::Cells => SheetKind::Cells.what(),
            LimitKind::CellsInOnePlace => SheetKind::CellsInOnePlace.what(),
            LimitKind::SectionsInASheet => SheetKind::SectionsInASheet.what(),
            LimitKind::RowsInASection => SheetKind::RowsInASection.what(),
            LimitKind::NameBytes => SheetKind::NameBytes.what(),
            LimitKind::Items => SheetKind::Items.what(),
            LimitKind::XmlBytes => "bytes of XML in the parts of the document",
            LimitKind::Colors => "entries in the colour table",
            LimitKind::Memory => SheetKind::Bytes.what(),
        }
    }
}

/// Why a file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VsdxError {
    /// The container: not a package, damaged, or past one of `opc`'s limits.
    Package(OpcError),
    /// The package has no VSDX document: no document relationship, its target is not a part, or the part's root is not the document element
    /// The words say which.
    NoDrawingParts(&'static str),
    /// A part the reader needs whole (the document part, the pages index, the masters index) is not XML it accepts: not well-formed, a DTD
    /// or an entity, or past an XML limit.
    Xml { part: String, error: XmlError },
    /// A cap passed: the cap and its number.
    Limit { kind: LimitKind, max: u64 },
    /// The document the reader built fails the model's own check of its caps (`Document::check_limits`). The reader keeps every cap as it
    /// builds, so this is a bug in the reader and not a property of the file; it is refused and not handed out.
    Model(diacraft_doc::DocError),
    /// A thread that was working for the read stopped (a panic in it), so the file was not read: a bug, and never a property of the file.
    Worker(&'static str),
}

impl VsdxError {
    /// The class word: `opc`'s for the container, `no_drawing_parts`, the XML ones, or the cap's.
    pub fn class(&self) -> &'static str {
        match self {
            VsdxError::Package(e) => e.class(),
            VsdxError::NoDrawingParts(_) => "no_drawing_parts", // brand-ok: the class word of the format spec
            VsdxError::Xml { error, .. } => error.class(),
            VsdxError::Limit { kind, .. } => kind.class(),
            VsdxError::Model(_) => "model_limit",
            VsdxError::Worker(_) => "read_worker_failed",
        }
    }

    /// Which of three things a person is told ([`ErrorKind`]): not a package, too big for the limits, or damaged.
    pub fn kind(&self) -> ErrorKind {
        match self {
            VsdxError::Package(e) => e.kind(),
            VsdxError::NoDrawingParts(_) => ErrorKind::NotAPackage,
            VsdxError::Xml { error, .. } => match error {
                XmlError::Malformed { .. } | XmlError::EntityRefused { .. } => ErrorKind::Damaged,
                XmlError::TooDeep { .. } | XmlError::TooManyNodes { .. } | XmlError::AttributeTooLarge { .. } => ErrorKind::TooBig,
            },
            VsdxError::Limit { .. } | VsdxError::Model(_) => ErrorKind::TooBig,
            VsdxError::Worker(_) => ErrorKind::Damaged,
        }
    }
}

impl From<LimitError> for VsdxError {
    fn from(e: LimitError) -> VsdxError {
        VsdxError::Limit { kind: e.kind.into(), max: e.max }
    }
}

impl From<OpcError> for VsdxError {
    fn from(e: OpcError) -> VsdxError {
        VsdxError::Package(e)
    }
}

impl fmt::Display for VsdxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VsdxError::Package(e) => write!(f, "{e}"),
            VsdxError::NoDrawingParts(why) => write!(f, "this is not a drawing file: {why}"),
            VsdxError::Xml { part, error } => write!(f, "\"{}\": {error}", part.escape_debug()),
            VsdxError::Limit { kind: LimitKind::Memory, max } => {
                write!(f, "reading the file would take more than {max} bytes of memory: it is too large to read here")
            }
            VsdxError::Limit { kind, max } => write!(f, "the file holds more than {max} {}", kind.what()),
            VsdxError::Model(e) => write!(f, "the document passes a limit of the model: {e}"),
            VsdxError::Worker(why) => write!(f, "the file was not read: {why}"),
        }
    }
}

impl std::error::Error for VsdxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VsdxError::Package(e) => Some(e),
            VsdxError::Xml { error, .. } => Some(error),
            VsdxError::Model(e) => Some(e),
            _ => None,
        }
    }
}
