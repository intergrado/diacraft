//! Shapes: the `Shape` elements of a page or master part, their text, their pictures and the `Connect` rows beside them.
//!
//! **Depth and count are checked before a shape is built.** A shape one level past `Limits::max_shape_depth`, the one past the cap of shapes in
//! a part, and the one past the document's total are refused (`VsdxError::Limit`) before anything is stored for it. The recursion is one
//! frame for each level and the level is checked first, so it cannot pass the cap (200, and `opc`'s XML depth cap is below it in the
//! default limits); a test runs it on a small stack.
//!
//! **What `Master` and `MasterShape` do here.** In a page part they are the shape's ties to a master: a
//! nested shape's `MasterShape` is looked up in the master of the nearest ancestor that has a `Master`. A
//! `Master` or `MasterShape` that names nothing is read all the same and warned of: the model keeps the attribute and the
//! resolver treats the shape as unlinked. In a master part neither is read (a master step is one level), so a master cannot name a master,
//! and no cycle of masters can be made: the attributes are kept in `raw`.

use std::collections::hash_map::Entry;
use std::sync::Arc;

use diacraft_doc::{Connect, Connects, ForeignData, Kept, Media, RawAttrs, RawNode, Shape, ShapeList, ShapeType, Sheet, Text, TextPart};
use diacraft_opc::xml::{El, Node};

use diacraft_sheetxml::price::{self, arc_str, kept_bytes};

use crate::cost::{self, FOREIGN_BYTES, TEXT_BYTES};
use crate::error::{LimitKind, VsdxError};
use crate::held::Derived;
use crate::ns::{Scope, ScopeExt};
use crate::passthrough::{all_attrs, canonical_u32, element_cost, raw_element, read_flag, read_u32};
use crate::reader::{Parsed, Reader, RelMap};
use crate::sheet::{SheetTally, Took, style_link};

/// The part being read, for the shapes in it.
pub(crate) struct PartState<'a> {
    /// The part's name, for messages.
    pub name: &'a str,
    /// A master part: `Master` and `MasterShape` are not read in one.
    pub is_master: bool,
    /// The part's relationships, for the pictures its shapes name.
    pub rels: &'a RelMap,
    /// Shapes read from this part so far, nested ones counted.
    pub shapes: usize,
    /// The shape `ID`s met in this part so far, with whether the shape was deleted, for the warning about an `ID` used twice.
    pub ids: std::collections::HashMap<u32, bool>,
    /// The `Id`s of the part's relationships that a picture was loaded through and whose rows say no more than the picture's fields do (`held.rs`): the first
    /// row of each is the picture's, and is not held.
    pub derived: Derived,
}

const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

impl Reader<'_> {
    /// Reads a part's `Shapes` element: the shapes it holds, at depth 1, and what it makes of them. `master_scope` is the master a
    /// shape's `MasterShape` is looked up in, as the index of that master in the document's list. **What the element holds besides its shapes** (its attributes,
    /// its comments and text, and every child that is no shape, each after the shapes before it) comes back as a [`Kept`], for the owner of the element to hold.
    pub(crate) fn read_shapes(
        &mut self,
        ps: &mut PartState<'_>,
        el: &El,
        sc: &Scope,
        depth: usize,
        master_scope: Option<usize>,
    ) -> Result<(ShapeList, Kept), VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, shapes.len(), node)?;
                continue;
            };
            let csc = sc.enter(child);
            if csc.ns_of(child).is_ours() && child.is("Shape") {
                let shape = self.read_shape(ps, child, &csc, depth, master_scope, shapes.len())?;
                shapes.push(Arc::new(shape));
            } else {
                self.pass_element(&mut kept.kept, shapes.len(), child)?;
            }
        }
        Ok((ShapeList::from_vec(shapes), kept))
    }

    /// One `Shape` element, at `depth` (1 for a top-level shape).
    fn read_shape(
        &mut self,
        ps: &mut PartState<'_>,
        el: &El,
        sc: &Scope,
        depth: usize,
        master_scope: Option<usize>,
        before: usize,
    ) -> Result<Shape, VsdxError> {
        // The caps, before anything is stored for this shape.
        if depth > self.caps.shape_depth {
            return Err(VsdxError::Limit { kind: LimitKind::ShapeDepth, max: self.caps.shape_depth as u64 });
        }
        if ps.shapes >= self.caps.shapes_per_part {
            return Err(VsdxError::Limit { kind: LimitKind::ShapesInAPart, max: self.caps.shapes_per_part as u64 });
        }
        if self.shapes_left == 0 {
            return Err(VsdxError::Limit { kind: LimitKind::Shapes, max: self.caps.shapes as u64 });
        }
        self.take_items(1 + el.attrs.len())?;
        // The shape, its sheet and its place in the list of its parent, and every string it keeps: its names, and the attributes the model has no field for (a
        // number or a flag that reads as it is written is typed).
        let bytes = cost::attrs_bytes(cost::shape_bytes(before), &el.attrs, cost::shape_attr(ps.is_master));
        self.take_bytes(bytes)?;
        ps.shapes += 1;
        self.shapes_left -= 1;
        self.max_shape_depth = self.max_shape_depth.max(depth);

        let mut shape = Shape::default();
        let mut sheet = Sheet::default();
        let mut has_id = false;
        let mut id_is_a_number = false;
        for (k, v) in &el.attrs {
            match k.as_str() {
                "ID" => {
                    has_id = true;
                    let (id, needs_raw) = read_u32(v);
                    id_is_a_number = id.is_some();
                    shape.id = id.unwrap_or(0);
                    if needs_raw {
                        shape.raw.push(k, v);
                        self.notes.warn("shape_id_not_a_number", || {
                            format!("a shape in {} has an ID that is not a plain unsigned number", ps.name.escape_debug())
                        });
                    }
                }
                "Name" => shape.name = Some(Arc::from(v.as_str())),
                "NameU" => shape.name_u = Some(Arc::from(v.as_str())),
                "Type" => shape.kind = ShapeType::read(Some(v)),
                "Master" if !ps.is_master => {
                    let (master, needs_raw) = read_u32(v);
                    shape.master = master;
                    if needs_raw {
                        shape.raw.push(k, v);
                    }
                }
                "MasterShape" if !ps.is_master => {
                    let (master_shape, needs_raw) = read_u32(v);
                    shape.master_shape = master_shape;
                    if needs_raw {
                        shape.raw.push(k, v);
                    }
                }
                "Del" => {
                    let (del, needs_raw) = read_flag(v);
                    shape.del = del;
                    if needs_raw {
                        shape.raw.push(k, v);
                    }
                }
                _ => {
                    if !style_link(&mut sheet, k, v) {
                        shape.raw.push(k, v);
                    }
                }
            }
        }
        if !has_id {
            self.notes.warn("shape_without_id", || format!("a shape in {} has no ID and is read as ID 0", ps.name.escape_debug()));
        } else if id_is_a_number {
            // (`ID="007"` is the number 7 with its text kept, and 7 again is a repeat; `ID="x"` is no number, was warned of, and is not tracked.)
            // An `ID` used twice in a part: a reference to it finds the first shape, in document order (a shape before its children, a
            // shape before its next sibling), and the later one is read and kept all the same. 487 of the sample files' 495 are between shapes one of
            // which is deleted (`Del="1"`), which the other class says.
            match ps.ids.entry(shape.id) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(shape.del);
                }
                Entry::Occupied(first) => {
                    let class = if *first.get() || shape.del { "duplicate_shape_id_deleted" } else { "duplicate_shape_id" };
                    self.notes.warn(class, || format!("two shapes in {} have ID {}; a reference finds the first", ps.name.escape_debug(), shape.id));
                }
            }
        }
        self.check_links(ps, &shape, &sheet);

        // The master this shape's members look their `MasterShape` up in: its own, when it names one, else the one it was given.
        let own_master = shape.master.and_then(|id| self.master_index.get(&id).copied());
        let scope_for_members = if shape.master.is_some() { own_master } else { master_scope };

        let mut typed = 0usize;
        let mut tally = SheetTally::default();
        let mut text_seen = false;
        let mut foreign_seen = false;
        let mut shapes_seen = false;
        let mut order = crate::unheld::KeptOrder::default();
        for node in &el.children {
            order.note(shape.passthrough.len(), sheet.passthrough.len());
            let Node::El(child) = node else {
                self.pass_other(&mut shape.passthrough, typed, node)?;
                continue;
            };
            let csc = sc.enter(child);
            if !csc.ns_of(child).is_ours() {
                self.pass_element(&mut shape.passthrough, typed, child)?;
                continue;
            }
            match self.sheet_child(&mut sheet, &mut tally, child, &csc, ps.name, typed)? {
                Took::Typed => {
                    typed += 1;
                    continue;
                }
                Took::Kept => continue,
                Took::No => {}
            }
            let typed_here = match child.local() {
                "Text" if !text_seen => {
                    text_seen = true;
                    shape.text = Some(Arc::new(self.read_text(child, &csc)?));
                    true
                }
                "Data1" | "Data2" | "Data3" => {
                    let slot = match child.local() {
                        "Data1" => &mut shape.data1,
                        "Data2" => &mut shape.data2,
                        _ => &mut shape.data3,
                    };
                    match plain_text(child) {
                        Some(text) if slot.is_none() => {
                            self.take_bytes(arc_str(text.len()))?;
                            *slot = Some(Arc::from(text));
                            true
                        }
                        _ => false,
                    }
                }
                // `ForeignData` is read only when `Type` is `Foreign`; on any other shape it is an unknown element (kept, no picture loaded).
                "ForeignData" if !foreign_seen && shape.kind == ShapeType::Foreign => {
                    foreign_seen = true;
                    shape.foreign = Some(Arc::new(self.read_foreign(ps, child, &csc)?));
                    true
                }
                "Shapes" if !shapes_seen => {
                    shapes_seen = true;
                    let (members, rest) = self.read_shapes(ps, child, &csc, depth + 1, scope_for_members)?;
                    shape.children = members;
                    self.put_rest(&mut shape.ext, "Shapes", rest)?;
                    true
                }
                _ => false,
            };
            if typed_here {
                typed += 1;
            } else {
                self.pass_element(&mut shape.passthrough, typed, child)?;
            }
        }
        order.note(shape.passthrough.len(), sheet.passthrough.len());
        self.put_order(&mut shape.ext, order)?;
        if let Some(ms) = shape.master_shape
            && shape.master.is_none()
            && !ps.is_master
        {
            self.check_master_shape(ps, ms, master_scope);
        }
        self.note_sheet(&sheet);
        shape.sheet = self.share_sheet(sheet, &tally);
        Ok(shape)
    }

    /// Counts the ties of a shape that name nothing (and the style links): they are read as
    /// they are, and the resolver treats a missing one as no link.
    fn check_links(&mut self, ps: &PartState<'_>, shape: &Shape, sheet: &Sheet) {
        if let Some(id) = shape.master
            && !self.master_index.contains_key(&id)
        {
            self.notes
                .warn("master_not_found", || format!("a shape in {} names master {id}, which the document does not have", ps.name.escape_debug()));
        }
        for link in [sheet.links.line, sheet.links.fill, sheet.links.text].into_iter().flatten() {
            if !self.style_ids.contains(&link) {
                self.notes.warn("style_not_found", || {
                    format!("a shape in {} names style {link}, which the document does not have", ps.name.escape_debug())
                });
            }
        }
    }

    /// `MasterShape` names a shape of the master of the nearest ancestor with a `Master`: it must be one.
    fn check_master_shape(&mut self, ps: &PartState<'_>, id: u32, master_scope: Option<usize>) {
        let Some(master) = master_scope else {
            self.notes.warn("master_shape_without_master", || {
                format!("a shape in {} names master shape {id} and no shape above it names a master", ps.name.escape_debug())
            });
            return;
        };
        if !self.master_shape_ids(master).contains(&id) {
            self.notes.warn("master_shape_not_found", || {
                format!("a shape in {} names master shape {id}, which its master does not have", ps.name.escape_debug())
            });
        }
    }

    /// The `ID`s of all the shapes in a master's part, nested ones too, built when first asked for.
    fn master_shape_ids(&mut self, master: usize) -> &std::collections::HashSet<u32> {
        if !self.master_shape_ids.contains_key(&master) {
            let mut ids = std::collections::HashSet::new();
            if let Some(Some(parsed)) = self.masters_parsed.get(master) {
                collect_ids(&parsed.shapes, &mut ids);
            }
            self.master_shape_ids.insert(master, ids);
        }
        self.master_shape_ids.get(&master).unwrap_or(&EMPTY_IDS)
    }

    /// A shape's `Text` element: mixed content, kept in pieces so that the markers and the rows they name stay in step.
    fn read_text(&mut self, el: &El, sc: &Scope) -> Result<Text, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        // Its attributes are held; its comments, instructions, text and children are its parts.
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        // The text, and the list of its parts, made whole from the children.
        self.take_bytes(TEXT_BYTES.saturating_add(cost::text_parts_bytes(el.children.len())))?;
        let mut parts = Vec::with_capacity(el.children.len());
        for node in &el.children {
            self.take_items(1)?;
            parts.push(match node {
                Node::Text(t) | Node::CData(t) => {
                    self.take_bytes(cost::text_part_bytes(t.len()))?;
                    TextPart::Chars(Arc::from(t.as_str()))
                }
                Node::Comment(c) => {
                    self.take_bytes(cost::text_part_bytes(c.len()))?;
                    TextPart::Raw(RawNode::Comment(Arc::from(c.as_str())))
                }
                Node::Pi { target, data } => {
                    self.take_bytes(cost::text_part_bytes(target.len()).saturating_add(cost::text_part_bytes(data.len())))?;
                    TextPart::Raw(RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) })
                }
                Node::El(child) => {
                    let csc = sc.enter(child);
                    // The one marker that holds text is a field: its text is a string of its own, made before the element is known to be a marker.
                    if child.is("fld") {
                        self.take_bytes(child.children.iter().fold(0u64, |sum, n| match n {
                            Node::Text(t) | Node::CData(t) => sum.saturating_add(cost::text_part_bytes(t.len())).saturating_add(price::heap(t.len())),
                            _ => sum,
                        }))?;
                    }
                    match marker(child, &csc) {
                        Some(part) => part,
                        None => {
                            // The element's own item is taken above; what is in it is taken here.
                            self.take_items(element_cost(child).saturating_sub(1))?;
                            self.take_bytes(kept_bytes(child))?;
                            TextPart::Raw(RawNode::Element(raw_element(child)))
                        }
                    }
                }
            });
        }
        let mut text = Text { parts, ..Default::default() };
        self.put_rest(&mut text.ext, "Text", kept)?;
        Ok(text)
    }

    /// A `ForeignData` element: its attributes as they are, the relationship it names, and the picture that relationship reaches.
    fn read_foreign(&mut self, ps: &mut PartState<'_>, el: &El, sc: &Scope) -> Result<ForeignData, VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        self.take_bytes(FOREIGN_BYTES.saturating_add(cost::all_attrs_bytes(el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())))))?;
        let mut foreign = ForeignData { attrs: all_attrs(el), ..ForeignData::default() };
        let mut typed = 0usize;
        let mut text = String::new();
        let mut has_text = false;
        let mut text_first = false;
        for node in &el.children {
            match node {
                Node::El(child) => {
                    let csc = sc.enter(child);
                    if foreign.rel_id.is_none() && csc.ns_of(child).is_ours() && child.is("Rel") && csc.rel_id(child).is_some() {
                        text_first = has_text;
                        foreign.rel_id = csc.rel_id(child).map(Arc::from);
                        self.take_bytes(arc_str(foreign.rel_id.as_deref().map_or(0, str::len)))?;
                        let rest = self.hold_rel(child, &csc)?;
                        self.put_rest(&mut foreign.ext, "Rel", rest)?;
                        typed += 1;
                    } else {
                        self.pass_element(&mut foreign.passthrough, typed, child)?;
                    }
                }
                Node::Text(t) | Node::CData(t) => {
                    if !crate::passthrough::is_blank(t) {
                        has_text = true;
                    }
                    text.push_str(t);
                }
                other => self.pass_other(&mut foreign.passthrough, typed, other)?,
            }
        }
        if has_text {
            self.take_items(1)?;
            self.take_bytes(arc_str(text.len()).saturating_add(price::heap(text.len())))?;
            foreign.text = Some(Arc::from(text));
            // A text that came before the relationship is held as having: a writer puts it after the `Rel` otherwise.
            if text_first {
                self.put_mark(&mut foreign.ext, "TextFirst", Kept::default())?;
            }
        }
        if let Some(id) = foreign.rel_id.clone() {
            foreign.picture = self.resolve_picture(ps, &id)?;
        }
        Ok(foreign)
    }

    /// The picture a `Rel` of a part's shape reaches: the target itself when the relationship is an image, else the image that target
    /// has a relationship to (an embedded object's fallback). `None` and a warning when there is none, or it is past the budget.
    fn resolve_picture(&mut self, ps: &mut PartState<'_>, id: &str) -> Result<Option<Arc<Media>>, VsdxError> {
        let rels = ps.rels;
        let Some(rel) = rels.get(id) else {
            self.notes.warn("picture_unresolved", || format!("a picture in {} names a relationship that is not there", ps.name.escape_debug()));
            return Ok(None);
        };
        let Some(target) = rel.target.clone() else {
            self.notes.warn("picture_unresolved", || format!("a picture in {} names a target that is not a part", ps.name.escape_debug()));
            return Ok(None);
        };
        // The target of an `image` relationship is a picture when it is of a format a viewer draws. A relationship of that type to a part of
        // another kind (the document part, say) is no picture, and is treated as the unsupported target it is: its own fallback, if it has one.
        if rel.ty == IMAGE_REL && self.is_supported_picture(&target) {
            let media = self.load_image(ps.name, &target);
            // A picture that was loaded is the shape's: the row that says so is the picture's too when it says no more than its fields do (`held.rs`).
            if media.is_some() && crate::write::relative_target(ps.name, &target) == rel.target_raw {
                self.derive(&mut ps.derived, id)?;
            }
            return Ok(media);
        }
        let key = diacraft_opc::name::fold(&target);
        if let Some(found) = self.media.fallback(&key) {
            return Ok(found.clone());
        }
        let fallback = match self.rels_of(&target) {
            Ok(rels) => {
                let image =
                    rels.items.iter().filter(|r| r.ty == IMAGE_REL).filter_map(|r| r.target.clone()).find(|name| self.is_supported_picture(name));
                image.and_then(|name| self.load_image(ps.name, &name))
            }
            Err(VsdxError::Package(_)) => None,
            Err(e) => return Err(e),
        };
        if fallback.is_none() {
            self.notes.warn("picture_unresolved", || format!("a shape in {} names an object with no picture to show", ps.name.escape_debug()));
        }
        self.media.put_fallback(key, fallback.clone());
        Ok(fallback)
    }

    /// Whether the part `name` is of one of the formats R45 lists (BMP, EMF, GIF, JPEG, PNG, TIFF), by its content type.
    fn is_supported_picture(&self, name: &str) -> bool {
        self.pkg.content_type(name).is_some_and(is_picture_type)
    }

    /// Takes from the pool what a picture of `len` bytes held under `name` costs, the bytes and the rest, all of it or nothing: what is taken for the bytes is given back
    /// when the rest does not fit.
    fn take_picture(&mut self, len: usize, name: &str) -> bool {
        let copy = arc_str(len);
        if self.take_exact(copy).is_err() {
            return false;
        }
        if self.take_bytes(cost::picture_bytes(0, name.len(), self.pkg.content_type(name).map_or(24, str::len))).is_err() {
            self.give_bytes(copy);
            return false;
        }
        true
    }

    /// The part `name` as a picture, copied once however many shapes show it.
    fn load_image(&mut self, in_part: &str, name: &str) -> Option<Arc<Media>> {
        let key = diacraft_opc::name::fold(name);
        if let Some(found) = self.media.image(&key) {
            return found.clone();
        }
        let loaded = match self.pkg.part(name) {
            None => {
                self.notes.warn("picture_unresolved", || format!("a picture in {} names a part that is not in the package", in_part.escape_debug()));
                None
            }
            Some(bytes) => {
                let size = bytes.len() as u64;
                if self.media.bytes.saturating_add(size) > self.limits.max_media_bytes {
                    self.notes.warn("media_over_budget", || {
                        format!(
                            "a picture in {} is not loaded: the pictures would pass {} bytes",
                            in_part.escape_debug(),
                            self.limits.max_media_bytes
                        )
                    });
                    None
                } else if !self.take_picture(bytes.len(), name) {
                    // A picture is the one thing the pool can leave out: the shape shows nothing, and the report says so.
                    self.notes.warn("media_over_budget", || {
                        format!("a picture in {} is not loaded: there is no memory left for it", in_part.escape_debug())
                    });
                    None
                } else {
                    self.media.bytes += size;
                    let content_type = self.pkg.content_type(name).unwrap_or("application/octet-stream");
                    Some(Arc::new(Media { part: Arc::from(name), content_type: Arc::from(content_type), bytes: Arc::from(bytes) }))
                }
            }
        };
        self.media.put_image(key, loaded.clone());
        loaded
    }

    /// A `Connects` element: its `Connect` rows. A row with no readable `FromSheet` or `ToSheet` is not a connection and is skipped.
    pub(crate) fn read_connects(&mut self, el: &El, sc: &Scope) -> Result<(Connects, Kept), VsdxError> {
        self.take_items(1 + el.attrs.len())?;
        let mut kept = Kept::default();
        self.hold_attrs(el, &mut kept, |_| false, false)?;
        let mut list = Vec::new();
        for node in &el.children {
            let Node::El(child) = node else {
                self.pass_other(&mut kept.kept, list.len(), node)?;
                continue;
            };
            {
                let csc = sc.enter(child);
                if csc.ns_of(child).is_ours() && child.is("Connect") {
                    self.take_items(1 + child.attrs.len())?;
                    // The row in the list, its names, and the attributes it keeps (a number that reads as it is written is typed).
                    self.take_bytes(
                        cost::connect_bytes(list.len())
                            .saturating_add(child.attrs.iter().fold(0u64, |sum, (k, v)| match k.as_str() {
                                "FromSheet" | "ToSheet" | "FromPart" | "ToPart" if diacraft_sheetxml::passthrough::canonical_u32(v).is_some() => sum,
                                "FromCell" | "ToCell" => sum,
                                _ => sum.saturating_add(price::raw_attr_bytes(0, k.len(), v.len())),
                            }))
                            .saturating_add(arc_str(child.attr("FromCell").map_or(0, str::len)))
                            .saturating_add(arc_str(child.attr("ToCell").map_or(0, str::len))),
                    )?;
                    match connect(child) {
                        Some(mut c) => {
                            // The row's attributes are the model's (`raw`); a child of it, or a comment, is held.
                            let mut rest = Kept::default();
                            self.hold_children(child, &mut rest)?;
                            self.put_rest(&mut c.ext, "Connect", rest)?;
                            list.push(c);
                        }
                        // A row with no `FromSheet` or no `ToSheet` that is a number is no connection: held where it was.
                        None => self.pass_element(&mut kept.kept, list.len(), child)?,
                    }
                } else {
                    self.pass_element(&mut kept.kept, list.len(), child)?;
                }
            }
        }
        self.connects += list.len();
        Ok((Connects::from_vec(list), kept))
    }

    /// Takes a parsed part into the cache the pages and masters share.
    pub(crate) fn remember(&mut self, folded: String, parsed: Option<Parsed>) {
        self.parsed.insert(folded, parsed);
    }
}

static EMPTY_IDS: std::sync::LazyLock<std::collections::HashSet<u32>> = std::sync::LazyLock::new(std::collections::HashSet::new);

/// The `ID` of every shape in a tree, nested ones too, with no recursion.
fn collect_ids(top: &ShapeList, into: &mut std::collections::HashSet<u32>) {
    let mut stack: Vec<&ShapeList> = vec![top];
    while let Some(list) = stack.pop() {
        for shape in list.iter() {
            into.insert(shape.id);
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
}

/// The text of an element that holds text and nothing else (no attributes, no child elements, comments or instructions).
fn plain_text(el: &El) -> Option<String> {
    if !el.attrs.is_empty() || !el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) {
        return None;
    }
    Some(el.text())
}

/// `<cp IX="n"/>`, `<pp IX="n"/>`, `<tp IX="n"/>` and `<fld IX="n">text</fld>`, as the model's text parts. Anything with another attribute,
/// another child, a number that is not plain, or another namespace is `None`, and the caller keeps the element whole.
fn marker(el: &El, sc: &Scope) -> Option<TextPart> {
    if !sc.ns_of(el).is_ours() {
        return None;
    }
    let ix = match el.attrs.as_slice() {
        [(k, v)] if k == "IX" => canonical_u32(v)?,
        _ => return None,
    };
    match el.local() {
        "cp" if el.children.is_empty() => Some(TextPart::Cp(ix)),
        "pp" if el.children.is_empty() => Some(TextPart::Pp(ix)),
        "tp" if el.children.is_empty() => Some(TextPart::Tp(ix)),
        "fld" if el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) => Some(TextPart::Fld { ix, text: Arc::from(el.text()) }),
        _ => None,
    }
}

/// A `Connect` row, or `None` when it has no `FromSheet` or no `ToSheet` that is a number.
fn connect(el: &El) -> Option<Connect> {
    let mut c = Connect::default();
    let (mut from, mut to) = (false, false);
    let mut raw = RawAttrs::new();
    for (k, v) in &el.attrs {
        match k.as_str() {
            "FromSheet" => {
                let (n, needs_raw) = read_u32(v);
                c.from_sheet = n?;
                from = true;
                if needs_raw {
                    raw.push(k, v);
                }
            }
            "ToSheet" => {
                let (n, needs_raw) = read_u32(v);
                c.to_sheet = n?;
                to = true;
                if needs_raw {
                    raw.push(k, v);
                }
            }
            "FromCell" => c.from_cell = Some(Arc::from(v.as_str())),
            "ToCell" => c.to_cell = Some(Arc::from(v.as_str())),
            "FromPart" => {
                let (n, needs_raw) = read_u32(v);
                c.from_part = n;
                if needs_raw {
                    raw.push(k, v);
                }
            }
            "ToPart" => {
                let (n, needs_raw) = read_u32(v);
                c.to_part = n;
                if needs_raw {
                    raw.push(k, v);
                }
            }
            _ => raw.push(k, v),
        }
    }
    c.raw = raw;
    (from && to).then_some(c)
}

/// Whether `content_type` is one of the types R45 lists as a picture a viewer draws (BMP, EMF, GIF, JPEG, PNG and TIFF, and the spellings seen of them): the reader finds a picture, and an
/// embedded object's fallback, by the type of its part, and the writer types a picture it writes by the same list (`write/model.rs`, `picture_content_type`).
pub(crate) fn is_picture_type(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    matches!(
        ct.as_str(),
        "image/bmp"
            | "image/x-ms-bmp"
            | "image/x-emf"
            | "image/emf"
            | "image/gif"
            | "image/jpeg"
            | "image/jpg"
            | "image/pjpeg"
            | "image/png"
            | "image/x-png"
            | "image/tiff"
    )
}
