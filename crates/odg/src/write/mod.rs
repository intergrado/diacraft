//! Saving a drawing as an `.odg`: **a save that changes only what an edit touched**, spliced into the original package (the design's "The writer that changes only what an edit touched").
//!
//! [`write_with`] takes the model as it is and the package it was read from ([`OriginalPackage`]: the file's bytes and the [`ShapeIndex`] that maps
//! each shape to its element) and gives the bytes of the package saved. **Nothing is written again from a model.**
//!
//! **With no original** (`write_with(doc, None, ..)`) the drawing is written from the model alone, by [`model`]: a package is made for it (`crate::base`) with the
//! model's `meta.xml`, `settings.xml`, manifest, package entries and containers' markup in it, and the splice below is run on that package with every page and shape of the model
//! new. Everything below that says what a save writes or refuses for a new shape is what the model writer writes and refuses, until its own tasks close those.
//!
//! 1. **The baseline.** The model the original bytes give is read again (`read`) while the caller's model is still alive, so the save is first
//!    charged what the two together hold: the read that made the caller's model left its cost on the index ([`crate::ReadCost`]), a save whose
//!    estimate (the model twice, and what a read holds beside one) is past [`Limits::max_write_memory`] is refused as [`WriteError::Limit`]
//!    before the read, and the read is held to the room that leaves (a file near the reader's caps read in 2.0 GB and aborted the
//!    process at the second read, 2.93 GB, before this). **The estimate goes by sizes the archive states as well as the reader's per-item
//!    figures** (a file of 250 shapes with 1 MB of text in an attribute each was 189 MB, passed an estimate of items, and aborted the
//!    process in the edit, which holds the part and the package several times over): the file, its biggest part, the shapes the index holds, and
//!    the edit's own buffers are added to the read's (`Beside`). **An index no read made** (no cost, other pages than the model's, fewer shapes
//!    than it) is refused before a byte is read. And **every large buffer of a save is made by `try_reserve`**: the part inflated, the splice's
//!    output, the part deflated, the package put together, the lists of a scan and of a plan; memory that is not had is an error
//!    (`write_limit` or `out_of_memory`) and never an abort. The model as it is is compared with the one read ([`compare`]): page
//!    by page, shape by shape, matched by the element each came from, so that a second save, whose model keeps the `ID`s of the first, still
//!    finds each shape's element. What differs is a change of a family the table owns ([`own`]) or one the writer does not yet write, which
//!    refuses the save as [`WriteError::Unsupported`], with nothing written: a change is never dropped.
//! 2. **An unedited save** (the model is the baseline) copies every entry as it is: every part is byte for byte the file's, and so is the file when
//!    the archive is one a copy keeps whole. `meta.xml`, the thumbnail and every other part are the file's, except that **the first save of a new
//!    drawing** ([`WriteOptions::first_save`]) gives a package that has no thumbnail the one it is passed, with its manifest row ([`first`]).
//! 3. **An edited save** scans only the parts a change reaches (`content.xml`, and `styles.xml` for a master page's shapes), finds each changed
//!    element ([`scan`]), makes the edits in the bytes ([`frame`], [`plan`], [`splice`]: one sweep, each byte copied once, counted) and writes
//!    those parts under their own headers; it writes DiaCraft's `meta:generator` and the thumbnail the caller passes ([`meta`]), takes out the
//!    object entries and manifest rows that go with a deleted frame ([`objects`], [`manifest`]), adds the rows a manifest lacked, and copies every
//!    other entry as it is ([`archive`]). `office:version` is never written, so a file's own is kept.
//! 4. **The new index.** The file's elements are numbered anew by the reader's rule; the index of the save maps the model's `ID`s to them, and the
//!    engine keeps it with the file.
//! 5. **The check.** The bytes are checked before they are returned, by a scan of the tags of `content.xml` and `styles.xml` and not by a third
//!    read, which would be a third model beside the caller's: the package must open, the parts must scan, each page must hold the shape elements
//!    the new index says and the index's ordinals must be those elements, each once; **and the start tags the save wrote attributes on are read
//!    back**: each names no attribute twice, by namespace and local name, and each attribute written is there and is read by the
//!    reader's own parser for it (`read::readback`). A package that does not check is an error and the caller never sees it.
//!
//! **The kind of package** ([`media`]): a template (`.otg`) is saved as a template, with every rule above, and a drawing as a drawing.
//! [`WriteOptions::variant`] asks for a kind, which is always stated: the other kind changes the media type in the two places a package states it,
//! `mimetype` and the manifest's root row, and in nothing else, and the package's own kind writes `mimetype` as R2 asks when it is not (an
//! unedited model included: the save is then the file with those statements written, and [`Written::edited`] is false); a package that cannot state
//! the other kind is refused before the file is read ([`WriteError::MediaType`]).
//!
//! **What it writes** besides a move, a resize, a turn, a delete and a reorder: a restyle ([`styles`], [`stylemap`]: a shared automatic style is
//! cloned, one shape names is edited, a common style or none gets a new one, orphans go; a line end is a marker of DiaCraft's own in `office:styles`
//! ([`markers`])), a retext ([`text`], [`runs`]: the paragraphs that differ, by their characters and the cells of their rows, written as runs of automatic text
//! styles, a paragraph added or taken out, a new list item beside one with its row; the others keep their bytes), an edit of an
//! outline ([`geometry`], [`outline`]: `draw:points`, `svg:d`, a custom shape's `draw:enhanced-path` and `draw:modifiers`), a resized page ([`pages`]: the
//! layout, or a copy of the master page and the layout when other pages share them), and new shapes and connectors ([`shapes`]: tier 1 only), **put in the model's
//! order in the same save as a reorder, and with `draw:z-index` on a page that carries it** ([`plan`]); a new picture is a frame that names a new entry under
//! `Pictures/`, which the save adds to the package with a row of the manifest ([`picture`]: the type told by the bytes, the same bytes one entry, a name of the writer's own, the
//! caps of the package checked before a byte is copied).
//!
//! **The pages of the drawing** ([`structure`]): a page the model adds is written as a new `draw:page` with its new shapes, naming a master page that has its size (or a
//! copy of one, [`pages`]); a page it takes away is taken out with its shapes, the styles and package files only they named, and its document-file entry; a page it puts in
//! another place is moved whole with the edits made inside it; a page it renames has its `draw:name` written. The model's pages are matched with the file's by `ID` (the
//! index records each entry's), so the index of a save is in the model's order, each page at the place its element has.
//! **What it refuses**, as an error that writes nothing: a change the writer does not yet write (a style cell with no attribute of its own, a line end it has no
//! art for, a retext that holds what it cannot write whole (a link, a change mark or index mark, a format that cuts a field, a list row no list item beside it has), an outline of tier 2, a new shape that is not tier 1, a flip, a glue made between shapes of the file), an
//! original that is not the file the model came from, a cap passed, and an original that cannot be read again. **What it says**, as notes: a signature
//! that no longer matches, a shear dropped, a route kept as the file has it, and what a new shape held that a new element of this writer cannot hold
//! (attributes and elements of the file's own, `Data1` to `Data3`, foreign data: [`SaveNote::NewShapeUnwritten`]), and a field of a rewritten paragraph whose
//! characters changed ([`SaveNote::FieldWritten`]), but for a field the model holds as a field whose characters a refresh changed, whose element is
//! written again around the new characters.
//!
//! **Every loop is counted in the units it walks and charged before it runs** ([`WriteWork`]; [`crate::Limits`]'s `max_write_*`): bytes scanned and
//! copied, elements walked, edits made, attributes written, shapes and cells compared. A cap passed is [`WriteError::Limit`] and the caller's file
//! is untouched: the writer returns bytes, and writes no file.

mod archive;
mod attrs;
mod comments;
mod compare;
mod convert;
mod count;
mod emit;
mod first;
mod flat;
mod frame;
pub(crate) mod fx;
mod geometry;
mod groups;
mod held;
pub(crate) mod kept;
mod layers;
pub(crate) mod lengths;
mod manifest;
mod marker_art;
mod markers;
mod media;
mod meta;
mod model;
pub(crate) mod newstyles;
mod objects;
mod outline;
mod own;
mod pages;
mod parts;
mod picture;
mod plan;
mod route;
pub(crate) mod runs;
mod scan;
mod shapes;
mod splice;
mod structure;
pub use self::model::{model_refusal, without_carried_objects};
pub(crate) use self::structure::Structure;
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use self::{
    comments::counts_after,
    kept::{Need, declared_by, need_of},
    layers::{NewLayer, deciding},
    model::{Left, bounded, chosen_flags, head_total, node_total},
    runs::model::field_text_ok,
    shapes::{glue_id, past_frame_point, type_agrees},
};
mod stylemap;
mod styles;
mod text;
mod transform;
mod zero;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use diacraft_container::xml_scan::{AttrSpan, tag_attributes};
use diacraft_container::zip::Method;
use diacraft_container::{ContainerError, Work};
use diacraft_doc::Document;

use self::archive::Changes;
use self::compare::{Diff, PageDiff};
use self::count::Counted;
use self::frame::Editor;
use self::manifest::Manifest;
use self::media::Retype;
pub(crate) use self::meta::GENERATOR;
use self::objects::PartRefs;
use self::parts::{Made, Source};
use self::scan::{Ask, Fam, HrefAt, PartKind, scan_part, scan_part_asking};
use self::splice::Splicer;
use crate::index::{Fingerprint, ITEM_BYTES, OriginalPackage, PageShapes, PageSource, ReadCost, ShapeIndex};
use crate::limits::Limits;
use crate::package::{MANIFEST_PATH, Opened};
use crate::read::names::Ns;
use crate::read::units::LenUnit;
use crate::{Form, LimitKind, OdgError, ReadWork, Variant, read};

/// The cells of a page that a save writes into its layout: native, and so never in DiaCraft's document file.
pub(crate) const PAGE_CELL_NAMES: [&str; 7] = stylemap::PAGE_CELLS;

/// What a save may be asked.
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    /// The picture of the drawing's first page, a PNG of at most 256 pixels on its long side, which an edited save writes as the thumbnail
    /// (the engine renders it; this crate renders nothing). Without it an edited save keeps the package's own thumbnail, and an
    /// unedited one always does, unless it is a first save ([`WriteOptions::first_save`]). A flat drawing has no thumbnail: the picture is checked and
    /// not written.
    pub thumbnail: Option<Vec<u8>>,
    /// The save is the first of a new drawing, which has no file yet and was made from a package of DiaCraft's own or from a template:
    /// **a package with no thumbnail gains the one passed even when the model is not edited**, with its manifest row and nothing else changed
    /// ([`first`]), and a package that has one keeps it. Only the caller knows that a drawing has not been saved before, so it says so; it is false for a
    /// drawing opened from a file, whose unedited save is the file's own bytes. An edited save, and a flat drawing, are as they are without it.
    pub first_save: bool,
    /// Saving to another format leaves macros out only when the caller says so. An `.odg` save keeps the file's own (ODF) macro project whatever this says; the macro project of another format that
    /// DiaCraft's document file carries is left out when this says so, by the model writer and by a save into a file (the engine says so for a file a model save made with it). Used.
    pub drop_macros: bool,
    /// Likewise for embedded objects: an `.odg` save keeps them. Used.
    pub drop_objects: bool,
    /// The kind of package to write: a drawing (`.odg`) or a drawing template (`.otg`). `None` keeps the kind the original says it is (a template
    /// is saved as a template, a drawing as a drawing). **Asking for a kind always states it**: asking for the other kind
    /// changes the media type in the two places the package states it, `mimetype` and the manifest's root row, and in nothing else of the package
    /// (a template that a new drawing was made from is saved as a drawing with `Some(Variant::Drawing)`); asking for
    /// the kind the package says it is writes `mimetype` as R2 asks (first and stored) when it is not, even for a model that is not edited. A
    /// package that has no root row in its manifest to state another kind in is refused ([`WriteError::MediaType`]), before the file is read.
    pub variant: Option<Variant>,
    /// The form to write: a package (`.odg`, `.otg`) or one flat XML document (`.fodg`). `None` keeps the form the original is in, and a flat
    /// original is saved flat (whatever its file is called). `Some(Form::Flat)` for a package original writes the drawing as a flat document: the package's
    /// parts become the document's children, its pictures inline; `Some(Form::Package)` for a flat original is refused in words.
    pub form: Option<Form>,
    /// **The content of the drawing is as it was when it was read** (the content stamp of the native format, design section 4.4): a save from the model alone has no file to
    /// compare with, so the caller says whether the model is the one it read. With `true` the model writer writes the bookkeeping as the file had it: `meta.xml`'s generator
    /// (and, as the model holds them, its dates) and the thumbnail. With `false` (the default) it writes the bookkeeping of an edited save: DiaCraft's generator and the thumbnail
    /// passed. **Only the model writer reads it** (`write_with` with no original): a save into a file judges for itself, by comparing the model with the file's.
    pub content_unchanged: bool,
    /// The caps of the read of the original and of the writer's own.
    pub limits: Limits,
}

/// What a save said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveNote {
    /// The package holds a signature file and the save changed the drawing: the signature no longer matches it. The file is kept as it was.
    SignatureStale,
    /// A shape's `draw:transform` held a shear, a scale or a matrix, which the save dropped by writing the transform whole.
    ShearDropped { page: usize, shape: u32 },
    /// A connector's route in the model could not be written as a path: its path is the file's own.
    RouteNotWritten { page: usize, shape: u32 },
    /// A connector was routed anew in a part of ODF 1.0 or 1.1 (or one that declares no version), which has no place for a connector's path (`svg:d`, `svg:viewBox`) or
    /// its skew: the save wrote the connector's ends and left those out, and one the file had is taken out, so the file's reader draws the route its own way
    /// (the connector rows are for 1.2 and later).
    RouteDroppedForVersion { page: usize, shape: u32 },
    /// `meta.xml` could not be changed (it is too large, or is not a part the writer edits): it is the file's own, with its old generator.
    MetaNotUpdated,
    /// A new shape held what a new shape of this writer cannot: the model keeps it, and the file has none of it. A copy of a shape the
    /// reader read carries attributes and child elements of the file's own that the model has no field for, and a shape can hold the text of
    /// `Data1` to `Data3` and data of a kind of its own (`foreign`). The counts are of what the shape held beside what this save writes: the
    /// attributes and the elements it writes itself (the style, the layer, the ends, the geometry) and the ids it never copies are not among them.
    NewShapeUnwritten { page: usize, shape: u32, attributes: usize, elements: usize, data: bool, foreign: bool },
    /// A new shape's outline is not one the writer writes as a path: the file holds a rectangle on its frame as the native outline, and the shape's own
    /// outline as tier 2, which DiaCraft reads and another program does not.
    OutlineApproximated { page: usize, shape: u32 },
    /// Shapes of a page whose own style carries a DiaCraft id that an earlier shape carries too, which is what a LibreOffice copy and paste, or a
    /// conversion to curves, makes: they are read from their native markup alone and hold none of DiaCraft's data.
    DuplicateIds { page: usize, shapes: Vec<u32>, count: usize },
    /// A text field of a paragraph written whole had its characters changed, so the paragraph holds them as plain text and the field's element is not in the file
    /// (item 4). `element` is the field's name in the text namespace, as the standard names it (`author-name`). **One note for each kind of
    /// field in a shape, however many of that kind changed** (a text can hold a million of them): a shape has at most as many
    /// notes as the standard has names for a field.
    FieldWritten { page: usize, shape: u32, element: &'static str },
    /// A group's own frame is not kept: it is not the box of its members (a size it was held to by `LockCalcWH` after a member moved out or turned, a margin, or a pin set
    /// away from the middle), and a `draw:g` has no frame of its own, so the save wrote the group as its members and the file reads the group back as their box.
    /// **One note for each such group, new or already in the file** (a group of the file that a save changed something under, whose frame the model
    /// has held to a size), in the order the page holds them. `glued` are the connectors glued to the group itself (the first [`MOST_GLUED_NAMED`], in the order of their
    /// `ID`s) and `glued_total` how many there are: a connection point of the group moves to the box, and a connector's end stays where it was in the file.
    GroupFrameNotKept { page: usize, shape: u32, glued: Vec<u32>, glued_total: usize },
    /// DiaCraft's document file in the package was of an older version than this build reads, and so was never read: the save replaced it with a file of this build's
    /// version (or took it out), and what the old one held (masters, the sheets of its entries) is not carried (the design's 7.3; no release wrote a file of that version).
    DocumentFileReplaced,
    /// The model holds ODF data of the document that a save from the model alone does not write yet: fill images (a package entry the definition's bytes make, which has no manifest row here),
    /// the automatic styles of the shapes' own families that no shape written takes up, and pieces of the document's containers a part made has no place for. The model keeps them; the file
    /// has none of them. **The named definitions but fill images, the layer set, the other style families and the pages' master pages are written, so `layers` is always 0.**
    /// The model keeps what is not written, and a save says it and never leaves it out without a word.
    DocumentDataUnwritten { definitions: usize, layers: usize, styles: usize, containers: usize },
    /// A field of a shape's text that the `.odg` has no element for the reader would read back as a field (a page name, a shape data field, a field that shows a cell's value, a date or time
    /// the document calculates): the save wrote **the characters it shows now, as plain text**. The field itself rides in DiaCraft's extension, which DiaCraft
    /// reads and another program does not. One note for each shape that has any, with how many.
    FieldAsText { page: usize, shape: u32, fields: usize },
    /// Elements round the runs of a shape's text that the file held (a link, a span with an attribute of its own) were **not written**: each crosses another element of its
    /// paragraph (a file's elements nest), or is nested deeper than the files DiaCraft reads allow, or is in a paragraph with more runs than is matched. Their characters are written, with the
    /// runs' formatting, and the elements are in the model only. One note for each shape that has any, with how many.
    TextElementsNotWritten { page: usize, shape: u32, elements: usize },
    /// Elements of the text of a shape that the file held (a link, a field, a span or paragraph with an attribute of its own, a mark) use a namespace prefix that nothing binds where they are
    /// written, so the model writer **could not write them as they were**: an element that is a run of the text is left out, and one that is a paragraph, a span, a list or
    /// a field is written without its attributes. The characters are written all the same, and the elements are in the model only. One note for each shape that has any, with how many.
    /// (A prefix an element above declared on its own start tag is declared again on each element that uses it when the file is read, so this is for a prefix the file itself left
    /// unbound, or one a drawing of an older build kept without its declaration.)
    TextMarkupUnbound { page: usize, shape: u32, elements: usize },
    /// The properties of an automatic text or paragraph style of the file held attributes that no cell reads and that the model writer **could not write**: a name that is no
    /// qualified name, a value XML cannot hold, a prefix nothing binds, or one the writer has for another namespace. The rest of what such a style kept is written, each attribute where it was.
    /// One note for the save, with how many.
    StyleAttributesNotWritten { attributes: usize },
    /// The drawing holds data of the VSDX format that an `.odg` has no place for, and the file carries it (the design's 5.1, 5.3 and 5.6): on the shapes' own styles, in DiaCraft's document
    /// file and in the entries of its own folder. `classes` are the kinds of it, in plain words; `document_level` says that some of it is in the document file and the folder, which a program that
    /// edits the drawing may keep or drop **whole** (the manifest's row of the folder keeps it through a LibreOffice save, but what is kept is judged against the drawing it now sits in, and an item
    /// that no longer matches is dropped); `macros` and `objects` say that a macro project or embedded objects ride as bytes.
    Carried { classes: Vec<&'static str>, document_level: bool, macros: bool, objects: bool },
    /// The caller chose to leave out the macro project of the other format ([`WriteOptions::drop_macros`]): the number of its parts that were left out of the file.
    MacroProjectLeftOut { parts: usize },
}

/// The most connectors a note of a group's frame names (the rest are counted): a group can have a connector glued for each of a million shapes.
pub const MOST_GLUED_NAMED: usize = 8;

impl SaveNote {
    /// A word for a harness to compare.
    pub fn class(&self) -> &'static str {
        match self {
            SaveNote::SignatureStale => "signature_stale",
            SaveNote::ShearDropped { .. } => "shear_dropped",
            SaveNote::RouteNotWritten { .. } => "route_not_written",
            SaveNote::RouteDroppedForVersion { .. } => "route_dropped_for_version",
            SaveNote::MetaNotUpdated => "meta_not_updated",
            SaveNote::NewShapeUnwritten { .. } => "new_shape_unwritten",
            SaveNote::OutlineApproximated { .. } => "outline_approximated",
            SaveNote::DuplicateIds { .. } => "duplicate_ids",
            SaveNote::FieldWritten { .. } => "field_written",
            SaveNote::GroupFrameNotKept { .. } => "group_frame_not_kept",
            SaveNote::DocumentFileReplaced => "document_file_replaced",
            SaveNote::DocumentDataUnwritten { .. } => "document_data_unwritten",
            SaveNote::FieldAsText { .. } => "field_as_text",
            SaveNote::TextElementsNotWritten { .. } => "text_elements_not_written",
            SaveNote::TextMarkupUnbound { .. } => "text_markup_unbound",
            SaveNote::StyleAttributesNotWritten { .. } => "style_attributes_not_written",
            SaveNote::Carried { .. } => "carried",
            SaveNote::MacroProjectLeftOut { .. } => "macro_project_left_out",
        }
    }
}

impl fmt::Display for SaveNote {
    /// Words and numbers, never a name from the file.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveNote::SignatureStale => write!(f, "the drawing is signed and was changed, so the signature no longer matches it"),
            SaveNote::ShearDropped { page, shape } => {
                write!(f, "shape {shape} on page {page} was sheared or scaled by its transform, which the save wrote without")
            }
            SaveNote::RouteNotWritten { page, shape } => {
                write!(f, "the route of connector {shape} on page {page} could not be written, and the file's own is kept")
            }
            SaveNote::RouteDroppedForVersion { page, shape } => write!(
                f,
                "the route of connector {shape} on page {page} was not written: the part is ODF 1.0 or 1.1 (or declares no version), which has no place for a connector's path, so the save wrote the connector's ends and left its path and skew out"
            ),
            SaveNote::MetaNotUpdated => write!(f, "the document's meta part could not be changed, and keeps its generator"),
            SaveNote::DocumentFileReplaced => write!(
                f,
                "DiaCraft's document file in the package was of an older version than this build reads, so it was never read: the save replaced it, and what it held is not carried"
            ),
            SaveNote::DocumentDataUnwritten { definitions, layers, styles, containers } => {
                let held: Vec<String> = [
                    (*definitions, "named definition", "named definitions"),
                    (*layers, "layer of the layer set", "layers of the layer set"),
                    (*styles, "style element of another family", "style elements of other families"),
                    (*containers, "piece of the containers' markup", "pieces of the containers' markup"),
                ]
                .into_iter()
                .filter(|(n, _, _)| *n > 0)
                .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
                .collect();
                write!(f, "the document holds {} that this save does not write: the drawing keeps them, and the file does not", held.join(", "))
            }
            SaveNote::OutlineApproximated { page, shape } => write!(
                f,
                "new shape {shape} on page {page} has an outline that is written as a rectangle for programs other than DiaCraft, which read its own outline from the data kept with it"
            ),
            SaveNote::DuplicateIds { page, shapes, count } => {
                let named: Vec<String> = shapes.iter().map(u32::to_string).collect();
                let more = count.saturating_sub(shapes.len());
                write!(
                    f,
                    "{count} shape{} on page {page} (shapes {}{}) hold{} a DiaCraft id that an earlier shape holds, as a copy made by another program does, and are read without DiaCraft's data",
                    if *count == 1 { "" } else { "s" },
                    named.join(", "),
                    if more > 0 { format!(" and {more} more") } else { String::new() },
                    if *count == 1 { "s" } else { "" }
                )
            }
            SaveNote::NewShapeUnwritten { page, shape, attributes, elements, data, foreign } => {
                let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
                let mut held: Vec<String> = Vec::new();
                if *attributes > 0 {
                    held.push(plural(*attributes, "attribute", "attributes"));
                }
                if *elements > 0 {
                    held.push(plural(*elements, "element", "elements"));
                }
                if *data {
                    held.push("shape data (Data1 to Data3)".to_owned());
                }
                if *foreign {
                    held.push("foreign data".to_owned());
                }
                write!(
                    f,
                    "new shape {shape} on page {page} held {} that this save does not write: the drawing keeps them, and the file does not",
                    held.join(", ")
                )
            }
            SaveNote::FieldWritten { page, shape, element } => write!(
                f,
                "the characters of a text:{element} field in shape {shape} on page {page} changed (and those of every other field of that kind that changed there), so the save wrote them as plain text, and the field's element is not in the file"
            ),
            SaveNote::GroupFrameNotKept { page, shape, glued, glued_total } => {
                write!(
                    f,
                    "the frame of group {shape} on page {page} is not kept: it is not the box of the group's members (a size it was held to, a margin, or a pin set away from the middle), and a drawing file has no place for a group's frame, so the file holds the group as its members and reads it back as their box"
                )?;
                if *glued_total > 0 {
                    let named: Vec<String> = glued.iter().map(u32::to_string).collect();
                    let more = glued_total.saturating_sub(glued.len());
                    let (name, end) = if *glued_total == 1 {
                        ("the connector glued", "keeps its end where it was")
                    } else {
                        ("the connectors glued", "keep their ends where they were")
                    };
                    write!(
                        f,
                        "; {name} to the group itself ({}{}) {end} in the file, and the group's connection points are on the box",
                        named.join(", "),
                        if more > 0 { format!(" and {more} more") } else { String::new() }
                    )?;
                }
                Ok(())
            }
            SaveNote::Carried { classes, document_level, macros, objects } => {
                write!(
                    f,
                    "this drawing has data an OpenDocument drawing has no place for ({}): DiaCraft keeps it inside the file and reads it back",
                    classes.join(", ")
                )?;
                // What a LibreOffice save does with it was measured (26.8.2.2): it kept the data on shapes, and DiaCraft's folder and document file byte for
                // byte in 129 of 145 null saves, because the manifest lists the folder with a media type; another program may keep it or not. What that program changes wins where DiaCraft can tell.
                write!(
                    f,
                    "; another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell"
                )?;
                if *document_level {
                    write!(f, " (DiaCraft reads each piece of its own folder of document data back only while it still matches the drawing)")?;
                }
                if *macros || *objects {
                    write!(f, "; the macro project and the embedded objects are kept as bytes and are never run or opened")?;
                }
                Ok(())
            }
            SaveNote::MacroProjectLeftOut { parts } => {
                write!(
                    f,
                    "the macro project of the other format was left out of the file at your choice: {parts} part{} not written",
                    if *parts == 1 { " was" } else { "s were" }
                )
            }
            SaveNote::TextElementsNotWritten { page, shape, elements } => write!(
                f,
                "{elements} element{} round runs of the text of shape {shape} on page {page} {} not written: each crosses another element of its paragraph, is nested deeper than the files DiaCraft reads allow, or is in a paragraph with too many runs to match, so the save wrote the characters with the runs' formatting and left {} out",
                if *elements == 1 { "" } else { "s" },
                if *elements == 1 { "was" } else { "were" },
                if *elements == 1 { "it" } else { "them" }
            ),
            SaveNote::TextMarkupUnbound { page, shape, elements } => {
                let (noun, uses, they, them, were) =
                    if *elements == 1 { ("element", "uses", "it is", "it", "it was") } else { ("elements", "use", "they are", "them", "they were") };
                write!(
                    f,
                    "{elements} {noun} of the text of shape {shape} on page {page} {uses} a namespace prefix that nothing binds where {they} written, so the save could not write {them} as {were}: a link, a field or a mark is left out, a paragraph, span or list is written without its attributes, and the characters are written all the same"
                )
            }
            SaveNote::StyleAttributesNotWritten { attributes } => write!(
                f,
                "{attributes} attribute{} of the formatting of the text styles of the drawing could not be written as the file had {}: a name that is no qualified name, a value XML cannot hold, or a prefix that nothing binds or that this part has for another namespace, so the save wrote the rest of each style and left {} out",
                if *attributes == 1 { "" } else { "s" },
                if *attributes == 1 { "it" } else { "them" },
                if *attributes == 1 { "it" } else { "them" }
            ),
            SaveNote::FieldAsText { page, shape, fields } => write!(
                f,
                "{fields} field{} of shape {shape} on page {page} {} no element in an .odg that is read back as a field, so the save wrote the characters {} shows now as plain text: the field itself is in DiaCraft's data, which another program does not read",
                if *fields == 1 { "" } else { "s" },
                if *fields == 1 { "has" } else { "have" },
                if *fields == 1 { "it" } else { "they" }
            ),
        }
    }
}

/// The result of a save.
#[derive(Clone)]
pub struct Written {
    /// The package.
    pub bytes: Vec<u8>,
    /// Where each shape of the model is in the bytes: what the next save maps the model's shapes by. It also holds, for each page this save made or
    /// changed, the model's page as this save had it, which the next save compares the model with (saving the model again, unchanged,
    /// through this index and these bytes is no edit and gives the same bytes), and the checksum of the parts it goes with: an index is refused for
    /// bytes other than those of the save that gave it.
    pub index: ShapeIndex,
    /// What the save said.
    pub notes: Vec<SaveNote>,
    /// Whether the model had changed: an unedited save is the original, copied. It says nothing of the media type: a save as the other kind
    /// ([`WriteOptions::variant`]) of a model that did not change is no edit, and the package it gives is the original's but for the two
    /// statements of the media type.
    pub edited: bool,
    /// What the save cost.
    pub work: WriteWork,
}

impl fmt::Debug for Written {
    /// The size, the notes and the cost, not the package.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Written")
            .field("bytes", &self.bytes.len())
            .field("notes", &self.notes)
            .field("edited", &self.edited)
            .field("work", &self.work)
            .finish()
    }
}

/// Why a drawing was not saved. Nothing was written. A message holds words, numbers and part names that are package structure, never the
/// content of the drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The original cannot be read again.
    Original(OdgError),
    /// The archive could not be written.
    Package(ContainerError),
    /// A change to the model that the writer does not write yet.
    Unsupported(&'static str),
    /// The original, its index and the model do not agree: the original is not the file the model was read from, or the index is not the file's.
    Mismatch(&'static str),
    /// A cap of the writer passed.
    Limit { what: &'static str, max: u64 },
    /// The thumbnail is not one the writer accepts.
    Thumbnail(&'static str),
    /// The bytes written do not read back as planned.
    SelfCheck(String),
    /// The package cannot state the media type that was asked for ([`WriteOptions::variant`]): its manifest has no root entry to put it in, or a
    /// root entry with no media type to change.
    MediaType(&'static str),
    /// A part of the other formats that the drawing carries cannot be carried in the file: its name is not one a package of its format can hold, or it is past the cap of an entry, or the
    /// carried parts are past the file's. `part` names it (empty when it is all of them). Nothing is cut: leave the part out, or save a `.diacraft` file.
    Carried { part: String, why: &'static str },
    /// A group of a page that the save cannot write as it is: `why` in words, and the group named by the page it is on (its name, and `page_nth` among the pages of that name) and
    /// its number among the groups with no name of that page (`nth`), so that a person can find it and name it.
    Group { page: String, page_nth: u32, nth: u32, why: &'static str },
}

impl WriteError {
    /// A word for a harness to compare.
    pub fn class(&self) -> &'static str {
        match self {
            WriteError::Original(e) => e.class(),
            WriteError::Package(e) => e.class(),
            WriteError::Unsupported(_) => "write_unsupported",
            WriteError::Mismatch(_) => "write_mismatch",
            WriteError::Limit { .. } => "write_limit",
            WriteError::Thumbnail(_) => "write_thumbnail",
            WriteError::SelfCheck(_) => "write_self_check",
            WriteError::MediaType(_) => "write_media_type",
            WriteError::Carried { .. } => "carried_entry_refused",
            WriteError::Group { .. } => "write_unsupported",
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Original(e) => write!(f, "the file this drawing was opened from cannot be read again: {e}"),
            WriteError::Package(e) => write!(f, "the file cannot be made: {e}"),
            WriteError::Unsupported(what) => write!(f, "saving does not yet write this change: {what}"),
            WriteError::Mismatch(what) => write!(f, "the file on record is not the one this drawing was opened from: {what}"),
            WriteError::Limit { what, max } => write!(f, "the save is past a limit: more than {max} {what}"),
            WriteError::Thumbnail(what) => write!(f, "{what}"),
            WriteError::SelfCheck(why) => write!(f, "the file just made does not read back, so it was not kept: {why}"),
            WriteError::MediaType(what) => write!(f, "the file cannot be saved as the other kind of document: {what}"),
            WriteError::Carried { part, why } if part.is_empty() => write!(f, "the file cannot carry the drawing's other data: {why}"),
            WriteError::Carried { part, why } => write!(f, "the file cannot carry the part \"{part}\": {why}"),
            WriteError::Group { page, page_nth, nth, why } => {
                let page_place = if *page_nth > 1 { format!(" (the page numbered {page_nth} of that name)") } else { String::new() };
                write!(
                    f,
                    "saving does not yet write this change: {why}; it is the group with no name numbered {nth} on the page \"{page}\"{page_place}"
                )
            }
        }
    }
}

impl std::error::Error for WriteError {}

/// What a save cost, in the units each loop walks. Every field only grows; [`WriteWork::default`] is all zeros but the two caps, which a save
/// sets from its [`Limits`] when it starts.
#[derive(Clone, Debug)]
pub struct WriteWork {
    /// The package: the archive's directory, bytes inflated, bytes of the tag scans' events, and the extra-field records walked.
    pub package: Work,
    /// What reading the original again cost: the baseline.
    pub baseline_read: ReadWork,
    /// Bytes of parts scanned for the elements a change reaches.
    pub scan_bytes: u64,
    /// Elements those scans walked.
    pub elements_walked: u64,
    /// Bytes of start tags looked at for their attributes.
    pub attribute_bytes: u64,
    /// Bytes copied: of the parts the splice wrote between its edits, and of the entries copied or replaced.
    pub bytes_copied: u64,
    /// Bytes of text the edits wrote.
    pub bytes_written: u64,
    /// Bytes the save held to write the annotations of the comments, as it charged them before it made any: never fewer than the annotations come to.
    pub comment_bytes_held: u64,
    /// Edits made to parts.
    pub edits: u64,
    /// Attributes written, added or taken out.
    pub attributes_written: u64,
    /// Shapes compared with the baseline's.
    pub shapes_compared: u64,
    /// Cells compared with the baseline's.
    pub cells_compared: u64,
    /// Items the tier 2 that styles carry cost the save, over all of it: a decode of a style takes the items its attributes stand for, and each shape whose edit
    /// uses what a style carries is charged what that decode took. Charged against [`Limits::max_items`], the budget one read of the drawing has, so
    /// that the decodes of a save with many styles share one budget and do not each have a whole one.
    pub tier2_items: u64,
    /// Decodes of the tier 2 that a style carries that the save made: a style that many shapes name is decoded once, and the shapes that name it take a copy.
    pub tier2_decodes: u64,
    /// Steps along the chain of namespace declarations a prefix was looked up in.
    pub scope_steps: u64,
    /// Shapes and elements the plan of `draw:z-index` and of the order walked.
    pub order_steps: u64,
    /// What a retext did with zero-width elements and fields: the bytes of a paragraph it took out that it looked at for a tag, the elements it listed, carried
    /// over to where the new text has them, and put in, and the characters its block comparisons walked.
    pub zero_steps: u64,
    /// Looks in the sets and maps of names the manifest's rows are chosen by: each entry of the package is looked up in the rows the manifest
    /// has, the entries a save drops, and the names already queued, once each.
    pub manifest_probes: u64,
    /// The comparisons of names those looks made: the units they walked, which is about one a look for a set that is looked up and a pass over
    /// the set for each look for one that is searched (the calls are not the cost).
    pub manifest_compares: u64,
    /// Looks in the prefixes in force on `meta.xml`'s elements, and among those that name the meta namespace when the generator's prefix is chosen.
    pub meta_probes: u64,
    /// The comparisons of prefixes those looks made.
    pub meta_compares: u64,
    /// What the save estimated it would hold at its peak, in bytes, before it read the file again and again once the read's cost was known (the
    /// larger): the figure the gate compares with [`Limits::max_write_memory`].
    pub estimated_memory: u64,
    /// The work of drawing the markers of the line ends a save wrote or checked: the pairs of edges tried when pieces of a mark are united, and the edges walked
    /// by each winding count that tells which part of an edge is the union's (`marker_art.rs`). A marker is drawn once to write it and once to check it.
    pub marker_art_steps: u64,
    /// The pages a save found, matched, moved, planned and wrote: each page of the model and of the index once for the matching, and each page the save adds, takes
    /// away or moves for its own work. Charged to [`Limits::max_write_steps`] with the other steps.
    pub page_steps: u64,
    /// The names of master pages looked at and the numbers tried when a copy of a master page is named: the units that choice walks (it was a walk
    /// from `_2` for each split; it is the names in use once and a cursor that only moves on).
    pub master_name_tries: u64,
    /// What the walk of the caller's model counted: shapes, cells and text parts, each once ([`crate::figures`]).
    pub model_walk_steps: u64,
    /// The shapes of the caller's model, members included (the same walk): what a read of the file made charges the walk that maps the links to the style sheets.
    pub model_shapes: u64,
    /// Bytes the caller's model has grown by since the read that made it, by that walk against the read's: charged to the memory a save holds
    /// before the file is read again.
    pub grown_bytes: u64,
    /// Bytes of start tags the check of the bytes made looked at: those the save wrote attributes on.
    pub checked_tag_bytes: u64,
    /// Attributes those tags hold, each looked at once.
    pub checked_attributes: u64,
    /// Paragraphs the save wrote that the check of the bytes made read back, and shapes whose geometry element it read back.
    pub checked_paragraphs: u64,
    pub checked_outlines: u64,
    /// Paragraphs the save put in whose paragraph style the check of the bytes made looked for (each is one of `checked_paragraphs` too).
    pub checked_inserted: u64,
    /// Members of new groups the save gathered into their groups' elements: each is looked at once to be listed under its group and once in each of the two passes that
    /// assemble the elements, so a save of `m` members counts `3m` (review: a scan of every new shape for each new group was quadratic).
    pub group_members: u64,
    /// New shapes the pass that finds what each new group's members take up looked at: one each, from the last to the first, whatever the groups hold, so a
    /// save of `n` new shapes of which any is a member of a group counts `n`, and one with no member of a group counts none.
    pub group_boxes: u64,
    /// Paragraphs the check of the bytes made read again for the zero-width elements put in them.
    pub checked_zero: u64,
    /// Bytes of a paragraph's content the retext tokenized (the paragraph it edits, and a neighbour it takes the styles of its runs from, each once: charged to
    /// [`WriteWork::scan_bytes`] too, before the tokens are made).
    pub tokenized_bytes: u64,
    /// Bytes of the package the check of the bytes made read again through the reader, to count the paragraphs of the shapes that had one put in, taken out or
    /// made a list's (charged to [`WriteWork::scan_bytes`] too).
    pub read_back_bytes: u64,
    /// The memory a save may hold (`Limits::max_write_memory`), and what is left of it after the estimate the gate made before the read: the lists a retext makes
    /// of the paragraphs of the two texts and of its plan are charged against it, before they are made ([`text::text_edit`]).
    pub memory_cap: u64,
    pub memory_room: u64,
    /// The most steps ([`WriteWork::steps_used`]) and bytes of start tags the save may take, as its limits say ([`Limits::max_write_steps`],
    /// [`Limits::max_write_bytes`]): the loops that walk a chain of namespace declarations or a start tag hold them from inside, as they walk.
    pub step_cap: u64,
    pub byte_cap: u64,
    /// What the file's own markup gives each style sheet of the baseline (the name it holds it by, and the sheet before the document file was put over it): what the
    /// writer names a style by and judges a change of a style sheet against (`newstyles.rs`). Empty for a model no read made.
    pub(crate) file_styles: Vec<crate::ext::docfile::NativeStyle>,
    /// The `Layer` section each page of the file reads from the file's own markup (the layer set of the page, else the document's), before DiaCraft's document file puts
    /// its rows over it, by the page's place in the file; and the section a page the save adds reads (the document's set). A new shape's `draw:layer` is resolved by the
    /// reader against these and not against the model's rows ([`layers`]).
    pub(crate) file_layers: Vec<Option<std::sync::Arc<diacraft_doc::Section>>>,
    pub(crate) new_page_layers: Option<std::sync::Arc<diacraft_doc::Section>>,
    /// What the trees the reader reads may still grow by: the lesser of [`Limits::max_read_xml_nodes`] and `container.xml.max_nodes` (what the reader caps a tree at) less what the read of the file counted ([`ReadCost::tree_nodes`], which
    /// is at least the items of either tree). What a save adds to a part that is more than a few items (a style copied whole) is charged against it
    /// before it is made, so that a save never writes a file the reader refuses ([`WriteWork::charge_tree_items`]).
    pub(crate) tree_room: u64,
    /// The items (elements, attributes, texts) the styles this save copied add to their part, net of those given back when a copy turned out to change nothing.
    pub copied_items: u64,
    /// The bytes of the styles this save copied, and of the keys it holds to find them again: each is charged to [`WriteWork::bytes_copied`] too, before the copy is put in.
    pub style_copy_bytes: u64,
    pub style_key_bytes: u64,
    /// The most bytes of memory a save held at once for the trees of DiaCraft's document file, each taken from the room of [`WriteWork::memory_room`] **before** the tree was
    /// made, from the figures of the models: the masters' trees, and the sheets of the entries.
    pub docfile_held: u64,
    /// The trees of masters the save made for the document file. A save refused for its memory before the first tree is made counts none.
    pub docfile_trees: u64,
    /// The items (elements, attributes and text nodes) of the tree of the document file the save wrote, which the memory held for it is never below.
    pub docfile_items: u64,
    /// The document file the package held was of an older version than this build reads, and the save replaced it (or took it out): what it held is not carried, and the
    /// save says so (`SaveNote::DocumentFileReplaced`).
    pub docfile_old_dropped: bool,
    /// The model writer asks the splice to leave `meta.xml`'s generator as the package it made holds it, because the content is as it was read (`WriteOptions::content_unchanged`,
    /// design 4.4): an edited save writes DiaCraft's generator otherwise. Set and cleared by the model writer; false for every save into a file.
    pub(crate) keep_meta: bool,
    /// The save is the model writer's (`write_with` with no original): every shape is new, and a new shape **writes the markup of the file it was read from** that it keeps in its `odf`
    /// slot. Set and cleared by the model writer; false for every save into a file, which says what a new shape held that it did not write instead.
    pub(crate) model_path: bool,
    /// The automatic paragraph and text styles the model writer wrote **under the names the file's paragraphs and runs gave them**: `(is a text style, name)`. The styles of the
    /// file the model holds that are not among them are still unwritten, and the note says so.
    pub(crate) text_names_written: std::collections::HashSet<(bool, String)>,
    /// The fingerprint of the formatting of the text the model writer's first pass wrote for each new shape that carries its text in DiaCraft's data, as a read of the file that pass made gives
    /// it, by the page's place in the model and the shape's `ID`: the second pass stamps `diacraft:fp-format` with them. Empty in the first pass.
    pub(crate) formats: std::collections::HashMap<(usize, u32), crate::ext::fingerprint::Fp>,
    /// How many new shapes carried their text in DiaCraft's data: the model writer makes its second pass only when there are some.
    pub(crate) carried_texts: u64,
    /// The form of each style sheet the model writer writes in ODF 1.4's form where the drawing holds an earlier version's, by its place (`ext::odf14::prepare`): worked out
    /// once a save, before anything is written, and read by the step that writes the styles and by the document file's. Empty for a save into a file.
    pub(crate) odf14: std::sync::Arc<Vec<Option<std::sync::Arc<crate::ext::odf14::Form14>>>>,
    /// **The model writer wrote some of the drawing in the form ODF 1.4 takes where the drawing held an earlier version's**: a style's property 1.4 has no place for, a list style with
    /// no name, a negative label width, a font size of 0. DiaCraft's data carries the source's form, so a DiaCraft read gives it back; the engine's card says it in the group of what is written as
    /// values the file format holds.
    pub forms_1_4: bool,
    /// How many new shapes and groups carried VSDX data on their own style or in an item of the document file (the note says it).
    pub(crate) carried_shapes: u64,
    /// The caller chose to leave out the macro projects of the other formats (`WriteOptions::drop_macros`), which the document file's writer asks.
    pub(crate) drop_macros: bool,
    /// The entries of DiaCraft's own that the document file's items name, for the save to put in the package.
    pub(crate) carried: Vec<crate::ext::items::Blob>,
    /// The classes of data the save carried, for the note: see `ext::items::Built::classes`.
    pub(crate) carried_classes: Vec<&'static str>,
    pub(crate) carried_macros: bool,
    pub(crate) carried_objects: bool,
    /// The parts of a macro project the caller chose to leave out.
    pub(crate) macros_dropped: Vec<String>,
    /// What the model writer made of the whole model's data before it handed the splice a model with that data taken out (the document file's items and the entries they name): the document file's writer
    /// takes it from here, since the model it is given no longer holds it.
    pub(crate) built: Option<Arc<crate::ext::items::Built>>,
    /// The automatic graphic styles whose kept attributes (the ones no cell reads) the model writer wrote with the style, by name: the rest of the model's kept styles of that
    /// family are still unwritten, and the note says so.
    pub(crate) graphic_remainders_written: std::collections::HashSet<String>,
}

impl Default for WriteWork {
    fn default() -> WriteWork {
        WriteWork {
            package: Work::default(),
            baseline_read: ReadWork::default(),
            scan_bytes: 0,
            elements_walked: 0,
            attribute_bytes: 0,
            bytes_copied: 0,
            bytes_written: 0,
            comment_bytes_held: 0,
            edits: 0,
            attributes_written: 0,
            shapes_compared: 0,
            cells_compared: 0,
            tier2_items: 0,
            tier2_decodes: 0,
            scope_steps: 0,
            order_steps: 0,
            zero_steps: 0,
            manifest_probes: 0,
            manifest_compares: 0,
            meta_probes: 0,
            meta_compares: 0,
            checked_tag_bytes: 0,
            checked_attributes: 0,
            checked_paragraphs: 0,
            checked_outlines: 0,
            checked_inserted: 0,
            group_members: 0,
            group_boxes: 0,
            checked_zero: 0,
            tokenized_bytes: 0,
            read_back_bytes: 0,
            memory_cap: u64::MAX,
            memory_room: u64::MAX,
            model_walk_steps: 0,
            model_shapes: 0,
            master_name_tries: 0,
            page_steps: 0,
            grown_bytes: 0,
            estimated_memory: 0,
            marker_art_steps: 0,
            step_cap: crate::limits::MAX_WRITE_STEPS,
            byte_cap: crate::limits::MAX_WRITE_BYTES,
            file_styles: Vec::new(),
            file_layers: Vec::new(),
            new_page_layers: None,
            tree_room: u64::MAX,
            copied_items: 0,
            style_copy_bytes: 0,
            style_key_bytes: 0,
            docfile_held: 0,
            docfile_trees: 0,
            docfile_items: 0,
            docfile_old_dropped: false,
            keep_meta: false,
            model_path: false,
            text_names_written: std::collections::HashSet::new(),
            formats: std::collections::HashMap::new(),
            carried_texts: 0,
            odf14: std::sync::Arc::new(Vec::new()),
            forms_1_4: false,
            carried_shapes: 0,
            drop_macros: false,
            carried: Vec::new(),
            carried_classes: Vec::new(),
            carried_macros: false,
            carried_objects: false,
            macros_dropped: Vec::new(),
            built: None,
            graphic_remainders_written: std::collections::HashSet::new(),
        }
    }
}

impl WriteWork {
    /// The steps of every loop charged to [`Limits::max_write_steps`]: shapes and cells compared, steps along a chain of declarations, and the pages a save works on.
    pub fn steps_used(&self) -> u64 {
        self.shapes_compared.saturating_add(self.cells_compared).saturating_add(self.scope_steps).saturating_add(self.page_steps)
    }

    /// One step along a chain of namespace declarations, charged before it is taken: `false` when it is past the step cap, and the walk stops
    /// (its answer is then no answer: [`WriteWork::check_steps`] refuses the save at the next write, and at the end of each part).
    pub(crate) fn step(&mut self) -> bool {
        self.scope_steps = self.scope_steps.saturating_add(1);
        self.steps_used() <= self.step_cap
    }

    /// `Err` once the steps taken are past the cap: a loop that stopped at the cap said nothing, and the save is refused.
    pub(crate) fn check_steps(&self) -> Result<(), WriteError> {
        if self.steps_used() > self.step_cap {
            return Err(WriteError::Limit {
                what: "steps of looking namespace prefixes up and of comparing the drawing with the file",
                max: self.step_cap,
            });
        }
        Ok(())
    }

    /// Charges `bytes` of start tags looked at for their attributes, before they are: refused past the byte cap.
    pub(crate) fn charge_attribute_bytes(&mut self, bytes: usize) -> Result<(), WriteError> {
        let next = self.attribute_bytes.saturating_add(bytes as u64);
        if next > self.byte_cap {
            return Err(WriteError::Limit { what: "bytes of start tags looked at", max: self.byte_cap });
        }
        self.attribute_bytes = next;
        Ok(())
    }

    /// Charges `bytes` of a part scanned, before the scan: refused past [`Limits::max_write_bytes`] over the save.
    pub(crate) fn charge_scan(&mut self, bytes: usize, limits: &Limits) -> Result<(), WriteError> {
        let next = self.scan_bytes.saturating_add(bytes as u64);
        if next > limits.max_write_bytes {
            return Err(WriteError::Limit { what: "bytes of parts scanned", max: limits.max_write_bytes });
        }
        self.scan_bytes = next;
        Ok(())
    }

    /// Charges `bytes` of a paragraph's content about to be tokenized: counted apart, and against the bytes a save may scan.
    pub(crate) fn charge_tokens(&mut self, bytes: usize, limits: &Limits) -> Result<(), WriteError> {
        self.charge_scan(bytes, limits)?;
        self.tokenized_bytes = self.tokenized_bytes.saturating_add(bytes as u64);
        Ok(())
    }

    /// Takes `bytes` of the memory a save may hold for a time (a paragraph's tokens, while it is planned): refused as a limit, in the words `what`, when there is not
    /// that much of it left, and given back by [`WriteWork::release`] when what holds it is done. What is kept for good is charged from `memory_room` and never given
    /// back.
    pub(crate) fn hold(&mut self, bytes: u64, what: &'static str) -> Result<(), WriteError> {
        self.memory_room = self.memory_room.checked_sub(bytes).ok_or(WriteError::Limit { what, max: self.memory_cap })?;
        Ok(())
    }

    /// Gives back the `bytes` that [`WriteWork::hold`] took.
    pub(crate) fn release(&mut self, bytes: u64) {
        self.memory_room = self.memory_room.saturating_add(bytes);
    }

    /// Charges `bytes` copied, before the copy.
    pub(crate) fn charge_copy(&mut self, bytes: usize, limits: &Limits) -> Result<(), WriteError> {
        let next = self.bytes_copied.saturating_add(bytes as u64);
        if next > limits.max_write_bytes {
            return Err(WriteError::Limit { what: "bytes copied", max: limits.max_write_bytes });
        }
        self.bytes_copied = next;
        Ok(())
    }

    /// What the save has left of the budget of items its decodes of tier 2 share ([`Limits::max_items`]).
    pub(crate) fn tier2_room(&self, limits: &Limits) -> u64 {
        (limits.max_items as u64).saturating_sub(self.tier2_items)
    }

    /// Charges `items` that a use of the tier 2 a style carries takes: refused past [`Limits::max_items`] over the save.
    pub(crate) fn charge_tier2(&mut self, items: u64, limits: &Limits) -> Result<(), WriteError> {
        let next = self.tier2_items.saturating_add(items);
        if next > limits.max_items as u64 {
            return Err(WriteError::Limit { what: "items of the tier 2 the styles of the drawing carry", max: limits.max_items as u64 });
        }
        self.tier2_items = next;
        Ok(())
    }

    /// Charges `n` steps of a loop over what a shape's edit carries, before the loop: refused past the steps a save may take.
    pub(crate) fn charge_steps(&mut self, n: usize) -> Result<(), WriteError> {
        self.cells_compared = self.cells_compared.saturating_add(n as u64);
        self.check_steps()
    }

    /// The most items the reader reads in one tree: the lesser of [`Limits::max_read_xml_nodes`] and `container.xml.max_nodes`, as `read` and the flat reader set the
    /// limit of the trees they parse.
    fn tree_cap(limits: &Limits) -> u64 {
        limits.max_read_xml_nodes.min(limits.container.xml.max_nodes) as u64
    }

    /// Sets what a tree may still grow by from the cost the read of the file had (see [`WriteWork::tree_room`]).
    pub(crate) fn set_tree_room(&mut self, cost: &crate::index::ReadCost, limits: &Limits) {
        self.tree_room = Self::tree_cap(limits).saturating_sub(cost.tree_nodes);
    }

    /// Charges `items` that a style copied whole adds to a part, before it is made: refused when the part would hold more than the reader reads.
    pub(crate) fn charge_tree_items(&mut self, items: u64, limits: &Limits) -> Result<(), WriteError> {
        match self.tree_room.checked_sub(items) {
            Some(room) => {
                self.tree_room = room;
                self.copied_items = self.copied_items.saturating_add(items);
                Ok(())
            }
            None => Err(WriteError::Limit {
                what: "items in a part, counting the styles this save copies for the runs and paragraphs whose formatting changed (the most DiaCraft reads in one part)",
                max: Self::tree_cap(limits),
            }),
        }
    }

    /// Charges the bytes of a style copied whole, before the copy is put in ([`WriteWork::style_copy_bytes`]).
    pub(crate) fn charge_style_copy(&mut self, bytes: usize, limits: &Limits) -> Result<(), WriteError> {
        self.charge_copy(bytes, limits)?;
        self.style_copy_bytes = self.style_copy_bytes.saturating_add(bytes as u64);
        Ok(())
    }

    /// Charges the bytes of the key a style made is found again by, before it is kept ([`WriteWork::style_key_bytes`]).
    pub(crate) fn charge_style_key(&mut self, bytes: usize, limits: &Limits) -> Result<(), WriteError> {
        self.charge_copy(bytes, limits)?;
        self.style_key_bytes = self.style_key_bytes.saturating_add(bytes as u64);
        Ok(())
    }

    /// Gives back `items` charged for a copy that was not made.
    pub(crate) fn refund_tree_items(&mut self, items: u64) {
        self.tree_room = self.tree_room.saturating_add(items);
        self.copied_items = self.copied_items.saturating_sub(items);
    }

    /// Takes from what the read found the layers the file's own markup gives each of its pages and a page the save adds (see [`WriteWork::file_layers`]).
    pub(crate) fn take_markup_layers(&mut self, native: &crate::ext::docfile::Native) {
        let key = diacraft_doc::SectionKey::new("Layer", None);
        self.file_layers = native.pages.iter().map(|p| p.sheet.section(&key).cloned()).collect();
        self.new_page_layers.clone_from(&native.new_page_layers);
    }

    /// Charges one attribute written, before it is.
    pub(crate) fn charge_attribute(&mut self, limits: &Limits) -> Result<(), WriteError> {
        self.check_steps()?;
        if self.attributes_written >= limits.max_write_attributes as u64 {
            return Err(WriteError::Limit { what: "attributes written", max: limits.max_write_attributes as u64 });
        }
        self.attributes_written += 1;
        Ok(())
    }

    /// Charges one shape compared, and the steps its cells and the namespace lookups have taken, against [`Limits::max_write_steps`].
    pub(crate) fn charge_compared(&mut self, limits: &Limits) -> Result<(), WriteError> {
        self.shapes_compared = self.shapes_compared.saturating_add(1);
        if self.steps_used() > limits.max_write_steps {
            return Err(WriteError::Limit { what: "steps of comparing the drawing with the file", max: limits.max_write_steps });
        }
        Ok(())
    }
}

/// A connector's `svg:d` moved down the page by `by` hundredths of a millimetre, as a save that changes the page's height moves the path of a connector no edit touched
/// (`route::shifted`): `None` for a path the writer cannot place, which a save keeps as it is and says so. **Public for the `odg_write` fuzz target only**, which
/// holds a save to it byte for byte; no part of the crate's interface.
#[doc(hidden)]
pub fn shifted_path(path: &str, by: f64) -> Option<String> {
    route::shifted(path, by)
}

/// Saves `doc`, read from `original`, with `options`. See the [module documentation](self).
pub fn write_with(doc: &Document, original: Option<&OriginalPackage<'_>>, options: &WriteOptions) -> Result<Written, WriteError> {
    write_counted(doc, original, options, &mut WriteWork::default())
}

/// [`write_with`], adding what the save cost to `work` whether it saved or was refused.
pub fn write_counted(
    doc: &Document,
    original: Option<&OriginalPackage<'_>>,
    options: &WriteOptions,
    work: &mut WriteWork,
) -> Result<Written, WriteError> {
    // No original: the drawing is written from the model alone (`model`).
    let Some(original) = original else { return model::write(doc, options, work) };
    let limits = &options.limits;
    work.step_cap = limits.max_write_steps;
    work.byte_cap = limits.max_write_bytes;
    // A save into a file whose macro project of the other format the caller chose to leave out leaves it out of DiaCraft's document file (the file a model save made with
    // `drop_macros`, saved into again, is as that save made it).
    work.drop_macros = options.drop_macros;
    if let Some(thumbnail) = &options.thumbnail {
        meta::check_thumbnail(thumbnail, limits)?;
    }
    // A flat drawing has no package: its save is one document (`flat.rs`); so is the save of a package as a flat document.
    if Form::of(original.bytes) == Form::Flat || options.form == Some(Form::Flat) {
        return flat::write(doc, original, options, work);
    }
    // The package is opened first: its directory states the size of every part, which is what a save holds beside the models.
    let mut opened = crate::package::open(original.bytes, limits, &mut work.package).map_err(WriteError::Original)?;
    // What a save that asked for a kind writes of it, and a package that cannot state another kind is refused here, before a byte of the file is read again.
    let retype = media::wanted(&opened, options.variant)?;
    // An index no read made, or one that is not the drawing's, is refused before a byte is read: the cost it carries is what the gate goes by.
    check_index(doc, original.index, limits)?;
    // An index a save made goes with the bytes that save made: the baseline it holds for the pages the save changed is the model as saved.
    if original.index.fingerprint().is_some_and(|f| *f != fingerprint_of(&opened)) {
        return Err(WriteError::Mismatch(
            "the index is the one a save made for other bytes than these: a save's index goes with the package that save gave",
        ));
    }
    // The caller's model as it is now, against the model the read made: what it has grown by since is memory the save holds beside it (
    // review L2: 2 GB of text put into cells after the read was not charged by a gate that went by the read's cost alone).
    let figures = crate::figures::of(doc);
    work.model_walk_steps = figures.steps;
    work.model_shapes = figures.shapes;
    let grown = growth(&figures, &original.index.read_cost().figures);
    work.grown_bytes = grown;
    let beside = Beside::of(original.bytes, &opened, original.index, grown);
    // The caller's model is alive while the file is read again, so the read is charged against what the model holds: a
    // save whose estimate is past the memory it may hold is refused here, before the read, and the read itself is held to what is left.
    work.estimated_memory = beside.holds(original.index.read_cost());
    work.memory_cap = limits.max_write_memory;
    let baseline_limits = save_limits(limits, original.index.read_cost(), &beside)?;
    let (baseline, report) = read(original.bytes, &baseline_limits).map_err(|e| baseline_error(e, &baseline_limits, limits))?;
    let cost = report.work.cost();
    work.file_styles.clone_from(&report.native.styles);
    work.take_markup_layers(&report.native);
    // The index the caller passed may have said less than the file costs: what the read cost is checked again, now that it is known.
    work.estimated_memory = work.estimated_memory.max(beside.holds(&cost));
    check_memory(&cost, &beside, limits)?;
    work.memory_room = limits.max_write_memory.saturating_sub(work.estimated_memory);
    work.set_tree_room(&cost, limits);
    work.baseline_read = report.work;
    let mut diff = compare::compare(doc, &baseline, original.index, &report.index, limits, work)?;
    // DiaCraft's document file: what the model holds of the document that the file lacks, or lacks now.
    let old_file = match opened.archive.position(crate::ext::docfile::PATH) {
        Some(at) => Some(opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?),
        None => None,
    };
    // A layer the save declares in the document's layer set is read on every page that reads that set: the document file is planned against the markup as it will be.
    // A page with a layer set of its own reads that set with the flags the save writes in it.
    let own_flags: HashMap<usize, &[layers::NewLayer]> =
        diff.pages.iter().filter(|p| !p.layer_flags.is_empty()).filter_map(|p| Some((p.held_at?, p.layer_flags.as_slice()))).collect();
    let declared;
    let native = if diff.new_layers.is_empty() && diff.layer_flags.is_empty() && own_flags.is_empty() {
        &report.native
    } else {
        let own = |h| report.index.page(h).is_some_and(|p| p.own_layers());
        declared = layers::native_with(&report.native, &diff.new_layers, &diff.layer_flags, own, &own_flags, work)?;
        &declared
    };
    diff.docfile = crate::ext::docfile::plan(doc, &baseline, native, &diff.structure, old_file.as_deref(), limits, work)?;
    // The style sheets the file lacks: each is an empty common style of `styles.xml` (`newstyles.rs`), and the document file carries its sheet.
    diff.styles = newstyles::plan(doc, &baseline, &work.file_styles, &mut work.cells_compared, work.model_path).map_err(WriteError::Unsupported)?;
    // The names a shape's parent is written with: for a shape of the file whose links changed, and for a new shape whose links name a style sheet.
    let named = |sheet: &diacraft_doc::Sheet| [sheet.links.line, sheet.links.fill, sheet.links.text].iter().any(|l| l.is_some_and(|id| id != 0));
    if diff
        .pages
        .iter()
        .any(|p| p.shapes.iter().any(|s| s.style.as_ref().is_some_and(|e| e.parent.is_some())) || p.new.iter().any(|n| named(&n.shape.sheet)))
    {
        work.cells_compared = work.cells_compared.saturating_add(doc.styles.len() as u64);
        let names = newstyles::names_by_id(doc, &baseline, &work.file_styles, &diff.styles.new);
        diff.styles.names = names;
    }
    drop(baseline);
    // The shapes read as copies of another (their style carries a DiaCraft id an earlier shape carries): said on every save, edited or not.
    let copies = duplicate_notes(&report.index);
    if !diff.any() {
        // The file copied, and the two statements of the media type changed when the other kind was asked for: the model is not edited either way. The first
        // save of a new drawing also gives a package that has none the thumbnail it was passed (`first.rs`).
        let first_thumbnail = options.thumbnail.as_deref().filter(|_| options.first_save && opened.archive.position(meta::THUMBNAIL).is_none());
        let (bytes, mut notes) = match (first_thumbnail, retype) {
            (Some(thumbnail), retype) => first::copy_with_thumbnail(&mut opened, thumbnail, retype, limits, work)?,
            (None, Some(retype)) => media::retyped_copy(&mut opened, retype, limits, work)?,
            (None, None) => (archive::copy_all(&opened, limits, work)?, Vec::new()),
        };
        notes.extend(copies);
        return Ok(Written { bytes, index: original.index.clone(), notes, edited: false, work: work.clone() });
    }
    let version = report.office_version.clone();
    drop(report);
    // The next save counts from the model this one saved: the cost of the read, with the growth since taken in.
    let mut cost = cost;
    cost.items = cost.items.saturating_add(grown / ITEM_BYTES);
    cost.figures = figures;
    edited(&mut opened, original, Saving { doc, diff: &diff, options, version: version.as_deref(), cost, retype, copies }, work)
}

/// The notes for the shapes of each page that were read as copies: one note a page, with the first shapes named.
fn duplicate_notes(index: &ShapeIndex) -> Vec<SaveNote> {
    /// The most shapes a note names.
    const MOST_NAMED: usize = 20;
    (0..index.pages())
        .filter_map(|at| {
            let page = index.page(at)?;
            let copies = page.copies();
            (!copies.is_empty()).then(|| SaveNote::DuplicateIds {
                page: at,
                shapes: copies.iter().copied().take(MOST_NAMED).collect(),
                count: copies.len(),
            })
        })
        .collect()
}

/// The refusal of a save that would hold more memory than it may.
/// The media type a manifest row of a carried entry has: the part's own, or `application/octet-stream` when it has none or is not a `type/subtype` of token characters (a media type comes from a file and may be
/// anything).
pub(crate) fn media_for_blob(media_type: &str) -> String {
    let token = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b));
    match media_type.split_once('/') {
        Some((kind, sub)) if token(kind) && token(sub) => media_type.to_owned(),
        _ => "application/octet-stream".to_owned(),
    }
}

/// Whether an entry's name is one of DiaCraft's carried entries, `DiaCraft/blob<n>.<ext>` (`ext::items`): the entries that a document file names by number, and nothing else of the folder (the
/// document file itself, or an entry a later version of DiaCraft put there, which is kept as any unknown entry is).
fn is_carried_entry(name: &str) -> bool {
    name.strip_prefix(crate::ext::items::FOLDER).and_then(|rest| rest.strip_prefix("blob")).is_some_and(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest.get(digits..).is_some_and(|tail| tail.starts_with('.') && !tail.contains('/'))
    })
}

/// **The entries of DiaCraft's own are what the document file names**. `carried` are the entries the document file this save made names, each `blob<n>` by its place
/// among the byte strings the parts hold, and the place of a byte string moves when the parts that ride change (an object whose frame was deleted rides from then on). A save that **writes the document
/// file** writes each entry that the package does not hold with the bytes the file names (an entry of that name that holds other bytes is written anew), and **takes out the carried entries it no
/// longer names**: before, an entry of that name was taken for the one named, so the document file named the VBA project's bytes in an entry that held the object's, and the project went into the next
/// name, held twice. A save that **takes the document file out** takes out every carried entry, which nothing names. A save that **leaves the document file as the file has it** adds only an entry the
/// package lacks, as before: the file's document file names what its entries hold.
fn carry_entries(
    opened: &mut Opened<'_>,
    carried: &[crate::ext::items::Blob],
    docfile_written: bool,
    changes: &mut Changes,
    rows_new: &mut Vec<(String, String)>,
    rows_gone: &mut HashSet<String>,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    let docfile_removed = changes.remove.contains(crate::ext::docfile::PATH);
    if docfile_written || docfile_removed {
        let named: HashSet<&str> = carried.iter().map(|blob| blob.name.as_str()).collect();
        let stale: Vec<String> = opened
            .archive
            .entries()
            .iter()
            .filter(|e| !e.folder && is_carried_entry(&e.name) && !named.contains(e.name.as_str()))
            .map(|e| e.name.clone())
            .collect();
        for name in stale {
            changes.remove.insert(name.clone());
            rows_gone.insert(name);
        }
    }
    for blob in carried {
        let at = opened.archive.position(&blob.name);
        if let Some(at) = at {
            // (`holds` reads nothing when the row's size or checksum is another, and otherwise reads the entry through and keeps none of it.)
            if !docfile_written || opened.archive.holds(at, &blob.bytes, &mut work.package).map_err(WriteError::Package)? {
                continue;
            }
            // The entry holds other bytes than the ones named: it is written anew, under its row.
            changes.remove.insert(blob.name.clone());
        } else {
            rows_new.push((blob.name.clone(), media_for_blob(&blob.media_type)));
        }
        let method = if blob.bytes.len() >= crate::ext::items::STORED_FROM { Method::Stored } else { Method::Deflated };
        changes.pictures.push((blob.name.clone(), Arc::clone(&blob.bytes), method));
    }
    Ok(())
}

pub(crate) fn too_much_memory(limits: &Limits) -> WriteError {
    WriteError::Limit { what: "bytes of memory a save holds (the model and the file read again beside it)", max: limits.max_write_memory }
}

/// Bytes of memory a save holds for each shape element beside the models: the nodes of the two walks of the comparison, the entry of the scan,
/// and the diff. Measured at about 150; this is the round figure above it.
const WALK_BYTES: u64 = 256;

/// What the caller's model has grown by since the read that made it, in bytes of memory: the cells, shapes, nodes and bytes it holds now beyond
/// those the read's model held, the cells and shapes at what the reader charges for each, the nodes (rows, sections, kept attributes and kept
/// elements) at [`NODE_ITEMS`] and the text and the pictures at a byte a byte. A model that shrank grew by nothing.
fn growth(now: &crate::figures::Figures, then: &crate::figures::Figures) -> u64 {
    let more = |a: u64, b: u64| a.saturating_sub(b);
    more(now.cells, then.cells)
        .saturating_mul(CELL_ITEMS * ITEM_BYTES)
        .saturating_add(more(now.shapes, then.shapes).saturating_mul(crate::read::cost::SHAPE as u64 * ITEM_BYTES))
        .saturating_add(more(now.nodes, then.nodes).saturating_mul(NODE_ITEMS * ITEM_BYTES))
        .saturating_add(more(now.text_bytes, then.text_bytes))
        .saturating_add(more(now.media_bytes, then.media_bytes))
}

/// Items the reader charges for one cell.
const CELL_ITEMS: u64 = 4;

/// Items charged for one row, section, kept attribute or kept node of a model: an allocation of its own, rounded up to whole items.
const NODE_ITEMS: u64 = 2;

/// What a save holds beside the models, from sizes the archive states and the index holds, never from the reader's per-item figures alone
/// (a file of 250 shapes with a 1 MB attribute each, 189 MB and 16.6 million items, passed an estimate of items and nodes and
/// aborted the process at 2.75 GB in the edit, which holds the part several times over).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Beside {
    /// The original's bytes, which the caller holds, and the same again for the package made, which is at most that and the edit.
    file: u64,
    /// The largest part a save inflates (`content.xml`, `styles.xml` or `meta.xml`), by the size the archive states for it: the tree holds the
    /// text of a part that holds it, and the edit holds the part five times (inflated, scanned, the splice's output, the part replaced and the
    /// buffer it is deflated into).
    part: u64,
    /// Shape elements the index holds, over all pages.
    shapes: u64,
    /// What the caller's model has grown by since the read that made it (cells, shapes and text), counted from the model itself.
    grown: u64,
}

impl Beside {
    fn of(bytes: &[u8], opened: &Opened<'_>, index: &ShapeIndex, grown: u64) -> Beside {
        let part = opened
            .archive
            .entries()
            .iter()
            .filter(|e| !e.folder && matches!(e.name.as_str(), "content.xml" | "styles.xml" | "meta.xml"))
            .map(|e| e.size)
            .max()
            .unwrap_or(0);
        let shapes = (0..index.pages()).filter_map(|at| index.page(at)).map(|p| u64::from(p.elements())).sum();
        Beside { file: bytes.len() as u64, part, shapes, grown }
    }

    /// What a save of a flat document holds beside the models: the document is the one part, which the tree holds the text of and the edit holds again.
    fn flat(bytes: &[u8], index: &ShapeIndex, grown: u64) -> Beside {
        let shapes = (0..index.pages()).filter_map(|at| index.page(at)).map(|p| u64::from(p.elements())).sum();
        Beside { file: bytes.len() as u64, part: bytes.len() as u64, shapes, grown }
    }

    /// What a save holds at its peak: **the read of the file again** (the caller's model and the one read, what that read holds beside it, the
    /// file, the text in the tree, and the walks), **and on top of it the edit**: the part twice (inflated, and spliced), and the package made
    /// twice (the parts deflated, and the package put together). The two are added and never taken as the larger of the two, because what the
    /// read frees is not given back to the system: the second model's small allocations stay in the process's heap (measured: after the second
    /// read of the review's input was dropped the resident size was the same to the kibibyte, and the edit's buffers, each one its own mapping,
    /// came on top: a peak of 2.71 GB resident and 2.98 GB mapped, where the larger of the two would have said 2.52).
    fn holds(&self, cost: &ReadCost) -> u64 {
        let walks = self.shapes.saturating_mul(WALK_BYTES);
        let reading = cost.save_bytes().saturating_add(self.file).saturating_add(self.part).saturating_add(walks);
        let editing = self.part.saturating_mul(2).saturating_add(self.file.saturating_mul(2));
        // The growth is in the caller's model only: the model read again is the file's.
        reading.saturating_add(editing).saturating_add(self.grown)
    }
}

/// Whether a save of a model that cost `cost` to read fits in the memory it may hold.
fn check_memory(cost: &ReadCost, beside: &Beside, limits: &Limits) -> Result<(), WriteError> {
    if beside.holds(cost) > limits.max_write_memory { Err(too_much_memory(limits)) } else { Ok(()) }
}

/// The refusal of an index that is not the one a read of this drawing made: one that carries no cost (the only index code outside
/// this crate can make is `ShapeIndex::default`), one of other pages than the drawing's, and one that holds fewer shapes than the model, which
/// is a model that grew after it was read. A save does not read the file to find these out: its gate goes by what the index says.
fn check_index(doc: &Document, index: &ShapeIndex, limits: &Limits) -> Result<(), WriteError> {
    if *index.read_cost() == ReadCost::default() {
        return Err(WriteError::Mismatch("the index carries no cost: no read of this drawing made it"));
    }
    // A model of other pages than the index's is a model whose pages were added, taken away or put in another order, which a save writes (`structure`); one of more
    // pages than a drawing may hold is refused before the file is read.
    if doc.pages.len() > structure::most_pages(limits) {
        return Err(structure::too_many(limits));
    }
    // A model with more shapes than the index holds is a model with new shapes, which a save writes: what they hold is charged to the memory
    // gate as the growth of the model since the read (`growth`), counted from the model itself.
    Ok(())
}

/// The limits the file is read again under: the caller's, with the items lowered to what is left of the memory a save may hold once the
/// caller's model and what its read held are taken from it, so that a file that costs more than its index said is refused as it is read. A
/// save whose estimate is past the memory it may hold is refused (`write_limit`) with nothing read.
fn save_limits(limits: &Limits, cost: &ReadCost, beside: &Beside) -> Result<Limits, WriteError> {
    check_memory(cost, beside, limits)?;
    let held = cost
        .model_bytes()
        .saturating_add(beside.grown)
        .saturating_add(cost.transient_bytes())
        .saturating_add(beside.file)
        .saturating_add(beside.part);
    let room = limits.max_write_memory.saturating_sub(held).saturating_sub(beside.shapes.saturating_mul(WALK_BYTES));
    let items = usize::try_from(room / ITEM_BYTES).unwrap_or(usize::MAX);
    Ok(Limits { max_items: limits.max_items.min(items), ..limits.clone() })
}

/// What a read of the file again refused as: an item cap the save lowered is the save's memory limit, and any other is the reader's.
fn baseline_error(e: OdgError, lowered: &Limits, limits: &Limits) -> WriteError {
    match e {
        OdgError::Limit { kind: LimitKind::Items, .. } if lowered.max_items < limits.max_items => too_much_memory(limits),
        other => WriteError::Original(other),
    }
}

/// The references of a part, and which elements of each of its pages are gone.
type Scanned = (Vec<HrefAt>, HashMap<u32, Vec<bool>>);

/// An attribute the save wrote, placed in the part made: the page of the part it is on and the ordinal its element has there, and which
/// attribute it is. What the check of the bytes made reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EditedTag {
    kind: PartKind,
    page: usize,
    ordinal: u32,
    ns: Ns,
    local: &'static str,
}

impl EditedTag {
    /// The order the check goes in: the tags of one part, then of one page, then of one element come together.
    fn key(&self) -> (bool, usize, u32) {
        (self.kind == PartKind::Styles, self.page, self.ordinal)
    }
}

/// What the splice of one part gave: the part itself is not in it (it is moved to the package's changes, never copied).
struct PartOut {
    /// Each page changed: its place in the document and its new index.
    applied: Vec<(usize, PageShapes)>,
    hrefs: Vec<HrefAt>,
    /// Which elements are gone, by the page's place in the part's scan.
    gone: HashMap<u32, Vec<bool>>,
    /// The attributes written, placed in the part made.
    edited: Vec<EditedTag>,
    /// The styles the save made in the part, which the check of the bytes made looks for by name.
    styles: Vec<styles::Written>,
    /// The master pages a copy was put after, by their order among the part's (`styles.xml`): the master pages after them are one further on.
    masters_inserted: Vec<usize>,
    /// The master pages taken out because no page names them after the save, by their order among the part's: those after them are one back.
    masters_removed: Vec<usize>,
    /// The paragraphs the save rewrote: the page's place in the part, the element's ordinal after the save, and the paragraphs with the text each reads as.
    texts: Vec<PagedParas>,
    /// The paragraphs the save put in, among those: the paragraph style written for each, which the paragraph must name in the part made.
    inserted: Vec<PagedStyles>,
    /// The zero-width elements and fields put in paragraphs: what each paragraph written, kept or edited in place that took one must hold, by the page's
    /// place in the part and the element's ordinal after the save.
    zero: Vec<PagedZero>,
    /// The attributes written on the geometry elements of custom shapes: the page's place in the part, the element's ordinal after the save, and which.
    kid_edits: Vec<PagedAttrs>,
    /// The line ends whose markers the styles of this part name (`content.xml`), which `styles.xml` holds the elements of; and, in `styles.xml`, the
    /// markers the save wrote there, which the check of the bytes made looks for.
    markers: std::collections::BTreeSet<markers::Def>,
    /// In `styles.xml`, the markers among them that this save put there (the others the file held): the check of the bytes made looks at the form of these.
    markers_written: std::collections::BTreeSet<markers::Def>,
    /// In `styles.xml`, the markers of DiaCraft's name that this save took out because no style names them after it: the check of the bytes made
    /// looks that they are gone. In `content.xml`, the markers its styles named before the save, when it looked (the splice of `styles.xml` is asked about those no
    /// style names after it).
    markers_removed: std::collections::BTreeSet<markers::Def>,
    marker_refs: std::collections::BTreeSet<markers::Def>,
    /// The `style:font-face` declarations the save added to the part: the name and the family of each.
    faces: Vec<(String, String)>,
    /// A paragraph was put in, taken out or made a list's in the part: the reader is asked how many paragraphs each shape it changed has.
    restructured: bool,
    /// The shapes written with tier 2: the page's place in the document, the element's ordinal after the save and what the shape carries. The file made is
    /// read again and what each carries is compared with it.
    tier2: Vec<(usize, u32, Arc<crate::ext::payload::Tier2>)>,
    /// The shapes whose text was edited: the page's place in the document, the element's ordinal after the save, and how many paragraphs the model has.
    read_back: Vec<ReadBack>,
}

/// A shape whose text was edited, as the read of the file made is asked about it.
#[derive(Clone, Debug)]
pub(crate) struct ReadBack {
    page_at: usize,
    ordinal: u32,
    paragraphs: usize,
    /// The paragraphs written, by their place in the text, with the characters each reads as.
    written: Vec<(usize, String)>,
}

/// What an edited save is made from besides the package: the model, what it changed, the options, the office version of the file and the cost of
/// the read the next save counts from.
struct Saving<'a> {
    doc: &'a Document,
    diff: &'a Diff,
    options: &'a WriteOptions,
    version: Option<&'a str>,
    cost: ReadCost,
    /// What the save writes of the package's kind, when a kind was asked for ([`media::wanted`]).
    retype: Option<Retype>,
    /// The notes for the shapes the read found to be copies.
    copies: Vec<SaveNote>,
}

/// An edited save: see the module documentation.
fn edited(opened: &mut Opened<'_>, original: &OriginalPackage<'_>, saving: Saving<'_>, work: &mut WriteWork) -> Result<Written, WriteError> {
    let Saving { doc, diff, options, version, cost, retype, copies } = saving;
    let limits = &options.limits;
    let mut notes: Vec<SaveNote> = copies;
    let mut changes = Changes { retype, ..Changes::default() };
    // The pages of the file that changed, by part: a page the save adds has no element in the file, and is the splice of `content.xml`'s own (`structure::apply`).
    let by_part = |kind: PartKind| -> Vec<&PageDiff> {
        diff.pages
            .iter()
            .filter(|p| {
                p.any()
                    && p.held_at.is_some()
                    && matches!((kind, p.source), (PartKind::Content, PageSource::Page(_)) | (PartKind::Styles, PageSource::MasterPage(_)))
            })
            .collect()
    };
    let added: Vec<&PageDiff> = diff.pages.iter().filter(|p| p.held_at.is_none()).collect();
    let mut outs: Vec<(PartKind, PartOut)> = Vec::new();
    let mut taken: Vec<(Fam, String)> = Vec::new();
    // The pages whose size was edited, and the pages the model added: which master page and layout each goes into (the layout is in `styles.xml`, the page in `content.xml`).
    let pages_plan = pages::plan(&mut Source::Package(opened), diff, doc, limits, work)?;
    let plans = &pages_plan.plans;
    let mut needs: std::collections::BTreeSet<markers::Def> = std::collections::BTreeSet::new();
    // The markers of DiaCraft's name that a style of `content.xml` named before the save and none names after it: `styles.xml` is asked to take them out.
    let mut orphans: std::collections::BTreeSet<markers::Def> = std::collections::BTreeSet::new();
    // The new pictures of the save, named once over both parts (`picture.rs`); a save that makes none never builds the book.
    let wants_pictures = diff.pages.iter().any(|p| p.new.iter().any(|n| shapes::is_new_picture(&n.shape)));
    let mut book = if wants_pictures { Some(picture::PictureBook::of(opened, limits, work)?) } else { None };
    // Every name either XML part already uses is reserved before any picture is named, whichever part the save changes (the review).
    if let Some(book) = book.as_mut() {
        book.reserve_parts(opened, limits, work)?;
    }
    for (kind, name) in [(PartKind::Content, "content.xml"), (PartKind::Styles, "styles.xml")] {
        let pages = by_part(kind);
        // (A page the model adds is a page of the structure: `any` holds for it.)
        let structural = kind == PartKind::Content && diff.structure.any();
        let styles_wanted = kind == PartKind::Styles
            && (!plans.is_empty()
                || !needs.is_empty()
                || !orphans.is_empty()
                || !diff.styles.new.is_empty()
                || !diff.styles.gone.is_empty()
                || !diff.new_layers.is_empty()
                || !diff.layer_flags.is_empty());
        if pages.is_empty() && !structural && !styles_wanted {
            continue;
        }
        if kind == PartKind::Styles && !needs.is_empty() && opened.archive.position(name).is_none() {
            return Err(WriteError::Unsupported("a line end in a package with no styles.xml to put its marker in"));
        }
        if kind == PartKind::Styles && !(diff.styles.new.is_empty() && diff.styles.gone.is_empty()) && opened.archive.position(name).is_none() {
            return Err(WriteError::Unsupported("a style sheet added or taken out in a package with no styles.xml to do it in"));
        }
        if kind == PartKind::Styles && !diff.new_layers.is_empty() && opened.archive.position(name).is_none() {
            return Err(WriteError::Unsupported("a new layer in a package with no styles.xml to declare it in"));
        }
        let ctx = PartCtx {
            plans: &pages_plan,
            structure: &diff.structure,
            added: &added,
            doc,
            taken: &taken,
            markers: &needs,
            orphans: &orphans,
            styles: &diff.styles,
            layers: &diff.new_layers,
            layer_flags: &diff.layer_flags,
        };
        let (part, out) =
            splice_part(&mut Source::Package(opened), name, kind, &pages, original.index, &ctx, book.as_mut(), limits, work, &mut notes)?;
        taken.extend(out.styles.iter().map(|s| (s.family, s.name.clone())));
        if kind == PartKind::Content {
            needs.clone_from(&out.markers);
            orphans = orphaned_markers(&part, &out.marker_refs, limits, work)?;
        }
        changes.replace.insert(name.to_owned(), part);
        outs.push((kind, out));
    }
    let doomed = doomed_entries(opened, diff, &outs, limits, work)?;
    changes.remove.clone_from(&doomed.entries);
    let mut rows_gone = doomed.rows.clone();
    // The entries of the new pictures and their rows come first among the new ones (the order: the pictures, then the thumbnail).
    let new_pictures = book.map(picture::PictureBook::finish).unwrap_or_default();
    let mut rows_new: Vec<(String, String)> = picture::put_in(&new_pictures, &mut changes);
    match &diff.docfile {
        Some(crate::ext::docfile::Change::Write(bytes)) => {
            if opened.archive.position(crate::ext::docfile::PATH).is_some() {
                changes.replace.insert(crate::ext::docfile::PATH.to_owned(), bytes.clone());
            } else {
                changes.add.push((crate::ext::docfile::PATH.to_owned(), bytes.clone(), Method::Deflated));
                rows_new.push((crate::ext::docfile::PATH.to_owned(), crate::ext::docfile::MEDIA_TYPE.to_owned()));
            }
        }
        Some(crate::ext::docfile::Change::Remove) => {
            changes.remove.insert(crate::ext::docfile::PATH.to_owned());
            rows_gone.insert(crate::ext::docfile::PATH.to_owned());
        }
        None => {}
    }
    // The entries of DiaCraft's own that the document file's items name, shared with the model and added before every other, and the manifest's row of their folder, which keeps them through a
    // save by another program.
    let carried = std::mem::take(&mut work.carried);
    let docfile_written = matches!(&diff.docfile, Some(crate::ext::docfile::Change::Write(_)));
    carry_entries(opened, &carried, docfile_written, &mut changes, &mut rows_new, &mut rows_gone, work)?;
    // **The folder's row is there whenever an entry of DiaCraft's is**: the document file alone is such an entry, and LibreOffice 26.8.2.2 dropped it on a save of a file whose manifest
    // listed it and not its folder (measured: a VSDX drawing of three texts saved as an `.odg`, whose data is the document file and no part), so the items on the document, its pages and its faces were
    // lost; with the folder's row every entry came through.
    // (A save that leaves the document file as the file has it leaves the manifest as it is too.) **A save that takes the last of them out takes the folder's row out with it**: a row for a folder the
    // package does not have is one a reader warns of.
    let folder = crate::ext::items::FOLDER;
    if (docfile_written || !carried.is_empty()) && opened.manifest.entry(folder).is_none() && !rows_new.iter().any(|(name, _)| name == folder) {
        rows_new.push((folder.to_owned(), crate::ext::items::FOLDER_MEDIA_TYPE.to_owned()));
    }
    let docfile_removed = matches!(&diff.docfile, Some(crate::ext::docfile::Change::Remove));
    if docfile_removed && carried.is_empty() && opened.manifest.entry(folder).is_some() {
        let left = opened.archive.entries().iter().any(|e| e.name.starts_with(folder) && e.name != folder && !changes.remove.contains(&e.name));
        if !left {
            rows_gone.insert(folder.to_owned());
        }
    }
    package_changes(opened, options, version, &rows_gone, rows_new, &mut changes, &mut notes, work)?;
    if opened.signed {
        notes.push(SaveNote::SignatureStale);
    }
    if work.docfile_old_dropped {
        notes.push(SaveNote::DocumentFileReplaced);
    }
    let bytes = archive::assemble(opened, &changes, limits, work)?;
    // The pictures are looked at first: the package made is opened here for the first time, and a cap it passes because of them is told in their words (a save that asks
    // for a kind opens it again below, and would say any refusal as a fault of its own).
    picture::check_made(&bytes, &new_pictures, limits, work)?;
    if let Some(retype) = retype {
        media::check_made(&bytes, retype, limits, work)?;
    }
    let index = new_index(original.index, &outs, cost, doc, diff, fingerprint_of_made(&bytes, limits, work)?);
    check_pages(&bytes, false, &pages_plan, limits, work)?;
    // The parts made are in the package now: what holds them again goes before the check inflates one.
    drop(changes);
    let mut tags: Vec<EditedTag> = Vec::new();
    let held: usize = outs.iter().map(|(_, out)| out.edited.len()).sum();
    tags.try_reserve_exact(held).map_err(|_| WriteError::Limit { what: "memory for the attributes written", max: held as u64 })?;
    for (_, out) in outs.iter_mut() {
        tags.append(&mut out.edited);
    }
    tags.sort_unstable_by_key(EditedTag::key);
    self_check(&bytes, false, &index, &tags, limits, work)?;
    check_made_styles(&bytes, false, &outs, limits, work)?;
    check_made_texts(&bytes, false, &outs, limits, work)?;
    check_read_back(&bytes, &outs, limits, work)?;
    check_tier2(&bytes, &outs, limits, work)?;
    check_made_kids(&bytes, false, &outs, limits, work)?;
    check_made_markers(&bytes, false, &outs, limits, work)?;
    newstyles::check(&bytes, doc, &newstyles::Expect { plan: Some(&diff.styles), links: newstyles::links_written(diff) }, &index, limits, work)?;
    Ok(Written { bytes, index, notes, edited: true, work: work.clone() })
}

/// The paragraphs written on a shape: the page's place in the part, the element's ordinal after the save, and each paragraph with the text it reads as.
pub(crate) type PagedParas = (usize, u32, Vec<(usize, String)>);

/// The paragraph style written for each paragraph a retext put in (by its number in the text), of the shape of a part's page that its ordinal names.
pub(crate) type PagedStyles = (usize, u32, StyleNames);

/// What a retext expects of the zero-width elements of the paragraphs of the shape of a part's page that its ordinal names.
pub(crate) type PagedZero = (usize, u32, Vec<zero::Expect>);

/// The paragraph style written for each paragraph a retext put in, by the paragraph's number in the model's text.
pub(crate) type StyleNames = Vec<(usize, Option<String>)>;

/// The attributes written on a shape's geometry element: the page's place in the part, the element's ordinal after the save, and which.
type PagedAttrs = (usize, u32, Vec<(Ns, &'static str)>);

/// What the splice of a part keeps of each page it applies (the new shapes of a page the save adds included), for the checks of the bytes made and the new index.
#[derive(Default)]
pub(crate) struct PageOutputs {
    /// Each page changed: its place in the model and its new index.
    applied: Vec<(usize, PageShapes)>,
    /// Which elements are gone, by the page's place in the part's scan.
    gone: HashMap<u32, Vec<bool>>,
    /// The ordinal each element of a page has after the save, by the page's place in the part's scan.
    new_ords: HashMap<usize, Vec<u32>>,
    texts: Vec<PagedParas>,
    inserted: Vec<PagedStyles>,
    zero: Vec<PagedZero>,
    read_back: Vec<ReadBack>,
    kid_edits: Vec<PagedAttrs>,
    tier2: Vec<(usize, u32, Arc<crate::ext::payload::Tier2>)>,
    new_tags: Vec<EditedTag>,
}

impl PageOutputs {
    /// Takes what the application of one page's changes left: `at` is the page's place in the part's scan and its place among the part's pages after the save,
    /// `original` says the page is one the file has (a page the save adds has no element to say what is gone of).
    fn record(&mut self, editor: &mut Editor<'_, '_>, kind: PartKind, diff: &PageDiff, at: (usize, usize), original: bool, done: plan::Applied) {
        let (scan_at, now_at) = at;
        // A new connector's targets are given ids of the form `id` and a number above the largest the part holds; a page's shapes take as many as there are connections
        // at most, and the next page's go on from there, so that no id is given twice in a part that several pages changed.
        editor.max_id = editor.max_id.saturating_add(diff.new_connects.len() as u64);
        for change in diff.shapes.iter().filter(|c| c.text.is_some()) {
            let now = done.new_ord.get(change.ord as usize).copied().unwrap_or(0);
            let paras: Vec<(usize, String)> = change.text.iter().flat_map(|t| t.written()).collect();
            if now != 0 {
                if let Some(t) = &change.text {
                    self.read_back.push(ReadBack { page_at: diff.page_at, ordinal: now, paragraphs: t.new.len(), written: paras.clone() });
                }
                self.texts.push((now_at, now, paras));
            }
        }
        for (ord, styles) in std::mem::take(&mut editor.inserted) {
            let now = done.new_ord.get(ord as usize).copied().unwrap_or(0);
            if now != 0 {
                self.inserted.push((now_at, now, styles));
            }
        }
        for (ord, expect) in std::mem::take(&mut editor.zero) {
            let now = done.new_ord.get(ord as usize).copied().unwrap_or(0);
            if now != 0 {
                self.zero.push((now_at, now, expect));
            }
        }
        for (ord, attrs) in std::mem::take(&mut editor.kid_edits) {
            let now = done.new_ord.get(ord as usize).copied().unwrap_or(0);
            if now != 0 {
                self.kid_edits.push((now_at, now, attrs));
            }
        }
        // The shapes of the file an edit gave a tier 2 are read back as the new shapes are (a duplicate `ID` written by an edit reads the shape as a copy, and is
        // refused, never saved).
        for (ord, carried) in std::mem::take(&mut editor.tier2_edited) {
            let now = done.new_ord.get(ord as usize).copied().unwrap_or(0);
            if now != 0 {
                self.tier2.push((diff.page_at, now, carried));
            }
        }
        // What was written on the new shapes is read back as what was written on the others is: their paragraphs and their geometry elements.
        for (ordinal, paras) in done.new_texts {
            if ordinal != 0 {
                self.texts.push((now_at, ordinal, paras));
            }
        }
        for (ordinal, attrs) in done.new_kids {
            if ordinal != 0 {
                self.kid_edits.push((now_at, ordinal, attrs));
            }
        }
        for (ordinal, carried) in done.new_tier2 {
            self.tier2.push((diff.page_at, ordinal, carried));
        }
        for (ordinal, attrs) in &done.new_tags {
            self.new_tags.extend(attrs.iter().filter(|_| *ordinal != 0).map(|(ns, local)| EditedTag {
                kind,
                page: now_at,
                ordinal: *ordinal,
                ns: *ns,
                local,
            }));
        }
        if original {
            self.gone.insert(u32::try_from(scan_at).unwrap_or(u32::MAX), done.gone);
            self.new_ords.insert(scan_at, done.new_ord);
        }
        self.applied.push((diff.page_at, done.shapes));
    }
}

/// What a part's splice is told besides its pages: the page edits (which master pages and layouts they go into), and the names of the automatic
/// styles an earlier part of this save made, which no new name of this part may be.
struct PartCtx<'c> {
    plans: &'c pages::PagesPlan,
    /// What the model did to the pages of the drawing, and the pages it added, which the splice of `content.xml` writes.
    structure: &'c structure::Structure,
    added: &'c [&'c PageDiff],
    doc: &'c Document,
    taken: &'c [(Fam, String)],
    /// The markers the styles the save made in `content.xml` name: `styles.xml` gets the elements.
    markers: &'c std::collections::BTreeSet<markers::Def>,
    /// The markers of DiaCraft's name that the styles of `content.xml` named before the save and name no longer: `styles.xml` takes them out, those that none of
    /// its own styles names.
    orphans: &'c std::collections::BTreeSet<markers::Def>,
    /// The style sheets the save adds and takes out (`styles.xml` gets them as common styles, or loses them) and the names the file holds each by, which the parent of a
    /// shape's automatic style is written with (`newstyles.rs`).
    styles: &'c newstyles::Plan,
    /// The layers the save adds to the document's layer set of `styles.xml`.
    layers: &'c [layers::NewLayer],
    /// The layers of that set whose flags the model changed, with their new flags.
    layer_flags: &'c [layers::NewLayer],
}

/// The part's `office:automatic-styles`, made at `anchor` (the child it goes before) when the part has none and a style is to be put in it (`wants_book`).
fn auto_styles_at(auto: Option<scan::AutoStyles>, anchor: Option<scan::RootChild>, wants_book: bool) -> Result<Option<scan::AutoStyles>, WriteError> {
    Ok(match (auto, anchor) {
        (None, Some(anchor)) if wants_book => {
            if anchor.declares {
                return Err(WriteError::Unsupported(
                    "a part with no office:automatic-styles whose office:body (or office:master-styles) declares a namespace of its own",
                ));
            }
            let qname = match anchor.qname.split_once(':') {
                Some((prefix, _)) => format!("{prefix}:automatic-styles"),
                None => "automatic-styles".to_owned(),
            };
            Some(scan::AutoStyles {
                open: (anchor.start, anchor.start),
                empty: false,
                close_start: anchor.start,
                end: anchor.start,
                scope: anchor.scope,
                qname,
                made: true,
            })
        }
        (auto, _) => auto,
    })
}

/// Splices one part: inflates it, scans it, applies the pages' changes and makes the edits.
#[allow(clippy::too_many_arguments)]
fn splice_part(
    source: &mut Source<'_, '_>,
    name: &str,
    kind: PartKind,
    pages: &[&PageDiff],
    held: &ShapeIndex,
    part_ctx: &PartCtx<'_>,
    pictures: Option<&mut picture::PictureBook>,
    limits: &Limits,
    work: &mut WriteWork,
    notes: &mut Vec<SaveNote>,
) -> Result<(Vec<u8>, PartOut), WriteError> {
    let part = source.part(name, work)?.ok_or(WriteError::Mismatch("the part a change reaches is not in the package"))?;
    let bytes: &[u8] = &part;
    if diacraft_container::xml_scan::is_utf16(bytes) {
        return Err(WriteError::Unsupported("a change to a part written in UTF-16"));
    }
    // The styles of the part are looked at when a shape is restyled, and when a shape is taken out: its styles may be left with no one to name them.
    // A retext of a custom shape writes into its style too (`draw:auto-grow-height`), so the styles are looked at for one.
    let content = kind == PartKind::Content;
    let structural = content && part_ctx.structure.any();
    let restyled = pages.iter().any(|p| p.shapes.iter().any(|s| s.style.is_some() || s.text.is_some() || s.tier2.is_some()) || !p.new.is_empty())
        || (content && part_ctx.added.iter().any(|p| !p.new.is_empty()));
    let wants_book = restyled || pages.iter().any(|p| !p.deleted.is_empty()) || (content && !part_ctx.structure.deleted.is_empty());
    let mut others: Vec<(Fam, String)> = part_ctx.taken.to_vec();
    // **The names the file's shapes held for their automatic styles**: the model writer writes a style under the name its shape held while the shape's style is as it
    // was, so no style the save makes anew takes such a name.
    // **The names the file's paragraphs and runs held for their automatic styles**: the model writer writes a style under the name its paragraph held while the row it
    // rests on is as it was, so no new style of this save may take such a name.
    if work.model_path && content {
        others.extend(shapes::held_graphic_names(part_ctx.doc, work)?);
        others.extend(text::model::held_names(part_ctx.doc, work)?);
    }
    // The names of the common styles of the other part (`styles.xml`, when this is `content.xml`), which no new automatic style takes: a reader that
    // looks a name up among the common styles as well would find two styles of it. The part's own common styles (a flat drawing's, a master page's in `styles.xml`) are
    // avoided by the scan of its own bytes (`styles::count_refs`: a name some attribute holds, whatever it names).
    let mut common_names: Vec<String> = Vec::new();
    if restyled {
        let (autos, commons) = other_names(source, kind, limits, work)?;
        others.extend(autos);
        common_names = commons;
    }
    let inside: HashSet<(usize, u32)> = pages
        .iter()
        .flat_map(|p| {
            let (PageSource::Page(at) | PageSource::MasterPage(at)) = p.source;
            p.shapes.iter().filter(|s| s.text.is_some() || s.geo.is_some()).map(move |s| (at, s.ord))
        })
        .collect();
    // The styles of `styles.xml` are looked at for the markers they name when a marker may be taken out.
    // (Only the splice of `styles.xml` is given any: `content.xml` is spliced first, and what it orphans is known when it has been.)
    let looks_for_orphans = !part_ctx.orphans.is_empty();
    // A page that moves, is renamed or is added is looked at by its name, its master page and its place: the styles are asked for as for the names of a page.
    // The pages whose own layer set has flags to write: its layers are noted.
    let page_layers: HashSet<usize> = pages
        .iter()
        .filter(|p| !p.layer_flags.is_empty())
        .map(|p| {
            let (PageSource::Page(at) | PageSource::MasterPage(at)) = p.source;
            at
        })
        .collect();
    // The pages whose annotation children a save takes out: those whose comments it rewrites.
    let annotation_pages: HashSet<usize> = if content {
        pages
            .iter()
            .filter(|p| p.held_at.is_some() && p.comments.as_ref().is_some_and(|c| !c.remove.is_empty()))
            .map(|p| {
                let (PageSource::Page(at) | PageSource::MasterPage(at)) = p.source;
                at
            })
            .collect()
    } else {
        HashSet::new()
    };
    // A save that writes a page's comments asks for the prefixes the pages' holder binds to the namespaces an annotation is in.
    let writes_comments = |p: &&PageDiff| p.comments.as_ref().is_some_and(|c| !c.list.is_empty());
    let holder_names = content && (pages.iter().any(writes_comments) || part_ctx.added.iter().any(writes_comments));
    let ask = Ask {
        styles: wants_book || !part_ctx.plans.plans.is_empty() || looks_for_orphans || structural,
        inside,
        page_layers,
        annotation_pages,
        holder_names,
    };
    let scan = scan_part_asking(bytes, kind, &ask, limits, work)?;
    let scan::Scan {
        pages: scan_pages,
        scopes,
        hrefs,
        defs,
        auto,
        layouts,
        kids,
        max_id,
        body,
        drawing_scope,
        version,
        common,
        markers: held_markers,
        common_styles,
        marker_refs,
        faces,
        fonts,
        children,
        master_styles,
        layer_set,
        doc_layers,
        ..
    } = scan;
    // A `content.xml` with no `office:automatic-styles` gets the element, before `office:body` where the schema puts it, when a style is put in it (the scan finds
    // `office:body` in `content.xml` only).
    // A flat drawing puts it at its place in the order: before the first of `office:master-styles` and `office:body`.
    let flat = source.is_flat();
    let child_at =
        |which: &[crate::flat::Child]| which.iter().filter_map(|c| children.get(c.rank()).and_then(Option::as_ref)).min_by_key(|c| c.start).cloned();
    let anchor = if flat {
        child_at(&[crate::flat::Child::MasterStyles, crate::flat::Child::Body])
    } else {
        body.as_ref().map(|b| scan::RootChild { start: b.start, scope: b.scope, qname: b.qname.clone(), declares: b.declares })
    };
    let auto = auto_styles_at(auto, anchor, wants_book)?;
    let office = if flat {
        children.get(crate::flat::Child::Body.rank()).and_then(Option::as_ref).and_then(|c| c.qname.split_once(':')).map(|(p, _)| p.to_owned())
    } else {
        None
    };
    // Where `office:font-face-decls` is made when the document has none (it is asked for then and no other time): a flat drawing's is before the first of its styles
    // children, which R30 puts after it.
    let fonts_at = if flat {
        child_at(&[crate::flat::Child::Styles, crate::flat::Child::AutomaticStyles, crate::flat::Child::MasterStyles, crate::flat::Child::Body])
            .map(|c| c.start)
    } else {
        None
    };
    let book = if wants_book { Some(styles::StyleBook::build(bytes, defs, auto, (&others, &common_names), limits, work)?) } else { None };
    let mut editor = Editor {
        bytes,
        splicer: Splicer::new(bytes),
        scopes,
        work,
        limits,
        notes,
        height: 0.0,
        unit: LenUnit::Cm,
        book,
        kids,
        kid_edits: Vec::new(),
        inserted: Vec::new(),
        zero: Vec::new(),
        tier2_edited: Vec::new(),
        max_id,
        legacy: scan::is_legacy(version.as_deref()),
        markers: std::collections::BTreeSet::new(),
        texts: text::TextStyles::new(faces),
        fonts,
        fonts_at,
        flat,
        office,
        pictures,
        style_names: &part_ctx.styles.names,
        common_text: String::new(),
        kept: shapes::KeptMarkup::default(),
        doc: part_ctx.doc,
        commons: common_names.iter().cloned().collect(),
        rich: None,
        part_names: None,
        picture_names: None,
    };
    let mut out = PageOutputs::default();
    // Where each element of the part is after the save: by the place it has in the scan (the pages a save adds have places after those, in the order the diff has them).
    let ord_map = if content { structure::ordinals(part_ctx.structure, scan_pages.len())? } else { Vec::new() };
    for diff in pages {
        let (PageSource::Page(scan_at) | PageSource::MasterPage(scan_at)) = diff.source;
        let page = scan_pages.get(scan_at).ok_or(WriteError::Mismatch("the part holds fewer pages than the file was read with"))?;
        let held_page = diff.held_at.and_then(|h| held.page(h)).ok_or(WriteError::Mismatch("the index does not hold the pages of the drawing"))?;
        let done = plan::apply_page(&mut editor, page, diff, held_page)?;
        let now_at = if kind == PartKind::Content { ord_map.get(scan_at).copied().unwrap_or(scan_at) } else { scan_at };
        out.record(&mut editor, kind, diff, (scan_at, now_at), true, done);
        // The new flags of the layers of the page's own layer set, written on its `draw:layer` elements as the document's set has them.
        editor.reflag_layers(&page.layers, &diff.layer_flags)?;
    }
    if structural {
        structure::apply(&mut editor, &mut out, &scan_pages, drawing_scope, part_ctx, &ord_map)?;
    }
    let PageOutputs { applied, gone, new_ords, texts, inserted, zero: zeros, read_back, kid_edits, tier2, new_tags } = out;
    let (mut masters_inserted, mut masters_removed) = (Vec::new(), Vec::new());
    match kind {
        // The pages of `content.xml` that were resized name their master page.
        PartKind::Content => {
            for plan in &part_ctx.plans.plans {
                if let (Some(split), Some(at)) = (&plan.split, plan.scan_page) {
                    let page = scan_pages.get(at).ok_or(WriteError::Mismatch("the part holds fewer pages than the file was read with"))?;
                    editor.set_master(page, &split.master)?;
                }
            }
        }
        PartKind::Styles => (masters_inserted, masters_removed) = editor.write_pages(part_ctx.plans, &scan_pages, &layouts)?,
    }
    // The markers the line ends of this save name go into `office:styles` of `styles.xml`: those the styles of `content.xml` name, and those a master page's
    // shape does.
    let mut wrote_markers = std::collections::BTreeSet::new();
    let mut written_markers = std::collections::BTreeSet::new();
    let mut removed_markers = std::collections::BTreeSet::new();
    if kind == PartKind::Styles {
        let mut wanted = part_ctx.markers.clone();
        wanted.extend(editor.markers.iter().copied());
        // A marker the save took out of the styles of `content.xml` goes when no style of this part names it either, as it stood (the master pages' shapes edited
        // by this save are not looked into: one of them that stopped naming a marker leaves it for a later save).
        let gone: std::collections::BTreeSet<markers::Def> =
            part_ctx.orphans.iter().filter(|k| !marker_refs.contains(k) && !wanted.contains(k)).copied().collect();
        (written_markers, removed_markers) = editor.put_markers(common.as_ref(), &held_markers, &wanted, &gone)?;
        wrote_markers = wanted;
        editor.remove_common_styles(&common_styles, &part_ctx.styles.gone)?;
        editor.put_common_styles(common.as_ref(), &part_ctx.styles.new)?;
        editor.flush_common_styles(common.as_ref());
        // The layers the model added, declared where another program reads them, and the new flags of those the set declares.
        editor.reflag_layers(&doc_layers, part_ctx.layer_flags)?;
        editor.put_layers(master_styles.as_ref(), layer_set.as_ref(), part_ctx.layers)?;
    }
    editor.remove_orphans()?;
    // The paragraph styles the shapes of the file named for their text that the text writer has not made.
    if content && editor.work.model_path {
        editor.put_held_paragraphs()?;
    }
    editor.finish_styles();
    editor.count_index_walks();
    editor.finish_faces();
    if editor.texts.style_attributes_left > 0 {
        editor.notes.push(SaveNote::StyleAttributesNotWritten { attributes: std::mem::take(&mut editor.texts.style_attributes_left) });
    }
    let Editor { mut splicer, book, markers: named, texts: text_styles, .. } = editor;
    let (faces_added, restructured) = (text_styles.faces_added, text_styles.restructured);
    let styles = book.map(|b| b.written).unwrap_or_default();
    let written = std::mem::take(&mut splicer.written);
    let mut edited = place_written(&written, &scan_pages, &new_ords, kind)?;
    // The pages of the part made are not in the places the part had them: each attribute is placed on the page it is on now.
    if content {
        for tag in edited.iter_mut() {
            tag.page = ord_map.get(tag.page).copied().unwrap_or(tag.page);
        }
    }
    edited.extend(new_tags);
    drop(written);
    let part = splicer.apply(work, limits)?;
    let markers = if kind == PartKind::Styles { wrote_markers } else { named };
    Ok((
        part,
        PartOut {
            applied,
            hrefs,
            gone,
            edited,
            styles,
            masters_inserted,
            masters_removed,
            texts,
            inserted,
            zero: zeros,
            kid_edits,
            markers,
            markers_written: written_markers,
            markers_removed: removed_markers,
            marker_refs: if kind == PartKind::Content { marker_refs } else { std::collections::BTreeSet::new() },
            faces: faces_added,
            restructured,
            tier2,
            read_back,
        },
    ))
}

/// The markers of DiaCraft's name that the styles of the part the save made (`content.xml`) name no more, of those they named before (`before`, found when the
/// part was scanned for its styles): what `styles.xml` is asked to take out. A part that named none of them is not looked at again.
fn orphaned_markers(
    part: &[u8],
    before: &std::collections::BTreeSet<markers::Def>,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<std::collections::BTreeSet<markers::Def>, WriteError> {
    if before.is_empty() {
        return Ok(std::collections::BTreeSet::new());
    }
    let ask = Ask { styles: true, ..Ask::default() };
    let scan = scan_part_asking(part, PartKind::Content, &ask, limits, work)
        .map_err(|e| WriteError::SelfCheck(format!("the part made does not scan: {e}")))?;
    Ok(before.difference(&scan.marker_refs).copied().collect())
}

/// What [`other_names`] gives: the automatic styles of the other part (a family and a name each), and the names of its common styles.
type OtherNames = (Vec<(Fam, String)>, Vec<String>);

/// The names and families of the automatic styles of the part that `kind` is not, and the names of its common styles: a new name of the part being edited is a name
/// neither has.
fn other_names(source: &mut Source<'_, '_>, kind: PartKind, limits: &Limits, work: &mut WriteWork) -> Result<OtherNames, WriteError> {
    let (name, other) = match kind {
        PartKind::Content => ("styles.xml", PartKind::Styles),
        PartKind::Styles => ("content.xml", PartKind::Content),
    };
    // A flat drawing has one container of automatic styles, which the scan of the part being edited holds whole: no name of another part to avoid.
    if source.is_flat() {
        return Ok((Vec::new(), Vec::new()));
    }
    let Some(bytes) = source.part(name, work)? else { return Ok((Vec::new(), Vec::new())) };
    if diacraft_container::xml_scan::is_utf16(&bytes) {
        return Ok((Vec::new(), Vec::new()));
    }
    let ask = Ask { styles: true, ..Ask::default() };
    let scan = scan_part_asking(&bytes, other, &ask, limits, work)?;
    let commons = scan.common_styles.into_iter().map(|c| c.name).collect();
    Ok((scan.defs.into_iter().map(|d| (d.family, d.name)).collect(), commons))
}

/// Where each attribute written is in the part made: the start tag it was written on is a shape element of the part read, and the plan says
/// the ordinal that element has after the save (an element that is gone has none, and nothing of it is checked). The tags are found by the
/// place of their start in the page's elements, which lie in document order.
fn place_written(
    written: &[splice::WrittenAttr],
    scan_pages: &[scan::ScanPage],
    new_ords: &HashMap<usize, Vec<u32>>,
    kind: PartKind,
) -> Result<Vec<EditedTag>, WriteError> {
    let mut placed: Vec<EditedTag> = Vec::new();
    placed
        .try_reserve_exact(written.len())
        .map_err(|_| WriteError::Limit { what: "memory for the attributes written", max: written.len() as u64 })?;
    let lost = || WriteError::Mismatch("an attribute was written on a tag that is not an element of the page");
    for w in written {
        let page = scan_pages.partition_point(|p| p.end <= w.tag_start);
        let shapes = scan_pages.get(page).map(|p| p.shapes.as_slice()).unwrap_or_default();
        let place = shapes.binary_search_by_key(&w.tag_start, |s| s.open.0).map_err(|_| lost())?;
        let now = new_ords.get(&page).and_then(|ords| ords.get(place + 1)).copied().ok_or_else(lost)?;
        if now != 0 {
            placed.push(EditedTag { kind, page, ordinal: now, ns: w.ns, local: w.local });
        }
    }
    Ok(placed)
}

/// The entries and manifest rows that go with the shapes the model deleted.
fn doomed_entries(
    opened: &mut Opened<'_>,
    diff: &Diff,
    outs: &[(PartKind, PartOut)],
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<objects::Doomed, WriteError> {
    if diff.pages.iter().all(|p| p.deleted.is_empty()) && diff.structure.deleted.is_empty() {
        return Ok(objects::Doomed::default());
    }
    // What the whole drawing names, in both parts: a part the splice did not reach is scanned for its references alone.
    let mut scanned: Vec<Scanned> = Vec::new();
    for (kind, name) in [(PartKind::Content, "content.xml"), (PartKind::Styles, "styles.xml")] {
        match outs.iter().find(|(k, _)| *k == kind) {
            Some((_, out)) => scanned.push((out.hrefs.iter().map(clone_href).collect(), out.gone.clone())),
            None => {
                let Some(at) = opened.archive.position(name) else { continue };
                let bytes = opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?;
                if diacraft_container::xml_scan::is_utf16(&bytes) {
                    continue;
                }
                scanned.push((scan_part(&bytes, kind, limits, work)?.hrefs, HashMap::new()));
            }
        }
    }
    let refs: Vec<PartRefs<'_>> = scanned.iter().map(|(hrefs, gone)| PartRefs { hrefs, gone }).collect();
    let mut names: Vec<&str> = opened.archive.entries().iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    Ok(objects::doomed(&refs, &names))
}

fn clone_href(h: &HrefAt) -> HrefAt {
    HrefAt { page: h.page, shape: h.shape, holder: h.holder, href: h.href.clone() }
}

/// The media type a manifest row gets for a file it omitted, when the extension says: R10 gives `""` when it cannot be told.
pub(crate) fn media_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("xml") => "text/xml",
        _ => "",
    }
}

/// The package-level changes of an edited save: the generator, the thumbnail, the `mimetype` and the manifest rows.
#[allow(clippy::too_many_arguments)]
fn package_changes(
    opened: &mut Opened<'_>,
    options: &WriteOptions,
    version: Option<&str>,
    row_removals: &HashSet<String>,
    new_rows: Vec<(String, String)>,
    changes: &mut Changes,
    notes: &mut Vec<SaveNote>,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    let limits = &options.limits;
    let mut rows: Vec<(String, String)> = new_rows;
    // The generator, unless the model writer says the content is as it was read and the package it made holds the generator the model holds.
    match opened.archive.position("meta.xml") {
        // The model writer wrote `meta.xml` as the model holds it: it is left as it is.
        Some(_) if work.keep_meta => {}
        Some(at) => {
            let bytes = opened.archive.inflate(at, &mut work.package).map_err(WriteError::Package)?;
            match meta::with_generator(&bytes, limits, work)? {
                Some(new) => {
                    changes.replace.insert("meta.xml".to_owned(), new);
                }
                None => notes.push(SaveNote::MetaNotUpdated),
            }
        }
        None => {
            changes.add.push(("meta.xml".to_owned(), meta::new_meta(version).into_bytes(), Method::Deflated));
            rows.push(("meta.xml".to_owned(), "text/xml".to_owned()));
        }
    }
    // The thumbnail.
    if let Some(png) = &options.thumbnail {
        if opened.archive.position(meta::THUMBNAIL).is_some() {
            changes.replace.insert(meta::THUMBNAIL.to_owned(), png.clone());
        } else {
            changes.add.push((meta::THUMBNAIL.to_owned(), png.clone(), Method::Deflated));
            // A manifest that lists the thumbnail already (and has no entry for it: a row for a file the package lacks stays) gets no second row, as a first
            // save gives it none (`first.rs`).
            if opened.manifest.entry(meta::THUMBNAIL).is_none() {
                rows.push((meta::THUMBNAIL.to_owned(), "image/png".to_owned()));
            }
        }
    }
    // The kind asked for, when it is not the file's own, is stated in `mimetype` and in the manifest's root row below; a `mimetype` that is
    // not as R2 asks is written as it asks, with the media type the package says it is.
    match changes.retype {
        Some(retype) => media::state_mimetype(opened, retype.variant, changes),
        None if !opened.mimetype_regular => changes.mimetype.clone_from(&opened.media_type),
        None => {}
    }
    // The files the manifest omitted get a row, after the last. The names queued are kept in a set: a look through the rows for each entry
    // is quadratic in the entries.
    // Each of the three sets is of counted names, so that the comparisons a look makes are counted and a look that walks the set is seen (Ruling
    // Finding L9).
    let compared_before = count::compared();
    let mut probes = 0u64;
    {
        let mut queued: HashSet<Counted<String>> = rows.iter().map(|(n, _)| Counted(n.clone())).collect();
        let dropped: HashSet<Counted<&str>> = changes.remove.iter().map(|n| Counted(n.as_str())).collect();
        let listed: HashSet<Counted<&str>> = opened.manifest.entries.iter().map(|e| Counted(e.full_path.as_str())).collect();
        for entry in opened.archive.entries().iter().filter(|e| !e.folder) {
            probes += 1;
            let named = entry.name == "mimetype" || entry.name.starts_with("META-INF/") || dropped.contains(&Counted(entry.name.as_str()));
            if named {
                continue;
            }
            probes += 1;
            if !listed.contains(&Counted(entry.name.as_str())) {
                probes += 1;
                if queued.insert(Counted(entry.name.clone())) {
                    rows.push((entry.name.clone(), media_for(&entry.name).to_owned()));
                }
            }
        }
    }
    work.manifest_probes = work.manifest_probes.saturating_add(probes);
    work.manifest_compares = work.manifest_compares.saturating_add(count::compared().saturating_sub(compared_before));
    // The root row's media type is written when the kind changes (or the row says nothing); a kind that is the package's own leaves the row as it is.
    let root_media = changes.retype.filter(|r| r.root).map(|r| r.variant.media_type());
    if rows.is_empty() && row_removals.is_empty() && root_media.is_none() {
        return Ok(());
    }
    let bytes = std::mem::take(&mut opened.manifest_bytes);
    let mut manifest = Manifest::scan(&bytes, limits, work)?;
    if let Some(media) = root_media {
        manifest.state_root_media(media)?;
    }
    let edited_manifest = manifest.edit(row_removals, &rows, limits, work)?;
    // A manifest the reader would refuse is no manifest to write: the rows that were added (a picture's, a file the manifest omitted) may take it past its cap.
    if edited_manifest.len() as u64 > limits.max_manifest_bytes {
        return Err(WriteError::Limit { what: "bytes in the manifest", max: limits.max_manifest_bytes });
    }
    changes.replace.insert(MANIFEST_PATH.to_owned(), edited_manifest);
    opened.manifest_bytes = bytes;
    Ok(())
}

/// The page with its master page's place moved on by the master pages copied before it and back by those taken out before it: `inserted` are the
/// places a copy was put after and `removed` the places of those taken out, each sorted.
fn shifted(mut page: PageShapes, inserted: &[usize], removed: &[usize]) -> PageShapes {
    if let PageSource::MasterPage(k) = page.source {
        let (more, fewer) = (inserted.partition_point(|m| *m < k), removed.partition_point(|m| *m < k));
        page.source = PageSource::MasterPage((k + more).saturating_sub(fewer));
    }
    page
}

/// The checksum and size of `content.xml` and `styles.xml` in a package, as its directory states them.
fn fingerprint_of(opened: &Opened<'_>) -> Fingerprint {
    let of = |name: &str| opened.archive.position(name).and_then(|at| opened.archive.entries().get(at)).map_or((0, 0), |e| (e.crc, e.size));
    Fingerprint { content: of("content.xml"), styles: of("styles.xml") }
}

/// [`fingerprint_of`] for the package a save has just made.
fn fingerprint_of_made(bytes: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<Fingerprint, WriteError> {
    let opened = crate::package::open(bytes, limits, &mut work.package).map_err(|e| WriteError::SelfCheck(format!("{e} ({})", e.class())))?;
    Ok(fingerprint_of(&opened))
}

/// The index of the save: each page the splice changed has its new index, every other the one the caller held; the cost is that of the read of
/// the file just before the edits, which the file saved costs nearly the same as.
///
/// **The writer owns repeat saves.** The next save compares the model with the file read again, and a file read again is not the model
/// saved (a new shape comes back with the sections, formulas and attributes a read gives every shape; a shape a connector names comes back with an
/// id the model has not; a page that moved to a copy of its master page comes back naming it). So each page this save made or changed carries the
/// model's page as the model had it, which is what the next save compares the model with for that page; the file read again still says how many
/// elements there are, and the package's fingerprint says the index goes with the bytes. A page this save did not change keeps what it had.
fn new_index(held: &ShapeIndex, outs: &[(PartKind, PartOut)], cost: ReadCost, doc: &Document, diff: &Diff, fingerprint: Fingerprint) -> ShapeIndex {
    let mut changed: HashMap<usize, &PageShapes> = HashMap::new();
    for (_, out) in outs {
        for (at, shapes) in &out.applied {
            changed.insert(*at, shapes);
        }
    }
    // A master page copied is put after the master page it is the copy of, and the master pages after it are one further on. The places are sorted
    // once and each page is one binary search in them (a count of them for each page was quadratic in the pages and the splits).
    let mut inserted: Vec<usize> = outs.iter().flat_map(|(_, out)| out.masters_inserted.iter().copied()).collect();
    inserted.sort_unstable();
    let mut removed: Vec<usize> = outs.iter().flat_map(|(_, out)| out.masters_removed.iter().copied()).collect();
    removed.sort_unstable();
    // A page of the file that stays is put at the place its element has after the save, and each carries the `ID` of the page of the model it is.
    let mut foreground = 0usize;
    let pages = doc
        .pages
        .iter()
        .enumerate()
        .filter_map(|(at, model)| {
            let entry = diff.structure.matched.get(at).copied().flatten();
            let page = match (changed.get(&at), entry) {
                // A page this save changed (every page it applied is one that has a change) or added is read back as more than the model had of it (what its new shapes were
                // written with, the markup of its element): the next save compares the model with the model as it was saved.
                (Some(shapes), _) => (*shapes).clone().with_saved(Arc::clone(model)),
                (None, Some(h)) => held.page(h).cloned()?,
                (None, None) => return None,
            };
            let mut page = shifted(page, &inserted, &removed).with_page_id(model.id);
            if !model.background {
                page.source = PageSource::Page(foreground);
                foreground += 1;
            }
            Some(page)
        })
        .collect();
    ShapeIndex::from_pages(pages, cost).with_fingerprint(fingerprint)
}

/// Checks the bytes just made against the new index (the module documentation, step 5), by a scan of the tags of the two parts and not by a
/// read: a read would build a third model beside the caller's, which a drawing near the reader's caps has not the memory for. The
/// package must open, `content.xml` and `styles.xml` must scan, each page the index names must hold as many shape elements as the index says,
/// and the ordinals the index gives must lie within them, each once.
///
/// **The start tags the save wrote attributes on are read back** (a scan of tags sees neither a tag that names an attribute twice
/// nor a value the reader refuses, and a read of the file made both visible): `edited` says which, sorted by where they are (`content.xml`'s
/// first). Each attribute written is on its tag **once**, by namespace and local name as the reader finds them, and is read by the reader's
/// own parser for it (an attribute of the file's own that shares a name with another is the file's, and is not looked at). The work is linear
/// in the bytes of those tags, and holds no model.
fn self_check(bytes: &[u8], flat: bool, index: &ShapeIndex, edited: &[EditedTag], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut made = Made::open(bytes, flat, limits, work)?;
    let (content_tags, styles_tags) = edited.split_at(edited.partition_point(|t| t.kind == PartKind::Content));
    let mut scanned: Vec<(PartKind, Option<scan::Scan>)> = Vec::new();
    for (kind, name, tags) in [(PartKind::Content, "content.xml", content_tags), (PartKind::Styles, "styles.xml", styles_tags)] {
        let Some(part) = made.part(name, work)? else {
            if !tags.is_empty() {
                return Err(failed("an attribute was written in a part the package does not hold"));
            }
            scanned.push((kind, None));
            continue;
        };
        let mut scan = scan_part(&part, kind, limits, work).map_err(|e| WriteError::SelfCheck(format!("{name} does not scan: {e}")))?;
        check_edited_tags(&part, &mut scan, tags, limits, work)?;
        scanned.push((kind, Some(scan)));
    }
    let pages_of =
        |kind: PartKind| scanned.iter().find(|(k, _)| *k == kind).and_then(|(_, s)| s.as_ref()).map(|s| s.pages.as_slice()).unwrap_or_default();
    let (content, styles) = (pages_of(PartKind::Content), pages_of(PartKind::Styles));
    let drawing_pages = (0..index.pages()).filter(|at| matches!(index.page(*at).map(|p| p.source), Some(PageSource::Page(_)))).count();
    if content.len() != drawing_pages {
        return Err(failed("the pages read back are not the pages saved"));
    }
    for at in 0..index.pages() {
        let Some(planned) = index.page(at) else { continue };
        let page = match planned.source {
            PageSource::Page(k) => content.get(k),
            PageSource::MasterPage(k) => styles.get(k),
        };
        if page.map(|p| p.shapes.len()) != usize::try_from(planned.elements()).ok() {
            return Err(failed("a page does not hold the shape elements the save planned"));
        }
        if !planned.ordinals_fit() {
            return Err(failed("the index names an element twice or one the page does not hold"));
        }
    }
    Ok(())
}

/// Reads back the start tags of `part` that `tags` (those of one part, sorted by element) say attributes were written on: see [`self_check`].
/// What the lookups of prefixes cost is counted apart from the save's own steps, which the edit has used, and is bounded by the bytes of the
/// tags (charged to the same byte cap as the edit's own looks at them).
fn check_edited_tags(part: &[u8], scan: &mut scan::Scan, tags: &[EditedTag], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut own = WriteWork { step_cap: work.step_cap, byte_cap: work.byte_cap, ..WriteWork::default() };
    for group in tags.chunk_by(|a, b| a.key() == b.key()) {
        let Some(first) = group.first() else { continue };
        let Some(shape) = scan.pages.get(first.page).and_then(|p| p.shape(first.ordinal)).copied() else {
            return Err(failed("an element written on is not in the part"));
        };
        let wanted: Vec<(Ns, &'static str)> = group.iter().map(|t| (t.ns, t.local)).collect();
        check_tag(part, &mut scan.scopes, shape.open, shape.scope, &wanted, limits, &mut own)?;
    }
    work.checked_tag_bytes = work.checked_tag_bytes.saturating_add(own.attribute_bytes);
    work.checked_attributes = work.checked_attributes.saturating_add(own.checked_attributes);
    Ok(())
}

/// Reads back the start tag at `open` of `part`, seeing the namespaces in `scope`: each attribute of `wanted` is on it, none twice by its full
/// namespace URI (ODF's `svg:x` and an `x` of the W3C's SVG namespace are two names, though the reader folds both), and every attribute
/// of the folded name is one the reader reads (`read::readback`).
fn check_tag(
    part: &[u8],
    scopes: &mut scan::Scopes,
    open: (usize, usize),
    scope: u32,
    wanted: &[(Ns, &'static str)],
    limits: &Limits,
    own: &mut WriteWork,
) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut spans: Vec<AttrSpan> = Vec::new();
    own.charge_attribute_bytes(open.1.saturating_sub(open.0))?;
    if tag_attributes(part, open.0, open.1, &mut spans).is_none() {
        return Err(failed("a tag written on does not read as a start tag"));
    }
    let names: HashSet<(Ns, &str)> = wanted.iter().copied().collect();
    let mut found: HashMap<(Ns, bool, &str), (usize, usize)> = HashMap::new();
    found.try_reserve(names.len()).map_err(|_| too_much_memory(limits))?;
    // The values of each wanted name as the reader folds it: every attribute that reads as the name written is read back.
    let mut folded: HashMap<(Ns, &str), Vec<(usize, usize)>> = HashMap::new();
    folded.try_reserve(names.len()).map_err(|_| too_much_memory(limits))?;
    for a in &spans {
        let name =
            part.get(a.name.0..a.name.1).and_then(|n| std::str::from_utf8(n).ok()).ok_or_else(|| failed("a tag has a name that is not text"))?;
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let ((ns, w3c), local) = match name.split_once(':') {
            Some((prefix, local)) => (scopes.ns_of_prefix_exact(scope, prefix, own), local),
            None => ((Ns::None, false), name),
        };
        if names.contains(&(ns, local)) {
            if found.insert((ns, w3c, local), a.value).is_some() {
                return Err(failed("a tag names an attribute twice"));
            }
            let values = folded.entry((ns, local)).or_default();
            values.try_reserve(1).map_err(|_| too_much_memory(limits))?;
            values.push(a.value);
        }
    }
    own.check_steps()?;
    own.checked_attributes = own.checked_attributes.saturating_add(spans.len() as u64);
    for &(ns, local) in wanted {
        let Some(values) = folded.get(&(ns, local)) else { return Err(failed("an attribute written is not on its tag")) };
        for &(from, to) in values {
            let text = crate::package::unescape(part.get(from..to).unwrap_or_default());
            if !crate::read::readback::reads(ns, local, &text) {
                return Err(WriteError::SelfCheck(format!("the reader does not read what the save wrote as {local}")));
            }
        }
    }
    Ok(())
}

/// Reads back the page edits in the package made: each page names the master page it should, that master page is there once and names a layout
/// that is there once, and the attributes written into the layout are on it and read by the reader.
fn check_pages(bytes: &[u8], flat: bool, pages: &pages::PagesPlan, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let plans = &pages.plans;
    if plans.is_empty() && pages.new_masters.is_empty() {
        return Ok(());
    }
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut made = Made::open(bytes, flat, limits, work)?;
    let ask = Ask { styles: true, ..Ask::default() };
    let mut scans = Vec::new();
    for (name, kind) in [("content.xml", PartKind::Content), ("styles.xml", PartKind::Styles)] {
        let part = made.part(name, work)?.ok_or_else(|| failed("a page was edited in a part the package does not hold"))?;
        let scan = scan_part_asking(&part, kind, &ask, limits, work).map_err(|e| WriteError::SelfCheck(format!("{name} does not scan: {e}")))?;
        scans.push((part.into_owned(), scan));
    }
    let (Some((styles_part, mut styles)), Some((_, content))) = (scans.pop(), scans.pop()) else { return Err(failed("a part is missing")) };
    let mut own = WriteWork { step_cap: work.step_cap, byte_cap: work.byte_cap, ..WriteWork::default() };
    // The master pages and the layouts are indexed by name once, with how many there are of each name; each plan is one look in each, charged as a
    // step (a walk of every master page and layout for each plan was quadratic in them and charged nothing).
    let mut masters: HashMap<&str, (usize, usize)> = HashMap::new();
    masters.try_reserve(styles.pages.len()).map_err(|_| too_much_memory(limits))?;
    for (i, m) in styles.pages.iter().enumerate() {
        if let Some(n) = m.name.as_deref() {
            masters.entry(n).or_insert((0, i)).0 += 1;
        }
    }
    let mut layouts: HashMap<&str, (usize, usize)> = HashMap::new();
    layouts.try_reserve(styles.layouts.len()).map_err(|_| too_much_memory(limits))?;
    for (i, l) in styles.layouts.iter().enumerate() {
        layouts.entry(l.name.as_str()).or_insert((0, i)).0 += 1;
    }
    for plan in plans {
        if !work.step() || !work.step() {
            work.check_steps()?;
        }
        let page = content.pages.get(plan.made_page).ok_or_else(|| failed("a page that was edited is not in the part made"))?;
        let master = plan.split.as_ref().map_or(plan.master.as_str(), |s| s.master.as_str());
        let layout = plan.split.as_ref().map_or(plan.layout.as_str(), |s| s.layout.as_str());
        if page.master.as_deref() != Some(master) {
            return Err(failed("a page does not name the master page it was written to"));
        }
        let Some(&(1, at)) = masters.get(master) else { return Err(failed("a master page is not there once")) };
        let found = styles.pages.get(at).ok_or_else(|| failed("a master page is not there once"))?;
        if found.layout.as_deref() != Some(layout) {
            return Err(failed("a master page does not name the layout it was written with"));
        }
        let Some(&(1, at)) = layouts.get(layout) else { return Err(failed("a page layout is not there once")) };
        let found = styles.layouts.get(at).ok_or_else(|| failed("a page layout is not there once"))?;
        let props = found.props.ok_or_else(|| failed("a page layout has no properties"))?;
        let wanted: Vec<(Ns, &'static str)> = plan.ops.iter().filter(|o| o.val != stylemap::Val::Remove).map(|o| (o.ns, o.local)).collect();
        check_tag(&styles_part, &mut styles.scopes, props.open, props.scope, &wanted, limits, &mut own)?;
    }
    // A page the save added names the master page it was written with, and is where it was put.
    work.page_steps = work.page_steps.saturating_add(pages.new_masters.len() as u64);
    work.check_steps()?;
    for (made, master) in pages.new_masters.values() {
        let page = content.pages.get(*made).ok_or_else(|| failed("a page that was added is not in the part made"))?;
        if page.master.as_deref() != Some(master.as_str()) {
            return Err(failed("a page that was added does not name the master page it was written to"));
        }
        if !masters.contains_key(master.as_str()) {
            return Err(failed("a master page a new page names is not there"));
        }
    }
    // What no page names after the save is gone.
    if pages.unnamed_masters.iter().any(|(_, name)| masters.contains_key(name.as_str())) {
        return Err(failed("a master page that no page names is still there"));
    }
    work.checked_tag_bytes = work.checked_tag_bytes.saturating_add(own.attribute_bytes);
    work.checked_attributes = work.checked_attributes.saturating_add(own.checked_attributes);
    Ok(())
}

/// Reads back the attributes written on the geometry elements of custom shapes: each is on its element, once, and read by the reader.
fn check_made_kids(bytes: &[u8], flat: bool, outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if outs.iter().all(|(_, out)| out.kid_edits.is_empty()) {
        return Ok(());
    }
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let mut made = Made::open(bytes, flat, limits, work)?;
    for (kind, out) in outs {
        if out.kid_edits.is_empty() {
            continue;
        }
        let name = if *kind == PartKind::Content { "content.xml" } else { "styles.xml" };
        let part = made.part(name, work)?.ok_or_else(|| failed("an outline was written in a part the package does not hold"))?;
        let ask = Ask { styles: false, inside: out.kid_edits.iter().map(|(page, ord, _)| (*page, *ord)).collect(), ..Ask::default() };
        let mut scan =
            scan_part_asking(&part, *kind, &ask, limits, work).map_err(|e| WriteError::SelfCheck(format!("the part made does not scan: {e}")))?;
        let mut own = WriteWork { step_cap: work.step_cap, byte_cap: work.byte_cap, ..WriteWork::default() };
        for (page, ord, attrs) in &out.kid_edits {
            let shape = scan
                .pages
                .get(*page)
                .and_then(|p| p.shape(*ord))
                .copied()
                .ok_or_else(|| failed("a shape whose outline was written is not in the part"))?;
            let geometry = scan
                .kids
                .get(shape.kids.0 as usize..shape.kids.1 as usize)
                .unwrap_or_default()
                .iter()
                .find_map(|k| if let scan::Kid::Geometry { open, scope, .. } = k { Some((*open, *scope)) } else { None });
            let (open, scope) = geometry.ok_or_else(|| failed("a shape whose outline was written has no geometry element"))?;
            check_tag(&part, &mut scan.scopes, open, scope, attrs, limits, &mut own)?;
            work.checked_outlines = work.checked_outlines.saturating_add(1);
        }
        work.checked_tag_bytes = work.checked_tag_bytes.saturating_add(own.attribute_bytes);
        work.checked_attributes = work.checked_attributes.saturating_add(own.checked_attributes);
    }
    Ok(())
}

/// Reads back the markers the splice of `styles.xml` wrote: each is in `office:styles` once, and draws the form this save wrote.
fn check_made_markers(bytes: &[u8], flat: bool, outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let Some((_, out)) = outs.iter().find(|(kind, out)| *kind == PartKind::Styles && !(out.markers.is_empty() && out.markers_removed.is_empty()))
    else {
        return Ok(());
    };
    let mut made = Made::open(bytes, flat, limits, work)?;
    let part =
        made.part("styles.xml", work)?.ok_or_else(|| WriteError::SelfCheck("a marker was written in a part the package does not hold".to_owned()))?;
    markers::check(&part, &out.markers, &out.markers_written, &out.markers_removed, limits, work)
}

/// Reads back the paragraphs each part's splice rewrote, in the part as the package now holds it.
fn check_made_texts(bytes: &[u8], flat: bool, outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if outs.iter().all(|(_, out)| out.texts.is_empty()) {
        return Ok(());
    }
    let mut made = Made::open(bytes, flat, limits, work)?;
    for (kind, out) in outs {
        if out.texts.is_empty() {
            continue;
        }
        let name = if *kind == PartKind::Content { "content.xml" } else { "styles.xml" };
        let part =
            made.part(name, work)?.ok_or_else(|| WriteError::SelfCheck("a text was written in a part the package does not hold".to_owned()))?;
        text::check_paragraphs(&part, *kind, (&out.texts, &out.inserted, &out.zero), limits, work)?;
    }
    Ok(())
}

/// Reads back the styles each part's splice made, in the part as the package now holds it.
fn check_made_styles(bytes: &[u8], flat: bool, outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if outs.iter().all(|(_, out)| out.styles.is_empty() && out.faces.is_empty()) {
        return Ok(());
    }
    let mut made = Made::open(bytes, flat, limits, work)?;
    for (kind, out) in outs {
        if out.styles.is_empty() && out.faces.is_empty() {
            continue;
        }
        let name = if *kind == PartKind::Content { "content.xml" } else { "styles.xml" };
        let part = made.part(name, work)?.ok_or_else(|| WriteError::SelfCheck("a style was made in a part the package does not hold".to_owned()))?;
        check_styles(&part, &out.styles, &out.faces, *kind, limits, work)?;
    }
    Ok(())
}

/// The most bytes of a package the read-back of the paragraphs reads again.
const MOST_READ_BACK: usize = 16 << 20;

/// Reads back, **through the reader**, the paragraphs of the shapes whose text had a paragraph put in, taken out or made a list's (the review:
/// a paragraph written where the reader does not look is lost without a word, and the scan of the part sees it as written): the file made is read again, and each
/// shape has the number of paragraphs the model has, and each paragraph the save wrote reads as the text the model has. The read is of the whole package, so that
/// a package past [`MOST_READ_BACK`] bytes is left to the checks of the scan of the part ([`text::check_paragraphs`], which sees a paragraph that is no list item's
/// child in a list); the bytes read are charged to the bytes a save may scan.
fn check_read_back(bytes: &[u8], outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if !outs.iter().any(|(_, out)| out.restructured && !out.read_back.is_empty()) || bytes.len() > MOST_READ_BACK {
        return Ok(());
    }
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    work.charge_scan(bytes.len(), limits)?;
    work.read_back_bytes = work.read_back_bytes.saturating_add(bytes.len() as u64);
    let (doc, report) = crate::read::read(bytes, limits).map_err(|e| WriteError::SelfCheck(format!("the file made does not read again: {e}")))?;
    // The element each shape asked about has, as the id the read gave it: one pass over the ordinals of each page that is asked about.
    let mut wanted: HashMap<(usize, u32), Option<u32>> = HashMap::new();
    for (_, out) in outs.iter().filter(|(_, out)| out.restructured) {
        for rb in &out.read_back {
            wanted.try_reserve(1).map_err(|_| too_much_memory(limits))?;
            wanted.insert((rb.page_at, rb.ordinal), None);
        }
    }
    let pages: std::collections::BTreeSet<usize> = wanted.keys().map(|(page, _)| *page).collect();
    // Each page asked about is walked **once** for its shapes by `ID`: a search of the page from the top for each shape checked
    // (`Document::find_shape`) was a pass over the page for each of them, quadratic in the shapes whose text was written, and uncounted.
    let mut by_page: HashMap<usize, HashMap<u32, &diacraft_doc::Shape>> = HashMap::new();
    for page in pages {
        let shapes = report.index.page(page).ok_or_else(|| failed("a page whose text was written is not read again"))?;
        let held = shapes.ordinal_map().len();
        work.order_steps = work.order_steps.saturating_add(held as u64);
        for (id, ordinal) in shapes.ordinal_map() {
            if let Some(slot) = wanted.get_mut(&(page, *ordinal)) {
                *slot = Some(*id);
            }
        }
        let model = doc.pages.get(page).ok_or_else(|| failed("a page whose text was written is not read again"))?;
        by_page.try_reserve(1).map_err(|_| too_much_memory(limits))?;
        by_page.insert(page, shapes_by_id(model, held, work, limits)?);
    }
    for (_, out) in outs.iter().filter(|(_, out)| out.restructured) {
        for rb in &out.read_back {
            let id =
                wanted.get(&(rb.page_at, rb.ordinal)).copied().flatten().ok_or_else(|| failed("a shape whose text was written is not read again"))?;
            let shape = by_page
                .get(&rb.page_at)
                .and_then(|shapes| shapes.get(&id))
                .copied()
                .ok_or_else(|| failed("a shape whose text was written is not read again"))?;
            let text: String = shape
                .text
                .iter()
                .flat_map(|t| t.parts.iter())
                .filter_map(|p| if let diacraft_doc::TextPart::Chars(c) = p { Some(c.as_ref()) } else { None })
                .collect();
            // One for the shape, and one for each paragraph of it that is compared.
            work.order_steps = work.order_steps.saturating_add(1).saturating_add(rb.written.len() as u64);
            let read: Vec<&str> = text.split('\n').collect();
            if read.len() != rb.paragraphs {
                return Err(failed(
                    "the reader reads a shape with another number of paragraphs than the model has: a paragraph was written where it is not read",
                ));
            }
            for (index, expected) in &rb.written {
                if read.get(*index).copied() != Some(expected.as_str()) {
                    return Err(failed("the reader reads a paragraph that was written as another text than the model's"));
                }
            }
        }
    }
    Ok(())
}

/// The shapes of `page` by their `ID`, members of a group included: the first of an `ID` in document order, a group's members after the group and before the next shape,
/// as `Document::find_shape` finds one (a page may hold an `ID` twice), down to the depth it searches. One pass over the page, iterative; `held` is the number of elements
/// the file's page has, which the map is made room for, and the shapes visited are counted as steps.
fn shapes_by_id<'a>(
    page: &'a diacraft_doc::Page,
    held: usize,
    work: &mut WriteWork,
    limits: &Limits,
) -> Result<HashMap<u32, &'a diacraft_doc::Shape>, WriteError> {
    let mut by_id: HashMap<u32, &diacraft_doc::Shape> = HashMap::new();
    by_id.try_reserve(held).map_err(|_| too_much_memory(limits))?;
    // The shapes still to visit, with the level each is on, the next one to visit last.
    let mut stack: Vec<(&Arc<diacraft_doc::Shape>, usize)> = page.shapes.iter().map(|shape| (shape, 1)).collect();
    stack.reverse();
    let mut visited = 0u64;
    while let Some((shape, level)) = stack.pop() {
        visited = visited.saturating_add(1);
        by_id.entry(shape.id).or_insert(&**shape);
        if level < diacraft_doc::MAX_SHAPE_DEPTH {
            let at = stack.len();
            stack.extend(shape.children.iter().map(|child| (child, level + 1)));
            if let Some(added) = stack.get_mut(at..) {
                added.reverse();
            }
        }
    }
    work.order_steps = work.order_steps.saturating_add(visited);
    Ok(by_id)
}

/// Reads the file made again and compares each shape the save wrote with tier 2 with what it carried: the file must give back the cells, sections, text,
/// names and links to a master that tier 2 holds, and not the ones it said are gone (`ext::native::reads_back`). **This is what makes a rebuild predicted too
/// kindly a refused save and never a silent loss.** What is written natively is not compared here: the file's own markup has the checks of its own, and a read
/// adds cells and sections of its own (the layer, the text's rows) that a model that was never read does not have. A package past the most the read back reads
/// is left unchecked, as the check of the paragraphs leaves it.
fn check_tier2(bytes: &[u8], outs: &[(PartKind, PartOut)], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    if !outs.iter().any(|(_, out)| !out.tier2.is_empty()) || bytes.len() > MOST_READ_BACK {
        return Ok(());
    }
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    work.charge_scan(bytes.len(), limits)?;
    work.read_back_bytes = work.read_back_bytes.saturating_add(bytes.len() as u64);
    let (doc, report) = crate::read::read(bytes, limits).map_err(|e| WriteError::SelfCheck(format!("the file made does not read again: {e}")))?;
    // Each page read again is indexed once: the `ID` of each element ordinal, and each shape by its `ID`, members of a group included (the security review: a
    // search of the page for each shape that carries tier 2 was a pass over the page for each of them, counted and not held to a cap).
    type PageIndex<'a> = (HashMap<u32, u32>, HashMap<u32, &'a diacraft_doc::Shape>);
    let mut pages: HashMap<usize, PageIndex<'_>> = HashMap::new();
    for (_, out) in outs {
        for (page_at, ordinal, carried) in &out.tier2 {
            work.order_steps = work.order_steps.saturating_add(1);
            if !pages.contains_key(page_at) {
                let shapes = report.index.page(*page_at).ok_or_else(|| failed("a page whose shape carries tier 2 is not read again"))?;
                let page = doc.pages.get(*page_at).ok_or_else(|| failed("a page whose shape carries tier 2 is not read again"))?;
                let held = shapes.ordinal_map().len();
                work.order_steps = work.order_steps.saturating_add(held as u64);
                work.charge_steps(held.saturating_mul(2))?;
                let ids: HashMap<u32, u32> = shapes.ordinal_map().iter().map(|(id, o)| (*o, *id)).collect();
                let mut by_id: HashMap<u32, &diacraft_doc::Shape> = HashMap::new();
                let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().collect();
                while let Some(shape) = stack.pop() {
                    by_id.entry(shape.id).or_insert(&**shape);
                    stack.extend(shape.children.iter());
                }
                pages.insert(*page_at, (ids, by_id));
            }
            let (ids, by_id) = pages.get(page_at).ok_or_else(|| failed("a page whose shape carries tier 2 is not read again"))?;
            let read = ids.get(ordinal).and_then(|id| by_id.get(id)).ok_or_else(|| failed("a shape that carries tier 2 is not read again"))?;
            // The `ID` written is the one the shape is read with: an `ID` that an earlier shape holds too reads the shape as a copy of it, with none of the data.
            if carried.id.is_some_and(|written| written != read.id) {
                return Err(failed("a shape that carries tier 2 is read with another ID than the one written: an earlier shape holds it too"));
            }
            work.cells_compared = work.cells_compared.saturating_add(1);
            if let Err(what) = crate::ext::native::reads_back(carried, read, &mut work.cells_compared) {
                return Err(failed(&format!(
                    "a shape that carries tier 2 is not read back as the model has it: {what} of the shape with ID {}",
                    read.id
                )));
            }
        }
    }
    Ok(())
}

/// Reads back the styles the save made in `part` (a scan of the part made, asking for its styles): each is there once, as a style of its family,
/// and the attributes written on it are on its graphic properties, read by the reader.
fn check_styles(
    part: &[u8],
    made: &[styles::Written],
    faces: &[(String, String)],
    kind: PartKind,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let ask = Ask { styles: true, ..Ask::default() };
    let mut scan =
        scan_part_asking(part, kind, &ask, limits, work).map_err(|e| WriteError::SelfCheck(format!("the part made does not scan: {e}")))?;
    let mut own = WriteWork { step_cap: work.step_cap, byte_cap: work.byte_cap, ..WriteWork::default() };
    // The definitions are indexed by name once, and each made style is one look in the index, charged as a step (a walk of every definition for each
    // style made was quadratic in them and uncharged). **By name and family**: the names of ODF's styles are unique within a family, and a style of another family may have the name of the
    // one the save made (a presentation style `gr1` beside a graphic style `gr1`, found by the fuzz run, and a refusal of the save as its own fault before).
    let mut by_name: HashMap<(&str, Fam), (usize, usize)> = HashMap::new();
    by_name.try_reserve(scan.defs.len()).map_err(|_| too_much_memory(limits))?;
    for (i, d) in scan.defs.iter().enumerate() {
        let slot = by_name.entry((d.name.as_str(), d.family)).or_insert((0, i));
        slot.0 += 1;
    }
    // The faces the save declared: each is there once, for the family it was made for.
    if !faces.is_empty() {
        let mut declared: HashMap<&str, (usize, &str)> = HashMap::new();
        declared.try_reserve(scan.faces.len()).map_err(|_| too_much_memory(limits))?;
        for f in &scan.faces {
            let slot = declared.entry(f.name.as_str()).or_insert((0, f.family.as_str()));
            slot.0 += 1;
        }
        for (name, family) in faces {
            if !work.step() {
                work.check_steps()?;
            }
            if declared.get(name.as_str()) != Some(&(1, family.as_str())) {
                return Err(failed("a font face the save declared is not there once, for its family"));
            }
        }
    }
    for style in made {
        if !work.step() {
            work.check_steps()?;
        }
        let Some(&(1, at)) = by_name.get(&(style.name.as_str(), style.family)) else {
            // One walk, on the way out: a style of the name that is of another family only is told so.
            let elsewhere =
                !scan.defs.iter().any(|d| d.name == style.name && d.family == style.family) && scan.defs.iter().any(|d| d.name == style.name);
            return Err(failed(if elsewhere { "a style the save made is of another family" } else { "a style the save made is not there once" }));
        };
        let def = scan.defs.get(at).ok_or_else(|| failed("a style the save made is not there once"))?;
        if style.attrs.is_empty() {
            continue;
        }
        let Some(props) = def.props.get(style.prop).copied().flatten() else {
            return Err(failed("a style the save made has none of the properties it was written with"));
        };
        check_tag(part, &mut scan.scopes, props.open, props.scope, &style.attrs, limits, &mut own)?;
    }
    work.checked_tag_bytes = work.checked_tag_bytes.saturating_add(own.attribute_bytes);
    work.checked_attributes = work.checked_attributes.saturating_add(own.checked_attributes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// `shifted_path` is the writer's own shift (`route::shifted`), public for the fuzz target: a path moved down the page, and `None` for one that cannot be placed.
    #[test]
    fn shifted_path_is_the_writers_own_shift_of_a_connectors_path() {
        assert_eq!(shifted_path("M 4000 10000 v 2000", 2000.0).as_deref(), Some("M 4000 12000 v 2000"));
        assert_eq!(shifted_path("M 1 2 L 3 4", -10.0).as_deref(), Some("M 1 -8 L 3 -6"));
        assert_eq!(shifted_path("M 1 2 X", 10.0), None);
        assert_eq!(shifted_path("M 1 2", f64::NAN), None);
    }

    /// A cap error's numbers: which cap it names and the number.
    fn limit(r: Result<(), WriteError>) -> Option<(&'static str, u64)> {
        match r {
            Err(WriteError::Limit { what, max }) => Some((what, max)),
            _ => None,
        }
    }

    /// A part of `n` automatic graphic styles `gr1` to `grN`, each with graphic properties holding `draw:fill`, and the `n` styles a save made.
    fn made_styles(n: usize) -> (String, Vec<styles::Written>) {
        let body: String = (1..=n)
            .map(|k| format!(r#"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:fill="solid"/></style:style>"#))
            .collect();
        let part = format!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><office:automatic-styles>{body}</office:automatic-styles><office:body><office:drawing><draw:page/></office:drawing></office:body></office:document-content>"#
        );
        let made =
            (1..=n).map(|k| styles::Written { name: format!("gr{k}"), family: Fam::Graphic, attrs: vec![(Ns::Draw, "fill")], prop: 0 }).collect();
        (part, made)
    }

    /// A package of the two parts as given.
    fn package_of(content: &str, styles: &str) -> Vec<u8> {
        use diacraft_container::zip::ArchiveWriter;
        let manifest = concat!(
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
            r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
            r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
            r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
        );
        let mut w = ArchiveWriter::new();
        let parts = [
            ("mimetype", "application/vnd.oasis.opendocument.graphics", Method::Stored),
            ("content.xml", content, Method::Deflated),
            ("styles.xml", styles, Method::Deflated),
            ("META-INF/manifest.xml", manifest, Method::Deflated),
        ];
        for (name, data, method) in parts {
            assert!(w.add(name, data.as_bytes(), method).is_ok());
        }
        w.finish().unwrap_or_default()
    }

    /// A package of `pages` pages, page `i` on master page `M{i}` of layout `L{i}`, and `extra` more master pages and layouts no page names.
    fn pages_package(pages: usize, extra: usize) -> Vec<u8> {
        use diacraft_container::zip::ArchiveWriter;
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#
        );
        let manifest = concat!(
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
            r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
            r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
            r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
        );
        let n = pages + extra;
        let draw: String = (0..pages).map(|i| format!(r#"<draw:page draw:name="p{i}" draw:master-page-name="M{i}"/>"#)).collect();
        let content =
            format!("<office:document-content {root}><office:body><office:drawing>{draw}</office:drawing></office:body></office:document-content>");
        let layouts: String = (0..n)
            .map(|i| format!(r#"<style:page-layout style:name="L{i}"><style:page-layout-properties fo:page-width="1cm"/></style:page-layout>"#))
            .collect();
        let masters: String = (0..n).map(|i| format!(r#"<style:master-page style:name="M{i}" style:page-layout-name="L{i}"/>"#)).collect();
        let styles = format!(
            "<office:document-styles {root}><office:automatic-styles>{layouts}</office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"
        );
        let mut w = ArchiveWriter::new();
        let parts = [
            ("mimetype", "application/vnd.oasis.opendocument.graphics", Method::Stored),
            ("content.xml", content.as_str(), Method::Deflated),
            ("styles.xml", styles.as_str(), Method::Deflated),
            ("META-INF/manifest.xml", manifest, Method::Deflated),
        ];
        for (name, data, method) in parts {
            assert!(w.add(name, data.as_bytes(), method).is_ok());
        }
        w.finish().unwrap_or_default()
    }

    /// The splice of one part, as the checks of the bytes made are given it: nothing but what they look at.
    fn out_of(texts: Vec<PagedParas>, kid_edits: Vec<PagedAttrs>) -> PartOut {
        PartOut {
            inserted: Vec::new(),
            zero: Vec::new(),
            applied: Vec::new(),
            hrefs: Vec::new(),
            gone: HashMap::new(),
            edited: Vec::new(),
            styles: Vec::new(),
            masters_inserted: Vec::new(),
            masters_removed: Vec::new(),
            texts,
            kid_edits,
            markers: std::collections::BTreeSet::new(),
            markers_written: std::collections::BTreeSet::new(),
            markers_removed: std::collections::BTreeSet::new(),
            marker_refs: std::collections::BTreeSet::new(),
            faces: Vec::new(),
            restructured: false,
            tier2: Vec::new(),
            read_back: Vec::new(),
        }
    }

    /// The read-back of what a save wrote is shown to run. The paragraphs, the geometry elements and the layouts the save wrote
    /// are read back in the package made, each counted; what is not as written is a refusal of the check of the bytes made, and what is, is not.
    #[test]
    fn the_read_back_of_the_zero_width_elements_refuses_a_paragraph_that_does_not_hold_what_was_put_in_it() {
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
            r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#
        );
        // Paragraph 0: a bookmark in the middle; 1: a soft page break first and a field last; 2: a mark at each end.
        let content = format!(
            r#"<office:document-content {root}><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="M0"><draw:custom-shape svg:width="1cm" svg:height="1cm"><text:p>ab<text:bookmark text:name="B"/>cd</text:p><text:p><text:soft-page-break/>ef<text:title>T</text:title></text:p><text:p><text:bookmark text:name="S"/>gh<text:bookmark text:name="E"/></text:p><draw:enhanced-geometry svg:viewBox="0 0 10 10" draw:enhanced-path="M 0 0 L 10 10 N"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"#
        );
        let styles = format!(
            r#"<office:document-styles {root}><office:automatic-styles><style:page-layout style:name="L0"><style:page-layout-properties fo:page-width="1cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="M0" style:page-layout-name="L0"/></office:master-styles></office:document-styles>"#
        );
        let bytes = package_of(&content, &styles);
        let limits = Limits::default();
        let (bookmark, soft, title) = ("<text:bookmark text:name=\"B\"/>", "<text:soft-page-break/>", "<text:title>T</text:title>");
        let (start, end) = ("<text:bookmark text:name=\"S\"/>", "<text:bookmark text:name=\"E\"/>");
        let check = |zero: Vec<zero::Expect>| {
            let paras = vec![(0, "abcd".to_owned()), (1, "efT".to_owned()), (2, "gh".to_owned())];
            let mut out = out_of(vec![(0, 1, paras)], Vec::new());
            out.zero = vec![(0, 1, zero)];
            let mut work = WriteWork::default();
            let r = check_made_texts(&bytes, false, &[(PartKind::Content, out)], &limits, &mut work);
            (r, work.checked_zero)
        };
        let exact = |index: usize, items: &[(usize, &str)]| zero::Expect::Exact {
            index,
            items: items.iter().map(|(at, s)| (*at, (*s).to_owned())).collect(),
        };
        let edges = |index: usize, start: &[&str], end: &[&str]| zero::Expect::Edges {
            index,
            start: start.iter().map(|s| (*s).to_owned()).collect(),
            end: end.iter().map(|s| (*s).to_owned()).collect(),
        };
        // What is there is accepted, and each paragraph checked is counted.
        let (r, n) = check(vec![exact(0, &[(2, bookmark)]), exact(1, &[(0, soft), (2, title)]), edges(2, &[start], &[end])]);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(n, 3, "each paragraph that took an element was read back");
        // An element at another offset, another bytes, one missing, one more than was put in: each refused.
        for (what, zero) in [
            ("another offset", exact(0, &[(1, bookmark)])),
            ("other bytes", exact(0, &[(2, "<text:bookmark text:name=\"X\"/>")])),
            ("one missing", exact(1, &[(0, soft)])),
            ("one more", exact(0, &[(2, bookmark), (3, soft)])),
            ("none where one is", exact(0, &[])),
            ("the order", exact(1, &[(2, title), (0, soft)])),
            ("a start that is not at the start", edges(2, &[end], &[])),
            ("an end that is not at the end", edges(2, &[], &[start])),
            ("a start that is not there", edges(0, &[bookmark], &[])),
            ("more at the end than there are", edges(2, &[], &[end, end])),
            ("a paragraph that is not there", exact(7, &[])),
        ] {
            let (r, _) = check(vec![zero]);
            let words = why(r);
            assert!(words.contains("zero-width elements") || words.contains("took a zero-width element is not in the part"), "{what}: {words}");
        }
    }

    #[test]
    fn the_read_back_of_the_paragraphs_the_geometry_and_the_page_edits_runs_and_refuses_what_is_not_as_written() {
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
            r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#
        );
        let content = format!(
            r#"<office:document-content {root}><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="M0"><draw:custom-shape svg:width="1cm" svg:height="1cm"><text:p>hello</text:p><draw:enhanced-geometry svg:viewBox="0 0 10 10" draw:enhanced-path="M 0 0 L 10 10 N"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"#
        );
        let styles = format!(
            r#"<office:document-styles {root}><office:automatic-styles><style:page-layout style:name="L0"><style:page-layout-properties fo:page-width="1cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="M0" style:page-layout-name="L0"/></office:master-styles></office:document-styles>"#
        );
        let bytes = package_of(&content, &styles);
        let limits = Limits::default();
        // The paragraphs: the one written reads back, and is counted; another text is refused.
        let texts = |paras: Vec<(usize, String)>| [(PartKind::Content, out_of(vec![(0, 1, paras)], Vec::new()))];
        let mut work = WriteWork::default();
        assert!(check_made_texts(&bytes, false, &texts(vec![(0, "hello".to_owned())]), &limits, &mut work).is_ok());
        assert_eq!(work.checked_paragraphs, 1, "the paragraph was read back");
        assert!(
            why(check_made_texts(&bytes, false, &texts(vec![(0, "bye".to_owned())]), &limits, &mut WriteWork::default()))
                .contains("as the text the model has")
        );
        assert!(
            why(check_made_texts(&bytes, false, &texts(vec![(3, "x".to_owned())]), &limits, &mut WriteWork::default())).contains("not in the part")
        );
        // The geometry element: the attributes written are on it; one that is not is refused.
        let kids = |attrs: Vec<(Ns, &'static str)>| [(PartKind::Content, out_of(Vec::new(), vec![(0, 1, attrs)]))];
        let mut work = WriteWork::default();
        assert!(check_made_kids(&bytes, false, &kids(vec![(Ns::Svg, "viewBox"), (Ns::Draw, "enhanced-path")]), &limits, &mut work).is_ok());
        assert_eq!(work.checked_outlines, 1, "the geometry element was read back");
        assert!(
            why(check_made_kids(&bytes, false, &kids(vec![(Ns::Draw, "modifiers")]), &limits, &mut WriteWork::default())).contains("not on its tag")
        );
        // The page edits: the attribute written into the layout is on it; one that is not is refused, and what was taken out is not asked for.
        let plan = |ops: Vec<stylemap::Op>| pages::PagesPlan {
            plans: vec![pages::PagePlan { scan_page: Some(0), made_page: 0, master: "M0".to_owned(), layout: "L0".to_owned(), split: None, ops }],
            ..Default::default()
        };
        let op = |local: &'static str, val: stylemap::Val| stylemap::Op { prop: 3, ns: Ns::Fo, local, val };
        let width = plan(vec![op("page-width", stylemap::Val::Text("1cm".to_owned()))]);
        assert!(check_pages(&bytes_for_pages(&styles, &content), false, &width, &limits, &mut WriteWork::default()).is_ok());
        let height = plan(vec![op("page-height", stylemap::Val::Text("2cm".to_owned()))]);
        assert!(why(check_pages(&bytes_for_pages(&styles, &content), false, &height, &limits, &mut WriteWork::default())).contains("not on its tag"));
        let removed = plan(vec![op("page-height", stylemap::Val::Remove)]);
        assert!(check_pages(&bytes_for_pages(&styles, &content), false, &removed, &limits, &mut WriteWork::default()).is_ok());
        // A master page that should be gone and is not.
        let unnamed = pages::PagesPlan { unnamed_masters: vec![(0, "M0".to_owned())], ..width.clone() };
        assert!(
            why(check_pages(&bytes_for_pages(&styles, &content), false, &unnamed, &limits, &mut WriteWork::default()))
                .contains("no page names is still there")
        );
        // A page the save added (no test made these two refusals fire): it is in the part made, names the master page it was written with, and that master
        // page is in `styles.xml`.
        let added =
            |made: usize, master: &str| pages::PagesPlan { new_masters: HashMap::from([(0, (made, master.to_owned()))]), ..Default::default() };
        let mut work = WriteWork::default();
        assert!(check_pages(&bytes_for_pages(&styles, &content), false, &added(0, "M0"), &limits, &mut work).is_ok());
        assert_eq!(work.page_steps, 1, "each page added is one step");
        assert!(
            why(check_pages(&bytes_for_pages(&styles, &content), false, &added(0, "M9"), &limits, &mut WriteWork::default()))
                .contains("a page that was added does not name the master page it was written to")
        );
        assert!(
            why(check_pages(&bytes_for_pages(&styles, &content), false, &added(7, "M0"), &limits, &mut WriteWork::default()))
                .contains("a page that was added is not in the part made")
        );
        let ghost = content.replace("M0", "Ghost");
        assert!(
            why(check_pages(&bytes_for_pages(&styles, &ghost), false, &added(0, "Ghost"), &limits, &mut WriteWork::default()))
                .contains("a master page a new page names is not there")
        );
        // A cap one under the steps of that save (the scan's and the page's) refuses at the step the pages added are counted in.
        let mut tight = WriteWork { step_cap: work.steps_used() - 1, ..WriteWork::default() };
        let refused = check_pages(&bytes_for_pages(&styles, &content), false, &added(0, "M0"), &limits, &mut tight);
        assert!(limit(refused.clone()).is_some_and(|(what, _)| what.contains("steps")), "the pages added are counted against the cap: {refused:?}");
    }

    fn bytes_for_pages(styles: &str, content: &str) -> Vec<u8> {
        package_of(content, styles)
    }

    #[test]
    fn the_check_of_the_page_edits_is_one_look_in_an_index_of_the_master_pages_and_the_layouts_for_each_plan_and_is_charged() {
        // Security finding (MEDIUM): each plan walked every master page and every layout, uncharged. The steps are two for each plan, and the same
        // whether the part holds 10 more master pages and layouts or 2,000.
        let limits = Limits::default();
        let mut steps = Vec::new();
        for extra in [10usize, 2_000] {
            let bytes = pages_package(50, extra);
            let plans = pages::PagesPlan {
                plans: (0..50)
                    .map(|i| pages::PagePlan {
                        scan_page: Some(i),
                        made_page: i,
                        master: format!("M{i}"),
                        layout: format!("L{i}"),
                        split: None,
                        ops: Vec::new(),
                    })
                    .collect(),
                ..Default::default()
            };
            let mut work = WriteWork::default();
            assert!(check_pages(&bytes, false, &plans, &limits, &mut work).is_ok(), "{extra}");
            steps.push(work.scope_steps);
        }
        assert!(steps[0] >= 100, "two looks for each of 50 plans are steps: {steps:?}");
        assert_eq!(steps[0], steps[1], "the cost follows the plans and not the master pages: {steps:?}");
        // A cap under the count refuses.
        let bytes = pages_package(50, 0);
        let plans = pages::PagesPlan {
            plans: (0..50)
                .map(|i| pages::PagePlan {
                    scan_page: Some(i),
                    made_page: i,
                    master: format!("M{i}"),
                    layout: format!("L{i}"),
                    split: None,
                    ops: Vec::new(),
                })
                .collect(),
            ..Default::default()
        };
        let mut work = WriteWork { step_cap: 60, ..WriteWork::default() };
        assert!(limit(check_pages(&bytes, false, &plans, &limits, &mut work)).is_some_and(|(what, max)| what.contains("steps") && max == 60));
        // A plan whose master page is missing, or is there twice, is refused.
        let mut wrong = plans.clone();
        wrong.plans[0].master = "nowhere".to_owned();
        assert!(why(check_pages(&bytes, false, &wrong, &limits, &mut WriteWork::default())).contains("master"));
    }

    #[test]
    fn master_pages_are_moved_on_by_the_copies_before_them_with_a_search_and_not_a_count_for_each_page() {
        let page = |k| PageShapes::new(PageSource::MasterPage(k));
        let moved = |k, inserted: &[usize]| match shifted(page(k), inserted, &[]).source {
            PageSource::MasterPage(n) => n,
            PageSource::Page(n) => n,
        };
        // A copy after master page 1 moves those after it, and two copies after it move them by two; those up to it stay.
        assert_eq!((moved(0, &[1]), moved(1, &[1]), moved(2, &[1]), moved(5, &[1, 1])), (0, 1, 3, 7));
        assert_eq!(shifted(PageShapes::new(PageSource::Page(4)), &[0], &[0]).source, PageSource::Page(4), "a page of the drawing stays");
        // A master page taken out moves those after it back by one, and a copy after one and a removal before it cancel.
        let after = |k, inserted: &[usize], removed: &[usize]| match shifted(page(k), inserted, removed).source {
            PageSource::MasterPage(n) | PageSource::Page(n) => n,
        };
        assert_eq!((after(0, &[], &[1]), after(1, &[], &[1]), after(2, &[], &[1]), after(5, &[], &[1, 3])), (0, 1, 1, 3));
        assert_eq!(after(4, &[1], &[0]), 4, "one more and one less");
        // Security finding (MEDIUM): 100,000 master pages and 50,000 copies; a count of the copies for each page is 5 * 10^9 steps, a search is
        // under a million.
        let inserted: Vec<usize> = (0..50_000).map(|i| i * 2).collect();
        let started = std::time::Instant::now();
        let total: usize = (0..100_000).map(|k| moved(k, &inserted)).sum();
        assert!(total > 0 && started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
        // The same answers as a count, over a range.
        for k in 0..200 {
            assert_eq!(moved(k, &inserted[..20]), k + inserted[..20].iter().filter(|m| **m < k).count());
        }
    }

    #[test]
    fn the_check_of_the_styles_made_is_one_look_in_an_index_of_the_definitions_for_each_style_and_is_charged() {
        // Security finding (MEDIUM): a walk of every definition for each style made was quadratic in them and charged no step. Chained sizes of
        // the same shape: the steps the check takes are the styles it looks up, one each, at 500, 2,000 and 8,000 (a ratio of 4, not 16).
        let limits = Limits::default();
        let mut steps = Vec::new();
        for n in [500usize, 2_000, 8_000] {
            let (part, made) = made_styles(n);
            let mut work = WriteWork::default();
            assert!(check_styles(part.as_bytes(), &made, &[], PartKind::Content, &limits, &mut work).is_ok(), "{n}");
            steps.push(work.scope_steps);
            // One step for each style made, and the few the root's declarations take.
            assert!((n as u64..n as u64 + 16).contains(&work.scope_steps), "{n} styles: {} steps", work.scope_steps);
        }
        assert!(steps[1] <= steps[0] * 4 + 16 && steps[2] <= steps[1] * 4 + 16, "{steps:?}");
        // The look is held to the cap of steps: a cap under the count refuses, and nothing is left unchecked.
        let (part, made) = made_styles(100);
        let mut work = WriteWork { step_cap: 50, ..WriteWork::default() };
        assert!(
            limit(check_styles(part.as_bytes(), &made, &[], PartKind::Content, &limits, &mut work))
                .is_some_and(|(what, max)| what.contains("steps") && max == 50)
        );
        // Styles that were written with no attribute are one look each and nothing else: the look is a step each against the cap, and it stops where the cap is
        // passed.
        let (part, mut made) = made_styles(100);
        for style in &mut made {
            style.attrs.clear();
        }
        let mut work = WriteWork { step_cap: 50, ..WriteWork::default() };
        assert!(
            limit(check_styles(part.as_bytes(), &made, &[], PartKind::Content, &limits, &mut work))
                .is_some_and(|(what, max)| what.contains("steps") && max == 50)
        );
        assert!(work.scope_steps <= 60, "{} steps taken for a cap of 50", work.scope_steps);
        // A style that is missing, one that is there twice and one of another family are each refused.
        let (part, mut made) = made_styles(3);
        made.push(styles::Written { name: "gr9".to_owned(), family: Fam::Graphic, attrs: Vec::new(), prop: 0 });
        assert!(why(check_styles(part.as_bytes(), &made, &[], PartKind::Content, &limits, &mut WriteWork::default())).contains("not there once"));
        let twice = part.replace("gr3", "gr2");
        let (_, made) = made_styles(3);
        assert!(why(check_styles(twice.as_bytes(), &made, &[], PartKind::Content, &limits, &mut WriteWork::default())).contains("not there once"));
        let mut other = made_styles(3).1;
        other[0].family = Fam::Paragraph;
        assert!(why(check_styles(part.as_bytes(), &other, &[], PartKind::Content, &limits, &mut WriteWork::default())).contains("another family"));
    }

    /// A part that declares `n` font faces `F1` to `Fn` of the families `Family 1` to `Family n`, and the faces a save declared in it.
    fn declared_faces(n: usize) -> (String, Vec<(String, String)>) {
        let decls: String = (1..=n).map(|k| format!(r#"<style:font-face style:name="F{k}" svg:font-family="'Family {k}'"/>"#)).collect();
        let part = format!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><office:font-face-decls>{decls}</office:font-face-decls><office:automatic-styles/><office:body><office:drawing><draw:page/></office:drawing></office:body></office:document-content>"#
        );
        (part, (1..=n).map(|k| (format!("F{k}"), format!("Family {k}"))).collect())
    }

    #[test]
    fn the_check_of_the_faces_made_looks_for_each_once_for_its_family_and_is_held_to_the_step_cap() {
        let limits = Limits::default();
        let check = |part: &str, faces: &[(String, String)], work: &mut WriteWork| {
            check_styles(part.as_bytes(), &[], faces, PartKind::Content, &limits, work)
        };
        let (part, faces) = declared_faces(50);
        let mut work = WriteWork::default();
        assert!(check(&part, &faces, &mut work).is_ok());
        // A step for each face looked for, and the few the root's declarations take.
        assert!((50..66).contains(&work.scope_steps), "{} steps for 50 faces", work.scope_steps);
        // A face declared for another family, one that is not there, and one that is there twice are each refused.
        let mut other = faces.clone();
        other[3].1 = "Family 9".to_owned();
        assert!(why(check(&part, &other, &mut WriteWork::default())).contains("font face the save declared"));
        let mut missing = faces.clone();
        missing.push(("Nowhere".to_owned(), "Nowhere".to_owned()));
        assert!(why(check(&part, &missing, &mut WriteWork::default())).contains("font face the save declared"));
        let twice = part.replace(r#"style:name="F2""#, r#"style:name="F1""#);
        assert!(why(check(&twice, &faces, &mut WriteWork::default())).contains("font face the save declared"));
        // The look is a step each against the cap, and it stops where the cap is passed: it does not go on through the rest of the faces.
        let mut work = WriteWork { step_cap: 10, ..WriteWork::default() };
        assert!(limit(check(&part, &faces, &mut work)).is_some_and(|(what, max)| what.contains("steps") && max == 10));
        assert!(work.scope_steps <= 12, "{} steps taken for a cap of 10", work.scope_steps);
    }

    #[test]
    fn a_package_of_more_than_the_most_the_read_back_reads_is_not_read_again_and_one_of_that_many_is() {
        let limits = Limits::default();
        let mut out = out_of(Vec::new(), Vec::new());
        out.restructured = true;
        out.read_back = vec![ReadBack { page_at: 0, ordinal: 0, paragraphs: 1, written: Vec::new() }];
        let outs = [(PartKind::Content, out)];
        // One byte over the most: the checks of the scan of the part stand for it, and nothing is read.
        let mut work = WriteWork::default();
        assert!(check_read_back(&vec![0u8; MOST_READ_BACK + 1], &outs, &limits, &mut work).is_ok());
        assert_eq!(work.read_back_bytes, 0);
        // The most itself is read (here: bytes of no package, which the read refuses), and charged.
        let mut work = WriteWork::default();
        assert!(why(check_read_back(&vec![0u8; MOST_READ_BACK], &outs, &limits, &mut work)).contains("does not read again"));
        assert_eq!(work.read_back_bytes, MOST_READ_BACK as u64);
    }

    /// The check of tier 2 reads the file made again when it is of the most the read back reads, and leaves one byte more unchecked.
    #[test]
    fn a_package_of_more_than_the_most_the_read_back_reads_is_not_checked_for_tier_2_and_one_of_that_many_is() {
        let limits = Limits::default();
        let mut out = out_of(Vec::new(), Vec::new());
        out.tier2 = vec![(0, 0, Arc::new(crate::ext::payload::Tier2::default()))];
        let outs = [(PartKind::Content, out)];
        let mut work = WriteWork::default();
        assert!(check_tier2(&vec![0u8; MOST_READ_BACK + 1], &outs, &limits, &mut work).is_ok());
        assert_eq!(work.read_back_bytes, 0);
        let mut work = WriteWork::default();
        assert!(why(check_tier2(&vec![0u8; MOST_READ_BACK], &outs, &limits, &mut work)).contains("does not read again"));
        assert_eq!(work.read_back_bytes, MOST_READ_BACK as u64);
    }

    /// The check of tier 2 refuses a shape whose `ID` is written twice: the second shape that holds an `ID` is read as a copy of the
    /// first, with none of its own data, whatever the edit wrote. The check of the `ID` by itself: what is carried holds nothing but the `ID` (no cell, so that the
    /// comparison of the cells cannot be what refuses it).
    #[test]
    fn an_id_written_twice_is_refused_by_the_check_of_tier_2_and_the_same_file_with_the_ids_told_apart_is_not() {
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
            r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
            r#"xmlns:diacraft="urn:app.diacraft:extension""#
        );
        let package = |second_id: u32| {
            let style = |name: &str, id: u32| {
                format!(
                    r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties diacraft:version="1" diacraft:id="{id}"/></style:style>"#
                )
            };
            let shape = |name: &str, style: &str| {
                format!(
                    r#"<draw:custom-shape draw:name="{name}" draw:style-name="{style}" svg:width="1cm" svg:height="1cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 10 10" draw:enhanced-path="M 0 0 L 10 10 N"/></draw:custom-shape>"#
                )
            };
            let content = format!(
                r#"<office:document-content {root}><office:automatic-styles>{}{}</office:automatic-styles><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="M0">{}{}</draw:page></office:drawing></office:body></office:document-content>"#,
                style("gr1", 5),
                style("gr2", second_id),
                shape("a", "gr1"),
                shape("b", "gr2")
            );
            let styles = format!(
                r#"<office:document-styles {root}><office:automatic-styles><style:page-layout style:name="L0"><style:page-layout-properties fo:page-width="1cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="M0" style:page-layout-name="L0"/></office:master-styles></office:document-styles>"#
            );
            package_of(&content, &styles)
        };
        let limits = Limits::default();
        // The second shape is element 2; its tier 2 holds the `ID` 7 and nothing else.
        let outs = || {
            let mut out = out_of(Vec::new(), Vec::new());
            out.tier2 = vec![(0, 2, Arc::new(crate::ext::payload::Tier2 { id: Some(7), ..crate::ext::payload::Tier2::default() }))];
            [(PartKind::Content, out)]
        };
        // `ID` 7 on the second style: it is read with it, and the check passes.
        assert!(check_tier2(&package(7), &outs(), &limits, &mut WriteWork::default()).is_ok());
        // `ID` 5 on both: the second is a copy, numbered by the reader, and not read with the `ID` that was written.
        assert!(why(check_tier2(&package(5), &outs(), &limits, &mut WriteWork::default())).contains("another ID"));
    }

    /// The read-back through the reader, each check by itself (parked 2): the number of paragraphs the shape reads as is the
    /// model's, and each paragraph the save wrote reads as the text the model has. Deleting either check leaves the rest of the suite passing, since the scan
    /// of the part made refuses the same faults first.
    #[test]
    fn the_read_back_through_the_reader_refuses_a_count_of_paragraphs_and_a_text_that_are_not_the_models() {
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
            r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#
        );
        let content = format!(
            r#"<office:document-content {root}><office:body><office:drawing><draw:page draw:name="p" draw:master-page-name="M0"><draw:custom-shape svg:width="1cm" svg:height="1cm"><text:p>one</text:p><text:p>two</text:p><draw:enhanced-geometry svg:viewBox="0 0 10 10" draw:enhanced-path="M 0 0 L 10 10 N"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"#
        );
        let styles = format!(
            r#"<office:document-styles {root}><office:automatic-styles><style:page-layout style:name="L0"><style:page-layout-properties fo:page-width="1cm"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="M0" style:page-layout-name="L0"/></office:master-styles></office:document-styles>"#
        );
        let bytes = package_of(&content, &styles);
        let limits = Limits::default();
        let outs = |paragraphs: usize, written: Vec<(usize, String)>| {
            let mut out = out_of(Vec::new(), Vec::new());
            out.restructured = true;
            out.read_back = vec![ReadBack { page_at: 0, ordinal: 1, paragraphs, written }];
            [(PartKind::Content, out)]
        };
        let check = |paragraphs: usize, written: Vec<(usize, String)>| {
            check_read_back(&bytes, &outs(paragraphs, written), &limits, &mut WriteWork::default())
        };
        // The paragraphs the model has are the two the file reads as, and the ones written read as they are.
        assert!(check(2, vec![(0, "one".to_owned()), (1, "two".to_owned())]).is_ok());
        assert!(check(2, Vec::new()).is_ok());
        // Another number of paragraphs, either way.
        assert!(why(check(3, Vec::new())).contains("another number of paragraphs"));
        assert!(why(check(1, Vec::new())).contains("another number of paragraphs"));
        // Another text, or a paragraph the shape does not have.
        assert!(why(check(2, vec![(1, "twp".to_owned())])).contains("another text"));
        assert!(why(check(2, vec![(0, "two".to_owned())])).contains("another text"));
        assert!(why(check(2, vec![(5, String::new())])).contains("another text"));
    }

    #[test]
    fn the_read_of_the_file_again_is_held_to_what_the_models_leave_of_the_memory_a_save_may_hold() {
        // A cost of 1,000 items of 64 bytes (54 before) and a tree and parts of 10,000 bytes: the model weighs 64,000, a read holds 10,000 beside it,
        // and a save needs the model twice and that: 138,000.
        let cost = ReadCost { items: 1_000, media_bytes: 0, tree_nodes: 40, inflated_bytes: 1_600, ..ReadCost::default() };
        assert_eq!((cost.model_bytes(), cost.transient_bytes(), cost.save_bytes()), (64_000, 10_000, 138_000));
        let memory = |max_write_memory: u64| Limits { max_write_memory, ..Limits::default() };
        // At the estimate the save is let be, one byte under it refused, with the cap in the words; and nothing is lowered for a cost of nothing.
        let none = Beside::default();
        assert!(check_memory(&cost, &none, &memory(138_000)).is_ok());
        assert_eq!(
            limit(check_memory(&cost, &none, &memory(137_999))),
            Some(("bytes of memory a save holds (the model and the file read again beside it)", 137_999))
        );
        assert_eq!(
            limit(save_limits(&memory(137_999), &cost, &none).map(|_| ())),
            Some(("bytes of memory a save holds (the model and the file read again beside it)", 137_999))
        );
        assert!(check_memory(&ReadCost::default(), &none, &memory(0)).is_ok());
        // The items the read may take are what is left of the memory once the model and the read's own parts are taken from it, in items: here
        // (200,000 - 64,000 - 10,000) / 64, and never more than the caller's own cap.
        let lowered = save_limits(&memory(200_000), &cost, &none).map(|l| l.max_items);
        assert_eq!(lowered.as_ref().ok(), Some(&((200_000 - 64_000 - 10_000) / 64)));
        let capped = Limits { max_write_memory: 200_000, max_items: 100, ..Limits::default() };
        assert_eq!(save_limits(&capped, &cost, &none).map(|l| l.max_items).ok(), Some(100), "a cap the caller lowered is kept");
        // Everything else the caller set is kept.
        let others = Limits { max_write_memory: 200_000, max_shapes: 77, max_write_steps: 5, ..Limits::default() };
        assert_eq!(save_limits(&others, &cost, &none).map(|l| (l.max_shapes, l.max_write_steps, l.max_write_memory)).ok(), Some((77, 5, 200_000)));
    }

    #[test]
    fn a_refusal_of_the_items_a_save_lowered_is_the_saves_memory_and_any_other_is_the_readers() {
        let (kept, lowered) = (Limits::default(), Limits { max_items: 10, ..Limits::default() });
        let items = || OdgError::Limit { kind: LimitKind::Items, max: 10 };
        assert!(matches!(baseline_error(items(), &lowered, &kept), WriteError::Limit { .. }), "the items cap was lowered by the save");
        assert!(matches!(baseline_error(items(), &kept, &kept), WriteError::Original(_)), "the caller's own cap, not lowered");
        assert!(matches!(baseline_error(OdgError::Limit { kind: LimitKind::Cells, max: 1 }, &lowered, &kept), WriteError::Original(_)));
        assert!(matches!(baseline_error(OdgError::EncryptedPackage, &lowered, &kept), WriteError::Original(_)));
    }

    #[test]
    fn the_words_a_harness_compares_are_each_their_own() {
        let notes = [
            (SaveNote::SignatureStale, "signature_stale"),
            (SaveNote::ShearDropped { page: 1, shape: 2 }, "shear_dropped"),
            (SaveNote::RouteNotWritten { page: 1, shape: 2 }, "route_not_written"),
            (SaveNote::MetaNotUpdated, "meta_not_updated"),
            (SaveNote::NewShapeUnwritten { page: 1, shape: 4, attributes: 2, elements: 1, data: true, foreign: true }, "new_shape_unwritten"),
            (SaveNote::FieldWritten { page: 1, shape: 3, element: "author-name" }, "field_written"),
            (SaveNote::RouteDroppedForVersion { page: 3, shape: 7 }, "route_dropped_for_version"),
        ];
        for (note, word) in &notes {
            assert_eq!(note.class(), *word);
            assert!(note.to_string().len() > 20, "{word}: {note}");
        }
        assert!(notes[1].0.to_string().contains("shape 2 on page 1"), "the numbers are in the words");
        assert!(notes[2].0.to_string().contains("connector 2 on page 1"));
        let dropped = notes[6].0.to_string();
        for said in ["connector 7 on page 3", "ODF 1.0 or 1.1", "no version", "ends", "path and skew"] {
            assert!(dropped.contains(said), "{said}: {dropped}");
        }
        let field = notes[5].0.to_string();
        for said in ["text:author-name", "shape 3 on page 1", "plain text"] {
            assert!(field.contains(said), "{said}: {field}");
        }
        let unwritten = notes[4].0.to_string();
        for said in ["shape 4 on page 1", "2 attributes", "1 element", "shape data", "foreign"] {
            assert!(unwritten.contains(said), "{said}: {unwritten}");
        }
        let one = SaveNote::NewShapeUnwritten { page: 0, shape: 9, attributes: 1, elements: 0, data: false, foreign: false }.to_string();
        assert!(one.contains("1 attribute") && !one.contains("attributes") && !one.contains("element") && !one.contains("data"), "{one}");
        // What a shape does not hold is not in the words, each kind by itself.
        let elements = SaveNote::NewShapeUnwritten { page: 0, shape: 9, attributes: 0, elements: 2, data: false, foreign: false }.to_string();
        assert!(elements.contains("2 elements") && !elements.contains("attribute") && !elements.contains("data"), "{elements}");
        let data = SaveNote::NewShapeUnwritten { page: 0, shape: 9, attributes: 0, elements: 0, data: true, foreign: false }.to_string();
        assert!(data.contains("shape data") && !data.contains("foreign") && !data.contains("attribute") && !data.contains("element"), "{data}");
        let foreign = SaveNote::NewShapeUnwritten { page: 0, shape: 9, attributes: 0, elements: 0, data: false, foreign: true }.to_string();
        assert!(foreign.contains("foreign data") && !foreign.contains("shape data") && !foreign.contains("attribute"), "{foreign}");
        let errors = [
            (WriteError::Unsupported("a thing"), "write_unsupported", "a thing"),
            (WriteError::Mismatch("a mismatch"), "write_mismatch", "a mismatch"),
            (WriteError::Limit { what: "bytes", max: 7 }, "write_limit", "more than 7 bytes"),
            (WriteError::Thumbnail("a picture"), "write_thumbnail", "a picture"),
            (WriteError::SelfCheck("why".to_owned()), "write_self_check", "why"),
            (WriteError::Package(ContainerError::TooManyParts { limit: 3 }), "too_many_parts", "the file cannot be made"),
        ];
        for (error, word, said) in &errors {
            assert_eq!(error.class(), *word);
            assert!(error.to_string().contains(said), "{word}: {error}");
        }
        let written = Written {
            bytes: vec![0; 5],
            index: ShapeIndex::default(),
            notes: vec![SaveNote::SignatureStale],
            edited: true,
            work: WriteWork::default(),
        };
        let shown = format!("{written:?}");
        assert!(
            shown.contains("Written") && shown.contains("bytes: 5") && shown.contains("SignatureStale") && shown.contains("edited: true"),
            "{shown}"
        );
    }

    /// A package of one page that holds `body`, made by the container's writer.
    fn package(body: &str) -> Vec<u8> {
        package_with(body, true)
    }

    /// As [`package`], with `styles.xml` left out when `styles_part` is false.
    fn package_with(body: &str, styles_part: bool) -> Vec<u8> {
        use diacraft_container::zip::ArchiveWriter;
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
            r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3""#
        );
        let content = format!(
            "<?xml version=\"1.0\"?><office:document-content {root}><office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">{body}</draw:page></office:drawing></office:body></office:document-content>"
        );
        let styles = format!(
            "<?xml version=\"1.0\"?><office:document-styles {root}><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
        );
        let manifest = concat!(
            r#"<?xml version="1.0"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
            r#"<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>"#,
            r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
            r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/></manifest:manifest>"#
        );
        let mut w = ArchiveWriter::new();
        let parts =
            [("mimetype", "application/vnd.oasis.opendocument.graphics", Method::Stored), ("content.xml", content.as_str(), Method::Deflated)];
        for (name, data, method) in parts {
            assert!(w.add(name, data.as_bytes(), method).is_ok());
        }
        if styles_part {
            assert!(w.add("styles.xml", styles.as_bytes(), Method::Deflated).is_ok());
        }
        assert!(w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).is_ok());
        w.finish().unwrap_or_default()
    }

    #[test]
    fn the_bytes_just_made_are_checked_against_the_index_one_difference_at_a_time() {
        let limits = Limits::default();
        let bytes = package(r#"<draw:rect svg:width="1cm" svg:height="1cm"/><draw:rect svg:width="2cm" svg:height="1cm"/><ex:n xmlns:ex="urn:x"/>"#);
        let Ok((_, report)) = read(&bytes, &limits) else { panic!("the package did not read") };
        let Some(page) = report.index.page(0) else { panic!("no page") };
        assert!(
            self_check(&bytes, false, &report.index, &[], &limits, &mut WriteWork::default()).is_ok(),
            "the index of the file's own read agrees with it"
        );
        let ordinals = page.ordinal_map().clone();
        assert_eq!(ordinals.len(), 2);
        let mut fewer = ordinals.clone();
        fewer.retain(|id, _| *id == 1);
        let mut more_kept = page.kept().to_vec();
        more_kept.push(99);
        let pages = |p: PageShapes| ShapeIndex::from_pages(vec![p], ReadCost::default());
        let variants = [
            ("one shape fewer modelled", pages(PageShapes::from_parts(page.source, fewer, page.kept().to_vec(), page.elements(), page.high_water()))),
            (
                "another number of elements",
                pages(PageShapes::from_parts(page.source, ordinals.clone(), page.kept().to_vec(), page.elements() + 1, page.high_water())),
            ),
            ("another number of kept elements", pages(PageShapes::from_parts(page.source, ordinals, more_kept, page.elements(), page.high_water()))),
            ("another number of pages", ShapeIndex::default()),
        ];
        for (what, index) in variants {
            assert!(matches!(self_check(&bytes, false, &index, &[], &limits, &mut WriteWork::default()), Err(WriteError::SelfCheck(_))), "{what}");
        }
        // Bytes that do not read are refused, with the reader's class in the words.
        let Err(WriteError::SelfCheck(why)) = self_check(b"not a package", false, &report.index, &[], &limits, &mut WriteWork::default()) else {
            panic!("bytes that do not read were accepted")
        };
        assert!(why.contains("not_zip"), "{why}");
    }

    /// A `content.xml` of one page holding `body`, with `s2` and `w3` bound to the SVG namespaces (ODF's own and the W3C's, which the reader reads
    /// as one).
    fn content_part(body: &str) -> String {
        format!(
            concat!(
                r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
                r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
                r#"xmlns:s2="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:w3="http://www.w3.org/2000/svg" xmlns:w4="http://www.w3.org/2000/svg">"#,
                r#"<office:body><office:drawing><draw:page>{}</draw:page></office:drawing></office:body></office:document-content>"#
            ),
            body
        )
    }

    /// The check of the tags written on, for attributes `(element ordinal, namespace, name)` of the first page of a part holding `body`.
    fn checked(body: &str, written: &[(u32, Ns, &'static str)]) -> Result<(), WriteError> {
        let part = content_part(body);
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let Ok(mut scan) = scan_part(part.as_bytes(), PartKind::Content, &limits, &mut work) else { panic!("the part did not scan") };
        let tags: Vec<EditedTag> =
            written.iter().map(|(ordinal, ns, local)| EditedTag { kind: PartKind::Content, page: 0, ordinal: *ordinal, ns: *ns, local }).collect();
        check_edited_tags(part.as_bytes(), &mut scan, &tags, &limits, &mut work)
    }

    /// The words of a refusal of the check.
    fn why(r: Result<(), WriteError>) -> String {
        match r {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("expected a refusal of the check, got {other:?}"),
        }
    }

    #[test]
    fn the_tags_a_save_wrote_on_are_read_back_for_repeated_names_and_for_values_the_reader_refuses() {
        let (x, y) = ((1, Ns::Svg, "x"), (1, Ns::Svg, "y"));
        // What is well made: every attribute written is there, once, and read; a reference in the value is resolved first.
        assert!(checked(r#"<draw:rect svg:x="1cm" svg:y="2cm"/>"#, &[x, y]).is_ok());
        assert!(checked(r#"<draw:rect svg:x="1&#99;m"/>"#, &[x]).is_ok(), "the value is read as the reader reads it, references resolved");
        // An attribute in no namespace is another from one in the SVG's, and a declaration is no attribute.
        assert!(checked(r#"<draw:rect svg:x="1cm" x="oops" xmlns:q="urn:q"/>"#, &[x]).is_ok());
        assert!(checked(r#"<draw:rect svg:x="1cm" x="oops" xmlns:x="urn:q" xmlns="urn:d"/>"#, &[x]).is_ok(), "`xmlns:x` is not the attribute `x`");
        // Names of the file's own that read alike are the file's: two prefixes bound to nothing both read as no namespace (the fuzz target found a
        // file of them, and a check that looked for every name twice refused its save), and a name the save did not write is not looked for twice.
        assert!(checked(r#"<draw:rect qvg:x="1cm" ssvg:x="1cm" svg:y="1cm"/>"#, &[y]).is_ok(), "two prefixes bound to nothing");
        assert!(checked(r#"<draw:rect svg:x="1cm" s2:x="2cm" svg:y="1cm"/>"#, &[y]).is_ok(), "a repeat the save did not write");
        // A name twice: by the same name, and by two prefixes of one namespace URI.
        for (what, tag) in [
            ("the same name", r#"<draw:rect svg:x="1cm" svg:x="2cm"/>"#),
            ("two prefixes of one namespace", r#"<draw:rect svg:x="1cm" s2:x="2cm"/>"#),
            ("two prefixes of the W3C's namespace", r#"<draw:rect w3:x="1cm" w4:x="2cm" svg:y="1cm"/>"#),
        ] {
            assert!(why(checked(tag, &[x])).contains("twice"), "{what}");
        }
        // `svg:x` and an `x` in the W3C's SVG namespace are two names in two URIs, though the reader folds them into one: no repeat. Both
        // are read back, so one the reader refuses is found.
        assert!(checked(r#"<draw:rect svg:x="1cm" w3:x="2cm"/>"#, &[x]).is_ok(), "ODF's own and the W3C's");
        assert!(checked(r#"<draw:rect w3:x="2cm" svg:x="1cm"/>"#, &[x]).is_ok(), "the other way round");
        assert!(why(checked(r#"<draw:rect svg:x="1cm" w3:x="wide"/>"#, &[x])).contains("does not read what the save wrote as x"));
        // A value the reader refuses: a length without its unit, a transform that is not closed, an integer that is not one.
        assert!(why(checked(r#"<draw:rect svg:x="wide"/>"#, &[x])).contains("does not read what the save wrote as x"));
        assert!(why(checked(r#"<draw:rect draw:transform="rotate(0.5"/>"#, &[(1, Ns::Draw, "transform")])).contains("transform"));
        assert!(why(checked(r#"<draw:rect draw:z-index="1.5"/>"#, &[(1, Ns::Draw, "z-index")])).contains("z-index"));
        // One bad attribute among several written on a tag is found, and so is one that is not there.
        assert!(why(checked(r#"<draw:rect svg:x="1cm" svg:y="soon"/>"#, &[x, y])).contains("as y"));
        assert!(why(checked(r#"<draw:rect svg:x="1cm"/>"#, &[x, y])).contains("not on its tag"));
        // Only the elements written on are read back, and each by its own ordinal: the second is bad and the first is the one named.
        let two = r#"<draw:rect svg:x="1cm"/><draw:rect svg:x="bad"/>"#;
        assert!(checked(two, &[x]).is_ok());
        assert!(why(checked(two, &[x, (2, Ns::Svg, "x")])).contains("as x"));
        assert!(why(checked(two, &[(3, Ns::Svg, "x")])).contains("not in the part"));
        assert!(why(checked(two, &[(0, Ns::Svg, "x")])).contains("not in the part"));
    }

    #[test]
    fn the_check_of_the_bytes_made_reads_the_tags_by_the_part_they_are_in() {
        let limits = Limits::default();
        let bytes = package(r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#);
        let Ok((_, report)) = read(&bytes, &limits) else { panic!("the package did not read") };
        let tag = |kind, ordinal, local| EditedTag { kind, page: 0, ordinal, ns: Ns::Svg, local };
        let run = |tags: &[EditedTag]| self_check(&bytes, false, &report.index, tags, &limits, &mut WriteWork::default());
        assert!(run(&[tag(PartKind::Content, 1, "width"), tag(PartKind::Content, 1, "height")]).is_ok());
        assert!(why(run(&[tag(PartKind::Content, 1, "x")])).contains("not on its tag"), "an attribute the element does not have");
        // A tag in `styles.xml` is looked for there: its master page holds no shape element.
        assert!(why(run(&[tag(PartKind::Styles, 1, "width")])).contains("not in the part"));
    }

    #[test]
    fn an_edit_that_writes_a_value_the_reader_refuses_or_a_name_twice_is_not_kept() {
        // The injected faults of `attrs::fault`: the same save is made without one, and with each it is refused by the check of the bytes made, in
        // words that say which (and not by the scan of tags, which sees neither).
        use diacraft_doc::Value;
        let limits = Limits::default();
        let bytes = package(
            r#"<draw:rect draw:name="a" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/><draw:rect svg:x="3cm" svg:y="3cm" svg:width="1cm" svg:height="1cm"/>"#,
        );
        let Ok((mut doc, report)) = read(&bytes, &limits) else { panic!("the package did not read") };
        {
            let Some(page) = doc.pages.get_mut(0) else { panic!("no page") };
            let Some(shape) = Arc::make_mut(page).shapes.get_mut(0) else { panic!("no shape") };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            let Some(cell) = sheet.cells.get_mut("PinX") else { panic!("no PinX") };
            let before = cell.value.as_ref().and_then(Value::number).unwrap_or(0.0);
            cell.value = Value::from_number(before + 0.5);
        }
        let save = |fault: attrs::fault::Fault| {
            attrs::fault::INJECTED.set(fault);
            let saved = write_with(&doc, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &WriteOptions::default());
            attrs::fault::INJECTED.set(attrs::fault::Fault::Off);
            saved
        };
        assert!(save(attrs::fault::Fault::Off).is_ok_and(|w| w.edited), "the move is saved when nothing is wrong");
        let unreadable = save(attrs::fault::Fault::Unreadable);
        assert!(why(unreadable.map(|_| ())).contains("the reader does not read what the save wrote as"), "a value the reader refuses");
        let repeated = save(attrs::fault::Fault::Repeated);
        assert!(why(repeated.map(|_| ())).contains("twice"), "an attribute written twice");
    }

    #[test]
    fn an_attribute_written_is_placed_by_the_element_it_is_on_and_the_ordinal_that_element_has_after_the_save() {
        let shape = |at: usize| scan::ScanShape {
            kids: (0, 0),
            kind: scan::Kind::Rect,
            open: (at, at + 5),
            close_start: at + 5,
            end: at + 5,
            lead: at,
            parent: 0,
            scope: 0,
        };
        let pages = [
            scan::ScanPage { close_start: 90, end: 100, shapes: vec![shape(10), shape(30), shape(50)], ..Default::default() },
            scan::ScanPage { close_start: 190, end: 200, shapes: vec![shape(110), shape(130)], ..Default::default() },
        ];
        // Page 0 reorders its elements (1, 2, 3 become 2, 3, 1); on page 1 the first is gone and the second is the first's place.
        let new_ords: HashMap<usize, Vec<u32>> = HashMap::from([(0, vec![0, 2, 3, 1]), (1, vec![0, 0, 1])]);
        let at = |tag_start, ns, local| splice::WrittenAttr { tag_start, ns, local };
        let written = [at(10, Ns::Svg, "x"), at(30, Ns::Draw, "transform"), at(50, Ns::Svg, "y"), at(110, Ns::Svg, "x"), at(130, Ns::Svg, "width")];
        let Ok(placed) = place_written(&written, &pages, &new_ords, PartKind::Content) else { panic!("the attributes were not placed") };
        let wanted = |page, ordinal, ns, local| EditedTag { kind: PartKind::Content, page, ordinal, ns, local };
        assert_eq!(
            placed,
            [wanted(0, 2, Ns::Svg, "x"), wanted(0, 3, Ns::Draw, "transform"), wanted(0, 1, Ns::Svg, "y"), wanted(1, 1, Ns::Svg, "width")],
            "an element that is gone is not placed"
        );
        // A tag that no element of the page starts at is a fault, not a thing to skip.
        assert!(matches!(place_written(&[at(12, Ns::Svg, "x")], &pages, &new_ords, PartKind::Content), Err(WriteError::Mismatch(_))));
        assert!(matches!(place_written(&[at(500, Ns::Svg, "x")], &pages, &new_ords, PartKind::Content), Err(WriteError::Mismatch(_))));
    }

    #[test]
    fn what_a_save_holds_is_the_read_and_the_edit_together_and_an_index_no_read_made_is_refused() {
        // The read: 2 x the model (6,400) and what it holds beside it (1,000), the file (100), the part's text (50) and 2 shapes' walks; the edit on
        // top: the part twice and the package made twice.
        let cost = ReadCost { items: 100, media_bytes: 0, tree_nodes: 0, inflated_bytes: 1_000, ..ReadCost::default() };
        assert_eq!((cost.model_bytes(), cost.save_bytes()), (6_400, 13_800));
        let beside = Beside { file: 100, part: 50, shapes: 2, grown: 0 };
        assert_eq!(beside.holds(&cost), 13_800 + 100 + 50 + 2 * WALK_BYTES + 2 * 50 + 2 * 100);
        assert_eq!(Beside::default().holds(&ReadCost::default()), 0);
        // The gate is at the figure and not beside it.
        let memory = |max_write_memory: u64| Limits { max_write_memory, ..Limits::default() };
        let held = beside.holds(&cost);
        assert!(check_memory(&cost, &beside, &memory(held)).is_ok());
        assert!(check_memory(&cost, &beside, &memory(held - 1)).is_err());
        // An index the reader made, and the ways it can be wrong.
        let limits = Limits::default();
        let bytes = package(r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#);
        let Ok((mut doc, report)) = read(&bytes, &limits) else { panic!("the package did not read") };
        assert!(check_index(&doc, &report.index, &limits).is_ok());
        assert!(matches!(check_index(&doc, &ShapeIndex::default(), &limits), Err(WriteError::Mismatch(_))), "no cost: no read made it");
        let mut other = doc.clone();
        other.pages = diacraft_doc::SharedVec::new();
        // Other pages than the index's are pages added, taken away or moved, which a save writes (`structure`): the gate does not refuse them, and does refuse more than a drawing
        // may hold, before the file is read.
        assert!(check_index(&other, &report.index, &limits).is_ok(), "other pages than the index's");
        assert!(
            matches!(
                check_index(&doc, &report.index, &Limits { max_pages: 0, ..Limits::default() }),
                Err(WriteError::Limit { what: "pages in the drawing", max: 0 })
            ),
            "more pages than a drawing may hold"
        );
        if let Some(page) = doc.pages.get_mut(0) {
            Arc::make_mut(page).shapes.push(Arc::new(diacraft_doc::Shape::new(99)));
        }
        assert!(
            check_index(&doc, &report.index, &limits).is_ok(),
            "a shape the index does not hold is a new shape, which a save writes: its growth is charged by the walk of the model"
        );
    }

    #[test]
    fn the_tags_written_on_are_checked_in_the_order_of_their_part_page_and_element() {
        let tag = |kind, page, ordinal| EditedTag { kind, page, ordinal, ns: Ns::Svg, local: "x" };
        let mut tags = [
            tag(PartKind::Styles, 0, 1),
            tag(PartKind::Content, 1, 2),
            tag(PartKind::Content, 0, 3),
            tag(PartKind::Content, 1, 1),
            tag(PartKind::Styles, 0, 1),
        ];
        tags.sort_unstable_by_key(EditedTag::key);
        let order: Vec<(bool, usize, u32)> = tags.iter().map(|t| (t.kind == PartKind::Styles, t.page, t.ordinal)).collect();
        assert_eq!(
            order,
            [(false, 0, 3), (false, 1, 1), (false, 1, 2), (true, 0, 1), (true, 0, 1)],
            "content.xml's first, then by page, then by element"
        );
        // The check splits them where `content.xml`'s end, which is where the sort put it.
        assert_eq!(tags.partition_point(|t| t.kind == PartKind::Content), 3);
    }

    #[test]
    fn what_a_save_holds_beside_the_models_is_taken_from_the_sizes_the_archive_states() {
        use diacraft_container::zip::ArchiveWriter;
        // A package whose biggest entry is a picture, which is not a part a save inflates, and whose biggest part is `styles.xml`.
        let root =
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0""#;
        let content = format!(
            "<office:document-content {root}><office:body><office:drawing><draw:page/></office:drawing></office:body></office:document-content>"
        );
        let styles =
            format!("<office:document-styles {root}><office:master-styles>{}</office:master-styles></office:document-styles>", " ".repeat(3_000));
        let picture = vec![7u8; 9_000];
        let mut w = ArchiveWriter::new();
        let manifest = r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;
        let parts: [(&str, &[u8], Method); 5] = [
            ("mimetype", b"application/vnd.oasis.opendocument.graphics", Method::Stored),
            ("content.xml", content.as_bytes(), Method::Deflated),
            ("styles.xml", styles.as_bytes(), Method::Deflated),
            ("Pictures/big.bin", &picture, Method::Stored),
            ("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated),
        ];
        for (name, data, method) in parts {
            assert!(w.add(name, data, method).is_ok());
        }
        let bytes = w.finish().unwrap_or_default();
        let Ok(opened) = crate::package::open(&bytes, &Limits::default(), &mut Work::default()) else { panic!("the package did not open") };
        let index = ShapeIndex::default();
        let beside = Beside::of(&bytes, &opened, &index, 7);
        assert_eq!(beside, Beside { file: bytes.len() as u64, part: styles.len() as u64, shapes: 0, grown: 7 });
    }

    #[test]
    fn what_the_caller_s_model_has_grown_by_is_charged_by_the_cells_shapes_and_text_it_gained() {
        use crate::figures::Figures;
        let (then, now) = (
            Figures { shapes: 10, cells: 100, nodes: 20, text_bytes: 1_000, media_bytes: 10, steps: 0 },
            Figures { shapes: 12, cells: 130, nodes: 25, text_bytes: 5_000, media_bytes: 100, steps: 0 },
        );
        let shape = crate::read::cost::SHAPE as u64 * ITEM_BYTES;
        assert_eq!(growth(&now, &then), 30 * CELL_ITEMS * ITEM_BYTES + 2 * shape + 5 * NODE_ITEMS * ITEM_BYTES + 4_000 + 90);
        assert_eq!(growth(&then, &then), 0);
        assert_eq!(growth(&then, &now), 0, "a model that shrank grew by nothing");
        // The growth is charged once, on top of what the read and the edit hold, and is taken from the room the read may use.
        let cost = ReadCost { items: 100, tree_nodes: 0, inflated_bytes: 1_000, ..ReadCost::default() };
        let (plain, grown) = (Beside { file: 100, part: 50, shapes: 2, grown: 0 }, Beside { file: 100, part: 50, shapes: 2, grown: 9_999 });
        assert_eq!(grown.holds(&cost), plain.holds(&cost) + 9_999);
        let memory = Limits { max_write_memory: plain.holds(&cost), ..Limits::default() };
        assert!(check_memory(&cost, &plain, &memory).is_ok());
        assert!(check_memory(&cost, &grown, &memory).is_err(), "the growth alone puts a save past its memory");
        let room = |b: &Beside| save_limits(&Limits { max_write_memory: 1_000_000, ..Limits::default() }, &cost, b).map(|l| l.max_items).ok();
        // Each room is the bytes left over the bytes of an item, rounded down, so the two differ by the growth in items rounded down or up.
        let fewer = room(&plain).zip(room(&grown)).map(|(a, b)| a - b);
        assert!(
            fewer.is_some_and(|n| n == 9_999 / ITEM_BYTES as usize || n == 9_999u64.div_ceil(ITEM_BYTES) as usize),
            "the read's items are what is left: {fewer:?}"
        );
    }

    #[test]
    fn the_read_back_of_the_tags_is_held_to_the_caps_of_the_save_it_checks() {
        let part = content_part(r#"<draw:rect svg:x="1cm" svg:y="2cm"/>"#);
        let limits = Limits::default();
        let tags: Vec<EditedTag> = [("x", 1), ("y", 1)]
            .iter()
            .map(|(local, ordinal)| EditedTag { kind: PartKind::Content, page: 0, ordinal: *ordinal, ns: Ns::Svg, local })
            .collect();
        let run = |mut work: WriteWork| {
            let Ok(mut scan) = scan_part(part.as_bytes(), PartKind::Content, &limits, &mut WriteWork::default()) else {
                panic!("the part did not scan")
            };
            let result = check_edited_tags(part.as_bytes(), &mut scan, &tags, &limits, &mut work);
            (result, work)
        };
        let (result, work) = run(WriteWork::default());
        assert!(result.is_ok(), "{result:?}");
        // What it looked at is counted: the bytes of the one tag, and the attributes it holds (the two written and nothing else).
        assert_eq!((work.checked_tag_bytes, work.checked_attributes), (r#"<draw:rect svg:x="1cm" svg:y="2cm"/>"#.len() as u64, 2));
        // The caps the save runs under are the check's: bytes of start tags looked at, and steps of looking prefixes up.
        let (refused, _) = run(WriteWork { byte_cap: 10, ..WriteWork::default() });
        assert!(limit(refused).is_some_and(|(what, max)| what.contains("start tags") && max == 10));
        let (refused, _) = run(WriteWork { step_cap: 1, ..WriteWork::default() });
        assert!(limit(refused).is_some_and(|(what, max)| what.contains("steps") && max == 1));
        // A declaration on the tag is no attribute and costs no step: one prefix looked up, and three declarations beside it, are one step.
        let declared = content_part(r#"<draw:rect svg:x="1cm" xmlns:a="urn:a" xmlns:b="urn:b" xmlns:c="urn:c"/>"#);
        let Ok(mut scan) = scan_part(declared.as_bytes(), PartKind::Content, &limits, &mut WriteWork::default()) else {
            panic!("the part did not scan")
        };
        let mut work = WriteWork { step_cap: 1, ..WriteWork::default() };
        let x = [EditedTag { kind: PartKind::Content, page: 0, ordinal: 1, ns: Ns::Svg, local: "x" }];
        assert!(check_edited_tags(declared.as_bytes(), &mut scan, &x, &limits, &mut work).is_ok(), "declarations cost no step");
    }

    #[test]
    fn an_attribute_written_in_a_part_the_package_does_not_hold_is_a_fault() {
        let limits = Limits::default();
        let bytes = package_with(r#"<draw:rect svg:width="1cm" svg:height="1cm"/>"#, false);
        let Ok((_, report)) = read(&bytes, &limits) else { panic!("the package did not read") };
        let run = |tags: &[EditedTag]| self_check(&bytes, false, &report.index, tags, &limits, &mut WriteWork::default());
        assert!(run(&[]).is_ok(), "nothing written there, nothing to find");
        let tag = EditedTag { kind: PartKind::Styles, page: 0, ordinal: 1, ns: Ns::Svg, local: "width" };
        assert!(why(run(&[tag])).contains("does not hold"));
    }

    #[test]
    fn a_file_the_manifest_omitted_gets_the_media_type_its_extension_says() {
        for (name, media) in [
            ("Pictures/a.png", "image/png"),
            ("Pictures/a.PNG", "image/png"),
            ("Pictures/a.jpg", "image/jpeg"),
            ("Pictures/a.jpeg", "image/jpeg"),
            ("Pictures/a.gif", "image/gif"),
            ("Pictures/a.svg", "image/svg+xml"),
            ("Object 1/content.xml", "text/xml"),
            ("Pictures/a.bmp", ""),
            ("Pictures/no_extension", ""),
            ("a.tar.png", "image/png"),
        ] {
            assert_eq!(media_for(name), media, "{name}");
        }
    }

    /// The decodes of the tier 2 that styles carry share one budget over the save, the items a read of the drawing has (the security review).
    #[test]
    fn the_decodes_of_a_save_share_one_budget_of_items_and_a_charge_past_it_is_refused_and_not_taken() {
        let limits = Limits { max_items: 100, ..Limits::default() };
        let mut work = WriteWork::default();
        assert_eq!(work.tier2_room(&limits), 100);
        assert!(work.charge_tier2(60, &limits).is_ok());
        assert_eq!(work.tier2_room(&limits), 40);
        assert_eq!(limit(work.charge_tier2(41, &limits)).map(|(_, max)| max), Some(100));
        assert_eq!(work.tier2_items, 60, "a charge that is refused is not taken");
        assert!(work.charge_tier2(40, &limits).is_ok(), "exactly what is left");
        assert_eq!(work.tier2_room(&limits), 0);
        assert!(work.charge_tier2(1, &limits).is_err());
    }

    /// The steps of a loop are charged before it runs, and the save is refused past the step cap.
    #[test]
    fn the_steps_of_a_loop_are_charged_before_it_runs_and_refused_past_the_cap() {
        let mut work = WriteWork { step_cap: 10, ..WriteWork::default() };
        assert!(work.charge_steps(7).is_ok());
        assert_eq!(work.steps_used(), 7);
        assert!(work.charge_steps(3).is_ok(), "exactly the cap");
        assert!(work.charge_steps(1).is_err(), "one past it");
    }

    #[test]
    fn the_step_cap_holds_at_exactly_the_cap() {
        let mut work = WriteWork { step_cap: 10, ..WriteWork::default() };
        work.shapes_compared = 4;
        work.cells_compared = 3;
        work.scope_steps = 3;
        assert_eq!(work.steps_used(), 10);
        assert!(work.check_steps().is_ok(), "ten steps of a cap of ten");
        assert!(!work.step(), "the eleventh step is past it");
        assert_eq!(limit(work.check_steps()), Some(("steps of looking namespace prefixes up and of comparing the drawing with the file", 10)));
        // `step` itself: taken while within the cap, and counted either way.
        let mut work = WriteWork { step_cap: 2, ..WriteWork::default() };
        assert!(work.step() && work.step() && !work.step());
        assert_eq!(work.scope_steps, 3);
    }

    #[test]
    fn each_charge_refuses_past_its_cap_and_not_at_it() {
        let limits = Limits { max_write_bytes: 10, max_write_attributes: 2, max_write_steps: 5, ..Limits::default() };
        // Scanned bytes: 10 of 10 pass and are counted, one more is refused and is not counted.
        let mut work = WriteWork::default();
        assert!(work.charge_scan(6, &limits).is_ok() && work.charge_scan(4, &limits).is_ok());
        assert_eq!(limit(work.charge_scan(1, &limits)), Some(("bytes of parts scanned", 10)));
        assert_eq!(work.scan_bytes, 10);
        // Copied bytes, the same.
        let mut work = WriteWork::default();
        assert!(work.charge_copy(10, &limits).is_ok());
        assert_eq!(limit(work.charge_copy(1, &limits)), Some(("bytes copied", 10)));
        assert_eq!(work.bytes_copied, 10);
        // Start tags looked at, against the work's own byte cap.
        let mut work = WriteWork { byte_cap: 10, ..WriteWork::default() };
        assert!(work.charge_attribute_bytes(10).is_ok());
        assert_eq!(limit(work.charge_attribute_bytes(1)), Some(("bytes of start tags looked at", 10)));
        assert_eq!(work.attribute_bytes, 10, "a refused charge is not counted");
        let mut work = WriteWork { byte_cap: 10, ..WriteWork::default() };
        assert_eq!(limit(work.charge_attribute_bytes(11)), Some(("bytes of start tags looked at", 10)));
        assert_eq!(work.attribute_bytes, 0);
        // Attributes written: two of two, then refused.
        let mut work = WriteWork::default();
        assert!(work.charge_attribute(&limits).is_ok() && work.charge_attribute(&limits).is_ok());
        assert_eq!(limit(work.charge_attribute(&limits)), Some(("attributes written", 2)));
        assert_eq!(work.attributes_written, 2);
        // Shapes compared: five steps of five pass, the sixth is refused; the steps of cells and declarations count with them.
        let mut work = WriteWork::default();
        for _ in 0..5 {
            assert!(work.charge_compared(&limits).is_ok());
        }
        assert_eq!(limit(work.charge_compared(&limits)), Some(("steps of comparing the drawing with the file", 5)));
        let mut work = WriteWork { cells_compared: 4, scope_steps: 0, ..WriteWork::default() };
        assert!(work.charge_compared(&limits).is_ok(), "one shape and four cells make five");
        assert!(work.charge_compared(&limits).is_err());
        // A walk past the step cap stops a charge for an attribute too (the cap is held from inside the loops that cannot return an error).
        let mut work = WriteWork { step_cap: 1, scope_steps: 2, ..WriteWork::default() };
        assert!(limit(work.charge_attribute(&limits)).is_some());
        assert_eq!(work.attributes_written, 0);
    }

    /// The read-back of the markers a save wrote looks in `styles.xml` for what that part's splice wrote, and refuses a marker that is not there; the numbers the
    /// styles of `content.xml` name are `styles.xml`'s to write, and are not looked for in it by this check.
    #[test]
    fn the_read_back_of_the_markers_a_save_wrote_refuses_one_that_is_not_in_styles_xml() {
        let root = concat!(
            r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
            r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
        );
        let content = format!(r#"<office:document-content {root}><office:body><office:drawing/></office:body></office:document-content>"#);
        let styles = |inner: &str| format!(r#"<office:document-styles {root}><office:styles>{inner}</office:styles></office:document-styles>"#);
        let four = markers::Key { number: 4, band: 0 };
        let art = markers::art(four, &mut 0).unwrap_or_else(|e| panic!("{e}"));
        let marker = markers::element(four, &art, "draw", "svg", "");
        let outs = |kind: PartKind, numbers: &[u32]| {
            let mut out = out_of(Vec::new(), Vec::new());
            out.markers = numbers.iter().map(|n| markers::Def::Marker(markers::Key { number: *n, band: 0 })).collect();
            out.markers_written = out.markers.clone();
            vec![(kind, out)]
        };
        let limits = Limits::default();
        let check = |styles: &str, outs: Vec<(PartKind, PartOut)>| {
            check_made_markers(&package_of(&content, styles), false, &outs, &limits, &mut WriteWork::default())
        };
        assert!(check(&styles(&marker), outs(PartKind::Styles, &[4])).is_ok(), "the marker is there");
        assert!(matches!(check(&styles(""), outs(PartKind::Styles, &[4])), Err(WriteError::SelfCheck(_))), "the marker is not there");
        assert!(check(&styles(""), outs(PartKind::Content, &[4])).is_ok(), "the numbers of `content.xml` are not looked for");
        assert!(check(&styles(""), outs(PartKind::Styles, &[])).is_ok(), "no marker was written");
    }

    /// Memory held for a time is taken from what a save may hold, refused as a limit in the words asked when there is not that much, and given back
    /// by `release`; what is kept for good is taken from the room by hand and never given back.
    #[test]
    fn memory_held_for_a_time_is_taken_from_the_room_refused_past_it_and_given_back() {
        let mut work = WriteWork { memory_cap: 1_000, memory_room: 600, ..WriteWork::default() };
        work.hold(250, "the words").expect("held");
        assert_eq!(work.memory_room, 350, "taken");
        work.release(250);
        assert_eq!(work.memory_room, 600, "and given back");
        // Not enough: refused in the words with the cap, and nothing is taken.
        assert!(matches!(work.hold(601, "the words"), Err(WriteError::Limit { what: "the words", max: 1_000 })));
        assert_eq!(work.memory_room, 600);
        // Exactly what is left is held, and the room is nothing.
        work.hold(600, "the words").expect("all of it");
        assert_eq!(work.memory_room, 0);
        assert!(matches!(work.hold(1, "the words"), Err(WriteError::Limit { .. })));
        work.release(600);
        assert_eq!(work.memory_room, 600);
        // What is kept for good (taken from the room by hand, while something is held) stays taken.
        work.hold(100, "the words").expect("held");
        work.memory_room -= 50;
        work.release(100);
        assert_eq!(work.memory_room, 550);
        // A save with no limit (`u64::MAX`) holds and gives back to the same.
        let mut free = WriteWork::default();
        free.hold(1 << 40, "the words").expect("held");
        free.release(1 << 40);
        assert_eq!(free.memory_room, u64::MAX);
    }

    /// What a part may still grow by is the reader's limit on items less what the file read cost; a copy is charged against it before it is made,
    /// refused in words (with the limit) past it with nothing taken, and given back when it was not made.
    #[test]
    fn the_room_for_a_tree_is_the_readers_limit_less_what_the_file_cost_and_a_charge_is_taken_from_it_and_given_back() {
        let limits = Limits { max_read_xml_nodes: 1_000, ..Limits::default() };
        let mut work = WriteWork::default();
        assert_eq!(work.tree_room, u64::MAX, "no room is set until the file is read");
        work.set_tree_room(&crate::index::ReadCost { tree_nodes: 300, ..crate::index::ReadCost::default() }, &limits);
        assert_eq!(work.tree_room, 700, "the limit less the items of the tree the read parsed");
        work.charge_tree_items(500, &limits).expect("room for 500");
        assert_eq!((work.tree_room, work.copied_items), (200, 500));
        // Past the room: refused in words with the reader's limit, and nothing is taken.
        let refused = work.charge_tree_items(201, &limits).expect_err("past the room");
        assert!(matches!(&refused, WriteError::Limit { what, max: 1_000 } if what.contains("items in a part")), "{refused:?}");
        assert_eq!((work.tree_room, work.copied_items), (200, 500), "nothing taken");
        // Exactly what is left is taken.
        work.charge_tree_items(200, &limits).expect("all of it");
        assert_eq!((work.tree_room, work.copied_items), (0, 700));
        assert!(work.charge_tree_items(1, &limits).is_err());
        // A copy that was not made gives its charge back.
        work.refund_tree_items(200);
        assert_eq!((work.tree_room, work.copied_items), (200, 500));
        // A file that read more than the limit leaves no room (and the subtraction does not wrap).
        work.set_tree_room(&crate::index::ReadCost { tree_nodes: 5_000, ..crate::index::ReadCost::default() }, &limits);
        assert_eq!(work.tree_room, 0);
    }

    /// The reader's cap on a tree is the **lesser** of `max_read_xml_nodes` and `container.xml.max_nodes` (`read::mod`), and so is the room a save has for
    /// copies, and the cap a refusal names.
    #[test]
    fn the_room_for_a_tree_is_held_to_the_lesser_of_the_two_caps_the_reader_has() {
        let cost = crate::index::ReadCost { tree_nodes: 300, ..crate::index::ReadCost::default() };
        for (read_cap, container_cap, lesser) in [(1_000usize, 400usize, 400u64), (400, 1_000, 400), (700, 700, 700), (1_000, 1_000, 1_000)] {
            let mut limits = Limits { max_read_xml_nodes: read_cap, ..Limits::default() };
            limits.container.xml.max_nodes = container_cap;
            let mut work = WriteWork::default();
            work.set_tree_room(&cost, &limits);
            assert_eq!(work.tree_room, lesser - 300, "{read_cap} and {container_cap}");
            let refused = work.charge_tree_items(lesser - 300 + 1, &limits).expect_err("one past the room");
            assert!(
                matches!(&refused, WriteError::Limit { max, .. } if *max == lesser),
                "{read_cap} and {container_cap}: the cap named is the lesser: {refused:?}"
            );
            work.charge_tree_items(lesser - 300, &limits).expect("exactly the room");
        }
        // A file that read more than the lesser cap leaves no room, whichever cap it is.
        let mut limits = Limits { max_read_xml_nodes: 1_000, ..Limits::default() };
        limits.container.xml.max_nodes = 200;
        let mut work = WriteWork::default();
        work.set_tree_room(&cost, &limits);
        assert_eq!(work.tree_room, 0);
    }

    /// A shape with `id`, and the members `children` (each a group's member).
    fn shape_with(id: u32, name: &str, children: Vec<Arc<diacraft_doc::Shape>>) -> Arc<diacraft_doc::Shape> {
        let mut shape = diacraft_doc::Shape::new(id);
        shape.name = Some(Arc::from(name));
        shape.children = diacraft_doc::ShapeList::from_vec(children);
        Arc::new(shape)
    }

    /// The shapes of a page by `ID` are those `Document::find_shape` finds: members of a group included, the first of an `ID` in document order (a group's
    /// members after the group and before the next shape), and none deeper than it searches; and each shape visited is a step.
    #[test]
    fn the_shapes_of_a_page_by_id_are_the_first_of_each_id_in_document_order_down_to_the_depth_searched() {
        let name_of = |page: &diacraft_doc::Page, id: u32| {
            let held = shapes_by_id(page, 0, &mut WriteWork::default(), &Limits::default()).expect("made");
            held.get(&id).and_then(|s| s.name.as_deref().map(str::to_owned))
        };
        let mut page = diacraft_doc::Page::new(1, "p");
        // `a` holds `c` (ID 3), `d` and another with ID 3; then `b`; then another shape with ID 3 and another with ID 1; `e` is inside `d`.
        let e = shape_with(5, "e", Vec::new());
        let d = shape_with(4, "d", vec![e]);
        let c = shape_with(3, "c", Vec::new());
        let a = shape_with(1, "a", vec![c, d, shape_with(3, "second c", Vec::new())]);
        for shape in [a, shape_with(2, "b", Vec::new()), shape_with(3, "second 3", Vec::new()), shape_with(1, "second 1", Vec::new())] {
            page.shapes.push(shape);
        }
        // The group's members come after the group and before the next shape, so the member `c` is the first with ID 3, and `a` the first with ID 1.
        assert_eq!(name_of(&page, 1).as_deref(), Some("a"));
        assert_eq!(name_of(&page, 3).as_deref(), Some("c"));
        assert_eq!(name_of(&page, 2).as_deref(), Some("b"));
        assert_eq!(name_of(&page, 4).as_deref(), Some("d"));
        assert_eq!(name_of(&page, 5).as_deref(), Some("e"), "a member of a member");
        assert_eq!(name_of(&page, 6), None);
        // Each of the eight shapes is a step, the room is made for what was asked, and the lookup agrees with the model's own for every `ID`.
        let mut work = WriteWork::default();
        let held = shapes_by_id(&page, 8, &mut work, &Limits::default()).expect("made");
        assert_eq!((work.order_steps, held.len()), (8, 5));
        let mut doc = diacraft_doc::Document::empty();
        doc.pages.push(Arc::new(page.clone()));
        for id in 1..=6 {
            let owner = diacraft_doc::Owner::Page(doc.pages.get(0).map_or(0, |p| p.id));
            let found = doc.find_shape(owner, id).and_then(|at| doc.shape(&at)).map(|s| s.name.clone());
            assert_eq!(held.get(&id).map(|s| s.name.clone()), found, "ID {id}");
        }
        // The depth: a chain of MAX_SHAPE_DEPTH shapes is searched to its last, one more is not (the model's search stops where this does).
        let mut deep = shape_with(1_000, "bottom", Vec::new());
        let mut page = diacraft_doc::Page::new(1, "deep");
        for level in (1..=diacraft_doc::MAX_SHAPE_DEPTH).rev() {
            let id = u32::try_from(level).unwrap_or(1);
            deep = shape_with(id, "link", vec![deep]);
        }
        page.shapes.push(deep);
        let held = shapes_by_id(&page, 0, &mut WriteWork::default(), &Limits::default()).expect("made");
        assert!(held.contains_key(&u32::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap_or(0)), "the last level searched is there");
        assert!(!held.contains_key(&1_000), "the shape one level deeper is not");
        let mut doc = diacraft_doc::Document::empty();
        doc.pages.push(Arc::new(page));
        let owner = diacraft_doc::Owner::Page(doc.pages.get(0).map_or(0, |p| p.id));
        assert!(doc.find_shape(owner, 1_000).is_none(), "and the model's own search agrees");
    }
}
