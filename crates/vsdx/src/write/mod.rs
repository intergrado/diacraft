//! Saving: a document to a package.
//!
//! [`write`] takes the document and, for a document that was read from a file, the bytes of that file ([`OriginalBytes`]), and gives the bytes
//! of the package. **A save into a file does not rewrite the whole document from the model**:
//!
//! * **A part no edit touched is the original's, byte for byte**: every unknown part, the themes, the pictures, a VBA project, the
//!   embedded objects, the solutions of other writers, and every page and master the edit did not reach. They are taken from the original
//!   bytes and are not read into the model.
//! * **A part an edit touched is the original's tree, patched at the edited nodes** (`patch.rs`): the model as it was read and the
//!   model as it is are compared, and only what differs is written; unknown elements and attributes, comments, namespace declarations and the
//!   order of all of it stay where they were, and a cell nobody wrote keeps the text it had (`1.50` stays `1.50`).
//! * **Theme parts are added, rewritten and taken out** when the model's themes differ from the baseline's (`themes.rs`): a theme the
//!   model gained is a new part with a relationship from the document part and an `Override`, one whose bytes changed is rewritten in its own
//!   part, one it lost goes with its relationship, and every other theme part is the original's, byte for byte.
//! * **The content types and every relationships part are regenerated** (`parts.rs`) from the original's rows: every
//!   `Default` and `Override`, including those for no part, and every relationship with its `Id`, `Type`, `Target` and `TargetMode`, are carried
//!   A row is added for a part the edit added and taken out for one it removed. A relationships part no edit touched is not rewritten.
//!   **The content types stream is the file's own bytes when no row changes** (a save that adds no part, takes none out and gives none a
//!   type changes nothing in it), and when one does it is the file's tree, laid out as the file laid it out (`TypesTree`).
//!
//! **A new drawing is written the same way**, from [`skeleton`]: the package of a drawing with no page, which the save then brings to the
//! document, so adding the first page of a new drawing and the tenth page of a file read are one code path.
//!
//! **Background pages come first in the pages index**: the writers that make the index from the model, a drawing never saved and a model written
//! from the model alone, list every background page before every foreground page, as all 70 files of the corpus that hold both kinds do, with each page's `ID`, name, `BackPage` and content
//! unchanged and the pages of each kind in the order the model has ([`diacraft_doc::Document::pages_background_first`]). **A save into a file keeps the order the file has**, whatever it is: the
//! splice changes only what an edit changed, and a page the model moved to another place is written to that place.
//!
//! **A model that holds anything of a VSDX package is written from the model alone** (`held.rs` is its plan and `model.rs` its trees and package): a document that was read from a package
//! ([`diacraft_doc::Document::vsdx`]), one that holds a part of its own, or one with unknown elements of its document, has no file to splice into when it comes from a `.diacraft` file, and its parts, rows, names and
//! slots are the model's. `write_with(doc, None, options)` writes all of it, and [`WriteOptions::content_unchanged`] says whether the bookkeeping of the file is kept or written as an edited save writes it. A save into a
//! file (`original` given) also writes a change to the document properties and to the unknown elements of the document, which it refused before.
//!
//! **The order of the entries is a defined order**, so that one model saved twice is the same bytes, in one process or in two: the
//! content types stream first and the package relationships second, then the parts the source package had, in the order of its
//! central directory, then the parts the save made, in the order the writer makes them, then the relationships parts it made, by the place of the part
//! each belongs to. The lists of what a save did ([`Written::rewritten`], [`Written::added`], [`Written::removed`]) are in the same orders. Nothing in
//! the writer takes its order from a hash map.
//!
//! **An object frame of an `.odg` that is carried back is written as an embedding** (`objects.rs`): by all three writers (a drawing never saved, one written from the model alone, and a save into a file), through the rows
//! `page_rows.rs` makes for the shapes of a page part that a save makes new. The object part, its relationship part with the picture and a relationship of the type `oleObject` from the page are made, and the carrier leaves out what went natively.
//!
//! **A save is checked before it is returned.** The bytes are read again with this crate's own reader; a package that does not read is an
//! error and the caller never sees it, so the file it was going to replace is not replaced by a damaged one. The document that reading gives is
//! returned too: it is what the file holds, and the baseline for the next save.
//!
//! **What a save does beyond the model** (all of it in the report as assumed about the reference app unless a spec settles it): `Application` of
//! `docProps/app.xml` is `DiaCraft`; the thumbnail is removed when there are edits; `modified` is set when there are
//! edits and a time is given; the page names in `app.xml` follow the pages; `RecalcDocument` is added on request. A
//! property part that is not usable XML is kept as it was, and the save says so (the reader opens such a file, so the save must not refuse it). A
//! property part is found by the package relationships and by its content type, so that an extended properties part under another properties
//! relationship is the one that is named (`Plan::property_part`).
//!
//! **The six types**. A Save As to another type ([`WriteOptions::variant`]) rewrites the document
//! part's `Override` and keeps every other part. A VBA project (the project, its signature parts, their relationships and types) is bytes in every
//! macro type; a type that cannot hold one is refused unless the caller chose to leave it out ([`WriteOptions::drop_macros`]), and then the
//! warning names what was left out. A stencil written as a drawing or a template gets an empty page part for each page row that had none; a drawing
//! or template written as a stencil keeps its pages and is told so. A copy made from a template is a new document ([`WriteOptions::new_copy`]):
//! its `created` and `modified` are the save's.
//!
//! **What it refuses**, as an error that writes nothing: a change to something the model keeps and this does not write yet
//! ([`WriteError::Unsupported`]), an original that is not the file the model was read from ([`WriteError::Mismatch`]), saving a package with a
//! VBA project to a type that cannot hold one unless the caller chose to drop it, a character XML cannot hold, and a package the
//! container cannot hold (4 GiB, 65,535 parts).

mod build;
mod comments;
mod diacraft;
mod held;
mod model;
mod objects;
mod page_rows;
mod par;
mod parts;
mod patch;
mod props;
mod root;
mod settings;
mod skeleton;
pub use skeleton::new_windows_part;
mod themes;
mod types;

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use diacraft_doc::{Connects, Document, Ext, Format, Master, Page, Shape, ShapeList};
use diacraft_opc::name::{fold, is_rels_part, rels_source, resolve};
use diacraft_opc::xml::{El, Node};
use diacraft_opc::{Package, PackageWriter};

use self::build::{master_attrs, page_attrs, rest_of};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use self::model::{picture_by_bytes, typed_place};
#[cfg(kani)]
pub(crate) use self::objects::{is_compound, sextet, too_large as object_too_large};
use self::par::{Rewrite, patch_together};
pub(crate) use self::parts::relative_target;
use self::parts::{CT_RELS, NS_TYPES, OD, PK, RelRow, RelsTree, Tree, VIS, folder_of, rel_type, rels_name};
use self::types::TypesTree;
use crate::limits::Limits;
use crate::ns::Scope;
use crate::props::{CT_APP, CT_CORE, CT_CUSTOM};
use crate::read::Known;
use crate::report::{Kind, Variant};
use crate::unheld::part_label;
use crate::{ReadReport, VsdxError};

const CT_PAGE: &str = "application/vnd.ms-visio.page+xml"; // brand-ok: a content type the format defines
const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml"; // brand-ok: a content type the format defines
const CT_MASTER: &str = "application/vnd.ms-visio.master+xml"; // brand-ok: a content type the format defines
const CT_MASTERS: &str = "application/vnd.ms-visio.masters+xml"; // brand-ok: a content type the format defines
const CT_OCTET: &str = "application/octet-stream";
const VBA_PROJECT: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject"; // brand-ok: a URI the format defines
/// The content types of the VBA project and its signature parts all begin with this.
const VBA_CONTENT_TYPE: &str = "application/vnd.ms-office.vbaProject";
/// The most part names a warning about dropped macros lists; the count says the rest.
const MACRO_NAMES_SHOWN: usize = 6;
const REMOVABLE_TARGETS: [&str; 4] = ["/image", "/oleObject", "/package", "/activeXControlBinary"];

/// The file a document was read from, for a save that keeps what the model does not hold.
#[derive(Clone, Copy)]
pub struct OriginalBytes<'a> {
    /// The bytes of the file as they were read.
    pub bytes: &'a [u8],
    /// The document as the reader made it from `bytes`, when the caller has it: the model the edits are compared with. Without it the save
    /// reads `bytes` again.
    pub baseline: Option<&'a Document>,
}

/// What a save may be asked.
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    /// The variant to write: `None` keeps the original's, and a new drawing is a `.vsdx`.
    pub variant: Option<Variant>,
    /// Seconds since 1970-01-01 UTC, for `modified` when there are edits and for `created` and `modified` of a new drawing. The writer has no
    /// clock: the caller gives the time (so that a save in a test is the same bytes, and a web build has nothing to panic on). `None` leaves
    /// the times as they are.
    pub now: Option<i64>,
    /// Ask the reference app to recalculate the whole document when it opens: for a caller that changed geometry and did not recompute what
    /// depends on it. Added once, found by name first.
    pub recalc_document: bool,
    /// The caller's choice to leave the macro project out when the type written cannot hold one: without it, a package with a VBA
    /// project saved to `.vsdx`, `.vssx` or `.vstx` is [`WriteError::MacroProject`] and nothing is written. With it, the project, its
    /// signature parts and their relationships and types are taken out, and a warning says so in words. It does nothing when the type written
    /// allows macros or the package has no project.
    pub drop_macros: bool,
    /// The file is the source of a **new document**, a copy made from a template and not the template itself: `created` and `modified` of
    /// `docProps/core.xml` are set to `now` (they are the template's otherwise), and every other property stays.
    pub new_copy: bool,
    /// **The content of the drawing is as it was when it was read** (the content stamp of the native format, design section 4.4): a save from the model alone has no file to
    /// compare with, so the caller says whether the model is the one it read. With `true` the writer keeps the bookkeeping as the file had it: the thumbnail and its
    /// relationship, `Application` of `docProps/app.xml`, and `modified` of `docProps/core.xml`. With `false` (the default) it writes the bookkeeping of an edited save: the
    /// thumbnail part and its relationship are left out (the `Default` row for its extension stays), `Application` is `DiaCraft`, and `modified` is the save's `now`. **Only the
    /// model writer reads it**: a save into a file (`original` given) judges for itself, by comparing the model with the file's.
    pub content_unchanged: bool,
    /// **A Save with no edit since the caller last wrote this very file** (an unedited Save after a Save As is no edit and writes the same bytes). A save into a file
    /// (`original` given) that finds no edit then leaves `Application` of `docProps/app.xml` as the file has it, where every other save names DiaCraft there: a
    /// file the model writer wrote with the bookkeeping it read (`content_unchanged`, the unedited visit) names the program it was read from, and the Save after it
    /// named DiaCraft, other bytes with no edit. The caller gives `false` for a file it read from the disk, which is saved as before; an edit is stamped as before. The model writer does not read it.
    pub resave_unedited: bool,
}

/// The result of a save.
pub struct Written {
    /// The package.
    pub bytes: Vec<u8>,
    /// The document as reading `bytes` gives it: the baseline for the next save.
    pub document: Document,
    /// What reading `bytes` said.
    pub read_back: ReadReport,
    /// What was odd about the save, in words.
    pub warnings: Vec<String>,
    /// The parts that were written (new bytes) and existed before; the parts that are new; the parts that were taken out. Names, with the slash.
    pub rewritten: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// How many parts are the original's, byte for byte.
    pub kept: usize,
    /// The parts of the macro project that were left out at the caller's choice ([`WriteOptions::drop_macros`]): names, with the slash. Empty
    /// when nothing was dropped.
    pub macros_dropped: Vec<String>,
    /// What the file carries of the other format's data (the save note says it).
    pub carried: Carried,
    /// **The parts of the other format that this save wrote as real VSDX**: the files of the objects it wrote as embeddings, by the name the model has them (`Object 1`), in the model's
    /// order. The file holds each object once, as an embedding, and the model the caller saved still holds the part it was written from. A caller that saves into this file again gives the writer the
    /// model without these parts: the writer judges what rides by comparing the model with the file, and would find a part riding that the file does not carry, and write the object a second time
    /// in DiaCraft's own part. Empty when no object was written as an embedding.
    pub natively: Vec<std::sync::Arc<str>>,
}

/// What a save wrote into the file of the data of the other format that a VSDX file has no place for (`carry.rs`): how many items of DiaCraft's solution part and parts of DiaCraft's own, and whether a macro
/// project or embedded objects of the other format are among the parts. The classes of the data are the engine's to say (it knows the model); this says that something rides, and that macros and objects do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Carried {
    /// The items written, one for the document's data and one for each page, master, style and shape that holds some.
    pub items: usize,
    /// The parts of DiaCraft's own (`/diacraft/blob<n>.<ext>`): the bytes of the other format's parts and pictures.
    pub parts: usize,
    /// A macro project of the other format rides.
    pub macros: bool,
    /// Embedded objects of the other format ride.
    pub objects: bool,
}

impl fmt::Debug for Written {
    /// The sizes and the part lists, not the package or the document.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Written")
            .field("bytes", &self.bytes.len())
            .field("warnings", &self.warnings)
            .field("rewritten", &self.rewritten)
            .field("added", &self.added)
            .field("removed", &self.removed)
            .field("kept", &self.kept)
            .field("macros_dropped", &self.macros_dropped)
            .field("carried", &self.carried)
            .finish()
    }
}

/// Why a document could not be written. A message holds words, numbers and part names, never the content of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The original bytes could not be read again.
    Original(VsdxError),
    /// The container refused: a name, a type, a size.
    Package(diacraft_opc::WriteError),
    /// A tree that XML cannot hold: a character, a name.
    Xml { part: String, error: diacraft_opc::xml::XmlWriteError },
    /// An edit to something the model keeps and the writer does not write yet. Nothing was written.
    Unsupported(&'static str),
    /// The original is not the file the model was read from. Nothing was written.
    Mismatch(&'static str),
    /// A cap of the model passed.
    Limit(&'static str),
    /// A package with a VBA project, to a type that cannot hold one.
    MacroProject,
    /// A part of the other format's data that cannot ride in the file: its name is not one a package can hold, or it is past the cap of a part. Nothing was written.
    Carried { part: String, why: &'static str },
    /// The bytes written do not read back.
    SelfCheck(String),
    /// A worker thread of the save stopped while it made a part of the file (a bug, which the save reports and does not abort for). Nothing was written.
    Worker(&'static str),
}

impl WriteError {
    /// A word for a harness to compare.
    pub fn class(&self) -> &'static str {
        match self {
            WriteError::Original(e) => e.class(),
            WriteError::Package(e) => e.class(),
            WriteError::Xml { .. } => "xml_not_writable",
            WriteError::Unsupported(_) => "write_unsupported",
            WriteError::Mismatch(_) => "write_mismatch",
            WriteError::Limit(_) => "write_limit",
            WriteError::MacroProject => "macro_project_dropped",
            WriteError::Carried { .. } => "carried_part_refused",
            WriteError::SelfCheck(_) => "write_self_check",
            WriteError::Worker(_) => "write_worker",
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Original(e) => write!(f, "the file this drawing was opened from cannot be read again: {e}"),
            WriteError::Package(e) => write!(f, "the file cannot be made: {e}"),
            // A part name can hold a person's words: only a name that is package structure is shown (`part_label`).
            WriteError::Xml { part, error } => write!(f, "{} cannot be written: {error}", part_label("a part", part)),
            WriteError::Unsupported(what) => write!(f, "saving does not yet write this change: {what}"),
            WriteError::Mismatch(what) => write!(f, "the file on record is not the one this drawing was opened from: {what}"),
            WriteError::Limit(what) => write!(f, "the drawing is past a limit: {what}"),
            WriteError::MacroProject => {
                write!(
                    f,
                    "this file holds a macro project, which a file of the type chosen cannot hold; save it as a type that allows macros (.vsdm, .vssm or .vstm) to keep it, or choose to leave the macros out of the new file"
                )
            }
            WriteError::Carried { part, why } => {
                // The name of an entry of the other format is a person's, as any file name is: it is shown when it holds nothing but letters, digits, spaces and `/ . _ -`, and is short.
                let plain = !part.is_empty()
                    && part.len() <= 80
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b' '));
                let named = if plain { format!("the part {part}") } else { "a part".to_owned() };
                write!(f, "{named} of the data DiaCraft carries from another format cannot be written into the file: {why}")
            }
            WriteError::SelfCheck(why) => write!(f, "the file just made does not read back, so it was not kept: {why}"),
            WriteError::Worker(what) => write!(f, "a worker thread of the save stopped while it {what}, so nothing was saved"),
        }
    }
}

impl std::error::Error for WriteError {}

type R<T> = Result<T, WriteError>;

/// The error a check of the file just made gives: **a read that ran out of memory says so, in the words of a save** (the file was made, and what is left of the memory does not hold the check of it: the file
/// is not kept), and not that the file does not read, which it was never found not to. Any other failure is a file that does not read back.
pub(super) fn read_back_failed(e: VsdxError) -> WriteError {
    match e {
        // The reader's budget, and the memory of the process running out while a part of the file is inflated (the container's `try_reserve`): both are a check the memory left does not hold.
        VsdxError::Limit { kind: crate::error::LimitKind::Memory, .. } | VsdxError::Package(diacraft_opc::OpcError::OutOfMemory { .. }) => {
            WriteError::Limit(
                "there is not enough memory left to check the file just made (it is as large as the parts and pictures it holds), so it was not kept",
            )
        }
        other => WriteError::SelfCheck(other.to_string()),
    }
}

/// Writes `doc` as a package, with default options. See the [module documentation](self).
pub fn write(doc: &Document, original: Option<&OriginalBytes<'_>>) -> Result<Vec<u8>, WriteError> {
    write_with(doc, original, &WriteOptions::default()).map(|w| w.bytes)
}

/// [`write`] with options, and the report of what was done. One thread: see [`write_with_threads`].
pub fn write_with(doc: &Document, original: Option<&OriginalBytes<'_>>, options: &WriteOptions) -> Result<Written, WriteError> {
    write_with_threads(doc, original, options, 1)
}

/// [`write_with`] **on up to `threads` threads**: the parts of the pages and masters that an edit touched are patched on workers, one part to a thread and no more at once than the
/// memory of their trees allows, and the parts of the package are compressed on workers (`diacraft_opc::PackageWriter::add_all`); the results are put back in the order the parts have. **The bytes
/// are the bytes of one thread, at any number of threads**, and so is every error, which is the first of the sequence. 1 is the single-threaded path, which makes no thread.
pub fn write_with_threads(
    doc: &Document,
    original: Option<&OriginalBytes<'_>>,
    options: &WriteOptions,
    threads: usize,
) -> Result<Written, WriteError> {
    match original {
        Some(o) => run(doc, o.bytes, o.baseline, options, false, threads),
        None => write_new(doc, options, threads),
    }
}

/// A drawing with no file to splice into: a drawing never saved, and a model written from the model alone. **The pages index is made from the model, with the background pages first** (in each of the 70 files of the corpus that hold both kinds, every background page comes before every foreground one):
/// a page keeps its `ID`, its name, its
/// `BackPage` and what it holds, and the pages of each kind keep the order the model has. The drawing handed back is what the file reads as, in that order. A file that is saved into
/// keeps the order it has (the splice changes only what an edit changed), so this is for the writers that make the index.
fn write_new(doc: &Document, options: &WriteOptions, threads: usize) -> R<Written> {
    // **A language cell is written as the files hold one** (`plain_languages.rs`): the tag alone, where the drawing holds a string formula for it. A model has no file to compare
    // with, so every cell is looked at; the engine asks for the same drawing before it writes one from the model, and the check after the save finds the file as it was written.
    let plain = crate::plain_languages::plain_languages(doc, None);
    let doc = plain.as_ref().unwrap_or(doc);
    // **The text of a `User` or `Field` row says `STR`** (`string_units.rs`): the `Value` of every such row that holds a text is given the unit, whatever made it (a `.diacraft` file of an
    // earlier build holds the text of a container's `User` rows with none), **in a drawing whose units are DiaCraft's to state**: one that came from no VSDX package or from one that names DiaCraft. The
    // cells of the package of another program are written as they were read. The engine asks for the same drawing before it writes one from the model.
    let strung = crate::string_units::string_units(doc);
    let doc = strung.as_ref().map_or(doc, |fitted| &fitted.doc);
    let ordered = doc.pages_background_first();
    // With the pages put in order the file is not what the drawing was read as, so the bookkeeping of an unedited visit (the thumbnail, the program's name) is not kept.
    let edited_options;
    let (doc, options) = match &ordered {
        Some(in_order) => {
            edited_options = WriteOptions { content_unchanged: false, ..options.clone() };
            (in_order, &edited_options)
        }
        None => (doc, options),
    };
    // **The root style states what the program assumes**: a model that came from no VSDX file is written with the cells of its style 0 that a reader would take as another value
    // than the assumed one, and the model handed back keeps the root the caller had (see [`root::stated`]).
    let stated = root::stated(doc);
    let to_write = stated.as_ref().map_or(doc, |s| &s.doc);
    // A model that holds anything of a VSDX package is written from the model alone (`held.rs`); a drawing made new is the skeleton, patched.
    let mut written = if held::wanted(to_write) {
        held::write(to_write, options)?
    } else {
        // **The page names are the ones the program that opens the file keeps** (`page_names.rs`), before the skeleton is made: its list of page names in the properties is made from
        // the drawing it is given. `run` finds nothing more to rename in what it is handed.
        let kept = crate::page_names::keep_page_names(to_write, None);
        let to_make = kept.as_ref().map_or(to_write, |k| &k.doc);
        let skeleton = skeleton::build(to_make, options)?;
        let mut made = run(to_make, &skeleton, None, options, true, threads)?;
        if let Some(words) = kept.as_ref().filter(|k| !k.renamed.is_empty()).map(crate::page_names::KeptNames::words) {
            made.warnings.insert(0, words);
        }
        made
    };
    if let Some(stated) = &stated {
        stated.give_back(&mut written.document);
    }
    Ok(written)
}

fn run(doc: &Document, bytes: &[u8], baseline: Option<&Document>, options: &WriteOptions, new_file: bool, threads: usize) -> R<Written> {
    let limits = Limits::default();
    // **The parts the model holds as bytes are the model's**: the file's entry of such a part, when it holds those bytes (its size and checksum), is not inflated into a
    // copy beside them, and the part the save writes is the model's bytes, which are the file's. The model holds every part it does not rebuild, so a copy of each, here
    // and in the check of the file made (`finish`), would hold every embedded object two or three times over where the save before it held it once.
    let known = Known::of(doc);
    let pkg = known.package(bytes, &limits.opc).map_err(|e| WriteError::Original(VsdxError::Package(e)))?;
    let owned;
    let base: &Document = match baseline {
        Some(b) => b,
        None => {
            let mut read_back = crate::read::read_back(bytes, &limits, &known).map_err(WriteError::Original)?.doc;
            if new_file {
                // The skeleton's font table is the model's table and the faces its cells name (`skeleton.rs`). What it holds beyond the model's
                // own is the writer's, not an edit of the caller's: the baseline says the table is the caller's, so that the table is not
                // rewritten to the caller's (shorter) one.
                read_back.fonts = doc.fonts.clone();
            }
            owned = read_back;
            &owned
        }
    };
    // (A drawing that was never saved and holds unknown elements of its document is the model writer's, `held::wanted`; the skeleton holds only the Snap & Glue words.)
    // **The page names are the ones the program that opens the file keeps** (`page_names.rs`): a page the file holds under its names is left as it is, and a page that is new or
    // renamed has the names that program would give, said in the save's warnings.
    let kept = crate::page_names::keep_page_names(doc, Some(base));
    let doc = kept.as_ref().map_or(doc, |k| &k.doc);
    // **A language cell is written as the files hold one** (`plain_languages.rs`): a cell the file holds as it is stands, and one the edit wrote is the tag alone.
    let plain = crate::plain_languages::plain_languages(doc, Some(base));
    let doc = plain.as_ref().map_or(doc, |p| p);
    // **The text of a `User` or `Field` row of a file DiaCraft wrote says `STR`** (`string_units.rs`): the cells an earlier build wrote with no unit (the cached text of a field row, which a save keeps
    // as the file had it) are given it, and a file another program wrote is kept as that program wrote it. A drawing never saved was given the unit by `write_new`.
    let strung = (!new_file && props::names_diacraft(&pkg)).then(|| crate::string_units::string_units_in_file(doc)).flatten();
    let doc = strung.as_ref().map_or(doc, |fitted| &fitted.doc);
    // The themes of a drawing that was never saved are added to the skeleton like any other (`themes.rs`).
    let mut plan = Plan::new(pkg, base, doc, options, new_file)?;
    if let Some(words) = kept.as_ref().filter(|k| !k.renamed.is_empty()).map(crate::page_names::KeptNames::words) {
        plan.warnings.push(words);
    }
    plan.threads = threads;
    plan.layout()?;
    plan.variant()?;
    plan.document_part()?;
    plan.themes()?;
    plan.comments()?;
    plan.masters()?;
    plan.parts_for_pages()?;
    plan.pages()?;
    plan.diacraft()?;
    plan.properties()?;
    // After every step that may have edited the file, so that it knows whether the save writes the file again (`diacraft.rs`).
    plan.drop_old_declaration()?;
    plan.finish(&limits)
}

// ------------------------------------------------------------------------------------------------------------------------ the plan

enum Body {
    New(Vec<u8>),
    /// A part written from bytes the model holds (a part of DiaCraft's own): shared with the model, not copied.
    Shared(std::sync::Arc<[u8]>),
    Gone,
}

/// The macro project of a package: the relationships to it (their source and `Id`) and the parts that are VBA.
#[derive(Default)]
struct MacroProject {
    rows: Vec<(String, String)>,
    parts: Vec<String>,
    /// The folded names of `parts`, so that a part is added once in constant time.
    seen: std::collections::HashSet<String>,
}

impl MacroProject {
    fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.parts.is_empty()
    }

    /// Adds a part once, whatever the case it is named in.
    fn add(&mut self, part: String) {
        if self.seen.insert(fold(&part)) {
            self.parts.push(part);
        }
    }
}

/// A walk up the numbers of the names `{dir}{stem}{n}{ext}`, kept between the calls of [`Plan::next_name`] so that a part made after another does
/// not start from the first number again.
struct NameWalk {
    dir: String,
    stem: &'static str,
    ext: &'static str,
    n: u32,
}

impl NameWalk {
    fn new(dir: &str, stem: &'static str, ext: &'static str) -> NameWalk {
        NameWalk { dir: dir.to_owned(), stem, ext, n: 0 }
    }
}

struct Plan<'a> {
    pkg: Package,
    base: &'a Document,
    doc: &'a Document,
    opts: &'a WriteOptions,
    new_file: bool,
    /// The parts, in the order they are written: the original's, in the order of its central directory, then the new ones in the order they are made, the
    /// relationships parts last (`finish`, by [`Plan::source_place`]). By name as present, with the slash. `[Content_Types].xml` and `_rels/.rels` are
    /// written before all of these.
    order: Vec<String>,
    present: HashMap<String, usize>,
    /// Names given to parts that are not written yet, so that a second new part does not take the same name.
    reserved: std::collections::HashSet<String>,
    /// What replaces a part, by folded name. A part not here is the original's.
    bodies: HashMap<String, Body>,
    /// The relationships parts that were opened to edit, by the folded name of their source.
    rels: HashMap<String, RelsTree>,
    types: TypesTree,
    root_rels: Vec<RelRow>,
    document_part: String,
    document_rels: Vec<RelRow>,
    pages_index: Option<String>,
    masters_index: Option<String>,
    base_page_parts: Vec<Option<String>>,
    base_master_parts: Vec<Option<String>>,
    /// The part of each master of the new model, by `ID`.
    master_part: HashMap<u32, String>,
    page_part: HashMap<u32, String>,
    removed_pages: Vec<u32>,
    removed_masters: Vec<u32>,
    warnings: Vec<String>,
    rewritten: Vec<String>,
    added: Vec<String>,
    removed: Vec<String>,
    edited: bool,
    /// The file was a stencil and is written as another kind: a page with no part gets one.
    leaving_stencil: bool,
    /// The parts of the macro project the caller chose to leave out.
    macros_dropped: Vec<String>,
    /// What the file carries of the other format's data.
    carried_report: Carried,
    /// The parts of the other format this save wrote as embeddings (`Written::natively`).
    natively_parts: Vec<std::sync::Arc<str>>,
    /// This save wrote the model's DiaCraft data into the file (or took it out): the baseline it returns is then the model's.
    diacraft_written: bool,
    /// The parts of DiaCraft's own that this save wrote from bytes of the model, by name: the check of the file made shares them with the model.
    carried_blobs: Vec<(String, std::sync::Arc<[u8]>)>,
    /// The Snap & Glue words were left as the file has them (the model and the writer would not have meant the same element): the baseline the save returns
    /// holds the file's words, so that the next save tries again and says so again.
    words_left: bool,
    /// The most threads the save may use: 1 makes none.
    threads: usize,
    /// The parts the rows of a page part this save makes reach (`page_rows.rs`), folded: a part taken out with another is not taken out when one of them leads to it.
    keep: std::collections::HashSet<String>,
    /// What the rows of the page parts this save makes are found from (`page_rows.rs`).
    new_rows: page_rows::SpliceRows,
}

fn rows_of(rels: &diacraft_opc::Rels) -> Vec<RelRow> {
    rels.items
        .iter()
        .map(|r| RelRow {
            id: r.id.clone(),
            ty: r.ty.clone(),
            target: r.target.clone().unwrap_or_else(|| r.target_raw.clone()),
            external: r.mode == diacraft_opc::TargetMode::External,
        })
        .collect()
}

/// The first internal relationship of `ty`: its target part.
fn first_target(rels: &diacraft_opc::Rels, ty: &str) -> Option<String> {
    rels.items.iter().find(|r| r.ty == ty && r.target.is_some()).and_then(|r| r.target.clone())
}

impl<'a> Plan<'a> {
    fn new(pkg: Package, base: &'a Document, doc: &'a Document, opts: &'a WriteOptions, new_file: bool) -> R<Plan<'a>> {
        let order: Vec<String> = pkg.parts().map(str::to_owned).collect();
        let present = order.iter().enumerate().map(|(i, n)| (fold(n), i)).collect();
        // The content types are the file's own stream: every row is carried, and when none changes the bytes are written as they were.
        let types = TypesTree::read(pkg.content_types_bytes())?;
        Ok(Plan {
            pkg,
            base,
            doc,
            opts,
            new_file,
            order,
            present,
            reserved: std::collections::HashSet::new(),
            bodies: HashMap::new(),
            rels: HashMap::new(),
            types,
            root_rels: Vec::new(),
            document_part: String::new(),
            document_rels: Vec::new(),
            pages_index: None,
            masters_index: None,
            base_page_parts: Vec::new(),
            base_master_parts: Vec::new(),
            master_part: HashMap::new(),
            page_part: HashMap::new(),
            removed_pages: Vec::new(),
            removed_masters: Vec::new(),
            warnings: Vec::new(),
            rewritten: Vec::new(),
            added: Vec::new(),
            removed: Vec::new(),
            edited: new_file,
            leaving_stencil: false,
            macros_dropped: Vec::new(),
            carried_report: Carried::default(),
            natively_parts: Vec::new(),
            diacraft_written: false,
            carried_blobs: Vec::new(),
            words_left: false,
            threads: 1,
            keep: std::collections::HashSet::new(),
            new_rows: page_rows::SpliceRows::default(),
        })
    }

    // ------------------------------------------------------------------------------------------------------------- parts

    fn alive(&self, name: &str) -> bool {
        let key = fold(name);
        self.present.contains_key(&key) && !matches!(self.bodies.get(&key), Some(Body::Gone))
    }

    /// Where the relationships of the source `key` (a folded name, `/` for the package) go among the relationships parts a save writes: the package's
    /// own first, then those of the parts in the order of `order` (so those of `page2.xml` come before those of `page10.xml`, whatever their names),
    /// then those of a source that is no part (a relationships part whose part the package does not have).
    fn source_place(&self, key: &str) -> (u8, usize) {
        if key == "/" {
            (0, 0)
        } else {
            match self.present.get(key) {
                Some(&at) => (1, at),
                None => (2, 0),
            }
        }
    }

    fn bytes_of(&self, name: &str) -> Option<&[u8]> {
        match self.bodies.get(&fold(name)) {
            Some(Body::New(bytes)) => Some(bytes),
            Some(Body::Shared(bytes)) => Some(bytes),
            Some(Body::Gone) => None,
            None => self.pkg.part(name),
        }
    }

    /// The name of the part `name` names as the file or this save spelt it, which is the name it is written under (a lookup folds ASCII case, and
    /// the case a part arrived with is kept): `name` itself for a part that is not here. `key` is `name` folded, which the caller has made.
    fn spelt(&self, key: &str, name: &str) -> String {
        self.present.get(key).and_then(|&at| self.order.get(at)).map_or_else(|| name.to_owned(), String::clone)
    }

    /// Puts `bytes` in the part `name`, new or not, and gives it the type `content_type` by an `Override` when one is given. A part that is here under another spelling
    /// keeps its own, in the lists of what the save did and in a row it is given.
    fn set_part(&mut self, name: &str, bytes: Vec<u8>, content_type: Option<&str>) {
        let key = fold(name);
        let name = self.spelt(&key, name);
        if self.present.contains_key(&key) {
            if matches!(self.bodies.get(&key), Some(Body::Gone)) {
                self.removed.retain(|n| fold(n) != key);
                self.added.push(name.clone());
            } else if !self.added.iter().any(|n| fold(n) == key) {
                self.rewritten.push(name.clone());
            }
        } else {
            self.present.insert(key.clone(), self.order.len());
            self.order.push(name.clone());
            self.added.push(name.clone());
        }
        self.bodies.insert(key, Body::New(bytes));
        if let Some(ct) = content_type {
            self.types.set_override(&name, ct);
        }
    }

    /// [`Plan::set_part`] for a part written from bytes the model holds: they are shared with the model and not copied.
    fn set_shared_part(&mut self, name: &str, bytes: std::sync::Arc<[u8]>, content_type: Option<&str>) {
        self.set_part(name, Vec::new(), content_type);
        self.bodies.insert(fold(name), Body::Shared(bytes));
    }

    /// Takes a part out, with its relationships part and its `Override`.
    fn remove_part(&mut self, name: &str) {
        if !self.alive(name) {
            return;
        }
        let key = fold(name);
        self.bodies.insert(key.clone(), Body::Gone);
        self.types.remove_override(name);
        self.rels.remove(&key);
        self.added.retain(|n| fold(n) != key);
        self.removed.push(self.spelt(&key, name));
        if !is_rels_part(name) {
            let own = rels_name(name);
            self.remove_part(&own);
        }
    }

    /// [`Plan::remove_part`] for many parts in one pass over the content types and the lists: the parts, their relationships parts and their
    /// `Override`s go. (One `remove_part` for each of tens of thousands of parts scans the content types once each.)
    fn remove_parts(&mut self, names: &[String]) {
        let mut gone: std::collections::HashSet<String> = std::collections::HashSet::new();
        crate::work::parts(names.len());
        for name in names {
            let own = rels_name(name);
            for target in [name.as_str(), own.as_str()] {
                if !self.alive(target) {
                    continue;
                }
                let key = fold(target);
                self.bodies.insert(key.clone(), Body::Gone);
                self.rels.remove(&key);
                self.removed.push(self.spelt(&key, target));
                gone.insert(key);
            }
        }
        self.types.remove_overrides(&gone);
        self.added.retain(|n| !gone.contains(&fold(n)));
    }

    /// The relationships of `source` (`/` for the package), opened for editing.
    fn rels_mut(&mut self, source: &str) -> R<&mut RelsTree> {
        let key = fold(source);
        if !self.rels.contains_key(&key) {
            let name = rels_name(source);
            let tree = match self.bytes_of(&name) {
                Some(bytes) => RelsTree::parse(&name, bytes)?,
                None => RelsTree::empty(&name),
            };
            self.rels.insert(key.clone(), tree);
        }
        self.rels.get_mut(&key).ok_or(WriteError::Mismatch("a relationships part was not kept"))
    }

    /// The next name for a new part of `walk`: `{dir}{stem}{n}{ext}` for the least `n` above the last one the walk gave that no part has and none
    /// has reserved. The walk goes up the numbers **once**, however many names it hands out (each number is tried once, so the tries are the parts
    /// made and the names in the way, and not that many for each part). `None` only if the numbers run out.
    fn next_name(&self, walk: &mut NameWalk) -> Option<String> {
        loop {
            walk.n = walk.n.checked_add(1)?;
            let name = format!("{}{}{}{}", walk.dir, walk.stem, walk.n, walk.ext);
            crate::work::name_probe();
            let key = fold(&name);
            if !self.present.contains_key(&key) && !self.reserved.contains(&key) {
                return Some(name);
            }
        }
    }

    /// Names for `count` new parts in `dir` (with its trailing slash), in one walk up the numbers: the `count` least `n` no part has and none
    /// has reserved. Fewer than `count` come back only if the numbers run out.
    fn fresh_names(&self, dir: &str, stem: &'static str, ext: &'static str, count: usize) -> Vec<String> {
        let mut walk = NameWalk::new(dir, stem, ext);
        let mut names = Vec::new();
        while names.len() < count {
            let Some(name) = self.next_name(&mut walk) else { break };
            names.push(name);
        }
        names
    }

    // ----------------------------------------------------------------------------------------------------------- layout

    fn layout(&mut self) -> R<()> {
        let original = |e| WriteError::Original(VsdxError::Package(e));
        let root_rels = self.pkg.rels("/").map_err(original)?;
        let Some(document) = first_target(&root_rels, &rel_type(VIS, "document")) else {
            return Err(WriteError::Original(VsdxError::NoDrawingParts("no relationship leads to a document part")));
        };
        if self.pkg.part(&document).is_none() {
            return Err(WriteError::Original(VsdxError::NoDrawingParts("the document relationship names a part the package does not have")));
        }
        let doc_rels = self.pkg.rels(&document).map_err(original)?;
        let exists = |p: Option<String>, pkg: &Package| p.filter(|n| pkg.part(n).is_some());
        self.pages_index = exists(first_target(&doc_rels, &rel_type(VIS, "pages")), &self.pkg);
        self.masters_index = exists(first_target(&doc_rels, &rel_type(VIS, "masters")), &self.pkg);
        self.base_page_parts = self.row_parts(
            self.pages_index.clone(),
            &self.base.pages.iter().map(|p| p.rel_id.as_deref().map(str::to_owned)).collect::<Vec<_>>(),
            "page",
        )?;
        self.base_master_parts = self.row_parts(
            self.masters_index.clone(),
            &self.base.masters.iter().map(|m| m.rel_id.as_deref().map(str::to_owned)).collect::<Vec<_>>(),
            "master",
        )?;
        self.root_rels = rows_of(&root_rels);
        self.document_rels = rows_of(&doc_rels);
        self.document_part = document;
        for (i, part) in self.base_master_parts.iter().enumerate() {
            if let (Some(part), Some(master)) = (part, self.base.masters.get(i)) {
                self.master_part.entry(master.id).or_insert_with(|| part.clone());
            }
        }
        Ok(())
    }

    /// The part each row of an index names, through the index's relationships: of the type that kind of row has, internal, and in the package.
    fn row_parts(&self, index: Option<String>, ids: &[Option<String>], word: &str) -> R<Vec<Option<String>>> {
        let Some(index) = index else { return Ok(vec![None; ids.len()]) };
        // An index whose relationships do not read, in a file that opened, is one the reader did not read either (its root was not the one it
        // should have, say): it has no rows in the model, and the save keeps it as bytes. A save that edits it is refused by `Mismatch` later.
        let Ok(rels) = self.pkg.rels(&index) else { return Ok(vec![None; ids.len()]) };
        let ty = rel_type(VIS, word);
        Ok(ids
            .iter()
            .map(|id| {
                let rel = rels.get(id.as_deref()?)?;
                (rel.ty == ty).then_some(())?;
                rel.target.clone().filter(|t| self.pkg.part(t).is_some())
            })
            .collect())
    }

    // ---------------------------------------------------------------------------------------------------------- variant

    /// The document part's type follows the variant asked for, and a macro project is never dropped, silently or at all, by a type
    /// that cannot hold one unless the caller chose to ([`WriteOptions::drop_macros`]), and then the file says so in words.
    ///
    /// Everything else a Save As between the six does is in the document part's `Override`. The two kinds that differ in what the file
    /// should hold are said: a stencil is used for its masters, and a stencil's pages have no parts, which a
    /// drawing or a template should, so a stencil written as one of those gets a part for each page that has none
    /// ([`Plan::parts_for_pages`]).
    fn variant(&mut self) -> R<()> {
        let current = self.pkg.content_type(&self.document_part).and_then(Variant::from_content_type);
        let Some(target) = self.opts.variant.or(current) else { return Ok(()) };
        let changing = Some(target) != current;
        if !target.macro_enabled {
            let found = self.macro_project()?;
            if !found.is_empty() {
                if self.opts.drop_macros {
                    self.drop_macros(found, target)?;
                } else if changing {
                    return Err(WriteError::MacroProject);
                }
            }
        }
        // A new drawing is the skeleton, a drawing, whatever it is written as.
        let from = if self.new_file { Some(Variant { kind: Kind::Drawing, macro_enabled: false }) } else { current };
        if target.kind == Kind::Stencil && !from.is_some_and(|v| v.kind == Kind::Stencil) {
            self.stencil_notes();
        }
        if !changing {
            return Ok(());
        }
        self.leaving_stencil = !self.new_file && current.is_some_and(|v| v.kind == Kind::Stencil) && target.kind != Kind::Stencil;
        let part = self.document_part.clone();
        self.types.set_override(&part, target.content_type());
        Ok(())
    }

    /// What a drawing or a template saved as a stencil should be told: its pages stay in the file, and a stencil with no masters has
    /// nothing to offer.
    fn stencil_notes(&mut self) {
        let with_shapes = self.doc.pages.iter().filter(|p| !p.shapes.is_empty()).count();
        if with_shapes > 0 {
            let (page, stays) = if with_shapes == 1 { ("page", "stays") } else { ("pages", "stay") };
            self.warnings.push(format!(
                "saved as a stencil: the {with_shapes} {page} with shapes {stays} in the file, but a stencil offers its masters to other drawings, not its pages"
            ));
        }
        if self.doc.masters.is_empty() {
            self.warnings.push("the stencil has no masters, so it offers no shapes to other drawings".to_owned());
        }
    }

    /// The macro project of the package: the relationships to it, from the package and from the document, the parts they reach, the signature
    /// parts the project's own relationships reach, and any other part whose content type is a VBA one (a part no relationship reaches is
    /// still VBA, and R18 lets only a macro-enabled type hold it).
    fn macro_project(&mut self) -> R<MacroProject> {
        let mut found = MacroProject::default();
        let sources: [(String, Vec<RelRow>); 2] =
            [("/".to_owned(), self.root_rels.clone()), (self.document_part.clone(), self.document_rels.clone())];
        for (source, rows) in &sources {
            for row in rows.iter().filter(|r| r.ty == VBA_PROJECT && !r.external) {
                found.rows.push((source.clone(), row.id.clone()));
                if let Some(part) = resolve(source, &row.target).filter(|p| self.alive(p)) {
                    found.add(part);
                }
            }
        }
        for project in found.parts.clone() {
            if !self.alive(&rels_name(&project)) {
                continue;
            }
            // A relationships part that does not read is not found out here: the project goes with it, and a signature part is found by its
            // content type below. (The save of a file that kept the project would never have read it.)
            let Ok(rels) = self.pkg.rels(&project) else { continue };
            for row in rels.items.iter().filter(|r| r.mode != diacraft_opc::TargetMode::External && r.ty.contains("/vbaProjectSignature")) {
                if let Some(part) = row.target.clone().filter(|p| self.alive(p)) {
                    found.add(part);
                }
            }
        }
        for name in &self.order {
            if self.alive(name) && !is_rels_part(name) && self.pkg.content_type(name).is_some_and(|t| t.starts_with(VBA_CONTENT_TYPE)) {
                found.add(name.clone());
            }
        }
        Ok(found)
    }

    /// Leaves the macro project out at the caller's choice: its relationships, its parts with their own relationships and types, and a warning in
    /// words that names the parts. The `Default` of `bin` stays, as every `Default` does.
    fn drop_macros(&mut self, found: MacroProject, target: Variant) -> R<()> {
        // Each source's rows go in one pass, and so do the parts: a package may hold tens of thousands of either.
        let mut sources: Vec<&str> = found.rows.iter().map(|(source, _)| source.as_str()).collect();
        sources.sort_unstable();
        sources.dedup();
        for source in sources {
            let ids: std::collections::HashSet<&str> = found.rows.iter().filter(|(s, _)| s == source).map(|(_, id)| id.as_str()).collect();
            self.rels_mut(source)?.remove_ids(&ids);
        }
        self.root_rels.retain(|r| r.ty != VBA_PROJECT);
        self.document_rels.retain(|r| r.ty != VBA_PROJECT);
        self.remove_parts(&found.parts);
        self.edited = true;
        // Only the names that are package structure are told (`part_label`: a part name can hold a person's words); the count says the rest.
        let n = found.parts.len();
        let shown: Vec<&str> = found.parts.iter().map(String::as_str).filter(|p| part_label("", p).as_ref() == *p).take(MACRO_NAMES_SHOWN).collect();
        let more = n.saturating_sub(shown.len());
        let names = match (shown.is_empty(), more > 0) {
            (_, false) => shown.join(", "),
            (true, true) => format!("{more} named by their owner"),
            (false, true) => format!("{} and {more} more", shown.join(", ")),
        };
        self.warnings.push(format!(
            "the macro project was left out of the new file, because a {} cannot hold one: {n} {} removed ({names})",
            target.kind_word(),
            if n == 1 { "part" } else { "parts" }
        ));
        self.macros_dropped = found.parts;
        Ok(())
    }

    /// A stencil written as a drawing or a template: a page row with no `Rel` gets a part, empty, with its relationship, so that no page of the
    /// new file is left without one (both schemas require the `Rel`, and only stencils are seen without it).
    fn parts_for_pages(&mut self) -> R<()> {
        if !self.leaving_stencil {
            return Ok(());
        }
        let Some(index) = self.pages_index.clone() else { return Ok(()) };
        let wanted: Vec<usize> = self
            .base
            .pages
            .iter()
            .enumerate()
            .filter(|(i, p)| p.rel_id.is_none() && self.base_page_parts.get(*i).is_some_and(Option::is_none))
            .map(|(i, _)| i)
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        self.edited = true;
        self.rels_mut(&index)?;
        // The names and the ids of all the new parts, found in one pass each over what is there (a page at a time would try the names from the
        // first and collect the ids in use again for each, which is the square of the pages: the 1,000 the model takes at most were 500,500 tries).
        let names = self.fresh_names(folder_of(&index), "page", ".xml", wanted.len());
        let ids = self.rels.get(&fold(&index)).ok_or(WriteError::Mismatch("a relationships part was not kept"))?.next_ids(wanted.len());
        if names.len() < wanted.len() || ids.len() < wanted.len() {
            return Err(WriteError::Limit("no part name or relationship id was left for a page"));
        }
        let mut made: Vec<(usize, String, String)> = Vec::new();
        for ((place, part), id) in wanted.into_iter().zip(names).zip(ids) {
            self.reserved.insert(fold(&part));
            let rels = self.rels.get_mut(&fold(&index)).ok_or(WriteError::Mismatch("a relationships part was not kept"))?;
            rels.add(&id, &rel_type(VIS, "page"), &relative_target(&index, &part));
            made.push((place, id, part));
        }
        let bytes = self.bytes_of(&index).ok_or(WriteError::Mismatch("the pages part is not in the file"))?;
        let held_bytes = bytes.len() as u64;
        let mut tree = Tree::parse(&index, bytes)?;
        let _room = build::PartRoom::enter(build::tally(&tree.root.children).0, held_bytes);
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Pages") {
            return Err(WriteError::Mismatch("the pages part has another root"));
        }
        let rows: Vec<(usize, String)> = made.iter().map(|(place, id, _)| (*place, id.clone())).collect();
        patch::add_page_rels(&mut tree.root, &sc, &rows)?;
        let out = tree.bytes(&index)?;
        self.set_part(&index, out, None);
        for (place, _, part) in made {
            let content =
                new_content("PageContents", &ShapeList::default(), &Connects::default(), &diacraft_doc::Ext::<diacraft_doc::Kept>::default())?;
            self.set_part(&part, content.bytes(&part)?, Some(CT_PAGE));
            if let Some(slot) = self.base_page_parts.get_mut(place) {
                *slot = Some(part.clone());
            }
            if let Some(page) = self.base.pages.get(place) {
                self.page_part.insert(page.id, part);
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------------------------------------------- document part

    /// The colour table, the font table, the styles and the document sheet of `visio/document.xml`.
    fn document_part(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        let styles = base.styles != doc.styles;
        let colors = base.colors != doc.colors;
        let fonts = base.fonts != doc.fonts;
        let sheet = base.sheet != doc.sheet;
        // The unknown elements of the document, other than its Snap & Glue words, are written as the model has them (a change to them is the model's: a drawing another program
        // changed); the words alone are patched where they sit.
        let unknown = !self.new_file && !base.same_but_snap_words(doc);
        let words = !unknown && base.snap_words() != doc.snap_words();
        if !(styles || colors || fonts || sheet || words || unknown) {
            return Ok(());
        }
        let name = self.document_part.clone();
        let bytes = self.bytes_of(&name).ok_or(WriteError::Mismatch("the document part is not in the file"))?;
        let held_bytes = bytes.len() as u64;
        let mut tree = Tree::parse(&name, bytes)?;
        let _room = build::PartRoom::enter(build::tally(&tree.root.children).0, held_bytes);
        // When only the words changed, the file's bytes are kept but for the text of the words (`settings::patch_bytes`).
        let original = (words && !(styles || colors || fonts || sheet)).then(|| bytes.to_vec());
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "VisioDocument") {
            return Err(WriteError::Mismatch("the document part has another root"));
        }
        let before = original.as_ref().and_then(|_| settings::snapshot(&tree.root, &sc));
        let prefix = tree.root.prefix().to_owned();
        let find = |root: &El, local: &str| root.children.iter().position(|n| matches!(n, Node::El(e) if sc.enter(e).is(e, local)));
        // The table that comes after: where a new one goes.
        let insert_before = |root: &El, locals: &[&str]| {
            root.children.iter().position(|n| matches!(n, Node::El(e) if locals.iter().any(|l| sc.enter(e).is(e, l)))).unwrap_or(root.children.len())
        };
        let replace = |root: &mut El, local: &str, later: &[&str], mut made: El| {
            build::with_prefix(&mut made, &prefix);
            match find(root, local) {
                Some(at) => {
                    if let Some(slot) = root.children.get_mut(at) {
                        *slot = Node::El(made);
                    }
                }
                None => {
                    let at = insert_before(root, later);
                    root.children.insert(at, Node::El(made));
                }
            }
        };
        if words && let settings::Patched::Left(why) = settings::patch_words(&mut tree.root, &sc, &prefix, &doc.snap_words())? {
            self.warnings.push(why.to_owned());
            self.words_left = true;
            // Nothing else changed: the part is as it was.
            if !(styles || colors || fonts || sheet) {
                return Ok(());
            }
        }
        if unknown {
            replace_unknown(&mut tree.root, &sc, doc)?;
        }

        if colors {
            replace(&mut tree.root, "Colors", &["FaceNames", "StyleSheets", "DocumentSheet"], build::colors_el_ext(&doc.colors, &doc.ext)?);
        }
        if fonts {
            replace(&mut tree.root, "FaceNames", &["StyleSheets", "DocumentSheet"], build::fonts_el_ext(&doc.fonts, &doc.ext)?);
        }
        if styles {
            let b: Vec<_> = base.styles.iter().map(|s| &**s).collect();
            let n: Vec<_> = doc.styles.iter().map(|s| &**s).collect();
            match find(&tree.root, "StyleSheets") {
                Some(at) => {
                    if let Some(Node::El(el)) = tree.root.children.get_mut(at) {
                        let esc = sc.enter(el);
                        patch::patch_styles(el, &esc, &b, &n)?;
                    }
                }
                None => {
                    if !b.is_empty() {
                        return Err(WriteError::Mismatch("the document part lacks the styles the model was read from"));
                    }
                    let styles: Vec<Node> = n.iter().map(|s| build::style_el(s).map(Node::El)).collect::<R<Vec<Node>>>()?;
                    let mut el = build::element("StyleSheets", Vec::new(), styles);
                    build::with_prefix(&mut el, &prefix);
                    let at = insert_before(&tree.root, &["DocumentSheet"]);
                    tree.root.children.insert(at, Node::El(el));
                }
            }
        }
        if sheet {
            patch::patch_sheet_only(&mut tree.root, &sc, "DocumentSheet", &[], &base.sheet, &doc.sheet)?;
        }
        let kept = match (&original, &before) {
            (Some(bytes), Some(before)) => settings::patch_bytes(bytes, before, &sc, &doc.snap_words(), &tree.root),
            _ => None,
        };
        let out = match kept {
            Some(out) => out,
            None => tree.bytes(&name)?,
        };
        self.set_part(&name, out, None);
        self.edited = true;
        Ok(())
    }

    // --------------------------------------------------------------------------------------------------------- masters

    fn masters(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        let b: Vec<&Master> = base.masters.iter().map(|m| &**m).collect();
        let n: Vec<&Master> = doc.masters.iter().map(|m| &**m).collect();
        let (matched, used) = patch::match_items(&b, &n, &|m: &Master| Some(m.id));
        // The content of each master part that is there.
        let mut index_changed = b.len() != n.len();
        let mut jobs: Vec<Rewrite<'a>> = Vec::new();
        let mut refused: Option<WriteError> = None;
        for (j, m) in n.iter().enumerate() {
            let Some(i) = matched.get(j).copied().flatten() else {
                index_changed = true;
                continue;
            };
            let Some(old) = b.get(i) else { continue };
            index_changed |= !master_row_same(old, m);
            if old.shapes == m.shapes && old.connects == m.connects {
                continue;
            }
            // (A part patched is an edit when its patch changes its bytes: `rewrite_all`. A master that has no part of its own is one through its index.)
            match self.base_master_parts.get(i).cloned().flatten() {
                Some(part) => jobs.push(Rewrite {
                    part,
                    root: "MasterContents",
                    shapes: (&old.shapes, &m.shapes),
                    connects: (&old.connects, &m.connects),
                    rels_made: false,
                }),
                None if old.rel_id.is_none() => index_changed = true,
                None => {
                    refused = Some(WriteError::Unsupported("an edit to a master whose part is not in the file"));
                    break;
                }
            }
        }
        self.rewrite_all(jobs)?;
        if let Some(refusal) = refused {
            return Err(refusal);
        }
        for (i, was_used) in used.iter().enumerate() {
            if !was_used {
                index_changed = true;
                if let Some(old) = b.get(i) {
                    self.removed_masters.push(old.id);
                }
            }
        }
        if !index_changed {
            return Ok(());
        }
        self.edited = true;
        let index = match self.masters_index.clone() {
            Some(index) => index,
            None if n.is_empty() => return Ok(()),
            None => self.create_index("masters", "masters.xml", "Masters", CT_MASTERS)?,
        };
        self.rels_mut(&index)?;
        // The masters taken out go in one pass: their relationships, their parts, their types and what only they reached.
        let gone: Vec<(Option<String>, Option<String>)> = used
            .iter()
            .enumerate()
            .filter(|(_, was_used)| !**was_used)
            .filter_map(|(i, _)| b.get(i).map(|old| (old.rel_id.as_deref().map(str::to_owned), self.base_master_parts.get(i).cloned().flatten())))
            .collect();
        self.remove_items(&index, &gone)?;
        let bytes = self.bytes_of(&index).ok_or(WriteError::Mismatch("the masters part is not in the file"))?;
        let held_bytes = bytes.len() as u64;
        let mut tree = Tree::parse(&index, bytes)?;
        let _room = build::PartRoom::enter(build::tally(&tree.root.children).0, held_bytes);
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Masters") {
            return Err(WriteError::Mismatch("the masters part has another root"));
        }
        let mut pending: Vec<(u32, String)> = Vec::new();
        // The names and the ids are found by walks that go up the numbers once across all the masters made: a master at a time would try the names
        // from the first and collect the ids in use again for each, which is the square of the masters.
        let mut names = NameWalk::new(folder_of(&index), "master", ".xml");
        let mut ids = self.rels_mut(&index)?.id_walk();
        let mut out_of_numbers = false;
        {
            let mut alloc = |m: &Master, is_new: bool| -> Option<String> {
                if !is_new && m.shapes.is_empty() && m.connects.is_empty() {
                    return None;
                }
                let (Some(part), Some(id)) = (self.next_name(&mut names), ids.next()) else {
                    out_of_numbers = true;
                    return None;
                };
                self.reserved.insert(fold(&part));
                let rels = self.rels.get_mut(&fold(&index))?;
                rels.add(&id, &rel_type(VIS, "master"), &relative_target(&index, &part));
                self.master_part.insert(m.id, part.clone());
                pending.push((m.id, part));
                Some(id)
            };
            patch::patch_masters(&mut tree.root, &sc, &b, &n, &mut alloc)?;
        }
        if out_of_numbers {
            return Err(WriteError::Limit("no part name or relationship id was left for a master"));
        }
        let out = tree.bytes(&index)?;
        self.set_part(&index, out, None);
        for (id, part) in pending {
            let Some(m) = doc.master(id) else { continue };
            let content = new_content("MasterContents", &m.shapes, &m.connects, &m.ext)?;
            self.set_part(&part, content.bytes(&part)?, Some(CT_MASTER));
        }
        Ok(())
    }

    /// Makes an index part (`Pages` or `Masters`) that the file lacks: an empty one, the document's relationship to it, its type.
    fn create_index(&mut self, folder: &str, file: &str, root: &str, content_type: &str) -> R<String> {
        let name = format!("{}{folder}/{file}", folder_of(&self.document_part));
        let tree = Tree::new(skeleton::part_root(root, Vec::new()));
        let bytes = tree.bytes(&name)?;
        self.set_part(&name, bytes, Some(content_type));
        let document = self.document_part.clone();
        let rels = self.rels_mut(&document)?;
        let id = rels.next_id();
        rels.add(&id, &rel_type(VIS, folder), &relative_target(&document, &name));
        Ok(name)
    }

    /// Items of an index taken out, `(relationship Id, part)` each: their relationships, their parts, and what only they reached. **In one pass over each
    /// list**, however many items there are (a relationship removed, a part removed and a content type scanned for each of 1,000 pages is a million
    /// steps): the relationships by `Id` in one pass of the index's relationships, the parts and their types in one pass of the stream, and what the
    /// parts reached, a picture or an object, in a second pass for the ones nothing else reaches.
    fn remove_items(&mut self, index: &str, items: &[(Option<String>, Option<String>)]) -> R<()> {
        let ids: std::collections::HashSet<&str> = items.iter().filter_map(|(id, _)| id.as_deref()).collect();
        if !ids.is_empty() {
            self.rels_mut(index)?.remove_ids(&ids);
        }
        // **A part that a row of the index still leads to stays**: two pages or two masters may name one part, and the one taken out takes its row and
        // leaves the part to the others, with what it reaches. The rows of the index are those left once the rows of the items taken out have gone, in one pass of them.
        let still_named: std::collections::HashSet<String> =
            self.rels_mut(index)?.rows().iter().filter(|r| !r.external).filter_map(|r| resolve(index, &r.target)).map(|t| fold(&t)).collect();
        // What their relationships reach, by kind: a picture or an embedded object may be dropped with them, but only when nothing else reaches it
        // A master, a page and every other target stay.
        let mut parts: Vec<String> = Vec::new();
        let mut candidates: Vec<String> = Vec::new();
        for part in items.iter().filter_map(|(_, part)| part.as_deref()).filter(|part| !still_named.contains(&fold(part))) {
            if self.alive(&rels_name(part)) {
                let tree = self.rels_mut(part)?;
                candidates.extend(
                    tree.rows()
                        .iter()
                        .filter(|r| !r.external && REMOVABLE_TARGETS.iter().any(|k| r.ty.ends_with(k)))
                        .filter_map(|r| resolve(part, &r.target)),
                );
            }
            parts.push(part.to_owned());
        }
        if parts.is_empty() {
            return Ok(());
        }
        self.remove_parts(&parts);
        if candidates.is_empty() {
            return Ok(());
        }
        let reached = self.all_targets()?;
        let mut seen = std::collections::HashSet::new();
        let unreached: Vec<String> =
            candidates.into_iter().filter(|t| !reached.contains(&fold(t)) && !self.keep.contains(&fold(t)) && seen.insert(fold(t))).collect();
        self.remove_parts(&unreached);
        Ok(())
    }

    /// Every part a relationship **of another part than `except`** reaches now, by folded name: what a part of DiaCraft's own may not be, if it is to be taken out.
    fn targets_elsewhere(&mut self, except: &str) -> R<std::collections::HashSet<String>> {
        let own = fold(except);
        let names: Vec<String> = self.order.iter().filter(|n| self.alive(n) && is_rels_part(n)).cloned().collect();
        let mut out = std::collections::HashSet::new();
        for rels in names {
            let Some(source) = rels_source(&rels) else { continue };
            if fold(&source) == own {
                continue;
            }
            let rows = match self.rels.get(&fold(&source)) {
                Some(tree) => tree.rows(),
                None => match self.bytes_of(&rels) {
                    Some(bytes) => RelsTree::parse(&rels, bytes)?.rows(),
                    None => continue,
                },
            };
            out.extend(rows.iter().filter(|r| !r.external).filter_map(|r| resolve(&source, &r.target)).map(|t| fold(&t)));
        }
        Ok(out)
    }

    /// Every part some relationship reaches now, by folded name.
    fn all_targets(&mut self) -> R<std::collections::HashSet<String>> {
        let names: Vec<String> = self.order.iter().filter(|n| self.alive(n) && is_rels_part(n)).cloned().collect();
        let mut out = std::collections::HashSet::new();
        for rels in names {
            let Some(source) = rels_source(&rels) else { continue };
            let rows = match self.rels.get(&fold(&source)) {
                Some(tree) => tree.rows(),
                None => match self.bytes_of(&rels) {
                    Some(bytes) => RelsTree::parse(&rels, bytes)?.rows(),
                    None => continue,
                },
            };
            out.extend(rows.iter().filter(|r| !r.external).filter_map(|r| resolve(&source, &r.target)).map(|t| fold(&t)));
        }
        Ok(out)
    }

    // ----------------------------------------------------------------------------------------------------------- pages

    fn pages(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        let b: Vec<&Page> = base.pages.iter().map(|p| &**p).collect();
        let n: Vec<&Page> = doc.pages.iter().map(|p| &**p).collect();
        let (matched, used) = patch::match_items(&b, &n, &|p: &Page| Some(p.id));
        // **A page that went to another place among the pages the file has is a change of the index** (R52 when a Save As puts the background pages first): the rows of the pages
        // that stay are in the order of the model, and an index whose rows are all the same rows in another order is not the file's.
        let mut index_changed = b.len() != n.len() || pages_moved(&matched);
        // Pages whose part is rewritten, with the masters their shapes used before and use now.
        let mut touched: Vec<(String, Vec<u32>, Vec<u32>, Background)> = Vec::new();
        // The parts to patch are found first and patched together (on threads when there are some): a refusal found on the way is held until the patches before it are made, since the first error of
        // the pages in their order is the one that is given.
        // **A picture new to a page part the file has gets a row of the part and a part** (`page_rows`): the shapes each patched page part is given, with those rows named.
        let mut dressed: Vec<Option<ShapeList>> = Vec::with_capacity(n.len());
        for (j, p) in n.iter().enumerate() {
            let found = matched.get(j).copied().flatten().and_then(|i| Some((b.get(i)?, self.base_page_parts.get(i).cloned().flatten()?)));
            dressed.push(match found {
                Some((old, part)) if old.shapes != p.shapes => self.dress_new_pictures(&part, &old.shapes, &p.shapes)?,
                _ => None,
            });
        }
        let mut jobs: Vec<Rewrite<'_>> = Vec::new();
        let mut refused: Option<WriteError> = None;
        for (j, p) in n.iter().enumerate() {
            let Some(i) = matched.get(j).copied().flatten() else {
                index_changed = true;
                continue;
            };
            let Some(old) = b.get(i) else { continue };
            index_changed |= !page_row_same(old, p);
            let part = self.base_page_parts.get(i).cloned().flatten();
            if let Some(part) = &part {
                self.page_part.insert(p.id, part.clone());
            }
            let content = old.shapes != p.shapes || old.connects != p.connects;
            let background = old.back_page != p.back_page;
            if !content && !background {
                continue;
            }
            // **A part patched is an edit when its patch changes its bytes** (`rewrite_all`): a page whose shapes differ from the file's model only where the two say one thing (a
            // picture the drawing holds by its bytes, which the file names by a row: `patch::shows_the_same`) is patched to the bytes it has, and a save that is no edit stamps no time and no name.
            if background {
                self.edited = true;
            }
            match part {
                Some(part) => {
                    if self.shared_part(&part) {
                        refused = Some(WriteError::Unsupported("an edit to a page whose part another page shares"));
                        break;
                    }
                    if content {
                        let given = dressed.get(j).and_then(Option::as_ref);
                        jobs.push(Rewrite {
                            part: part.clone(),
                            root: "PageContents",
                            shapes: (&old.shapes, given.unwrap_or(&p.shapes)),
                            connects: (&old.connects, &p.connects),
                            rels_made: given.is_some(),
                        });
                    }
                    let back = background.then_some((old.back_page, p.back_page));
                    touched.push((part, masters_used(&old.shapes).into_iter().collect(), masters_used(&p.shapes).into_iter().collect(), back));
                }
                None if old.rel_id.is_none() => index_changed = true,
                None => {
                    refused = Some(WriteError::Unsupported("an edit to a page whose part is not in the file"));
                    break;
                }
            }
        }
        self.rewrite_all(jobs)?;
        if let Some(refusal) = refused {
            return Err(refusal);
        }
        for (i, was_used) in used.iter().enumerate() {
            if !was_used {
                index_changed = true;
                if let Some(old) = b.get(i) {
                    self.removed_pages.push(old.id);
                }
            }
        }
        if index_changed {
            self.edited = true;
            self.pages_index_edit(&b, &n, &used)?;
        }
        for (part, before, now, back) in touched {
            self.sync_page_rels(&part, &before, &now, back)?;
        }
        self.sync_windows()
    }

    /// Whether more than one page of the baseline model names this part.
    fn shared_part(&self, part: &str) -> bool {
        self.base_page_parts.iter().flatten().filter(|p| fold(p) == fold(part)).count() > 1
    }

    /// The pages index: rows added, taken out and changed, and the parts of the pages added.
    fn pages_index_edit(&mut self, b: &[&Page], n: &[&Page], used: &[bool]) -> R<()> {
        let doc = self.doc;
        let index = match self.pages_index.clone() {
            Some(index) => index,
            None if n.is_empty() => return Ok(()),
            None => {
                let name = self.create_index("pages", "pages.xml", "Pages", CT_PAGES)?;
                self.pages_index = Some(name.clone());
                name
            }
        };
        self.rels_mut(&index)?;
        // **The rows of the pages that get a part of their own**: a row for every picture and object their shapes name, found before any page is taken out, so that a
        // copy whose source goes in this save keeps what it shows (`page_rows.rs`).
        let carried = self.carry_new_pages(n)?;
        // The pages taken out go in one pass: their relationships, their parts, their types and what only they reached.
        let gone: Vec<(Option<String>, Option<String>)> = used
            .iter()
            .enumerate()
            .filter(|(_, was_used)| !**was_used)
            .filter_map(|(i, _)| b.get(i).map(|old| (old.rel_id.as_deref().map(str::to_owned), self.base_page_parts.get(i).cloned().flatten())))
            .collect();
        self.remove_items(&index, &gone)?;
        let bytes = self.bytes_of(&index).ok_or(WriteError::Mismatch("the pages part is not in the file"))?;
        let held_bytes = bytes.len() as u64;
        let mut tree = Tree::parse(&index, bytes)?;
        let _room = build::PartRoom::enter(build::tally(&tree.root.children).0, held_bytes);
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Pages") {
            return Err(WriteError::Mismatch("the pages part has another root"));
        }
        let mut pending: Vec<(u32, String)> = Vec::new();
        // One walk up the names and one collection of the ids in use for all the pages made, as for masters.
        let mut names = NameWalk::new(folder_of(&index), "page", ".xml");
        let mut ids = self.rels_mut(&index)?.id_walk();
        let mut out_of_numbers = false;
        {
            let mut alloc = |p: &Page, is_new: bool| -> Option<String> {
                if !is_new && p.shapes.is_empty() && p.connects.is_empty() {
                    return None;
                }
                let (Some(part), Some(id)) = (self.next_name(&mut names), ids.next()) else {
                    out_of_numbers = true;
                    return None;
                };
                self.reserved.insert(fold(&part));
                let rels = self.rels.get_mut(&fold(&index))?;
                rels.add(&id, &rel_type(VIS, "page"), &relative_target(&index, &part));
                self.page_part.insert(p.id, part.clone());
                pending.push((p.id, part));
                Some(id)
            };
            patch::patch_pages(&mut tree.root, &sc, b, n, &mut alloc)?;
        }
        if out_of_numbers {
            return Err(WriteError::Limit("no part name or relationship id was left for a page"));
        }
        let out = tree.bytes(&index)?;
        self.set_part(&index, out, None);
        for (id, part) in &pending {
            let Some(page) = doc.page(*id) else { continue };
            let made = carried.get(id);
            let shapes = made.and_then(|c| c.shapes.as_ref()).unwrap_or(&page.shapes);
            let content = new_content("PageContents", shapes, &page.connects, &page.ext)?;
            self.set_part(part, content.bytes(part)?, Some(CT_PAGE));
            // The rows the shapes name come first, under the ids they name: the rows to the masters and the background are given ids of their own after them.
            if let Some(made) = made {
                self.write_carried(part, made)?;
            }
        }
        for (id, part) in &pending {
            let Some(page) = doc.page(*id) else { continue };
            let used: Vec<u32> = masters_used(&page.shapes).into_iter().collect();
            self.sync_page_rels(part, &[], &used, Some((None, page.back_page)))?;
        }
        Ok(())
    }

    /// The relationships of a page part that the model implies and the XML does not name:
    /// one to each master the page's shapes use, added for a master used now and not before and taken out for one used before and not now; and the
    /// one to the background page.
    fn sync_page_rels(&mut self, part: &str, before: &[u32], now: &[u32], back: Background) -> R<()> {
        let master_ty = rel_type(VIS, "master");
        let page_ty = rel_type(VIS, "page");
        let mut wanted: Vec<(String, String, bool)> = Vec::new();
        for id in now {
            if !before.contains(id)
                && let Some(target) = self.master_part.get(id)
            {
                wanted.push((master_ty.clone(), target.clone(), true));
            }
        }
        for id in before {
            if !now.contains(id)
                && let Some(target) = self.master_part.get(id)
            {
                wanted.push((master_ty.clone(), target.clone(), false));
            }
        }
        if let Some((old, new)) = back {
            for (id, add) in [(old, false), (new, true)] {
                if let Some(target) = id.and_then(|id| self.page_part.get(&id)) {
                    wanted.push((page_ty.clone(), target.clone(), add));
                }
            }
        }
        if wanted.is_empty() {
            return Ok(());
        }
        let tree = self.rels_mut(part)?;
        for (ty, target, add) in wanted {
            let rows = tree.rows();
            let points = |r: &RelRow| r.ty == ty && !r.external && resolve(part, &r.target).is_some_and(|t| fold(&t) == fold(&target));
            if add {
                if !rows.iter().any(points) {
                    let id = tree.next_id();
                    tree.add(&id, &ty, &relative_target(part, &target));
                }
            } else {
                for row in rows.iter().filter(|r| points(r)) {
                    tree.remove(&row.id);
                }
            }
        }
        Ok(())
    }

    /// The windows part. A `Window` row that names a page or a master since taken out is dropped, and **the window settings the model holds
    /// are written into it**: each flag of the model's drawing window that the file's does not hold already is set where the file holds it,
    /// and nothing else of the part changes ([`diacraft_doc::WindowFlags::written`]; a part whose drawing window was dropped gains one, so the settings stay, and a part that held
    /// none, and so no settings, gains the model's drawing window). A drawing whose
    /// settings are those it was read with, and that was not cut, writes the part as it was: nothing is parsed. A model that holds no windows part sets nothing. A file
    /// with no windows part is given one, the model's, when the model holds one. A part that is not a `Windows` the writer can read keeps its bytes, and the save says so.
    fn sync_windows(&mut self) -> R<()> {
        let dropping = !(self.removed_pages.is_empty() && self.removed_masters.is_empty());
        if dropping {
            self.warnings
                .push("a page or master was removed; comments, published-page lists and data recordsets that name it are not edited".to_owned());
        }
        // The flags the model holds a value for. They are written where the model's differ from the file's as it was read, or where a cut may have taken the window that held them.
        let wanted = self.doc.window_flags().filter(|w| !w.is_empty());
        let setting = wanted.filter(|w| dropping || self.base.window_flags() != Some(*w));
        if !dropping && setting.is_none() {
            return Ok(());
        }
        let found =
            self.document_rels.iter().find(|r| r.ty == rel_type(VIS, "windows") && !r.external).and_then(|r| resolve(&self.document_part, &r.target));
        let Some(part) = found else { return self.add_windows(setting.is_some()) };
        let Some(bytes) = self.bytes_of(&part) else { return Ok(()) };
        let mut tree = match Tree::parse(&part, bytes) {
            Ok(tree) => tree,
            Err(e) if dropping => return Err(e),
            Err(_) => {
                self.warnings.push(
                    "the window's settings (grid, rulers, guides, connection points) were not saved: the file's windows part is not XML".to_owned(),
                );
                return Ok(());
            }
        };
        let mut changed = false;
        let (pages, masters) = (self.removed_pages.clone(), self.removed_masters.clone());
        let names = |el: &El, attr: &str, gone: &[u32]| el.attr(attr).and_then(|v| v.parse::<u32>().ok()).is_some_and(|id| gone.contains(&id));
        let cut = |w: &El| w.is("Window") && (names(w, "Page", &pages) || names(w, "Master", &masters));
        if dropping {
            let before = tree.root.children.len();
            tree.root.children.retain(|n| !matches!(n, Node::El(w) if cut(w)));
            changed = tree.root.children.len() != before;
        }
        if let Some(flags) = setting {
            if !tree.root.is("Windows") {
                self.warnings
                    .push("the window's settings (grid, rulers, guides, connection points) were not saved: the file's windows part has another root than Windows".to_owned());
            } else {
                let mut root = diacraft_sheetxml::passthrough::raw_element(&tree.root);
                // **A part that holds no drawing window** (it held no settings) is given the model's, the window of a new drawing's part that `window.settings.set` put in
                // So that the file holds the window the model holds; but not one that names a page or a master taken out: its flags go into a window of their own.
                let model = self.doc.vsdx.as_ref().and_then(|v| v.windows.as_deref());
                let given = model
                    .and_then(|model| diacraft_doc::with_drawing_window(&root, model))
                    .filter(|given| !diacraft_sheetxml::passthrough::to_tree(given).children.iter().any(|n| matches!(n, Node::El(w) if cut(w))));
                let gained = given.is_some();
                if let Some(given) = given {
                    root = given;
                }
                let made = flags.written(&root);
                let set = made.is_some();
                if let Some(made) = made {
                    root = made;
                }
                if gained || set {
                    tree.root = diacraft_sheetxml::passthrough::to_tree(&root);
                    changed = true;
                    self.edited = true;
                }
            }
        }
        if changed {
            let out = tree.bytes(&part)?;
            self.set_part(&part, out, None);
        }
        Ok(())
    }

    /// A file with no windows part, whose model holds one with settings to write (`setting`): the model's part goes in, `windows.xml` beside the document part (or the next free
    /// name), with the document's relationship to it and its content type.
    fn add_windows(&mut self, setting: bool) -> R<()> {
        let held = self.doc.vsdx.as_ref().and_then(|v| v.windows.as_deref());
        let Some(held) = held.filter(|_| setting) else { return Ok(()) };
        let folder = folder_of(&self.document_part);
        let preferred = format!("{folder}windows.xml");
        let name = if !self.present.contains_key(&fold(&preferred)) && !self.reserved.contains(&fold(&preferred)) {
            preferred
        } else {
            self.fresh_names(folder, "windows", ".xml", 1).pop().ok_or(WriteError::Limit("no part name was left for the windows part"))?
        };
        self.reserved.insert(fold(&name));
        let bytes = Tree::new(diacraft_sheetxml::passthrough::to_tree(held)).bytes(&name)?;
        self.set_part(&name, bytes, Some(held::CT_WINDOWS));
        let document = self.document_part.clone();
        let target = relative_target(&document, &name);
        let rels = self.rels_mut(&document)?;
        let id = rels.next_id();
        rels.add(&id, &rel_type(VIS, "windows"), &target);
        self.edited = true;
        Ok(())
    }

    // ------------------------------------------------------------------------------------------------ page and master parts

    /// Patches the shapes and connects of a page or master part: `shapes` and `connects` are the (baseline, new) models.
    /// The patches of the parts of `jobs`, **made on up to [`Plan::threads`] threads** when there is more than one, and put in the plan in the order of the jobs: the first job that failed
    /// is the error, as it is when each is made as it is met. The trees of the parts in flight are held to [`REWRITE_WINDOW`], each charged [`Rewrite::charge`] before it is made.
    fn rewrite_all(&mut self, jobs: Vec<Rewrite<'_>>) -> R<()> {
        const MISSING: WriteError = WriteError::Mismatch("a page or master part is not in the file");
        if self.threads <= 1 || jobs.len() < 2 {
            for job in jobs {
                let out = job.run(self.bytes_of(&job.part).ok_or(MISSING)?)?;
                self.put_patched(&job.part, out);
            }
            return Ok(());
        }
        let outs = {
            let mut work: Vec<(&Rewrite<'_>, &[u8])> = Vec::new();
            work.try_reserve_exact(jobs.len()).map_err(|_| WriteError::Limit("the parts to patch do not fit in memory"))?;
            for job in &jobs {
                work.push((job, self.bytes_of(&job.part).ok_or(MISSING)?));
            }
            patch_together(self.threads, &work)?
        };
        for (job, out) in jobs.iter().zip(outs) {
            self.put_patched(&job.part, out);
        }
        Ok(())
    }

    /// A page or master part patched to `out`: **an edit when its bytes are not the part's**, and then written; a patch that changed nothing leaves the part as the file has it, and is
    /// no edit.
    fn put_patched(&mut self, part: &str, out: Vec<u8>) {
        if self.bytes_of(part).is_some_and(|held| held == out.as_slice()) {
            return;
        }
        self.edited = true;
        self.set_part(part, out, None);
    }

    // -------------------------------------------------------------------------------------------------------- properties

    fn properties(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        // A change to a field of the properties is an edit like any other: the save stamps it.
        let fields_changed = !self.new_file && props_fields_differ(&base.props, &doc.props);
        if fields_changed {
            self.edited = true;
        }
        let root_part = |plan: &Plan<'_>, ty: String| {
            plan.root_rels.iter().find(|r| r.ty == ty && !r.external).and_then(|r| resolve("/", &r.target)).filter(|p| plan.alive(p))
        };
        // After an edit the thumbnail is stale. Both the part and its relationship go; its `Default` stays.
        if self.edited && !self.new_file {
            let ty = rel_type(PK, "thumbnail");
            if let Some(part) = root_part(self, ty.clone()) {
                let ids: Vec<String> = self.root_rels.iter().filter(|r| r.ty == ty).map(|r| r.id.clone()).collect();
                let rels = self.rels_mut("/")?;
                for id in ids {
                    rels.remove(&id);
                }
                self.remove_part(&part);
            }
        }
        // An edit makes a digital signature false. Its parts are kept, and the user is told.
        if self.edited && !self.new_file && self.root_rels.iter().any(|r| r.ty.ends_with("/digital-signature/origin")) {
            self.warnings.push("the file was digitally signed, and the signature no longer holds".to_owned());
        }
        let time = self.opts.now.and_then(props::format_time);
        if let Some(part) = self.property_part(&rel_type(OD, "extended-properties"), CT_APP)
            && let Some(mut tree) = self.property_tree(&part, "Properties")?
        {
            let names = |d: &Document| d.pages.iter().map(|p| p.name.to_string()).collect::<Vec<_>>();
            let (old, new) = if self.new_file { (Vec::new(), Vec::new()) } else { (names(base), names(doc)) };
            let mut warnings = Vec::new();
            // (An unedited Save into a file the caller wrote keeps the file's `Application`: `WriteOptions::resave_unedited`.)
            let keeps_name = self.opts.resave_unedited && !self.edited && !self.new_file;
            let mut changed = !keeps_name && props::patch_app(&mut tree, &old, &new, &mut warnings);
            if !self.new_file {
                changed |= props::patch_app_fields(&mut tree, &base.props, &doc.props);
            }
            if changed {
                let out = tree.bytes(&part)?;
                self.set_part(&part, out, None);
            }
            self.warnings.append(&mut warnings);
        }
        if let Some(part) = self.property_part(&rel_type(PK, "core-properties"), CT_CORE) {
            let title = (!self.new_file && base.title != doc.title).then_some(&*doc.title);
            // A copy made from a template is a new document: its times are its own (`WriteOptions::new_copy`).
            let fresh = self.opts.new_copy && !self.new_file;
            let modified = if (self.edited || fresh) && !self.new_file { time.as_deref() } else { None };
            let created = if fresh { time.as_deref() } else { None };
            if (title.is_some() || modified.is_some() || created.is_some() || fields_changed)
                && let Some(mut tree) = self.property_tree(&part, "coreProperties")?
            {
                let mut changed = props::patch_core(&mut tree, title, created, modified);
                if !self.new_file {
                    changed |= props::patch_core_fields(&mut tree, &base.props, &doc.props);
                }
                if changed {
                    let out = tree.bytes(&part)?;
                    self.set_part(&part, out, None);
                }
            }
        }
        // The custom properties are written when the model lists some that are not the file's. A model with none is **not** a change: the model of a drawing that was never saved holds none, where
        // the file the first save made holds the build number and the unit system a new drawing has, and neither is an edit (so a list taken down to nothing is not written: no command does it).
        // **The part holds the VSDX custom properties, and only they are compared** (the vector): the user-defined fields of an `.odg` ride in DiaCraft's part, which compares
        // them. A drawing made from an `.otg` that has some held them alone, so its list was not empty and not the file's, and an unedited Save wrote the part again from the VSDX ones, none: the
        // build number and the unit system the first save wrote were taken out.
        if !self.new_file && vsdx_custom(&doc.props).next().is_some() && !vsdx_custom(&base.props).eq(vsdx_custom(&doc.props)) {
            self.write_custom_list()?;
        }
        if self.opts.recalc_document {
            match self.property_part(&rel_type(OD, "custom-properties"), CT_CUSTOM) {
                Some(part) => {
                    if let Some(mut tree) = self.property_tree(&part, "Properties")?
                        && props::ensure_recalc(&mut tree)
                    {
                        let out = tree.bytes(&part)?;
                        self.set_part(&part, out, None);
                    }
                }
                None => {
                    let part = "/docProps/custom.xml".to_owned();
                    let tree = props::new_custom(true);
                    self.set_part(&part, tree.bytes(&part)?, Some(CT_CUSTOM));
                    let rels = self.rels_mut("/")?;
                    let id = rels.next_id();
                    rels.add(&id, &rel_type(OD, "custom-properties"), "docProps/custom.xml");
                }
            }
        }
        Ok(())
    }

    /// The custom properties of the model, written as the part's list: the part is made again from them, or made when the file has none (a part the model holds as bytes is not a list, and its
    /// list is empty on both sides, so it is not here).
    fn write_custom_list(&mut self) -> R<()> {
        let list: Vec<&diacraft_doc::CustomProp> = self.doc.props.custom.iter().filter(|c| c.format == Format::Vsdx).collect();
        let tree = {
            let _room = build::PartRoom::enter(0, 0);
            props::model_custom(&list, false, self.opts.recalc_document)?
        };
        match self.property_part(&rel_type(OD, "custom-properties"), CT_CUSTOM) {
            Some(part) => {
                let out = tree.bytes(&part)?;
                self.set_part(&part, out, None);
            }
            None => {
                let part = "/docProps/custom.xml".to_owned();
                self.set_part(&part, tree.bytes(&part)?, Some(CT_CUSTOM));
                let rels = self.rels_mut("/")?;
                let id = rels.next_id();
                rels.add(&id, &rel_type(OD, "custom-properties"), "docProps/custom.xml");
            }
        }
        self.edited = true;
        Ok(())
    }

    /// The property part of one kind (`props::property_part`, which the reader uses too): found through the package's relationships by its content type
    /// as well as by the relationship's type, so that an extended properties part hung under another properties relationship is the one that is named.
    fn property_part(&self, rel: &str, content_type: &str) -> Option<String> {
        let reached: Vec<(&str, String)> = self
            .root_rels
            .iter()
            .filter(|r| !r.external)
            .filter_map(|r| Some((r.ty.as_str(), resolve("/", &r.target)?)))
            .filter(|(_, part)| self.alive(part))
            .collect();
        let names: Vec<(&str, &str)> = reached.iter().map(|(ty, part)| (*ty, part.as_str())).collect();
        crate::props::property_part(&names, rel, content_type, |part| self.pkg.content_type(part)).map(str::to_owned)
    }

    /// A property part's tree, to edit. A part that is not well-formed XML, or whose root is not `root`, is **not edited and not dropped**: the reader
    /// opens such a file (a warning, `properties_unreadable`), so the save must not refuse it, and replacing a part this crate cannot read would lose
    /// what it holds. The part stays as it was, and the save says so in words (its `Application` is then not this writer's).
    fn property_tree(&mut self, part: &str, root: &str) -> R<Option<Tree>> {
        let bytes = self.bytes_of(part).ok_or(WriteError::Mismatch("a property part is not in the file"))?;
        let why = match Tree::parse(part, bytes) {
            Ok(tree) if tree.root.is(root) => return Ok(Some(tree)),
            Ok(_) => "its root is not the one a property part has".to_owned(),
            Err(WriteError::Original(VsdxError::Xml { error, .. })) => format!("it is not usable XML ({})", error.class()),
            Err(e) => return Err(e),
        };
        // The name is told only when it is package structure (`part_label`): the part may be named by a person.
        self.warnings.push(format!("{} was kept as it was: {}", part_label("a property part", part), why));
        Ok(None)
    }

    // --------------------------------------------------------------------------------------------------------- the file

    fn finish(mut self, limits: &Limits) -> R<Written> {
        // The relationships parts that were edited, **in the order of [`Plan::source_place`]** and not in the order the map lists them (that order
        // is new for every map, so two saves of one model put the relationships parts they make in two orders, in the zip and in the lists of what
        // the save did). The places are all different but for the sources that are no part, and the key, which is unique, settles those.
        let mut edited: Vec<RelsTree> = Vec::new();
        let mut keys: Vec<((u8, usize), String)> =
            self.rels.iter().filter(|(_, r)| r.changed).map(|(k, _)| (self.source_place(k), k.clone())).collect();
        keys.sort_unstable();
        for (_, key) in keys {
            if let Some(tree) = self.rels.remove(&key) {
                edited.push(tree);
            }
        }
        for tree in edited {
            let bytes = tree.tree.bytes(&tree.name)?;
            self.set_part(&tree.name, bytes, None);
        }
        // The stream is the file's own when no row changed (`TypesTree::bytes`).
        let mut types_bytes = self.types.bytes()?;
        let names: Vec<String> = self.order.iter().filter(|n| self.alive(n)).cloned().collect();
        let untyped: Vec<String> = {
            let probe = PackageWriter::new(&types_bytes).map_err(WriteError::Package)?;
            names.iter().filter(|n| probe.content_type(n).is_none()).cloned().collect()
        };
        if !untyped.is_empty() {
            // A part with no type gets one so that the output is legal. A relationships part's type is known, and
            // it is by extension: the `Default` of `rels` (every sample has one) gives it to all of them at once, whether they were
            // written now or are the file's, and no part gets an `Override` for a type a `Default` gives. Any other part's type is not known: it
            // gets `application/octet-stream` by an `Override` of its own.
            let (rels_parts, others): (Vec<&String>, Vec<&String>) = untyped.iter().partition(|n| is_rels_part(n));
            if !rels_parts.is_empty() {
                self.types.ensure_default("rels", CT_RELS);
            }
            for name in &others {
                self.types.set_override(name, CT_OCTET);
            }
            // The save says so of the parts the file had without a type; a relationships part this save made is the writer's, and has none to tell.
            let ours: std::collections::HashSet<String> = self.added.iter().map(|n| fold(n)).collect();
            let told: Vec<&String> = untyped.iter().filter(|n| !ours.contains(&fold(n))).collect();
            let (all, rels) = (told.len(), told.iter().filter(|n| is_rels_part(n)).count());
            if all > 0 {
                self.warnings.push(if rels == 0 {
                    format!("{all} parts had no content type and were given {CT_OCTET}")
                } else if rels == all {
                    format!("{all} parts had no content type and were given the relationships type")
                } else {
                    format!(
                        "{all} parts had no content type: {rels} relationships parts were given the relationships type and the other {} {CT_OCTET}",
                        all - rels
                    )
                });
            }
            types_bytes = self.types.bytes()?;
            // Every part again, with the stream as it now is: a `Default` the reader does not count (a row with no `ContentType`) gave no type to the
            // parts it was added for, and a part that still has none gets an `Override` of its own.
            let still: Vec<String> = {
                let probe = PackageWriter::new(&types_bytes).map_err(WriteError::Package)?;
                names.iter().filter(|n| probe.content_type(n).is_none()).cloned().collect()
            };
            if !still.is_empty() {
                for name in &still {
                    self.types.set_override(name, if is_rels_part(name) { CT_RELS } else { CT_OCTET });
                }
                types_bytes = self.types.bytes()?;
            }
        }
        if self.types.compacted() {
            self.warnings.push(
                "the content types, with the file's own bytes, would have been larger than a package allows for them: they were written as their rows alone, and their comments and white space were left out".to_owned(),
            );
        }
        let mut writer = PackageWriter::new(&types_bytes).map_err(WriteError::Package)?;
        // The package relationships come second.
        let first = names.iter().find(|n| fold(n) == "/_rels/.rels");
        let mut parts: Vec<(&str, &[u8])> = Vec::new();
        parts.try_reserve_exact(names.len()).map_err(|_| WriteError::Package(diacraft_opc::WriteError::OutOfMemory))?;
        for name in first.into_iter().chain(names.iter().filter(|n| fold(n) != "/_rels/.rels")) {
            let bytes = self.bytes_of(name).ok_or(WriteError::Mismatch("a part that should be written is not there"))?;
            parts.push((name, bytes));
        }
        // The compression of the parts is made on workers when the save has them: the package is the same bytes at any number of threads.
        writer.add_all(&parts, diacraft_container::zip::Workers::new(self.threads)).map_err(WriteError::Package)?;
        drop(parts);
        let out = writer.finish().map_err(WriteError::Package)?;

        // The check: the package reads, and has the parts it should. The parts the model holds as bytes, written from them, are checked against them and not inflated (`read::Known`).
        let known = Known::of(self.doc).with_parts(self.carried_blobs.iter().map(|(name, bytes)| (name.as_str(), bytes)));
        let crate::read::ReadBack { doc: mut document, report: read_back, names: got } =
            crate::read::read_back(&out, limits, &known).map_err(read_back_failed)?;
        // The parts the model holds as bytes are the model's: the baseline holds those, and not a second copy of what the check read (`read_back`). Unless the save wrote objects of the other format as
        // embeddings, or left a part out: then the parts are the ones the file holds (`objects::baseline_parts`).
        document.parts = objects::baseline_parts(self.doc, &self.natively_parts, &self.macros_dropped, self.made_embeddings());
        // The baseline the next save compares with is what the caller's model says, in the two places where the file may hold more than the model.
        // A drawing that was never opened from a file is the skeleton plus the model: the skeleton wrote a `DocumentSettings` that its model does not
        // hold, and a font table that may list more faces than the model's (the ones its cells name, or a default). **That is so for every save of
        // such a drawing, not only the first** (the second save, in place, reads the file the first wrote): the unknown elements of the document are
        // the caller's, and a later save compares them with its own, so the file's own `DocumentSettings` must not be taken for an edit the writer
        // cannot write ("a change to the unknown elements of the document"), nor the font table for the caller dropping faces (which would rewrite it).
        // For a file that was read, the model holds what the file does (an edit to either is refused above or written to the file), so this is the
        // same model.
        let file_words = document.snap_words();
        document.passthrough = self.doc.passthrough.clone();
        if self.words_left {
            document.set_snap_words(&file_words);
        }
        document.fonts = self.doc.fonts.clone();
        // DiaCraft's own data: when this save wrote the model's data and the file now has a part that reads, the file says what the model says, and the
        // baseline is the model's (a re-read of the same elements may differ in nothing the model keeps). When the save did not write it (the writer
        // refused, and said so in a warning), the baseline is what the file gives, so that the next save sees the same difference and refuses again.
        // A part this save wrote that does not read back is a warning, never a failed save.
        if self.diacraft_written && self.doc.diacraft.is_some() {
            if matches!(read_back.diacraft, Some(crate::DiacraftPart::Loaded { .. })) {
                document.diacraft = self.doc.diacraft.clone();
            } else {
                self.warnings.push(
                    "DiaCraft's own settings (the Snap and Glue options with no flag in the file) were not saved: the part just written does not read back"
                        .to_owned(),
                );
            }
        }
        // The names of the parts the check read (`ReadBack::names`, folded): the package is read once, and not a second time for its names.
        let mut wanted: Vec<String> = names.iter().map(|n| fold(n)).collect();
        let mut got = got;
        wanted.sort();
        got.sort();
        if wanted != got {
            return Err(WriteError::SelfCheck("the parts of the file are not the parts that were written".to_owned()));
        }
        let kept = names.iter().filter(|n| !matches!(self.bodies.get(&fold(n)), Some(Body::New(_) | Body::Shared(_)))).count();
        Ok(Written {
            bytes: out,
            document,
            read_back,
            warnings: self.warnings,
            rewritten: self.rewritten,
            added: self.added,
            removed: self.removed,
            kept,
            macros_dropped: self.macros_dropped,
            carried: self.carried_report,
            natively: self.natively_parts,
        })
    }
}

// ------------------------------------------------------------------------------------------------------------------------ helpers

/// The custom properties of the model that `docProps/custom.xml` holds: the VSDX ones (the other formats' ride in DiaCraft's part).
fn vsdx_custom(p: &diacraft_doc::DocProps) -> impl Iterator<Item = &diacraft_doc::CustomProp> {
    p.custom.iter().filter(|c| c.format == Format::Vsdx)
}

/// Whether the fields of the properties that the file's property parts say differ: all but the two times (a save stamps them) and the custom properties (their own list).
fn props_fields_differ(a: &diacraft_doc::DocProps, b: &diacraft_doc::DocProps) -> bool {
    let fields = |p: &diacraft_doc::DocProps| diacraft_doc::DocProps { created: None, modified: None, custom: Vec::new(), ..p.clone() };
    fields(a) != fields(b)
}

/// The unknown elements of the document part made the model's: every child that is none of the four the model types goes, and the model's own are put back after as many of the typed ones as they
/// followed. A part that has its words patched is not here (the words are in the model's `DocumentSettings`).
fn replace_unknown(root: &mut El, sc: &Scope, doc: &Document) -> R<()> {
    // What the model keeps is taken from the room of the part before it is made: a model that keeps more than a part may hold is refused in words.
    build::take_kept(&[&doc.passthrough])?;
    let (mut colors, mut fonts, mut styles, mut sheet) = (false, false, false, false);
    let mut typed: Vec<Node> = Vec::new();
    for node in std::mem::take(&mut root.children) {
        let Node::El(child) = &node else { continue };
        let csc = sc.enter(child);
        let seen = if csc.is(child, "Colors") {
            &mut colors
        } else if csc.is(child, "FaceNames") {
            &mut fonts
        } else if csc.is(child, "StyleSheets") {
            &mut styles
        } else if csc.is(child, "DocumentSheet") {
            &mut sheet
        } else {
            continue;
        };
        if !*seen {
            *seen = true;
            typed.push(node);
        }
    }
    root.children = diacraft_sheetxml::build::weave(typed, &[&doc.passthrough]);
    Ok(())
}

/// A page's background before and after, when it changed: `(old, new)`.
type Background = Option<(Option<u32>, Option<u32>)>;

/// A page or master part for a page or master the edit added: the shapes and the connects, in the standard root, **with what the item keeps of its part** (its slot's entries for the root,
/// the `Shapes` and the `Connects`: the nodes beside the shapes and the rows, and the prefixes the root declared for them), as the model writer writes them (`model.rs`). A master brought from a
/// stencil keeps those (`Master::leave_package`), and a part written without them would lose them; an item with none is the part it always was.
fn new_content<T: Default + Clone + PartialEq>(root: &str, shapes: &ShapeList, connects: &Connects, ext: &Ext<T>) -> R<Tree> {
    let _room = build::PartRoom::enter(0, 0);
    let (root_kept, shapes_kept, connects_kept) = (rest_of(ext, root), rest_of(ext, "Shapes"), rest_of(ext, "Connects"));
    let _r = build::ForeignR::enter(root_kept);
    let mut children = Vec::new();
    if !shapes.is_empty() || shapes_kept.is_some() {
        children.push(Node::El(build::shapes_el_with(shapes, 0, shapes_kept)?));
    }
    if !connects.is_empty() || connects_kept.is_some() {
        children.push(Node::El(build::connects_el_with(connects, connects_kept)?));
    }
    Ok(Tree::new(model::root_el(root, true, children, root_kept)?))
}

/// The `ID` of each master the shapes of a part use, nested shapes too.
fn masters_used(list: &ShapeList) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<&ShapeList> = vec![list];
    while let Some(shapes) = stack.pop() {
        for shape in shapes.iter() {
            let shape: &Shape = shape;
            if let Some(id) = shape.master {
                out.insert(id);
            }
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
    out
}

/// Whether the pages that stay are in another order than the file has them: `matched[j]` is the place in the baseline of the `j`th page of the model, when it had one. True when a page that
/// stays comes after one that stood after it in the file (`matched` is not rising); a page that is new or gone says nothing of the order of the others.
fn pages_moved(matched: &[Option<usize>]) -> bool {
    let mut reached = 0usize;
    for place in matched.iter().flatten() {
        if *place < reached {
            return true;
        }
        reached = *place;
    }
    false
}

/// Whether two pages are the same row of the pages index: everything but their shapes and the writer's `r:id`.
fn page_row_same(a: &Page, b: &Page) -> bool {
    std::ptr::eq(a, b) || (page_attrs(a) == page_attrs(b) && a.sheet == b.sheet && a.passthrough == b.passthrough)
}

fn master_row_same(a: &Master, b: &Master) -> bool {
    std::ptr::eq(a, b) || (master_attrs(a) == master_attrs(b) && a.sheet == b.sheet && a.passthrough == b.passthrough && a.icon == b.icon)
}

#[allow(dead_code)]
const _: &str = NS_TYPES;

#[cfg(test)]
mod page_order_words {
    use super::pages_moved;

    #[test]
    fn pages_are_moved_when_the_places_they_had_do_not_rise() {
        assert!(!pages_moved(&[]));
        assert!(!pages_moved(&[Some(0), Some(1), Some(2)]), "the order the file has");
        assert!(pages_moved(&[Some(1), Some(0)]), "two pages that changed places");
        assert!(pages_moved(&[Some(0), Some(2), Some(1)]), "one of three taken back");
        // A page that is new (`None`) or taken out (a place no page of the model has) says nothing of the order of the others.
        assert!(!pages_moved(&[None, Some(0), None, Some(2), Some(3)]));
        assert!(pages_moved(&[None, Some(3), None, Some(2)]));
        assert!(!pages_moved(&[None, None]));
    }
}

#[cfg(test)]
mod read_back_words {
    use super::*;

    #[test]
    fn a_refusal_for_memory_is_worded_as_a_save_and_not_as_a_file_that_does_not_read() {
        let words = read_back_failed(VsdxError::Limit { kind: crate::error::LimitKind::Memory, max: 1 });
        assert!(matches!(words, WriteError::Limit(what) if what.contains("not enough memory")), "{words}");
        assert_eq!(words.class(), "write_limit");
        assert!(!words.to_string().contains("does not read back"), "{words}");
        // The memory of the process running out while the check inflates a part is the same, and not a file that does not read back ("out of memory while reading").
        let inflate =
            read_back_failed(VsdxError::Package(diacraft_opc::OpcError::OutOfMemory { what: "/visio/embeddings/oleObject3.bin".to_owned() }));
        assert!(matches!(&inflate, WriteError::Limit(what) if what.contains("not enough memory left to check the file just made")), "{inflate}");
        assert!(!inflate.to_string().contains("does not read back"), "{inflate}");
        // Any other failure of the check is a file that does not read back.
        let other = read_back_failed(VsdxError::NoDrawingParts("none"));
        assert!(matches!(other, WriteError::SelfCheck(_)), "{other}");
        assert!(other.to_string().contains("does not read back"), "{other}");
    }
}
