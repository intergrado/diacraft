//! The save's pre-flight (findings 5 and 6): **no file is written that an open refuses.** An open reads a package at [`Limits`] and
//! decodes it within [`Budget::for_open`]; a save that writes what the first or the second refuses writes a drawing its own program cannot open, and the first
//! figures of the open budget did refuse drawings that the encoder had written (two pages of 50,000 shapes). The save makes, before anything is written, the checks an
//! open will make, with the limits and the budget the open will have:
//!
//! * **each part, as written**, is at most `max_part_bytes` ([`written_len`] counts the bytes the container's writer makes of a tree, without making them), has no more
//!   items than the XML reader takes in one tree and no more levels than it reads, and the package (the parts and the blobs) is within `max_package_bytes` and
//!   `max_parts`: a model whose namespace is copied onto a thousand kept elements is 0.9 MB of input and 295 MiB written;
//! * **the budget**: the parts are decoded as an open decodes them, each page and master let go as it is read, within the budget an open has, so that the items, the
//!   model's bytes and the blobs it is charged are the open's, and what it refuses is what the open refuses.
//!
//! **A part at a time** ([`save_parts`]). The save holds the document and one part's tree, as the open does: it makes each page and master, measures it, reads it
//! as an open would, hands it to the caller to write, and lets it go. The document part lists every blob, which the pages and masters number as they use them, so
//! the parts are made twice: the first time to learn the blobs and to measure every part against its caps (a drawing past one is refused before any part is handed
//! over), and the second, after the document part is whole, to check each part against the budget in the order an open reads them (the document part first)
//! and to hand it over. A part is made twice and held once. A caller that already holds every part's tree has [`encode_for_save`] and [`preflight`].
//!
//! A save that passes is a package an open of the same limits reads. What a caller adds to the package (its content types, relationships and manifest) is the
//! caller's own few bytes.
use diacraft_container::Limits;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::par::Lane;
use diacraft_container::xml::{El, Node};
use diacraft_doc::Document;

use super::decode::{Check, check_decodes, part_lists};
use super::encode::{Caps, Encoder, NoSink, encode_capped, held_at_once};
use super::escape::attr_written_total;
use super::stream::{ByteSink, ChildCap};
use super::weigh::{grown, held_bytes_of};
use super::{
    ADDRESS_SPACE_BYTES, BlobSink, BlobSource, Budget, CodecError, EDIT_MODEL_BYTES, Encoded, FORMAT_PART_ITEMS, OPEN_BLOB_BYTES, OPEN_MODEL_BYTES,
    OPEN_PART_BYTES, OPEN_TREE_ITEMS, PartLists, SECTION_STREAM_BYTES,
};

/// The XML declaration the container's writer starts a part with.
const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

fn limit(what: &'static str, max: u64) -> CodecError {
    CodecError::Limit { what, max }
}

/// How many bytes `text` is as the container's writer spells it in text: `&` is `&amp;`, `<` is `&lt;`, `>` is `&gt;`, a carriage return is `&#13;`.
fn text_len(text: &str) -> u64 {
    text.chars()
        .map(|c| match c {
            '&' => 5,
            '<' => 4,
            '>' => 4,
            '\r' => 5,
            other => other.len_utf8() as u64,
        })
        .sum()
}

/// How many bytes `root` is as a part: what [`diacraft_container::xml::to_bytes`] makes of it with its declaration, counted and not made, exactly. A tree the
/// writer would refuse (a name that is no XML name, a character XML forbids) is counted as it would have been written, and is the writer's to refuse. Saturating.
pub fn written_len(root: &El) -> u64 {
    let mut total = DECLARATION.len() as u64;
    let mut add = |n: u64| total = total.saturating_add(n);
    // (element, index of the next child to count)
    let mut stack: Vec<(&El, usize)> = Vec::new();
    let open = |add: &mut dyn FnMut(u64), e: &El| {
        // `<name`, each ` name="value"`, and `/>` or `>`.
        add(1 + e.name.len() as u64);
        for (k, v) in &e.attrs {
            add(1 + k.len() as u64 + 2 + attr_written_total(v) + 1);
        }
        add(if e.children.is_empty() { 2 } else { 1 });
    };
    open(&mut add, root);
    stack.push((root, 0));
    while let Some((el, at)) = stack.pop() {
        let Some(node) = el.children.get(at) else {
            if !el.children.is_empty() {
                add(2 + el.name.len() as u64 + 1);
            }
            continue;
        };
        stack.push((el, at + 1));
        match node {
            Node::El(child) => {
                open(&mut add, child);
                stack.push((child, 0));
            }
            Node::Text(t) => add(text_len(t)),
            Node::CData(c) => {
                if c.contains('\r') {
                    add(text_len(c));
                } else {
                    // `<![CDATA[`, the text with each `]]>` split into `]]]]><![CDATA[>`, and `]]>`.
                    add(9 + c.len() as u64 + 12 * c.matches("]]>").count() as u64 + 3);
                }
            }
            Node::Comment(c) => add(4 + c.len() as u64 + 3),
            Node::Pi { target, data } => add(2 + target.len() as u64 + if data.is_empty() { 0 } else { 1 + data.len() as u64 } + 2),
        }
    }
    total
}

/// The items of a tree as the XML reader counts them (each element, attribute, text, comment and instruction), and its depth, the root being level 1.
pub(super) fn items_and_depth(root: &El) -> (u64, usize) {
    let (mut n, mut deepest) = (0u64, 0usize);
    let mut stack = vec![(root, 1usize)];
    while let Some((e, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        n = n.saturating_add(1).saturating_add(e.attrs.len() as u64);
        for c in &e.children {
            match c {
                Node::El(c) => stack.push((c, depth.saturating_add(1))),
                _ => n = n.saturating_add(1),
            }
        }
    }
    (n, deepest)
}

/// The bytes of the model of `doc` **as it is held**, at the least: every page, master, style, shape, cell, row and section charged what the open charges its
/// element, the maps of cells their nodes, and the characters of the cells' values, units, errors and formulas and of the text's parts, and none of the other text,
/// attributes and kept content that the open charges besides. **An allocation that more than one place holds is counted once** (a shape, a sheet, a section, a row, a text
/// and a string: the model shares what a decode of equal units and a file's `same` give it, and a copy of the document shares everything with the one it was made from).
/// It allocates nothing but the addresses of what is shared, so that a drawing whose model is too large to hold beside the tree of a part is refused **before any
/// tree is made**. A floor is never more than the open will charge, so a drawing it refuses is one the open refuses. This walks all of it; [`held_bytes_until`]
/// stops where a figure is passed, and [`held_bytes_of`] counts several documents between them.
pub fn held_bytes(doc: &Document) -> u64 {
    held_bytes_until(doc, u64::MAX)
}

/// [`held_bytes`], **stopped at the first count past `most`** (and then the count is more than `most` and not the whole): the save asks it of a drawing that might be larger
/// than what it may hold, and does not need to count what is past the figure.
pub fn held_bytes_until(doc: &Document, most: u64) -> u64 {
    held_bytes_of(std::iter::once(doc), most)
}

/// What the edit that made `next` of `before` **added** to the model, at the least and as a figure that never counts a free, by the prices of [`held_bytes`] and with the
/// characters the edit made: the shapes, sheets, sections, rows and strings of `next` that are not the very allocations `before` holds in the same places (a page, a chunk of a
/// list of shapes, a shape, a sheet, a section, a row and a string that an edit did not copy is the same pointer in both), each allocation once. A walk of what an edit copied
/// and not of the drawing, so an edit of one shape of a million costs a chunk of them. Stopped at the first count past `most`.
pub fn grown_bytes_until(before: &Document, next: &Document, most: u64) -> u64 {
    grown(before, next, most).bytes
}

/// What a save holds a drawing's model to, as it is held ([`held_bytes`]): what an edit may leave of one ([`EDIT_MODEL_BYTES`], 1.5 GiB at the limits of an open: the pool
/// of the model is 960 MiB and the figure is 1.6 times it), and in proportion to the pool the limits give. **A drawing is saved when the model it holds is more than
/// an open takes, as long as what is written of it is not**: a reader that shares nothing makes a model of 1.2 GB of units that are equal in pairs, and the open of the
/// file that says them in full shares what is equal ([`Budget::for_open`]), so that what refuses it is the check of the second pass and not this count.
fn held_most(limits: &Limits) -> u64 {
    // In proportion to the pool the limits give: the figure of an edit at the pool of an open ([`EDIT_MODEL_BYTES`] at [`OPEN_MODEL_BYTES`]).
    let scaled = u128::from(Budget::for_open(limits).bytes) * u128::from(EDIT_MODEL_BYTES) / u128::from(OPEN_MODEL_BYTES);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// The items a tree of a part may have **while a model of `held` bytes is held beside it**: what is left of the address space, with a twentieth to spare, once the
/// model, the tables of the check's streams, the strings of the tree (a part's bytes), the blobs and what a process has are held (`SAVE_WORST_BYTES` is this sum at the
/// figure of an open's pool), at [`TREE_ITEM_BYTES`](super::cost::TREE_ITEM_BYTES) an item, and never more than the items of an open. This is what makes the peak of a
/// save a sum that holds by arithmetic: the model it holds and the tree it makes together are no more than the address space less its twentieth, whatever the model.
pub fn save_tree_items(held: u64) -> u64 {
    save_room_items(held).min(OPEN_TREE_ITEMS)
}

/// The most items one child of a part made as a stream may have in a save while `held` bytes of model are held, at the limits of an open: the room of the address space
/// ([`save_room_items`]) and the child limit ([`super::OPEN_ELEMENT_ITEMS`]). A caller that makes the package beside the codec holds its memory back for this tree.
pub fn save_child_items(held: u64) -> u64 {
    save_room_items(held).min(super::OPEN_ELEMENT_ITEMS)
}

/// The items of a tree that the address space has room for in a save while `held` bytes of model are held, whatever a tree of an open is ([`save_tree_items`] is this, held to
/// [`OPEN_TREE_ITEMS`]): what a child of a part made as a stream may have, up to the child limit ([`super::OPEN_ELEMENT_ITEMS`]).
pub fn save_room_items(held: u64) -> u64 {
    let room = (ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20)
        .saturating_sub(held)
        .saturating_sub(SECTION_STREAM_BYTES)
        .saturating_sub(OPEN_PART_BYTES)
        .saturating_sub(OPEN_BLOB_BYTES)
        .saturating_sub(super::cost::BASE_BYTES);
    room / super::cost::TREE_ITEM_BYTES
}

/// Refuses, in words, a drawing whose model, as it is held, is more than a save holds ([`held_most`]): before any part is made. Gives what it holds, which the items of
/// the trees are derived from ([`save_tree_items`]).
fn check_held(doc: &Document, limits: &Limits) -> Result<u64, CodecError> {
    let most = held_most(limits);
    let held = held_bytes_until(doc, most);
    floor_within(held, most)?;
    Ok(held)
}

/// The items a part may have in a save of a drawing that holds `held` bytes of model at `limits`: what the reader takes, and what the save has room for beside the model.
fn part_items_for(limits: &Limits, held: u64) -> u64 {
    u64::try_from(limits.xml.max_nodes).unwrap_or(u64::MAX).min(save_tree_items(held))
}

/// Whether a floor of `floor` bytes is within `most`: as many is enough, and one more is refused.
fn floor_within(floor: u64, most: u64) -> Result<(), CodecError> {
    if floor > most { Err(limit("bytes of the model in memory", most)) } else { Ok(()) }
}

/// Whether `total` items are within `most`: as many is enough, and one more is refused.
fn items_within(total: u64, most: u64) -> Result<(), CodecError> {
    if total > most { Err(limit("items in the drawing", most)) } else { Ok(()) }
}

/// The most bytes an XML part may be as written: what the limits say of an entry, and never more than [`OPEN_PART_BYTES`].
fn part_cap(limits: &Limits) -> u64 {
    limits.max_part_bytes.min(OPEN_PART_BYTES)
}

/// What one part must be for an open at `limits` to read it: within the part cap as written, the items of one tree, and the levels the reader reads. Gives the
/// bytes of the part as written.
fn check_part(part: &El, limits: &Limits) -> Result<u64, CodecError> {
    let bytes = written_len(part);
    // An XML part is held to what the budget is sized for, whatever the limits say of an entry: the bytes of a part are held beside the tree that is made of them.
    let most = part_cap(limits);
    if bytes > most {
        return Err(limit("bytes in one part as written", most));
    }
    let (items, depth) = items_and_depth(part);
    if items > u64::try_from(limits.xml.max_nodes).unwrap_or(u64::MAX) {
        return Err(limit("items in one part", u64::try_from(limits.xml.max_nodes).unwrap_or(u64::MAX)));
    }
    // The reader refuses an element past `max_depth` levels, and never reads past the container's ceiling whatever the limit says.
    let deepest_read = limits.xml.max_depth.min(XML_DEPTH_CEILING);
    if depth > deepest_read {
        return Err(limit("levels of nested elements in one part", u64::try_from(deepest_read).unwrap_or(u64::MAX)));
    }
    Ok(bytes)
}

/// The package: the parts' bytes and the blobs within `max_package_bytes`, and the parts and blobs within `max_parts`.
fn check_package(parts_bytes: u64, parts: usize, blob_sizes: impl Iterator<Item = u64>, blobs: usize, limits: &Limits) -> Result<(), CodecError> {
    let package = blob_sizes.fold(parts_bytes, u64::saturating_add);
    if package > limits.max_package_bytes {
        return Err(limit("bytes in the package as written", limits.max_package_bytes));
    }
    if parts.saturating_add(blobs) > limits.max_parts {
        return Err(limit("parts in the package", u64::try_from(limits.max_parts).unwrap_or(u64::MAX)));
    }
    Ok(())
}

/// What the parts `encoded` would be as a package, and whether an open at `limits` would take them: the check [`encode_for_save`] makes, on parts the caller
/// already has. `blobs` is where the bytes of the blobs the document part names are. Nothing is written; what is refused is [`CodecError::Limit`], in words.
pub fn preflight(encoded: &Encoded, blobs: &dyn BlobSource, limits: &Limits) -> Result<(), CodecError> {
    let lists = part_lists(&encoded.document)?;
    // The parts: each within the part cap and the tree cap of the reader, and with the blobs within the package.
    let mut bytes = 0u64;
    let parts = std::iter::once(&encoded.document).chain(encoded.pages.iter().map(|(_, e)| e)).chain(encoded.masters.iter().map(|(_, e)| e));
    for part in parts {
        bytes = bytes.saturating_add(check_part(part, limits)?);
    }
    let count = 1usize.saturating_add(lists.pages.len()).saturating_add(lists.masters.len());
    check_package(bytes, count, lists.blobs.iter().map(|b| b.size), lists.blobs.len(), limits)?;
    // The budget an open has, spent as an open spends it.
    let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&El> = encoded.masters.iter().map(|(_, e)| e).collect();
    check_decodes(&encoded.document, &pages, &masters, blobs, &mut Budget::for_open(limits), &mut Vec::new())
}

/// The parts of `doc`, as [`encode`] makes them, **and only if an open at `limits` would take them** ([`preflight`]): the save calls this, with the limits of its
/// open ([`open_limits`](super::open_limits)), and writes what it returns. A drawing that the open would refuse is refused here, in words, before a file is
/// written. `blobs` is both where the byte strings go and where the check finds them. This holds every part's tree at once; [`save_parts`] holds one.
pub fn encode_for_save<B: BlobSink + BlobSource>(doc: &Document, blobs: &mut B, limits: &Limits) -> Result<Encoded, CodecError> {
    doc.check_limits()?;
    let held = check_held(doc, limits)?;
    // Every part's tree is held at once, so the parts are held to the items one tree may have between them, whatever the budget's items (the time of a decode) are.
    let items = Budget::for_open(limits).items;
    let part_items = part_items_for(limits, held);
    let mut caps: Caps = held_at_once(part_items, part_cap(limits));
    caps.total_items = caps.total_items.min(save_tree_items(held));
    if items <= caps.total_items {
        (caps.total_items, caps.total_items_error) = (items, ("items in the drawing", items));
    }
    let encoded = encode_capped(doc, blobs, caps)?;
    preflight(&encoded, &*blobs, limits)?;
    Ok(encoded)
}

/// What a part handed to the caller of [`save_parts`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRole {
    /// The document part, with the lists of the other parts and of the blobs. Its name is the caller's own.
    Document,
    /// The part of the page whose `ID` this is.
    Page { id: u32 },
    /// The part of the master whose `ID` this is.
    Master { id: u32 },
}

/// A part of a package that [`save_parts`] hands over: what it is, the name the document part lists it under (empty for the document part itself), and its tree.
#[derive(Clone, Copy, Debug)]
pub struct SavedPart<'a> {
    pub role: PartRole,
    pub name: &'a str,
    pub tree: &'a El,
}

/// The parts of `doc`, **one at a time**, and only if an open at `limits` would take them: each part is made, checked as an open reads it, given to `emit`, and let
/// go, so that the save holds the document, the blobs and one part's tree (the module documentation). The document part comes first, then every page, then every
/// master, the order an open reads them in; `emit` writes the tree (the container's [`to_bytes`](diacraft_container::xml::to_bytes)) wherever the package goes.
///
/// A drawing past a cap of a part or of the package (its bytes as written, its items, its levels, the parts and the blobs it would be) is refused **before the first
/// part is handed over**. One that the budget of an open refuses is refused at the part that takes the budget past its pools, after the parts before it were handed
/// over: **the parts emitted before an error are not a package**, and the caller discards them. `blobs` is both where the byte strings go and where the check finds
/// them; an `emit` that fails ends the save with its error.
pub fn save_parts<B: BlobSink + BlobSource>(
    doc: &Document,
    blobs: &mut B,
    limits: &Limits,
    emit: &mut dyn FnMut(SavedPart<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    doc.check_limits()?;
    let held = check_held(doc, limits)?;
    let max_items = Budget::for_open(limits).items;
    let over = |total: u64| items_within(total, max_items);
    // The first pass: every page and master is made, measured and let go; the blobs they use are numbered; then the document part is whole.
    let mut enc = Encoder::new(blobs);
    enc.check_document(doc)?;
    // One tree is held at a time, and it is made to the items an open reads one tree at (and refused at the item that passes them).
    let part_items = part_items_for(limits, held);
    enc.set_cap(part_items, "XML items in one part", part_items);
    let part_bytes = part_cap(limits);
    enc.set_byte_cap(part_bytes, "bytes in one part as written", part_bytes);
    let (mut document, doc_items) = enc.document_part(doc)?;
    let doc_bytes = enc.part_bytes();
    // The document part's tree is held beside the tree of each page and master, and beside the model: they have the room that is left.
    let part_items = part_items_for(limits, held.saturating_add(doc_items.saturating_mul(super::cost::TREE_ITEM_BYTES)));
    enc.set_cap(part_items, "XML items in one part", part_items);
    let (mut total, mut bytes) = (doc_items, 0u64);
    over(total)?;
    for page in &doc.pages {
        let (tree, items) = enc.page_part(page)?;
        total = total.saturating_add(items);
        over(total)?;
        bytes = bytes.saturating_add(check_part(&tree, limits)?);
    }
    for master in &doc.masters {
        let (tree, items) = enc.master_part(master)?;
        total = total.saturating_add(items);
        over(total)?;
        bytes = bytes.saturating_add(check_part(&tree, limits)?);
    }
    enc.add_blob_list(&mut document, doc_items, doc_bytes)?;
    enc.finish_document(&mut document, doc)?;
    bytes = bytes.saturating_add(check_part(&document, limits)?);
    let sizes: Vec<u64> = enc.records().iter().map(|r| r.size).collect();
    let count = 1usize.saturating_add(doc.pages.len()).saturating_add(doc.masters.len());
    check_package(bytes, count, sizes.iter().copied(), sizes.len(), limits)?;
    let state = enc.into_state();
    // The second pass: the budget an open has, spent in the order an open spends it, each part made again and handed over once it is read.
    let (mut budget, mut notes) = (Budget::for_open(limits), Vec::new());
    let blobs: &B = blobs;
    let mut check = Check::begin(&document, blobs, &mut budget, &mut notes)?;
    let (pages, masters) = (check.lists().pages.clone(), check.lists().masters.clone());
    emit(SavedPart { role: PartRole::Document, name: "", tree: &document })?;
    drop(document);
    let mut sink = NoSink;
    let mut again = Encoder::resume(&mut sink, state);
    again.set_cap(part_items, "XML items in one part", part_items);
    again.set_byte_cap(part_bytes, "bytes in one part as written", part_bytes);
    for (page, (id, name)) in doc.pages.iter().zip(&pages) {
        let (tree, _) = again.page_part(page)?;
        check.page(&tree, *id)?;
        emit(SavedPart { role: PartRole::Page { id: *id }, name, tree: &tree })?;
    }
    for (master, (id, name)) in doc.masters.iter().zip(&masters) {
        let (tree, _) = again.master_part(master)?;
        check.master(&tree, *id)?;
        emit(SavedPart { role: PartRole::Master { id: *id }, name, tree: &tree })?;
    }
    check.finish()
}

/// A part of a package that [`save_parts_bytes`] hands over: what it is, the name the document part lists it under (empty for the document part itself), and its bytes as an
/// XML document (the declaration and the root), which a package stores as they are.
#[derive(Clone, Copy, Debug)]
pub struct SavedBytes<'a> {
    pub role: PartRole,
    pub name: &'a str,
    pub bytes: &'a [u8],
    /// What the document part lists (the page and master parts, and each blob), for the document part and for no other: the save has read them from the part it made, so that a
    /// caller that has to know them does not parse the bytes again.
    pub lists: Option<&'a PartLists>,
}

/// A part that [`save_parts_owned`] hands over: [`SavedBytes`], with the bytes **given to the caller** and not lent, so that a caller may hold them beyond the call (to compress them on
/// another thread while the next part is made) and is the one who lets them go.
#[derive(Debug)]
pub struct SavedOwned<'a> {
    pub role: PartRole,
    pub name: &'a str,
    pub bytes: Vec<u8>,
    pub lists: Option<&'a PartLists>,
}

/// How [`save_parts_owned`] may use threads and memory. **The parts it hands over are the same bytes, in the same order, at any thread count.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveThreads {
    /// The most threads that make the parts: 1 makes none, and every part is made twice and held once (the module documentation).
    pub threads: usize,
    /// The bytes of parts that the save may hold **beyond** what one part takes: half of it for the parts being made together, and half for the parts made in the first pass that are held
    /// until the second needs them, which then does not make them again. 0 holds none, which is the single-threaded save.
    pub keep: u64,
}

impl SaveThreads {
    /// One thread and nothing held.
    pub const ONE: SaveThreads = SaveThreads { threads: 1, keep: 0 };
}

/// The bytes that a part with `shapes` shapes at its top is expected to be, which is what a part made on a thread is charged and held to: a shape of a diagram is some 1.6 KB, and this is
/// two and a half times that. A part that is more is made again by the thread that hands the parts over, as a single thread makes it.
pub(crate) fn expected_bytes(shapes: usize) -> u64 {
    u64::try_from(shapes).unwrap_or(u64::MAX).saturating_mul(4096).max(256 * 1024)
}

/// What the first pass makes of one part: its items, and its bytes or why they are not (so that the order of the refusals is the single-threaded one: the items of a part are counted before its bytes are).
type Made = Result<(u64, Result<Vec<u8>, CodecError>), CodecError>;

/// What a save is made of, for the parts it makes in the first pass.
#[derive(Clone, Copy)]
struct FirstPass<'d> {
    doc: &'d Document,
    caps: (u64, u64),
    part_error: (&'static str, u64),
}

impl FirstPass<'_> {
    /// Part `at` of `doc` (the pages and then the masters) made as a stream by `enc` into a sink of `cap` bytes.
    fn make(&self, enc: &mut Encoder<'_>, at: usize, cap: u64, child: ChildCap) -> Made {
        let mut sink = ByteSink::new(cap, self.part_error);
        let items = match self.doc.pages.get(at) {
            Some(page) => enc.page_stream(page, Some(child), &mut sink)?,
            None => match self.doc.masters.get(at - self.doc.pages.len()) {
                Some(master) => enc.master_stream(master, Some(child), &mut sink)?,
                None => return Err(CodecError::malformed("document", "a part the save has no model for")),
            },
        };
        Ok((items, sink.finish()))
    }

    /// The same, as a task that makes it with an encoder of its own: **it meets no blob it could number** (the numbers are given in the order the parts use them, and a part made out of order
    /// cannot know them), and a part that does, or passes a cap that is the task's share, is an `Err` that the caller makes again in its place.
    fn alone(&self, at: usize, cap: u64, child: ChildCap) -> Made {
        let mut none = NoSink;
        let mut enc = Encoder::new(&mut none);
        enc.set_cap(self.caps.0, "XML items in one part", self.caps.0);
        enc.set_byte_cap(self.caps.1, self.part_error.0, self.part_error.1);
        self.make(&mut enc, at, cap, child)
    }
}

/// What one child of the root of a part may have in a save of `doc` at `limits`: the most the reader takes of one child (a child may be more than a tree: the tree of what it
/// has beyond [`OPEN_TREE_ITEMS`] is held back from the model's pool by the open, and the save's check says what is not afforded, [`Budget::child_items`]) and what the save has
/// room for beside the model it holds ([`save_room_items`]).
fn child_items_for(limits: &Limits, held: u64) -> u64 {
    super::child_items_of(limits).min(save_room_items(held))
}

/// [`save_parts`] for a package that stores **the bytes of each part, with a page and a master made as a stream of the children of its root**: the save holds
/// the document part's tree, the bytes of the part in hand (at most the part cap, and let go once handed over) and **one child's tree**, where [`save_parts`] holds the part's
/// whole tree, which a page of more than [`OPEN_TREE_ITEMS`] items cannot be. What is handed over is the bytes the container's writer makes of the tree that [`save_parts`]
/// hands over, byte for byte, and a package that holds them is read by a source that gives its parts as bytes ([`PartSource::page_bytes`](super::PartSource::page_bytes)).
///
/// A child of the root (a shape and what it holds, a connect, a sheet, what the page keeps) of more items than the reader takes in one tree is refused in words
/// (`XML items in one element of a part`) **while it is made**, and a part past the bytes a part may be (`bytes in one part as written`) at the child that passes them, in both passes,
/// so that a drawing past a cap of a part or of the package is refused before the first part is handed over. The second pass reads each part's bytes as an open reads them, within
/// the budget of an open, so that **a save that passes is a package an open takes**, and one that the budget refuses is refused at the part that takes it past its pools after the parts
/// before it were handed over (**the parts emitted before an error are not a package**).
pub fn save_parts_bytes<B: BlobSink + BlobSource>(
    doc: &Document,
    blobs: &mut B,
    limits: &Limits,
    emit: &mut dyn FnMut(SavedBytes<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    save_parts_owned(doc, blobs, limits, SaveThreads::ONE, &mut |part| {
        emit(SavedBytes { role: part.role, name: part.name, bytes: &part.bytes, lists: part.lists })
    })
}

/// [`save_parts_bytes`] that gives each part's bytes to `emit` (it may hold them), **and makes the parts on more than one thread when `par` allows**.
///
/// **The first pass** makes every page and master to learn the blobs and measure them. With threads the parts are made together, each by a thread with an encoder of its own: a part that
/// uses a blob cannot be numbered out of order, a part that passes a cap (the child's cap is the part's share of it, so that the trees of the parts made together are what one tree is) or
/// the bytes it is expected to be ([`expected_bytes`]) is not made on a thread, and **each of those is made again, in its place in the order, by the encoder that numbers the blobs**, which
/// is what a single thread does for every part. So the numbers of the blobs, the measures and every refusal are the single-threaded save's. A part that fits is **held** (up to
/// [`SaveThreads::keep`]) and the second pass does not make it again. **The second pass** checks each part as an open reads it, in order and on the one thread, since the budget of an
/// open is spent from one part to the next and a check that did not spend it in that order would not refuse where the open does, and hands the part over.
pub fn save_parts_owned<B: BlobSink + BlobSource>(
    doc: &Document,
    blobs: &mut B,
    limits: &Limits,
    par: SaveThreads,
    emit: &mut dyn FnMut(SavedOwned<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    doc.check_limits()?;
    let held = check_held(doc, limits)?;
    let max_items = Budget::for_open(limits).items;
    let over = |total: u64| items_within(total, max_items);
    let part_bytes = part_cap(limits);
    let part_error = ("bytes in one part as written", part_bytes);
    // The first pass: every page and master is made as a stream of its children, measured and let go; the blobs they use are numbered; then the document part is whole.
    let mut enc = Encoder::new(blobs);
    enc.check_document(doc)?;
    // The document part is a tree, held to what the reader takes in one.
    let doc_items_cap = part_items_for(limits, held);
    enc.set_cap(doc_items_cap, "XML items in one part", doc_items_cap);
    enc.set_byte_cap(part_bytes, part_error.0, part_error.1);
    let (mut document, doc_items) = enc.document_part(doc)?;
    let doc_bytes = enc.part_bytes();
    // A child has the room that is left beside the model and the document part's tree, which is held while the pages and masters are made.
    let child_items = child_items_for(limits, held.saturating_add(doc_items.saturating_mul(super::cost::TREE_ITEM_BYTES)));
    // A part, however it is made, is at most the items the format allows (6.1): a page the reader opens as a stream of children is not a file that may pass it.
    let child = ChildCap { items: child_items, what: "XML items in one element of a part", max: child_items, part: FORMAT_PART_ITEMS };
    let first = FirstPass { doc, caps: (doc_items_cap, part_bytes), part_error };
    let (mut total, mut bytes) = (doc_items, 0u64);
    over(total)?;
    let parts = doc.pages.len().saturating_add(doc.masters.len());
    // The parts the first pass held for the second, by place (the pages and then the masters), and how many bytes they are.
    let mut kept: Vec<Option<Vec<u8>>> = Vec::new();
    kept.try_reserve_exact(parts).map_err(|_| limit("parts in the drawing", u64::MAX))?;
    let mut kept_bytes = 0u64;
    {
        // One part of the first pass taken into the totals: counted in the order of the single-threaded pass (the items, then the bytes), and held for the second pass when it fits.
        let mut take = |made: Made, kept: &mut Vec<Option<Vec<u8>>>| -> Result<(), CodecError> {
            let (items, finished) = made?;
            total = total.saturating_add(items);
            over(total)?;
            let part = finished?;
            let len = u64::try_from(part.len()).unwrap_or(u64::MAX);
            bytes = bytes.saturating_add(len);
            if kept_bytes.saturating_add(len) <= par.keep / 2 {
                kept_bytes += len;
                kept.push(Some(part));
            } else {
                kept.push(None);
            }
            Ok(())
        };
        let window = usize::try_from(par.keep / 2).unwrap_or(usize::MAX);
        let together = par.threads > 1 && window > 0 && parts > 1;
        if together {
            // The share of a child's tree that a part made beside others has: the trees of the parts in flight are together what one tree is.
            let share = ChildCap { items: (child_items / u64::try_from(par.threads.min(parts)).unwrap_or(1)).max(1), ..child };
            let expected = |at: usize| -> usize {
                let shapes =
                    doc.pages.get(at).map(|p| p.shapes.len()).or_else(|| doc.masters.get(at - doc.pages.len()).map(|m| m.shapes.len())).unwrap_or(0);
                usize::try_from(expected_bytes(shapes)).unwrap_or(usize::MAX)
            };
            let speculative = |at: usize| expected(at) <= window;
            diacraft_container::par::lane(par.threads, |lane: &Lane<'_, '_, Made>| -> Result<(), CodecError> {
                // The part `next` is the next one to take into the totals: its result from the lane if it was given to a thread, and made here otherwise.
                let mut next = 0usize;
                let mut commit = |lane: &Lane<'_, '_, Made>, at: usize| -> Result<(), CodecError> {
                    let made = match speculative(at).then(|| lane.take()).flatten() {
                        // A part that fits its share, meets no blob and is not refused is the single-threaded part; any other is made as a single thread makes it, here and now.
                        Some(Ok(done @ Ok((_, Ok(_))))) => done,
                        _ => first.make(&mut enc, at, part_bytes, child),
                    };
                    take(made, &mut kept)
                };
                for at in 0..parts {
                    if speculative(at) {
                        let charge = expected(at);
                        while lane.pending() > 0 && lane.charged().saturating_add(charge) > window {
                            commit(lane, next)?;
                            next += 1;
                        }
                        let cap = u64::try_from(charge).unwrap_or(u64::MAX).min(part_bytes);
                        lane.submit(charge, move || first.alone(at, cap, share));
                    }
                }
                while next < parts {
                    commit(lane, next)?;
                    next += 1;
                }
                Ok(())
            })?;
        } else {
            for at in 0..parts {
                take(first.make(&mut enc, at, part_bytes, child), &mut kept)?;
            }
        }
    }
    enc.add_blob_list(&mut document, doc_items, doc_bytes)?;
    enc.finish_document(&mut document, doc)?;
    bytes = bytes.saturating_add(check_part(&document, limits)?);
    let sizes: Vec<u64> = enc.records().iter().map(|r| r.size).collect();
    let count = 1usize.saturating_add(doc.pages.len()).saturating_add(doc.masters.len());
    check_package(bytes, count, sizes.iter().copied(), sizes.len(), limits)?;
    let state = enc.into_state();
    // The second pass: the budget an open has, spent in the order an open spends it, each part made again (or held from the first pass), read as an open reads its bytes, and handed over.
    let (mut budget, mut notes) = (Budget::for_open(limits), Vec::new());
    let blobs: &B = blobs;
    let mut check = Check::begin(&document, blobs, &mut budget, &mut notes)?;
    let (pages, masters) = (check.lists().pages.clone(), check.lists().masters.clone());
    let document_bytes = diacraft_container::xml::to_bytes(&document, &[], &[], true)?;
    drop(document);
    emit(SavedOwned { role: PartRole::Document, name: "", bytes: document_bytes, lists: Some(check.lists()) })?;
    let mut sink = NoSink;
    let mut again = Encoder::resume(&mut sink, state);
    again.set_byte_cap(part_bytes, part_error.0, part_error.1);
    for (at, (page, (id, name))) in doc.pages.iter().zip(&pages).enumerate() {
        let bytes = match kept.get_mut(at).and_then(Option::take) {
            Some(held) => held,
            None => {
                let mut out = ByteSink::new(part_bytes, part_error);
                again.page_stream(page, Some(child), &mut out)?;
                out.finish()?
            }
        };
        check.page_bytes(&bytes, *id)?;
        emit(SavedOwned { role: PartRole::Page { id: *id }, name, bytes, lists: None })?;
    }
    for (at, (master, (id, name))) in doc.masters.iter().zip(&masters).enumerate() {
        let bytes = match kept.get_mut(doc.pages.len().saturating_add(at)).and_then(Option::take) {
            Some(held) => held,
            None => {
                let mut out = ByteSink::new(part_bytes, part_error);
                again.master_stream(master, Some(child), &mut out)?;
                out.finish()?
            }
        };
        check.master_bytes(&bytes, *id)?;
        emit(SavedOwned { role: PartRole::Master { id: *id }, name, bytes, lists: None })?;
    }
    check.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_xml_part_is_held_to_the_least_of_what_the_limits_say_of_an_entry_and_what_the_budget_is_sized_for() {
        let with = |max_part_bytes: u64| Limits { max_part_bytes, ..Limits::default() };
        assert_eq!(part_cap(&with(1_000)), 1_000);
        assert_eq!(part_cap(&with(OPEN_PART_BYTES)), OPEN_PART_BYTES);
        assert_eq!(part_cap(&with(OPEN_PART_BYTES + 1)), OPEN_PART_BYTES);
        assert_eq!(part_cap(&with(u64::MAX)), OPEN_PART_BYTES);
        assert_eq!(part_cap(&Limits::default()), OPEN_PART_BYTES, "the container's 256 MiB is more than a part of XML may be");
    }

    #[test]
    fn the_items_of_a_tree_are_what_the_address_space_has_left_beside_the_model_a_save_holds_and_never_more_than_an_open_reads() {
        use super::super::cost::{BASE_BYTES, TREE_ITEM_BYTES};
        // A drawing that holds nothing may have the items of an open.
        assert_eq!(save_tree_items(0), OPEN_TREE_ITEMS);
        // As the model grows the items come down, by what the model takes, and together they are within the address space less its twentieth.
        let at = |held: u64| (held, save_tree_items(held));
        for (held, items) in [at(super::super::OPEN_MODEL_BYTES), at(1 << 30), at(super::super::EDIT_MODEL_BYTES)] {
            assert!(items >= 1_000_000, "{held}: {items} items");
            let sum = held + items * TREE_ITEM_BYTES + SECTION_STREAM_BYTES + OPEN_PART_BYTES + OPEN_BLOB_BYTES + BASE_BYTES;
            assert!(sum <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20, "{held}: {sum}");
        }
        // The most a drawing is saved at (1.5 GiB) still leaves room for the items of an open; past it, the items come down, by what the model takes.
        assert_eq!(save_tree_items(1 << 30), OPEN_TREE_ITEMS, "a gibibyte of model leaves room for the items of an open");
        assert_eq!(save_tree_items(super::super::EDIT_MODEL_BYTES), OPEN_TREE_ITEMS, "so does the most an edit leaves");
        let more = super::super::EDIT_MODEL_BYTES + (256 << 20);
        assert!(save_tree_items(more) < OPEN_TREE_ITEMS && save_tree_items(more) < save_tree_items(1 << 30), "a larger model, fewer items");
        // And one byte more of model, or past the address space, is never a negative room.
        assert_eq!(save_tree_items(u64::MAX), 0);
        // What a save holds a drawing to is 1.6 times the pool an open has (1.5 GiB of 960 MiB), at the limits of an open.
        assert_eq!(held_most(&Limits { max_package_bytes: super::super::OPEN_PACKAGE_BYTES, ..Limits::default() }), super::super::EDIT_MODEL_BYTES);
        assert_eq!(held_most(&Limits { max_package_bytes: 0, ..Limits::default() }), 0);
    }

    #[test]
    fn the_check_of_what_a_save_holds_gives_what_is_held_and_refuses_what_is_past_what_a_save_holds_to() {
        use diacraft_doc::{Page, Shape, ShapeList};
        let mut doc = Document::empty();
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec((0..5).map(|i| std::sync::Arc::new(Shape::new(i))).collect());
        doc.pages.push(std::sync::Arc::new(page));
        let limits = Limits { max_package_bytes: super::super::OPEN_PACKAGE_BYTES, ..Limits::default() };
        let held = held_bytes(&doc);
        assert!(held > 0);
        assert_eq!(check_held(&doc, &limits), Ok(held), "what is held, from which the items of the trees are derived");
        // Past the figure it is refused, in words, with the figure.
        let tight = Limits { max_package_bytes: 1, ..Limits::default() };
        assert_eq!(check_held(&doc, &tight), Err(CodecError::Limit { what: "bytes of the model in memory", max: held_most(&tight) }));
    }

    #[test]
    fn a_part_may_have_the_items_the_reader_takes_or_what_the_model_leaves_room_for_whichever_is_less() {
        let limits = |max_nodes: usize| {
            let mut l = Limits::default();
            l.xml.max_nodes = max_nodes;
            l
        };
        // A small model leaves room for what the reader takes, and a reader that takes less is the less.
        assert_eq!(part_items_for(&limits(1_000), 0), 1_000);
        assert_eq!(part_items_for(&limits(usize::MAX), 0), OPEN_TREE_ITEMS);
        // A model that leaves less room than the reader takes is the less, and says what the room is.
        let held = super::super::EDIT_MODEL_BYTES + (256 << 20);
        assert_eq!(part_items_for(&limits(usize::MAX), held), save_tree_items(held));
        assert!(part_items_for(&limits(usize::MAX), held) < OPEN_TREE_ITEMS);
        assert_eq!(part_items_for(&limits(10), held), 10);
    }

    #[test]
    fn a_child_may_have_more_than_a_tree_of_an_open_up_to_what_the_address_space_leaves_room_for_beside_the_model() {
        // B1: a child of a part made as a stream is not a tree of an open: it is held to the child limit (4 million, what a part was held to before pages were
        // streams) and to what the address space has room for beside the model the save holds.
        use super::super::OPEN_ELEMENT_ITEMS;
        let limits = |max_nodes: usize| {
            let mut l = Limits::default();
            l.xml.max_nodes = max_nodes;
            l
        };
        const { assert!(OPEN_ELEMENT_ITEMS > OPEN_TREE_ITEMS) };
        // A drawing that holds nothing: the child limit, which is more than a tree.
        assert_eq!(child_items_for(&limits(usize::MAX), 0), OPEN_ELEMENT_ITEMS);
        assert_eq!(
            child_items_for(&Limits { max_package_bytes: super::super::OPEN_PACKAGE_BYTES, ..crate::model::open_limits() }, 0),
            OPEN_ELEMENT_ITEMS
        );
        // A caller that holds a tree to less than an open's holds a child to as much: the limits' own tree.
        assert_eq!(child_items_for(&limits(1_000), 0), 1_000);
        // As the model grows the room comes down, by what the model takes, to what is left beside it (and never past the address space).
        let edit = super::super::EDIT_MODEL_BYTES;
        assert_eq!(child_items_for(&limits(usize::MAX), edit), save_room_items(edit).min(OPEN_ELEMENT_ITEMS));
        assert!(
            child_items_for(&limits(usize::MAX), edit) >= OPEN_TREE_ITEMS,
            "a drawing held at the most an edit leaves still has the room of a tree"
        );
        let more = edit + (512 << 20);
        assert!(child_items_for(&limits(usize::MAX), more) < child_items_for(&limits(usize::MAX), edit), "a larger model, a smaller child");
        assert_eq!(child_items_for(&limits(usize::MAX), u64::MAX), 0);
        // The room is the items of a tree that the address space leaves, whatever a tree of an open is.
        assert_eq!(save_tree_items(0), save_room_items(0).min(OPEN_TREE_ITEMS));
        assert!(save_room_items(0) > OPEN_ELEMENT_ITEMS, "an empty model leaves room for more than the child limit");
    }

    #[test]
    fn a_floor_or_a_total_as_large_as_the_most_is_enough_and_one_more_is_refused_in_words() {
        assert_eq!(floor_within(10, 10), Ok(()));
        assert_eq!(floor_within(0, 0), Ok(()));
        assert_eq!(floor_within(11, 10), Err(CodecError::Limit { what: "bytes of the model in memory", max: 10 }));
        assert_eq!(items_within(10, 10), Ok(()));
        assert_eq!(items_within(0, 0), Ok(()));
        assert_eq!(items_within(11, 10), Err(CodecError::Limit { what: "items in the drawing", max: 10 }));
        assert_eq!(items_within(u64::MAX, u64::MAX), Ok(()));
    }
}
