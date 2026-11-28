//! The model writer: a drawing written from the model alone, with no file to splice into (the native format's design, sections 4.1, 4.2 and 4.5).
//!
//! **When it is the writer.** `write_with(doc, None, ..)` writes a drawing that was never saved by [`skeleton`](super::skeleton) and the splice machinery (a package of no page, to which the save adds
//! what the model has). That path knows nothing of what a drawing that came from a package holds beside its cells, so **a model that holds anything of VSDX is written here instead** ([`wanted`]): a
//! document that was read from a package (`Document::vsdx`), one that holds a part of its own (`Document::parts`), or one that has unknown elements of its document. A drawing made new in
//! DiaCraft holds none of it and is written as it always was. Both are one package of one kind: the same reader reads it, the same content check compares it.
//!
//! **What it writes, from the model alone** (`Document::parts`, `Document::vsdx`, the extension slots, the part names and relationship ids of pages and masters):
//!
//! * every **held part**, from the bytes the model holds (written from the `Arc`, never copied), under its name, with its type and the relationships from it as they were;
//! * every **relationship row** the model holds, with its `Id`, type, target and mode, and the rows for the parts the writer makes (a page, a master, a picture, a theme, a window, a solution), **never
//!   reusing an `Id` or a part name that a held row or part has, compared ASCII case-insensitively as OPC compares them**; a row from a part that is not written (a deleted page's) is dropped, and so
//!   is an embedded object that only such a row reached. **Every name a part of the model goes by is reserved before the writer names a part of its own** (the part of each page, master and theme,
//!   the source and the internal target of each held row, the name of each picture a shape names a relationship to), so that a page added after another was taken out is given none of the rows of
//!   the other's part and a picture that was there is not renamed; **a part's rows are the rows of the name its page or master had** (a page written under another name carries its rows to it, their
//!   targets taken to where they lead), and **the rows of a page to the masters its shapes use and to its background page are made from the model** (`model.rs`, `sync_rels`): a missing one is
//!   made, one to a part that is not written goes, and those the model does not explain (to a master no shape uses, to a background page that is not the page's) are the file's own while the
//!   drawing is as it was read and are made from the model after an edit;
//! * the **document part, the indexes and the page and master parts**, with the elements and attributes of every place the reader types and gives no field to (the `vsdx` side of each slot, `unheld.rs`)
//!   put back where they were, a page's part under the name it had (`Page::part_name`) and its row under the id it had (`Page::rel_id`), a part that two pages name written once while their content is the same (each gets a part of its own once one of them differs);
//! * the **properties** (`core`, `app` and `custom`) from the fields of `DocProps` and what the parts held beside them, and `windows.xml` as it was held;
//! * the **content types**: a row for each part, **a `Default` for a part whose type its extension gives (a picture, a `.bin` part, a type that is no XML) and an `Override` for
//!   the others, and for every extension that a `Default` the model holds names** (a `Default` that no part relied on is held, and it says that every part of its extension had an `Override`), and
//!   the rows the model holds beside them. **The choice between the two is not recorded in the model** (the report, item 7), so a package that types a part the other way gives a
//!   row more and a row less; the content check says so, and EVID measures how often.
//!
//! **What a save may change** (design 4.4): with [`WriteOptions::content_unchanged`] the bookkeeping is as it was read (the thumbnail and its relationship, `Application`, `modified`); otherwise
//! the thumbnail part and its relationship are left out (their `Default` row stays), `Application` is `DiaCraft` and `modified` is the save's. **A signature's parts are kept** and the save says they no
//! longer match. A VBA project goes only into a macro-enabled type.
//!
//! **Costs.** Parts, rows, names and ids are looked at once each (`work::model_*`): a map of the held rows by source, a map of the names in use, and one walk up the numbers for each kind of name. What the
//! held parts and rows would be charged by the read-back is counted before anything is made, and a model past a cap the reader holds a package to is refused in words.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{ContentTypeRow, Document, Format, Master, Media, Page, Part, PartRole, Rel, ShapeList, Theme, VsdxHeld};
use diacraft_opc::name::{fold, is_rels_part, lenient_decode, resolve};
use diacraft_opc::{MAX_CONTENT_TYPE_ENTRIES, MAX_PACKAGE_XML_BYTES, MAX_PARTS, MAX_RELATIONSHIPS};

use super::parts::{OD, PK, RelRow, VIS, folder_of, rel_type, relative_target};
use super::{CT_MASTER, CT_MASTERS, CT_PAGE, CT_PAGES, MACRO_NAMES_SHOWN, R, VBA_CONTENT_TYPE, VBA_PROJECT, WriteError, WriteOptions, Written};
use crate::report::{Kind, Variant};
use crate::unheld::part_label;

pub(super) const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
pub(super) const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml"; // brand-ok: a content type the format defines
pub(super) const CT_COMMENTS: &str = "application/vnd.ms-visio.comments+xml"; // brand-ok: a content type the format defines
pub(super) const CT_SOLUTIONS: &str = "application/vnd.ms-visio.solutions+xml"; // brand-ok: a content type the format defines
pub(super) const CT_SOLUTION: &str = "application/vnd.ms-visio.solution+xml"; // brand-ok: a content type the format defines
pub(super) const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// What a save holds beside the bytes of the parts and pictures: the process (its stacks and its arenas, 0.37 GB measured), the trees of the part it is making, the archive's tables, the room a
/// file grows into and the read-back's model of the drawing. **Measured** under `ulimit -v 3000000` (`model_hostile::child`): a model of 671 MB of parts is written and read back at an address
/// space of 2.52 GB, and one of 805 MB at 2.79 GB, which is the bytes of the model, the file and the package the check inflates (each as large as the parts are) and 0.4 to 0.5 GB beside them.
const PEAK_MARGIN_BYTES: u64 = 512 << 20;
/// The most relationship rows all the held parts and rows of a model hold together. The reader holds each of them in the model it makes, and a model past this was not read from a package.
const MAX_HELD_ROWS: usize = 2_000_000;
/// The kinds of target a row to a part that only that row reached takes the part with it.
const REMOVABLE: [&str; 4] = ["/image", "/oleObject", "/package", "/activeXControlBinary"];
/// The relationship type of a thumbnail, and the base of those of a digital signature's parts.
const THUMBNAIL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
/// The extension a part is typed by: the text after the last dot of its last segment, in lower case.
fn extension_of(name: &str) -> Option<String> {
    let last = name.rsplit('/').next()?;
    let (_, extension) = last.rsplit_once('.')?;
    (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

/// Whether a name a model gives to a part of its own can be one: a slash, then segments that are not empty or dots, and no character a package refuses or a reader would take for something else.
pub(super) fn plausible(name: &str) -> bool {
    name.len() <= 200
        && name.starts_with('/')
        && !name.ends_with('/')
        && !is_rels_part(name)
        && name.get(1..).is_some_and(|rest| rest.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != ".."))
        && !name.chars().any(|c| c.is_control() || matches!(c, '\\' | '%' | '?' | '#' | '"' | '<' | '>' | '|' | '*' | ':'))
}

/// Whether the parts a save would make are more than a package holds: the `entries` the writer writes, and `with_rels` of them that have a relationships part, and the content types (one more).
fn parts_refused(entries: usize, with_rels: usize) -> bool {
    entries.saturating_add(with_rels).saturating_add(1) > MAX_PARTS
}

/// The bytes a part of the model is written as, from the model's own: a held part's, a picture's, an embedding's, a theme's, and none for a part the writer makes from fields.
fn body_len(body: Body<'_>, media: &[MediaPlan<'_>], objects: &[super::objects::Object]) -> u64 {
    (match body {
        Body::Held(part) => part.bytes.len(),
        Body::Media(i) => media.get(i).map_or(0, |m| m.media.bytes.len()),
        Body::Object(i) => objects.get(i).map_or(0, |o| o.bytes.len()),
        Body::Theme(theme) => theme.bytes.len(),
        _ => 0,
    }) as u64
}

/// Whether a save of parts that hold `held` bytes is more than the address space `space` has for it (a twentieth of it is kept for the process): three times the bytes (the model's, the file's and the
/// package's) and the margin.
fn peak_refused(held: u64, space: u64) -> bool {
    let most = space - space / 20;
    held.saturating_mul(3).saturating_add(PEAK_MARGIN_BYTES) > most
}

// ------------------------------------------------------------------------------------------------------------------------ the rows

/// The rows of the relationships of one source, in the order they are written, with the `Id`s in use (ASCII case-insensitively) and the walk that makes a new one.
#[derive(Default)]
pub(super) struct RowSet {
    pub rows: Vec<RelRow>,
    ids: HashMap<String, usize>,
    n: u32,
}

impl RowSet {
    /// The rows of the model's `rows`, the first of each `Id`: OPC knows one row by an `Id`, and a second is not written.
    pub(super) fn of<'r>(rows: impl IntoIterator<Item = &'r Rel>) -> RowSet {
        let mut set = RowSet::default();
        for row in rows {
            set.push(RelRow { id: row.id.to_string(), ty: row.kind.to_string(), target: row.target.to_string(), external: row.external });
        }
        set
    }

    /// Adds a row, unless its `Id` is in use. → whether it was added.
    pub(super) fn push(&mut self, row: RelRow) -> bool {
        crate::work::model_row();
        let key = row.id.to_ascii_lowercase();
        if self.ids.contains_key(&key) {
            return false;
        }
        self.ids.insert(key, self.rows.len());
        self.rows.push(row);
        true
    }

    /// The row with this `Id`.
    pub(super) fn get(&self, id: &str) -> Option<&RelRow> {
        self.ids.get(&id.to_ascii_lowercase()).and_then(|at| self.rows.get(*at))
    }

    /// The least `rId` that is not in use and that no walk before gave: the walk goes up the numbers once, however many are asked for.
    pub(super) fn fresh_id(&mut self) -> Option<String> {
        loop {
            self.n = self.n.checked_add(1)?;
            crate::work::id_scan(1);
            let id = format!("rId{}", self.n);
            if !self.ids.contains_key(&id.to_ascii_lowercase()) {
                return Some(id);
            }
        }
    }

    /// Adds a row of a new `Id`. → the `Id`.
    pub(super) fn add(&mut self, ty: &str, target: &str, external: bool) -> Option<String> {
        let id = self.fresh_id()?;
        self.push(RelRow { id: id.clone(), ty: ty.to_owned(), target: target.to_owned(), external });
        Some(id)
    }

    /// Takes out every row `goes` says goes, in one pass.
    pub(super) fn drop_where(&mut self, goes: impl Fn(&RelRow) -> bool) {
        let rows = std::mem::take(&mut self.rows);
        self.ids.clear();
        for row in rows {
            if !goes(&row) {
                self.ids.insert(row.id.to_ascii_lowercase(), self.rows.len());
                self.rows.push(row);
            }
        }
    }

    /// The first row of this type that leads to a part inside the package.
    pub(super) fn first_of(&self, ty: &str) -> Option<&RelRow> {
        self.rows.iter().find(|r| r.ty == ty && !r.external)
    }
}

// ----------------------------------------------------------------------------------------------------------------- what is written

/// What a part is made from.
#[derive(Clone, Copy)]
pub(super) enum Body<'a> {
    Document,
    Pages,
    Masters,
    Windows,
    Comments,
    Core,
    App,
    Custom,
    SolutionsIndex,
    Solution,
    Theme(&'a Arc<Theme>),
    Page(usize),
    Master(usize),
    Media(usize),
    Held(&'a Arc<Part>),
    /// A byte string that the data of the other format rides with, in a part of DiaCraft's own (`carry.rs`): the number of the blob in [`Book::carried`].
    Blob(usize),
    /// An object an `.odg` frame carried back, written as an embedding (`objects.rs`): its number in [`Book::embedded`].
    Object(usize),
}

/// A part to write: its name, the content type it is given, and what it is made from. Its place in the list is its place in the package.
pub(super) struct Entry<'a> {
    pub name: String,
    pub ct: String,
    pub body: Body<'a>,
}

/// The picture of one or more shapes: the part it is written as, and the type it is given.
pub(super) struct MediaPlan<'a> {
    pub name: String,
    pub media: &'a Arc<Media>,
}

/// The content of the part a page or master names, which is what a second page naming the part must have too.
#[derive(Clone, Copy)]
pub(super) struct Content<'a> {
    shapes: &'a ShapeList,
    connects: &'a diacraft_doc::Connects,
}

impl PartialEq for Content<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes && self.connects == other.connects
    }
}

/// The names the writer makes: where each kind's walk up the numbers stands.
#[derive(Default)]
pub(super) struct Walks {
    pub page: u32,
    pub master: u32,
    pub theme: u32,
    pub media: u32,
    pub solution: u32,
    /// The names of the parts of DiaCraft's own that carry the data of the other format.
    pub blob: u32,
    /// The names of the object parts of the frames carried back (`objects.rs`): `oleObject<n>.bin`, starting above the greatest number in use.
    pub object: u32,
    /// The names of the parts that have no kind of their own: a document part that had to be renamed, a property part, an index.
    pub other: u32,
}

/// The model, and what the writer has made of it so far.
pub(super) struct Book<'a> {
    pub doc: &'a Document,
    pub opts: &'a WriteOptions,
    pub vsdx: Option<&'a VsdxHeld>,
    /// The document holds nothing of a VSDX package: the writer makes the rows and the parts a new drawing has.
    pub fresh: bool,
    /// The bookkeeping of an edited save (or of a new file) is written.
    pub edited: bool,
    pub target: Variant,
    pub now: Option<String>,
    pub warnings: Vec<String>,
    /// The names in use, folded: the held parts, and every part the writer has named.
    pub taken: HashSet<String>,
    /// The names that rows of the content types the model holds give a type to, and that a name the writer makes does not take.
    reserved: HashSet<String>,
    /// The name of each part the model knows by a name, folded, to that name as the model holds it: the part of each page, master and theme, each held part and the source of each
    /// held row. A held row's target may spell such a part in another case than the package did ([`Book::as_held`]).
    spelling: HashMap<String, &'a str>,
    /// The folder of each name of `spelling`, folded, to the folder as that name spells it.
    folders: HashMap<String, &'a str>,
    pub walks: Walks,
    /// The held relationship rows, by the folded name of their source (`/` for the package).
    pub held: HashMap<String, Vec<&'a Rel>>,
    /// The rows of the sources the writer made or edits, by the folded name of the source.
    pub rows: HashMap<String, RowSet>,
    pub entries: Vec<Entry<'a>>,
    pub held_parts: Vec<&'a Arc<Part>>,
    /// The held parts left out: the names, as the model has them.
    pub removed: Vec<String>,
    pub macros_dropped: Vec<String>,
    /// What the file carries of the other format's data.
    pub carried_report: super::Carried,
    /// The parts of the other format this save wrote as embeddings (`Written::natively`).
    pub natively_parts: Vec<std::sync::Arc<str>>,
    pub document_part: String,
    pub pages_index: Option<String>,
    pub masters_index: Option<String>,
    pub core: Option<String>,
    pub app: Option<String>,
    pub custom: Option<String>,
    /// The `Id` of the row of the index that names the part of each page and of each master, which the `Rel` of its row says (none for a page or master with no part).
    pub page_rels: Vec<Option<String>>,
    pub master_rels: Vec<Option<String>>,
    /// The part of each page and master, by `ID`, for the rows between them.
    pub page_part_of: HashMap<u32, String>,
    pub master_part_of: HashMap<u32, String>,
    pub media: Vec<MediaPlan<'a>>,
    pub media_of: HashMap<usize, usize>,
    pub types: TypeBook,
    /// The `Id` of the row of the solutions index to the DiaCraft solution.
    pub solution_rel: Option<String>,
    /// What the model carries of the other formats' data, when it carries anything (`carry.rs`): the items the solution part holds and the byte strings that ride in parts of their own.
    pub carried: Option<crate::carry::Built>,
    /// The names of the parts of DiaCraft's own, by the number of the blob in `carried`: for the read-back, which shares their bytes with the model's.
    pub carried_names: Vec<String>,
    /// The parts of the pages written, folded, and those of them that are background pages: the rows between pages are derived from them (`model.rs`, `sync_rels`).
    pub written_pages: HashSet<String>,
    pub written_masters: HashSet<String>,
    pub background_pages: HashSet<String>,
    /// Every part the package holds, folded, once the plan is made (`model.rs`, `emit`): a row from a page or master to one of them leads somewhere.
    pub written: HashSet<String>,
    /// The bytes a held part is written as in place of its own (the DiaCraft solution a model edits: `model.rs`, `held_solutions`), by folded name, and the rows from it when they are not its own.
    pub patched: HashMap<String, Vec<u8>>,
    pub held_rows: HashMap<String, RowSet>,
    /// The shapes a page part that had none is written with, by the place of the page, when a `Rel` was given or changed (`page_rows.rs`).
    pub carried_shapes: HashMap<usize, ShapeList>,
    /// The object frames carried back and the names of their object parts (`objects.rs`).
    pub embedded: Option<super::objects::Planned>,
}

/// Whether the model writer writes `doc`: see the module documentation. A drawing of no file whose window settings were set holds a windows part and nothing else of VSDX:
/// it is written here, as a drawing made new ([`Book::new`]'s `fresh`), so that the part is charged before it is made as any held part is.
pub(super) fn wanted(doc: &Document) -> bool {
    doc.vsdx.is_some() || doc.parts.iter().any(|p| p.format == Format::Vsdx) || doc.holds_more_than_snap_words()
}

/// Writes `doc` from the model alone.
pub(super) fn write(doc: &Document, options: &WriteOptions) -> R<Written> {
    // **The page names are the ones the program that opens the file keeps** (`page_names.rs`): a model has no file to compare with, so every page is looked at.
    let kept = crate::page_names::keep_page_names(doc, None);
    let doc = kept.as_ref().map_or(doc, |k| &k.doc);
    // A page given a new name is a change of the content: the bookkeeping of an unedited visit (the program's name, the thumbnail) is not kept for it.
    let edited_options;
    let options = if kept.is_some() && options.content_unchanged {
        edited_options = WriteOptions { content_unchanged: false, ..options.clone() };
        &edited_options
    } else {
        options
    };
    let mut book = Book::new(doc, options);
    if let Some(words) = kept.as_ref().filter(|k| !k.renamed.is_empty()).map(crate::page_names::KeptNames::words) {
        book.warn(words);
    }
    book.plan()?;
    book.emit()
}

impl<'a> Book<'a> {
    fn new(doc: &'a Document, opts: &'a WriteOptions) -> Book<'a> {
        let vsdx = doc.vsdx.as_deref();
        // A model that holds a windows part and nothing else of a package came from no file: it is written as a new one, with that part.
        let fresh = vsdx.is_none_or(VsdxHeld::holds_windows_alone);
        Book {
            doc,
            opts,
            vsdx,
            fresh,
            edited: fresh || !opts.content_unchanged,
            target: opts.variant.unwrap_or(Variant { kind: Kind::Drawing, macro_enabled: false }),
            now: opts.now.and_then(super::props::format_time),
            warnings: Vec::new(),
            taken: HashSet::new(),
            reserved: HashSet::new(),
            spelling: HashMap::new(),
            folders: HashMap::new(),
            walks: Walks::default(),
            held: HashMap::new(),
            rows: HashMap::new(),
            entries: Vec::new(),
            held_parts: Vec::new(),
            removed: Vec::new(),
            macros_dropped: Vec::new(),
            carried_report: super::Carried::default(),
            natively_parts: Vec::new(),
            document_part: String::new(),
            pages_index: None,
            masters_index: None,
            core: None,
            app: None,
            custom: None,
            page_rels: Vec::new(),
            master_rels: Vec::new(),
            page_part_of: HashMap::new(),
            master_part_of: HashMap::new(),
            media: Vec::new(),
            media_of: HashMap::new(),
            types: TypeBook::default(),
            solution_rel: None,
            carried: None,
            carried_names: Vec::new(),
            written_pages: HashSet::new(),
            written_masters: HashSet::new(),
            background_pages: HashSet::new(),
            written: HashSet::new(),
            patched: HashMap::new(),
            held_rows: HashMap::new(),
            carried_shapes: HashMap::new(),
            embedded: None,
        }
    }

    pub(super) fn plan(&mut self) -> R<()> {
        self.claim_held()?;
        self.document_name()?;
        self.macros()?;
        self.thumbnail();
        self.signatures();
        self.stencil_notes();
        self.indexes()?;
        self.windows()?;
        self.properties()?;
        self.themes()?;
        self.pages_and_masters()?;
        self.pictures()?;
        // The object frames carried back, after the pictures they lead to and before the data the carrier holds: the carrier leaves out what went natively.
        self.objects()?;
        self.comments()?;
        self.solutions()?;
        self.held_entries();
        self.carry_new_pages()?;
        self.orphans();
        self.check_parts()?;
        self.check_peak()?;
        self.decide_types()
    }

    /// The parts the package would have: those written, and a relationships part for each that can have rows, which the model writer counts for every part it makes (a held part has them when it holds
    /// rows), against the parts a package of this reader holds. Refused before any is written.
    fn check_parts(&self) -> R<()> {
        let with_rels =
            self.entries.iter().filter(|e| !(matches!(e.body, Body::Blob(_)) || matches!(e.body, Body::Held(p) if p.rels.is_empty()))).count();
        if parts_refused(self.entries.len(), with_rels) {
            return Err(WriteError::Limit("more parts than a package holds"));
        }
        Ok(())
    }

    /// **What the save and its read-back hold beside the model**: the bytes of the parts and pictures in the model, the file made (as large as they are) and the package the read-back inflates (as
    /// large again), and [`PEAK_MARGIN_BYTES`] beside them. **The check of the file reads it without moving the parts into a document** (`read::read_back`), so no copy of the largest part is made
    /// and the reader's pool is not asked for one. A model whose parts and pictures would take the address space a process has for it is refused in words, before any of it is written. Since
    /// the package of the check shares the bytes of the held parts with the model where its entries hold them (`read::Known`), so the third time is met by the pictures and
    /// the themes alone: the rule is kept as it was measured, and errs toward a refusal for a model of large held parts.
    fn check_peak(&self) -> R<()> {
        let objects: &[super::objects::Object] = self.embedded.as_ref().map_or(&[], |p| &p.emb.objects);
        let held = self.entries.iter().fold(0u64, |sum, entry| sum.saturating_add(body_len(entry.body, &self.media, objects)));
        // The bytes that ride in parts of DiaCraft's own are the model's, written from it, and are in the file and in the package of its check like any other part's.
        let held =
            held.saturating_add(self.carried.as_ref().map_or(0, |c| c.blobs.iter().fold(0u64, |sum, b| sum.saturating_add(b.bytes.len() as u64))));
        if peak_refused(held, diacraft_sheetxml::model::ADDRESS_SPACE_BYTES) {
            return Err(WriteError::Limit(
                "the parts and pictures of the drawing are too large to be written and read back in the memory a save may use (the file and the package are each as large as they are)",
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------------------------------- names

    pub(super) fn claim(&mut self, name: &str) -> bool {
        self.taken.insert(fold(name))
    }

    /// Keeps a name from the parts the writer names: the part that goes by it claims it.
    pub(super) fn reserve(&mut self, name: &str) {
        self.reserved.insert(fold(name));
    }

    /// Every name the writer must not give a new part, folded: the parts in use and the names the model's rows and content types give.
    pub(super) fn names_in_use(&self) -> impl Iterator<Item = &str> {
        self.taken.iter().chain(self.reserved.iter()).map(String::as_str)
    }

    /// A name for a new part in `dir` (with its trailing slash): `{dir}{stem}{n}{ext}` for the least `n` above the walk's last that nothing has taken. The walk goes up the numbers once.
    pub(super) fn fresh_name(&mut self, dir: &str, stem: &str, ext: &str, counter: fn(&mut Walks) -> &mut u32) -> R<String> {
        loop {
            let n = counter(&mut self.walks);
            *n = n.checked_add(1).ok_or(WriteError::Limit("no part name was left"))?;
            let name = format!("{dir}{stem}{n}{ext}");
            crate::work::name_probe();
            let key = fold(&name);
            if !self.taken.contains(&key) && !self.reserved.contains(&key) {
                self.taken.insert(key);
                return Ok(name);
            }
        }
    }

    /// The first row of `source` that is of type `ty`, leads inside the package and to a name a part of the writer's can have, and the name, spelt as the model holds the part ([`Book::as_held`]).
    pub(super) fn usable(&self, source: &str, ty: &str) -> Option<String> {
        let rows = self.rows.get(&fold(source))?;
        let row = rows.first_of(ty)?;
        let name = self.as_held(resolve(source, &row.target)?);
        (plausible(&name) && !self.taken.contains(&fold(&name))).then_some(name)
    }

    /// `name`, a name a held row's target leads to, **spelt as the model holds the part it names**: OPC compares part names ASCII case-insensitively,
    /// so a target of `VISIO/Document.XML` reaches the part the package called `/visio/document.xml`, and the case a part arrived with is the case it keeps. A name the model knows a part by (the
    /// part of a page, a master or a theme, a held part, the source of held rows: [`Book::spelling`]) is that name; a name it knows no part by goes into the folder as the model's parts in that
    /// folder spell it, so that a part the save makes is not put in a second spelling of a folder beside the first; and a name in a folder the model has no part in is the name as it resolved.
    pub(super) fn as_held(&self, name: String) -> String {
        if let Some(known) = self.spelling.get(&fold(&name)) {
            return (*known).to_owned();
        }
        let cut = folder_of(&name).len();
        let (folder, file) = name.split_at_checked(cut).unwrap_or((name.as_str(), ""));
        match self.folders.get(&fold(folder)) {
            Some(spelt) if *spelt != folder => format!("{spelt}{file}"),
            _ => name,
        }
    }

    /// Takes `name` as a name the model knows a part by, spelt as it holds it, and its folder, unless a name or a folder of another spelling was taken first: the first in the order of the
    /// model, so that the names written do not hang on the order of a map.
    /// `key` is `name` folded, which the caller has made already.
    fn spelt(&mut self, key: &str, name: &'a str) {
        if self.spelling.contains_key(key) {
            return;
        }
        self.spelling.insert(key.to_owned(), name);
        let folder = folder_of(name);
        self.folders.entry(fold(folder)).or_insert(folder);
    }

    /// Makes the rows of `source` from the rows the model holds for it, if they are not made.
    pub(super) fn rows_of(&mut self, source: &str) -> &mut RowSet {
        let key = fold(source);
        let held = self.held.get(&key);
        self.rows.entry(key).or_insert_with(|| RowSet::of(held.map(|rows| rows.iter().copied()).into_iter().flatten()))
    }

    pub(super) fn warn(&mut self, text: String) {
        self.warnings.push(text);
    }

    // ------------------------------------------------------------------------------------------------------- what is held

    /// Takes the held parts' names and the held rows, and refuses a model past what a package is held to.
    fn claim_held(&mut self) -> R<()> {
        // The cheap refusals first: a count a model states is looked at before any of what it counts.
        if self.doc.parts.len() >= MAX_PARTS {
            return Err(WriteError::Limit("more parts than a package holds"));
        }
        if self.vsdx.is_some_and(|v| v.rels.len() > MAX_HELD_ROWS) {
            return Err(WriteError::Limit("more relationships held than the reader holds"));
        }
        let mut rows = 0usize;
        for part in self.doc.parts.iter().filter(|p| p.format == Format::Vsdx) {
            crate::work::model_part();
            if self.taken.insert(fold(&part.name)) {
                self.held_parts.push(part);
                rows = rows.saturating_add(part.rels.len());
                check_rows(&part.name, part.rels.len(), part.rels.iter().map(row_bytes))?;
            } else {
                self.warn("a held part was left out: another part has its name, which differs from it at most in case".to_owned());
            }
        }
        if self.held_parts.len() >= MAX_PARTS {
            return Err(WriteError::Limit("more parts than a package holds"));
        }
        if let Some(vsdx) = self.vsdx {
            let mut per_source: HashMap<String, (usize, u64)> = HashMap::new();
            for row in &vsdx.rels {
                crate::work::model_row();
                self.held.entry(fold(&row.source)).or_default().push(row);
                let (n, bytes) = per_source.entry(fold(&row.source)).or_default();
                *n = n.saturating_add(1);
                *bytes = bytes.saturating_add(row_bytes(row));
            }
            rows = rows.saturating_add(vsdx.rels.len());
            for (source, (n, bytes)) in &per_source {
                check_rows(source, *n, std::iter::once(*bytes))?;
            }
            for row in &vsdx.content_types {
                if let ContentTypeRow::Override { part, .. } = row {
                    self.reserved.insert(fold(&lenient_decode(part)));
                }
            }
        }
        if rows > MAX_HELD_ROWS {
            return Err(WriteError::Limit("more relationships held than the reader holds"));
        }
        self.reserve_names();
        Ok(())
    }

    /// **Reserves every name a part of the model already goes by, before anything is named**: the part of every page and master, the source and the internal target of every relationship the
    /// model holds (those of the held parts too), and the part of every theme. A name the writer makes is never one of them: a page that is added takes none of a page that was taken out
    /// (whose rows a held row would give it), and a part that is kept is not renamed because a part made before it took its name. Folded, as the package compares names.
    fn reserve_names(&mut self) {
        let doc = self.doc;
        for name in doc.pages.iter().filter_map(|p| p.part_name.as_deref()).chain(doc.masters.iter().filter_map(|m| m.part_name.as_deref())) {
            let key = fold(name);
            self.spelt(&key, name);
            self.reserved.insert(key);
        }
        for theme in doc.themes.iter() {
            let key = fold(&theme.part);
            self.spelt(&key, &theme.part);
            self.reserved.insert(key);
        }
        for part in doc.parts.iter().filter(|p| p.format == Format::Vsdx) {
            self.spelt(&fold(&part.name), &part.name);
        }
        let held = self.vsdx.map(|v| v.rels.iter()).into_iter().flatten();
        let of_parts = doc.parts.iter().filter(|p| p.format == Format::Vsdx).flat_map(|p| p.rels.iter());
        for row in held.chain(of_parts) {
            if &*row.source != "/" {
                let key = fold(&row.source);
                self.spelt(&key, &row.source);
                self.reserved.insert(key);
            }
            if !row.external
                && let Some(target) = resolve(&row.source, &row.target)
            {
                self.reserved.insert(fold(&target));
            }
        }
    }

    // ------------------------------------------------------------------------------------------ the document, the indexes, windows

    /// The name of the document part and the rows of the package and of the document.
    fn document_name(&mut self) -> R<()> {
        self.rows_of("/");
        let ty = rel_type(VIS, "document");
        let found = self.usable("/", &ty);
        let (name, existing) = match found {
            Some(name) => (name, true),
            None => (self.fresh_document_name()?, false),
        };
        self.claim(&name);
        self.document_part = name;
        self.rows_of(&self.document_part.clone());
        if !existing && !self.fresh {
            self.root_row(&ty, &self.document_part.clone());
        }
        let ct = self.target.content_type().to_owned();
        self.entries.push(Entry { name: self.document_part.clone(), ct, body: Body::Document });
        Ok(())
    }

    fn fresh_document_name(&mut self) -> R<String> {
        let preferred = "/visio/document.xml"; // brand-ok: a part name the format defines
        if !self.taken.contains(&fold(preferred)) {
            return Ok(preferred.to_owned());
        }
        self.fresh_name("/visio/", "document", ".xml", |w| &mut w.other) // brand-ok: a part name the format defines
    }

    /// A row of the package to the part `name`.
    fn root_row(&mut self, ty: &str, name: &str) {
        let target = name.trim_start_matches('/').to_owned();
        let rows = self.rows_of("/");
        if rows.add(ty, &target, false).is_none() {
            self.warn("a relationship of the package could not be given an id".to_owned());
        }
    }

    /// A row of the document to the part `name`.
    pub(super) fn document_row(&mut self, ty: &str, name: &str) {
        let source = self.document_part.clone();
        let target = relative_target(&source, name);
        let rows = self.rows_of(&source);
        if rows.add(ty, &target, false).is_none() {
            self.warn("a relationship of the document could not be given an id".to_owned());
        }
    }

    /// The part of the document's relationship of this type, or a new one in `dir` and the row to it: `(the name, whether it is new)`.
    pub(super) fn structural(&mut self, ty: &str, dir_file: (&str, &str, &str)) -> R<(String, bool)> {
        let document = self.document_part.clone();
        if let Some(name) = self.usable(&document, ty) {
            self.claim(&name);
            return Ok((name, true));
        }
        let name = self.fresh_structural_name(dir_file)?;
        self.document_row(ty, &name);
        Ok((name, false))
    }

    /// A new part for a relationship of the document of this type, in `dir`: a name no part has, and the row to it.
    pub(super) fn fresh_structural(&mut self, ty: &str, dir_file: (&str, &str, &str)) -> R<String> {
        let name = self.fresh_structural_name(dir_file)?;
        self.document_row(ty, &name);
        Ok(name)
    }

    /// The name of a new part of the document's own folder: the preferred one when no part has it, else the next free.
    fn fresh_structural_name(&mut self, dir_file: (&str, &str, &str)) -> R<String> {
        let document = self.document_part.clone();
        let (dir, stem, ext) = dir_file;
        let folder = format!("{}{dir}", folder_of(&document));
        let preferred = format!("{folder}{stem}{ext}");
        let name = if plausible(&preferred) && !self.taken.contains(&fold(&preferred)) && !self.reserved.contains(&fold(&preferred)) {
            self.claim(&preferred);
            preferred
        } else {
            self.fresh_name(&folder, stem, ext, |w| &mut w.other)?
        };
        Ok(name)
    }

    /// The pages index and the masters index: written when the model has pages or masters, or a row of the document says there is one.
    fn indexes(&mut self) -> R<()> {
        let pages_ty = rel_type(VIS, "pages");
        let document = self.document_part.clone();
        let have_pages = !self.doc.pages.is_empty() || self.usable(&document, &pages_ty).is_some();
        if have_pages {
            let (name, _) = self.structural(&pages_ty, ("pages/", "pages", ".xml"))?;
            self.rows_of(&name);
            self.entries.push(Entry { name: name.clone(), ct: CT_PAGES.to_owned(), body: Body::Pages });
            self.pages_index = Some(name);
        }
        Ok(())
    }

    /// The comments part (`comments.rs`): written when the model holds comments, or a relationship of the document names one (a part that was read is the model's, and
    /// its row is held: a save that left it out would leave the row with no part).
    ///
    /// **Only the part the model says it was read as** ([`diacraft_doc::Comments::part`]) is taken from the document's relationship of the type; a relationship leads wherever a file says, and a
    /// save that wrote the comments over a page or the document would destroy it. Without such a part the comments are a part of their own, with a relationship of their own. It is planned after the
    /// pages and the masters, so that their names are taken before it looks for its own.
    fn comments(&mut self) -> R<()> {
        let ty = crate::comments::comments_type();
        let document = self.document_part.clone();
        let read_as = self.doc.comments.part.as_deref().map(fold);
        let existing = self.usable(&document, &ty).filter(|name| read_as.as_deref() == Some(fold(name).as_str()));
        if self.doc.comments.is_empty() && existing.is_none() {
            return Ok(());
        }
        let name = match existing {
            Some(name) => {
                self.claim(&name);
                name
            }
            None => self.fresh_structural(&ty, ("", "comments", ".xml"))?,
        };
        self.entries.push(Entry { name, ct: CT_COMMENTS.to_owned(), body: Body::Comments });
        Ok(())
    }

    fn windows(&mut self) -> R<()> {
        let want = self.fresh || self.vsdx.is_some_and(|v| v.windows.is_some());
        if !want {
            return Ok(());
        }
        let ty = rel_type(VIS, "windows");
        let (name, _) = self.structural(&ty, ("", "windows", ".xml"))?;
        self.entries.push(Entry { name, ct: CT_WINDOWS.to_owned(), body: Body::Windows });
        Ok(())
    }

    // ----------------------------------------------------------------------------------------------------- macros and the rest

    /// A VBA project goes only into a macro-enabled type: the parts of it that the model holds (the project, its signature parts, anything typed as VBA), and the rows to them.
    fn macros(&mut self) -> R<()> {
        let found = self.macro_parts();
        if found.is_empty() || self.target.macro_enabled {
            return Ok(());
        }
        if !self.opts.drop_macros {
            return Err(WriteError::MacroProject);
        }
        let n = found.len();
        let names: Vec<String> = found.iter().map(|p| p.name.to_string()).collect();
        let gone: HashSet<String> = names.iter().map(|n| fold(n)).collect();
        self.held_parts.retain(|p| !gone.contains(&fold(&p.name)));
        let document = fold(&self.document_part);
        for key in ["/".to_owned(), document] {
            if let Some(rows) = self.rows.get_mut(&key) {
                rows.drop_where(|r| r.ty == VBA_PROJECT);
            }
        }
        let shown: Vec<&str> = names.iter().map(String::as_str).filter(|p| part_label("", p).as_ref() == *p).take(MACRO_NAMES_SHOWN).collect();
        let more = n.saturating_sub(shown.len());
        let list = match (shown.is_empty(), more > 0) {
            (_, false) => shown.join(", "),
            (true, true) => format!("{more} named by their owner"),
            (false, true) => format!("{} and {more} more", shown.join(", ")),
        };
        let kind = self.target.kind_word();
        self.warn(format!(
            "the macro project was left out of the new file, because a {kind} cannot hold one: {n} {} removed ({list})",
            if n == 1 { "part" } else { "parts" }
        ));
        self.macros_dropped = names;
        Ok(())
    }

    /// The held parts that are a macro project: reached by the package's or the document's relationship to one, reached from the project as its signature, or typed as VBA.
    fn macro_parts(&self) -> Vec<&'a Arc<Part>> {
        let by_name: HashMap<String, &'a Arc<Part>> = self.held_parts.iter().map(|p| (fold(&p.name), *p)).collect();
        let mut found: Vec<&'a Arc<Part>> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut add = |part: &'a Arc<Part>, found: &mut Vec<&'a Arc<Part>>| {
            if seen.insert(fold(&part.name)) {
                found.push(part);
            }
        };
        for source in ["/".to_owned(), self.document_part.clone()] {
            for row in self.held.get(&fold(&source)).into_iter().flatten().filter(|r| &*r.kind == VBA_PROJECT && !r.external) {
                if let Some(part) = resolve(&source, &row.target).and_then(|n| by_name.get(&fold(&n)).copied()) {
                    add(part, &mut found);
                }
            }
        }
        for project in found.clone() {
            for row in project.rels.iter().filter(|r| !r.external && r.kind.contains("/vbaProjectSignature")) {
                if let Some(part) = resolve(&project.name, &row.target).and_then(|n| by_name.get(&fold(&n)).copied()) {
                    add(part, &mut found);
                }
            }
        }
        for part in &self.held_parts {
            let by_role = matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData);
            if by_role || part.media_type.starts_with(VBA_CONTENT_TYPE) {
                add(part, &mut found);
            }
        }
        found
    }

    /// An edited save leaves the thumbnail out: its part and the package's relationship to it. The `Default` row for its extension stays (`decide_types` types it all the same).
    fn thumbnail(&mut self) {
        if !self.edited {
            return;
        }
        let mut gone: Vec<&'a Arc<Part>> = Vec::new();
        self.held_parts.retain(|p| {
            let is = p.role == PartRole::Thumbnail;
            if is {
                gone.push(p);
            }
            !is
        });
        if gone.is_empty() {
            return;
        }
        for part in gone {
            self.removed.push(part.name.to_string());
            self.types.thumbnails.push((part.name.to_string(), part.media_type.to_string()));
        }
        if let Some(rows) = self.rows.get_mut("/") {
            rows.drop_where(|r| r.ty == THUMBNAIL);
        }
    }

    /// The signatures of the file no longer verify, as the bytes they cover differ in a file written from the model: the parts are kept, and the save says so.
    fn signatures(&mut self) {
        let n =
            self.held_parts.iter().filter(|p| p.role == PartRole::Signature && p.media_type.to_ascii_lowercase().contains("xmlsignature")).count();
        if n > 0 {
            self.warn(format!("{n} digital signature(s) in this file no longer match it."));
        }
    }

    /// What a drawing or a template saved as a stencil should be told (as a save into a file tells it).
    fn stencil_notes(&mut self) {
        if self.target.kind != Kind::Stencil {
            return;
        }
        let with_shapes = self.doc.pages.iter().filter(|p| !p.shapes.is_empty()).count();
        if with_shapes > 0 {
            let (page, stays) = if with_shapes == 1 { ("page", "stays") } else { ("pages", "stay") };
            self.warn(format!(
                "saved as a stencil: the {with_shapes} {page} with shapes {stays} in the file, but a stencil offers its masters to other drawings, not its pages"
            ));
        }
        if self.doc.masters.is_empty() {
            self.warn("the stencil has no masters, so it offers no shapes to other drawings".to_owned());
        }
    }

    // ----------------------------------------------------------------------------------------------------------- properties

    /// The property parts: each is written when the package's relationships say there is one that the model rebuilds (a part the model could not read is a held part), or the model holds what it says.
    fn properties(&mut self) -> R<()> {
        let kinds = [
            (rel_type(PK, "core-properties"), "/docProps/core.xml", crate::props::CT_CORE),
            (rel_type(OD, "extended-properties"), "/docProps/app.xml", crate::props::CT_APP),
            (rel_type(OD, "custom-properties"), "/docProps/custom.xml", crate::props::CT_CUSTOM),
        ];
        for (at, (ty, default, ct)) in kinds.iter().enumerate() {
            let existing = self.usable("/", ty);
            let wanted = self.fresh || existing.is_some() || self.holds_properties(at) || (at == 2 && self.opts.recalc_document);
            // **A part the package named and did not have** (the reader's `-absent/` entry): a save of the drawing as it was read makes none, and the row stays
            // as the file had it. An edited save makes it, for the bookkeeping it writes (`Application`, `modified`).
            let word = ty.rsplit('/').next().unwrap_or("");
            let absent = self.vsdx.is_some_and(|v| v.props_rest.iter().any(|(k, _)| k.strip_prefix(crate::held::ABSENT_PROPERTY) == Some(word)));
            if !wanted || (absent && !self.edited) {
                continue;
            }
            let name = match existing {
                Some(name) => name,
                None => {
                    let name = if plausible(default) && !self.taken.contains(&fold(default)) {
                        (*default).to_owned()
                    } else {
                        self.fresh_name("/docProps/", "properties", ".xml", |w| &mut w.other)?
                    };
                    self.root_row(ty, &name);
                    name
                }
            };
            self.claim(&name);
            let body = [Body::Core, Body::App, Body::Custom].get(at).copied().unwrap_or(Body::Core);
            match at {
                0 => self.core = Some(name.clone()),
                1 => self.app = Some(name.clone()),
                _ => self.custom = Some(name.clone()),
            }
            self.entries.push(Entry { name, ct: (*ct).to_owned(), body });
        }
        if self.fresh {
            // The document's own relationship comes last of the package's, as a new drawing's does.
            let ty = rel_type(VIS, "document");
            let has = self.rows.get("/").is_some_and(|rows| rows.rows.iter().any(|r| r.ty == ty));
            if !has {
                self.root_row(&ty, &self.document_part.clone());
            }
        }
        Ok(())
    }

    /// Whether the model holds something one of the three property parts says, so that a part is made for it when the package has none: the title and the fields of the core part, the fields of
    /// the app part, a property of the custom part. (`kind` is 0 for core, 1 for app and 2 for custom.)
    fn holds_properties(&self, kind: usize) -> bool {
        let p = &self.doc.props;
        let rest = |key: &str| self.vsdx.is_some_and(|v| v.props_rest.iter().any(|(k, _)| &**k == key));
        match kind {
            0 => {
                !self.doc.title.is_empty()
                    || [
                        &p.subject,
                        &p.creator,
                        &p.keywords,
                        &p.description,
                        &p.last_modified_by,
                        &p.last_printed,
                        &p.created,
                        &p.modified,
                        &p.category,
                        &p.language,
                    ]
                    .iter()
                    .any(|f| f.is_some())
                    || rest("coreProperties")
            }
            1 => [&p.company, &p.manager, &p.hyperlink_base, &p.app_version].iter().any(|f| f.is_some()) || rest("Properties"),
            _ => p.custom.iter().any(|c| c.format == Format::Vsdx),
        }
    }

    // ------------------------------------------------------------------------------------------------------------------ themes

    fn themes(&mut self) -> R<()> {
        if self.doc.themes.len() > crate::limits::MAX_THEMES {
            return Err(WriteError::Limit("theme parts"));
        }
        let ty = rel_type(OD, "theme");
        let document = self.document_part.clone();
        let rows = self.rows_of(&document);
        let by_target: HashSet<String> =
            rows.rows.iter().filter(|r| r.ty == ty && !r.external).filter_map(|r| resolve(&document, &r.target)).map(|t| fold(&t)).collect();
        for theme in self.doc.themes.iter() {
            crate::work::model_part();
            let key = fold(&theme.part);
            // A theme keeps the part name it has, and gets a relationship when the file's document part did not reach it (the reader found an orphan
            // by its content type): the writer must not rename a part the model holds (`themes.rs` for a splice).
            let known = plausible(&theme.part) && !self.taken.contains(&key);
            let name = if known {
                self.claim(&theme.part);
                if !by_target.contains(&key) {
                    self.document_row(&ty, &theme.part);
                }
                theme.part.to_string()
            } else {
                let folder = format!("{}theme/", folder_of(&document));
                let name = self.fresh_name(&folder, "theme", ".xml", |w| &mut w.theme)?;
                self.document_row(&ty, &name);
                name
            };
            self.entries.push(Entry { name, ct: CT_THEME.to_owned(), body: Body::Theme(theme) });
        }
        Ok(())
    }

    // ----------------------------------------------------------------------------------------------------- pages and masters

    fn pages_and_masters(&mut self) -> R<()> {
        // The masters first: the rows of a page to the masters its shapes use name their parts.
        let ty = rel_type(VIS, "masters");
        let document = self.document_part.clone();
        if !self.doc.masters.is_empty() || self.usable(&document, &ty).is_some() {
            let (name, _) = self.structural(&ty, ("masters/", "masters", ".xml"))?;
            self.rows_of(&name);
            self.entries.push(Entry { name: name.clone(), ct: CT_MASTERS.to_owned(), body: Body::Masters });
            self.masters_index = Some(name);
        }
        let doc = self.doc;
        let mut claimed: HashMap<String, Content<'a>> = HashMap::new();
        if let Some(index) = self.masters_index.clone() {
            let items = doc.masters.iter().enumerate().map(|(i, m)| MasterItem { at: i, master: m });
            self.place_all(&index, "master", items, &mut claimed, true)?;
        }
        let mut claimed: HashMap<String, Content<'a>> = HashMap::new();
        if let Some(index) = self.pages_index.clone() {
            let items = doc.pages.iter().enumerate().map(|(i, p)| PageItem { at: i, page: p });
            self.place_all(&index, "page", items, &mut claimed, false)?;
        }
        Ok(())
    }

    /// Places each page or master: the part it is written as, the row that names it, and whether it is the one that writes the part.
    fn place_all<I: Item<'a>>(
        &mut self,
        index: &str,
        word: &str,
        items: impl Iterator<Item = I>,
        claimed: &mut HashMap<String, Content<'a>>,
        masters: bool,
    ) -> R<()> {
        let ty = rel_type(VIS, word);
        let folder = folder_of(index).to_owned();
        let mut ids: HashSet<String> = HashSet::new();
        let mut parts: HashSet<String> = HashSet::new();
        for item in items {
            crate::work::model_part();
            let needs = item.needs_part(self.target.kind != Kind::Stencil);
            let part = if needs { Some(self.part_for(&item, &folder, word, claimed, masters)?) } else { None };
            let rel = self.row_for(index, &ty, item.rel_id(), part.as_ref().map(|(name, _)| name.as_str()))?;
            if let Some(id) = &rel {
                ids.insert(id.to_ascii_lowercase());
            }
            if let Some((name, first)) = &part {
                let map = if masters { &mut self.master_part_of } else { &mut self.page_part_of };
                map.entry(item.id()).or_insert_with(|| name.clone());
                parts.insert(fold(name));
                if masters {
                    self.written_masters.insert(fold(name));
                } else {
                    self.written_pages.insert(fold(name));
                    if item.background() {
                        self.background_pages.insert(fold(name));
                    }
                }
                if *first {
                    // **The rows of a part are the rows of the part the item had** (its own `part_name`), and no other's: a name the writer gives a new item is never one a held row names (they are
                    // reserved), and an item that is written under another name than it had carries its rows over, their targets taken to where they lead from the new name.
                    let rows = self.rows_of_item(item.part_name(), name);
                    self.rows.insert(fold(name), rows);
                    self.entries.push(Entry { name: name.clone(), ct: item.ct().to_owned(), body: item.body() });
                }
            }
            if masters { self.master_rels.push(rel) } else { self.page_rels.push(rel) }
        }
        // A row of the index to a page or master that is not written, and that no item of the model names, is the row of one that was taken out: it is not written.
        let rows = self.rows_of(index);
        rows.drop_where(|r| {
            r.ty == ty
                && !r.external
                && !ids.contains(&r.id.to_ascii_lowercase())
                && resolve(index, &r.target).is_some_and(|target| !parts.contains(&fold(&target)))
        });
        Ok(())
    }

    /// The rows of the part a page or master had (`had`, the name it was read under), made the rows of the part it is written as (`written`): the same rows, with the targets of those that lead inside
    /// the package spelt from the new name when it is another.
    fn rows_of_item(&self, had: Option<&str>, written: &str) -> RowSet {
        let Some(had) = had else { return RowSet::default() };
        let held = self.held.get(&fold(had)).into_iter().flatten().copied();
        if fold(had) == fold(written) {
            return RowSet::of(held);
        }
        let mut set = RowSet::default();
        for row in held {
            let target = match (row.external, resolve(had, &row.target)) {
                (false, Some(leads_to)) => relative_target(written, &leads_to),
                _ => row.target.to_string(),
            };
            set.push(RelRow { id: row.id.to_string(), ty: row.kind.to_string(), target, external: row.external });
        }
        set
    }

    /// The part a page or master is written as: the name it had, when it is a name the writer can use (a second that names the part has its content, or gets a part of its own), else a new one.
    fn part_for<I: Item<'a>>(
        &mut self,
        item: &I,
        folder: &str,
        word: &str,
        claimed: &mut HashMap<String, Content<'a>>,
        masters: bool,
    ) -> R<(String, bool)> {
        if let Some(name) = item.part_name().filter(|n| plausible(n)) {
            let key = fold(name);
            match claimed.get(&key) {
                Some(same) if *same == item.content() => return Ok((name.to_owned(), false)),
                Some(_) => {}
                None if !self.taken.contains(&key) => {
                    self.taken.insert(key.clone());
                    claimed.insert(key, item.content());
                    return Ok((name.to_owned(), true));
                }
                None => {}
            }
        }
        let name = self.fresh_name(folder, word, ".xml", if masters { |w| &mut w.master } else { |w| &mut w.page })?;
        claimed.insert(fold(&name), item.content());
        Ok((name, true))
    }

    /// The row of the index that names the part (`None` for an item with no part): the id the item had when its row is the item's, else a new one.
    fn row_for(&mut self, index: &str, ty: &str, had: Option<&str>, part: Option<&str>) -> R<Option<String>> {
        let rows = self.rows_of(index);
        let Some(part) = part else { return Ok(had.map(str::to_owned)) };
        let target = relative_target(index, part);
        let row = RelRow { id: String::new(), ty: ty.to_owned(), target: target.clone(), external: false };
        if let Some(id) = had {
            match rows.get(id) {
                None => {
                    rows.push(RelRow { id: id.to_owned(), ..row });
                    return Ok(Some(id.to_owned()));
                }
                Some(existing)
                    if existing.ty == ty && !existing.external && resolve(index, &existing.target).is_some_and(|t| fold(&t) == fold(part)) =>
                {
                    return Ok(Some(id.to_owned()));
                }
                Some(_) => {}
            }
        }
        let id = rows.fresh_id().ok_or(WriteError::Limit("no relationship id was left for a page or master"))?;
        rows.push(RelRow { id: id.clone(), ..row });
        Ok(Some(id))
    }

    // --------------------------------------------------------------------------------------------------------------- held

    fn held_entries(&mut self) {
        for part in std::mem::take(&mut self.held_parts) {
            self.entries.push(Entry { name: part.name.to_string(), ct: part.media_type.to_string(), body: Body::Held(part) });
        }
    }

    /// A relationship from a part that is not written (a deleted page's) is not written, and a part that only such a row reached, of a kind a row takes with it, is left out.
    fn orphans(&mut self) {
        let written: HashSet<String> = std::iter::once("/".to_owned()).chain(self.entries.iter().map(|e| fold(&e.name))).collect();
        let mut candidates: Vec<String> = Vec::new();
        for (source, rows) in &self.held {
            if written.contains(source) {
                continue;
            }
            for row in rows.iter().filter(|r| !r.external && REMOVABLE.iter().any(|k| r.kind.ends_with(k))) {
                if let Some(target) = resolve(source, &row.target) {
                    candidates.push(fold(&target));
                }
            }
        }
        if candidates.is_empty() {
            return;
        }
        let mut reached: HashSet<String> = HashSet::new();
        for (source, rows) in &self.held {
            if written.contains(source) {
                reached.extend(rows.iter().filter(|r| !r.external).filter_map(|r| resolve(source, &r.target)).map(|t| fold(&t)));
            }
        }
        for entry in &self.entries {
            if let Body::Held(part) = entry.body {
                reached.extend(part.rels.iter().filter(|r| !r.external).filter_map(|r| resolve(&part.name, &r.target)).map(|t| fold(&t)));
            }
        }
        for (source, rows) in &self.rows {
            reached.extend(rows.rows.iter().filter(|r| !r.external).filter_map(|r| resolve(source, &r.target)).map(|t| fold(&t)));
        }
        let gone: HashSet<String> = candidates.into_iter().filter(|c| !reached.contains(c)).collect();
        let removed = &mut self.removed;
        self.entries.retain(|e| {
            let goes = matches!(e.body, Body::Held(_)) && gone.contains(&fold(&e.name));
            if goes {
                removed.push(e.name.clone());
            }
            !goes
        });
    }
}

// ----------------------------------------------------------------------------------------------------------------- the items

/// What the writer needs of a page or a master to place it.
pub(super) trait Item<'a> {
    fn id(&self) -> u32;
    fn rel_id(&self) -> Option<&'a str>;
    fn part_name(&self) -> Option<&'a str>;
    fn content(&self) -> Content<'a>;
    fn ct(&self) -> &'static str;
    fn body(&self) -> Body<'a>;
    /// Whether the item has a part of its own in the file written: it had one, or has content, or is a page of a drawing (a stencil's pages have none).
    fn needs_part(&self, drawing: bool) -> bool;
    /// Whether the item is a background page.
    fn background(&self) -> bool {
        false
    }
}

struct PageItem<'a> {
    at: usize,
    page: &'a Arc<Page>,
}

struct MasterItem<'a> {
    at: usize,
    master: &'a Arc<Master>,
}

impl<'a> Item<'a> for PageItem<'a> {
    fn id(&self) -> u32 {
        self.page.id
    }
    fn rel_id(&self) -> Option<&'a str> {
        self.page.rel_id.as_deref()
    }
    fn part_name(&self) -> Option<&'a str> {
        self.page.part_name.as_deref()
    }
    fn content(&self) -> Content<'a> {
        Content { shapes: &self.page.shapes, connects: &self.page.connects }
    }
    fn ct(&self) -> &'static str {
        CT_PAGE
    }
    fn body(&self) -> Body<'a> {
        Body::Page(self.at)
    }
    fn background(&self) -> bool {
        self.page.background
    }
    fn needs_part(&self, drawing: bool) -> bool {
        self.page.part_name.is_some() || !self.page.shapes.is_empty() || !self.page.connects.is_empty() || (self.page.rel_id.is_none() && drawing)
    }
}

impl<'a> Item<'a> for MasterItem<'a> {
    fn id(&self) -> u32 {
        self.master.id
    }
    fn rel_id(&self) -> Option<&'a str> {
        self.master.rel_id.as_deref()
    }
    fn part_name(&self) -> Option<&'a str> {
        self.master.part_name.as_deref()
    }
    fn content(&self) -> Content<'a> {
        Content { shapes: &self.master.shapes, connects: &self.master.connects }
    }
    fn ct(&self) -> &'static str {
        CT_MASTER
    }
    fn body(&self) -> Body<'a> {
        Body::Master(self.at)
    }
    fn needs_part(&self, _: bool) -> bool {
        self.master.part_name.is_some() || !self.master.shapes.is_empty() || !self.master.connects.is_empty()
    }
}

// ------------------------------------------------------------------------------------------------------------ the checks of rows

/// The bytes a held row takes in the relationships part it is written in, near enough: its three attributes and the element around them.
fn row_bytes(row: &Rel) -> u64 {
    // `<Relationship Id="" Type="" Target=""/>` is 39 bytes beside its three values, and ` TargetMode="External"` 22 more.
    (row.id.len() + row.kind.len() + row.target.len() + 39 + if row.external { 22 } else { 0 }) as u64
}

/// A relationships part is held to the rows and the bytes a read of the package takes of one (`MAX_RELATIONSHIPS`, `MAX_PACKAGE_XML_BYTES`): counted from the model's own lengths, before any row is made.
fn check_rows(source: &str, rows: usize, bytes: impl Iterator<Item = u64>) -> R<()> {
    if rows > MAX_RELATIONSHIPS {
        return Err(WriteError::Limit("more relationships from one part than a package holds"));
    }
    let total = bytes.fold(0u64, u64::saturating_add);
    if total > MAX_PACKAGE_XML_BYTES {
        let _ = source;
        return Err(WriteError::Limit("the relationships from one part would be larger than a package allows for them"));
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------ the content types

/// The rows of the content types: what each part is typed by.
#[derive(Default)]
pub(super) struct TypeBook {
    /// The `Default` rows: the two every package has, the held rows, and those the writer chose.
    pub defaults: Vec<(String, String)>,
    /// The `Default`s the writer chose and the type of each, by extension in lower case.
    chosen: HashMap<String, String>,
    /// The extensions of the `Default` rows the model holds: a part of such an extension is typed by an `Override`.
    held_defaults: HashSet<String>,
    pub overrides: Vec<(String, String)>,
    /// The thumbnails an edited save leaves out, whose `Default` rows stay.
    pub thumbnails: Vec<(String, String)>,
}

impl TypeBook {
    /// How a part named `name` of type `ct` is typed: a `Default` of its extension (a row is made once), or an `Override` (`emit` says whether it is written: a part left out has no row of its own).
    fn part(&mut self, name: &str, ct: &str, emit: bool) {
        crate::work::model_type();
        let ct = if ct.is_empty() { super::CT_OCTET } else { ct };
        let extension = extension_of(name);
        let by_default = match &extension {
            Some(ext) => self.by_default(ext, ct),
            None => false,
        };
        if !by_default && emit {
            self.overrides.push((name.to_owned(), ct.to_owned()));
        }
    }

    /// Whether the extension's `Default` gives `ct` (made if the writer chooses one): the two standard ones, a type that is no XML (a picture, a `.bin`, a signature origin) when no held `Default` of the
    /// extension says that every part had an `Override`.
    fn by_default(&mut self, ext: &str, ct: &str) -> bool {
        if self.held_defaults.contains(ext) {
            return false;
        }
        if let Some((_, existing)) = self.defaults.iter().find(|(e, _)| e.eq_ignore_ascii_case(ext) && (ext == "rels" || ext == "xml")) {
            return existing.eq_ignore_ascii_case(ct);
        }
        if let Some(existing) = self.chosen.get(ext) {
            return existing.eq_ignore_ascii_case(ct);
        }
        let lower = ct.to_ascii_lowercase();
        // The format's own parts and the property parts are XML and typed by an `Override`; a signature of a macro project is too.
        let xml = lower.ends_with("xml") || lower.starts_with("application/vnd.ms-office.vbaprojectsignature");
        if xml {
            return false;
        }
        self.chosen.insert(ext.to_owned(), ct.to_owned());
        self.defaults.push((ext.to_owned(), ct.to_owned()));
        true
    }
}

impl Book<'_> {
    /// Types every part: the two `Default` rows every package has, the rows the model holds, then a row for each part, and the `Default` of a thumbnail that is left out.
    fn decide_types(&mut self) -> R<()> {
        let mut types = TypeBook::default();
        types.thumbnails = std::mem::take(&mut self.types.thumbnails);
        types.defaults.push(("rels".to_owned(), super::parts::CT_RELS.to_owned()));
        types.defaults.push(("xml".to_owned(), super::parts::CT_XML.to_owned()));
        if let Some(vsdx) = self.vsdx {
            for row in &vsdx.content_types {
                if let ContentTypeRow::Default { extension, content_type } = row {
                    types.held_defaults.insert(extension.to_ascii_lowercase());
                    types.defaults.push((extension.to_string(), content_type.to_string()));
                }
            }
        }
        // The parts the model holds are typed first: a package's own `Default` for an extension is the type of its parts, and a part the writer makes (an embedding, a picture) of that
        // extension with another type is the one that gets an `Override`.
        for entry in self.entries.iter().filter(|e| matches!(e.body, Body::Held(_))) {
            types.part(&entry.name, &entry.ct, true);
        }
        for entry in self.entries.iter().filter(|e| !matches!(e.body, Body::Held(_))) {
            types.part(&entry.name, &entry.ct, true);
        }
        let thumbnails = std::mem::take(&mut types.thumbnails);
        for (name, ct) in &thumbnails {
            types.part(name, ct, false);
        }
        types.thumbnails = thumbnails;
        let rows = types.defaults.len() + types.overrides.len() + self.vsdx.map_or(0, |v| v.content_types.len());
        if rows > MAX_CONTENT_TYPE_ENTRIES {
            return Err(WriteError::Limit("more rows of content types than a package holds"));
        }
        self.types = types;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    #[test]
    fn the_parts_a_save_would_make_are_refused_from_one_more_than_a_package_holds() {
        // The entries, the relationships parts of the ones that have rows, and the content types make the package: the most it holds is `MAX_PARTS`.
        assert!(!parts_refused(MAX_PARTS - 2, 1), "the package of exactly the most parts is written");
        assert!(parts_refused(MAX_PARTS - 1, 1), "one more is refused");
        assert!(parts_refused(MAX_PARTS - 2, 2));
        assert!(!parts_refused(0, 0));
        assert!(parts_refused(usize::MAX, usize::MAX), "a count that does not fit is a count past the most");
    }

    #[test]
    fn a_save_is_refused_from_one_byte_more_than_the_address_space_has_for_it() {
        let space = 20 * 1024 * MIB;
        // 3 times the bytes the parts hold and the margin make `19 GiB`, which a twentieth less than the space is.
        let held = (19 * 1024 * MIB - PEAK_MARGIN_BYTES) / 3;
        assert!(!peak_refused(held, space), "exactly what is left is written");
        assert!(peak_refused(held + 1, space), "a byte more of the parts");
        assert!(!peak_refused(0, space));
        assert!(peak_refused(u64::MAX, space), "the sums do not overflow");
    }

    #[test]
    fn the_bytes_a_part_is_written_as_are_a_held_parts_a_pictures_and_a_themes() {
        let part = Arc::new(Part {
            format: Format::Vsdx,
            name: Arc::from("/p.bin"),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: diacraft_doc::RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(&[0u8; 7][..]),
        });
        let media = Arc::new(Media { part: Arc::from("/m.png"), content_type: Arc::from("image/png"), bytes: Arc::from(&[0u8; 11][..]) });
        let plans = [MediaPlan { name: "/m.png".to_owned(), media: &media }];
        let theme = Arc::new(Theme { bytes: Arc::from(&[0u8; 13][..]), ..Theme::default() });
        assert_eq!(body_len(Body::Held(&part), &plans, &[]), 7);
        assert_eq!(body_len(Body::Media(0), &plans, &[]), 11);
        assert_eq!(body_len(Body::Media(1), &plans, &[]), 0, "a picture that is not planned has no bytes");
        assert_eq!(body_len(Body::Theme(&theme), &plans, &[]), 13);
        assert_eq!(body_len(Body::Core, &plans, &[]), 0, "a part made from fields is not held bytes");
        assert_eq!(body_len(Body::Page(0), &plans, &[]), 0);
    }
}
