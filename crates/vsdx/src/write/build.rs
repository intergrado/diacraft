//! The model as a tree: the inverse of what the reader does, for elements that are new.
//!
//! **The attribute lists are the inverse of the reader's.** `shape_attrs`, `page_attrs` and the rest give, for an element of the model, the
//! attributes the reader would have read it from: the typed fields as text, and the attributes kept in `raw`, **where `raw` wins** (an `ID`
//! of `007` is kept in `raw` as it was, and the typed field holds the number; `passthrough.rs`). The patcher uses the same lists to see which
//! attribute of an element the model changed, so a reader and a writer cannot disagree about what an attribute is.
//!
//! **The sheet's half is `diacraft-sheetxml`'s** (`build.rs` there): the cell, row, section and sheet elements and their attributes,
//! the unknown nodes put back among them, and the builder of an attribute list. What is this crate's is the elements around a sheet.
//!
//! **Order.** New elements are written in the order of R18: `Cell`, `Section`, `Text`, `Data`, `ForeignData`, `Shapes`; cells in the
//! model are kept by name, so they come out in the order of their names. Unknown nodes (the model's `Passthrough`) are put back after as
//! many of the typed children as they followed (`Placed::after`). Attributes come typed first, in the order of the schema, then the rest of
//! `raw` in the order it was read.
//!
//! **Cells are written as the strings held**: `V` is the text of the value, `F` the text of the formula or its marker, `U` and `E`
//! verbatim. Nothing is parsed and printed again.

use diacraft_doc::{
    ColorEntry, Connect, Connects, Ext, FaceName, ForeignData, Kept, MAX_SHAPE_DEPTH, Master, OdfDoc, Page, Passthrough, RawNode, Shape, ShapeList,
    Style, Text, TextPart,
};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::build::{AttrBuilder, flag, sheet_is_empty, sheet_nodes, sheet_only_el, text, weave};
pub(super) use diacraft_sheetxml::build::{Attrs, element, with_prefix};

use super::WriteError;
use crate::limits::Limits;
use crate::passthrough::node_to_tree;

// ------------------------------------------------------------------------------------------------------------------------ the room of a part

/// What is left of what the reader takes of **one part**, as the part is made: the items of its tree and the bytes it is written as, at the caps the
/// read-back of the save holds it to (`Limits::default()`: [`diacraft_opc::xml::XmlLimits::max_nodes`] and [`Limits::max_xml_bytes`]). A model whose shapes hold equal sheets is a
/// few megabytes held and, written out shape by shape, a tree of gigabytes (ten thousand shapes of a thousand rows are 100 million items): the writer counts what each shape
/// makes and stops at the shape that passes a cap, in words, and does not wait for the read-back to find a part it has no room for.
struct Room {
    items: usize,
    bytes: u64,
}

thread_local! {
    static ROOM: std::cell::RefCell<Option<Room>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    static FOREIGN_R: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// **A page or master part whose root bound the prefix `r` to another namespace than the relationships one** (a file may bind it to the package
/// relationships namespace), while the part is made, until it is dropped. What the part keeps below its root that is spelt `r:` means what it meant in the file
/// only under the file's binding, so the root keeps it (`model::root_el`), and each `Rel` the writer makes declares `r` for itself (`rel_el`, `rel_el_with`):
/// a reader finds its relationship by the namespace, and the file's spelling keeps its own.
pub(super) struct ForeignR {
    outer: bool,
}

impl ForeignR {
    /// Starts a part whose root held `kept` (what the reader held of it, the prefixes it declared among it).
    pub(super) fn enter(kept: Option<&Kept>) -> ForeignR {
        let foreign = kept.and_then(|k| k.raw.get("xmlns:r")).is_some_and(|uri| uri != crate::ns::NS_REL);
        ForeignR { outer: FOREIGN_R.with(|f| f.replace(foreign)) }
    }

    /// Starts a part that is patched, whose root binds `r` to another namespace than the relationships one when `foreign` (a picture new to a page part names a relationship from it).
    pub(super) fn enter_foreign(foreign: bool) -> ForeignR {
        ForeignR { outer: FOREIGN_R.with(|f| f.replace(foreign)) }
    }

    /// Whether the part being made is one.
    pub(super) fn active() -> bool {
        FOREIGN_R.with(std::cell::Cell::get)
    }
}

impl Drop for ForeignR {
    fn drop(&mut self) {
        FOREIGN_R.with(|f| f.set(self.outer));
    }
}

/// The attributes of a `Rel` the writer makes: its relationship id, and `r` declared for it in a part whose root binds `r` elsewhere ([`ForeignR`]), unless what it
/// keeps declares it.
fn rel_attrs(id: &str, kept: Option<&Kept>) -> Attrs {
    let mut attrs = vec![("r:id".to_owned(), id.to_owned())];
    if ForeignR::active() && kept.and_then(|k| k.raw.get("xmlns:r")).is_none() {
        attrs.push(("xmlns:r".to_owned(), crate::ns::NS_REL.to_owned()));
    }
    attrs
}

/// The room of the part that is being made, until it is dropped (and then the room of the part that was being made before it, if any: a part is made inside another's save).
pub(super) struct PartRoom {
    outer: Option<Room>,
}

impl PartRoom {
    /// Starts a part that already holds `items` items and `bytes` bytes (a part that is patched holds what the file has of it).
    pub(super) fn enter(items: usize, bytes: u64) -> PartRoom {
        let limits = Limits::default();
        let room = Room { items: limits.opc.xml.max_nodes.saturating_sub(items), bytes: limits.max_xml_bytes.saturating_sub(bytes) };
        PartRoom { outer: ROOM.with(|r| r.borrow_mut().replace(room)) }
    }
}

impl Drop for PartRoom {
    fn drop(&mut self) {
        ROOM.with(|r| *r.borrow_mut() = self.outer.take());
    }
}

/// Takes `items` items and `bytes` bytes from the room of the part being made: an error in words when it has not so much. Nothing is counted when no part is being made.
fn take_room(items: usize, bytes: u64) -> Result<(), WriteError> {
    ROOM.with(|r| {
        let mut room = r.borrow_mut();
        let Some(room) = room.as_mut() else { return Ok(()) };
        if items > room.items {
            return Err(WriteError::Limit(
                "more items in one page or master part than a read of the file takes (its shapes are written out one by one, shared sheets and all)",
            ));
        }
        if bytes > room.bytes {
            return Err(WriteError::Limit(
                "more bytes in one page or master part than a read of the file takes (its shapes are written out one by one, shared sheets and all)",
            ));
        }
        room.items -= items;
        room.bytes -= bytes;
        Ok(())
    })
}

/// The items and the bytes (as written, near enough) that the kept nodes of `kept` make as a tree (`node_to_tree` makes an element of each kept element, with its attributes, and an item
/// of each text, comment and instruction): counted from the model's own nodes, **before any is made**. A stack of its own: nothing recurses on depth.
fn kept_tally(kept: &[&Passthrough]) -> (usize, u64) {
    raw_tally(kept.iter().flat_map(|p| p.iter().map(|placed| &placed.node)).collect())
}

/// [`kept_tally`] of the kept nodes `start`.
fn raw_tally(start: Vec<&RawNode>) -> (usize, u64) {
    let (mut items, mut bytes) = (0usize, 0u64);
    let mut stack: Vec<&RawNode> = start;
    while let Some(node) = stack.pop() {
        match node {
            RawNode::Element(e) => {
                items = items.saturating_add(1).saturating_add(e.attrs.len());
                bytes = bytes.saturating_add(2 * e.name.len() as u64 + 5);
                for (k, v) in e.attrs.iter() {
                    bytes = bytes.saturating_add((k.len() + v.len() + 4) as u64);
                }
                stack.extend(e.children.iter());
            }
            RawNode::Text(t) | RawNode::Comment(t) => {
                items = items.saturating_add(1);
                bytes = bytes.saturating_add(t.len() as u64);
            }
            RawNode::Pi { target, data } => {
                items = items.saturating_add(1);
                bytes = bytes.saturating_add((target.len() + data.len() + 5) as u64);
            }
        }
    }
    (items, bytes)
}

/// Takes from the room of the part what the kept content of an element will make, **before it is made**: a model that carries a large tree of unknown XML (kept from a file for the
/// save) in each of many shapes is refused at the shape that passes the part's caps, and the tree is not converted for it.
pub(super) fn take_kept(kept: &[&Passthrough]) -> Result<(), WriteError> {
    let (items, bytes) = kept_tally(kept);
    take_room(items, bytes)
}

/// Takes from the room of the part what the element `el`, kept whole (`windows.xml`, the DiaCraft data), will make as a tree, **before it is made**.
pub(super) fn take_raw(el: &diacraft_doc::RawElement) -> Result<(), WriteError> {
    let (items, bytes) = raw_tally(el.children.iter().collect());
    let own = el.attrs.iter().fold(2 * el.name.len() as u64 + 5, |sum, (k, v)| sum.saturating_add((k.len() + v.len() + 4) as u64));
    take_room(items.saturating_add(1).saturating_add(el.attrs.len()), bytes.saturating_add(own))
}

/// Takes from the room of the part what the properties of a custom properties part will make (each an element of its attributes and of the element that holds its value), **before any is made**.
pub(super) fn take_properties(list: &[&diacraft_doc::CustomProp]) -> Result<(), WriteError> {
    let (mut items, mut bytes) = (0usize, 0u64);
    for prop in list {
        // The `property`, the element of its value and the text in it, and the attributes (the name among them).
        items = items.saturating_add(3).saturating_add(prop.raw.len()).saturating_add(1);
        bytes = bytes.saturating_add((2 * prop.value_type.len() + prop.value.len() + prop.name.len() + 64) as u64);
        bytes = prop.raw.iter().fold(bytes, |sum, (k, v)| sum.saturating_add((k.len() + v.len() + 4) as u64));
    }
    take_room(items, bytes)
}

/// Takes from the room of the part the attributes of `kept` (what a `Text` keeps: its attributes are all of it that is written), before they are made.
fn take_kept_attrs(kept: &Kept) -> Result<(), WriteError> {
    take_room(kept.raw.len(), kept.raw.iter().map(|(k, v)| (k.len() + v.len() + 4) as u64).fold(0u64, u64::saturating_add))
}

/// Takes from the room of the part everything `kept` will make, its attributes and its nodes, before it is made.
pub(super) fn charge_kept(kept: &Kept) -> Result<(), WriteError> {
    take_kept(&[&kept.kept])?;
    take_kept_attrs(kept)
}

/// The items and the bytes (as written, near enough: the escapes are not counted) of `nodes` and everything under them. A stack of its own: nothing recurses on depth.
pub(super) fn tally(nodes: &[Node]) -> (usize, u64) {
    let (mut items, mut bytes) = (0usize, 0u64);
    let mut stack: Vec<&Node> = nodes.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            Node::El(e) => {
                items = items.saturating_add(1).saturating_add(e.attrs.len());
                bytes = bytes.saturating_add(2 * e.name.len() as u64 + 5);
                for (k, v) in &e.attrs {
                    bytes = bytes.saturating_add((k.len() + v.len() + 4) as u64);
                }
                stack.extend(e.children.iter());
            }
            Node::Text(t) | Node::CData(t) | Node::Comment(t) => {
                items = items.saturating_add(1);
                bytes = bytes.saturating_add(t.len() as u64);
            }
            Node::Pi { target, data } => {
                items = items.saturating_add(1);
                bytes = bytes.saturating_add((target.len() + data.len() + 5) as u64);
            }
        }
    }
    (items, bytes)
}

// ------------------------------------------------------------------------------------------------------------------ the slots of VSDX

/// What the VSDX side of the slot `ext` holds under `key`, the `n`th time the key comes (a colour entry's key comes again for an `IX` a hostile table uses twice).
pub(super) fn rest_nth<'a, T: Default + Clone + PartialEq>(ext: &'a Ext<T>, key: &str, n: usize) -> Option<&'a Kept> {
    ext.get()?.vsdx.0.iter().filter(|(k, _)| &**k == key).map(|(_, kept)| kept).nth(n)
}

/// What the VSDX side of the slot `ext` holds under `key`.
pub(super) fn rest_of<'a, T: Default + Clone + PartialEq>(ext: &'a Ext<T>, key: &str) -> Option<&'a Kept> {
    rest_nth(ext, key, 0)
}

/// `attrs`, and then the attributes of `kept` that `attrs` does not name: what an element holds that no field says comes after the fields' own, as everywhere (`AttrBuilder`).
pub(super) fn with_kept_attrs(mut attrs: Attrs, kept: Option<&Kept>) -> Attrs {
    if let Some(kept) = kept {
        for (name, value) in kept.raw.iter() {
            if !attrs.iter().any(|(n, _)| n == name) {
                attrs.push((name.to_owned(), value.to_owned()));
            }
        }
    }
    attrs
}

/// An element whose attributes and children are the fields' (`attrs` and `typed`) and what `kept` holds beside them: its attributes after the fields', and each node of it after as
/// many of the typed children as it came after. **What `kept` makes is taken from the room of the part before it is made**, as the kept content of a shape is.
pub(super) fn element_with_kept(name: &str, attrs: Attrs, typed: Vec<Node>, kept: Option<&Kept>) -> Result<El, WriteError> {
    let Some(kept) = kept else { return Ok(element(name, attrs, typed)) };
    charge_kept(kept)?;
    Ok(element(name, with_kept_attrs(attrs, Some(kept)), weave(typed, &[&kept.kept])))
}

/// Takes from the room of the part what the kept content of `kept` will make, **before it is made** (the writers of the property parts, whose elements are made beside the part's own).
pub(super) fn take_kept_for(kept: Option<&Kept>) -> Result<(), WriteError> {
    match kept {
        Some(kept) => take_kept(&[&kept.kept]),
        None => Ok(()),
    }
}

/// The kept nodes of an element and of its sheet put back among the typed children: in the order the file had them when that is held (`KeptOrder`), else the element's first.
fn weave_kept(typed: Vec<Node>, own: &Passthrough, sheet: &diacraft_doc::Sheet, order: Option<&Kept>) -> Vec<Node> {
    match order.and_then(|k| k.raw.get("order")) {
        Some(order) => diacraft_sheetxml::build::weave_ordered(typed, own, &sheet.passthrough, order),
        None => weave(typed, &[own, &sheet.passthrough]),
    }
}

/// A `Shape`'s attributes.
pub(super) fn shape_attrs(s: &Shape) -> Attrs {
    let raws = [&s.raw, &s.sheet.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("ID", Some(s.id.to_string()));
    b.typed("Name", text(&s.name));
    b.typed("NameU", text(&s.name_u));
    b.typed("Type", s.kind.text().map(str::to_owned));
    b.typed("Master", s.master.map(|n| n.to_string()));
    b.typed("MasterShape", s.master_shape.map(|n| n.to_string()));
    b.typed("Del", flag(s.del));
    b.links(&s.sheet);
    b.finish()
}

/// A `StyleSheet`'s attributes.
pub(super) fn style_attrs(s: &Style) -> Attrs {
    let raws = [&s.raw, &s.sheet.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("ID", Some(s.id.to_string()));
    b.typed("Name", text(&s.name));
    b.typed("NameU", text(&s.name_u));
    b.links(&s.sheet);
    b.finish()
}

/// A `Page` row's attributes.
pub(super) fn page_attrs(p: &Page) -> Attrs {
    let raws = [&p.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("ID", Some(p.id.to_string()));
    b.typed("Name", (!name_as_read_without_one(p)).then(|| p.name.to_string()));
    b.typed("NameU", text(&p.name_u));
    b.typed("Background", flag(p.background));
    b.typed("BackPage", p.back_page.map(|n| n.to_string()));
    b.finish()
}

/// Whether a page row had no `Name` (`NoName`) and the page's name is still the one the reader gave it for that: its `NameU`, or nothing when it had
/// none (`read.rs`, `read_page_row`). A page renamed since gets its `Name`; one whose name is still the row's reads back the same without it.
fn name_as_read_without_one(p: &Page) -> bool {
    rest_of(&p.ext, "NoName").is_some() && *p.name == *p.name_u.as_deref().unwrap_or("")
}

/// A `Master` row's attributes.
pub(super) fn master_attrs(m: &Master) -> Attrs {
    let raws = [&m.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("ID", Some(m.id.to_string()));
    b.typed("Name", text(&m.name));
    b.typed("NameU", text(&m.name_u));
    b.typed("UniqueID", text(&m.unique_id));
    b.typed("PatternFlags", m.pattern_flags.map(|n| n.to_string()));
    b.finish()
}

/// A `Connect`'s attributes.
pub(super) fn connect_attrs(c: &Connect) -> Attrs {
    let raws = [&c.raw];
    let mut b = AttrBuilder::new(&raws);
    b.typed("FromSheet", Some(c.from_sheet.to_string()));
    b.typed("FromCell", text(&c.from_cell));
    b.typed("FromPart", c.from_part.map(|n| n.to_string()));
    b.typed("ToSheet", Some(c.to_sheet.to_string()));
    b.typed("ToCell", text(&c.to_cell));
    b.typed("ToPart", c.to_part.map(|n| n.to_string()));
    b.finish()
}

// ------------------------------------------------------------------------------------------------------------------ elements

/// The pieces of a shape's text as nodes.
pub(super) fn text_children(t: &Text) -> Vec<Node> {
    t.parts.iter().filter_map(text_part).collect()
}

/// One piece of a text as a node. ODF's markup and a newer build's are not VSDX's, so they are never written into `Text` (the carriers route them).
fn text_part(part: &TextPart) -> Option<Node> {
    Some(match part {
        TextPart::Chars(s) => Node::Text(s.to_string()),
        TextPart::Cp(ix) => Node::El(element("cp", vec![("IX".to_owned(), ix.to_string())], vec![])),
        TextPart::Pp(ix) => Node::El(element("pp", vec![("IX".to_owned(), ix.to_string())], vec![])),
        TextPart::Tp(ix) => Node::El(element("tp", vec![("IX".to_owned(), ix.to_string())], vec![])),
        TextPart::Fld { ix, text } => Node::El(element("fld", vec![("IX".to_owned(), ix.to_string())], vec![Node::Text(text.to_string())])),
        TextPart::Raw(node) => node_to_tree(node),
        TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => return None,
    })
}

/// A shape's `Text` element.
pub(super) fn text_el(t: &Text) -> El {
    // What the element holds beside its parts is its attributes (its comments, text and children are the parts).
    element("Text", with_kept_attrs(Vec::new(), rest_of(&t.ext, "Text")), text_children(t))
}

fn foreign_el(f: &ForeignData) -> Result<El, WriteError> {
    // What the picture keeps is in the element made here, which is in the typed nodes of its shape, and the shape charges those once (charging it here as well would count it twice).
    let attrs: Attrs = f.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    let mut typed = Vec::new();
    if let Some(id) = &f.rel_id {
        typed.push(Node::El(rel_el_with(id, rest_of(&f.ext, "Rel"))?));
    }
    let mut children = weave(typed, &[&f.passthrough]);
    if let Some(t) = &f.text {
        // The text that came before the relationship came before it again.
        let at = if rest_of(&f.ext, "TextFirst").is_some() { 0 } else { children.len() };
        children.insert(at, Node::Text(t.to_string()));
    }
    Ok(element("ForeignData", attrs, children))
}

/// A shape, with its members, as an element. `depth` is how many shapes are above it; past the model's cap of nesting it is an error and
/// not a recursion.
pub(super) fn shape_el(s: &Shape, depth: usize) -> Result<El, WriteError> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(WriteError::Limit("levels of nested shapes"));
    }
    // The shape's own kept content (unknown XML read from a file and kept for the save) is counted before anything of the shape is made.
    take_kept(&[&s.passthrough, &s.sheet.passthrough])?;
    let mut typed = sheet_nodes(&s.sheet);
    if let Some(t) = &s.text {
        if let Some(rest) = rest_of(&t.ext, "Text") {
            take_kept_attrs(rest)?;
        }
        typed.push(Node::El(text_el(t)));
    }
    for (name, data) in [("Data1", &s.data1), ("Data2", &s.data2), ("Data3", &s.data3)] {
        if let Some(d) = data {
            typed.push(Node::El(element(name, vec![], vec![Node::Text(d.to_string())])));
        }
    }
    if let Some(f) = &s.foreign {
        typed.push(Node::El(foreign_el(f)?));
    }
    // What the shape made of its own (its element, attributes, sheet, text and data; the members are counted by their own shapes) is taken from the room of the part before
    // the members are made: a shape that passes a cap is the last that is made.
    let attrs = shape_attrs(s);
    let (items, bytes) = tally(&typed);
    take_room(items.saturating_add(1).saturating_add(attrs.len()), bytes.saturating_add(16))?;
    let members = rest_of(&s.ext, "Shapes");
    if !s.children.is_empty() || members.is_some() {
        typed.push(Node::El(shapes_el_with(&s.children, depth + 1, members)?));
    }
    let children = weave_kept(typed, &s.passthrough, &s.sheet, rest_of(&s.ext, "KeptOrder"));
    Ok(element("Shape", attrs, children))
}

/// A list of shapes as a `Shapes` element.
pub(super) fn shapes_el(list: &ShapeList, depth: usize) -> Result<El, WriteError> {
    shapes_el_with(list, depth, None)
}

/// A list of shapes as a `Shapes` element, with what the element holds beside its shapes (`kept`: its attributes, and the comments and elements between them).
pub(super) fn shapes_el_with(list: &ShapeList, depth: usize, kept: Option<&Kept>) -> Result<El, WriteError> {
    let mut children = Vec::with_capacity(list.len());
    for shape in list.iter() {
        children.push(Node::El(shape_el(shape, depth)?));
    }
    element_with_kept("Shapes", Vec::new(), children, kept)
}

/// A `Connect` row.
pub(super) fn connect_el(c: &Connect) -> El {
    // What the row holds beside its attributes is its children and comments, all before any typed child (a row has none).
    let children = rest_of(&c.ext, "Connect").map_or_else(Vec::new, |kept| weave(Vec::new(), &[&kept.kept]));
    element("Connect", connect_attrs(c), children)
}

/// A `Connects` element.
pub(super) fn connects_el(connects: &Connects) -> El {
    element("Connects", Vec::new(), connects.iter().map(|c| Node::El(connect_el(c))).collect())
}

/// A `Connects` element with what it holds beside its rows (`kept`), the content of the rows' own slots charged to the room of the part before it is made.
pub(super) fn connects_el_with(connects: &Connects, kept: Option<&Kept>) -> Result<El, WriteError> {
    let mut children = Vec::with_capacity(connects.len());
    for connect in connects.iter() {
        if let Some(rest) = rest_of(&connect.ext, "Connect") {
            charge_kept(rest)?;
        }
        children.push(Node::El(connect_el(connect)));
    }
    element_with_kept("Connects", Vec::new(), children, kept)
}

/// The `Rel` element that names a relationship: `<Rel r:id="rId1"/>`.
pub(super) fn rel_el(id: &str) -> El {
    element("Rel", rel_attrs(id, None), vec![])
}

/// The `Rel` element of a row, a picture or a part, with what it holds beside its relationship id (`kept`).
pub(super) fn rel_el_with(id: &str, kept: Option<&Kept>) -> Result<El, WriteError> {
    element_with_kept("Rel", rel_attrs(id, kept), Vec::new(), kept)
}

/// A `Page` row of the pages index; `rel` is the `r:id` of its relationship to the page part, when it has a part.
pub(super) fn page_row_el(p: &Page, rel: Option<&str>) -> Result<El, WriteError> {
    page_row_el_with(p, rel, false)
}

/// A `Page` row of the pages index; `sheet` says the `PageSheet` is written whatever it holds (every page of a file has one: an empty one is the file's, and the model cannot tell it from none).
pub(super) fn page_row_el_with(p: &Page, rel: Option<&str>, sheet: bool) -> Result<El, WriteError> {
    take_kept(&[&p.passthrough])?;
    let mut typed = Vec::new();
    if sheet || !sheet_is_empty(&p.sheet) {
        typed.push(Node::El(sheet_only_el("PageSheet", &p.sheet)));
    }
    if let Some(id) = rel {
        typed.push(Node::El(rel_el_with(id, rest_of(&p.ext, "Rel"))?));
    }
    let attrs = page_attrs(p);
    let (items, bytes) = tally(&typed);
    take_room(items.saturating_add(1).saturating_add(attrs.len()), bytes.saturating_add(16))?;
    Ok(element("Page", attrs, weave(typed, &[&p.passthrough])))
}

/// A `Master` row of the masters index.
pub(super) fn master_row_el(m: &Master, rel: Option<&str>) -> Result<El, WriteError> {
    master_row_el_with(m, rel, false)
}

/// A `Master` row of the masters index; `sheet` as for [`page_row_el_with`].
pub(super) fn master_row_el_with(m: &Master, rel: Option<&str>, sheet: bool) -> Result<El, WriteError> {
    take_kept(&[&m.passthrough])?;
    let mut typed = Vec::new();
    if sheet || !sheet_is_empty(&m.sheet) {
        typed.push(Node::El(sheet_only_el("PageSheet", &m.sheet)));
    }
    if let Some(icon) = &m.icon {
        typed.push(Node::El(element("Icon", vec![], vec![Node::Text(icon.to_string())])));
    }
    if let Some(id) = rel {
        typed.push(Node::El(rel_el_with(id, rest_of(&m.ext, "Rel"))?));
    }
    let attrs = master_attrs(m);
    let (items, bytes) = tally(&typed);
    take_room(items.saturating_add(1).saturating_add(attrs.len()), bytes.saturating_add(16))?;
    Ok(element("Master", attrs, weave(typed, &[&m.passthrough])))
}

/// A `StyleSheet`.
pub(super) fn style_el(s: &Style) -> Result<El, WriteError> {
    take_kept(&[&s.passthrough, &s.sheet.passthrough])?;
    let typed = sheet_nodes(&s.sheet);
    let attrs = style_attrs(s);
    let (items, bytes) = tally(&typed);
    take_room(items.saturating_add(1).saturating_add(attrs.len()), bytes.saturating_add(16))?;
    let children = weave_kept(typed, &s.passthrough, &s.sheet, rest_of(&s.ext, "KeptOrder"));
    Ok(element("StyleSheet", attrs, children))
}

/// The `Colors` element with what the document's slot holds of it: its own attributes and children (`Colors`), and what each entry holds beside its `IX` and `RGB` (`ColorEntry/<IX>`:
/// an `IX` that is not the text of its number is the entry's own attribute, which wins).
pub(super) fn colors_el_ext(colors: &[ColorEntry], ext: &Ext<OdfDoc>) -> Result<El, WriteError> {
    let mut seen: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut rows = Vec::with_capacity(colors.len());
    for c in colors {
        let nth = seen.entry(c.ix).or_insert(0);
        let rest = rest_nth(ext, &format!("ColorEntry/{}", c.ix), *nth);
        *nth = nth.saturating_add(1);
        if let Some(rest) = rest {
            charge_kept(rest)?;
        }
        let empty = diacraft_doc::RawAttrs::new();
        let raws = [rest.map_or(&empty, |k| &k.raw)];
        let mut b = AttrBuilder::new(&raws);
        b.typed("IX", Some(c.ix.to_string()));
        b.typed("RGB", Some(c.rgb.to_string()));
        let attrs = b.finish();
        let children = rest.map_or_else(Vec::new, |k| weave(Vec::new(), &[&k.kept]));
        rows.push(Node::El(element("ColorEntry", attrs, children)));
    }
    element_with_kept("Colors", Vec::new(), rows, rest_of(ext, "Colors"))
}

/// The `FaceNames` element.
pub(super) fn fonts_el(fonts: &[FaceName]) -> El {
    let rows = fonts.iter().map(|f| Node::El(face_el(f))).collect();
    element("FaceNames", Vec::new(), rows)
}

/// One `FaceName`: its `NameU` and the attributes it keeps, and what is inside it (`FaceName` in the face's own slot).
fn face_el(f: &FaceName) -> El {
    let mut attrs = vec![("NameU".to_owned(), f.name_u.to_string())];
    attrs.extend(f.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())));
    let children = rest_of(&f.ext, "FaceName").map_or_else(Vec::new, |k| weave(Vec::new(), &[&k.kept]));
    element("FaceName", attrs, children)
}

/// The `FaceNames` element with what the document's slot holds of it (`FaceNames`).
pub(super) fn fonts_el_ext(fonts: &[FaceName], ext: &Ext<OdfDoc>) -> Result<El, WriteError> {
    let mut rows = Vec::with_capacity(fonts.len());
    for face in fonts {
        if let Some(rest) = rest_of(&face.ext, "FaceName") {
            charge_kept(rest)?;
        }
        rows.push(Node::El(face_el(face)));
    }
    element_with_kept("FaceNames", Vec::new(), rows, rest_of(ext, "FaceNames"))
}

/// The `StyleSheets` element: the styles, and what the document's slot holds of it (`StyleSheets`).
pub(super) fn styles_el_ext(styles: &[&Style], ext: &Ext<OdfDoc>) -> Result<El, WriteError> {
    let mut rows = Vec::with_capacity(styles.len());
    for style in styles {
        rows.push(Node::El(style_el(style)?));
    }
    element_with_kept("StyleSheets", Vec::new(), rows, rest_of(ext, "StyleSheets"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Passthrough, Placed, RawAttrs, RawElement, RawNode, Row, Section, Shape, ShapeList, Sheet, Value};

    use super::*;

    /// A shape of `rows` rows of a path.
    fn shape_of(rows: usize) -> Shape {
        let mut section = Section::new("Geometry", Some(0));
        for r in 0..rows {
            let mut row = Row { ix: Some(u32::try_from(r + 1).unwrap()), ..Row::default() };
            row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text("1")));
            section.rows.push(Arc::new(row));
        }
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(section));
        let mut shape = Shape::new(1);
        shape.sheet = Arc::new(sheet);
        shape
    }

    #[test]
    fn a_shape_is_made_while_the_part_has_room_and_the_shape_that_passes_the_items_of_a_part_is_refused_in_words() {
        let limits = Limits::default();
        let one = shape_of(100);
        let (items, _) = tally(&[Node::El(shape_el(&one, 0).expect("no part is being made: nothing is counted"))]);
        // A part that has room for exactly one such shape (and the element of the list): the first is made, the second is refused, and not made.
        let _room = PartRoom::enter(limits.opc.xml.max_nodes - items - 1, 0);
        let list = ShapeList::from_vec(vec![Arc::new(shape_of(100))]);
        assert!(shapes_el(&list, 0).is_ok(), "a part with room for one");
        let two = ShapeList::from_vec((0..2).map(|_| Arc::new(shape_of(100))).collect());
        let refused = shapes_el(&two, 0).expect_err("the second shape passes the items of the part");
        assert!(matches!(refused, WriteError::Limit(what) if what.contains("items in one page or master part")), "{refused}");
        assert_eq!(refused.class(), "write_limit");
    }

    #[test]
    fn the_bytes_of_a_part_are_held_to_what_a_read_takes_and_a_part_made_inside_another_leaves_the_room_of_the_first_as_it_was() {
        let limits = Limits::default();
        let outer = PartRoom::enter(0, limits.max_xml_bytes - 10);
        assert!(matches!(shape_el(&shape_of(10), 0), Err(WriteError::Limit(what)) if what.contains("bytes in one page or master part")));
        {
            // Another part, with all of its room: it makes the shape, and when it ends the first part's room is back.
            let _inner = PartRoom::enter(0, 0);
            assert!(shape_el(&shape_of(10), 0).is_ok());
        }
        assert!(shape_el(&shape_of(10), 0).is_err(), "the room of the part that is being made is the first's again");
        drop(outer);
        assert!(shape_el(&shape_of(10), 0).is_ok(), "no part is being made: nothing is counted");
    }

    /// Kept content: a tree of `children` unknown elements of two attributes each (3 items each), as a file's unknown XML is kept.
    fn kept(children: usize) -> Passthrough {
        let leaf = |i: usize| {
            RawNode::Element(RawElement {
                name: Arc::from("x:c"),
                attrs: RawAttrs::from_pairs([("a", "1"), ("b", format!("{i}").as_str())]),
                children: Vec::new(),
            })
        };
        let tree = RawNode::Element(RawElement {
            name: Arc::from("x:k"),
            attrs: RawAttrs::from_pairs([("xmlns:x", "urn:kept")]),
            children: (0..children).map(leaf).collect(),
        });
        Passthrough::from_nodes(vec![Placed { after: 0, node: tree }])
    }

    #[test]
    fn the_kept_content_of_a_shape_is_charged_before_it_is_made_and_a_part_with_room_for_one_is_refused_at_the_second() {
        let limits = Limits::default();
        let mut shape = Shape::new(1);
        shape.passthrough = kept(20_000);
        // What the kept tree is: 1 + 1 attribute, and 20,000 children of 3 items. A part that has room for one shape of it and a little more takes one, and refuses the second.
        let _room = PartRoom::enter(limits.opc.xml.max_nodes - 60_050, 0);
        let one = ShapeList::from_vec(vec![Arc::new(shape.clone())]);
        assert!(shapes_el(&one, 0).is_ok(), "a part with room for one");
        let two = ShapeList::from_vec((0..2).map(|_| Arc::new(shape.clone())).collect());
        let refused = shapes_el(&two, 0).expect_err("the kept content of the second shape passes the items of the part");
        assert!(matches!(refused, WriteError::Limit(what) if what.contains("items in one page or master part")), "{refused}");
        // The same for what a shape keeps on its sheet, and for the rows of the pages and masters indexes, a style and a picture.
        let mut on_sheet = Shape::new(2);
        Arc::make_mut(&mut on_sheet.sheet).passthrough = kept(40_000);
        assert!(shape_el(&on_sheet, 0).is_err(), "kept content of the sheet of a shape");
    }

    #[test]
    fn the_kept_content_of_a_page_row_a_master_row_a_style_and_a_picture_is_charged_too() {
        let limits = Limits::default();
        let _room = PartRoom::enter(limits.opc.xml.max_nodes - 100, 0);
        let mut page = diacraft_doc::Page::new(1, "P");
        page.passthrough = kept(1_000);
        assert!(matches!(page_row_el(&page, None), Err(WriteError::Limit(_))), "a page row");
        let master = diacraft_doc::Master { passthrough: kept(1_000), ..diacraft_doc::Master::default() };
        assert!(matches!(master_row_el(&master, None), Err(WriteError::Limit(_))), "a master row");
        let style = diacraft_doc::Style { passthrough: kept(1_000), ..diacraft_doc::Style::default() };
        assert!(matches!(style_el(&style), Err(WriteError::Limit(_))), "a style");
        let mut picture = Shape::new(3);
        picture.foreign = Some(Arc::new(diacraft_doc::ForeignData { passthrough: kept(1_000), ..diacraft_doc::ForeignData::default() }));
        assert!(matches!(shape_el(&picture, 0), Err(WriteError::Limit(_))), "the picture of a shape");
    }

    #[test]
    fn a_shape_with_kept_content_is_made_the_same_when_a_part_is_being_made_and_when_none_is() {
        let mut shape = shape_of(3);
        shape.passthrough = kept(5);
        let free = shape_el(&shape, 0).expect("no part is being made");
        let _room = PartRoom::enter(0, 0);
        assert_eq!(shape_el(&shape, 0).expect("room enough"), free, "what the writer makes does not depend on the room it is held to");
    }

    #[test]
    fn what_a_group_of_shapes_costs_is_counted_once_for_each_shape_and_not_again_for_each_level() {
        let mut group = Shape::new(1);
        let mut members = Vec::new();
        for _ in 0..50 {
            members.push(Arc::new(shape_of(2)));
        }
        group.children = ShapeList::from_vec(members);
        let made = shape_el(&group, 0).expect("no part");
        let (whole, _) = tally(&[Node::El(made)]);
        // A part with room for exactly what the group is made of takes it, and one item less does not.
        let limits = Limits::default();
        let room = limits.opc.xml.max_nodes;
        let own = {
            let _room = PartRoom::enter(room - whole - 8, 0);
            shape_el(&group, 0).is_ok()
        };
        assert!(own, "the group's own items and its members', counted once each, are within a room of the whole and a few over");
        let none = {
            let _room = PartRoom::enter(room - whole / 2, 0);
            shape_el(&group, 0).is_err()
        };
        assert!(none, "half of what the group is made of is not room for it");
    }
}
