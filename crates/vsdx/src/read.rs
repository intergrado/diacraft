//! Reading a package into a document: the order of it, and what each part gives.
//!
//! 1. The package is read by `opc` within its limits. Whatever it refuses is the answer, with its class.
//! 2. **The document part is found by relationship**, from the package's `document` relationship, and never by its name. No such
//!    relationship, no part at its target, or a root that is not `VisioDocument` is `no_drawing_parts`; XML that is not well-formed is the XML
//!    class. Both VSDX namespaces are read as one vocabulary (`ns.rs`).
//! 3. The document part gives the colour and font tables, the style sheets and the document sheet; every other child (`DocumentSettings`,
//!    `EventList`, ...) is kept in the document's passthrough. Then the masters (so that a shape can be checked against them), then the pages,
//!    the themes, the properties, and the DiaCraft solution if there is one.
//! 4. **A part that is not XML this reader accepts refuses the file**, whichever part it is: not well-formed (`xml_malformed`), a DTD or an
//!    entity (`xml_entity_refused`), or past an XML limit (`xml_too_complex`). A page that opened empty with a warning that no one reads
//!    would look like a drawing with nothing on it. What opens and says so is a part that is *missing* (a dangling `Rel`, E1: a page with no
//!    shapes) or whose root is not the expected one (read as unknown, kept, warned of).
//!
//! **Caps** are `Limits`', applied before what they count is stored (see `limits.rs`). **Names, IDs and links from the file are hostile**: a
//! style, master or page `ID` used twice keeps both and finds the first, with a warning; a link to a style or master that is not there is read as
//! it is and warned of; a style link cycle is found once, in linear time, and warned of; the resolver walks at most 64 steps, so none of these loops.
//! A master cannot name a master (`shape.rs`), so no master cycle exists.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use diacraft_doc::{
    ColorEntry, Document, Ext, FaceName, Kept, MAX_COLOR_ENTRIES, Master, OdfDoc, Page, Passthrough, RawAttrs, SharedVec, Style, Theme, check_text,
};
use diacraft_opc::xml::{El, Node};
use diacraft_opc::{Package, Rel, Warning as OpcWarning};
use diacraft_sheetxml::model::cost::push_bytes;
use diacraft_sheetxml::price::{self as price, arc_str, heap, kept_bytes};

use crate::ahead::{Ahead, Closer, Hook, Lookahead, Taken};
use crate::cost::{self, MASTER_BYTES, PAGE_BYTES, PARSED_BYTES, REL_ROW_BYTES, STYLE_BYTES, TABLE_ROW_BYTES};
use crate::error::{LimitKind, VsdxError};
use crate::held::{Derived, Unparsed};
use crate::limits::Limits;
use crate::ns::{Scope, ScopeExt};
use crate::passthrough::{element_cost, read_flag, read_u32};
use crate::props::{APP_READ, CORE_READ, CT_APP, CT_CORE, Props, is_standard_app, is_standard_core, property_part, read_app, read_core};
use crate::reader::{Parsed, Reader, RelMap};
use crate::report::{DiacraftPart, ReadReport, Variant};
use crate::shape::PartState;
use crate::sheet::{SheetTally, Took, style_link};
use crate::theme::{CONVENTIONAL_THEME, CT_THEME, read_theme};
use crate::unheld;

const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines
const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const PK: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/";

fn rel_type(base: &str, word: &str) -> String {
    format!("{base}{word}")
}

/// Reads the package in `bytes` into a document within `limits`, and says what it made of it. See the module documentation.
///
/// The result is all of it or nothing: a file past a cap, or not a package, or whose document part is not XML we accept, is an error and no
/// document. What cannot go in the model (`Skipped`, counted by class, exactly) and what was odd (`warnings`) are in the report, and the file is
/// read all the same.
///
/// **The model holds what the file has that no field of it reads** (`held.rs` and `unheld.rs`): the parts of the package it does not rebuild (a VBA
/// project, an embedded object, the thumbnail) as `Document::parts`, the relationships, content-type rows, property parts and `windows.xml` no field says as
/// `Document::vsdx`, and the elements and attributes of each place it types and has no field for in the `vsdx` side of the extension slots. The `Package` is dropped when
/// this returns, and nothing a read leaves out is only in `bytes`. A caller that saves into the same file still keeps `bytes` (the engine's session holds
/// them behind an `Arc`, with the path, and the splice writer takes every part it does not edit from them). All of it is charged to the read's budget before it is kept.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<(Document, ReadReport), VsdxError> {
    let (result, report) = read_partial(bytes, limits);
    result.map(|doc| (doc, report))
}

/// As [`read`], and the report is given whether or not the file was read: **a refused file reports what the reader knew when it refused it**
/// (its size, the rows of its archive, the sizes of the parts it got to, and the budgets spent), which is what the sample-file run needs to move a
/// cap that a real file hit. The figures of a refused file are as far as the reader got and no further.
pub fn read_partial(bytes: &[u8], limits: &Limits) -> (Result<Document, VsdxError>, ReadReport) {
    read_with_parts(bytes, limits, true, None, None)
}

/// The bytes of the parts a model holds (`Document::parts`, VSDX's), by folded name: what a save wrote them from, so that the check of the file it made and the read of the file it splices
/// into **share them and do not inflate a copy beside them** (`Package::read_knowing`). A part of the file is the model's bytes only when its entry holds them (their
/// size and checksum); any other is inflated.
pub(crate) struct Known {
    by_name: HashMap<String, Arc<[u8]>>,
}

impl Known {
    /// The held parts of `doc`, the first of a name (ASCII case-insensitively) winning, as the writers write the first.
    pub(crate) fn of(doc: &Document) -> Known {
        let mut by_name: HashMap<String, Arc<[u8]>> = HashMap::with_capacity(doc.parts.len());
        for part in doc.parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx && !p.bytes.is_empty()) {
            by_name.entry(diacraft_opc::name::fold(&part.name)).or_insert_with(|| Arc::clone(&part.bytes));
        }
        Known { by_name }
    }

    /// Adds parts a save made from bytes of the model's: the parts of DiaCraft's own that carry the data of the other format (`carry.rs`), by name.
    pub(crate) fn with_parts<'n>(mut self, parts: impl IntoIterator<Item = (&'n str, &'n Arc<[u8]>)>) -> Known {
        for (name, bytes) in parts {
            if !bytes.is_empty() {
                self.by_name.entry(diacraft_opc::name::fold(name)).or_insert_with(|| Arc::clone(bytes));
            }
        }
        self
    }

    pub(crate) fn get(&self, name: &str) -> Option<Arc<[u8]>> {
        if self.by_name.is_empty() {
            return None;
        }
        self.by_name.get(&diacraft_opc::name::fold(name)).cloned()
    }

    /// Reads the package in `bytes`, sharing these bytes where its entries hold them.
    pub(crate) fn package(&self, bytes: &[u8], limits: &diacraft_opc::Limits) -> Result<Package, diacraft_opc::OpcError> {
        if self.by_name.is_empty() {
            return Package::read(bytes, limits);
        }
        Package::read_knowing(bytes, limits, &|name| self.get(name))
    }
}

/// What [`read_back`] gives: the document, the report, and the names of the parts of the package, folded.
pub(crate) struct ReadBack {
    pub doc: Document,
    pub report: ReadReport,
    pub names: Vec<String>,
}

/// The read a save checks the file it made with: the same read, **but the parts the model holds as bytes are not moved out of the package into the document** (`Document::parts` is left empty). The
/// bytes of a held part are the caller's model's already, and a document made from the file written would hold them a second time, beside the model, for as long as the engine keeps it as the
/// baseline. **Nor does the package inflate a copy of them** (`known`, the model's parts): an entry that holds the bytes of the model's part is that part, checked by its size
/// and checksum as it is read and kept nowhere, so that the check holds the parts once, as the model does. What the check needs of the file is that it reads, what it holds, and the names of
/// its parts.
pub(crate) fn read_back(bytes: &[u8], limits: &Limits, known: &Known) -> Result<ReadBack, VsdxError> {
    let mut names = Vec::new();
    let (result, report) = read_with_parts(bytes, limits, false, Some(known), Some(&mut names));
    result.map(|doc| ReadBack { doc, report, names })
}

fn read_with_parts(
    bytes: &[u8],
    limits: &Limits,
    move_held: bool,
    known: Option<&Known>,
    names: Option<&mut Vec<String>>,
) -> (Result<Document, VsdxError>, ReadReport) {
    // The threads of this read: its own and the ones that work ahead (`ahead.rs`), as many as its caller asks and its address space has room for beside the budget it may hold; none on the web.
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { diacraft_container::spare::threads_beside(limits.threads, limits.max_memory) };
    let read = match known {
        Some(known) => known.package(bytes, &limits.opc),
        None => Package::read_threads(bytes, &limits.opc, threads),
    };
    let mut pkg = match read {
        Ok(pkg) => pkg,
        Err(e) => {
            let mut report = ReadReport::default();
            report.stats.file_bytes = bytes.len() as u64;
            return (Err(VsdxError::Package(e)), report);
        }
    };
    // The reader borrows the package for the whole read; when it is done, the bytes of the parts the document holds move out of the package into it (`held::move_parts`).
    let workers = (threads - 1).min(AHEAD_THREADS_MOST);
    let ahead: Option<Arc<Ahead>> = (workers > 0).then(|| Arc::new(Ahead::new(limits.opc.xml.clone())));
    let pkg_ref = &pkg;
    let (outcome, mut report, pending) = std::thread::scope(|scope| {
        // Whatever ends the read (it returns, or it unwinds), the threads are told to go, so that the scope can join them.
        let _closer = ahead.as_deref().map(Closer);
        let mut started = 0usize;
        for _ in 0..workers {
            let Some(work) = ahead.as_ref().map(Arc::clone) else { break };
            // A thread that cannot be made is a thread that is not there: the parts wait for the read, which parses them.
            let spawned = std::thread::Builder::new()
                .name("diacraft-vsdx-ahead".to_owned())
                .spawn_scoped(scope, move || work.serve(&|name| pkg_ref.part(name)));
            if spawned.is_err() {
                break;
            }
            started += 1;
        }
        let mut reader = Reader::new(pkg_ref, limits, bytes.len());
        if let Some(ahead) = &ahead
            && started > 0
        {
            reader.look = Some(Lookahead::new(Arc::clone(ahead), started));
            reader.sheets.set_lookahead_hook(Arc::new(Hook(Arc::clone(ahead))));
        }
        reader.package_warnings(pkg_ref.warnings());
        // The model is whole; then the room for the move of the held parts is looked for, as the last thing the budget is asked.
        let read = reader.read_document().and_then(|doc| if move_held { reader.room_to_move().map(|()| doc) } else { Ok(doc) });
        match read {
            Ok(doc) => {
                let pending = std::mem::take(&mut reader.held.pending);
                let report = reader.finish(Some(&doc), bytes.len());
                (Ok(doc), report, pending)
            }
            Err(e) => {
                let mut report = reader.finish(None, bytes.len());
                report.refused_at = Some(crate::report::RefusedAt {
                    part: reader.current_part.as_deref().unwrap_or("").to_owned(),
                    section: reader.sheets.current_section(),
                });
                // A refusal for memory says the budget of the read, whichever of the charges made it (the pool is what is left of it).
                let e = match e {
                    VsdxError::Limit { kind: LimitKind::Memory, .. } => VsdxError::Limit { kind: LimitKind::Memory, max: limits.max_memory },
                    other => other,
                };
                (Err(e), report, Vec::new())
            }
        }
    });
    drop(ahead);
    if let Some(names) = names {
        names.extend(pkg.parts().map(diacraft_opc::name::fold));
    }
    match outcome {
        Ok(mut doc) => {
            if move_held {
                crate::held::move_parts(&mut pkg, pending, &mut doc, &mut report);
            }
            // The cap of every sheet and the document as a whole, once more over what was built: the reader keeps each cap as it builds, so
            // this finds nothing, and a document that fails it must not be handed out.
            match doc.check_limits() {
                Ok(()) => (Ok(doc), report),
                Err(e) => (Err(VsdxError::Model(e)), report),
            }
        }
        Err(e) => (Err(e), report),
    }
}

/// The most threads that work ahead on the parts of a read: the read builds the model on its own thread and is the one that limits the speed, so a few threads keep ahead of it (`ahead.rs`).
const AHEAD_THREADS_MOST: usize = 3;

/// How a theme part was found: by a relationship of the document part, or, where the document part has no theme relationship at all, by its content type or by
/// the place themes have (`Reader::read_themes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeFound {
    Relationship,
    ContentType,
    Place,
}

impl<'p> Reader<'p> {
    /// The package's own warnings, as classes (`opc` words them; this counts them).
    fn package_warnings(&mut self, warnings: &[OpcWarning]) {
        for w in warnings {
            self.opc_warning(w);
        }
    }

    fn opc_warning(&mut self, w: &OpcWarning) {
        let class = match w {
            OpcWarning::BytesAfterEndRecord { .. } => "opc_bytes_after_end_record",
            OpcWarning::DirectoryItem { .. } | OpcWarning::MoreDirectoryItems { .. } => "opc_directory_item",
            OpcWarning::OrphanRelationships { .. } | OpcWarning::MoreOrphanRelationships { .. } => "opc_orphan_relationships",
            OpcWarning::DuplicateDefault { .. } => "opc_duplicate_default",
            OpcWarning::DuplicateOverride { .. } => "opc_duplicate_override",
            OpcWarning::IncompleteContentTypeEntry { .. } => "opc_incomplete_content_type_entry",
            OpcWarning::IncompleteRelationship { .. } => "opc_incomplete_relationship",
            OpcWarning::DuplicateRelationshipId { .. } => "opc_duplicate_relationship_id",
        };
        self.notes.warn(class, || w.to_string());
        // A warning that stands for many counts them all.
        if let OpcWarning::MoreDirectoryItems { count } | OpcWarning::MoreOrphanRelationships { count } = w
            && let Some(n) = self.notes.report.warning_counts.get_mut(class)
        {
            *n = n.saturating_add((*count as u64).saturating_sub(1));
        }
    }

    /// The relationships of `part`, with the package's warnings about them counted. A part with none has none. **The relationships part is
    /// charged to the document's XML bytes before it is parsed, and its rows to the items**: a package of eighty pages with a hundred
    /// thousand relationships each was twenty-one seconds and 1.1 GB that no budget saw, and the parse is repeated for every call.
    pub(crate) fn rels_of(&mut self, part: &str) -> Result<diacraft_opc::Rels, VsdxError> {
        let rels_name = diacraft_opc::name::rels_part_name(part);
        crate::work::rels_parse();
        let bytes = self.bytes_of(&rels_name);
        let size = bytes.map_or(0, <[u8]>::len);
        if size > 0 {
            self.current_part = Some(unheld::part_label("a relationships part", &rels_name));
            self.take_xml_bytes(size)?;
        }
        // The tree of the part is built inside `opc` and let go before it returns, so the pool is not charged for it, and the parse is bounded by what the pool can hold
        // beside what it holds now, as every parse of this reader is.
        let rels = match bytes {
            Some(bytes) => {
                let limits = self.xml_limits(bytes)?;
                self.pkg.rels_within(part, limits.max_nodes).map_err(|error| self.rels_failure(error, &limits))?
            }
            None => diacraft_opc::Rels::default(),
        };
        self.take_items(rels.items.len())?;
        self.take_bytes((rels.items.len() as u64).saturating_mul(REL_ROW_BYTES))?;
        self.max_rels_bytes = self.max_rels_bytes.max(size as u64);
        self.max_rels_rows = self.max_rels_rows.max(rels.items.len());
        for w in &rels.warnings {
            self.opc_warning(w);
        }
        Ok(rels)
    }

    /// The first internal relationship of `ty` from `rels`: its target part's name. Two of one type are warned of.
    pub(crate) fn target_of(&mut self, rels: &diacraft_opc::Rels, ty: &str) -> Option<String> {
        let mut found = rels.items.iter().filter(|r| r.ty == ty && r.target.is_some());
        let first = found.next()?.target.clone();
        if found.next().is_some() {
            self.notes.warn("several_relationships_of_one_type", || {
                "a part has more than one relationship of a type that is read once; the first is used".to_owned()
            });
        }
        first
    }

    /// Parses a part of the document's own (not a package part of `opc`'s): its bytes are taken from the document's total first, and a part
    /// `opc`'s XML limits refuse is the error with that part's name. **The tree is parsed within the items the pool can hold, and held in the pool until the
    /// caller gives it back** (`Reader::give_bytes`, with the bytes returned here): the model built from the tree is charged beside it.
    fn parse_part(&mut self, name: &str, bytes: &[u8]) -> Result<(El, u64), VsdxError> {
        self.current_part = Some(unheld::part_label("an XML part", name));
        self.take_xml_bytes(bytes.len())?;
        // The limits of the read with no work ahead: what the read is refused by, and what a tree parsed ahead is taken by (`ahead.rs`).
        let limits = self.xml_limits_settled(bytes)?;
        // The tree a thread parsed ahead is the tree this parse makes when it made no more items than these limits allow; any other answer is parsed here, within the limits, and is the
        // single-threaded read's. **What was set aside for the tree is given back at the charge of what it is**, with nothing between.
        let (ready, reserved) = self.take_ahead(name);
        let (root, held) = match ready {
            Taken::Failed => {
                self.sheets.set_aside_back(reserved);
                return Err(VsdxError::Worker("a thread that was parsing a part of the drawing stopped"));
            }
            Taken::Ready(fetched) if fetched.items <= limits.max_nodes => {
                crate::work::ahead_used();
                self.sheets.set_aside_back(reserved);
                let held = self.xml.merge(&fetched.figures);
                (fetched.tree, held)
            }
            other => {
                // No tree is in hand (or one that is not this parse's, which is dropped): the memory set aside for it is free, and this parse allocates only where the budget has it free.
                drop(other);
                self.sheets.set_aside_back(reserved);
                self.settle_if_binding(bytes, &limits);
                let root = crate::work::parse(bytes, &limits).map_err(|error| self.xml_failure(name, error, &limits))?;
                let held = self.xml.note(&root);
                (root, held)
            }
        };
        self.take_exact(held)?;
        self.top_up_ahead();
        Ok((root, held))
    }

    /// `mc:MustUnderstand` on a part's root: the part says the reader must understand the namespaces it
    /// names. The reader understands none of them, so the file opens (refusing would lose the drawing) and says so once for the part.
    fn check_markup_compatibility(&mut self, part: &str, root: &El, sc: &Scope) {
        if root.attrs.iter().any(|(k, _)| sc.is_attr_in(k, "MustUnderstand", crate::ns::NS_MC)) {
            self.notes.warn("must_understand", || {
                format!(
                    "the part {} says that namespaces must be understood (mc:MustUnderstand), and none are; it is read all the same",
                    part.escape_debug()
                )
            });
        }
    }

    /// What a root holds that the model has no field for (its attributes but the standard declarations: its children are the reader's of each part), and
    /// `mc:MustUnderstand`: for the root of each part the reader reads.
    fn note_root(&mut self, part: &str, root: &El, sc: &Scope) -> Result<Kept, VsdxError> {
        let kept = self.hold_root(root)?;
        self.check_markup_compatibility(part, root, sc);
        Ok(kept)
    }

    /// The bytes of a part, if it exists.
    pub(crate) fn bytes_of(&self, name: &str) -> Option<&'p [u8]> {
        let pkg: &'p Package = self.pkg;
        pkg.part(name)
    }

    fn read_document(&mut self) -> Result<Document, VsdxError> {
        // The document part, by relationship.
        let root_rels = self.rels_of("/")?;
        let document_type = rel_type(VIS, "document");
        // An external relationship has no target (`opc`: a URL is not a part name), so `target.is_some()` is also "internal".
        let Some(doc_part) = root_rels.items.iter().find(|r| r.ty == document_type && r.target.is_some()) else {
            return Err(VsdxError::NoDrawingParts("no relationship leads to a document part"));
        };
        // One document relationship. A second is warned of, and the first is the one used.
        if root_rels.items.iter().filter(|r| r.ty == document_type && r.target.is_some()).count() > 1 {
            self.notes.warn("several_relationships_of_one_type", || {
                "the package has more than one relationship to a document part; the first is used".to_owned()
            });
        }
        let doc_name = doc_part.target.clone().unwrap_or_default();
        let Some(bytes) = self.bytes_of(&doc_name) else {
            return Err(VsdxError::NoDrawingParts("the document relationship names a part the package does not have"));
        };
        let (root, root_held) = self.parse_part(&doc_name, bytes)?;
        let sc = Scope::empty().enter(&root);
        if !sc.is(&root, "VisioDocument") {
            // Say what was found: a `VisioDocument` in a namespace this reader does not read (2 real files, in a 2003 vocabulary) is not a
            // document whose root has another name.
            return Err(VsdxError::NoDrawingParts(if root.is("VisioDocument") {
                "the document part's root is a VisioDocument in a namespace this reader does not read"
            } else {
                "the document part's root is not VisioDocument"
            }));
        }
        self.notes.report.namespaces.count(sc.ns_of(&root));
        self.notes.report.document_part = doc_name.clone();
        let root_kept = self.note_root(&doc_name, &root, &sc)?;
        match self.pkg.content_type(&doc_name) {
            None => self.notes.warn("document_content_type_missing", || "the document part has no content type".to_owned()),
            Some(ct) => match Variant::from_content_type(ct) {
                Some(variant) => self.notes.report.variant = Some(variant),
                None => self
                    .notes
                    .warn("document_content_type_unknown", || "the document part's content type is none of the six a drawing package has".to_owned()),
            },
        }

        let mut doc = Document::empty();
        let doc_rels = self.rels_of(&doc_name)?;
        // The relationships of the package and of the document are the writer's to keep, as they are: a model writer makes the document and its parts and has no
        // other way to give them the ids and the targets they had.
        self.hold_rows("/", &root_rels.items, &HashSet::new())?;
        self.hold_rows(&doc_name, &doc_rels.items, &HashSet::new())?;
        self.rebuild(&doc_name)?;
        self.put_rest(&mut doc.ext, "VisioDocument", root_kept)?;
        self.read_document_children(&root, &sc, &doc_name, &mut doc)?;
        // The tree of the document part is not read again: it is let go before the masters and the pages are parsed, and not held beside theirs.
        drop(sc);
        drop(root);
        self.give_bytes(root_held);
        self.check_style_links(&doc.styles);
        self.style_ids = doc.styles.iter().map(|s| s.id).collect();
        self.read_masters(&doc_rels, &mut doc)?;
        self.read_pages(&doc_rels, &mut doc)?;
        self.read_themes(&doc_name, &doc_rels, &mut doc);
        self.read_comments(&doc_rels, &mut doc)?;
        self.read_properties(&root_rels, &mut doc)?;
        self.read_windows(&doc_rels)?;
        self.read_diacraft(&doc_rels, &mut doc)?;
        self.read_held(&mut doc)?;
        Ok(doc)
    }

    /// The children of `VisioDocument`: the tables, the styles and the document sheet are typed; the rest is passthrough.
    fn read_document_children(&mut self, root: &El, sc: &Scope, part: &str, doc: &mut Document) -> Result<(), VsdxError> {
        let (mut colors, mut fonts, mut styles, mut sheet) = (false, false, false, false);
        let mut typed = 0usize;
        let mut passthrough = Passthrough::new();
        for node in &root.children {
            let Node::El(child) = node else {
                self.pass_other(&mut passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            let ours = csc.ns_of(child).is_ours();
            let taken = if !ours {
                false
            } else if child.is("Colors") && !colors {
                colors = true;
                let entries = self.read_colors(child, &csc, &mut doc.ext)?;
                doc.colors = Arc::new(entries);
                true
            } else if child.is("FaceNames") && !fonts {
                fonts = true;
                let faces = self.read_fonts(child, &csc, &mut doc.ext)?;
                doc.fonts = Arc::new(faces);
                true
            } else if child.is("StyleSheets") && !styles {
                styles = true;
                let list = self.read_styles(child, &csc, part, &mut doc.ext)?;
                doc.styles = SharedVec::from_vec(list);
                true
            } else if child.is("DocumentSheet") && !sheet {
                sheet = true;
                doc.sheet = Arc::new(self.read_sheet_only(child, &csc, part)?);
                true
            } else {
                false
            };
            if taken {
                typed += 1;
            } else {
                self.pass_element(&mut passthrough, typed, child)?;
            }
        }
        doc.passthrough = passthrough;
        // A part that had no `FaceNames` says so (`NoFaceNames`, empty: its presence says it), as a row with no `PageSheet` does: the model's table is empty either way, and a save from the model
        // writes none where the file had none (an empty one is no table the schema allows, and the elements after it would move).
        if !fonts {
            self.put_mark(&mut doc.ext, "NoFaceNames", Kept::default())?;
        }
        Ok(())
    }

    /// The `Colors` element. A child that is no entry (no `IX` that is a number, no `RGB`, or another element) and what is between the entries are held in
    /// the document's slot under `Colors`, each after the entries before it; what an entry has beside its `IX` and `RGB` is held under `ColorEntry/<IX>`.
    fn read_colors(&mut self, el: &El, sc: &Scope, ext: &mut Ext<OdfDoc>) -> Result<Vec<ColorEntry>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        let mut out = Vec::new();
        let mut entry_rest: Vec<(String, Kept)> = Vec::new();
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, out.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            let entry = if csc.is(child, "ColorEntry") { color_entry(child) } else { None };
            match entry {
                Some((entry, exact)) => {
                    // The row stays in the table, whatever else it carries; what the entry cannot give back is held beside it.
                    if out.len() >= MAX_COLOR_ENTRIES {
                        return Err(VsdxError::Limit { kind: LimitKind::Colors, max: MAX_COLOR_ENTRIES as u64 });
                    }
                    self.take_items(1 + child.attrs.len())?;
                    let mut rest = Kept::default();
                    self.hold_attrs(child, &mut rest, |name| name == "RGB" || (name == "IX" && exact), false)?;
                    self.hold_children(child, &mut rest)?;
                    self.take_bytes(TABLE_ROW_BYTES.saturating_add(arc_str(entry.rgb.len())))?;
                    entry_rest.push((format!("ColorEntry/{}", entry.ix), rest));
                    out.push(entry);
                }
                None => self.pass_element(&mut kept.kept, out.len(), child)?,
            }
        }
        self.put_rest(ext, "Colors", kept)?;
        for (key, rest) in entry_rest {
            self.put_rest(ext, &key, rest)?;
        }
        Ok(out)
    }

    /// The `FaceNames` element: each face's attributes are in its `raw`; what is inside it, and what is in the table beside the faces, is held.
    fn read_fonts(&mut self, el: &El, sc: &Scope, ext: &mut Ext<OdfDoc>) -> Result<Vec<FaceName>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        let mut out = Vec::new();
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, out.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            let face = if csc.is(child, "FaceName") { face_name(child) } else { None };
            match face {
                Some(mut face) => {
                    self.take_items(1 + child.attrs.len())?;
                    // The attributes of a face, declarations among them, are in its `raw`; its children and what is between them are held.
                    let mut rest = Kept::default();
                    self.hold_children(child, &mut rest)?;
                    // The face's attributes are pushed one at a time into its `raw` (`face_name`), a list with room for four at the first.
                    self.take_bytes(TABLE_ROW_BYTES.saturating_add(arc_str(face.name_u.len())).saturating_add(cost::pushed_attrs_bytes(
                        child.attrs.iter().filter(|(k, _)| k != "NameU").map(|(k, v)| (k.as_str(), v.as_str())),
                    )))?;
                    self.put_rest(&mut face.ext, "FaceName", rest)?;
                    out.push(face);
                }
                None => self.pass_element(&mut kept.kept, out.len(), child)?,
            }
        }
        self.put_rest(ext, "FaceNames", kept)?;
        Ok(out)
    }

    /// The `StyleSheets` element: each `StyleSheet`, with its cap checked before it is built. A style with no `ID` cannot be named by any link: it is held in the
    /// document's slot under `StyleSheets`, with every other child that is no style and what is between them, each after the styles before it.
    fn read_styles(&mut self, el: &El, sc: &Scope, part: &str, ext: &mut Ext<OdfDoc>) -> Result<Vec<Arc<Style>>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        let mut out: Vec<Arc<Style>> = Vec::new();
        let mut ids: HashSet<u32> = HashSet::new();
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, out.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            if !csc.is(child, "StyleSheet") {
                self.pass_element(&mut kept.kept, out.len(), child)?;
                continue;
            }
            if out.len() >= self.caps.styles {
                return Err(VsdxError::Limit { kind: LimitKind::Styles, max: self.caps.styles as u64 });
            }
            let Some(style) = self.read_style(child, &csc, part)? else {
                self.pass_element(&mut kept.kept, out.len(), child)?;
                continue;
            };
            if !ids.insert(style.id) {
                self.notes.warn("duplicate_style_id", || format!("two styles have ID {}; the first is the one found", style.id));
            }
            self.styles_read += 1;
            self.take_bytes(push_bytes(out.len(), price::POINTER_BYTES))?;
            out.push(Arc::new(style));
        }
        if !out.iter().any(|s| s.id == 0) {
            self.notes.warn("root_style_missing", || "the document has no style with ID 0, the root of style inheritance".to_owned());
        }
        self.put_rest(ext, "StyleSheets", kept)?;
        Ok(out)
    }

    fn read_style(&mut self, el: &El, sc: &Scope, part: &str) -> Result<Option<Style>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        // The style and what it keeps of its attributes: its names, and every attribute the model has no field for (a link that reads as it is written is typed).
        let bytes = cost::attrs_bytes(STYLE_BYTES, &el.attrs, cost::style_attr);
        self.take_bytes(bytes)?;
        let mut style = Style::default();
        let mut sheet = diacraft_doc::Sheet::default();
        let mut has_id = false;
        for (k, v) in &el.attrs {
            match k.as_str() {
                "ID" => {
                    let (id, needs_raw) = read_u32(v);
                    if let Some(id) = id {
                        has_id = true;
                        style.id = id;
                        if needs_raw {
                            style.raw.push(k, v);
                        }
                    }
                }
                "Name" => style.name = Some(Arc::from(v.as_str())),
                "NameU" => style.name_u = Some(Arc::from(v.as_str())),
                _ => {
                    if !style_link(&mut sheet, k, v) {
                        style.raw.push(k, v);
                    }
                }
            }
        }
        if !has_id {
            return Ok(None);
        }
        let mut typed = 0usize;
        let mut tally = SheetTally::default();
        let mut order = crate::unheld::KeptOrder::default();
        for node in &el.children {
            order.note(style.passthrough.len(), sheet.passthrough.len());
            let Node::El(child) = node else {
                self.pass_other(&mut style.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            if csc.ns_of(child).is_ours() {
                match self.sheet_child(&mut sheet, &mut tally, child, &csc, part, typed)? {
                    Took::Typed => {
                        typed += 1;
                        continue;
                    }
                    Took::Kept => continue,
                    Took::No => {}
                }
            }
            self.pass_element(&mut style.passthrough, typed, child)?;
        }
        order.note(style.passthrough.len(), sheet.passthrough.len());
        self.put_order(&mut style.ext, order)?;
        self.note_sheet(&sheet);
        style.sheet = Arc::new(sheet);
        Ok(Some(style))
    }

    /// Finds the style links that name no style and the ones that go round, in the three families, in time linear in the styles. They are
    /// kept as the file has them (`doc`'s resolver walks at most 64 steps and warns); this only counts them.
    fn check_style_links(&mut self, styles: &SharedVec<Arc<Style>>) {
        let mut first: HashMap<u32, usize> = HashMap::with_capacity(styles.len());
        for (i, s) in styles.iter().enumerate() {
            first.entry(s.id).or_insert(i);
        }
        type Family = fn(&Style) -> Option<u32>;
        let families: [(&str, Family); 3] = [("line", |s| s.sheet.links.line), ("fill", |s| s.sheet.links.fill), ("text", |s| s.sheet.links.text)];
        for (family, link) in families {
            let mut next: Vec<Option<usize>> = Vec::with_capacity(styles.len());
            for s in styles.iter() {
                let target = link(s);
                if let Some(id) = target
                    && !first.contains_key(&id)
                {
                    self.notes.warn("style_link_not_found", || format!("a style's {family} link names style {id}, which the document does not have"));
                }
                // The root's own link to itself ends the chain: the root is where every chain ends, so that is no circle.
                let root_to_itself = s.id == 0 && target == Some(0);
                next.push(if root_to_itself { None } else { target.and_then(|id| first.get(&id).copied()) });
            }
            // 0 not seen, 1 on the walk now, 2 done.
            let mut state = vec![0u8; next.len()];
            for start in 0..next.len() {
                let mut path = Vec::new();
                let mut at = Some(start);
                while let Some(i) = at {
                    match state.get(i).copied() {
                        Some(0) => {
                            if let Some(s) = state.get_mut(i) {
                                *s = 1;
                            }
                            path.push(i);
                            at = next.get(i).copied().flatten();
                        }
                        Some(1) => {
                            self.notes.warn("style_link_cycle", || format!("the {family} links of some styles go round in a circle"));
                            break;
                        }
                        _ => break,
                    }
                }
                for i in path {
                    if let Some(s) = state.get_mut(i) {
                        *s = 2;
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------------------------------------------------- masters

    fn read_masters(&mut self, doc_rels: &diacraft_opc::Rels, doc: &mut Document) -> Result<(), VsdxError> {
        let Some(part) = self.target_of(doc_rels, &rel_type(VIS, "masters")) else {
            return Ok(());
        };
        let Some(bytes) = self.bytes_of(&part) else {
            self.notes.warn("part_missing", || format!("the masters part {} is not in the package", part.escape_debug()));
            return Ok(());
        };
        let (root, held) = self.parse_part(&part, bytes)?;
        let sc = Scope::empty().enter(&root);
        if !sc.is(&root, "Masters") {
            // Read as unknown: the part is held as the bytes it is, and the warning says so.
            self.notes
                .warn("part_unexpected_root", || format!("{} is reached as the masters index and its root is not Masters", part.escape_debug()));
            drop(sc);
            drop(root);
            self.give_bytes(held);
            return Ok(());
        }
        self.notes.report.namespaces.count(sc.ns_of(&root));
        let mut kept = self.note_root(&part, &root, &sc)?;
        self.rebuild(&part)?;
        let rels = RelMap::new(self.rels_of(&part)?);
        let mut derived = Derived::default();
        let mut unique_ids: HashSet<String> = HashSet::new();
        let mut masters: Vec<Arc<Master>> = Vec::new();
        self.plan_ahead(&root, &sc, &rels, "Master", &rel_type(VIS, "master"));
        for node in &root.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, masters.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            // A child that is no master (another element, a `MasterShortcut`, whose `ShortcutURL` is never fetched) is held in place.
            if !csc.ns_of(child).is_ours() || !child.is("Master") {
                self.pass_element(&mut kept.kept, masters.len(), child)?;
                continue;
            }
            if masters.len() >= self.caps.masters {
                return Err(VsdxError::Limit { kind: LimitKind::Masters, max: self.caps.masters as u64 });
            }
            let Some(mut master) = self.read_master_row(child, &csc, &part)? else {
                self.pass_element(&mut kept.kept, masters.len(), child)?;
                continue;
            };
            if let Some(unique) = &master.unique_id
                && !unique_ids.insert(unique.to_string())
            {
                self.notes
                    .warn("duplicate_master_unique_id", || format!("two masters have one UniqueID (master {}); they are not merged", master.id));
            }
            let at = masters.len();
            if self.master_index.insert_first(master.id, at) {
                self.notes.warn("duplicate_master_id", || format!("two masters have ID {}; the first is the one found", master.id));
            }
            let parsed = match master.rel_id.clone().as_deref() {
                None => None,
                Some(id) => match self.row_part(&rels, id, &rel_type(VIS, "master"), "master", master.id, &part) {
                    Some((target, whole)) => {
                        let parsed = self.parse_content_part(&target, true)?;
                        if parsed.is_some() {
                            // The part was read as this master's: its name is the master's, and the row is the master's too when it says no more than the
                            // master's fields do.
                            self.take_bytes(arc_str(target.len()))?;
                            master.part_name = Some(Arc::from(target.as_str()));
                            if whole {
                                self.derive(&mut derived, id)?;
                            }
                        }
                        parsed
                    }
                    None => None,
                },
            };
            if let Some(parsed) = &parsed {
                master.shapes = parsed.shapes.clone();
                master.connects = parsed.connects.clone();
                self.attach_rest(&mut master.ext, &parsed.ext)?;
                self.notes.report.stats.max_shapes_in_a_master = self.notes.report.stats.max_shapes_in_a_master.max(parsed.shape_count);
            }
            self.masters_parsed.push(parsed);
            self.masters_read += 1;
            self.take_bytes(push_bytes(masters.len(), price::POINTER_BYTES))?;
            masters.push(Arc::new(master));
        }
        self.end_ahead();
        doc.masters = SharedVec::from_vec(masters);
        self.put_rest(&mut doc.ext, "Masters", kept)?;
        self.hold_rows(&part, rels.rows(), &derived.ids)?;
        self.finish_derived(derived);
        drop(sc);
        drop(root);
        self.give_bytes(held);
        Ok(())
    }

    /// A `Master` row of the masters index: its attributes, `PageSheet` and `Icon`, and the `r:id` of its `Rel`. `None` without an `ID`.
    fn read_master_row(&mut self, el: &El, sc: &Scope, part: &str) -> Result<Option<Master>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        // The master and its names (the `UniqueID` is held twice: in the model, and in the set that finds a repeat), and every attribute the model has no field for.
        let bytes = cost::attrs_bytes(MASTER_BYTES, &el.attrs, cost::master_attr);
        self.take_bytes(bytes)?;
        let mut master = Master::default();
        let mut has_id = false;
        for (k, v) in &el.attrs {
            match k.as_str() {
                "ID" => {
                    let (id, needs_raw) = read_u32(v);
                    if let Some(id) = id {
                        has_id = true;
                        master.id = id;
                        if needs_raw {
                            master.raw.push(k, v);
                        }
                    }
                }
                "Name" => master.name = Some(Arc::from(v.as_str())),
                "NameU" => master.name_u = Some(Arc::from(v.as_str())),
                "UniqueID" => master.unique_id = Some(Arc::from(v.as_str())),
                "PatternFlags" => {
                    let (flags, needs_raw) = read_u32(v);
                    master.pattern_flags = flags;
                    if needs_raw {
                        master.raw.push(k, v);
                    }
                }
                _ => master.raw.push(k, v),
            }
        }
        if !has_id {
            return Ok(None);
        }
        let (mut sheet_seen, mut icon_seen) = (false, false);
        let mut typed = 0usize;
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut master.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            let ours = csc.ns_of(child).is_ours();
            let taken = if ours && child.is("PageSheet") && !sheet_seen {
                sheet_seen = true;
                master.sheet = Arc::new(self.read_sheet_only(child, &csc, part)?);
                true
            } else if ours && child.is("Rel") && master.rel_id.is_none() && csc.rel_id(child).is_some() {
                master.rel_id = csc.rel_id(child).map(Arc::from);
                self.take_bytes(arc_str(master.rel_id.as_deref().map_or(0, str::len)))?;
                let rest = self.hold_rel(child, &csc)?;
                self.put_rest(&mut master.ext, "Rel", rest)?;
                true
            } else if ours
                && child.is("Icon")
                && !icon_seen
                && child.attrs.is_empty()
                && child.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_)))
            {
                icon_seen = true;
                let text = child.text();
                self.take_bytes(arc_str(text.len()))?;
                master.icon = Some(Arc::from(text));
                true
            } else {
                false
            };
            if taken {
                typed += 1;
            } else {
                self.pass_element(&mut master.passthrough, typed, child)?;
            }
        }
        // A row with no `PageSheet` is held as one with none: a writer gives every row of a file one, and an empty one is not the same content.
        if !sheet_seen {
            self.put_mark(&mut master.ext, "NoPageSheet", Kept::default())?;
        }
        Ok(Some(master))
    }

    // ------------------------------------------------------------------------------------------------------------ pages

    fn read_pages(&mut self, doc_rels: &diacraft_opc::Rels, doc: &mut Document) -> Result<(), VsdxError> {
        let Some(part) = self.target_of(doc_rels, &rel_type(VIS, "pages")) else {
            return Ok(());
        };
        let Some(bytes) = self.bytes_of(&part) else {
            self.notes.warn("part_missing", || format!("the pages part {} is not in the package", part.escape_debug()));
            return Ok(());
        };
        let (root, held) = self.parse_part(&part, bytes)?;
        let sc = Scope::empty().enter(&root);
        if !sc.is(&root, "Pages") {
            // Read as unknown: the part is held as the bytes it is, and the warning says so.
            self.notes.warn("part_unexpected_root", || format!("{} is reached as the pages index and its root is not Pages", part.escape_debug()));
            drop(sc);
            drop(root);
            self.give_bytes(held);
            return Ok(());
        }
        self.notes.report.namespaces.count(sc.ns_of(&root));
        let mut kept = self.note_root(&part, &root, &sc)?;
        self.rebuild(&part)?;
        let rels = RelMap::new(self.rels_of(&part)?);
        let mut derived = Derived::default();
        let mut pages: Vec<Arc<Page>> = Vec::new();
        let mut ids: HashSet<u32> = HashSet::new();
        let mut shapes_per_page = Vec::new();
        self.plan_ahead(&root, &sc, &rels, "Page", &rel_type(VIS, "page"));
        for node in &root.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, pages.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            if !csc.is(child, "Page") {
                self.pass_element(&mut kept.kept, pages.len(), child)?;
                continue;
            }
            if pages.len() >= self.caps.pages {
                return Err(VsdxError::Limit { kind: LimitKind::Pages, max: self.caps.pages as u64 });
            }
            let Some(mut page) = self.read_page_row(child, &csc, &part)? else {
                self.pass_element(&mut kept.kept, pages.len(), child)?;
                continue;
            };
            if !ids.insert(page.id) {
                self.notes.warn("duplicate_page_id", || format!("two pages have ID {}; the first is the one found", page.id));
            }
            let mut count = 0;
            match page.rel_id.clone().as_deref() {
                // A page row with no part is a page with a sheet and no shapes.
                None => {}
                Some(id) => {
                    if let Some((target, whole)) = self.row_part(&rels, id, &rel_type(VIS, "page"), "page", page.id, &part)
                        && let Some(parsed) = self.parse_content_part(&target, false)?
                    {
                        page.shapes = parsed.shapes.clone();
                        page.connects = parsed.connects.clone();
                        count = parsed.shape_count;
                        // The part was read as this page's: its name is the page's, and the row is the page's too when it says no more than the page's fields do.
                        self.take_bytes(arc_str(target.len()))?;
                        page.part_name = Some(Arc::from(target.as_str()));
                        if whole {
                            self.derive(&mut derived, id)?;
                        }
                        self.attach_rest(&mut page.ext, &parsed.ext)?;
                    }
                }
            }
            self.take_bytes(push_bytes(pages.len(), price::POINTER_BYTES).saturating_add(push_bytes(shapes_per_page.len(), size_of::<usize>())))?;
            shapes_per_page.push(count);
            self.pages_read += 1;
            pages.push(Arc::new(page));
        }
        for page in &pages {
            if let Some(back) = page.back_page
                && !ids.contains(&back)
            {
                self.notes.warn("back_page_not_found", || format!("page {} names background page {back}, which the document does not have", page.id));
            }
        }
        self.end_ahead();
        self.notes.report.stats.max_shapes_in_a_page = shapes_per_page.iter().copied().max().unwrap_or(0);
        self.notes.report.stats.shapes_per_page = shapes_per_page;
        doc.pages = SharedVec::from_vec(pages);
        self.put_rest(&mut doc.ext, "Pages", kept)?;
        self.hold_rows(&part, rels.rows(), &derived.ids)?;
        self.finish_derived(derived);
        drop(sc);
        drop(root);
        self.give_bytes(held);
        Ok(())
    }

    /// A `Page` row of the pages index. `None` without an `ID`. A page with no `Name` takes its `NameU`, and with neither is called nothing
    /// (a warning says so): the model's name is not optional.
    fn read_page_row(&mut self, el: &El, sc: &Scope, part: &str) -> Result<Option<Page>, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        // The page and its names (a page without a `Name` is given its `NameU`, a second reference to a string already charged), and every attribute the model
        // has no field for.
        let bytes = cost::attrs_bytes(PAGE_BYTES, &el.attrs, cost::page_attr);
        self.take_bytes(bytes)?;
        let mut page = Page::new(0, "");
        page.name_u = None;
        let mut has_id = false;
        let mut has_name = false;
        for (k, v) in &el.attrs {
            match k.as_str() {
                "ID" => {
                    let (id, needs_raw) = read_u32(v);
                    if let Some(id) = id {
                        has_id = true;
                        page.id = id;
                        if needs_raw {
                            page.raw.push(k, v);
                        }
                    }
                }
                "Name" => {
                    has_name = true;
                    page.name = Arc::from(v.as_str());
                }
                "NameU" => page.name_u = Some(Arc::from(v.as_str())),
                "Background" => {
                    let (flag, needs_raw) = read_flag(v);
                    page.background = flag;
                    if needs_raw {
                        page.raw.push(k, v);
                    }
                }
                "BackPage" => {
                    let (back, needs_raw) = read_u32(v);
                    page.back_page = back;
                    if needs_raw {
                        page.raw.push(k, v);
                    }
                }
                _ => page.raw.push(k, v),
            }
        }
        if !has_id {
            return Ok(None);
        }
        if !has_name {
            match &page.name_u {
                Some(u) => page.name = Arc::clone(u),
                None => self.notes.warn("page_without_name", || format!("page {} has no Name and no NameU", page.id)),
            }
            // The row had no `Name`: a writer writes none while the name is the one read from it (`page_attrs`).
            self.put_mark(&mut page.ext, "NoName", Kept::default())?;
        }
        if check_text(&page.name).is_err() {
            self.notes.warn("page_name_not_valid", || {
                format!("the name of page {} is blank, too long or holds a control character; it is kept as it is", page.id)
            });
        }
        let mut sheet_seen = false;
        let mut typed = 0usize;
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut page.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            let ours = csc.ns_of(child).is_ours();
            let taken = if ours && child.is("PageSheet") && !sheet_seen {
                sheet_seen = true;
                page.sheet = Arc::new(self.read_sheet_only(child, &csc, part)?);
                true
            } else if ours && child.is("Rel") && page.rel_id.is_none() && csc.rel_id(child).is_some() {
                page.rel_id = csc.rel_id(child).map(Arc::from);
                self.take_bytes(arc_str(page.rel_id.as_deref().map_or(0, str::len)))?;
                let rest = self.hold_rel(child, &csc)?;
                self.put_rest(&mut page.ext, "Rel", rest)?;
                true
            } else {
                false
            };
            if taken {
                typed += 1;
            } else {
                self.pass_element(&mut page.passthrough, typed, child)?;
            }
        }
        if !sheet_seen {
            self.put_mark(&mut page.ext, "NoPageSheet", Kept::default())?;
        }
        Ok(Some(page))
    }

    /// The part the relationship `id` of a page or master row leads to: it must be in the index's relationships, internal, and **of the type
    /// that row kind has** (`.../page` for a page, `.../master` for a master: a type is compared exactly, V5 of the package spec). A
    /// relationship of another type (`.../Page`, or an `image`) leads to no part of that kind: the row is a row with no part, and it is
    /// warned of. `None` is also what a missing or external relationship gives.
    ///
    /// The second answer is whether the row says no more than the page's or master's own fields: its `Target` is spelled as the writer spells the target of a
    /// part from `source`, the part that holds the row (`held.rs`), so that the writer makes the row itself and it is not held.
    fn row_part(&mut self, rels: &RelMap, id: &str, ty: &str, what: &'static str, row: u32, source: &str) -> Option<(String, bool)> {
        let Some(rel) = rels.get(id) else {
            self.notes.warn(if what == "page" { "page_part_unresolved" } else { "part_missing" }, || {
                format!("{what} {row} names a relationship that leads to no part")
            });
            return None;
        };
        if rel.target.is_none() {
            self.notes.warn(if what == "page" { "page_part_unresolved" } else { "part_missing" }, || {
                format!("{what} {row} names a relationship that leads to no part")
            });
            return None;
        }
        if rel.ty != ty {
            self.notes.warn("relationship_wrong_type", || {
                format!("{what} {row} names a relationship that is not of the type a {what} has; no part is read")
            });
            return None;
        }
        let target = rel.target.clone()?;
        let whole = crate::write::relative_target(source, &target) == rel.target_raw;
        Some((target, whole))
    }

    // ----------------------------------------------------------------------------------------------- page and master parts

    /// The shapes and connects of a page or master part, parsed once however many rows name it. `None` (and a note) when it cannot be read:
    /// not in the package, XML that is not accepted, or a root that is not `PageContents` (or `MasterContents`).
    ///
    /// **A part named again is shared and charged again.** The second row gets the first's shapes (one copy, `SharedVec`), but every per-document
    /// budget is charged for the part as many times as it is used: the shapes, the cells and the items. What the budgets bound is what a walker of
    /// the document meets, and a walker (`Document::check_limits`, the oracle, a renderer) visits a shared list once for each page that holds it: a
    /// thousand pages that name one part of a million shapes would otherwise be a document of a billion shapes at the price of one.
    fn parse_content_part(&mut self, name: &str, is_master: bool) -> Result<Option<Parsed>, VsdxError> {
        // A part is read as a master part or as a page part, and the two are not the same read.
        let key = format!("{}{}", if is_master { "m:" } else { "p:" }, diacraft_opc::name::fold(name));
        if let Some(found) = self.parsed.get(&key).cloned() {
            if let Some(again) = &found {
                self.charge_again(again)?;
            }
            return Ok(found);
        }
        self.take_bytes(PARSED_BYTES.saturating_add(heap(key.len())))?;
        let (items_before, cells_before) = (self.sheets.items_left(), self.sheets.cells_left());
        let mut result = self.parse_content_part_uncached(name, is_master)?;
        if let Some(parsed) = &mut result {
            parsed.items = items_before.saturating_sub(self.sheets.items_left());
            parsed.cells = cells_before.saturating_sub(self.sheets.cells_left());
        }
        self.remember(key, result.clone());
        Ok(result)
    }

    /// Charges the budgets for one more use of a part that was parsed already.
    fn charge_again(&mut self, parsed: &Parsed) -> Result<(), VsdxError> {
        self.shapes_left =
            self.shapes_left.checked_sub(parsed.shape_count).ok_or(VsdxError::Limit { kind: LimitKind::Shapes, max: self.caps.shapes as u64 })?;
        self.sheets.charge_cells_again(parsed.cells)?;
        self.take_items(parsed.items)
    }

    fn parse_content_part_uncached(&mut self, name: &str, is_master: bool) -> Result<Option<Parsed>, VsdxError> {
        let Some(bytes) = self.bytes_of(name) else {
            self.notes.warn("part_missing", || format!("the part {} is not in the package", name.escape_debug()));
            return Ok(None);
        };
        // A part that is not XML this reader accepts refuses the file, whatever the reason: not well-formed, a DTD or an entity, or past an
        // XML limit. A page that opened empty with a warning no one reads would look like a drawing with nothing on it.
        let (root, held) = self.parse_part(name, bytes)?;
        let sc = Scope::empty().enter(&root);
        let expected = if is_master { "MasterContents" } else { "PageContents" };
        if !sc.is(&root, expected) {
            self.notes.warn("part_unexpected_root", || {
                format!(
                    "the part {} is reached as a {} part and its root is not {expected}",
                    name.escape_debug(),
                    if is_master { "master" } else { "page" }
                )
            });
            drop(sc);
            self.release_tree(root, held);
            return Ok(None);
        }
        self.notes.report.namespaces.count(sc.ns_of(&root));
        let mut root_kept = self.note_root(name, &root, &sc)?;
        let rels = match self.rels_of(name) {
            Ok(rels) => RelMap::new(rels),
            // A relationships part the package cannot read is warned of; one that is past a budget of the document is the file's end.
            Err(VsdxError::Package(_)) => {
                self.notes.warn("rels_unreadable", || {
                    format!("the relationships of {} are not readable; its pictures are not loaded", name.escape_debug())
                });
                // The part is held as the bytes it is, with the rows it had.
                self.hold_unreadable(&diacraft_opc::name::rels_part_name(name));
                RelMap::new(diacraft_opc::Rels::default())
            }
            Err(e) => return Err(e),
        };
        let mut ps = PartState { name, is_master, rels: &rels, shapes: 0, ids: std::collections::HashMap::new(), derived: Derived::default() };
        let mut parsed =
            Parsed { shapes: SharedVec::new(), connects: diacraft_doc::Connects::new(), ext: Vec::new(), shape_count: 0, items: 0, cells: 0 };
        let (mut shapes_seen, mut connects_seen) = (false, false);
        let (mut shapes_kept, mut connects_kept) = (Kept::default(), Kept::default());
        let mut typed = 0usize;
        for node in &root.children {
            let Node::El(child) = node else {
                self.pass_other(&mut root_kept.kept, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            if csc.ns_of(child).is_ours() && child.is("Shapes") && !shapes_seen {
                shapes_seen = true;
                let (list, rest) = self.read_shapes(&mut ps, child, &csc, 1, None)?;
                parsed.shapes = list;
                shapes_kept = rest;
                typed += 1;
            } else if csc.ns_of(child).is_ours() && child.is("Connects") && !connects_seen {
                connects_seen = true;
                let (list, rest) = self.read_connects(child, &csc)?;
                parsed.connects = list;
                connects_kept = rest;
                typed += 1;
            } else {
                self.pass_element(&mut root_kept.kept, typed, child)?;
            }
        }
        parsed.shape_count = ps.shapes;
        // What the part's root, its `Shapes` and its `Connects` hold that the model has no field for goes to the page or master that names the part.
        self.push_rest(&mut parsed.ext, expected, root_kept)?;
        self.push_rest(&mut parsed.ext, "Shapes", shapes_kept)?;
        self.push_rest(&mut parsed.ext, "Connects", connects_kept)?;
        // The relationships of the part: all but the rows through which a picture was loaded and that say no more than the picture's fields do.
        let derived = std::mem::take(&mut ps.derived);
        self.hold_rows(name, rels.rows(), &derived.ids)?;
        self.finish_derived(derived);
        drop(sc);
        self.release_tree(root, held);
        Ok(Some(parsed))
    }

    // ----------------------------------------------------------------------------------------------- themes and properties

    /// The themes of the drawing: the parts the document part's theme relationships lead to. **A document part with no theme relationship at all**
    /// has the theme parts the package lists by content type instead, in the order of their names, and failing that the part at the conventional place
    /// ([`CONVENTIONAL_THEME`]): a file may list the part in its content types and leave the relationship out, and its drawing is themed all the same
    /// (R26 once left such a part an unknown part). A part found so is taken for a theme only if it reads as one (an `a:theme` root): any other part is
    /// left as it was, an unknown part held as the bytes it is. The report says each one found so (`theme_without_relationship`).
    fn read_themes(&mut self, doc_name: &str, doc_rels: &diacraft_opc::Rels, doc: &mut Document) {
        let mut themes: Vec<Arc<Theme>> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut bytes_kept = 0u64;
        let ty = rel_type(OD, "theme");
        let mut any_relationship = false;
        for target in doc_rels.items.iter().filter(|r| r.ty == ty).filter_map(|r| r.target.as_deref()) {
            any_relationship = true;
            if !self.take_theme(target, ThemeFound::Relationship, &mut seen, &mut themes, &mut bytes_kept) {
                break;
            }
        }
        if !any_relationship {
            for (looked_at, (target, found)) in self.themes_without_relationship(doc_name).into_iter().enumerate() {
                // A part that is no theme is looked at all the same: only so many are, as only so many themes are read.
                if looked_at >= self.limits.max_themes {
                    self.notes.warn("themes_over_budget", || {
                        format!("more than {} parts that may be themes: the rest are not looked at", self.limits.max_themes)
                    });
                    break;
                }
                if !self.take_theme(&target, found, &mut seen, &mut themes, &mut bytes_kept) {
                    break;
                }
            }
        }
        self.notes.report.stats.themes = themes.len();
        doc.themes = SharedVec::from_vec(themes);
    }

    /// The parts of the package that are theme parts with no relationship to say so: those whose content type is the theme's, by the order of their names, else the
    /// part at the conventional place if the package has one. Names only: nothing is read of a part here.
    fn themes_without_relationship(&self, doc_name: &str) -> Vec<(String, ThemeFound)> {
        let pkg: &'p Package = self.pkg;
        let mut by_type: Vec<String> = pkg
            .parts()
            .filter(|name| !diacraft_opc::name::is_rels_part(name) && pkg.content_type(name).is_some_and(|ct| ct.eq_ignore_ascii_case(CT_THEME)))
            .map(str::to_owned)
            .collect();
        if !by_type.is_empty() {
            by_type.sort_by_cached_key(|name| diacraft_opc::name::fold(name));
            return by_type.into_iter().map(|name| (name, ThemeFound::ContentType)).collect();
        }
        match diacraft_opc::name::resolve(doc_name, CONVENTIONAL_THEME) {
            Some(place) if pkg.part(&place).is_some() => vec![(place, ThemeFound::Place)],
            _ => Vec::new(),
        }
    }

    /// Reads the theme part `target` into `themes`. `false` when the reader keeps no more themes, and the loop ends.
    fn take_theme(
        &mut self,
        target: &str,
        found: ThemeFound,
        seen: &mut HashSet<String>,
        themes: &mut Vec<Arc<Theme>>,
        bytes_kept: &mut u64,
    ) -> bool {
        if !seen.insert(diacraft_opc::name::fold(target)) {
            return true;
        }
        if themes.len() >= self.limits.max_themes {
            self.notes.warn("themes_over_budget", || format!("more than {} theme parts: the rest are not read", self.limits.max_themes));
            return false;
        }
        let Some(bytes) = self.bytes_of(target) else {
            self.notes.warn("part_missing", || format!("the theme part {} is not in the package", target.escape_debug()));
            return true;
        };
        let size = bytes.len() as u64;
        if bytes_kept.saturating_add(size) > self.limits.max_theme_bytes {
            self.notes.warn("themes_over_budget", || {
                format!("the theme parts would pass {} bytes: {} is not read", self.limits.max_theme_bytes, target.escape_debug())
            });
            return true;
        }
        // The part's bytes are kept, and what is read from them is lists of a few entries: charged at three times the part, at the least a few kilobytes. The tree
        // of the parse is bounded by what the pool holds beside that, and is let go before anything else is made.
        let kept = size.saturating_mul(3).saturating_add(4096);
        let Some(xml) = self.begin_small_part(bytes, kept) else {
            self.notes.warn("themes_over_budget", || format!("the memory is spent: the theme part {} is not read", target.escape_debug()));
            return true;
        };
        // The part is looked at whether or not it is a theme, and its bytes count against the budget of theme bytes: a package of parts that only say they are themes
        // costs a parse of what the budget allows, as the same parts behind relationships would.
        *bytes_kept += size;
        let read = read_theme(target, bytes, &xml);
        if found != ThemeFound::Relationship && read.problem.is_some() {
            // Not a theme: a part nothing says is one, and it is not taken for one. It stays an unknown part, and its charge is given back.
            self.give_bytes(price::with_margin(kept));
            return true;
        }
        if let Some(class) = read.problem {
            self.notes.warn("theme_unreadable", || {
                format!("nothing could be read from the theme part {} ({class}); its bytes are kept", target.escape_debug())
            });
        }
        if read.unread > 0 {
            self.notes.warn("theme_entry_unreadable", || {
                format!(
                    "{} entries of the theme part {} could not be read and are left unread; its bytes are kept",
                    read.unread,
                    target.escape_debug()
                )
            });
        }
        if read.capped > 0 {
            self.notes.warn("theme_list_capped", || {
                format!(
                    "{} lists of the theme part {} are longer than the reader keeps; the rest are left unread",
                    read.capped,
                    target.escape_debug()
                )
            });
        }
        if found != ThemeFound::Relationship {
            self.notes.warn("theme_without_relationship", || {
                let how = if found == ThemeFound::Place { "the place themes have" } else { "its content type" };
                format!("the document part has no relationship to a theme part: {} is read as a theme, found by {how}", target.escape_debug())
            });
        }
        themes.push(Arc::new(read.theme));
        true
    }

    /// `docProps/core.xml` and `app.xml`, found by the package's relationships (never by name), and `custom.xml`. What a field of `DocProps` reads is the
    /// field's; every other element of the two parts, and the root's own attributes, are held in `VsdxHeld::props_rest` (`held.rs`), and the custom properties are
    /// a list (`DocProps::custom`) when the part says no more than a list does. A part that is not read stays a part held as bytes.
    fn read_properties(&mut self, root_rels: &diacraft_opc::Rels, doc: &mut Document) -> Result<(), VsdxError> {
        let mut props = Props::default();
        for (ty, content_type, is_core) in [(rel_type(PK, "core-properties"), CT_CORE, true), (rel_type(OD, "extended-properties"), CT_APP, false)] {
            // The part read is the one found by content type as well as by type, as the writer finds it (`props::property_part`), so that a part
            // hung under another properties relationship is read as what it is. A type used more than once is warned of, and the warning says which
            // of them is used: not always the first.
            if root_rels.items.iter().filter(|r| r.ty == ty && r.target.is_some()).count() > 1 {
                self.notes.warn("several_relationships_of_one_type", || {
                    "the package has more than one relationship of a type for a property part; the first that leads to a part of the property's own content type is used, or failing that the first that does not lead to a part typed as another property"
                        .to_owned()
                });
            }
            let reached: Vec<(&str, &str)> = root_rels
                .items
                .iter()
                .filter_map(|r| Some((r.ty.as_str(), r.target.as_deref()?)))
                .filter(|(_, part)| self.pkg.part(part).is_some())
                .collect();
            let Some(target) = property_part(&reached, &ty, content_type, |part| self.pkg.content_type(part)).map(str::to_owned) else {
                self.note_absent_property(root_rels, &ty)?;
                continue;
            };
            let Some(bytes) = self.bytes_of(&target) else { continue };
            if bytes.len() as u64 > self.limits.max_small_part_bytes {
                self.notes.warn("properties_over_budget", || {
                    format!("the property part {} is over {} bytes and is not read", target.escape_debug(), self.limits.max_small_part_bytes)
                });
                continue;
            }
            // The tree of the part is bounded by what the pool holds beside what it holds now, and is held in it while the part is read, as the tree of every part is.
            let (root, tree) = match self.parse_held(&target, bytes) {
                Ok(parsed) => parsed,
                Err(Unparsed::Memory) => {
                    self.notes
                        .warn("properties_over_budget", || format!("the memory is spent: the property part {} is not read", target.escape_debug()));
                    continue;
                }
                Err(Unparsed::Xml(class)) => {
                    self.notes.warn("properties_unreadable", || {
                        format!("nothing could be read from the property part {} ({class})", target.escape_debug())
                    });
                    continue;
                }
            };
            let outcome = if is_core { read_core(&root, &mut props) } else { read_app(&root, &mut props) };
            let held = match outcome {
                Ok(()) if is_core => self.hold_props_rest(&root, CORE_READ, is_standard_core),
                Ok(()) => self.hold_props_rest(&root, APP_READ, is_standard_app),
                Err(class) => {
                    self.notes.warn("properties_unreadable", || {
                        format!("nothing could be read from the property part {} ({class})", target.escape_debug())
                    });
                    self.give_bytes(tree);
                    continue;
                }
            };
            self.give_bytes(tree);
            held?;
            self.rebuild(&target)?;
        }
        self.read_custom(root_rels)?;
        if let Some(title) = props.title {
            doc.title = title;
        }
        props.props.custom = std::mem::take(&mut self.held.custom);
        doc.props = Arc::new(props.props);
        self.notes.report.application = props.application;
        Ok(())
    }

    /// The DiaCraft solution: the `Solution` named `DiaCraft`, case-sensitively, whose part is a `SolutionXML` named
    /// `DiaCraft` holding a `Data` in DiaCraft's URN. The `Data` element comes into the model whole (`Document::diacraft`), kept as it is whatever
    /// its version, and the report says what it held.
    ///
    /// **When the index holds nothing but that solution and the part holds nothing but its `Data`, the model rebuilds both** (`Document::diacraft`), and they are not
    /// held as parts; the index's one relationship is the writer's. Otherwise the index and every part it names are held, the DiaCraft part among them, beside
    /// `Document::diacraft`.
    fn read_diacraft(&mut self, doc_rels: &diacraft_opc::Rels, doc: &mut Document) -> Result<(), VsdxError> {
        let Some(index) = self.target_of(doc_rels, &rel_type(VIS, "solutions")) else {
            self.notes.report.diacraft = Some(DiacraftPart::Absent);
            return Ok(());
        };
        let state = self.diacraft_in(&index, doc);
        self.notes.report.diacraft = Some(state);
        if let Some((index, part)) = self.held.solution.take() {
            self.rebuild(&index)?;
            self.rebuild(&part)?;
            // The index's relationship is the writer's own: it is not held.
            self.rows_taken(&index)?;
            // So are the relationships of the part, which lead to the parts of DiaCraft's own that it carries (`carry.rs`).
            self.rows_taken(&part)?;
        }
        if let Some(part) = self.held.solution_part.take() {
            self.rebuild(&part)?;
            self.rows_taken(&part)?;
        }
        // The parts that DiaCraft's part carries are DiaCraft's, whether their items were good or not: they are not held as parts of the package.
        for blob in std::mem::take(&mut self.held.solution_blobs) {
            self.rebuild(&blob)?;
        }
        Ok(())
    }

    /// The state of the DiaCraft part, and its `Data` element in the document (`Document::diacraft`) when it is read and the document can afford to hold it. The items of the other
    /// format's data that it carries are judged and put in the model (`carry.rs`).
    fn diacraft_in(&mut self, index: &str, doc: &mut Document) -> DiacraftPart {
        let Some(bytes) = self.bytes_of(index) else { return DiacraftPart::Absent };
        if bytes.len() as u64 > self.limits.max_small_part_bytes {
            return DiacraftPart::Unreadable("solutions index too large");
        }
        let Ok(xml) = self.xml_limits(bytes) else { return DiacraftPart::Unreadable("solutions index not read: the memory is spent") };
        let Ok(root) = crate::work::parse(bytes, &xml) else { return DiacraftPart::Unreadable("solutions index not XML") };
        // The tree of the index is alive while the solution is read beside it: held in the pool until it is let go.
        let index_tree = crate::unheld::tree_bytes(&root);
        if self.take_exact(index_tree).is_err() {
            return DiacraftPart::Unreadable("solutions index not read: the memory is spent");
        }
        let state = self.diacraft_solution(index, &root, doc);
        self.give_bytes(index_tree);
        state
    }

    /// The DiaCraft solution named by the row of `root`, the tree of the solutions index, and its `Data` in `kept` ([`Reader::diacraft_in`]).
    fn diacraft_solution(&mut self, index: &str, root: &El, doc: &mut Document) -> DiacraftPart {
        let sc = Scope::empty().enter(root);
        let Some(row) = root.elements().find(|e| sc.enter(e).is(e, "Solution") && e.attr("Name") == Some("DiaCraft")) else {
            return DiacraftPart::Absent;
        };
        let rsc = sc.enter(row);
        let Some(rel_id) = row.elements().find_map(|e| rsc.enter(e).rel_id(e)) else { return DiacraftPart::Unreadable("solution row has no Rel") };
        let Ok(rels) = self.rels_of(index) else { return DiacraftPart::Unreadable("solutions relationships not readable") };
        let target: Option<&Rel> = rels.get(rel_id);
        let Some(part) = target.and_then(|r| r.target.clone()) else { return DiacraftPart::Unreadable("solution relationship leads nowhere") };
        let Some(bytes) = self.bytes_of(&part) else { return DiacraftPart::Unreadable("solution part missing") };
        if bytes.len() as u64 > self.limits.max_solution_part_bytes {
            self.notes.warn("diacraft_not_kept", || {
                "the DiaCraft part of the file is larger than DiaCraft reads, so DiaCraft's own settings are not read and are left as they are"
                    .to_owned()
            });
            return DiacraftPart::Unreadable("solution part too large");
        }
        let Ok(xml) = self.xml_limits(bytes) else { return DiacraftPart::Unreadable("solution part not read: the memory is spent") };
        let Ok(solution) = crate::work::parse(bytes, &xml) else { return DiacraftPart::Unreadable("solution part not XML") };

        if !solution.is("SolutionXML") || solution.attr("Name") != Some("DiaCraft") {
            return DiacraftPart::Unreadable("solution part is not SolutionXML named DiaCraft");
        }
        let ssc = Scope::empty().enter(&solution);
        let Some(data) =
            solution.elements().find(|e| e.is("Data") && uri_of(&ssc.enter(e), e).is_some_and(|uri| diacraft_doc::is_diacraft_urn(&uri)))
        else {
            return DiacraftPart::Unreadable("no Data in the DiaCraft namespace");
        };
        // `dc:version`: an attribute named `version` whose prefix is bound to DiaCraft's namespace, not any other vocabulary's `version`.
        let dsc = ssc.enter(data);
        let version = data
            .attrs
            .iter()
            .find(|(k, _)| {
                dsc.is_attr_in(k, "version", diacraft_doc::DIACRAFT_URN) || dsc.is_attr_in(k, "version", diacraft_doc::DIACRAFT_URN_LEGACY)
            })
            .map(|(_, v)| v.chars().take(32).collect::<String>());
        let Some(version) = version else {
            self.notes.warn("diacraft_not_kept", || {
                "the DiaCraft data of the file states no version, so DiaCraft's own settings are not read and are left as they are".to_owned()
            });
            return DiacraftPart::Unreadable("no version");
        };
        let children = data.elements().count();
        // Both are the model's when nothing in them is anything but what the model writes back: the index is a `Solutions` of that one row, the row has that one
        // relationship, the part has no relationships but those to the parts of DiaCraft's own that it carries (`carry.rs`) and is a `SolutionXML` of that one `Data`.
        let only_row = root.elements().count() == 1
            && row.attrs.len() == 1
            && row.children.iter().all(|n| match n {
                Node::El(e) => e.is("Rel"),
                Node::Text(t) | Node::CData(t) => crate::passthrough::is_blank(t),
                _ => false,
            })
            && root.attrs.iter().all(|(k, v)| crate::unheld::is_standard_declaration(k, v))
            && root
                .children
                .iter()
                .all(|n| matches!(n, Node::El(_)) || matches!(n, Node::Text(t) | Node::CData(t) if crate::passthrough::is_blank(t)));
        let only_data = solution.elements().count() == 1
            && solution.attrs.iter().all(|(k, v)| (k == "Name" && v == "DiaCraft") || crate::unheld::is_standard_declaration(k, v))
            && solution
                .children
                .iter()
                .all(|n| matches!(n, Node::El(_)) || matches!(n, Node::Text(t) | Node::CData(t) if crate::passthrough::is_blank(t)));
        let Ok(part_rels) = self.rels_of(&part) else { return DiacraftPart::Unreadable("solution relationships not readable") };
        // **The relationships of the part are those of parts of DiaCraft's own**: each of the type, internal, to a part under `/diacraft/`, and that no other relationship of the package names
        // (a relationship of that type to a page, a master or the document is not DiaCraft's: its target is read as nothing and is never taken for bytes the data names).
        let well_formed =
            part_rels.items.iter().all(|r| crate::carry::is_blob_rel_type(&r.ty) && r.target.as_deref().is_some_and(crate::carry::is_blob_target));
        let carried_rels = well_formed && (part_rels.items.is_empty() || !self.named_elsewhere(&part, &part_rels));
        let ours = only_data && carried_rels;
        // The items of the other format's data, in data of the version this build reads, in a part that holds nothing else: they are the carrier's and not DiaCraft's settings.
        let known = if ours && version == diacraft_doc::DIACRAFT_VERSION { crate::carry::known_items(data, &dsc) } else { Vec::new() };
        let skipped: std::collections::HashSet<usize> = known.iter().map(|(at, _)| *at).collect();
        let (mut item_bytes, mut item_items) = (0u64, 0usize);
        for (_, item) in &known {
            item_bytes = item_bytes.saturating_add(kept_bytes(item.wrapper));
            item_items = item_items.saturating_add(element_cost(item.wrapper));
        }
        // Kept in the model when the items it costs are within the document's budget. A part that would go over is not read: the report says so, and the
        // writer, which compares with what the model holds, leaves the file's data as it is.
        // The tree of the whole part is alive beside the copy, and beside the tree of the index (held by the caller): each is charged, the tree to be given back once the
        // items are judged (they are in it). A charge that is taken and not kept is given back at once: what is charged last, the items, is the one that can still refuse.
        let tree = crate::unheld::tree_bytes(&solution);
        let copy = kept_bytes(data).saturating_sub(item_bytes).saturating_add(price::SEEN_ENTRY_BYTES);
        let tree_held = self.take_exact(tree).is_ok();
        let copy_held = tree_held && self.take_bytes(copy).is_ok();
        if !(copy_held && self.take_items(element_cost(data).saturating_sub(item_items)).is_ok()) {
            if copy_held {
                self.give_bytes(price::with_margin(copy));
            }
            if tree_held {
                self.give_bytes(tree);
            }
            self.notes.warn("diacraft_not_kept", || {
                "the DiaCraft data of the file has more items than the document can keep, so DiaCraft's own settings are not read".to_owned()
            });
            return DiacraftPart::Unreadable("too many items to keep");
        }
        // The data without its items: DiaCraft's own settings and what else it holds. Data that holds nothing besides (no child but blanks, no attribute but the declaration of
        // its namespace and its version) is none, as a document that has no settings has none.
        let mut own = crate::carry::raw_without(data, &skipped);
        // Data a build made before the id moved to `diacraft.app` wrote is held as the data this build writes (APPID): the namespace it declares is the current one.
        diacraft_doc::use_current_namespace(&mut own);
        let empty =
            !skipped.is_empty() && own.children.iter().all(|c| matches!(c, diacraft_doc::RawNode::Text(t) if crate::passthrough::is_blank(t)));
        doc.diacraft = if empty { None } else { Some(Arc::new(own)) };
        let items: Vec<crate::carry::Item<'_>> = known.iter().map(|(_, item)| *item).collect();
        self.carry_items(doc, &items, &part_rels);
        self.give_bytes(tree);
        let no_rels = self.pkg.part(&diacraft_opc::name::rels_part_name(&part)).is_none() || carried_rels;
        if only_row && only_data && no_rels && rels.items.len() == 1 {
            self.held.solution = Some((index.to_owned(), part));
        } else if only_data && no_rels {
            // The index is held (another program has a solution in it, or the index says more), and the part holds nothing but the `Data`: the model writes the part, and is not held beside it.
            self.held.solution_part = Some(part);
        }
        if ours {
            self.held.solution_blobs = part_rels.items.iter().filter_map(|r| r.target.clone()).collect();
        }
        DiacraftPart::Loaded { version, children }
    }

    /// Whether a relationship of another part names one of the parts the relationships `rels` of the DiaCraft solution part `part` name: a part under `/diacraft/` that a page, the
    /// document or any other part also names is that part's too, and is not DiaCraft's to take. A relationships part that cannot be read is taken to name it.
    fn named_elsewhere(&mut self, part: &str, rels: &diacraft_opc::Rels) -> bool {
        let ours: std::collections::HashSet<String> = rels.items.iter().filter_map(|r| r.target.as_deref()).map(diacraft_opc::name::fold).collect();
        let pkg: &'p Package = self.pkg;
        let own = diacraft_opc::name::fold(part);
        for name in pkg.parts() {
            if !diacraft_opc::name::is_rels_part(name) {
                continue;
            }
            let Some(source) = diacraft_opc::name::rels_source(name) else { continue };
            if diacraft_opc::name::fold(&source) == own {
                continue;
            }
            let Ok(other) = self.rels_of(&source) else { return true };
            if other.items.iter().filter_map(|r| r.target.as_deref()).any(|t| ours.contains(&diacraft_opc::name::fold(t))) {
                return true;
            }
        }
        false
    }

    /// Fills in the report once the document is built.
    pub(crate) fn finish(&mut self, doc: Option<&Document>, file_bytes: usize) -> ReadReport {
        let mut report = std::mem::take(&mut self.notes.report);
        let stats = &mut report.stats;
        stats.file_bytes = file_bytes as u64;
        stats.zip_rows = self.pkg.rows();
        stats.parts = self.pkg.parts().count();
        stats.uncompressed_bytes = self.pkg.parts().filter_map(|p| self.pkg.part(p)).map(|b| b.len() as u64).fold(0u64, u64::saturating_add);
        stats.max_part_bytes = self.pkg.parts().filter_map(|p| self.pkg.part(p)).map(|b| b.len() as u64).max().unwrap_or(0);
        stats.max_part_name_bytes = self.pkg.parts().map(str::len).max().unwrap_or(0);
        stats.content_type_rows = self.pkg.content_types().defaults.len().saturating_add(self.pkg.content_types().overrides.len());
        stats.max_rels_bytes = self.max_rels_bytes;
        stats.max_rels_rows = self.max_rels_rows;
        stats.max_xml_items = self.xml.items;
        stats.max_xml_depth = self.xml.depth;
        stats.max_xml_attr_bytes = self.xml.attr_bytes;
        // A refused file has no document: the counts are those of what had been read.
        stats.styles = doc.map_or(self.styles_read, |d| d.styles.len());
        stats.masters = doc.map_or(self.masters_read, |d| d.masters.len());
        stats.pages = doc.map_or(self.pages_read, |d| d.pages.len());
        stats.shapes = self.caps.shapes.saturating_sub(self.shapes_left);
        stats.max_shape_depth = self.max_shape_depth;
        let sheets = self.sheets.stats();
        stats.cells = sheets.cells;
        stats.items = sheets.items;
        stats.xml_bytes = self.limits.max_xml_bytes.saturating_sub(self.xml_bytes_left);
        stats.formula_nodes = sheets.formula_nodes;
        stats.max_cells_in_one_place = sheets.max_cells_in_one_place;
        stats.max_sections_in_a_sheet = sheets.max_sections_in_a_sheet;
        stats.max_rows_in_a_section = sheets.max_rows_in_a_section;
        stats.media_bytes = self.media.bytes;
        stats.memory_bytes = sheets.bytes;
        stats.memory_peak = sheets.bytes_peak;
        stats.max_tree_bytes = self.xml.tree_bytes;
        stats.connects = self.connects;
        report.stats.formula_cells = sheets.formula_cells;
        report
    }
}

/// The URI an element's name is in, by the scope it was entered into.
fn uri_of(sc: &Scope, el: &El) -> Option<String> {
    sc.uri_of_element(el)
}

/// A `ColorEntry`: an `IX` that is a number and an `RGB`, whatever else it carries (**a foreign attribute does not take the row out of the
/// table**: `doc.color(24)` gives the colour the entry names, and the attribute is counted by the caller). Whether the `IX` is exactly the
/// text of its number comes back too, for the caller to count an `IX` such as `024`, whose text the entry cannot give back.
fn color_entry(el: &El) -> Option<(ColorEntry, bool)> {
    let (mut ix, mut rgb, mut exact) = (None, None, true);
    for (k, v) in &el.attrs {
        match k.as_str() {
            "IX" => {
                let (n, needs_raw) = read_u32(v);
                exact = !needs_raw;
                ix = n;
            }
            "RGB" => rgb = Some(Arc::from(v.as_str())),
            _ => {}
        }
    }
    Some((ColorEntry { ix: ix?, rgb: rgb? }, exact))
}

/// A `FaceName`: `NameU` and every other attribute as it is. `None` without a `NameU`.
fn face_name(el: &El) -> Option<FaceName> {
    let name_u = el.attr("NameU")?;
    let mut raw = RawAttrs::new();
    for (k, v) in &el.attrs {
        if k != "NameU" {
            raw.push(k, v);
        }
    }
    Some(FaceName { name_u: Arc::from(name_u), raw, ext: Default::default() })
}

/// `HashMap::insert` that keeps the first value: whether the key was already there.
trait InsertFirst {
    fn insert_first(&mut self, key: u32, value: usize) -> bool;
}

impl InsertFirst for HashMap<u32, usize> {
    fn insert_first(&mut self, key: u32, value: usize) -> bool {
        match self.entry(key) {
            std::collections::hash_map::Entry::Occupied(_) => true,
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(value);
                false
            }
        }
    }
}
