//! DiaCraft OpenDocument Drawing: the `.odg` package, and an element-level comparison of two packages (the design is the `.odg` format's).
//!
//! This crate is the OpenDocument drawing format: its package layer, on the bounded ZIP and XML layer of `diacraft-container` (layer 0;  the caps and error classes are the VSDX reader's),
//! and its reader into the ShapeSheet model, which keeps what the model has no cell for in
//! its `odf` slots. The writer ([`write_with`]) splices an edit into a package and builds on both, and [`compare_content`] says whether
//! two packages hold the same content.
//!
//! * [`sniff`] says whether bytes are a drawing or a drawing template ([`Variant`]), from what the package says it is. A template
//!   (`.otg`) is read as a drawing is, and says what it is ([`ReadReport::variant`]); [`write_with`] saves it as a template, with every rule of the
//!   splice, or as a drawing (and a drawing as a template) when [`WriteOptions::variant`] asks, changing the media type in `mimetype` and in the
//!   manifest's root row and nothing else.
//! * [`package::Package::read`] reads a package whole: every entry by its exact name, the manifest's rows, what the package says it is,
//!   and whether it is signed or holds macros; an encrypted package is refused before anything but its manifest is inflated.
//! * [`read`] reads a drawing into a [`diacraft_doc::Document`] and a [`ReadReport`]: styles, pages and master pages, layers, the plain
//!   shapes, custom shapes with their enhanced geometry, connectors with their glue, groups, frames with text, a picture or an object, and
//!   text, with the [`ShapeIndex`] that maps each shape back to its element.
//!   What it does not model is kept and counted (see `read`).
//! * [`package::copy_unchanged`] writes a package again with every entry copied as it is: what an unedited save is. [`copy_account`]
//!   says what that copy rewrites and leaves out, so that a copy that is not the same file always has a stated cause.
//! * [`write_with`] with no original writes a drawing from the model alone: [`compose_base`] makes the package of a first page, the model's `meta.xml`, `settings.xml`,
//!   manifest and package entries are written into it, and the pages and shapes are spliced in, a shape that holds the markup it was read with written as it was read
//!   and the others as new ones ([`WriteOptions::content_unchanged`] says whether the bookkeeping is the model's or an edited save's). The engine saves a `.diacraft` drawing, a new one
//!   and one opened from a VSDX file as an `.odg` through it.
//! * **Both writers carry what ODF cannot say** (`ext`): DiaCraft's cells on a shape's own automatic style, items in DiaCraft's one
//!   document file, and a VSDX file's parts as entries of `DiaCraft/`, each stamped with what the file says of its object, and a read keeps an item only
//!   while its stamp holds. A page's comments are `office:annotation` elements in both writers.
//! * A flat drawing (`.fodg`, `flat`) is read as the package that holds the same elements, and the splice writes it back flat. The read's
//!   first warnings say what the drawing does not hold, by class with a count ([`NOT_KEPT_HEAD`]).
//! * [`diff_packages`] compares two packages part by part and, for the XML parts, element by element ([`PackageDiff`]), each element named
//!   by its path, so that a test of an edit can say exactly which elements changed, and a change of white space alone is told apart.
//!
//! **Nothing panics, and every loop's work is bounded and counted** in the units it walks: the container's [`Work`] for the archive and
//! the XML, [`ReadWork`] for a read and [`DiffWork`] for a comparison. Every error has a [`class`](OdgError::class), the spec's word where
//! it names one.
//!
//! **No file is opened and nothing is run**: names are keys, macros and scripts are bytes, signatures are bytes that are not
//! checked, and nothing is decrypted.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod base;
mod canon;
pub mod diff;
mod error;
mod ext;
mod figures;
mod flat;
mod index;
pub mod limits;
mod ns;
pub mod package;
mod package_data;
mod picturetype;
mod read;
mod report;
mod sections;
mod vocab;
pub mod write;

#[cfg(kani)]
mod kani_proofs;

pub use base::{BaseLayout, compose_base};
pub use canon::{CANON_RULES, compare_content, compare_content_counted};
pub use diacraft_container::Work;
pub use diacraft_container::zip::{CopyAccount, Dropped};
pub use diff::{
    DiffCounts, DiffWork, ElementChange, ElementPath, HeaderChange, Moved, PackageDiff, PartDiff, Unparsed, diff_packages, diff_packages_counted,
};
pub use error::{ErrorKind, LimitKind, MimetypeFault, Missing, OdgError, Warning};
pub use figures::Figures;
pub use flat::Form;
pub use index::{OriginalPackage, PageShapes, PageSource, ReadCost, ShapeIndex};
pub use limits::Limits;
pub use package::{Manifest, ManifestEntry, Package, copy_unchanged, copy_unchanged_counted, part_label};
pub use package_data::{PackageData, read_package_data};
pub use read::{TEXT_OBJECT_ROW, read, read_partial};
pub use report::{CarryReport, NOT_KEPT_HEAD, NOT_KEPT_LINE_CHARS, ReadReport, ReadWork, Stats};
pub use write::{SaveNote, WriteError, WriteOptions, WriteWork, Written, model_refusal, without_carried_objects, write_counted, write_with};

/// What an OpenDocument drawing package is, by its media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// `application/vnd.oasis.opendocument.graphics`: a drawing, `.odg`.
    Drawing,
    /// `application/vnd.oasis.opendocument.graphics-template`: a drawing template, `.otg`.
    Template,
}

impl Variant {
    /// The variant whose media type is exactly `media_type`, or `None`.
    pub fn from_media_type(media_type: &str) -> Option<Variant> {
        match media_type {
            package::DRAWING_MEDIA_TYPE => Some(Variant::Drawing),
            package::TEMPLATE_MEDIA_TYPE => Some(Variant::Template),
            _ => None,
        }
    }

    /// The media type.
    pub fn media_type(self) -> &'static str {
        match self {
            Variant::Drawing => package::DRAWING_MEDIA_TYPE,
            Variant::Template => package::TEMPLATE_MEDIA_TYPE,
        }
    }

    /// The file extension, without its dot: `odg` or `otg`.
    pub fn extension(self) -> &'static str {
        match self {
            Variant::Drawing => "odg",
            Variant::Template => "otg",
        }
    }
}

/// Whether `bytes` are an OpenDocument drawing or drawing template, by what the package says it is: the manifest's root row, else the
/// `mimetype` entry. `None` for anything else: another OpenDocument type, a ZIP that is not an OpenDocument package, a flat
/// `.fodg`, or bytes that are not a ZIP. Only the archive's directory, the manifest and `mimetype` are read, within the default
/// limits, and a package that is encrypted is still told by its manifest, so that its reader can say why it does not open.
pub fn sniff(bytes: &[u8]) -> Option<Variant> {
    sniff_form(bytes).map(|(variant, _)| variant)
}

/// [`sniff`], and the [`Form`] the drawing is written in: a package, or a flat `.fodg`. A flat file is told by its first bytes alone
/// (the root's start tag within the first 64 KiB, by its namespace), so a flat drawing whose `office:mimetype` is another kind of document is `None`, and one
/// that says nothing this crate knows is a [`Variant::Drawing`], which [`read`] decides.
pub fn sniff_form(bytes: &[u8]) -> Option<(Variant, Form)> {
    match Form::of(bytes) {
        Form::Flat => flat::head(bytes).and_then(|head| head.claim.variant()).map(|variant| (variant, Form::Flat)),
        Form::Package => {
            package::sniff_media(bytes, &Limits::default()).ok().flatten().and_then(|t| Variant::from_media_type(&t)).map(|v| (v, Form::Package))
        }
    }
}

/// What [`copy_unchanged`] makes of the package in `bytes`, without writing it (the container's
/// [`copy_account`](diacraft_container::zip::copy_account)): how many entries keep their headers byte for byte and how many are copied
/// under headers written anew, whether the directory lists the entries in another order than they lie, and the bytes the copy leaves out,
/// by why ([`Dropped`]: bytes no header announces, the directory's bytes after its last row, the ZIP64 end records, the comment, bytes
/// after the end record). The copy is the file, byte for byte, exactly when [`CopyAccount::is_exact`].
///
/// The archive's directory is read within `limits` as [`copy_unchanged`] reads it, every row kept (the package's own rules about names
/// and the manifest refuse more, never less), and nothing is inflated; what does not read is the error the archive gives.
pub fn copy_account(bytes: &[u8], limits: &Limits) -> Result<CopyAccount, OdgError> {
    copy_account_counted(bytes, limits, &mut Work::default())
}

/// [`copy_account`], adding what it cost to `work`: the archive's directory as it is read, and the records of the local headers' extra fields
/// the account looked through ([`Work::local_extra_records`]).
pub fn copy_account_counted(bytes: &[u8], limits: &Limits, work: &mut Work) -> Result<CopyAccount, OdgError> {
    let mut judge = diacraft_container::zip::Plain::new(&limits.container);
    let archive = diacraft_container::zip::Archive::read_with(bytes, &limits.container, &mut judge, work)?;
    Ok(diacraft_container::zip::copy_account_counted(&archive, work)?)
}
