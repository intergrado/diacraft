//! Shapes: a `Shape` element of a page or master part, with its sheet, its text and its members.
//!
//! The sheet is the shape. Position, size, rotation, geometry, connection points, text transform, line and fill are all
//! cells of [`Shape::sheet`]; nothing here caches any of them (`AGENTS.md`: no second source of truth). What a shape holds
//! besides its sheet is what the file holds outside the cells: its identity, its ties to a master, its text and its members.

use std::sync::Arc;

use crate::odf::{OdfShape, OdfText};
use crate::package::{Ext, Kept};
use crate::raw::{Passthrough, RawAttrs, RawNode};
use crate::shared::SharedVec;
use crate::sheet::Sheet;

/// Most levels of nested `Shape` elements: the cap (median 2, 99th percentile 8, maximum seen 11). Every
/// walk of a shape tree in this crate stops at it, and the reader of a file refuses a part nested deeper.
pub const MAX_SHAPE_DEPTH: usize = 200;

/// The shapes of a page, a master or a group, in file order: a later shape draws in front of an earlier one
/// So the order is data. A persistent list, so editing one shape of ten thousand copies a few
/// hundred reference counts and not the page: see [`SharedVec`].
pub type ShapeList = SharedVec<Arc<Shape>>;

/// The `Type` of a shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ShapeType {
    Shape,
    Group,
    Foreign,
    Guide,
    /// No `Type`: it occurs with `Del="1"` (68 files in the census), and MS-VSDX does not say what it means.
    #[default]
    Unspecified,
    /// A value no spec lists, kept as the text it was.
    Other(Arc<str>),
}

impl ShapeType {
    /// The `Type` attribute's value, if there is one.
    pub fn read(text: Option<&str>) -> ShapeType {
        match text {
            None => ShapeType::Unspecified,
            Some("Shape") => ShapeType::Shape,
            Some("Group") => ShapeType::Group,
            Some("Foreign") => ShapeType::Foreign,
            Some("Guide") => ShapeType::Guide,
            Some(other) => ShapeType::Other(Arc::from(other)),
        }
    }

    /// The text for the attribute, or `None` when there was none.
    pub fn text(&self) -> Option<&str> {
        match self {
            ShapeType::Shape => Some("Shape"),
            ShapeType::Group => Some("Group"),
            ShapeType::Foreign => Some("Foreign"),
            ShapeType::Guide => Some("Guide"),
            ShapeType::Unspecified => None,
            ShapeType::Other(text) => Some(text),
        }
    }
}

/// One piece of a shape's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPart {
    /// Characters, exactly as the file had them, blanks and line breaks included (`xml:space="preserve"`, R4 of the document spec).
    Chars(Arc<str>),
    /// `<cp IX="..."/>`: from here on, runs use `Character` row `IX`, until the next one.
    Cp(u32),
    /// `<pp IX="..."/>`: `Paragraph` row `IX`.
    Pp(u32),
    /// `<tp IX="..."/>`: `Tabs` row `IX`.
    Tp(u32),
    /// `<fld IX="...">text</fld>`: `Field` row `IX`, and the displayed form of the field.
    Fld { ix: u32, text: Arc<str> },
    /// Anything else inside `Text`: a marker whose `IX` is not a number, an unknown element. **VSDX's** markup, like every `raw` and `passthrough`.
    Raw(RawNode),
    /// A node the `.odg` reader kept inside the text (a bookmark, a field it does not map): **ODF's** markup, never written into VSDX (
    /// fills it; until then nothing does). Writers skip it, and rendering and text extraction treat it as they treat [`TextPart::Raw`]. **Boxed**, so that the
    /// part is the size it was (48 bytes) and not 56: it is rare, and a text of a million markers is a million parts.
    OdfRaw(Box<RawNode>),
    /// A node a newer `.diacraft` kept inside the text and this build does not know (filled by the decoder). No foreign writer emits it, and
    /// rendering and text extraction treat it as they treat [`TextPart::Raw`].
    NativeRaw(Box<RawNode>),
}

/// A shape's `Text` element: mixed content. The markers and the rows they name must stay in
/// step, so the text is kept in pieces, not flattened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    pub parts: Vec<TextPart>,
    /// The text's ODF data: the style each paragraph and run named, held per paragraph and per run with a stamp of its own, and what its markup held that
    /// nothing else does, tagged by format (see [`crate::package`]). Filled by the `.odg` reader.
    ///
    /// **It is the file's paragraphs, in the file's order** (an [`OdfParagraph`](crate::OdfParagraph) for each `text:p` the file had), and not the paragraphs
    /// of these parts: the writer matches the file's paragraphs to the text's by content when it saves, as its paragraph plan does (`write/runs.rs`, `plan`:
    /// the same characters and the same rows, at the head and the tail, then by the characters; two lists of one length are paired paragraph by paragraph), so a
    /// newline typed before a held paragraph leaves its style on it, and an edit of one paragraph leaves the others'. So **an edit of a text carries the slot
    /// forward** (`shape.set_text`, a range of it, and `text.format` do), and a text made new (a shape operation's, a text box's) starts with an empty one: nothing
    /// held is taken for what is new.
    pub ext: Ext<OdfText>,
}

/// A picture held by the package, as the reader found it through a relationship, shared by every shape that shows it.
/// The bytes are the part's, unchanged; the model never decodes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The part's name in the package.
    pub part: Arc<str>,
    pub content_type: Arc<str>,
    pub bytes: Arc<[u8]>,
}

/// `ForeignData`: what a `Foreign` shape shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignData {
    /// Every attribute, verbatim and in order (`ForeignType`, `ObjectType`, `ShowAsIcon`, `ObjectWidth`, `ObjectHeight`,
    /// `MappingMode`, `ExtentX`, `ExtentY`, `CompressionType`, `CompressionLevel`): the reader and the renderer take the ones
    /// they use with [`ForeignData::attr`].
    pub attrs: RawAttrs,
    /// The `r:id` of the `Rel`, kept so that a writer gives the relationship back under the same id.
    pub rel_id: Option<Arc<str>>,
    /// The picture to draw: the target of the relationship when it is a picture, or the picture an unsupported target
    /// (an embedded object) falls back to. `None` when there is none, or the reader could not load it.
    pub picture: Option<Arc<Media>>,
    /// Text inside the element: the schema makes it mixed content.
    pub text: Option<Arc<str>>,
    pub passthrough: Passthrough,
    /// What the element holds that no field reads, tagged by format (an ODF picture's link type and MIME type, a newer `.diacraft`'s additions); see
    /// [`crate::package`]. Filled by the `.odg` reader.
    pub ext: Ext<Kept>,
}

impl ForeignData {
    /// An attribute's value, matched exactly.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name)
    }
}

/// A `Shape` element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// `ID`: an unsigned 32-bit key, any value, not unique across a part.
    pub id: u32,
    /// `Name` and `NameU`: the local and universal names.
    pub name: Option<Arc<str>>,
    pub name_u: Option<Arc<str>>,
    pub kind: ShapeType,
    /// `Master`: the `ID` of the master this shape is an instance of.
    pub master: Option<u32>,
    /// `MasterShape`: the `ID` of the shape in the master of the nearest ancestor holding `Master` that this shape stands for
    /// Ignored on a shape that has `Master`.
    pub master_shape: Option<u32>,
    /// `Del`: a shape inherited from a master that was deleted locally.
    pub del: bool,
    /// The shape's cells, sections and style links.
    pub sheet: Arc<Sheet>,
    pub text: Option<Arc<Text>>,
    /// `Data1`, `Data2`, `Data3`: text MS-VSDX calls unused, read by the functions of the same names.
    pub data1: Option<Arc<str>>,
    pub data2: Option<Arc<str>>,
    pub data3: Option<Arc<str>>,
    pub foreign: Option<Arc<ForeignData>>,
    /// The members of a group, in file order.
    pub children: ShapeList,
    /// Attributes with no field: `OriginalID`, `UniqueID`, `IsCustomName`, `IsCustomNameU`, and any other.
    pub raw: RawAttrs,
    /// Elements with no field, between the typed children where the file put them.
    pub passthrough: Passthrough,
    /// The shape's ODF data (its automatic style name, held values, units, kept markup) and the VSDX elements and unknown `.diacraft` content it had no
    /// slot for. Filled by the `.odg` reader, the VSDX reader; see [`crate::package`].
    pub ext: Ext<OdfShape>,
}

impl Shape {
    /// An empty shape with this `ID` and an empty sheet.
    pub fn new(id: u32) -> Shape {
        Shape { id, ..Shape::default() }
    }

    /// Whether the shape is an instance: it names a master.
    pub fn is_instance(&self) -> bool {
        self.master.is_some()
    }
}
