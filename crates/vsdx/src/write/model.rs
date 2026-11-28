//! The model writer's trees and its package: the parts it makes from the model, the relationships and content types, and the check that what it wrote reads (`held.rs` is its plan).
//!
//! Every tree is made inside the room of one part (`build::PartRoom`), so a model whose shapes hold a part's worth of items twice over is refused in words at the shape that passes the cap and not
//! left to the read-back. A part is written as soon as it is made, and a held part is written from the bytes the model holds: nothing is held twice.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{ForeignData, Media, Page, RawAttrs, Shape, ShapeList, Style};
use diacraft_opc::name::{fold, resolve};
use diacraft_opc::xml::{El, Node};
use diacraft_opc::{CONTENT_TYPES_ITEM, MAX_PACKAGE_XML_BYTES, MAX_RELATIONSHIPS, PackageWriter, item_name};
use diacraft_sheetxml::build::sheet_only_el;

use super::build::{self, PartRoom, element, element_with_kept, master_row_el_with, page_row_el_with, rest_of};
use super::held::{Body, Book, CT_SOLUTION, CT_SOLUTIONS, Entry, IMAGE, MediaPlan, RowSet};
use super::parts::{NS_RELS, NS_TYPES, RelRow, Tree, VIS, rel_type, relative_target, rels_name};
use super::{R, WriteError, Written, masters_used};
use crate::limits::Limits;
use crate::ns::{NS_2012, NS_REL, Scope, ScopeExt};
use diacraft_doc::{DocProps, Format, MAX_SHAPE_DEPTH, Part, RawNode};

/// The root of a part the writer makes: the standard declarations, **the namespace the part was in when it was in that of 2011** (its declaration is held in `kept`), and what the part held beside them.
pub(super) fn root_el(name: &str, preserve: bool, typed: Vec<Node>, kept: Option<&diacraft_doc::Kept>) -> R<El> {
    let mut attrs = root_attrs(preserve);
    if let Some(ns) = kept.and_then(|k| k.raw.get("xmlns"))
        && let Some(declared) = attrs.iter_mut().find(|(n, _)| n == "xmlns")
    {
        declared.1 = ns.to_owned();
    }
    // A page or master part whose root bound `r` elsewhere keeps that binding (`build::ForeignR`: the writer's own `Rel`s declare theirs).
    if build::ForeignR::active()
        && let Some(ns) = kept.and_then(|k| k.raw.get("xmlns:r"))
        && let Some(declared) = attrs.iter_mut().find(|(n, _)| n == "xmlns:r")
    {
        declared.1 = ns.to_owned();
    }
    element_with_kept(name, attrs, typed, kept)
}

/// The attributes of the root of a part the writer makes: the standard declarations and, for a part the model has no root of its own for, `xml:space="preserve"`, which a drawing's parts have.
fn root_attrs(preserve: bool) -> Vec<(String, String)> {
    let mut attrs = vec![("xmlns".to_owned(), NS_2012.to_owned()), ("xmlns:r".to_owned(), NS_REL.to_owned())];
    if preserve {
        attrs.push(("xml:space".to_owned(), "preserve".to_owned()));
    }
    attrs
}

/// Whether the document part of a model read from a package is written with a `FaceNames`: **only when the model has a face**, or the table held something of its own beside its faces (an
/// element no field reads, kept as it was). MS-VSDX asks a `FaceNames` for one `FaceName` at least, so an empty one makes a valid file invalid: **no save writes one** (
/// found the VSDX written after an `.odg` trip with an empty table in 10 files, the F1b). A file that had none (`NoFaceNames`) or an empty one is written with none while the model has
/// no face, which the content check counts as the same (an empty container is an absent one); an empty table written back empty gives way to the schema.
fn has_font_table(doc: &diacraft_doc::Document) -> bool {
    !doc.fonts.is_empty() || rest_of(&doc.ext, "FaceNames").is_some_and(|rest| !rest.kept.is_empty())
}

/// Where [`weave`](diacraft_sheetxml::build::weave) puts the typed child `at` among the nodes it makes: after the `at` typed children before it and after every unknown node that followed `at`
/// of them or fewer (weave puts a node that followed `n` typed children just before the typed child `n`, and one whose count does not fit last).
pub(crate) fn typed_place(passthrough: &diacraft_doc::Passthrough, at: usize) -> usize {
    let before = passthrough.iter().filter(|p| usize::try_from(p.after).is_ok_and(|after| after <= at)).count();
    at.saturating_add(before)
}

// ------------------------------------------------------------------------------------------------------------------------ the rows

/// The relationships part of a source with these rows: held to the rows and the bytes a package reads one with.
fn rels_bytes(source: &str, rows: &RowSet) -> R<Vec<u8>> {
    let name = rels_name(source);
    if rows.rows.len() > MAX_RELATIONSHIPS {
        return Err(WriteError::Limit("more relationships from one part than a package holds"));
    }
    let children: Vec<Node> = rows.rows.iter().map(|row| Node::El(rel_row_el(row))).collect();
    let root = element("Relationships", vec![("xmlns".to_owned(), NS_RELS.to_owned())], children);
    let bytes = Tree::new(root).bytes(&name)?;
    if bytes.len() as u64 > MAX_PACKAGE_XML_BYTES {
        return Err(WriteError::Limit("the relationships from one part would be larger than a package allows for them"));
    }
    Ok(bytes)
}

fn rel_row_el(row: &RelRow) -> El {
    let mut attrs = vec![("Id".to_owned(), row.id.clone()), ("Type".to_owned(), row.ty.clone()), ("Target".to_owned(), row.target.clone())];
    if row.external {
        attrs.push(("TargetMode".to_owned(), "External".to_owned()));
    }
    element("Relationship", attrs, Vec::new())
}

// ------------------------------------------------------------------------------------------------------------------------ pictures

/// The extension and the content type a picture's part is given: the content type the model holds when it is one of a picture, else what the picture's first bytes say it is (BMP, EMF, GIF, JPEG,
/// PNG and TIFF), else an opaque part.
pub(super) fn picture_type(media: &Media) -> (&'static str, &'static str) {
    let by_type = match &*media.content_type {
        "image/png" => Some((".png", "image/png")),
        "image/jpeg" => Some((".jpeg", "image/jpeg")),
        "image/gif" => Some((".gif", "image/gif")),
        "image/bmp" => Some((".bmp", "image/bmp")),
        "image/x-emf" => Some((".emf", "image/x-emf")),
        "image/tiff" => Some((".tiff", "image/tiff")),
        _ => None,
    };
    by_type.unwrap_or_else(|| picture_by_bytes(&media.bytes).unwrap_or((".bin", "application/octet-stream")))
}

/// The extension and the content type a picture's first bytes say it has, when they are one of the six formats a drawing draws (BMP, EMF, GIF, JPEG, PNG and TIFF): `None` for any other bytes.
pub(crate) fn picture_by_bytes(b: &[u8]) -> Option<(&'static str, &'static str)> {
    match b {
        [0x89, b'P', b'N', b'G', ..] => Some((".png", "image/png")),
        [0xFF, 0xD8, 0xFF, ..] => Some((".jpeg", "image/jpeg")),
        [b'G', b'I', b'F', b'8', ..] => Some((".gif", "image/gif")),
        [b'B', b'M', ..] => Some((".bmp", "image/bmp")),
        [b'I', b'I', 0x2A, 0x00, ..] | [b'M', b'M', 0x00, 0x2A, ..] => Some((".tiff", "image/tiff")),
        [1, 0, 0, 0, ..] if b.get(40..44) == Some(b" EMF") => Some((".emf", "image/x-emf")),
        _ => None,
    }
}

/// The content type a picture's part is given: the type the model holds for it when a reader draws a picture of that type, else the type its bytes say it has. **A type the model claims that no
/// reader draws** (a manifest row that says `application/octet-stream` of a PNG) **would leave the picture undrawn, or an object's fallback unfound** (`shape.rs`, `resolve_picture`: the part is found
/// by its type), so the bytes are believed instead.
pub(super) fn picture_content_type(media: &Media) -> String {
    let (_, by_bytes) = picture_type(media);
    if media.content_type.is_empty() || !crate::shape::is_picture_type(&media.content_type) {
        return by_bytes.to_owned();
    }
    media.content_type.to_string()
}

/// The attributes of a picture's `ForeignData` with the `CompressionType` its content type calls for, when it has none (a PNG, a JPEG or a GIF names its own type).
pub(super) fn with_compression(attrs: &RawAttrs, content_type: &str) -> RawAttrs {
    let mut out = attrs.clone();
    let word = match content_type {
        "image/png" => Some("PNG"),
        "image/jpeg" => Some("JPEG"),
        "image/gif" => Some("GIF"),
        _ => None,
    };
    if let Some(word) = word
        && out.get("CompressionType").is_none()
        && out.get("ForeignType") == Some("Bitmap")
    {
        out.push("CompressionType", word);
    }
    out
}

/// The rows a page part has made for the shapes it has been given so far: for each picture (by the allocation of its `Media`) and each object part (by its name), the `Id` of its row.
#[derive(Default)]
struct PageRows {
    pictures: HashMap<usize, String>,
    objects: HashMap<String, String>,
}

impl<'a> Book<'a> {
    /// Plans the parts of the pictures the shapes show: a picture is one part however many shapes show it, under the name it had when a shape names a relationship to it (the rows that
    /// reach it from held parts say that name), and under a new name when no shape does.
    pub(super) fn pictures(&mut self) -> R<()> {
        let doc = self.doc;
        let mut uses: Vec<(&'a Arc<Media>, bool)> = Vec::new();
        let mut by_ptr: HashMap<usize, usize> = HashMap::new();
        let mut lists: Vec<&'a ShapeList> = doc.pages.iter().map(|p| &p.shapes).chain(doc.masters.iter().map(|m| &m.shapes)).collect();
        let mut at = 0usize;
        while let Some(list) = lists.get(at).copied() {
            at += 1;
            for shape in list.iter() {
                if let Some(foreign) = &shape.foreign
                    && let Some(media) = &foreign.picture
                {
                    let next = uses.len();
                    let slot = *by_ptr.entry(Arc::as_ptr(media) as usize).or_insert(next);
                    if slot == next {
                        uses.push((media, false));
                    }
                    if let Some(entry) = uses.get_mut(slot) {
                        entry.1 |= foreign.rel_id.is_some();
                    }
                }
                if !shape.children.is_empty() {
                    lists.push(&shape.children);
                }
            }
        }
        // The names the pictures that shapes name a relationship to go by are reserved before any picture is named: a new picture on an earlier page does not take the name of one that was there.
        for (media, named) in &uses {
            if *named && super::held::plausible(&media.part) {
                self.reserve(&media.part);
            }
        }
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for (ptr_media, named) in uses {
            let ptr = Arc::as_ptr(ptr_media) as usize;
            let (extension, _) = picture_type(ptr_media);
            let key = fold(&ptr_media.part);
            // The same part under one name, with the same bytes, is one picture.
            if named
                && let Some(&i) = by_name.get(&key)
                && self.media.get(i).is_some_and(|m| m.media.bytes == ptr_media.bytes)
            {
                self.media_of.insert(ptr, i);
                continue;
            }
            let name = if named && super::held::plausible(&ptr_media.part) && !self.taken.contains(&key) {
                self.claim(&ptr_media.part);
                ptr_media.part.to_string()
            } else {
                self.fresh_name("/visio/media/", "image", extension, |w| &mut w.media)? // brand-ok: a part name the format defines
            };
            let at = self.media.len();
            by_name.insert(fold(&name), at);
            self.media_of.insert(ptr, at);
            let ct = picture_content_type(ptr_media);
            self.entries.push(Entry { name: name.clone(), ct, body: Body::Media(at) });
            self.media.push(MediaPlan { name, media: ptr_media });
        }
        Ok(())
    }

    /// Makes the DiaCraft solution's parts when the model has data, or carries data of the other formats (`carry.rs`). When the file's solutions index is a held part (another program has
    /// solutions in it), the data goes where the file has its part (`held_solutions`).
    pub(super) fn solutions(&mut self) -> R<()> {
        // What rides in the solution part, whatever that part is: the items, and the bytes that go in parts of their own.
        let natively = self.natively();
        self.natively_parts = natively.part_names(self.doc);
        let mut built = crate::carry::build(self.doc, self.target.macro_enabled, self.opts.drop_macros, &natively)?;
        self.warnings.append(&mut built.warnings);
        self.macros_dropped.append(&mut built.macros_dropped);
        let writable = self.doc.diacraft.as_deref().is_none_or(|data| crate::carry::data_version(data) == Some(diacraft_doc::DIACRAFT_VERSION));
        if !built.items.is_empty() && !writable {
            self.warn(
                "data of the other format that this drawing carries was not saved: the file's DiaCraft data is of a version this build does not read, and is kept as it is".to_owned(),
            );
            built = crate::carry::Built::default();
        }
        self.carried_report = super::Carried { items: built.items.len(), parts: built.blobs.len(), macros: built.macros, objects: built.objects };
        self.carried = (!built.items.is_empty()).then_some(built);
        let ty = rel_type(VIS, "solutions");
        let document = self.document_part.clone();
        let target =
            self.rows.get(&fold(&document)).and_then(|r| r.first_of(&ty)).and_then(|r| resolve(&document, &r.target)).map(|t| self.as_held(t));
        if let Some(target) = target
            && self.held_parts.iter().any(|p| fold(&p.name) == fold(&target))
        {
            return self.held_solutions(&target);
        }
        if self.doc.diacraft.is_none() && self.carried.is_none() {
            return Ok(());
        }
        let (index, _) = self.structural(&ty, ("solutions/", "solutions", ".xml"))?;
        let dir = super::parts::folder_of(&index).to_owned();
        let part = self.fresh_name(&dir, "solution", ".xml", |w| &mut w.solution)?;
        let rows = self.rows_of(&index);
        self.solution_rel = rows.add(&rel_type(VIS, "solution"), &relative_target(&index, &part), false);
        if self.solution_rel.is_none() {
            return Err(WriteError::Limit("no relationship id was left for the solution"));
        }
        self.entries.push(Entry { name: index, ct: CT_SOLUTIONS.to_owned(), body: Body::SolutionsIndex });
        self.entries.push(Entry { name: part.clone(), ct: CT_SOLUTION.to_owned(), body: Body::Solution });
        self.make_blobs(&part)
    }

    /// The parts of DiaCraft's own that the solution part `part` names: one for each byte string the items carry, `/diacraft/blob<n>.<ext>` (the least `n` after the last walk that nothing
    /// has taken, compared ASCII case-insensitively), typed by the media type of its bytes, with a relationship of its type from the solution part, whose `Id` the document item names it by.
    fn make_blobs(&mut self, part: &str) -> R<()> {
        let Some(mut built) = self.carried.take() else { return Ok(()) };
        let mut rels: Vec<String> = Vec::with_capacity(built.blobs.len());
        for (i, blob) in built.blobs.iter().enumerate() {
            let ext = diacraft_sheetxml::model::blob_extension(&blob.media_type);
            let name = self.fresh_name(crate::carry::BLOB_DIR, "blob", &format!(".{ext}"), |w| &mut w.blob)?;
            let id = self
                .rows_of(part)
                .add(crate::carry::BLOB_REL_TYPE, &relative_target(part, &name), false)
                .ok_or(WriteError::Limit("no relationship id was left for a part of the data of the other format"))?;
            self.carried_names.push(name.clone());
            self.entries.push(Entry { name, ct: crate::carry::content_type_of(&blob.media_type).to_owned(), body: Body::Blob(i) });
            rels.push(id);
        }
        built.name_blobs(&rels);
        self.carried = Some(built);
        Ok(())
    }

    /// Says in a warning that the model's DiaCraft data was not written, and why (as the splice writer does).
    fn data_not_saved(&mut self, why: &str) {
        self.warn(format!("DiaCraft's own settings (the Snap and Glue options with no flag in the file) were not saved: {why}"));
    }

    /// Says in a warning that the data of the other format was not carried, and why.
    fn items_not_carried(&mut self, why: &str) {
        if self.carried.take().is_some() {
            self.warn(format!("data of the other format that this drawing carries was not saved: {why}"));
        }
    }

    /// **The DiaCraft data of the model, when the file's solutions index is a held part** (another program's solutions share it, and the index and the parts it names are held as the bytes they are):
    /// the part the index's row for DiaCraft leads to takes the model's `Data` where the file has its own (the rest of the part is as it was; **a part that holds anything beside the `Data` gets no
    /// items of the other format's data**, and the save says so), or is written from the model when the reader rebuilt it (it held nothing but the `Data`), and a file that has no row for DiaCraft is
    /// given one, in a part of its own. **Another program's solutions are never read or changed.** A model with no data leaves the file's as it is. Nothing here fails: what cannot be written is left as
    /// it is, and the save says so (`write/diacraft.rs` does the same on the splice path).
    fn held_solutions(&mut self, index: &str) -> R<()> {
        if self.doc.diacraft.is_none() && self.carried.is_none() {
            return Ok(());
        }
        let limit = Limits::default().max_small_part_bytes;
        let Some(held) = self.held_parts.iter().find(|p| fold(&p.name) == fold(index)).copied() else { return Ok(()) };
        if held.bytes.len() as u64 > limit {
            self.data_not_saved("the file's solutions index is larger than DiaCraft reads");
            self.items_not_carried("the file's solutions index is larger than DiaCraft reads");
            return Ok(());
        }
        let Ok(tree) = Tree::parse(index, &held.bytes) else {
            self.data_not_saved("the file's solutions index is not XML");
            self.items_not_carried("the file's solutions index is not XML");
            return Ok(());
        };
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Solutions") {
            self.data_not_saved("the file's solutions index has another root than Solutions");
            self.items_not_carried("the file's solutions index has another root than Solutions");
            return Ok(());
        }
        let row = tree.root.elements().find(|e| sc.enter(e).is(e, "Solution") && e.attr("Name") == Some("DiaCraft"));
        let rel_id = row.and_then(|row| {
            let rsc = sc.enter(row);
            row.elements().find_map(|e| rsc.enter(e).rel_id(e)).map(str::to_owned)
        });
        match (row, rel_id) {
            (None, _) => self.add_held_solution(index, held, tree),
            (Some(_), None) => {
                self.data_not_saved("the file's solution named DiaCraft has no relationship");
                self.items_not_carried("the file's solution named DiaCraft has no relationship");
                Ok(())
            }
            (Some(_), Some(id)) => {
                let leads = held
                    .rels
                    .iter()
                    .find(|r| r.id.eq_ignore_ascii_case(&id) && !r.external)
                    .and_then(|r| resolve(index, &r.target))
                    .map(|n| self.as_held(n));
                match leads.filter(|name| super::held::plausible(name)) {
                    Some(name) => self.set_diacraft_part(&name),
                    None => {
                        self.data_not_saved("the file's solution named DiaCraft leads to no part");
                        self.items_not_carried("the file's solution named DiaCraft leads to no part");
                        Ok(())
                    }
                }
            }
        }
    }

    /// The part `name` that the file's row for DiaCraft leads to: its `Data` replaced by the model's when the file has the part as a held one (and then no items ride: the part holds more than
    /// DiaCraft's data), else the part is made from the model's data and the data of the other format.
    fn set_diacraft_part(&mut self, name: &str) -> R<()> {
        let limit = Limits::default().max_solution_part_bytes;
        let Some(held) = self.held_parts.iter().find(|p| fold(&p.name) == fold(name)).copied() else {
            // The reader rebuilt the part: it held nothing but the `Data` and the relationships to the parts of DiaCraft's own.
            self.claim(name);
            self.entries.push(Entry { name: name.to_owned(), ct: CT_SOLUTION.to_owned(), body: Body::Solution });
            return self.make_blobs(name);
        };
        self.items_not_carried("the file's DiaCraft part holds more than DiaCraft's data");
        let Some(data) = self.doc.diacraft.clone() else { return Ok(()) };
        if held.bytes.len() as u64 > limit {
            self.data_not_saved("the file's DiaCraft part is larger than DiaCraft reads");
            return Ok(());
        }
        let data_el = match diacraft_sheetxml::passthrough::node_to_tree(&RawNode::Element((*data).clone())) {
            Node::El(el) => el,
            _ => return Err(WriteError::Mismatch("the DiaCraft data is not an element")),
        };
        let Ok(mut tree) = Tree::parse(name, &held.bytes) else {
            self.data_not_saved("the file's DiaCraft part is not XML");
            return Ok(());
        };
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "SolutionXML") || tree.root.attr("Name") != Some("DiaCraft") {
            self.data_not_saved("the file's solution named DiaCraft is not a SolutionXML named DiaCraft");
            return Ok(());
        }
        let at = tree.root.children.iter().position(|n| super::diacraft::is_data(&sc, n));
        match at {
            Some(at) => {
                if let Some(slot) = tree.root.children.get_mut(at) {
                    *slot = Node::El(data_el);
                }
            }
            None => tree.root.children.push(Node::El(data_el)),
        }
        // Written again, so with no XML declaration, as `solution_tree` writes the part.
        tree.drop_declaration();
        let out = tree.bytes(name)?;
        if out.len() as u64 > limit {
            self.data_not_saved("the part would be larger than DiaCraft reads");
            return Ok(());
        }
        self.patched.insert(fold(name), out);
        Ok(())
    }

    /// A file whose held solutions index has no row for DiaCraft: the row and the relationship to a part of its own are added to the index, and the part is made from the model.
    fn add_held_solution(&mut self, index: &str, held: &'a Arc<Part>, mut tree: Tree) -> R<()> {
        let dir = super::parts::folder_of(index).to_owned();
        let name = self.fresh_name(&dir, "solution", ".xml", |w| &mut w.solution)?;
        let mut set = RowSet::of(held.rels.iter());
        let Some(id) = set.add(&rel_type(VIS, "solution"), &relative_target(index, &name), false) else {
            self.data_not_saved("the file has no free relationship id for the part");
            self.items_not_carried("the file has no free relationship id for the part");
            return Ok(());
        };
        let sc = Scope::empty().enter(&tree.root);
        let mut existing: Option<String> = None;
        'rows: for row in tree.root.elements() {
            let rsc = sc.enter(row);
            for rel in row.elements() {
                let esc = rsc.enter(rel);
                if let Some((attr, _)) = rel.attrs.iter().find(|(k, _)| esc.is_rel_id_attr(k)) {
                    existing = Some(attr.clone());
                    break 'rows;
                }
            }
        }
        let attr = match existing {
            Some(attr) => attr,
            None => {
                if !tree.root.attrs.iter().any(|(k, _)| k == "xmlns:r") {
                    tree.root.attrs.push(("xmlns:r".to_owned(), NS_REL.to_owned()));
                }
                "r:id".to_owned()
            }
        };
        let prefix = tree.root.prefix().to_owned();
        let rel = element("Rel", vec![(attr, id)], Vec::new());
        let mut row = element("Solution", vec![("Name".to_owned(), "DiaCraft".to_owned())], vec![Node::El(rel)]);
        build::with_prefix(&mut row, &prefix);
        tree.root.children.push(Node::El(row));
        let out = tree.bytes(index)?;
        self.patched.insert(fold(index), out);
        self.held_rows.insert(fold(index), set);
        self.entries.push(Entry { name: name.clone(), ct: CT_SOLUTION.to_owned(), body: Body::Solution });
        self.make_blobs(&name)
    }

    // ------------------------------------------------------------------------------------------------------------------ the package

    /// The stream of the content types: the `Default` rows, the `Override` rows, and the rows the model holds beside them.
    fn types_bytes(&self) -> R<Vec<u8>> {
        let mut rows: Vec<Node> = Vec::with_capacity(self.types.defaults.len() + self.types.overrides.len());
        for (extension, ct) in &self.types.defaults {
            let attrs = vec![("Extension".to_owned(), extension.clone()), ("ContentType".to_owned(), ct.clone())];
            rows.push(Node::El(element("Default", attrs, Vec::new())));
        }
        for (name, ct) in &self.types.overrides {
            let written = item_name(name).map_or_else(|| name.clone(), |item| format!("/{item}"));
            let attrs = vec![("PartName".to_owned(), written), ("ContentType".to_owned(), ct.clone())];
            rows.push(Node::El(element("Override", attrs, Vec::new())));
        }
        if let Some(vsdx) = self.vsdx {
            for row in &vsdx.content_types {
                if let diacraft_doc::ContentTypeRow::Override { part, content_type } = row {
                    let attrs = vec![("PartName".to_owned(), part.to_string()), ("ContentType".to_owned(), content_type.to_string())];
                    rows.push(Node::El(element("Override", attrs, Vec::new())));
                }
            }
        }
        let root = element("Types", vec![("xmlns".to_owned(), NS_TYPES.to_owned())], rows);
        let bytes = Tree::new(root).bytes(CONTENT_TYPES_ITEM)?;
        if bytes.len() as u64 > MAX_PACKAGE_XML_BYTES {
            return Err(WriteError::Limit("the rows of the content types would be larger than a package allows for the stream"));
        }
        Ok(bytes)
    }

    /// Writes the package.
    pub(super) fn emit(mut self) -> R<Written> {
        let types = self.types_bytes()?;
        let mut writer = PackageWriter::new(&types).map_err(WriteError::Package)?;
        let mut out = Sink { writer: &mut writer, written: 0 };
        let entries = std::mem::take(&mut self.entries);
        self.written = entries.iter().map(|e| fold(&e.name)).collect();
        let root = self.rows.remove("/").unwrap_or_default();
        out.rels("/", &root)?;
        let mut added: Vec<String> = Vec::new();
        let mut kept = 0usize;
        for entry in &entries {
            crate::work::model_part();
            match self.part_bytes(entry, &mut out)? {
                Made::Held => kept += 1,
                Made::New => added.push(entry.name.clone()),
            }
        }
        let written = out.written;
        let bytes = writer.finish().map_err(WriteError::Package)?;
        self.finish(bytes, written, added, kept)
    }

    /// Writes one part, and the relationships from it.
    fn part_bytes(&mut self, entry: &Entry<'a>, out: &mut Sink<'_>) -> R<Made> {
        let key = fold(&entry.name);
        match entry.body {
            Body::Held(part) => {
                // The bytes the part is written as are its own, but for the DiaCraft solution a model edits (`held_solutions`).
                let patched = self.patched.remove(&key);
                let bytes: &[u8] = patched.as_deref().unwrap_or(&part.bytes);
                out.put(&entry.name, bytes)?;
                crate::work::model_bytes(bytes.len());
                match self.held_rows.remove(&key) {
                    Some(rows) => out.rels(&entry.name, &rows)?,
                    None => out.rels(&entry.name, &RowSet::of(part.rels.iter()))?,
                }
                return Ok(Made::Held);
            }
            Body::Theme(theme) => out.put(&entry.name, &theme.bytes)?,
            Body::Blob(i) => {
                let blob = self
                    .carried
                    .as_ref()
                    .and_then(|c| c.blobs.get(i))
                    .ok_or(WriteError::Mismatch("a part of the data of the other format was planned and is not there"))?;
                out.put(&entry.name, &blob.bytes)?;
                crate::work::model_bytes(blob.bytes.len());
            }
            Body::Media(i) => {
                let media = self.media.get(i).ok_or(WriteError::Mismatch("a picture was planned and is not there"))?;
                out.put(&entry.name, &media.media.bytes)?;
            }
            Body::Object(i) => {
                let object = self
                    .embedded
                    .as_ref()
                    .and_then(|p| p.emb.objects.get(i))
                    .ok_or(WriteError::Mismatch("an object was planned and is not there"))?;
                out.put(&entry.name, &object.bytes)?;
                crate::work::model_bytes(object.bytes.len());
            }
            Body::Page(i) | Body::Master(i) => {
                let is_page = matches!(entry.body, Body::Page(_));
                let (tree, rows) = self.content(&entry.name, i, is_page)?;
                out.put(&entry.name, &tree.bytes(&entry.name)?)?;
                out.rels(&entry.name, &rows)?;
                return Ok(Made::New);
            }
            _ => {
                let tree = self.tree_of(entry)?;
                out.put(&entry.name, &tree.bytes(&entry.name)?)?;
            }
        }
        // The rows of a part the model makes from fields are those the writer made for it (the document, the indexes), or else those the model holds for it (a theme's, `windows.xml`'s).
        let rows = match self.rows.remove(&key) {
            Some(rows) => Some(rows),
            None => self.held.get(&key).map(|held| RowSet::of(held.iter().copied())),
        };
        if let Some(rows) = rows {
            out.rels(&entry.name, &rows)?;
        }
        Ok(Made::New)
    }

    /// The tree of a part the writer makes from the model's fields.
    fn tree_of(&mut self, entry: &Entry<'a>) -> R<Tree> {
        match entry.body {
            Body::Document => self.document_tree(),
            Body::Pages => self.index_tree(true),
            Body::Masters => self.index_tree(false),
            Body::Windows => self.windows_tree(),
            Body::Comments => Ok(Tree::new(crate::comments::root(&self.doc.comments))),
            Body::Core => self.core_tree(),
            Body::App => self.app_tree(),
            Body::Custom => self.custom_tree(),
            Body::SolutionsIndex => self.solutions_tree(),
            Body::Solution => self.solution_tree(),
            Body::Held(_) | Body::Theme(_) | Body::Media(_) | Body::Page(_) | Body::Master(_) | Body::Blob(_) | Body::Object(_) => {
                Err(WriteError::Mismatch("a part is not made from fields"))
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------- the document part

    fn document_tree(&self) -> R<Tree> {
        let _room = PartRoom::enter(0, 0);
        let doc = self.doc;
        let mut typed: Vec<Node> = Vec::new();
        if !doc.colors.is_empty() || rest_of(&doc.ext, "Colors").is_some() {
            typed.push(Node::El(build::colors_el_ext(&doc.colors, &doc.ext)?));
        }
        // A table the file did not have (a face the model gained since) is no typed child the unknown elements were counted after: it goes in once they are put back.
        let mut gained: Option<El> = None;
        if self.fresh {
            typed.push(Node::El(super::skeleton::face_names(doc)));
        } else if has_font_table(doc) {
            let table = build::fonts_el_ext(&doc.fonts, &doc.ext)?;
            if rest_of(&doc.ext, "NoFaceNames").is_some() {
                gained = Some(table);
            } else {
                typed.push(Node::El(table));
            }
        }
        let styles_at = typed.len();
        let styles: Vec<&Style> = doc.styles.iter().map(|s| &**s).collect();
        typed.push(Node::El(build::styles_el_ext(&styles, &doc.ext)?));
        typed.push(Node::El(sheet_only_el("DocumentSheet", &doc.sheet)));
        build::take_kept(&[&doc.passthrough])?;
        let mut children = diacraft_sheetxml::build::weave(typed, &[&doc.passthrough]);
        if let Some(table) = gained {
            children.insert(typed_place(&doc.passthrough, styles_at).min(children.len()), Node::El(table));
        }
        if self.fresh
            && !doc.passthrough.iter().any(|p| matches!(&p.node, RawNode::Element(e) if e.name.rsplit(':').next() == Some("DocumentSettings")))
        {
            children.insert(0, Node::El(super::skeleton::default_settings(doc)));
        }
        let root = root_el("VisioDocument", self.fresh, children, rest_of(&doc.ext, "VisioDocument"))?;
        Ok(Tree::new(root))
    }

    /// The pages index or the masters index: a row for each page or master, the `Rel` of each naming its row, and what the index holds beside them.
    fn index_tree(&self, pages: bool) -> R<Tree> {
        let _room = PartRoom::enter(0, 0);
        let doc = self.doc;
        let mut rows: Vec<Node> = Vec::new();
        if pages {
            for (i, page) in doc.pages.iter().enumerate() {
                let rel = self.page_rels.get(i).and_then(|r| r.as_deref());
                rows.push(Node::El(page_row_el_with(page, rel, !self.fresh && rest_of(&page.ext, "NoPageSheet").is_none())?));
            }
        } else {
            for (i, master) in doc.masters.iter().enumerate() {
                let rel = self.master_rels.get(i).and_then(|r| r.as_deref());
                rows.push(Node::El(master_row_el_with(master, rel, !self.fresh && rest_of(&master.ext, "NoPageSheet").is_none())?));
            }
        }
        let (name, key) = if pages { ("Pages", "Pages") } else { ("Masters", "Masters") };
        let root = root_el(name, self.fresh, rows, rest_of(&doc.ext, key))?;
        Ok(Tree::new(root))
    }

    // ---------------------------------------------------------------------------------------------------------- windows

    fn windows_tree(&mut self) -> R<Tree> {
        let Some(raw) = self.vsdx.and_then(|v| v.windows.as_ref()) else {
            return Ok(Tree::new(super::skeleton::windows_root(self.doc)));
        };
        // The held element is made as a tree: what it makes is taken from the room of the part first.
        let _room = PartRoom::enter(0, 0);
        build::take_raw(raw)?;
        let mut root = diacraft_sheetxml::passthrough::to_tree(raw);
        // **A drawing as it was read has had nothing deleted since** (drops a row whose page or master was deleted since): every window is the file's
        // own, one that names a page or master the file did not have included, and stays. After an edit the model holds no record of which pages and
        // masters the file had, so a window that names one the model does not have goes, whether it went since or never was.
        if !self.edited {
            return Ok(Tree::new(root));
        }
        let pages: HashSet<u32> = self.doc.pages.iter().map(|p| p.id).collect();
        let masters: HashSet<u32> = self.doc.masters.iter().map(|m| m.id).collect();
        let names =
            |el: &El, attr: &str, have: &HashSet<u32>| el.attr(attr).and_then(|v| v.parse::<u32>().ok()).is_some_and(|id| !have.contains(&id));
        // A window that names a page or a master that is not there is dropped.
        let before = root.children.len();
        root.children.retain(|n| !matches!(n, Node::El(w) if w.is("Window") && (names(w, "Page", &pages) || names(w, "Master", &masters))));
        if root.children.len() != before {
            self.warnings
                .push("a page or master was removed; comments, published-page lists and data recordsets that name it are not edited".to_owned());
            // **The window settings stay**: a drawing window that went with its page leaves its flags to a drawing window of their own.
            let flags = diacraft_doc::WindowFlags::of(raw);
            if let Some(made) = flags.written(&diacraft_sheetxml::passthrough::raw_element(&root)) {
                root = diacraft_sheetxml::passthrough::to_tree(&made);
            }
        }
        Ok(Tree::new(root))
    }

    // -------------------------------------------------------------------------------------------------------- properties

    fn core_tree(&self) -> R<Tree> {
        let _room = PartRoom::enter(0, 0);
        let p = &self.doc.props;
        let now = self.now.as_deref();
        let copy = self.opts.new_copy && now.is_some();
        let created = if copy || (self.fresh && p.created.is_none()) { now } else { p.created.as_deref() };
        let modified = if (self.edited || copy) && now.is_some() { now } else { p.modified.as_deref() };
        let rest = self.vsdx.and_then(|v| v.props_rest.iter().find(|(k, _)| &**k == "coreProperties").map(|(_, kept)| kept));
        let fields: &[(Arc<str>, diacraft_doc::Kept)] = self.vsdx.map_or(&[], |v| &v.props_rest);
        super::props::model_core(&super::props::CoreWrite { title: &self.doc.title, props: p, rest, fields, created, modified })
    }

    fn app_tree(&mut self) -> R<Tree> {
        let _room = PartRoom::enter(0, 0);
        let rest = self.vsdx.and_then(|v| v.props_rest.iter().find(|(k, _)| &**k == "Properties").map(|(_, kept)| kept));
        // An edited save names its pages in the lists the part holds: the model's names, in the group that lists the pages.
        let mut updated: Option<diacraft_doc::Kept> = None;
        // **A new document's lists are its own** (`WriteOptions::new_copy`): a drawing with no file yet, made from a template, names the template's pages in its lists, and where those
        // cannot be told from its masters' they are made anew from the model, as for a drawing that was never saved, and not left as the template had them with a warning for a person who
        // only made a drawing and saved it.
        let mut remade = false;
        if self.edited
            && !self.doc.pages.is_empty()
            && let Some(rest) = rest
        {
            let pages: Vec<String> = self.doc.pages.iter().map(|p| p.name.to_string()).collect();
            let masters: Vec<String> = self.doc.masters.iter().map(|m| m.name.as_deref().or(m.name_u.as_deref()).unwrap_or("").to_owned()).collect();
            match super::props::update_page_names(rest, &pages, &masters) {
                super::props::Titles::Changed(kept) => updated = Some(kept),
                super::props::Titles::Left if self.opts.new_copy && super::props::has_lists(rest) => {
                    updated = Some(super::props::without_lists(rest));
                    remade = true;
                }
                super::props::Titles::Left
                    if rest.kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if e.name.ends_with("TitlesOfParts"))) =>
                {
                    self.warnings.push("the list of page names in docProps/app.xml was not found, and is left as it was".to_owned());
                }
                _ => {}
            }
        }
        let rest = updated.as_ref().or(rest);
        // A part the model has nothing of is made as a new one is, with the page and master names it lists.
        let lists = (rest.is_none() || remade).then(|| {
            let pages = self.doc.pages.iter().map(|p| p.name.to_string()).collect();
            let masters = self.doc.masters.iter().map(|m| m.name.as_deref().or(m.name_u.as_deref()).unwrap_or("").to_owned()).collect();
            (pages, masters)
        });
        let fields: &[(Arc<str>, diacraft_doc::Kept)] = self.vsdx.map_or(&[], |v| &v.props_rest);
        super::props::model_app(&super::props::AppWrite {
            props: &self.doc.props,
            rest,
            fields,
            names_writer: self.edited,
            lists: lists.filter(|_| self.fresh || remade),
        })
    }

    fn custom_tree(&self) -> R<Tree> {
        let _room = PartRoom::enter(0, 0);
        let p: &DocProps = &self.doc.props;
        let list: Vec<&diacraft_doc::CustomProp> = p.custom.iter().filter(|c| c.format == Format::Vsdx).collect();
        super::props::model_custom(&list, self.fresh, self.opts.recalc_document)
    }

    // ---------------------------------------------------------------------------------------------------------- solutions

    fn solutions_tree(&self) -> R<Tree> {
        // The one row: the part is the solution's, named in the relationships of the index, and the row says which.
        let id = self.solution_rel.clone().unwrap_or_else(|| "rId1".to_owned());
        let rel = element("Rel", vec![("r:id".to_owned(), id)], Vec::new());
        let row = element("Solution", vec![("Name".to_owned(), "DiaCraft".to_owned())], vec![Node::El(rel)]);
        let attrs = vec![("xmlns".to_owned(), NS_2012.to_owned()), ("xmlns:r".to_owned(), NS_REL.to_owned())];
        Ok(Tree::new(element("Solutions", attrs, vec![Node::El(row)])))
    }

    fn solution_tree(&self) -> R<Tree> {
        let none = crate::carry::Built::default();
        let (data, _) = crate::carry::data_element(self.doc.diacraft.as_deref(), self.carried.as_ref().unwrap_or(&none))?;
        let Some(data_el) = data else { return Err(WriteError::Mismatch("the DiaCraft data went away")) };
        let _room = PartRoom::enter(0, 0);
        if let Some(raw) = &self.doc.diacraft {
            build::take_raw(raw)?;
        }
        let root = element("SolutionXML", vec![("Name".to_owned(), "DiaCraft".to_owned())], vec![Node::El(data_el)]);
        // No XML declaration: the solution parts of the files the VSDX application writes have none.
        let tree = Tree::bare(root);
        if tree.bytes("the DiaCraft part")?.len() as u64 > Limits::default().max_solution_part_bytes {
            return Err(WriteError::Limit("the DiaCraft data would make a part larger than DiaCraft reads"));
        }
        Ok(tree)
    }

    // ----------------------------------------------------------------------------------------------- pages and masters

    /// The part of a page or master, and the rows from it: the rows the model holds for it, the rows of the pictures its shapes show, and the rows to the masters and the background page they use.
    fn content(&mut self, part: &str, at: usize, is_page: bool) -> R<(Tree, RowSet)> {
        let doc = self.doc;
        let key = fold(part);
        // The rows of the part are the rows its item was given when it was placed (`held.rs`, `rows_of_item`): those of the part the item had, and no other part's.
        let mut rows = self.rows.remove(&key).unwrap_or_default();
        let (shapes, connects, root, shapes_kept, connects_kept, preserve, used, back) = if is_page {
            let page: &Arc<Page> = doc.pages.get(at).ok_or(WriteError::Mismatch("a page was planned and is not there"))?;
            let kept = (rest_of(&page.ext, "PageContents"), rest_of(&page.ext, "Shapes"), rest_of(&page.ext, "Connects"));
            (&page.shapes, &page.connects, "PageContents", kept.1, kept.2, self.fresh || page.part_name.is_none(), kept.0, page.back_page)
        } else {
            let master = doc.masters.get(at).ok_or(WriteError::Mismatch("a master was planned and is not there"))?;
            let kept = (rest_of(&master.ext, "MasterContents"), rest_of(&master.ext, "Shapes"), rest_of(&master.ext, "Connects"));
            (&master.shapes, &master.connects, "MasterContents", kept.1, kept.2, self.fresh || master.part_name.is_none(), kept.0, None)
        };
        // A page part that had none has its shapes with the `Rel`s its rows were given (`page_rows.rs`).
        let carried = if is_page { self.carried_shapes.remove(&at) } else { None };
        let shapes = carried.as_ref().unwrap_or(shapes);
        let attached = {
            let mut made = PageRows::default();
            self.attach(part, &mut rows, shapes, &mut made, 0)?
        };
        let list = attached.as_ref().unwrap_or(shapes);
        self.sync_rels(part, &mut rows, list, back, is_page)?;
        let _room = PartRoom::enter(0, 0);
        let _r = build::ForeignR::enter(used);
        let mut typed: Vec<Node> = Vec::new();
        if !list.is_empty() || shapes_kept.is_some() {
            typed.push(Node::El(build::shapes_el_with(list, 0, shapes_kept)?));
        }
        if !connects.is_empty() || connects_kept.is_some() {
            typed.push(Node::El(build::connects_el_with(connects, connects_kept)?));
        }
        let el = root_el(root, preserve, typed, used)?;
        Ok((Tree::new(el), rows))
    }

    /// **The rows of a page part that the model implies and the XML does not name**: one row to the part of each master the shapes use,
    /// and the one to the background page's part. A row that is missing is made, and **a row to a part that is not written** (a master or page that was taken out, a target the file did not have)
    /// **is not written**. **The rows the model does not explain are the file's own while the drawing is as it was read** (`WriteOptions::content_unchanged`): a row to a master no
    /// shape on the page uses, and one to a background page that is not the page's (R6 found five files with such a row and no `BackPage`, and says to keep it), stay, so that a visit writes every
    /// row the page had. **After an edit they are made from the model**: a row to a master the shapes no longer use goes (the last instance's deletion takes it out), and so does one to a
    /// background page that is no longer the page's (a writer that sets or clears `BackPage` updates it), since the model holds no earlier state to tell such a row from one the file always had.
    /// Every other row stays: one to a part that is no master and no background page (a held part, a page that is not a background page) is the file's own.
    fn sync_rels(&self, part: &str, rows: &mut RowSet, shapes: &ShapeList, back: Option<u32>, is_page: bool) -> R<()> {
        let master_ty = rel_type(VIS, "master");
        let page_ty = rel_type(VIS, "page");
        let leads_nowhere = |row: &RelRow, target: &str| (row.ty == master_ty || row.ty == page_ty) && !self.written.contains(target);
        if !is_page {
            // The rows of a master's part to masters and pages are no page's: the spec says nothing of them, and they stay unless they lead to a part that is not written.
            rows.drop_where(|row| {
                let Some(target) = resolve(part, &row.target).map(|t| fold(&t)).filter(|_| !row.external) else { return false };
                leads_nowhere(row, &target)
            });
            return Ok(());
        }
        // The masters the shapes use, by the part each is written as (two that share a part are one row), in the order of their ids.
        let mut used: Vec<&String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for id in masters_used(shapes) {
            if let Some(target) = self.master_part_of.get(&id)
                && seen.insert(fold(target))
            {
                used.push(target);
            }
        }
        let background = back.and_then(|id| self.page_part_of.get(&id)).map(|target| fold(target));
        rows.drop_where(|row| {
            if row.external {
                return false;
            }
            let Some(target) = resolve(part, &row.target).map(|t| fold(&t)) else { return false };
            if leads_nowhere(row, &target) {
                return true;
            }
            if !self.edited {
                return false;
            }
            if row.ty == master_ty {
                return self.written_masters.contains(&target) && !seen.contains(&target);
            }
            row.ty == page_ty && self.background_pages.contains(&target) && background.as_deref() != Some(target.as_str())
        });
        let leads = |ty: &str| -> HashSet<String> {
            rows.rows.iter().filter(|r| r.ty == ty && !r.external).filter_map(|r| resolve(part, &r.target)).map(|t| fold(&t)).collect()
        };
        let (have_masters, have_pages) = (leads(&master_ty), leads(&page_ty));
        for target in used {
            if !have_masters.contains(&fold(target)) {
                rows.add(&master_ty, &relative_target(part, target), false).ok_or(WriteError::Limit("no relationship id was left for a master"))?;
            }
        }
        if let Some(target) = back.and_then(|id| self.page_part_of.get(&id))
            && !have_pages.contains(&fold(target))
        {
            rows.add(&page_ty, &relative_target(part, target), false)
                .ok_or(WriteError::Limit("no relationship id was left for a background page"))?;
        }
        Ok(())
    }

    /// The shapes with the pictures they show attached: a `Foreign` shape that shows a picture and names no relationship to it is another format's (an `.odg`'s frame with an image) or a copy of
    /// one, so the part is the picture's own and a row is made; a shape that names a relationship has its row made when the part has none. → the shapes, or `None`
    /// when no shape needed it.
    fn attach(&self, part: &str, rows: &mut RowSet, list: &ShapeList, made: &mut PageRows, depth: usize) -> R<Option<ShapeList>> {
        if depth >= MAX_SHAPE_DEPTH {
            return Err(WriteError::Limit("levels of nested shapes"));
        }
        let mut changed: Vec<(usize, Arc<Shape>)> = Vec::new();
        for (at, shape) in list.iter().enumerate() {
            let mut out: Option<Shape> = None;
            // An object frame an `.odg` made: its `Rel` names the object part, and the fallback picture hangs off that part, so the page has no row to the picture.
            if let Some(object) = self.object_lead(shape)
                && let Some(foreign) = &shape.foreign
            {
                let id = match made.objects.get(&object.lead.target) {
                    Some(id) => id.clone(),
                    None => {
                        let id = rows
                            .add(&object.lead.ty, &relative_target(part, &object.lead.target), false)
                            .ok_or(WriteError::Limit("no relationship id was left for an object"))?;
                        made.objects.insert(object.lead.target.clone(), id.clone());
                        id
                    }
                };
                let mut given = ForeignData::clone(foreign);
                given.rel_id = Some(Arc::from(id));
                given.attrs = object.attrs;
                let mut copy = Shape::clone(shape);
                copy.foreign = Some(Arc::new(given));
                out = Some(copy);
            } else if let Some(foreign) = &shape.foreign
                && let Some(media) = &foreign.picture
                && let Some(id) = self.picture_row(part, rows, foreign, media, made)?
            {
                let mut copy = Shape::clone(shape);
                let mut attached = ForeignData::clone(foreign);
                attached.rel_id = Some(Arc::from(id));
                attached.attrs = with_compression(&foreign.attrs, picture_type(media).1);
                copy.foreign = Some(Arc::new(attached));
                out = Some(copy);
            }
            if !shape.children.is_empty()
                && let Some(kids) = self.attach(part, rows, &shape.children, made, depth.saturating_add(1))?
            {
                out.get_or_insert_with(|| Shape::clone(shape)).children = kids;
            }
            if let Some(out) = out {
                changed.push((at, Arc::new(out)));
            }
        }
        if changed.is_empty() {
            return Ok(None);
        }
        let mut out = list.clone();
        for (at, shape) in changed {
            out.set(at, shape).map_err(|_| WriteError::Mismatch("a shape of the list went away"))?;
        }
        Ok(Some(out))
    }

    /// The row of the part to a shape's picture: made when the part has no row of that id. → the new id, when the shape named none.
    fn picture_row(&self, part: &str, rows: &mut RowSet, foreign: &ForeignData, media: &Arc<Media>, made: &mut PageRows) -> R<Option<String>> {
        let plan: &MediaPlan<'_> = self
            .media_of
            .get(&(Arc::as_ptr(media) as usize))
            .and_then(|i| self.media.get(*i))
            .ok_or(WriteError::Mismatch("a picture was shown and is not planned"))?;
        let target = relative_target(part, &plan.name);
        if let Some(id) = &foreign.rel_id {
            if rows.get(id).is_none() {
                rows.push(RelRow { id: id.to_string(), ty: IMAGE.to_owned(), target, external: false });
            }
            return Ok(None);
        }
        let key = Arc::as_ptr(media) as usize;
        if let Some(id) = made.pictures.get(&key) {
            return Ok(Some(id.clone()));
        }
        let id = rows.add(IMAGE, &target, false).ok_or(WriteError::Limit("no relationship id was left for a picture"))?;
        made.pictures.insert(key, id.clone());
        Ok(Some(id))
    }

    // ------------------------------------------------------------------------------------------------------- the check

    /// The package read again with the reader: it must read, and hold the parts that were written.
    fn finish(self, bytes: Vec<u8>, written: usize, added: Vec<String>, kept: usize) -> R<Written> {
        // The parts the model holds as bytes, written from them, are checked against them as the file is read and not inflated beside them (`read::Known`).
        let crate::read::ReadBack { doc: mut document, report: read_back, .. } = crate::read::read_back(
            &bytes,
            &Limits::default(),
            &crate::read::Known::of(self.doc).with_parts(
                self.carried
                    .iter()
                    .flat_map(|c| c.blobs.iter().map(|b| &b.bytes))
                    .zip(&self.carried_names)
                    .map(|(bytes, name)| (name.as_str(), bytes)),
            ),
        )
        .map_err(super::read_back_failed)?;
        // The parts the model holds as bytes are the model's: the baseline holds those, and not a second copy of what the check read (`read_back`). **But only the ones the file holds** (
        // `objects::baseline_parts`): a save that left a macro project out, or wrote objects of the other format as embeddings made from their files, holds the embeddings and not those files or that
        // project, and a caller that became this model (the engine after a save across) held an object shape that named an embedding it did not have, which the next save from the model wrote as a
        // relationship to nothing.
        document.parts = super::objects::baseline_parts(self.doc, &self.natively_parts, &self.macros_dropped, self.made_embeddings());
        if self.fresh {
            // The baseline of the next save into this file is the caller's model where the file holds more than it does: the `DocumentSettings` and the faces a new drawing's file is given beside what
            // the model has (as a save of a new drawing gives them, `mod.rs`), which an edit of the model would otherwise be told from.
            document.passthrough = self.doc.passthrough.clone();
            document.fonts = self.doc.fonts.clone();
        }
        if read_back.stats.parts != written {
            return Err(WriteError::SelfCheck("the parts of the file are not the parts that were written".to_owned()));
        }
        Ok(Written {
            bytes,
            document,
            read_back,
            warnings: self.warnings,
            rewritten: Vec::new(),
            added,
            removed: self.removed,
            kept,
            macros_dropped: self.macros_dropped,
            carried: self.carried_report,
            natively: self.natively_parts,
        })
    }
}

/// What a part was made from.
enum Made {
    Held,
    New,
}

/// The package being written, and how many parts it holds.
struct Sink<'w> {
    writer: &'w mut PackageWriter,
    written: usize,
}

impl Sink<'_> {
    fn put(&mut self, name: &str, bytes: &[u8]) -> R<()> {
        self.writer.add(name, bytes).map_err(WriteError::Package)?;
        self.written = self.written.saturating_add(1);
        Ok(())
    }

    /// The relationships part of `source`, when it has rows.
    fn rels(&mut self, source: &str, rows: &RowSet) -> R<()> {
        if rows.rows.is_empty() {
            return Ok(());
        }
        let bytes = rels_bytes(source, rows)?;
        self.put(&rels_name(source), &bytes)
    }
}
