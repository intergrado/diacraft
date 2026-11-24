//! What the reader holds beside the model's own fields, so that the model is the only record of a drawing that came from a package (the native format's
//! design, section 4.2).
//!
//! **A part is held unless the model rebuilds it from its own fields.** The model rebuilds the document part, the pages index and the masters index, each page
//! and master part that was read as one, the themes (by their bytes), the property parts that were read (the fields of `DocProps`, `props_rest` and `custom`),
//! `windows.xml` (held whole as a tree, in `VsdxHeld::windows`), the pictures that were loaded as `Media`, and DiaCraft's own solution part when the solutions
//! index holds nothing else. **Every other part of the package is a `Part` of `Document::parts`**, with its name as the package has it, its content type (empty
//! when no row gives it one), its role, the relationships from it, and its bytes, inflated: unknown parts, custom XML, comments, data connections and
//! recordsets, validation and extensions parts, other programs' solutions and their index, a VBA project with its signatures, embeddings, the thumbnail,
//! digital signatures, and any part a read did not take (a theme past the theme budget, a page part whose root is not a page's, a property part that is
//! not XML). A relationships part whose source is not a part of the package, or is itself a relationships part, or cannot be parsed, is held as the bytes it is.
//!
//! **A relationship row is held unless a field of the model says the whole of it** (`VsdxHeld::rels`, with the part the row comes from in `Rel::source`, `/` for
//! the package): the row of the pages index that names a page's part is the page's (`Page::rel_id`, `Page::part_name`), the row of the masters index that names a
//! master's is the master's, and the row of a page's or master's part through which a picture was loaded is the picture's (`ForeignData::rel_id`,
//! `Media::part`), **when the row's `Target` is spelled as the writer spells the target of a part**. A row spelled any other way (an absolute target, a
//! percent-escape) is held with its own spelling. The rows from a held part are its own (`Part::rels`). **The writer takes the held rows first** and makes the
//! others for what it writes, never reusing an `Id` or a part name a held row or part has (ASCII case-insensitively).
//!
//! **A content-type row is held when no part needs it** (`VsdxHeld::content_types`): an `Override` for a part that is not in the package, a `Default` for an
//! extension no part relies on, and a row for a key that an earlier row has. The rows the parts need are the writer's to make (`Part::media_type` and the types of
//! the parts it writes), and so are the two `Default` rows every package has, `rels` and `xml`. The `Default` rows come before the `Override` rows, each in file order.
//!
//! **`Page::part_name` is set when the part was read as the page's contents**, and is `None` otherwise (the row had no part, the part is missing, or its root is not
//! `PageContents`): a page with no part name has no part of its own in the file. Two pages that name one part both have its name. The same holds for masters.
//!
//! **Everything is charged before it is kept** (`cost.rs`): the part, its strings, its relationships, and every row, as prices. **The bytes of a held part live once,
//! and are the package's**: they are moved out of the `Package` into the document after the read (`move_parts`, F1 of this task), and not copied beside it, so the pool
//! counts them once, as the package it deducted at the start; `Stats::memory_bytes` adds them when they move, so that the ledger of the process counts the model's own.
//! **The move makes one `Arc<[u8]>` copy of a part at a time** (the model's type for the bytes of a part is `Arc<[u8]>`, which cannot take over the buffer of a `Vec`),
//! so for a moment the largest held part is live twice: **that copy is checked against the pool before any part moves** (`Reader::room_to_move`: the room the pool has
//! left, and the bytes of the package the move lets go of first, against the largest part), and a file whose copy does not fit is refused in words (`too_much_memory`).
//! A file whose model the pool cannot hold is refused in the same words; one whose held parts are big is refused for the one copy and not for all of them. **Relationship lookups are maps**, and each
//! part's relationships part is parsed once.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{ContentTypeRow, CustomProp, Document, Format, Kept, Part, PartRole, RawAttrs, Rel, SharedVec, VsdxHeld};
use diacraft_opc::name::{fold, is_rels_part, lenient_decode, rels_part_name, rels_source};
use diacraft_opc::xml::{El, Node};
use diacraft_opc::{Package, Rel as OpcRel, TargetMode};
use diacraft_sheetxml::model::cost::push_bytes;
use diacraft_sheetxml::price::{self, POINTER_BYTES, SEEN_ENTRY_BYTES, arc_str, heap, kept_bytes};

use crate::cost;
use crate::error::VsdxError;
use crate::passthrough::{element_cost, raw_element};
use crate::props::{CT_CUSTOM, NS_CUSTOM, NS_VT};
use crate::reader::Reader;
use crate::report::ReadReport;
use crate::work;

const VIS: &str = "http://schemas.microsoft.com/visio/2010/relationships/"; // brand-ok: a URI the format defines
/// The start of the entry of `VsdxHeld::props_rest` that says the package names a property part it does not hold (`note_absent_property`).
pub(crate) const ABSENT_PROPERTY: &str = "-absent/";
const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
/// The type of the relationship to the thumbnail, and the base of the relationships to a digital signature's parts.
const THUMBNAIL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
const SIGNATURES: &str = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/";
const MS: &str = "http://schemas.microsoft.com/office/"; // brand-ok: a URI the format defines

/// The content type of a relationships part, which every package has a `Default` row for.
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
/// The content type a `Default` row for `xml` gives.
const CT_XML: &str = "application/xml";

/// What a role entry costs the map that finds a held part's role from the relationship that names it (a key, a role and the table's room).
const ROLE_ENTRY_BYTES: u64 = 96;

/// Why a small part was not parsed: the memory the read may hold is spent (or the part is past what a small part may be), or the part is not XML this reader
/// accepts, with the class of the error.
pub(crate) enum Unparsed {
    Memory,
    Xml(&'static str),
}

/// The `Id`s of the relationship rows of one part that a field of the model says the whole of (`Page::rel_id`, `ForeignData::rel_id`), so that the rows are not
/// held. **Scratch**: it lives while the part's rows are held, and what it was charged is given back with it ([`Reader::finish_derived`]).
#[derive(Default)]
pub(crate) struct Derived {
    pub ids: HashSet<String>,
    charged: u64,
}

/// A held part but for its bytes, which are the package's and move into it when the read is done ([`move_parts`]).
pub(crate) struct PendingPart {
    name: Arc<str>,
    media_type: Arc<str>,
    role: PartRole,
    rels: Arc<Vec<Rel>>,
    /// The bytes the part will have, as the package states them.
    len: usize,
}

/// What the reader has taken for the model, and what it holds, until the document is whole.
#[derive(Default)]
pub(crate) struct HeldState {
    /// The names (as the package has them) of the parts the model rebuilds that nothing else records: the document, the two indexes, the property parts that were
    /// read, `windows.xml` and DiaCraft's solution. The page and master parts, the themes and the pictures are found in the tables that hold them.
    pub rebuilt: Vec<String>,
    /// The folded names of the sources whose relationship rows were taken: a part's rows are taken once.
    pub rels_done: HashSet<String>,
    /// The held relationship rows (`VsdxHeld::rels`).
    pub rels: Vec<Rel>,
    /// The role a part has from the type of a relationship that names it, by the folded name of the part, the first that says one winning.
    pub roles: HashMap<String, PartRole>,
    /// `VsdxHeld::props_rest`, and `DocProps::custom`.
    pub props_rest: Vec<(Arc<str>, Kept)>,
    pub custom: Vec<CustomProp>,
    /// `VsdxHeld::windows`.
    pub windows: Option<Arc<diacraft_doc::RawElement>>,
    /// The index and the part of DiaCraft's solution, when the model rebuilds both (`Reader::read_diacraft`).
    pub solution: Option<(String, String)>,
    /// The part of DiaCraft's solution when the model rebuilds it and the index is held (another program has a solution in it): the index names it, and the model writes it.
    pub solution_part: Option<String>,
    /// The parts of DiaCraft's own that its solution part names (`carry.rs`), when the model rebuilds the solution part: the model writes them from what it carries, and they are not held.
    pub solution_blobs: Vec<String>,
    /// The folded names of the relationships parts that could not be parsed: held as the bytes they are.
    pub unreadable_rels: HashSet<String>,
    /// The held parts, all but their bytes (`move_parts` gives them those once the read is done and the package can be let go of).
    pub pending: Vec<PendingPart>,
    /// What the tables of this state (the names of the parts the model rebuilds, the sources whose rows were taken, the roles) were charged: scratch, which is
    /// given back when the read of the parts that are held is done, so that what the read ends with is charged for what the model holds and not for the work.
    pub scratch: u64,
}

/// The role a relationship's type gives the part it names, if it says one.
fn role_of_type(ty: &str) -> Option<PartRole> {
    let last = ty.rsplit('/').next().unwrap_or("");
    if ty == THUMBNAIL {
        return Some(PartRole::Thumbnail);
    }
    if ty.starts_with(SIGNATURES) {
        return Some(PartRole::Signature);
    }
    if ty.starts_with(MS) {
        return match last {
            "vbaProject" => Some(PartRole::MacroProject),
            "vbaData" => Some(PartRole::MacroData),
            _ if last.starts_with("vbaProjectSignature") => Some(PartRole::MacroSignature),
            _ => None,
        };
    }
    if ty.starts_with(OD) {
        return match last {
            "oleObject" | "package" => Some(PartRole::Embedding),
            "customXml" | "customXmlProps" => Some(PartRole::CustomXml),
            "font" => Some(PartRole::Font),
            _ => None,
        };
    }
    None
}

/// The role a content type gives a part, if it says one.
fn role_of_content_type(content_type: &str) -> Option<PartRole> {
    let ct = content_type.trim().to_ascii_lowercase();
    if ct.contains("digital-signature") {
        return Some(PartRole::Signature);
    }
    if ct.starts_with("application/vnd.ms-office.vbaprojectsignature") {
        return Some(PartRole::MacroSignature);
    }
    if ct == "application/vnd.ms-office.vbaproject" {
        return Some(PartRole::MacroProject);
    }
    if ct == "application/vnd.openxmlformats-officedocument.oleobject" {
        return Some(PartRole::Embedding);
    }
    if ct.contains("customxml") {
        return Some(PartRole::CustomXml);
    }
    None
}

/// The extension of a part name, folded: the text after the last dot of the last segment.
fn extension_of(folded: &str) -> Option<&str> {
    folded.rsplit('/').next()?.rsplit_once('.').map(|(_, extension)| extension)
}

impl<'p> Reader<'p> {
    /// Charges scratch of the held read: it is given back at the end of [`Reader::read_held`].
    fn scratch(&mut self, n: u64) -> Result<(), VsdxError> {
        let price = price::with_margin(n);
        self.take_exact(price)?;
        self.held.scratch = self.held.scratch.saturating_add(price);
        Ok(())
    }

    /// Notes that the model rebuilds the part `name` from its own fields, so that it is not held as a part.
    pub(crate) fn rebuild(&mut self, name: &str) -> Result<(), VsdxError> {
        self.scratch(arc_str(name.len()).saturating_add(24))?;
        self.held.rebuilt.push(name.to_owned());
        Ok(())
    }

    /// Notes that the rows of the relationships part of `source` were taken, charging the table that says so.
    pub(crate) fn rows_taken(&mut self, source: &str) -> Result<(), VsdxError> {
        let key = fold(source);
        self.scratch(SEEN_ENTRY_BYTES.saturating_add(heap(key.len())))?;
        self.held.rels_done.insert(key);
        Ok(())
    }

    /// Notes that the row `id` is the whole of a field's, charging the set that says so (a key and the room of the table).
    pub(crate) fn derive(&mut self, set: &mut Derived, id: &str) -> Result<(), VsdxError> {
        if set.ids.contains(id) {
            return Ok(());
        }
        let price = price::with_margin(SEEN_ENTRY_BYTES.saturating_add(heap(id.len())));
        self.take_exact(price)?;
        set.charged = set.charged.saturating_add(price);
        set.ids.insert(id.to_owned());
        Ok(())
    }

    /// Gives back what a [`Derived`] was charged, when its rows are held.
    pub(crate) fn finish_derived(&mut self, set: Derived) {
        self.give_bytes(set.charged);
    }

    /// Notes the role the part a row names has from the row's type.
    fn note_role(&mut self, row: &OpcRel) -> Result<(), VsdxError> {
        let Some(role) = role_of_type(&row.ty) else { return Ok(()) };
        let Some(target) = &row.target else { return Ok(()) };
        let key = fold(target);
        if self.held.roles.contains_key(&key) {
            return Ok(());
        }
        self.scratch(ROLE_ENTRY_BYTES.saturating_add(heap(key.len())))?;
        self.held.roles.insert(key, role);
        Ok(())
    }

    /// Holds the rows of the relationships part of `source` (`/` for the package) that no field of the model says: all of them but the first row of each `Id` in
    /// `derived`, which the caller found to be the whole of a page's, a master's or a picture's. The rows of a source are taken once, whichever parse they came
    /// from. Each row is charged before it is made.
    pub(crate) fn hold_rows(&mut self, source: &str, rows: &[OpcRel], derived: &HashSet<String>) -> Result<(), VsdxError> {
        if self.held.rels_done.contains(&fold(source)) {
            return Ok(());
        }
        self.rows_taken(source)?;
        work::held_rows(rows.len());
        let mut source_name: Option<Arc<str>> = None;
        let mut taken: HashSet<&str> = HashSet::new();
        for row in rows {
            self.note_role(row)?;
            if derived.contains(row.id.as_str()) && taken.insert(row.id.as_str()) {
                continue;
            }
            let source_arc = match &source_name {
                Some(name) => Arc::clone(name),
                None => {
                    self.take_bytes(arc_str(source.len()))?;
                    let name: Arc<str> = Arc::from(source);
                    source_name = Some(Arc::clone(&name));
                    name
                }
            };
            self.take_bytes(cost::held_rel_bytes(self.held.rels.len(), row.id.len(), row.ty.len(), row.target_raw.len()))?;
            self.held.rels.push(doc_rel(&source_arc, row));
        }
        Ok(())
    }

    /// Parses a small part for the model to keep a copy of (a property part, `windows.xml`): the tree is bounded by what the pool holds beside what it holds
    /// now, and is held in the pool until the caller gives its bytes back. The part that cannot be read (not XML, or past the pool or the size of a small part)
    /// stays a part held as the bytes it is, and the caller says why.
    pub(crate) fn parse_held(&mut self, name: &str, bytes: &[u8]) -> Result<(El, u64), Unparsed> {
        if bytes.len() as u64 > self.limits.max_small_part_bytes {
            return Err(Unparsed::Memory);
        }
        self.current_part = Some(crate::unheld::part_label("an XML part", name));
        let limits = self.xml_limits(bytes).map_err(|_| Unparsed::Memory)?;
        let root = crate::work::parse(bytes, &limits).map_err(|error| {
            // A tree past the items the pool can hold is a read past the pool, not a part that is too complex.
            if matches!(error, diacraft_opc::XmlError::TooManyNodes { .. }) && limits.max_nodes < self.limits.opc.xml.max_nodes {
                Unparsed::Memory
            } else {
                Unparsed::Xml(error.class())
            }
        })?;
        let held = self.xml.note(&root);
        self.take_exact(held).map_err(|_| Unparsed::Memory)?;
        Ok((root, held))
    }

    /// `windows.xml`, held whole as a tree: found by the document's relationship (never by name). A part that is not XML, or is not a `Windows`, or whose
    /// copy the pool cannot hold, stays a part held as bytes.
    pub(crate) fn read_windows(&mut self, doc_rels: &diacraft_opc::Rels) -> Result<(), VsdxError> {
        let Some(part) = self.target_of(doc_rels, &format!("{VIS}windows")) else { return Ok(()) };
        let Some(bytes) = self.bytes_of(&part) else { return Ok(()) };
        let Ok((root, tree)) = self.parse_held(&part, bytes) else {
            self.notes.warn("windows_unreadable", || "the windows part is not read as XML, and is held as the bytes it is".to_owned());
            return Ok(());
        };
        if !root.is("Windows") {
            self.give_bytes(tree);
            self.notes
                .warn("windows_unexpected_root", || "the windows part's root is not Windows, and the part is held as the bytes it is".to_owned());
            return Ok(());
        }
        let copy = kept_bytes(&root).saturating_add(SEEN_ENTRY_BYTES);
        let paid = self.take_bytes(copy).is_ok() && self.take_items(element_cost(&root)).is_ok();
        if !paid {
            self.give_bytes(tree);
            self.notes.warn("windows_unreadable", || "the memory is spent: the windows part is held as the bytes it is".to_owned());
            return Ok(());
        }
        self.held.windows = Some(Arc::new(raw_element(&root)));
        self.give_bytes(tree);
        self.rebuild(&part)
    }

    /// The custom properties, as a list the writer can write back exactly, from the part `custom` of the package's relationships: a part whose root declares
    /// anything but the two standard namespaces, or whose properties hold anything but one text value each, is not one of these, and is held as bytes.
    pub(crate) fn read_custom(&mut self, root_rels: &diacraft_opc::Rels) -> Result<(), VsdxError> {
        let reached: Vec<(&str, &str)> = root_rels
            .items
            .iter()
            .filter_map(|r| Some((r.ty.as_str(), r.target.as_deref()?)))
            .filter(|(_, part)| self.pkg.part(part).is_some())
            .collect();
        let rel = format!("{OD}custom-properties");
        let Some(target) = crate::props::property_part(&reached, &rel, CT_CUSTOM, |part| self.pkg.content_type(part)).map(str::to_owned) else {
            return self.note_absent_property(root_rels, &rel);
        };
        let Some(bytes) = self.bytes_of(&target) else { return Ok(()) };
        let Ok((root, tree)) = self.parse_held(&target, bytes) else { return Ok(()) };
        let list = self.custom_list(&root);
        self.give_bytes(tree);
        match list? {
            Some(list) => {
                self.held.custom = list;
                self.rebuild(&target)
            }
            None => Ok(()),
        }
    }

    /// The properties of a custom properties part as a list, or `None` when the part says more than a list says. Each is charged before it is made.
    fn custom_list(&mut self, root: &El) -> Result<Option<Vec<CustomProp>>, VsdxError> {
        if !root.is("Properties") {
            return Ok(None);
        }
        let standard = |name: &str, value: &str| (name == "xmlns" && value == NS_CUSTOM) || (name == "xmlns:vt" && value == NS_VT);
        // A part that declares no namespace (or another) is no list of the vocabulary: the writer would give it the declaration.
        if root.attrs.iter().any(|(k, v)| !standard(k, v)) || !root.attrs.iter().any(|(k, v)| k == "xmlns" && v == NS_CUSTOM) {
            return Ok(None);
        }
        let mut list: Vec<CustomProp> = Vec::new();
        for node in &root.children {
            let child = match node {
                Node::El(child) => child,
                Node::Text(t) | Node::CData(t) if crate::passthrough::is_blank(t) => continue,
                _ => return Ok(None),
            };
            let Some(prop) = custom_prop(child) else { return Ok(None) };
            let raw_bytes =
                prop.raw.iter().enumerate().fold(0u64, |sum, (at, (k, v))| sum.saturating_add(price::raw_attr_bytes(at, k.len(), v.len())));
            self.take_items(element_cost(child))?;
            self.take_bytes(cost::custom_prop_bytes(list.len(), prop.name.len(), prop.value_type.len(), prop.value.len()).saturating_add(raw_bytes))?;
            list.push(prop);
        }
        Ok(Some(list))
    }

    /// **A property part the package's relationships name and the package does not hold** (a row of the type of `ty`, internal, whose target is no part): the
    /// list the document's `VsdxHeld` takes says so, as `-absent/{the type's last word}` (`core-properties`, `extended-properties`, `custom-properties`), so that a save of the
    /// drawing as it was read writes no part where the file had none (`write/held.rs`, `properties`). The row itself is held with the package's others.
    pub(crate) fn note_absent_property(&mut self, root_rels: &diacraft_opc::Rels, ty: &str) -> Result<(), VsdxError> {
        let dangling = root_rels.items.iter().any(|r| r.ty == ty && r.target.as_deref().is_some_and(|t| self.pkg.part(t).is_none()));
        if !dangling {
            return Ok(());
        }
        let name = format!("{ABSENT_PROPERTY}{}", ty.rsplit('/').next().unwrap_or(ty));
        self.take_bytes(cost::rest_entry_bytes(self.held.props_rest.len(), name.len()))?;
        self.held.props_rest.push((Arc::from(name.as_str()), Kept::default()));
        Ok(())
    }

    /// Holds what the property part's root has that no field of `DocProps` reads, in the list the document's `VsdxHeld` takes: the root's own attributes but the
    /// standard declarations in `raw`, and each comment, instruction, text and element no field reads, after as many of the elements a field reads as came before it.
    /// `read` is the local names of the elements the fields read, the first of each; `standard` says which declarations the writer regenerates.
    pub(crate) fn hold_props_rest(&mut self, root: &El, read: &[&str], standard: impl Fn(&str, &str) -> bool) -> Result<(), VsdxError> {
        let mut kept = Kept::default();
        self.take_items(root.attrs.len())?;
        for (name, value) in &root.attrs {
            if standard(name, value) {
                continue;
            }
            self.take_bytes(price::raw_attr_bytes(kept.raw.len(), name.len(), value.len()))?;
            kept.raw.push(name, value);
        }
        let mut typed = 0usize;
        let mut seen: Vec<&str> = Vec::new();
        let key = root.local();
        let canonical: &[&str] = if key == "coreProperties" { &crate::props::CORE_ORDER } else { &crate::props::APP_ORDER };
        let mut rest = std::mem::take(&mut self.held.props_rest);
        let outcome = (|| {
            for node in &root.children {
                match node {
                    Node::El(child) if read.contains(&child.local()) && !seen.contains(&child.local()) => {
                        seen.push(child.local());
                        typed += 1;
                        self.hold_field(&mut rest, key, child)?;
                    }
                    Node::El(child) => self.pass_element(&mut kept.kept, typed, child)?,
                    other => self.pass_other(&mut kept.kept, typed, other)?,
                }
            }
            // The fields in the order the file had them, when that is not the order a writer makes them in (`write/props.rs`): the kept nodes are placed by it.
            let wrote: Vec<&str> = canonical.iter().copied().filter(|name| seen.contains(name)).collect();
            if wrote != seen && !seen.is_empty() {
                let order = seen.join(",");
                let mut kept = Kept::default();
                self.take_items(1)?;
                self.take_bytes(price::raw_attr_bytes(0, "order".len(), order.len()))?;
                kept.raw.push("order", &order);
                let name = format!("{key}/-order");
                self.take_bytes(cost::rest_entry_bytes(rest.len(), name.len()))?;
                rest.push((Arc::from(name.as_str()), kept));
            }
            // A core part with no title says so: its field is text and cannot tell an empty element from none.
            if key == "coreProperties" && !seen.contains(&"title") {
                let name = format!("{key}/-title");
                self.take_bytes(cost::rest_entry_bytes(rest.len(), name.len()))?;
                rest.push((Arc::from(name.as_str()), Kept::default()));
            }
            self.push_rest(&mut rest, key, kept)
        })();
        self.held.props_rest = rest;
        outcome
    }

    /// Holds what the element of a property field has beside its text, which the field says: its attributes (`xsi:type` of a time, `xml:lang`) and the comments, instructions and elements among
    /// its text, each after as many pieces of text as came before it, under `{root}/{name}`. **A time's entry is held whether or not it holds anything** (a time with no `xsi:type` is not one with it).
    fn hold_field(&mut self, rest: &mut Vec<(Arc<str>, Kept)>, root: &str, el: &El) -> Result<(), VsdxError> {
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, true)?;
        let mut texts = 0usize;
        for node in &el.children {
            match node {
                Node::Text(t) | Node::CData(t) => {
                    if !crate::passthrough::is_blank(t) {
                        texts = 1;
                    }
                }
                Node::El(child) => self.pass_element(&mut kept.kept, texts, child)?,
                other => self.pass_other(&mut kept.kept, texts, other)?,
            }
        }
        let always = matches!(el.local(), "created" | "modified");
        if !always && kept.raw.is_empty() && kept.kept.is_empty() {
            return Ok(());
        }
        let name = format!("{root}/{}", el.local());
        self.take_bytes(cost::rest_entry_bytes(rest.len(), name.len()))?;
        rest.push((Arc::from(name.as_str()), kept));
        Ok(())
    }

    /// Whether the pool has room for the one copy of the largest held part that the move makes ([`move_parts`]). **The bytes of every part of the package that is not
    /// held are let go before any part is copied**, and they are the package's, which the pool deducted at the start: so the room is what the pool has left and what the
    /// move lets go of. A pool with less is refused in words (`too_much_memory`), where the copy would have been made past it. No part is copied here, and the figure is a
    /// check and not a charge: it is not in `memory_peak`, which is what the pool was charged.
    pub(crate) fn room_to_move(&self) -> Result<(), VsdxError> {
        let pkg: &'p Package = self.pkg;
        let largest = self.held.pending.iter().map(|part| part.len).max().unwrap_or(0);
        if largest == 0 {
            return Ok(());
        }
        let held = self.held.pending.iter().fold(0u64, |sum, part| sum.saturating_add(part.len as u64));
        let package = pkg.parts().filter_map(|name| pkg.part(name)).fold(0u64, |sum, bytes| sum.saturating_add(bytes.len() as u64));
        let room = self.sheets.bytes_left().saturating_add(package.saturating_sub(held));
        if room < arc_str(largest) {
            return Err(VsdxError::Limit { kind: crate::error::LimitKind::Memory, max: self.limits.max_memory });
        }
        Ok(())
    }

    /// Finishes the held part of a read: the parts, the rest of the relationships, the content-type rows, and the document's `VsdxHeld`. See the module
    /// documentation for what is held.
    pub(crate) fn read_held(&mut self, doc: &mut Document) -> Result<(), VsdxError> {
        let pkg: &'p Package = self.pkg;
        // The parts the model rebuilds, by folded name: found in the tables that hold them, so that nothing is made for each page and master. The set is scratch,
        // charged while it exists.
        let scratch = (self.held.rebuilt.len() + self.parsed.len() + doc.themes.len() + self.media.entries()) as u64 * 192;
        self.take_bytes(scratch)?;
        let mut rebuilt: HashSet<String> = HashSet::with_capacity(self.held.rebuilt.len() + self.parsed.len() + doc.themes.len());
        rebuilt.extend(self.held.rebuilt.iter().map(|name| fold(name)));
        rebuilt.extend(self.parsed.iter().filter(|(_, parsed)| parsed.is_some()).map(|(key, _)| key.get(2..).unwrap_or("").to_owned()));
        rebuilt.extend(doc.themes.iter().map(|theme| fold(&theme.part)));
        rebuilt.extend(self.media.loaded().map(str::to_owned));

        // Pass 1: the relationships. A part the model rebuilds has its rows held beside the document's; a part held has them read with it, below.
        let mut pending: Vec<(&'p str, Vec<Rel>)> = Vec::new();
        for name in pkg.parts() {
            work::held_part();
            if is_rels_part(name) {
                if let Some(source) = rels_source(name)
                    && (source == "/" || (pkg.part(&source).is_some() && !is_rels_part(&source)))
                    && (source == "/" || rebuilt.contains(&fold(&source)))
                {
                    self.hold_rels_of(&source, name)?;
                }
                continue;
            }
            if rebuilt.contains(&fold(name)) {
                continue;
            }
            let rows = self.rows_of_held(name)?;
            pending.push((name, rows));
        }

        // Pass 2: the parts. Their roles come from every row there is, which is why the rows are read first.
        let empty: Arc<Vec<Rel>> = Arc::new(Vec::new());
        let mut parts: Vec<PendingPart> = Vec::new();
        for (name, rows) in pending {
            let len = pkg.part(name).map_or(0, <[u8]>::len);
            let content_type = pkg.content_type(name).unwrap_or("");
            let role = self.held.roles.get(&fold(name)).copied().or_else(|| role_of_content_type(content_type)).unwrap_or(PartRole::Other);
            let rels = if rows.is_empty() {
                Arc::clone(&empty)
            } else {
                self.take_bytes(cost::HELD_RELS_BYTES)?;
                Arc::new(rows)
            };
            let part = self.plan_part(name, content_type, role, len, rels, parts.len())?;
            parts.push(part);
        }
        // The relationships parts nothing can own: held as the bytes they are.
        for name in pkg.parts() {
            if !is_rels_part(name) {
                continue;
            }
            let owned = rels_source(name).is_some_and(|source| source == "/" || (pkg.part(&source).is_some() && !is_rels_part(&source)));
            if owned && !self.held.unreadable_rels.contains(&fold(name)) {
                continue;
            }
            let len = pkg.part(name).map_or(0, <[u8]>::len);
            let content_type = pkg.content_type(name).unwrap_or("");
            let part = self.plan_part(name, content_type, PartRole::Other, len, Arc::clone(&empty), parts.len())?;
            parts.push(part);
        }
        let held_bytes = parts.iter().fold(0u64, |sum, part| sum.saturating_add(part.len as u64));
        self.notes.report.held_parts = parts.len();
        self.notes.report.held_bytes = held_bytes;
        self.held.pending = parts;

        let content_types = self.hold_content_types()?;
        self.take_bytes(chunk_of::<VsdxHeld>())?;
        doc.vsdx = Some(Arc::new(VsdxHeld {
            rels: std::mem::take(&mut self.held.rels),
            content_types,
            props_rest: std::mem::take(&mut self.held.props_rest),
            windows: self.held.windows.take(),
        }));
        self.give_bytes(price::with_margin(scratch));
        let tables = std::mem::take(&mut self.held.scratch);
        self.give_bytes(tables);
        Ok(())
    }

    /// Takes the rows of a relationships part whose source the model rebuilds. A part that cannot be parsed is held as the bytes it is, and the read goes on.
    fn hold_rels_of(&mut self, source: &str, rels_name: &str) -> Result<(), VsdxError> {
        if self.held.rels_done.contains(&fold(source)) {
            return Ok(());
        }
        match self.rels_of(source) {
            Ok(rels) => self.hold_rows(source, &rels.items, &HashSet::new()),
            Err(VsdxError::Package(_)) => {
                self.unreadable(rels_name);
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// The relationships from a held part, as rows of the model: its relationships part is parsed once, and a part that cannot be parsed is held as the bytes it
    /// is, and the part has no rows.
    fn rows_of_held(&mut self, name: &str) -> Result<Vec<Rel>, VsdxError> {
        let rels_name = rels_part_name(name);
        if self.pkg.part(&rels_name).is_none() {
            return Ok(Vec::new());
        }
        let rels = match self.rels_of(name) {
            Ok(rels) => rels,
            Err(VsdxError::Package(_)) => {
                self.unreadable(&rels_name);
                return Ok(Vec::new());
            }
            Err(other) => return Err(other),
        };
        work::held_rows(rels.items.len());
        let source: Arc<str> = Arc::from(name);
        self.take_bytes(arc_str(name.len()))?;
        let mut rows: Vec<Rel> = Vec::new();
        for row in &rels.items {
            self.note_role(row)?;
            self.take_bytes(cost::held_rel_bytes(rows.len(), row.id.len(), row.ty.len(), row.target_raw.len()))?;
            rows.push(doc_rel(&source, row));
        }
        Ok(rows)
    }

    fn unreadable(&mut self, rels_name: &str) {
        self.notes.warn("rels_unreadable", || {
            format!(
                "the relationships part {} is not readable, and is held as the bytes it is",
                crate::unheld::part_label("a relationships part", rels_name)
            )
        });
        self.hold_unreadable(rels_name);
    }

    /// Notes that the relationships part `rels_name` cannot be parsed, so that it is held as the bytes it is. The name is the part's own: a few bytes of the part's
    /// name, which the reader has in the package already.
    pub(crate) fn hold_unreadable(&mut self, rels_name: &str) {
        self.held.unreadable_rels.insert(fold(rels_name));
    }

    /// One held part but for its bytes, charged before it is made: the part, its strings and its place in the list, as prices. **Its bytes are not charged here**: they are
    /// the package's, which the pool deducted when the read began, and they move into the part when the read is done ([`move_parts`]).
    fn plan_part(
        &mut self,
        name: &str,
        content_type: &str,
        role: PartRole,
        len: usize,
        rels: Arc<Vec<Rel>>,
        have: usize,
    ) -> Result<PendingPart, VsdxError> {
        self.take_bytes(
            cost::HELD_PART_BYTES
                .saturating_add(arc_str(name.len()))
                .saturating_add(arc_str(content_type.len()))
                .saturating_add(push_bytes(have, POINTER_BYTES)),
        )?;
        // The list of parts to move is scratch: it is let go when the parts are in the document.
        self.scratch(push_bytes(have, std::mem::size_of::<PendingPart>()))?;
        Ok(PendingPart { name: Arc::from(name), media_type: Arc::from(content_type), role, rels, len })
    }

    /// The rows of `[Content_Types].xml` no part needs, each charged before it is made. A row is needed when it is the first row for its key and gives a part its
    /// type: the `Override` of a part the package has, or the `Default` of an extension a part with no `Override` has. The `Default` rows for `rels` and `xml` are
    /// every package's, and the writer makes them.
    fn hold_content_types(&mut self) -> Result<Vec<ContentTypeRow>, VsdxError> {
        let pkg: &'p Package = self.pkg;
        let types = pkg.content_types();
        let rows = types.defaults.len().saturating_add(types.overrides.len());
        work::type_rows(rows);
        if rows == 0 {
            return Ok(Vec::new());
        }
        let parts = pkg.parts().count();
        let scratch = (parts as u64).saturating_add(rows as u64).saturating_mul(192);
        self.take_bytes(scratch)?;
        let mut folded: HashSet<String> = HashSet::with_capacity(parts);
        folded.extend(pkg.parts().map(fold));
        let mut overridden: HashSet<String> = HashSet::with_capacity(types.overrides.len());
        overridden.extend(types.overrides.iter().map(|(part, _)| fold(&lenient_decode(part))));
        let mut relied_on: HashSet<&str> = HashSet::new();
        for name in &folded {
            if !overridden.contains(name)
                && let Some(extension) = extension_of(name)
            {
                relied_on.insert(extension);
            }
        }
        let mut held: Vec<ContentTypeRow> = Vec::new();
        let mut seen_defaults: HashSet<String> = HashSet::new();
        for (extension, content_type) in &types.defaults {
            let key = extension.to_ascii_lowercase();
            let first = seen_defaults.insert(key.clone());
            let standard = (key == "rels" && content_type == CT_RELS) || (key == "xml" && content_type == CT_XML);
            if first && (relied_on.contains(key.as_str()) || standard) {
                continue;
            }
            self.take_bytes(cost::held_type_row_bytes(held.len(), extension.len(), content_type.len()))?;
            held.push(ContentTypeRow::Default { extension: Arc::from(extension.as_str()), content_type: Arc::from(content_type.as_str()) });
        }
        let mut seen_overrides: HashSet<String> = HashSet::new();
        for (part, content_type) in &types.overrides {
            let key = fold(&lenient_decode(part));
            let first = seen_overrides.insert(key.clone());
            if first && folded.contains(&key) {
                continue;
            }
            self.take_bytes(cost::held_type_row_bytes(held.len(), part.len(), content_type.len()))?;
            held.push(ContentTypeRow::Override { part: Arc::from(part.as_str()), content_type: Arc::from(content_type.as_str()) });
        }
        self.give_bytes(price::with_margin(scratch));
        Ok(held)
    }
}

/// Moves the bytes of the held parts out of the package into the document, once the read is done and nothing reads the package again (F1): the bytes
/// live once, in the parts, and are not a copy beside the package's. **What no part keeps is let go first**, the bytes of every other part of the package (the pages, the
/// pictures the model copied), so that the copy of each held part, which an `Arc<[u8]>` needs, is made beside less than the read held; the package gives each held part up
/// as it is copied. The bytes are the pool's already, as the package it deducted, so they are not charged again; `memory_bytes` gains them, as the model's own.
pub(crate) fn move_parts(pkg: &mut Package, pending: Vec<PendingPart>, doc: &mut Document, report: &mut ReadReport) {
    if pending.is_empty() {
        return;
    }
    let kept: HashSet<String> = pending.iter().map(|part| fold(&part.name)).collect();
    pkg.release_parts(|name| kept.contains(&fold(name)));
    let mut parts: Vec<Arc<Part>> = Vec::with_capacity(pending.len());
    let mut moved = 0u64;
    for part in pending {
        // A part the package has is always there: the plan was made from it. Were it not, the part would be empty, and the fuzz property says so.
        let bytes = pkg.take_part(&part.name).unwrap_or_default();
        work::held_bytes(bytes.len());
        moved = moved.saturating_add(arc_str(bytes.len()));
        parts.push(Arc::new(Part {
            format: Format::Vsdx,
            name: part.name,
            media_type: part.media_type,
            role: part.role,
            manifest: RawAttrs::new(),
            rels: part.rels,
            bytes: Arc::from(bytes),
        }));
    }
    // The parts of the other format that DiaCraft's part carried (`carry.rs`) are in the document already, and come after the package's own: the model may hold no more than its cap of
    // parts, and a carried part that would pass it is left out and said.
    let carried: Vec<Arc<Part>> = doc.parts.iter().cloned().collect();
    let room = diacraft_doc::MAX_PARTS.saturating_sub(parts.len());
    let over = carried.len().saturating_sub(room);
    parts.extend(carried.into_iter().take(room));
    doc.parts = SharedVec::from_vec(parts);
    if over > 0 {
        let count = report.warning_counts.entry("diacraft_part_over_the_cap").or_insert(0);
        *count = count.saturating_add(1);
        if report.warnings.len() < crate::limits::MAX_REPORT_ENTRIES {
            report.warnings.push(format!("{over} parts that DiaCraft's part carries are left out: the drawing holds no more parts"));
        }
        report.carry.parts_kept = report.carry.parts_kept.saturating_sub(over as u64);
    }
    report.stats.memory_bytes = report.stats.memory_bytes.saturating_add(moved);
}

/// The bytes of an `Arc<T>` of the model.
fn chunk_of<T>() -> u64 {
    diacraft_sheetxml::model::cost::chunk(16 + std::mem::size_of::<T>()) as u64
}

/// A relationship row as the model holds it: the id, the type and the target as written.
fn doc_rel(source: &Arc<str>, row: &OpcRel) -> Rel {
    Rel {
        source: Arc::clone(source),
        id: Arc::from(row.id.as_str()),
        kind: Arc::from(row.ty.as_str()),
        target: Arc::from(row.target_raw.as_str()),
        external: row.mode == TargetMode::External,
    }
}

/// A `property` of a custom properties part as the list holds it: `name`, the other attributes, and one child element that holds only text, which is the
/// value, and whose name as written is the type. `None` when it says more.
fn custom_prop(el: &El) -> Option<CustomProp> {
    if !el.is("property") {
        return None;
    }
    let mut name = None;
    let mut raw = RawAttrs::new();
    for (k, v) in &el.attrs {
        if k == "name" && name.is_none() {
            name = Some(Arc::<str>::from(v.as_str()));
        } else {
            raw.push(k, v);
        }
    }
    let mut value_type: Option<(&El, Arc<str>)> = None;
    for node in &el.children {
        match node {
            Node::El(child)
                if value_type.is_none() && child.attrs.is_empty() && child.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) =>
            {
                value_type = Some((child, Arc::from(child.name.as_str())));
            }
            Node::Text(t) | Node::CData(t) if crate::passthrough::is_blank(t) => {}
            _ => return None,
        }
    }
    let (value, value_type) = value_type?;
    Some(CustomProp { format: Format::Vsdx, name: name?, value_type, value: Arc::from(value.text()), raw })
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_opc::{Limits as OpcLimits, PackageWriter};

    const TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"bin\" ContentType=\"application/octet-stream\"/></Types>";

    fn package() -> Package {
        let mut writer = PackageWriter::new(TYPES.as_bytes()).unwrap();
        writer.add("/page.xml", &[b'p'; 400]).unwrap();
        writer.add("/other.xml", &[b'o'; 300]).unwrap();
        writer.add("/held.bin", &[7u8; 50]).unwrap();
        writer.add("/held2.bin", &[8u8; 60]).unwrap();
        let bytes = writer.finish().unwrap();
        Package::read(&bytes, &OpcLimits::default()).unwrap()
    }

    fn pending(name: &str, len: usize) -> PendingPart {
        PendingPart {
            name: Arc::from(name),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            rels: Arc::new(Vec::new()),
            len,
        }
    }

    #[test]
    fn the_move_lets_go_of_every_part_it_does_not_keep_and_gives_the_held_ones_to_the_model() {
        let mut pkg = package();
        assert_eq!(pkg.part("/page.xml").map(<[u8]>::len), Some(400));
        let mut doc = Document::new_blank();
        let mut report = ReadReport::default();
        move_parts(&mut pkg, vec![pending("/held.bin", 50), pending("/held2.bin", 60)], &mut doc, &mut report);
        // What the model does not hold is let go before the copies are made (the package keeps the names, with no bytes).
        assert_eq!(pkg.part("/page.xml").map(<[u8]>::len), Some(0), "a part nothing keeps is freed");
        assert_eq!(pkg.part("/other.xml").map(<[u8]>::len), Some(0));
        // What it holds moved: the bytes are the model's, and the package has none of them.
        let held: Vec<(&str, &[u8])> = doc.parts.iter().map(|p| (&*p.name, &*p.bytes)).collect();
        assert_eq!(held, [("/held.bin", &[7u8; 50][..]), ("/held2.bin", &[8u8; 60][..])]);
        assert_eq!(pkg.part("/held.bin").map(<[u8]>::len), Some(0));
        // The figure of the model's own memory gains what moved, and a read that held nothing moves nothing.
        assert!(report.stats.memory_bytes >= 110, "{}", report.stats.memory_bytes);
        let mut again = package();
        let mut none = Document::new_blank();
        move_parts(&mut again, Vec::new(), &mut none, &mut ReadReport::default());
        assert_eq!(again.part("/page.xml").map(<[u8]>::len), Some(400), "nothing is let go when nothing is held");
        assert!(none.parts.is_empty());
    }
}
