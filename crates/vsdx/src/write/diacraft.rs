//! The DiaCraft solution part.
//!
//! `Document::diacraft` is the `Data` element of the solution part named `DiaCraft`. A save writes it when it differs from the model the file was
//! read as:
//!
//! * **the file has the part** (a `Solution` named `DiaCraft` in the solutions index, whose part is a `SolutionXML` named `DiaCraft`): the `Data` element
//!   in DiaCraft's URN is replaced where it is, and everything else of the part is as it was. When the model has **no data left**, the `Data` is taken out,
//!   and a part that holds nothing else is removed with its relationship, its content type and its row in the index (and the index too, when no row is
//!   left): a file with no DiaCraft data has none of it;
//! * **the file has none**: the part is made (`visio/solutions/solution#.xml`, the next free number), the `Solution` row in the solutions index, which is
//!   made too when the file has none (`visio/solutions/solutions.xml`, the document's relationship to it), and the types and relationships the rows of
//!   R27 give. A file whose model holds no DiaCraft data gets none of this;
//! * **anything the writer cannot use is kept as it is, and the save says so in a warning, and never fails**: a solutions index that is not XML, has another
//!   root, or whose relationships do not read; a `Solution` named `DiaCraft` that is not in the shape of R27; a part over the size the reader reads; a part whose own
//!   relationships do not read (kept as it is); another part's relationships that do not read, which the reader takes to name a part of DiaCraft's (the part is then patched as
//!   it is, gets no items, and no part of DiaCraft's own is taken out); and a part whose `Data` the reader did not keep (it has no `version`, or the document could not afford its items: the model has none to compare, so the writer
//!   does not replace what it did not read). The options the model holds are then not saved.
//!
//! **The data of the other format rides here too** (`carry.rs`): the items of the model's data that VSDX cannot say are written into the `Data` beside DiaCraft's own settings, and the
//! bytes that go with them into parts of DiaCraft's own (`/diacraft/blob<n>.<ext>`, each linked by a relationship from the solution part). **They are written when the part holds nothing but
//! the `Data` and the relationships to those parts** (the part is DiaCraft's then, and is made again as a whole whenever what it carries differs from what the file carried); a part that holds
//! anything else (another program's content beside the `Data`) is patched as before, gets no items, and the save says so. A save that has no data of either kind left takes the part, its parts
//! and its row out.
//!
//! Another writer's solutions are never read or changed: the index gains a row and nothing else, and its rows keep their names, attributes and order
//! (the index is written from its tree, which R9 allows).

use std::sync::Arc;

use diacraft_opc::name::{fold, resolve};
use diacraft_opc::xml::{El, Node};

use crate::carry::{BLOB_REL_TYPE, Built};
use crate::report::Variant;

use super::build::element;
use super::parts::{Tree, VIS, folder_of, rel_type, relative_target};
use super::{Plan, R, WriteError};
use crate::limits::Limits;
use crate::ns::{NS_REL, Scope, ScopeExt};

const CT_SOLUTIONS: &str = "application/vnd.ms-visio.solutions+xml"; // brand-ok: a content type the format defines
const CT_SOLUTION: &str = "application/vnd.ms-visio.solution+xml"; // brand-ok: a content type the format defines

/// What the file holds for DiaCraft.
enum Held {
    /// A part the writer can use: the index and the relationship that reach it, and its name.
    Part { index: String, rel_id: String, part: String },
    /// The file has a solutions package or a `Solution` named `DiaCraft` that the writer cannot use, and why.
    Unusable(&'static str),
    /// No `Solution` named `DiaCraft`; the index to put the row in when the file has a usable one, else none (it is made).
    None(Option<String>),
}

/// Whether an element is the `Data` of DiaCraft: that local name, in DiaCraft's URN or in the one earlier builds wrote (APPID: the save replaces a `Data` of either, where it is,
/// with the model's, which is written in the current one, and never adds a second beside one that is there).
pub(super) fn is_data(sc: &Scope, node: &Node) -> bool {
    matches!(node, Node::El(e) if e.name.rsplit(':').next() == Some("Data") && sc.enter(e).uri_of_element(e).as_deref().is_some_and(diacraft_doc::is_diacraft_urn))
}

/// Whether a `SolutionXML` holds nothing: no element, comment or instruction, and no text but blanks.
fn holds_nothing(root: &El) -> bool {
    root.children.iter().all(|n| matches!(n, Node::Text(t) if t.trim().is_empty()))
}

impl Plan<'_> {
    /// Whether the type this save writes allows a macro project: the type asked for, else the type the file has.
    fn target_macros(&self) -> bool {
        let current = self.pkg.content_type(&self.document_part).and_then(Variant::from_content_type);
        self.opts.variant.or(current).is_some_and(|v| v.macro_enabled)
    }

    /// Writes the DiaCraft data of the model, and the data of the other format that it carries, when either differs from what the file carried.
    pub(super) fn diacraft(&mut self) -> R<()> {
        let (base, doc) = (self.base, self.doc);
        // What rides: the items and the bytes of the model, and of the model the file was read as. The part is written when either differs.
        let natively = self.natively();
        self.natively_parts = natively.part_names(doc);
        let mut now = crate::carry::build(doc, self.target_macros(), self.opts.drop_macros, &natively)?;
        self.warnings.append(&mut now.warnings);
        self.macros_dropped.append(&mut now.macros_dropped);
        let writable = doc.diacraft.as_deref().is_none_or(|d| crate::carry::data_version(d) == Some(diacraft_doc::DIACRAFT_VERSION));
        if !now.items.is_empty() && !writable {
            self.warnings.push("data of the other format that this drawing carries was not saved: the file's DiaCraft data is of a version this build does not read, and is kept as it is".to_owned());
            now = Built::default();
        }
        self.carried_report = super::Carried { items: now.items.len(), parts: now.blobs.len(), macros: now.macros, objects: now.objects };
        let before = crate::carry::build(base, true, false, &crate::carry::Natively::default()).unwrap_or_default();
        if base.diacraft == doc.diacraft && crate::carry::same(&now, &before) {
            return Ok(());
        }
        self.edited = true;
        match self.held_diacraft() {
            Held::Part { index, rel_id, part } => self.patch_diacraft(&index, &rel_id, &part, now),
            Held::None(index) => {
                if doc.diacraft.is_none() && now.items.is_empty() {
                    return Ok(());
                }
                self.add_diacraft(index, now)
            }
            Held::Unusable(why) => {
                if doc.diacraft.is_some() || !now.items.is_empty() {
                    self.not_saved(why);
                }
                Ok(())
            }
        }
    }

    /// Says in a warning that the model's DiaCraft data was not written, and why.
    fn not_saved(&mut self, why: &str) {
        self.warnings.push(format!("DiaCraft's own settings (the Snap and Glue options with no flag in the file) were not saved: {why}"));
    }

    /// Says in a warning that the data of the other format was not carried, and why.
    fn items_not_carried(&mut self, built: &Built, why: &str) {
        if !built.items.is_empty() {
            self.warnings.push(format!("data of the other format that this drawing carries was not saved: {why}"));
        }
    }

    /// The solutions index of the file, if it has one that is there.
    fn solutions_index(&self) -> Option<String> {
        let ty = rel_type(VIS, "solutions");
        self.document_rels.iter().find(|r| r.ty == ty && !r.external).map(|r| r.target.clone()).filter(|t| self.alive(t))
    }

    /// The part of the `Solution` named `DiaCraft`, through the index's relationships. Nothing here fails: a package the writer cannot read is `Unusable`.
    fn held_diacraft(&mut self) -> Held {
        let Some(index) = self.solutions_index() else { return Held::None(None) };
        let Some(bytes) = self.bytes_of(&index) else { return Held::None(None) };
        let Ok(tree) = Tree::parse(&index, bytes) else { return Held::Unusable("the file's solutions index is not XML") };
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Solutions") {
            return Held::Unusable("the file's solutions index has another root than Solutions");
        }
        let row = tree.root.elements().find(|e| sc.enter(e).is(e, "Solution") && e.attr("Name") == Some("DiaCraft"));
        let rel_id = row.and_then(|row| {
            let rsc = sc.enter(row);
            row.elements().find_map(|e| rsc.enter(e).rel_id(e)).map(str::to_owned)
        });
        // The relationships of the index must read for a row to be added to it or followed.
        let Ok(rels) = self.rels_mut(&index) else { return Held::Unusable("the relationships of the file's solutions index do not read") };
        let rows = rels.rows();
        match (row, rel_id) {
            (None, _) => Held::None(Some(index)),
            (Some(_), None) => Held::Unusable("the file's solution named DiaCraft has no relationship"),
            (Some(_), Some(rel_id)) => {
                let part = rows.iter().find(|r| r.id == rel_id && !r.external).and_then(|r| resolve(&index, &r.target));
                match part {
                    Some(part) if self.alive(&part) => Held::Part { index, rel_id, part },
                    _ => Held::Unusable("the file's solution named DiaCraft leads to no part"),
                }
            }
        }
    }

    /// The names a fresh part of DiaCraft's own may take: `/diacraft/blob<n>.<ext>` for the least `n` after `walk` that no part has and none has reserved.
    fn blob_name(&mut self, walk: &mut u32, ext: &str) -> R<String> {
        loop {
            *walk = walk.checked_add(1).ok_or(WriteError::Limit("no part name was left"))?;
            let name = format!("{}blob{}.{ext}", crate::carry::BLOB_DIR, walk);
            crate::work::name_probe();
            let key = fold(&name);
            if !self.present.contains_key(&key) && !self.reserved.contains(&key) {
                self.reserved.insert(key);
                return Ok(name);
            }
        }
    }

    /// Makes the parts of DiaCraft's own that the solution part `part` names, one for each byte string `built` carries, with the relationships from `part` to them: the `Id` of each is the one
    /// the document item names it by. The bytes are the model's, shared and not copied.
    fn add_blobs(&mut self, part: &str, built: &mut Built) -> R<Vec<String>> {
        if built.blobs.is_empty() {
            return Ok(Vec::new());
        }
        let ids = self.rels_mut(part)?.next_ids(built.blobs.len());
        if ids.len() < built.blobs.len() {
            return Err(WriteError::Limit("no relationship id was left for a part of the data of the other format"));
        }
        let mut walk = 0u32;
        for (blob, id) in built.blobs.iter().zip(&ids) {
            let ext = diacraft_sheetxml::model::blob_extension(&blob.media_type);
            let name = self.blob_name(&mut walk, ext)?;
            self.rels_mut(part)?.add(id, BLOB_REL_TYPE, &relative_target(part, &name));
            self.set_shared_part(&name, Arc::clone(&blob.bytes), Some(crate::carry::content_type_of(&blob.media_type)));
            self.carried_blobs.push((name, Arc::clone(&blob.bytes)));
        }
        built.name_blobs(&ids);
        Ok(ids)
    }

    /// Replaces the `Data` element of the part `part` (`None` takes it out, and a part left holding nothing goes with its row).
    fn patch_diacraft(&mut self, index: &str, rel_id: &str, part: &str, mut built: Built) -> R<()> {
        let limit = Limits::default().max_solution_part_bytes;
        let doc = self.doc;
        let Some(bytes) = self.bytes_of(part) else { return Err(WriteError::Mismatch("the DiaCraft part is not in the file")) };
        let wanted = doc.diacraft.is_some() || !built.items.is_empty();
        // The reader reads no part over this size, so the model holds nothing of one; the writer does not replace what the reader did not read.
        if bytes.len() as u64 > limit {
            if wanted {
                self.not_saved("the file's DiaCraft part is larger than DiaCraft reads");
            }
            return Ok(());
        }
        let Ok(mut tree) = Tree::parse(part, bytes) else {
            if wanted {
                self.not_saved("the file's DiaCraft part is not XML");
            }
            return Ok(());
        };
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "SolutionXML") || tree.root.attr("Name") != Some("DiaCraft") {
            if wanted {
                self.not_saved("the file's solution named DiaCraft is not a SolutionXML named DiaCraft");
            }
            return Ok(());
        }
        let at = tree.root.children.iter().position(|n| is_data(&sc, n));
        // The part is DiaCraft's when it holds nothing but the `Data` and the relationships to the parts of DiaCraft's own: then the items ride in it, and it is made again whole.
        let only_data = tree.root.elements().count() == 1 && at.is_some();
        // The relationships of the part, which a package may hold in a form that does not read (another root: the reader holds such a part as the bytes it is, and says so). **A part whose
        // relationships do not read is left as it is, in words, as a part that is not XML is** (the save was refused as `Mismatch` for a file that opened).
        let rows = match self.rels_mut(part) {
            Ok(tree_rels) => tree_rels.rows(),
            Err(_) => {
                if wanted {
                    self.not_saved("the relationships of the file's DiaCraft part do not read");
                }
                return Ok(());
            }
        };
        // **A relationship of the type of DiaCraft's parts is DiaCraft's only when it leads to a part under `/diacraft/` that no other relationship of the package names**: a row that leads to
        // a page, a master or the document (or to a part a page also names) is not, and the part it leads to is never taken out by this save.
        let targets: Vec<Option<String>> = rows.iter().map(|r| resolve(part, &r.target)).collect();
        let well_formed = rows
            .iter()
            .zip(&targets)
            .all(|(r, t)| crate::carry::is_blob_rel_type(&r.ty) && !r.external && t.as_deref().is_some_and(crate::carry::is_blob_target));
        // The relationships of every other part are read to know what else names a part of DiaCraft's own. **One that does not read may name any of them**: no part is taken out then, and the
        // part is patched as it is, with no items, and the save says why.
        let mut others_unreadable = false;
        let ours = only_data
            && well_formed
            && (rows.is_empty()
                || match self.targets_elsewhere(part) {
                    Ok(elsewhere) => !targets.iter().flatten().any(|t| elsewhere.contains(&fold(t))),
                    Err(_) => {
                        others_unreadable = true;
                        false
                    }
                });
        // A `Data` of the file that the model does not hold, with settings in it, was not kept by the reader (it has no `version`, or the document could not afford it): it is not ours
        // to replace or to take out. A `Data` of items alone is held as none: the items are the carrier's.
        if let Some(at) = at
            && self.base.diacraft.is_none()
        {
            let settings = match tree.root.children.get(at) {
                Some(Node::El(data)) => {
                    let dsc = sc.enter(data);
                    let known: std::collections::HashSet<usize> = crate::carry::known_items(data, &dsc).iter().map(|(i, _)| *i).collect();
                    data.attr_local("version") != Some(diacraft_doc::DIACRAFT_VERSION)
                        || data.children.iter().enumerate().any(|(i, n)| !known.contains(&i) && matches!(n, Node::El(_)))
                }
                _ => false,
            };
            if settings {
                if wanted {
                    self.not_saved("the file's DiaCraft data was not read (it states no version, or is too large to keep), and is left as it is");
                }
                return Ok(());
            }
        }
        if !ours {
            let why = if others_unreadable {
                "a relationships part of the file does not read, and it may name a part of DiaCraft's"
            } else {
                "the file's DiaCraft part holds more than DiaCraft's data"
            };
            self.items_not_carried(&built, why);
            built = Built::default();
        } else {
            // The parts of DiaCraft's own the file had go, and the model's are made: the part is whole again.
            let old: Vec<(Option<String>, Option<String>)> = rows.iter().map(|r| (Some(r.id.clone()), resolve(part, &r.target))).collect();
            if !old.is_empty() {
                self.remove_items(part, &old)?;
            }
            self.add_blobs(part, &mut built)?;
            if built.blobs.is_empty() && self.rels_mut(part)?.rows().is_empty() {
                // No relationship is left: the part has no relationships part.
                self.rels.remove(&fold(part));
                self.remove_part(&crate::write::rels_name(part));
            }
        }
        let (data, _) = crate::carry::data_element(doc.diacraft.as_deref(), &built)?;
        let wanted = data.is_some();
        match (at, data) {
            (Some(at), Some(data)) => {
                if let Some(slot) = tree.root.children.get_mut(at) {
                    *slot = Node::El(data);
                }
            }
            (Some(at), None) => {
                tree.root.children.remove(at);
            }
            (None, Some(data)) => tree.root.children.push(Node::El(data)),
            (None, None) => return Ok(()),
        }
        if !wanted && holds_nothing(&tree.root) {
            return self.remove_diacraft(index, rel_id, part);
        }
        let out = tree.bytes(part)?;
        if out.len() as u64 > limit {
            return Err(WriteError::Limit("the DiaCraft data would make a part larger than DiaCraft reads"));
        }
        self.diacraft_written = true;
        self.set_part(part, out, None);
        Ok(())
    }

    /// Takes the part out: its relationship, its content type, its row in the index, and the index with the document's relationship to it when no row is
    /// left (another writer's rows, if there are any, stay).
    fn remove_diacraft(&mut self, index: &str, rel_id: &str, part: &str) -> R<()> {
        // The part, its relationships part and its `Override` go through the writer's own removal, which edits the content types stream at those
        // rows and nowhere else.
        self.remove_items(index, &[(Some(rel_id.to_owned()), Some(part.to_owned()))])?;
        let bytes = self.bytes_of(index).ok_or(WriteError::Mismatch("the solutions index is not in the file"))?;
        let mut tree = Tree::parse(index, bytes)?;
        let sc = Scope::empty().enter(&tree.root);
        let ours =
            tree.root.children.iter().position(|n| matches!(n, Node::El(e) if sc.enter(e).is(e, "Solution") && e.attr("Name") == Some("DiaCraft")));
        if let Some(at) = ours {
            tree.root.children.remove(at);
        }
        // The index goes only when it holds **nothing** else: no other `Solution` row, no other element, comment or instruction of another writer, and no
        // relationship left (L-d: it went when no other `Solution` row was left, with whatever else it held).
        let relationships_left = !self.rels_mut(index)?.rows().is_empty();
        if relationships_left || !holds_nothing(&tree.root) {
            let out = tree.bytes(index)?;
            self.set_part(index, out, None);
            return Ok(());
        }
        // Nothing is left: the index goes, and the relationship the document had to it.
        let ty = rel_type(VIS, "solutions");
        let id = self.document_rels.iter().find(|r| r.ty == ty && !r.external).map(|r| r.id.clone());
        self.remove_part(index);
        if let Some(id) = id {
            let document = self.document_part.clone();
            self.rels_mut(&document)?.remove(&id);
        }
        Ok(())
    }

    /// Makes the part: the solutions index if the file has none, the row for DiaCraft in it, and the part with its relationship and its type, and the parts of DiaCraft's own it names.
    fn add_diacraft(&mut self, index: Option<String>, mut built: Built) -> R<()> {
        // The part first, so that one too large is refused before the index is touched. The relationships of a part made new are numbered from `rId1`, in the order of the blobs.
        let provisional: Vec<String> = (1..=built.blobs.len()).map(|n| format!("rId{n}")).collect();
        built.name_blobs(&provisional);
        let (data, _) = crate::carry::data_element(self.doc.diacraft.as_deref(), &built)?;
        let Some(data) = data else { return Ok(()) };
        let part_root = element("SolutionXML", vec![("Name".to_owned(), "DiaCraft".to_owned())], vec![Node::El(data)]);
        let part_bytes = Tree::new(part_root).bytes("the DiaCraft part")?;
        if part_bytes.len() as u64 > Limits::default().max_solution_part_bytes {
            return Err(WriteError::Limit("the DiaCraft data would make a part larger than DiaCraft reads"));
        }
        // The name first, so that a file whose numbers have run out gets no index with no row.
        let dir = match &index {
            Some(index) => folder_of(index).to_owned(),
            None => format!("{}solutions/", folder_of(&self.document_part)),
        };
        let Some(part) = self.fresh_names(&dir, "solution", ".xml", 1).into_iter().next() else {
            self.not_saved("the file has no free name for the part");
            return Ok(());
        };
        // The index (when the file has none), the part and their types are made by the writer's own calls (`create_index`, `set_part`), which add the rows to
        // the content types stream after the file's own rows and leave every other byte of it as it was.
        let index = match index {
            Some(index) => index,
            None => self.create_index("solutions", "solutions.xml", "Solutions", CT_SOLUTIONS)?,
        };
        self.reserved.insert(fold(&part));
        let rel_id = {
            let rels = self.rels_mut(&index)?;
            let id = rels.next_id();
            rels.add(&id, &rel_type(VIS, "solution"), &relative_target(&index, &part));
            id
        };
        // The parts of DiaCraft's own, with the relationships from the new part: the `Id`s are the ones the data names.
        let ids = self.add_blobs(&part, &mut built)?;
        if ids != provisional {
            return Err(WriteError::Mismatch("the relationships of the new DiaCraft part were not numbered as planned"));
        }
        // The row, in the index as it is now.
        let bytes = self.bytes_of(&index).ok_or(WriteError::Mismatch("the solutions index is not in the file"))?;
        let mut tree = Tree::parse(&index, bytes)?;
        let sc = Scope::empty().enter(&tree.root);
        if !sc.is(&tree.root, "Solutions") {
            return Err(WriteError::Mismatch("the solutions index has another root"));
        }
        // The attribute that names a relationship in this index: the one its own rows use, else `r:id` with the prefix declared.
        let mut existing = None;
        'rows: for row in tree.root.elements() {
            let rsc = sc.enter(row);
            for rel in row.elements() {
                let esc = rsc.enter(rel);
                if let Some((name, _)) = rel.attrs.iter().find(|(k, _)| esc.is_rel_id_attr(k)) {
                    existing = Some(name.clone());
                    break 'rows;
                }
            }
        }
        let attr = match existing {
            Some(name) => name,
            None => {
                if !tree.root.attrs.iter().any(|(k, _)| k == "xmlns:r") {
                    tree.root.attrs.push(("xmlns:r".to_owned(), NS_REL.to_owned()));
                }
                "r:id".to_owned()
            }
        };
        let prefix = tree.root.prefix().to_owned();
        let rel = element("Rel", vec![(attr, rel_id)], Vec::new());
        let mut row = element("Solution", vec![("Name".to_owned(), "DiaCraft".to_owned())], vec![Node::El(rel)]);
        super::build::with_prefix(&mut row, &prefix);
        tree.root.children.push(Node::El(row));
        let out = tree.bytes(&index)?;
        self.set_part(&index, out, None);
        // The part: `SolutionXML Name="DiaCraft"` in no namespace.
        self.diacraft_written = true;
        self.set_part(&part, part_bytes, Some(CT_SOLUTION));
        Ok(())
    }
}
