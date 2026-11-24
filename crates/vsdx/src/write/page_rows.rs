//! **The rows of a page part a save makes new**: a `ForeignData`'s `Rel` is an `r:id` of the relationships of the part its shape is in,
//! so a part that a save makes for a page that had none (a page added, a copy of a page) must have a row for every picture, embedded object and other part its shapes name, or the shapes lose
//! them. Both writers make those rows here: the save into a file (`mod.rs`, `pages_index_edit`) and the model writer (`held.rs`, `model.rs`).
//!
//! **Where a row leads** ([`Leads`]): a shape that names a relationship came from a page of the drawing that holds the same `ForeignData` (the very allocation, which a copy of a page shares,
//! else the same id, picture and attributes), and its row is the row of that page's part: **the same target, so two pages reach one part and nothing is copied**. A shape whose source is not
//! found, and a picture that names no relationship (another format's), get a row to a part that holds the picture: a part of the file that has its bytes, else one made from the model's bytes
//! (shared with the model, not copied). A shape with neither is left as it is.
//!
//! **The ids** are unique in the part: a shape keeps the id it names when no row of the part has it (so the file says what the model says, and the next save has nothing to write for it), takes
//! the row of an earlier shape that leads to the same place when its own is taken, and is given a new one otherwise; its `Rel` is then written with that id. The rows a page has beside these (to
//! its masters and its background page) are made after them, with ids of their own.
//!
//! **Costs** (the security scan): the shapes of every page are walked once to know where each `ForeignData` is, a part's rows are mapped by `Id` once, and a shape's row is
//! one look in that map, so a copy of `n` shapes naming `n` rows is `n` looks and not `n * n / 2`. The model writer's search for a shape whose page is gone looks at no more than
//! [`MAX_GONE_TRIES`] rows of one `Id`, in maps of the rows of the parts not written and of what each held part reaches, made once, and keeps its answer for the `Id` and the picture. Every row
//! put in a map and every look is counted with the writers' other rows (`work::rels` for the save into a file, `work::model_row` for the model writer).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, ForeignData, MAX_SHAPE_DEPTH, Media, Page, Passthrough, Placed, RawNode, Shape, ShapeList};
use diacraft_opc::name::{fold, resolve};

mod kept;

use super::build::rest_of;
use super::held::{Book, IMAGE, plausible};
use super::model::{picture_content_type, picture_type, with_compression};
use super::parts::{RelRow, folder_of, relative_target};
use super::{NameWalk, Plan, R, WriteError};

/// Where a row leads: its type, its target (for a row inside the package, the name of the part it reaches; for one outside it, the target as written), and whether it leads outside.
#[derive(Clone, Debug)]
pub(super) struct Lead {
    pub ty: String,
    pub target: String,
    pub external: bool,
}

impl Lead {
    /// The row to a picture's part.
    fn picture(part: &str) -> Lead {
        Lead { ty: IMAGE.to_owned(), target: part.to_owned(), external: false }
    }

    /// The row to the part of an embedded object: a relationship of the type `oleObject`.
    pub(super) fn object(part: &str) -> Lead {
        Lead { ty: super::objects::ole_rel(), target: part.to_owned(), external: false }
    }

    /// What tells two leads apart: a part's name compared as a package compares names.
    fn key(&self) -> (String, String, bool) {
        let target = if self.external { self.target.clone() } else { fold(&self.target) };
        (self.ty.clone(), target, self.external)
    }

    /// The row, from the part `source`.
    pub(super) fn row(&self, id: &str, source: &str) -> RelRow {
        let target = if self.external { self.target.clone() } else { relative_target(source, &self.target) };
        RelRow { id: id.to_owned(), ty: self.ty.clone(), target, external: self.external }
    }
}

/// Where an object frame an `.odg` made leads once it is written as an embedding (`objects.rs`): the row to its object part, and the attributes its `ForeignData` is written with.
pub(super) struct ObjectLead {
    pub lead: Lead,
    pub attrs: diacraft_doc::RawAttrs,
}

/// What a writer knows of where the relationships of the shapes of a new part lead.
pub(super) trait Leads {
    /// The row a shape's `Rel` names in the part of the page the shape came from, when that page and the row are found and the row leads to a part the package holds.
    fn named(&mut self, foreign: &Arc<ForeignData>) -> R<Option<Lead>>;
    /// A row to a part that holds the picture: one the package has, or one made for it.
    fn picture(&mut self, media: &Arc<Media>) -> R<Option<Lead>>;
    /// The row to the object part of a shape that is an object frame carried back, which the writer has written as an embedding, and the attributes of its `ForeignData`: **the fallback picture hangs
    /// off the object part, so the page has a row to the object and none to the picture**.
    fn object(&mut self, shape: &Arc<Shape>) -> R<Option<ObjectLead>>;
    /// The row a relationship id of a shape's kept markup names in the part of the page the shape came from (`kept.rs`): the kept nodes `list` the shape holds, the place `at`
    /// of the node with the id among them, and the id. As [`Leads::named`], for a `ForeignData` the reader did not read.
    fn named_kept(&mut self, list: &Arc<Vec<Placed>>, at: usize, id: &str) -> R<Option<Lead>>;
}

/// The rows a new part is given for its shapes, in order, with their ids, and the shapes written in its place when a `Rel` was given or changed.
#[derive(Default)]
pub(super) struct Carried {
    pub rows: Vec<(String, Lead)>,
    pub shapes: Option<ShapeList>,
}

impl Carried {
    /// The parts inside the package the rows lead to, folded.
    pub(super) fn reached(&self) -> impl Iterator<Item = String> + '_ {
        self.rows.iter().filter(|(_, lead)| !lead.external).map(|(_, lead)| fold(&lead.target))
    }
}

/// The ids given in one part.
#[derive(Default)]
struct Ids {
    /// Where the row of each id given leads, by the id in lower case.
    by_id: HashMap<String, (String, String, bool)>,
    /// The id of the first row given to each place.
    by_lead: HashMap<(String, String, bool), String>,
    /// Where each `ForeignData` met in the part leads, by its allocation.
    leads: HashMap<usize, Option<Lead>>,
    /// Where each relationship id of kept markup met in the part leads, by the allocation of the kept nodes, the place of the node and the id: the shapes that share their kept nodes ask once.
    kept_leads: HashMap<(usize, usize, Arc<str>), Option<Lead>>,
    /// The prefixes declared around the shapes being walked (`kept.rs`): the root of the part, its `Shapes`, and each shape and group on the way down, a chain of frames.
    bindings: std::rc::Rc<kept::Frame>,
    n: u32,
}

impl Ids {
    /// The least `rId` no row of the part has: the walk goes up the numbers once, however many are asked for.
    fn fresh(&mut self) -> Option<String> {
        loop {
            self.n = self.n.checked_add(1)?;
            let id = format!("rId{}", self.n);
            if !self.by_id.contains_key(&id.to_ascii_lowercase()) {
                return Some(id);
            }
        }
    }
}

/// The rows of the new part of `page`, and the shapes with `Rel`s that name them. The prefixes the part's root and its `Shapes` declare, as the page keeps them, are in force around the shapes.
pub(super) fn carry(page: &Page, leads: &mut impl Leads) -> R<Carried> {
    let mut ids = Ids::default();
    for element in ["PageContents", "Shapes"] {
        if let Some(kept) = rest_of(&page.ext, element) {
            ids.bindings = kept::Frame::enter(&ids.bindings, &kept.raw);
        }
    }
    let mut rows = Vec::new();
    let shapes = walk(&page.shapes, leads, &mut ids, &mut rows, 0)?;
    Ok(Carried { rows, shapes })
}

fn walk(list: &ShapeList, leads: &mut impl Leads, ids: &mut Ids, rows: &mut Vec<(String, Lead)>, depth: usize) -> R<Option<ShapeList>> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(WriteError::Limit("levels of nested shapes"));
    }
    let mut changed: Vec<(usize, Arc<Shape>)> = Vec::new();
    for (at, shape) in list.iter().enumerate() {
        let mut out: Option<Shape> = None;
        if let Some(foreign) = &shape.foreign
            && let Some(object) = leads.object(shape)?
        {
            // An object frame: the row leads to the object part, shared by the frames of this part that lead to the same one; the shape says `Object` with its attributes.
            let id = row_for(ids, rows, None, object.lead)?;
            let mut given = ForeignData::clone(foreign);
            given.attrs = object.attrs;
            given.rel_id = Some(Arc::from(id));
            let mut copy = Shape::clone(shape);
            copy.foreign = Some(Arc::new(given));
            out = Some(copy);
        } else if let Some(foreign) = &shape.foreign
            && let Some(id) = row_of(foreign, leads, ids, rows)?
            && foreign.rel_id.as_deref() != Some(id.as_str())
        {
            let mut given = ForeignData::clone(foreign);
            // A picture that named no relationship is written as a picture of the format: its `CompressionType` says what it is, as the model writer gives it.
            if given.rel_id.is_none()
                && let Some(media) = &given.picture
            {
                given.attrs = with_compression(&given.attrs, picture_type(media).1);
            }
            given.rel_id = Some(Arc::from(id));
            let mut copy = Shape::clone(shape);
            copy.foreign = Some(Arc::new(given));
            out = Some(copy);
        }
        // Kept markup that names a relationship, such as a `ForeignData` on a shape whose `Type` is not `Foreign`: its rows too (`kept.rs`).
        let outer = std::rc::Rc::clone(&ids.bindings);
        ids.bindings = kept::Frame::enter(&outer, &shape.raw);
        if let Some(passthrough) = kept_rows(shape, leads, ids, rows)? {
            out.get_or_insert_with(|| Shape::clone(shape)).passthrough = passthrough;
        }
        if let Some(members) = rest_of(&shape.ext, "Shapes") {
            ids.bindings = kept::Frame::enter(&ids.bindings, &members.raw);
        }
        if !shape.children.is_empty()
            && let Some(kids) = walk(&shape.children, leads, ids, rows, depth.saturating_add(1))?
        {
            out.get_or_insert_with(|| Shape::clone(shape)).children = kids;
        }
        ids.bindings = outer;
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

/// The id of the row the shape's `Rel` names in the new part, made when needed. `None` for a shape whose relationship leads nowhere that is found and that shows no picture.
fn row_of(foreign: &Arc<ForeignData>, leads: &mut impl Leads, ids: &mut Ids, rows: &mut Vec<(String, Lead)>) -> R<Option<String>> {
    // The shapes that share one `ForeignData` (a picture copied many times over) ask where it leads once.
    let ptr = Arc::as_ptr(foreign) as usize;
    let lead = match ids.leads.get(&ptr) {
        Some(found) => found.clone(),
        None => {
            let found = lead_of(foreign, leads)?;
            ids.leads.insert(ptr, found.clone());
            found
        }
    };
    let Some(lead) = lead else { return Ok(None) };
    row_for(ids, rows, foreign.rel_id.as_deref(), lead).map(Some)
}

/// The id of the row of a part that leads to `lead`: the id the shape names (`own`) when no row of the part has it or the row it names leads to the same place, the row of an earlier shape that leads to
/// the same place, or a new one.
fn row_for(ids: &mut Ids, rows: &mut Vec<(String, Lead)>, own: Option<&str>, lead: Lead) -> R<String> {
    let key = lead.key();
    if let Some(own) = own {
        match ids.by_id.get(&own.to_ascii_lowercase()) {
            None => {
                give(ids, rows, own.to_owned(), key, lead);
                return Ok(own.to_owned());
            }
            Some(there) if *there == key => return Ok(own.to_owned()),
            Some(_) => {}
        }
    }
    if let Some(id) = ids.by_lead.get(&key) {
        return Ok(id.clone());
    }
    let id = ids.fresh().ok_or(WriteError::Limit("no relationship id was left for a picture"))?;
    give(ids, rows, id.clone(), key, lead);
    Ok(id)
}

/// The rows the relationship ids of a shape's kept markup name in the new part, each id kept where it may be (as [`row_for`] keeps a `Rel`'s), and the kept nodes written with the ids the part
/// gives where they differ (`kept.rs`). `None` when the kept nodes are written as they are. An id whose row is not found gets no row: the markup names what it named.
fn kept_rows(shape: &Shape, leads: &mut impl Leads, ids: &mut Ids, rows: &mut Vec<(String, Lead)>) -> R<Option<Passthrough>> {
    let Some(list) = shape.passthrough.shared_list() else { return Ok(None) };
    let named = kept::ids_of(&shape.passthrough, &ids.bindings);
    if named.is_empty() {
        return Ok(None);
    }
    let ptr = Arc::as_ptr(list) as usize;
    let mut renames: kept::Renames = HashMap::new();
    for kept::KeptId { at, id } in named {
        let asked = (ptr, at, Arc::clone(&id));
        let lead = match ids.kept_leads.get(&asked) {
            Some(found) => found.clone(),
            None => {
                let found = leads.named_kept(list, at, &id)?;
                ids.kept_leads.insert(asked, found.clone());
                found
            }
        };
        let Some(lead) = lead else { continue };
        let given = row_for(ids, rows, Some(&id), lead)?;
        if *given != *id {
            renames.entry(at).or_default().insert(id, Arc::from(given));
        }
    }
    Ok((!renames.is_empty()).then(|| kept::renamed(&shape.passthrough, &renames, &ids.bindings)))
}

/// Where the shape's `Rel` leads: the row of the page it came from, else a row to its picture.
fn lead_of(foreign: &Arc<ForeignData>, leads: &mut impl Leads) -> R<Option<Lead>> {
    let named = match foreign.rel_id {
        Some(_) => leads.named(foreign)?,
        None => None,
    };
    Ok(match (named, &foreign.picture) {
        (Some(lead), _) => Some(lead),
        (None, Some(media)) => leads.picture(media)?,
        (None, None) => None,
    })
}

fn give(ids: &mut Ids, rows: &mut Vec<(String, Lead)>, id: String, key: (String, String, bool), lead: Lead) {
    ids.by_id.insert(id.to_ascii_lowercase(), key.clone());
    ids.by_lead.entry(key).or_insert_with(|| id.clone());
    rows.push((id, lead));
}

// ------------------------------------------------------------------------------------------------------------- the pages shapes came from

/// What tells the `ForeignData` of one shape from another's when they are not one allocation: the id it names, the name of its picture's part and its attributes.
type Print = (String, String, Vec<(String, String)>);

fn print(foreign: &ForeignData) -> Option<Print> {
    let id = foreign.rel_id.as_deref()?;
    let picture = foreign.picture.as_ref().map_or_else(String::new, |m| fold(&m.part));
    Some((id.to_owned(), picture, foreign.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()))
}

/// What a kept element is known by when it is not the very allocation: its name and the list of its attributes, as the model holds them (shared, not copied).
type KeptKey = (Arc<str>, Option<Arc<Vec<(Arc<str>, Arc<str>)>>>);

fn kept_key(el: &diacraft_doc::RawElement) -> KeptKey {
    (Arc::clone(&el.name), el.attrs.shared_list().cloned())
}

/// The pages a shape may have come from: for each `ForeignData` that names a relationship on a page that has a part, that part, by allocation and by [`Print`] (the first page wins); and for
/// the kept markup of a shape that may name one (`kept.rs`), the same by the allocation of the kept nodes and by the element and the id. **The kept maps share the model's text** (the security
/// scan): an element is a key once, by its name and the list of its attributes as the model holds them, and each id it may name is an entry of its own map, so nothing of the
/// markup is copied for each id.
#[derive(Default)]
pub(super) struct Sources {
    by_ptr: HashMap<usize, String>,
    by_print: HashMap<Print, String>,
    kept_by_ptr: HashMap<usize, Arc<str>>,
    /// Each kept element that may name a relationship, by its name and its attributes as the model holds them: its place in `kept_by_id`.
    kept_elements: HashMap<KeptKey, usize>,
    /// For each such element, the part of its page by each id it may name a relationship by (the first page wins).
    kept_by_id: Vec<HashMap<Arc<str>, Arc<str>>>,
    /// The picture a `ForeignData` that is read shows, by the folded part of its page and the `Id` it names (the first wins): what a row of that `Id` leads to where the model writer holds none.
    pictures_by_row: HashMap<(String, String), Arc<Media>>,
}

impl Sources {
    /// The shapes of `pages`, each with the part its page is in: one walk of every shape, with a stack of its own.
    fn add<'p>(&mut self, pages: impl Iterator<Item = (&'p Page, &'p str)>) {
        for (page, part) in pages {
            let shared: Arc<str> = Arc::from(part);
            let mut stack: Vec<&ShapeList> = vec![&page.shapes];
            while let Some(list) = stack.pop() {
                for shape in list.iter() {
                    if let Some(foreign) = &shape.foreign
                        && let Some(p) = print(foreign)
                    {
                        self.by_ptr.entry(Arc::as_ptr(foreign) as usize).or_insert_with(|| part.to_owned());
                        self.by_print.entry(p).or_insert_with(|| part.to_owned());
                        if let (Some(id), Some(media)) = (foreign.rel_id.as_deref(), &foreign.picture) {
                            self.pictures_by_row.entry((fold(part), id.to_owned())).or_insert_with(|| Arc::clone(media));
                        }
                    }
                    if let Some(list) = shape.passthrough.shared_list() {
                        self.add_kept(list, &shared);
                    }
                    if !shape.children.is_empty() {
                        stack.push(&shape.children);
                    }
                }
            }
        }
    }

    /// The part of the page `foreign` came from.
    fn of(&self, foreign: &Arc<ForeignData>) -> Option<&str> {
        self.by_ptr.get(&(Arc::as_ptr(foreign) as usize)).or_else(|| print(foreign).and_then(|p| self.by_print.get(&p))).map(String::as_str)
    }

    /// The kept nodes of a shape of a page whose part is `part`: by their allocation, and each element that may name a relationship by its key, with the ids it may name it by.
    fn add_kept(&mut self, list: &Arc<Vec<Placed>>, part: &Arc<str>) {
        let mut named = false;
        for placed in list.iter() {
            let RawNode::Element(el) = &placed.node else { continue };
            let ids = kept::prefixed_ids(el);
            if ids.is_empty() {
                continue;
            }
            named = true;
            // The key is hashed once for the element, whatever the ids it names.
            crate::work::kept_step(1 + el.attrs.len());
            let next = self.kept_by_id.len();
            let at = *self.kept_elements.entry(kept_key(el)).or_insert(next);
            if at == next {
                self.kept_by_id.push(HashMap::new());
            }
            if let Some(by_id) = self.kept_by_id.get_mut(at) {
                for id in ids {
                    crate::work::kept_step(1);
                    by_id.entry(id).or_insert_with(|| Arc::clone(part));
                }
            }
        }
        if named {
            self.kept_by_ptr.entry(Arc::as_ptr(list) as usize).or_insert_with(|| Arc::clone(part));
        }
    }

    /// The picture a `ForeignData` that is read on a page whose part is `part` shows through the row of `Id` `id`.
    fn picture_of_row(&self, part: &str, id: &str) -> Option<&Arc<Media>> {
        self.pictures_by_row.get(&(fold(part), id.to_owned()))
    }

    /// The part of the page the kept nodes `list` came from, by their allocation, else by the node at `at` and the id `id`.
    fn of_kept(&self, list: &Arc<Vec<Placed>>, at: usize, id: &str) -> Option<&str> {
        if let Some(part) = self.kept_by_ptr.get(&(Arc::as_ptr(list) as usize)) {
            return Some(part);
        }
        let RawNode::Element(el) = &list.get(at)?.node else { return None };
        crate::work::kept_step(1 + el.attrs.len());
        let n = self.kept_elements.get(&kept_key(el))?;
        self.kept_by_id.get(*n)?.get(id).map(|part| &**part)
    }
}

// --------------------------------------------------------------------------------------------------------------- the save into a file

/// What the save into a file keeps between the pages it makes: the pages shapes came from, the relationships of their parts as the file has them, and the parts it made for pictures.
#[derive(Default)]
pub(super) struct SpliceRows {
    sources: Option<Sources>,
    /// The rows of each part a shape came from, as the file has them, by the folded part and then by `Id` (the first row of an `Id`, which is the one the reader takes): each part's
    /// relationships read and put in the map once, so that a shape's row is one look and not a walk of the part's rows (the security scan, item 2).
    rels: HashMap<String, HashMap<String, diacraft_opc::Rel>>,
    /// The part each picture is written in, by the picture's allocation.
    pictures: HashMap<usize, String>,
    /// The walks up the numbers of the picture parts made, by extension.
    walks: HashMap<&'static str, NameWalk>,
    /// The object frames carried back and the embeddings made for them (`objects.rs`).
    pub(super) objects: super::objects::SpliceObjects,
}

impl Plan<'_> {
    /// The rows of the pages of `now` that get a part of their own in this save (those with none yet), by `ID`, the first page of an `ID` winning: planned **before any page is taken out**,
    /// and every part they reach is kept by the removals that follow (`keep`), so that a copy whose source is taken out in the same save keeps what it shows.
    pub(super) fn carry_new_pages(&mut self, now: &[&Page]) -> R<HashMap<u32, Carried>> {
        let mut out: HashMap<u32, Carried> = HashMap::new();
        for page in now {
            if self.page_part.contains_key(&page.id) || out.contains_key(&page.id) || !holds_foreign(&page.shapes) {
                continue;
            }
            let carried = carry(page, self)?;
            let reached: Vec<String> = carried.reached().collect();
            self.keep.extend(reached);
            out.insert(page.id, carried);
        }
        Ok(out)
    }

    /// The row of `Id` `id` of the part `part` in the file. The part's rows are read and mapped by `Id` the first time it is asked about, each row looked at once (`work::rels`), and every
    /// look after is one more.
    fn file_row(&mut self, part: &str, id: &str) -> Option<diacraft_opc::Rel> {
        let key = fold(part);
        if !self.new_rows.rels.contains_key(&key) {
            let mut by_id: HashMap<String, diacraft_opc::Rel> = HashMap::new();
            // Relationships that do not read lead nowhere: the shapes are given their pictures.
            if let Ok(rels) = self.pkg.rels(part) {
                crate::work::rels(rels.items.len());
                for row in rels.items {
                    by_id.entry(row.id.clone()).or_insert(row);
                }
            }
            self.new_rows.rels.insert(key.clone(), by_id);
        }
        crate::work::rels(1);
        self.new_rows.rels.get(&key).and_then(|rows| rows.get(id)).cloned()
    }

    fn sources(&mut self) -> &Sources {
        if self.new_rows.sources.is_none() {
            let mut sources = Sources::default();
            // The pages of the drawing that have a part in the file (their shapes are the ones a copy shares), then the pages the file was read as (a page taken out since).
            let (doc, base) = (self.doc, self.base);
            sources.add(doc.pages.iter().filter_map(|p| self.page_part.get(&p.id).map(|part| (&**p, part.as_str()))));
            sources.add(base.pages.iter().zip(&self.base_page_parts).filter_map(|(p, part)| part.as_deref().map(|part| (&**p, part))));
            self.new_rows.sources = Some(sources);
        }
        self.new_rows.sources.get_or_insert_with(Sources::default)
    }

    /// The `Rel`s of a new page part, written: each row from the part.
    pub(super) fn write_carried(&mut self, part: &str, carried: &Carried) -> R<()> {
        if carried.rows.is_empty() {
            return Ok(());
        }
        let tree = self.rels_mut(part)?;
        for (id, lead) in &carried.rows {
            tree.add_row(&lead.row(id, part));
        }
        Ok(())
    }
}

impl Plan<'_> {
    /// Where the row of `Id` `id` of the part `source` in the file leads.
    fn file_lead(&mut self, source: &str, id: &str) -> Option<Lead> {
        let row = self.file_row(source, id)?;
        if row.mode == diacraft_opc::TargetMode::External {
            return Some(Lead { ty: row.ty, target: row.target_raw, external: true });
        }
        // A row that leads to a part the file does not have leads nowhere: the shape is given its picture instead.
        row.target.filter(|t| self.alive(t)).map(|target| Lead { ty: row.ty, target, external: false })
    }
}

impl Leads for Plan<'_> {
    fn named(&mut self, foreign: &Arc<ForeignData>) -> R<Option<Lead>> {
        let Some(id) = foreign.rel_id.as_deref() else { return Ok(None) };
        let Some(source) = self.sources().of(foreign).map(str::to_owned) else { return Ok(None) };
        Ok(self.file_lead(&source, id))
    }

    fn named_kept(&mut self, list: &Arc<Vec<Placed>>, at: usize, id: &str) -> R<Option<Lead>> {
        let Some(source) = self.sources().of_kept(list, at, id).map(str::to_owned) else { return Ok(None) };
        Ok(self.file_lead(&source, id))
    }

    fn picture(&mut self, media: &Arc<Media>) -> R<Option<Lead>> {
        let ptr = Arc::as_ptr(media) as usize;
        if let Some(name) = self.new_rows.pictures.get(&ptr) {
            return Ok(Some(Lead::picture(name)));
        }
        // A part of the file that holds the picture under the picture's own name is that picture: its row leads there, and nothing is copied.
        let name = if plausible(&media.part) && self.alive(&media.part) && self.bytes_of(&media.part) == Some(&media.bytes[..]) {
            media.part.to_string()
        } else {
            let (extension, _) = picture_type(media);
            let dir = format!("{}media/", folder_of(&self.document_part));
            let mut walk = self.new_rows.walks.remove(extension).unwrap_or_else(|| NameWalk::new(&dir, "image", extension));
            let name = self.next_name(&mut walk);
            self.new_rows.walks.insert(extension, walk);
            let Some(name) = name else { return Err(WriteError::Limit("no part name was left for a picture")) };
            let ct = picture_content_type(media);
            // The bytes are the model's, shared with it, and the check of the file shares them too (`finish`).
            self.reserved.insert(fold(&name));
            self.set_shared_part(&name, Arc::clone(&media.bytes), Some(&ct));
            self.carried_blobs.push((name.clone(), Arc::clone(&media.bytes)));
            name
        };
        self.new_rows.pictures.insert(ptr, name.clone());
        Ok(Some(Lead::picture(&name)))
    }

    fn object(&mut self, shape: &Arc<Shape>) -> R<Option<ObjectLead>> {
        self.object_lead(shape)
    }
}

/// **A picture new to a page part the file has** (refused it, "a picture new to a page the file already has"): a picture inserted from a file into a drawing saved as a VSDX
/// type, which the next Save refused. It is given what a new page's picture is given: **a row of the page part's relationships to a part that holds the picture** (one of the file's
/// that holds its bytes, else one made from the model's bytes, shared: [`Leads::picture`]), and its `ForeignData` names the row and says the `CompressionType` of its type, as the model writer
/// writes it. The shapes a picture is given to are the ones `patch_shapes` makes new: a shape of a list whose key (`ID`, type and master) no shape of the file's list at that place has, and the
/// members of one. A shape that holds data of another format (an `.odg`'s frame, an object) is left as it is, and refused there as before; so is a picture of a master. Two shapes of the page that
/// show one picture name one row.
impl Plan<'_> {
    /// `now`, the shapes the page part `part` is patched to from `base`, with each picture new to the part given a row: `None` when there is none (and no walk but one look at each shape).
    /// **The rows' ids are taken in one walk of the part's ids** (`RelsTree::next_ids`, the rule): the new pictures are found first, each once, and then given their ids and parts, so a page
    /// of many new pictures costs the rows there and the rows made, and not a walk of the part's ids for each.
    pub(super) fn dress_new_pictures(&mut self, part: &str, base: &ShapeList, now: &ShapeList) -> R<Option<ShapeList>> {
        if !holds_new_picture(now, 0) {
            return Ok(None);
        }
        let mut found: Vec<Arc<Media>> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        new_pictures(Some(base), now, &mut found, &mut seen, 0)?;
        if found.is_empty() {
            return Ok(None);
        }
        let ids = self.rels_mut(part)?.next_ids(found.len());
        if ids.len() < found.len() || ids.iter().any(String::is_empty) {
            return Err(WriteError::Limit("no relationship id was left for a picture new to a page"));
        }
        let mut rows: HashMap<usize, String> = HashMap::with_capacity(found.len());
        for (media, id) in found.iter().zip(ids) {
            let Some(lead) = Leads::picture(self, media)? else { return Err(WriteError::Mismatch("a picture new to a page has no part")) };
            self.rels_mut(part)?.add(&id, IMAGE, &relative_target(part, &lead.target));
            rows.insert(Arc::as_ptr(media).addr(), id);
        }
        dressed(Some(base), now, &rows, 0)
    }
}

/// The key a shape of a list is matched by to the file's (`patch_shapes`): its `ID`, its type and its master.
type ShapeKey = (u32, Option<String>, Option<u32>);

fn shape_key(s: &Shape) -> ShapeKey {
    (s.id, s.kind.text().map(str::to_owned), s.master)
}

/// The shapes of `base` by their key; none for a list that has no counterpart (the members of a shape new to the list).
fn keyed(base: Option<&ShapeList>) -> HashMap<ShapeKey, &Arc<Shape>> {
    base.map(|list| list.iter().map(|s| (shape_key(s), s)).collect()).unwrap_or_default()
}

/// The pictures of the shapes of `now` new to the list `base` (and of their members), each once by its allocation, in the order met.
fn new_pictures(base: Option<&ShapeList>, now: &ShapeList, found: &mut Vec<Arc<Media>>, seen: &mut HashSet<usize>, depth: usize) -> R<()> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(WriteError::Limit("a group is nested deeper than a drawing may hold"));
    }
    let held = keyed(base);
    for shape in now.iter() {
        let matched = held.get(&shape_key(shape)).copied();
        if matched.is_none()
            && let Some(foreign) = &shape.foreign
            && let Some(media) = &foreign.picture
            && is_new_picture(shape, foreign)
            && seen.insert(Arc::as_ptr(media).addr())
        {
            found.push(Arc::clone(media));
        }
        if !shape.children.is_empty() {
            new_pictures(matched.map(|m| &m.children), &shape.children, found, seen, depth.saturating_add(1))?;
        }
    }
    Ok(())
}

/// `now` with each new picture naming its row (`rows`, by the allocation of the picture), and the `CompressionType` of its type; `None` when no shape changed.
fn dressed(base: Option<&ShapeList>, now: &ShapeList, rows: &HashMap<usize, String>, depth: usize) -> R<Option<ShapeList>> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(WriteError::Limit("a group is nested deeper than a drawing may hold"));
    }
    let held = keyed(base);
    let mut out: Vec<Arc<Shape>> = Vec::with_capacity(now.len());
    let mut changed = false;
    for shape in now.iter() {
        let matched = held.get(&shape_key(shape)).copied();
        let mut own: Option<Shape> = None;
        if matched.is_none()
            && let Some(foreign) = &shape.foreign
            && let Some(media) = &foreign.picture
            && is_new_picture(shape, foreign)
            && let Some(id) = rows.get(&Arc::as_ptr(media).addr())
        {
            let mut data = ForeignData::clone(foreign);
            data.attrs = with_compression(&data.attrs, &picture_content_type(media));
            data.rel_id = Some(Arc::from(id.as_str()));
            let mut copy = Shape::clone(shape);
            copy.foreign = Some(Arc::new(data));
            own = Some(copy);
        }
        if !shape.children.is_empty()
            && let Some(members) = dressed(matched.map(|m| &m.children), &shape.children, rows, depth.saturating_add(1))?
        {
            own.get_or_insert_with(|| Shape::clone(shape)).children = members;
        }
        match own {
            Some(own) => {
                changed = true;
                out.push(Arc::new(own));
            }
            None => out.push(Arc::clone(shape)),
        }
    }
    Ok(changed.then(|| ShapeList::from_vec(out)))
}

/// Whether a shape shows a picture that no relationship names and that is the drawing's own: no data of another format (an `.odg`'s frame keeps its markup there) and no embedded object.
fn is_new_picture(shape: &Shape, foreign: &ForeignData) -> bool {
    foreign.rel_id.is_none()
        && foreign.picture.is_some()
        && shape.ext.get().is_none()
        && foreign.ext.get().is_none()
        && !matches!(foreign.attr("ForeignType"), Some("Object" | "Ink"))
}

/// Whether the list, or a member of one, holds a shape with a new picture ([`is_new_picture`]): one look at each shape, held to the model's depth.
fn holds_new_picture(list: &ShapeList, depth: usize) -> bool {
    depth < MAX_SHAPE_DEPTH
        && list.iter().any(|shape| {
            shape.foreign.as_ref().is_some_and(|f| is_new_picture(shape, f)) || holds_new_picture(&shape.children, depth.saturating_add(1))
        })
}

/// Whether a shape of the list, or a member of one, has a `ForeignData`, or kept markup, which may name a relationship (a `ForeignData` the reader did not read is kept markup).
fn holds_foreign(list: &ShapeList) -> bool {
    let mut stack: Vec<&ShapeList> = vec![list];
    while let Some(shapes) = stack.pop() {
        for shape in shapes.iter() {
            if shape.foreign.is_some() || !shape.passthrough.is_empty() {
                return true;
            }
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
    false
}

// ------------------------------------------------------------------------------------------------------------------------ the model writer

/// The most rows of one `Id` from parts that are not written that the search for a shape whose page is gone looks at (`ModelLeads::gone_row`). Past it the shape is given its picture: a
/// drawing whose taken-out pages hold more rows of one `Id` than this is a hostile one, and the search must not cost the shapes times those rows.
const MAX_GONE_TRIES: usize = 64;

/// The model writer's view: the rows the model holds for the parts pages had, and the parts the plan writes. **Every lookup is in a map made once** (the security scan):
/// the held rows of a part by `Id`, the rows of parts that are not written by `Id`, the targets each held part's own rows reach; and the answer for a shape whose page is gone is kept for its
/// `Id` and picture. Each row put in a map and each one looked at is counted in `work::model_rows_walked`, with the writer's other rows.
pub(super) struct ModelLeads<'b, 'a> {
    book: &'b Book<'a>,
    sources: Sources,
    written: HashSet<String>,
    /// The held rows of each part a shape came from, by the folded part and then by `Id` (the first of an `Id`), each part's rows mapped once.
    held_by_id: HashMap<String, HashMap<&'a str, &'a diacraft_doc::Rel>>,
    /// The rows held from parts that are not written (a page taken out), by `Id`, made when a shape's page is first not found: where a copy whose source is gone finds its row.
    gone_rows: Option<HashMap<&'a str, Vec<&'a diacraft_doc::Rel>>>,
    /// The parts each held part's own rows reach, folded, by the folded name of the held part: made once, with `gone_rows`.
    reaches: Option<HashMap<String, HashSet<String>>>,
    /// The answers of `gone_row`, by `Id` and the folded part of the picture.
    gone_found: HashMap<(String, String), Option<Lead>>,
}

impl<'a> ModelLeads<'_, 'a> {
    /// The held row of `Id` `id` of the part `source`.
    fn held_row(&mut self, source: &str, id: &str) -> Option<&'a diacraft_doc::Rel> {
        let key = fold(source);
        let book = self.book;
        let by_id = self.held_by_id.entry(key.clone()).or_insert_with(|| {
            let mut map: HashMap<&'a str, &'a diacraft_doc::Rel> = HashMap::new();
            for row in book.held.get(&key).into_iter().flatten() {
                crate::work::model_row();
                let row: &'a diacraft_doc::Rel = row;
                map.entry(&*row.id).or_insert(row);
            }
            map
        });
        crate::work::model_row();
        by_id.get(id).copied()
    }

    /// The row of `Id` `id` from a part that is not written that leads to the picture `shown`, or to a part whose own row leads to it (an object and its picture): the row of the page a
    /// copy was made from, when that page is gone and nothing else names the shape. Answered once for each `Id` and picture, from at most [`MAX_GONE_TRIES`] rows.
    fn gone_row(&mut self, id: &str, shown: &Media) -> Option<Lead> {
        let picture = fold(&shown.part);
        let asked = (id.to_owned(), picture);
        if let Some(found) = self.gone_found.get(&asked) {
            return found.clone();
        }
        let (book, written) = (self.book, &self.written);
        let gone = self.gone_rows.get_or_insert_with(|| {
            let mut by_id: HashMap<&'a str, Vec<&'a diacraft_doc::Rel>> = HashMap::new();
            for (source, rows) in &book.held {
                if !written.contains(source) {
                    for row in rows {
                        crate::work::model_row();
                        let row: &'a diacraft_doc::Rel = row;
                        by_id.entry(&*row.id).or_default().push(row);
                    }
                }
            }
            by_id
        });
        let reaches = self.reaches.get_or_insert_with(|| {
            let mut map: HashMap<String, HashSet<String>> = HashMap::new();
            for part in book.doc.parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx) {
                let set = map.entry(fold(&part.name)).or_default();
                for row in part.rels.iter().filter(|r| !r.external) {
                    crate::work::model_row();
                    if let Some(target) = resolve(&part.name, &row.target) {
                        set.insert(fold(&target));
                    }
                }
            }
            map
        });
        let mut found = None;
        for row in gone.get(id).into_iter().flatten().filter(|r| !r.external).take(MAX_GONE_TRIES) {
            crate::work::model_row();
            let Some(target) = resolve(&row.source, &row.target) else { continue };
            let key = fold(&target);
            if written.contains(&key) && (key == asked.1 || reaches.get(&key).is_some_and(|set| set.contains(&asked.1))) {
                found = Some(Lead { ty: row.kind.to_string(), target, external: false });
                break;
            }
        }
        self.gone_found.insert(asked, found.clone());
        found
    }
}

impl<'a> Book<'a> {
    /// The rows of the pages written in a part of their own that had none (`Page::part_name`: a page added, a copy of a page), and the shapes written in place of theirs: planned **before the
    /// parts no row reaches are left out** (`orphans`), so that an object a copy names stays when the page it was copied from is gone.
    pub(super) fn carry_new_pages(&mut self) -> R<()> {
        let doc: &'a Document = self.doc;
        let new: Vec<(usize, String)> = self
            .entries
            .iter()
            .filter_map(|e| match e.body {
                super::held::Body::Page(at) => {
                    doc.pages.get(at).filter(|p| p.part_name.is_none() && holds_foreign(&p.shapes)).map(|_| (at, e.name.clone()))
                }
                _ => None,
            })
            .collect();
        if new.is_empty() {
            return Ok(());
        }
        let mut sources = Sources::default();
        sources.add(doc.pages.iter().filter_map(|p| p.part_name.as_deref().map(|part| (&**p, part))));
        let written: HashSet<String> = self.entries.iter().map(|e| fold(&e.name)).collect();
        let mut made: Vec<(usize, String, Carried)> = Vec::new();
        {
            let mut leads =
                ModelLeads { book: self, sources, written, held_by_id: HashMap::new(), gone_rows: None, reaches: None, gone_found: HashMap::new() };
            for (at, part) in new {
                let Some(page) = doc.pages.get(at) else { continue };
                let carried = carry(page, &mut leads)?;
                made.push((at, part, carried));
            }
        }
        for (at, part, carried) in made {
            let rows = self.rows.entry(fold(&part)).or_default();
            for (id, lead) in &carried.rows {
                rows.push(lead.row(id, &part));
            }
            if let Some(shapes) = carried.shapes {
                self.carried_shapes.insert(at, shapes);
            }
        }
        Ok(())
    }
}

impl ModelLeads<'_, '_> {
    /// Where the held row of `Id` `id` of the part `source` leads, when it leads outside or to a part the plan writes.
    fn held_lead(&mut self, source: &str, id: &str) -> Option<Lead> {
        let row = self.held_row(source, id)?;
        if row.external {
            return Some(Lead { ty: row.kind.to_string(), target: row.target.to_string(), external: true });
        }
        resolve(source, &row.target).filter(|t| self.written.contains(&fold(t))).map(|target| Lead {
            ty: row.kind.to_string(),
            target,
            external: false,
        })
    }
}

impl Leads for ModelLeads<'_, '_> {
    fn named(&mut self, foreign: &Arc<ForeignData>) -> R<Option<Lead>> {
        let Some(id) = foreign.rel_id.as_deref() else { return Ok(None) };
        let Some(source) = self.sources.of(foreign).map(str::to_owned) else {
            return Ok(foreign.picture.as_ref().and_then(|shown| self.gone_row(id, shown)));
        };
        Ok(self.held_lead(&source, id))
    }

    /// A page taken out before the save is not searched for kept markup: with no picture read, the rows of gone pages that lead to it cannot be told apart (a copy whose source page is gone
    /// keeps its markup as it is).
    fn named_kept(&mut self, list: &Arc<Vec<Placed>>, at: usize, id: &str) -> R<Option<Lead>> {
        let Some(source) = self.sources.of_kept(list, at, id).map(str::to_owned) else { return Ok(None) };
        if let Some(lead) = self.held_lead(&source, id) {
            return Ok(Some(lead));
        }
        // A row that a picture that is read names is not held: the writer makes it from the picture (`attach`), so kept markup that names it leads to the part of that picture.
        match self.sources.picture_of_row(&source, id).cloned() {
            Some(media) => self.picture(&media),
            None => Ok(None),
        }
    }

    fn picture(&mut self, media: &Arc<Media>) -> R<Option<Lead>> {
        Ok(self.book.media_of.get(&(Arc::as_ptr(media) as usize)).and_then(|i| self.book.media.get(*i)).map(|plan| Lead::picture(&plan.name)))
    }

    fn object(&mut self, shape: &Arc<Shape>) -> R<Option<ObjectLead>> {
        Ok(self.book.object_lead(shape))
    }
}
