//! Making the items of a model: the part of the carrier that writes. See the module documentation of `carry`.
//!
//! [`build`] walks the model once and makes an item for the document and for each page, master, style and shape that holds something that rides (`shell.rs`), each with the stamp of
//! the native outputs it sits beside (`stamp.rs`). It makes no part and no relationship: [`Built`] says what the writers (`write/model.rs` and `write/diacraft.rs`) must make, the
//! bytes by their number, and [`Built::name_blobs`] puts the `Id` of each relationship in the document item once the writer has made them.
//!
//! **Cost.** One walk of the model: each shape once, each distinct sheet once (`Sheets`, `SheetStamps`), each part, custom property and face once; the shapes of a page that has
//! items are walked a second time to find a key that two shapes share, counting only the keys that have an item. An item's bytes are counted as it is made
//! (`diacraft_sheetxml::model::written_len`), and the walk ends in words when they pass what a part of the file may hold: **a drawing of a million shapes is never turned into a
//! tree of a million items before it is refused**.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, Format, MAX_SHAPE_DEPTH, Part, RawElement, RawNode, Shape, ShapeList};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::model::{BlobId, BlobSink, CodecError, MemBlobs, encode, encode_master, encode_page, encode_shape, encode_style, written_len};

use super::shell::{self, Sheets};
use super::stamp::{self, SheetStamps};
use super::{Kind, Natively, put_text};
use crate::limits::MAX_SOLUTION_PART_BYTES;
use crate::write::WriteError;

type R<T> = Result<T, WriteError>;

/// The most bytes one part of DiaCraft's may have: the package's cap for a part.
const MAX_PART_BYTES: u64 = diacraft_container::limits::MAX_PART_BYTES;

/// Whether a part of `len` bytes is past the package's cap for one: **one byte over is over**.
pub(crate) fn too_large(len: usize) -> bool {
    u64::try_from(len).unwrap_or(u64::MAX) > MAX_PART_BYTES
}

/// A byte string that rides in a part of its own.
pub(crate) struct Blob {
    pub media_type: String,
    pub bytes: Arc<[u8]>,
}

/// What a model carries: the items, and the byte strings they name.
#[derive(Default)]
pub(crate) struct Built {
    /// The items, in the order: the document's, the pages', the masters', the styles', the shapes' of each page and then of each master.
    pub items: Vec<El>,
    /// The byte strings, by the number the fragments name them by.
    pub blobs: Vec<Blob>,
    /// What was left out, in words.
    pub warnings: Vec<String>,
    /// The parts of the macro project that were left out at the caller's choice.
    pub macros_dropped: Vec<String>,
    /// A macro project of the other format rides (a part of one is among the parts that ride).
    pub macros: bool,
    /// Embedded objects ride: a file of an object (an `Object N` entry or folder, its replacement picture, an embedding) is among the parts that ride.
    pub objects: bool,
    /// The bytes the items will take, as the writer counts them.
    bytes: u64,
    /// The XML items they will be (elements, attributes, texts).
    nodes: u64,
}

/// The XML items a part may hold with room for the settings beside the items: the container's cap (6 million) less what the settings and the wrapper of a part take.
const MAX_PART_XML_ITEMS: u64 = (diacraft_container::limits::MAX_XML_NODES as u64) - 100_000;

/// How many XML items an element is: itself, each attribute, and each node in it, all the way down. The walk keeps its own stack.
fn xml_items(el: &El) -> u64 {
    let mut total = 0u64;
    let mut stack: Vec<&El> = vec![el];
    while let Some(e) = stack.pop() {
        total = total.saturating_add(1).saturating_add(e.attrs.len() as u64);
        for node in &e.children {
            match node {
                Node::El(child) => stack.push(child),
                _ => total = total.saturating_add(1),
            }
        }
    }
    total
}

/// Why a model's data cannot be carried: the codec's refusal, in the writer's words.
fn refused(e: CodecError) -> WriteError {
    match e {
        CodecError::Limit { .. } => WriteError::Limit("the data of the other format that this drawing carries is past a limit of the file format"),
        CodecError::Malformed { .. } => {
            WriteError::Unsupported("the data of the other format that this drawing carries cannot be written into the file")
        }
        CodecError::Xml(error) => WriteError::Xml { part: "the DiaCraft part".to_owned(), error },
    }
}

/// A sink for a fragment that holds no bytes: a shell never holds a picture or a part.
struct NoBlobs;

impl BlobSink for NoBlobs {
    fn put(&mut self, _: &Arc<[u8]>, _: &str) -> Result<BlobId, CodecError> {
        Err(CodecError::Malformed { part: "item".to_owned(), why: "a fragment that holds bytes" })
    }
}

/// An item: its wrapper, with its kind and stamp, and the fragment in it.
fn wrap(kind: Kind, keys: &[(&str, String)], fp: Option<diacraft_doc::stamp::Stamp>, fragment: El, places: &[(u32, u32)]) -> El {
    let mut attrs = vec![("kind".to_owned(), kind.word().to_owned())];
    attrs.extend(keys.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    if let Some(fp) = fp {
        attrs.push(("fp".to_owned(), fp.text()));
    }
    let mut children = vec![Node::El(fragment)];
    for (n, k) in places {
        children.push(Node::El(El {
            name: "at".to_owned(),
            attrs: vec![("n".to_owned(), n.to_string()), ("k".to_owned(), k.to_string())],
            children: Vec::new(),
        }));
    }
    El { name: super::ITEM.to_owned(), attrs, children }
}

/// Whether the part is a macro project of the other format, by its role or by its name.
fn is_macro(part: &Part) -> bool {
    part.format != Format::Vsdx && super::is_macro_part(part)
}

/// The parts of the other formats that ride, judged: a macro project only into a type that allows macros, a name that no package can hold refused, a name that differs from an earlier
/// one only by case, and a part past the cap of a part. → the parts that ride.
fn parts_that_ride(doc: &Document, macros: bool, drop_macros: bool, natively: &Natively, b: &mut Built) -> R<Vec<Arc<Part>>> {
    let mut out: Vec<Arc<Part>> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for part in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        crate::work::carry_walked(1);
        // An object the save wrote as an embedding is the embedding, and rides nowhere else.
        if natively.parts.contains(&Arc::as_ptr(part).addr()) {
            continue;
        }
        if is_macro(part) && !macros {
            if !drop_macros {
                return Err(WriteError::MacroProject);
            }
            b.macros_dropped.push(part.name.to_string());
            continue;
        }
        let ok = match part.format {
            Format::Odf => super::odf_entry_name_ok(&part.name),
            _ => super::native_entry_name_ok(&part.name),
        };
        if !ok {
            return Err(WriteError::Carried {
                part: part.name.to_string(),
                why: "its name is not one a package can hold (it reaches outside the package, is empty or is one the package makes itself)",
            });
        }
        if too_large(part.bytes.len()) {
            return Err(WriteError::Carried {
                part: part.name.to_string(),
                why: "it is larger than the 256 MiB a part of a file may be; save the drawing as a .diacraft file to keep it",
            });
        }
        if !seen.insert(part.name.to_ascii_lowercase()) {
            b.warnings.push("a part of the other format was left out: another part has its name, which differs from it at most in case".to_owned());
            continue;
        }
        b.macros |= is_macro(part);
        b.objects |= super::is_object_part(part);
        out.push(Arc::clone(part));
    }
    if !b.macros_dropped.is_empty() {
        let n = b.macros_dropped.len();
        b.warnings.push(format!(
            "the macro project of the other format was left out of the new file, because a file of the type chosen cannot hold one: {n} {} removed",
            if n == 1 { "part" } else { "parts" }
        ));
    }
    Ok(out)
}

impl Built {
    fn add(&mut self, item: El) -> R<()> {
        self.bytes = self.bytes.saturating_add(written_len(&item));
        // The part is read by a parser that holds a tree to the items the container allows one: an element, an attribute, a text. A part past them would not read back.
        self.nodes = self.nodes.saturating_add(xml_items(&item));
        if self.nodes > MAX_PART_XML_ITEMS {
            return Err(WriteError::Limit(
                "the data of the other format that this drawing carries has more XML items than a part of a file may hold; save the drawing as a .diacraft file to keep it",
            ));
        }
        // The data of the file is held to what a read of it holds, with the room of the settings beside it.
        if self.bytes > MAX_SOLUTION_PART_BYTES.saturating_sub(MAX_SOLUTION_PART_BYTES / 16) {
            return Err(WriteError::Limit(
                "the data of the other format that this drawing carries is larger than the 64 MiB DiaCraft reads from a VSDX file; save the drawing as a .diacraft file to keep it",
            ));
        }
        self.items.push(item);
        Ok(())
    }

    /// Puts the `Id` of the relationship to the part of each blob in the document item's list: `rels[n]` is the `Id` of the relationship to blob `n`. The list is the codec's own (a `blob`
    /// element for each, with its number, media type, size and stamp), and a reader finds the part by the `path` it holds.
    pub(crate) fn name_blobs(&mut self, rels: &[String]) {
        for fragment in self.items.iter_mut().filter(|i| matches!(i.attr("kind"), Some("document" | "parts"))).flat_map(|item| {
            item.children.iter_mut().filter_map(|n| match n {
                Node::El(e) if e.is("document") => Some(e),
                _ => None,
            })
        }) {
            for record in fragment.children.iter_mut().filter_map(|n| match n {
                Node::El(e) if e.is("blob") => Some(e),
                _ => None,
            }) {
                let id: usize = record.attr("id").and_then(|v| v.parse().ok()).unwrap_or(0);
                let Some(rel) = rels.get(id) else { continue };
                match record.attrs.iter_mut().find(|(k, _)| k == "path") {
                    Some(slot) => slot.1 = rel.clone(),
                    None => record.attrs.push(("path".to_owned(), rel.clone())),
                }
            }
        }
    }
}

/// The longest name a style is keyed by; a style with a longer one is keyed by its `ID`.
const MAX_KEY_NAME_BYTES: usize = 4096;

/// The key of a style in an item: **its name, when no other style of the drawing has it** (the design's 5.1), else its `id` when no other style has that (two styles of a
/// file may share a `Name`, and the `ID` is what the file keys a style by). `None` when both are another's too: the item would name no style, and the style's data is left out.
fn style_keys(style: &diacraft_doc::Style, names: &HashMap<&str, u32>, ids: &HashMap<u32, u32>) -> Option<Vec<(&'static str, String)>> {
    let alone = |n: Option<u32>| n.unwrap_or(0) == 1;
    match style.name.as_deref() {
        Some(name) if name.len() <= MAX_KEY_NAME_BYTES && alone(names.get(name).copied()) => {
            let mut holder = El { name: String::new(), attrs: Vec::new(), children: Vec::new() };
            put_text(&mut holder, "name", name);
            Some(holder.attrs.into_iter().map(|(k, v)| (if k == "name" { "name" } else { "name-b64" }, v)).collect())
        }
        _ if alone(ids.get(&style.id).copied()) => Some(vec![("id", style.id.to_string())]),
        _ => None,
    }
}

/// How many shapes of a list hold something that rides, members too: the shapes of a page or master whose `ID` another has, whose data is left out with theirs. One walk with a stack of
/// its own, to the model's depth.
fn shapes_with_data(list: &ShapeList, sheets: &mut Sheets, natively: &Natively) -> usize {
    let mut count = 0usize;
    let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 1)];
    while let Some((shapes, depth)) = stack.pop() {
        for shape in shapes.iter() {
            crate::work::carry_walked(1);
            if shell::shape_shell(shape, sheets, natively.shapes.contains(&std::ptr::from_ref::<Shape>(shape).addr())).is_some() {
                count = count.saturating_add(1);
            }
            if !shape.children.is_empty() && depth < MAX_SHAPE_DEPTH {
                stack.push((&shape.children, depth.saturating_add(1)));
            }
        }
    }
    count
}

/// A shape that holds something that rides: the shape, the places of its text's parts that ride, and its shell.
type Found<'a> = (&'a Shape, Vec<(u32, u32)>, Shape);

/// The items of the shapes of one page or master: each shape that holds something that rides, keyed by its `ID`. A shape whose `ID` another shape of the part has is left out and said.
fn shape_items(list: &ShapeList, kind: Kind, of: u32, sheets: &mut Sheets, stamps: &mut SheetStamps, natively: &Natively, b: &mut Built) -> R<()> {
    // One walk with a stack of its own: the depth is the model's own cap, and a model built in memory may pass it.
    let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 0)];
    let mut found: Vec<Found<'_>> = Vec::new();
    let mut deeper = false;
    while let Some((top, at)) = stack.last_mut() {
        let Some(shape) = top.get(*at) else {
            stack.pop();
            continue;
        };
        *at += 1;
        crate::work::carry_walked(1);
        if let Some((shell, places)) = shell::shape_shell(shape, sheets, natively.shapes.contains(&std::ptr::from_ref::<Shape>(shape).addr())) {
            found.push((shape, places, shell));
        }
        if !shape.children.is_empty() {
            if stack.len() >= MAX_SHAPE_DEPTH {
                deeper = true;
            } else {
                stack.push((&shape.children, 0));
            }
        }
    }
    if deeper {
        b.warnings.push("shapes nested deeper than a drawing holds them were not carried".to_owned());
    }
    if found.is_empty() {
        return Ok(());
    }
    // A key two shapes share names neither: the items are for the shapes whose `ID` is theirs alone in the part.
    let wanted: HashSet<u32> = found.iter().map(|(s, _, _)| s.id).collect();
    let mut uses: HashMap<u32, u32> = HashMap::with_capacity(wanted.len());
    let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 0)];
    while let Some((top, at)) = stack.last_mut() {
        let Some(shape) = top.get(*at) else {
            stack.pop();
            continue;
        };
        *at += 1;
        crate::work::carry_walked(1);
        if wanted.contains(&shape.id) {
            *uses.entry(shape.id).or_insert(0) += 1;
        }
        if !shape.children.is_empty() && stack.len() < MAX_SHAPE_DEPTH {
            stack.push((&shape.children, 0));
        }
    }
    let mut shared = 0usize;
    for (shape, places, shell) in found {
        if uses.get(&shape.id).copied().unwrap_or(0) > 1 {
            shared += 1;
            continue;
        }
        let fragment = encode_shape(&shell, &mut NoBlobs).map_err(refused)?;
        let fp = stamp::shape_stamp(shape, stamps);
        let item = wrap(kind, &[("of", of.to_string()), ("id", shape.id.to_string())], Some(fp), fragment, &places);
        b.add(item)?;
    }
    if shared > 0 {
        b.warnings.push(format!("the data of {shared} shapes was left out: another shape of the same page or master has the same ID"));
    }
    Ok(())
}

/// Everything the model carries. `macros` says the type written allows macro projects, and `drop_macros` that the caller chose to leave them out where it does not.
pub(crate) fn build(doc: &Document, macros: bool, drop_macros: bool, natively: &Natively) -> R<Built> {
    let mut b = Built::default();
    let mut sheets = Sheets::default();
    let mut stamps = SheetStamps::default();
    let parts = parts_that_ride(doc, macros, drop_macros, natively, &mut b)?;
    if parts.len() > super::MAX_CARRIED_PARTS {
        return Err(WriteError::Limit(
            "the drawing carries more parts of the other format than one file can name (30,000); save it as a .diacraft file to keep them",
        ));
    }
    // The document, and the parts and custom properties of the other formats, which are their own item: they have no stamp. The two share the numbering of the blobs.
    let mut blobs = MemBlobs::new();
    if let Some(shell) = shell::document_shell(doc, &mut sheets) {
        let encoded = encode(&shell, &mut blobs).map_err(refused)?;
        let item = wrap(Kind::Document, &[], Some(stamp::document_stamp(doc)), encoded.document, &[]);
        b.add(item)?;
    }
    if let Some(shell) = shell::parts_shell(doc, &parts) {
        let encoded = encode(&shell, &mut blobs).map_err(refused)?;
        let item = wrap(Kind::Parts, &[], None, encoded.document, &[]);
        b.add(item)?;
    }
    // The threads of the comments: the replies, which VSDX's comments part has no place for.
    if doc.comments.iter().any(|c| c.parent.is_some()) {
        let mut fragment = El { name: Kind::Comments.fragment().to_owned(), attrs: Vec::new(), children: Vec::new() };
        for c in doc.comments.iter() {
            crate::work::carry_walked(1);
            if let Some(parent) = c.parent {
                fragment.children.push(Node::El(El {
                    name: "link".to_owned(),
                    attrs: vec![("id".to_owned(), c.id.to_string()), ("parent".to_owned(), parent.to_string())],
                    children: Vec::new(),
                }));
            }
        }
        let item = wrap(Kind::Comments, &[], Some(stamp::comments_stamp(&doc.comments)), fragment, &[]);
        b.add(item)?;
    }
    if blobs.len() > super::MAX_CARRIED_PARTS {
        return Err(WriteError::Limit(
            "the drawing carries more byte strings of the other format than one file can name (30,000); save it as a .diacraft file to keep them",
        ));
    }
    for n in 0..blobs.len() {
        let id = BlobId(u32::try_from(n).unwrap_or(u32::MAX));
        if let Some((bytes, media)) = blobs.entry(id) {
            b.blobs.push(Blob { media_type: media.to_owned(), bytes: Arc::clone(bytes) });
        }
    }
    // The pages and the masters, each keyed by its `ID`; an `ID` the document has twice names neither. **What is left out, and said, is only the data such a page or master and its
    // shapes hold**: two that share an `ID` and hold nothing that rides lose nothing, and a save that said they did was a false word.
    let mut page_uses: HashMap<u32, u32> = HashMap::new();
    for page in doc.pages.iter() {
        *page_uses.entry(page.id).or_insert(0) += 1;
    }
    let mut master_uses: HashMap<u32, u32> = HashMap::new();
    for master in doc.masters.iter() {
        *master_uses.entry(master.id).or_insert(0) += 1;
    }
    let mut skipped = 0usize;
    let mut shapes_skipped = 0usize;
    for page in doc.pages.iter() {
        let shell = shell::page_shell(page, &mut sheets);
        if page_uses.get(&page.id).copied().unwrap_or(0) > 1 {
            skipped += usize::from(shell.is_some());
            shapes_skipped = shapes_skipped.saturating_add(shapes_with_data(&page.shapes, &mut sheets, natively));
            continue;
        }
        if let Some(shell) = shell {
            let fragment = encode_page(&shell, &mut NoBlobs).map_err(refused)?;
            let item = wrap(Kind::Page, &[("id", page.id.to_string())], Some(stamp::page_stamp(page, &mut stamps)), fragment, &[]);
            b.add(item)?;
        }
    }
    for master in doc.masters.iter() {
        let shell = shell::master_shell(master, &mut sheets);
        if master_uses.get(&master.id).copied().unwrap_or(0) > 1 {
            skipped += usize::from(shell.is_some());
            shapes_skipped = shapes_skipped.saturating_add(shapes_with_data(&master.shapes, &mut sheets, natively));
            continue;
        }
        if let Some(shell) = shell {
            let fragment = encode_master(&shell, &mut NoBlobs).map_err(refused)?;
            let item = wrap(Kind::Master, &[("id", master.id.to_string())], Some(stamp::master_stamp(master, &mut stamps)), fragment, &[]);
            b.add(item)?;
        }
    }
    // The styles, keyed by name, or by `ID` where the name is not the style's alone (`style_keys`); a style whose name and `ID` another has too names nothing, and only one that holds data
    // is left out and said.
    let mut name_uses: HashMap<&str, u32> = HashMap::new();
    let mut id_uses: HashMap<u32, u32> = HashMap::new();
    for style in doc.styles.iter() {
        if let Some(name) = style.name.as_deref() {
            *name_uses.entry(name).or_insert(0) += 1;
        }
        *id_uses.entry(style.id).or_insert(0) += 1;
    }
    for style in doc.styles.iter() {
        let Some(shell) = shell::style_shell(style, &mut sheets) else { continue };
        let Some(keys) = style_keys(style, &name_uses, &id_uses) else {
            skipped += 1;
            continue;
        };
        let fragment = encode_style(&shell, &mut NoBlobs).map_err(refused)?;
        let item = wrap(Kind::Style, &keys, Some(stamp::style_stamp(style, &mut stamps)), fragment, &[]);
        b.add(item)?;
    }
    // The shapes.
    for page in doc.pages.iter().filter(|p| page_uses.get(&p.id).copied().unwrap_or(0) == 1) {
        shape_items(&page.shapes, Kind::Shape, page.id, &mut sheets, &mut stamps, natively, &mut b)?;
    }
    for master in doc.masters.iter().filter(|m| master_uses.get(&m.id).copied().unwrap_or(0) == 1) {
        shape_items(&master.shapes, Kind::MasterShape, master.id, &mut sheets, &mut stamps, natively, &mut b)?;
    }
    if skipped > 0 {
        b.warnings.push(format!("the data of {skipped} pages, masters or styles was left out: another has the same ID or name"));
    }
    if shapes_skipped > 0 {
        b.warnings.push(format!("the data of {shapes_skipped} shapes was left out: another page or master has the ID of theirs"));
    }
    Ok(b)
}

/// Whether two builds carry the same: the same items, and the same bytes in the same parts.
pub(crate) fn same(a: &Built, b: &Built) -> bool {
    a.items == b.items
        && a.blobs.len() == b.blobs.len()
        && a.blobs.iter().zip(&b.blobs).all(|(x, y)| x.media_type == y.media_type && (Arc::ptr_eq(&x.bytes, &y.bytes) || x.bytes == y.bytes))
}

/// The `version` DiaCraft's data states: the attribute called `version` in the prefix of the element's own name.
pub(crate) fn data_version(data: &RawElement) -> Option<&str> {
    let prefix = data.name.split_once(':').map(|(p, _)| p);
    data.attrs.iter().find(|(name, _)| name.split_once(':').map(|(p, _)| p) == prefix && name.rsplit(':').next() == Some("version")).map(|(_, v)| v)
}

/// The `Data` element to write: DiaCraft's own settings as the model holds them (`base`) and the items after them, each named in the prefix of the data. `None` when the model has
/// neither. **The items are written only into data of the version this build writes**; data of another version is kept as it is and `false` says the items were not written.
pub(crate) fn data_element(base: Option<&RawElement>, built: &Built) -> R<(Option<El>, bool)> {
    let items = !built.items.is_empty();
    let writable = base.is_none_or(|b| data_version(b) == Some(diacraft_doc::DIACRAFT_VERSION));
    let mut data = match base {
        Some(base) => match diacraft_sheetxml::passthrough::node_to_tree(&RawNode::Element(base.clone())) {
            Node::El(el) => el,
            _ => return Err(WriteError::Mismatch("the DiaCraft data is not an element")),
        },
        None if items => {
            let mut fresh = El { name: "dc:Data".to_owned(), attrs: Vec::new(), children: Vec::new() };
            fresh.attrs.push(("xmlns:dc".to_owned(), diacraft_doc::DIACRAFT_URN.to_owned()));
            fresh.attrs.push(("dc:version".to_owned(), diacraft_doc::DIACRAFT_VERSION.to_owned()));
            fresh
        }
        None => return Ok((None, false)),
    };
    if items && writable {
        let prefix = data.prefix().to_owned();
        for item in &built.items {
            let mut item = item.clone();
            item.name = super::item_name(&prefix);
            for child in item.children.iter_mut() {
                if let Node::El(e) = child
                    && e.name == "at"
                {
                    e.name = if prefix.is_empty() { "at".to_owned() } else { format!("{prefix}:at") };
                }
            }
            data.children.push(Node::El(item));
        }
    }
    Ok((Some(data), items && writable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_is_too_large_from_one_byte_over_the_cap_of_a_part() {
        let cap = usize::try_from(MAX_PART_BYTES).unwrap();
        assert_eq!(cap, 256 << 20);
        assert!(!too_large(0) && !too_large(cap - 1) && !too_large(cap));
        assert!(too_large(cap + 1) && too_large(usize::MAX));
    }

    #[test]
    fn the_xml_items_of_an_element_are_itself_its_attributes_and_each_node_in_it() {
        let leaf = |name: &str| El { name: name.to_owned(), attrs: vec![("a".to_owned(), "1".to_owned())], children: Vec::new() };
        let el = El {
            name: "x".to_owned(),
            attrs: vec![("a".to_owned(), "1".to_owned()), ("b".to_owned(), "2".to_owned())],
            children: vec![
                Node::El(leaf("y")),
                Node::Text("t".to_owned()),
                Node::Comment("c".to_owned()),
                Node::El(El { name: "z".to_owned(), attrs: Vec::new(), children: vec![Node::El(leaf("w"))] }),
            ],
        };
        // x and its 2 attributes; y and its 1; a text; a comment; z; w and its 1.
        assert_eq!(xml_items(&el), 3 + 2 + 1 + 1 + 1 + 2);
        assert_eq!(xml_items(&El { name: "e".to_owned(), attrs: Vec::new(), children: Vec::new() }), 1);
    }

    #[test]
    fn two_builds_are_the_same_only_when_their_items_and_their_bytes_are() {
        let blob = |bytes: &[u8]| Blob { media_type: "x/y".to_owned(), bytes: Arc::from(bytes) };
        let item = |kind: &str| El { name: "item".to_owned(), attrs: vec![("kind".to_owned(), kind.to_owned())], children: Vec::new() };
        let a = Built { items: vec![item("page")], blobs: vec![blob(b"1")], ..Built::default() };
        let same = Built { items: vec![item("page")], blobs: vec![blob(b"1")], ..Built::default() };
        assert!(same_build(&a, &same));
        assert!(!same_build(&a, &Built { items: vec![item("style")], blobs: vec![blob(b"1")], ..Built::default() }), "an item");
        assert!(!same_build(&a, &Built { items: vec![item("page")], blobs: vec![blob(b"2")], ..Built::default() }), "bytes");
        assert!(!same_build(&a, &Built { items: vec![item("page")], blobs: Vec::new(), ..Built::default() }), "a part less");
        let mut typed = Built { items: vec![item("page")], blobs: vec![blob(b"1")], ..Built::default() };
        typed.blobs[0].media_type = "x/z".to_owned();
        assert!(!same_build(&a, &typed), "the type");
    }

    fn same_build(a: &Built, b: &Built) -> bool {
        super::same(a, b)
    }
}
