//! **ODF objects carried back become VSDX embeddings** (the native format's design 4.5 and Owner 9).
//!
//! A drawing that came from an `.odg` holds an object frame as a `Foreign` shape that shows the frame's replacement picture, with the frame's own `draw:object-ole` element in the shape's kept markup
//! (`ext.odf`) and the object's file as a part of the model (`Document::parts`, the format `Odf`, found by the element's `xlink:href`). **A frame is *carried back*** when all of these hold,
//! and is then written as a real embedding:
//!
//! * its object is an entry of the package that the model holds as a part (or inline base64, `office:binary-data`, as a flat file has it: E7, decoded once, charged before it is made);
//! * the bytes are a compound file (a check of a fixed number of steps that reads no more than the header and one directory entry);
//! * its replacement picture, the one the frame shows, is in one of the six formats a VSDX file draws (BMP, EMF, GIF, JPEG, PNG, TIFF) by its first bytes.
//!
//! **Nothing is read of the object beyond the check**: no XML, no stream, no class (`draw:class-id` is not read: the compound file's own class is the object's, and VSDX has no other place for
//! one). **Every other frame rides**: a `draw:object` (it has no OLE form), an object or a picture the package does not hold, a replacement in another format, bytes that are not
//! a compound file, a folder of entries, an entry past the cap of a part, and a part whose name says it is a macro project or is no object's. The shape of such a frame shows its replacement
//! natively, as every picture does, and the object's files ride as bytes in parts of DiaCraft's own.
//!
//! **What is written for a frame**: the object part `embeddings/oleObjectN.bin` under the root of the package (N the lowest number above every `oleObject` part in use, found once,
//! walked up once), typed by a `Default` for `bin` of the object type when no `Default` for `bin` says another and by an `Override` when one does; its relationship part with the one `image`
//! relationship to the replacement's part (**the fallback hangs off the object part, never off the page**); a relationship of the type `oleObject` from the page part; and the shape's `ForeignData`
//! with `ForeignType` `Object`, then the attributes that rode with the frame (every attribute but `ForeignType`, in their order) or, when none did, the defaults: `ObjectType` 33280, `ShowAsIcon`
//! 0, and `ObjectWidth` and `ObjectHeight` as the shape's `Width` and `Height` are written. The picture is the picture writer's (`image` part, named and typed as it names and types any picture), and
//! an object's picture is **typed by its bytes** when the type the model claims for it is none a reader draws, since the reader finds an object's fallback by its type.
//!
//! **Two frames that name one object and show one picture share one embedding** (one part, one relationship from each page part, as a copy of a page shares its parts); a frame whose picture is
//! another is another embedding with the same bytes, **charged to the budget of the save before it is made** ([`MAX_NATIVE_BYTES`]): a frame that would pass it rides instead, so that a hostile
//! file with a thousand frames of one large object cannot make a thousand copies. The bytes of a part-held object are the model's and are written from it, never copied ([`Body::Shared`](super)).
//!
//! **The carrier** (`carry.rs`) is told which parts and which shapes went natively ([`Natively`](crate::carry::Natively)): a part written as an embedding is left out of the parts that ride, unless a
//! frame that was not carried back names it too, and the frame's `draw:object-ole` element is left out of its shape's item, so that the model read from the file holds the embedding and no
//! markup that names an entry that is no longer there.
//!
//! **Costs and caps.** Every shape is looked at once and every node of its kept list at most once (`work::object_frames`); the parts of the model are put in a map once, when the first frame needs
//! it; an object's check reads at most 640 bytes of it (`work::object_bytes_checked`: at most 640 bytes of an object, whatever its size); the bytes written are the model's (`work::object_bytes_copied` counts only an inline object's decode).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::limits::{MAX_PACKAGE_BYTES, MAX_PART_BYTES};
use diacraft_doc::{
    Document, ForeignData, Format, MAX_SHAPE_DEPTH, Media, Part, PartRole, RawAttrs, RawElement, RawNode, Shape, ShapeList, ShapeType,
};
use diacraft_opc::name::lenient_decode;

use super::held::{Body, Book, Entry, IMAGE, plausible};
use super::model::picture_by_bytes;
use super::page_rows::{Lead, ObjectLead};
use super::parts::{OD, folder_of, relative_target};
use super::{NameWalk, Plan, R, WriteError};
use crate::carry::Natively;

/// The content type of an object part: what a `Default` for `bin` gives.
pub(super) const OLE_TYPE: &str = "application/vnd.openxmlformats-officedocument.oleObject";

/// The most bytes of one save's embeddings, counted once for each distinct embedding: the package's own cap on what it holds inflated. A frame that would pass it rides.
pub(crate) const MAX_NATIVE_BYTES: u64 = MAX_PACKAGE_BYTES;

/// The most bytes of inline objects a save decodes, in all: a part of an `.odg` that holds them is held to 64 MiB of XML, and an inline object is a third more characters than bytes.
pub(crate) const MAX_INLINE_BYTES: u64 = 64 * 1024 * 1024;

/// The name of the element a frame keeps for its object.
const OBJECT_ELEMENT: &str = "draw:object-ole";

/// `ObjectType` of an embedded OLE 2 object.
const OBJECT_TYPE: &str = "33280";

// ------------------------------------------------------------------------------------------------------------------------------ R9

/// Whether `bytes` are a compound file, by the five steps and no more: at least 512 bytes that begin with the signature; version 3 with sector shift 9, or version 4 with 12 (and the byte order
/// mark `0xFFFE`); a first directory sector below `0xFFFF_FFFA`; the entry that sector starts, at (sector + 1) times the sector size, inside the bytes; and a root entry (type 5) there. The check
/// reads the header and that one entry, and never the FAT or another entry: a broken stream deeper in the file is the source's matter, carried as found.
pub(crate) fn is_compound(bytes: &[u8]) -> bool {
    const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    crate::work::object_bytes_read(bytes.len().min(512));
    if bytes.len() < 512 || bytes.get(..8) != Some(&SIGNATURE[..]) {
        return false;
    }
    let u16_at = |at: usize| -> Option<u16> { Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at.checked_add(1)?)?])) };
    let u32_at = |at: usize| -> Option<u32> {
        let end = at.checked_add(4)?;
        let four: [u8; 4] = bytes.get(at..end)?.try_into().ok()?;
        Some(u32::from_le_bytes(four))
    };
    let (Some(version), Some(order), Some(shift), Some(first)) = (u16_at(0x1A), u16_at(0x1C), u16_at(0x1E), u32_at(0x30)) else { return false };
    if order != 0xFFFE || !matches!((version, shift), (3, 9) | (4, 12)) || first >= 0xFFFF_FFFA {
        return false;
    }
    let sector: usize = 1usize << shift;
    let Some(at) = usize::try_from(first).ok().and_then(|f| f.checked_add(1)).and_then(|n| n.checked_mul(sector)) else { return false };
    let Some(end) = at.checked_add(128) else { return false };
    crate::work::object_bytes_read(128);
    end <= bytes.len() && at.checked_add(0x42).and_then(|i| bytes.get(i)) == Some(&5)
}

/// The number in the name of an object part: `oleObject<digits>.bin` as the last segment of `name`, compared ASCII case-insensitively, whatever the folder.
pub(crate) fn object_number(name: &str) -> Option<u32> {
    let last = name.rsplit('/').next()?;
    let lower = last.to_ascii_lowercase();
    let digits = lower.strip_prefix("oleobject")?.strip_suffix(".bin")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The greatest number in use in the names given, `0` when there is none: where the walk up the numbers starts, so that a new object is numbered above every one in use.
pub(super) fn greatest_number<'n>(names: impl IntoIterator<Item = &'n str>) -> u32 {
    names.into_iter().filter_map(object_number).max().unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------------------------------------- the judge

/// An object that is written as an embedding: its bytes and the picture its object part's own relationship leads to.
pub(crate) struct Object {
    pub bytes: Arc<[u8]>,
    pub media: Arc<Media>,
}

/// A frame that is carried back: which embedding it is, and the attributes of its `ForeignData`.
pub(crate) struct Back {
    pub object: usize,
    pub attrs: RawAttrs,
    /// The part of the model it names, by allocation: none for an inline object.
    part: Option<usize>,
}

/// What names an object: the entry of the package, or the inline data of one frame.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Identity {
    Entry(usize),
    Inline(usize),
}

/// The frames of a model that are carried back, and the embeddings they make.
#[derive(Default)]
pub(crate) struct Embeddings {
    /// The objects to write, in the order the frames that need them come: pages in order, shapes in order, a group's members after it.
    pub objects: Vec<Object>,
    /// Each frame carried back, by the allocation of its shape.
    frames: HashMap<usize, Back>,
    /// The parts a frame that is not carried back names (by allocation): they ride.
    riding: HashSet<usize>,
    /// Which embedding an object and a picture make, by the allocations of both.
    made: HashMap<(Identity, usize), usize>,
    /// Whether each object is a compound file, by R9: an object that many frames name is checked once.
    verdicts: HashMap<Identity, bool>,
    /// The bytes of the embeddings, once for each, and the most the save has room for.
    bytes: u64,
    native: u64,
    /// The bytes decoded from inline objects, and the most the save has room for.
    inline: u64,
    inline_max: u64,
}

/// Whether a part may be written as an object: an entry of the package that is a file, with a name an `.odg` package could hold, that is no macro project, no signature, no thumbnail, no font and
/// no picture of its own, and within the cap of a part.
fn eligible(part: &Part) -> bool {
    part.format == Format::Odf
        && !part.name.ends_with('/')
        && crate::carry::odf_entry_name_ok(&part.name)
        && !crate::carry::is_macro_part(part)
        && matches!(part.role, PartRole::Other | PartRole::ObjectFile | PartRole::Embedding)
        && !too_large(part.bytes.len())
}

/// Whether a part of `len` bytes is past the cap of one (256 MiB): one byte over is over.
pub(crate) fn too_large(len: usize) -> bool {
    u64::try_from(len).unwrap_or(u64::MAX) > MAX_PART_BYTES
}

/// The element a frame keeps for its object: the first `draw:object-ole` among the shape's kept nodes. Every node looked at is a step.
fn object_element(shape: &Shape) -> Option<&RawElement> {
    let slot = shape.ext.get()?;
    for placed in slot.odf.kept.kept.iter() {
        crate::work::object_frames(1);
        if let RawNode::Element(el) = &placed.node
            && &*el.name == OBJECT_ELEMENT
        {
            return Some(el);
        }
    }
    None
}

/// The entry an `xlink:href` names: with a leading `./` taken off and then as written, and percent-decoded, matched exactly.
fn entry_of<'p>(parts: &HashMap<&str, &'p Arc<Part>>, href: &str) -> Option<&'p Arc<Part>> {
    let bare = href.strip_prefix("./").unwrap_or(href);
    if let Some(found) = parts.get(bare) {
        return Some(found);
    }
    let decoded = lenient_decode(bare);
    parts.get(decoded.as_str()).copied()
}

/// The value of one base64 character of the standard alphabet; `None` for a blank or the padding (skipped), `Some(None)` for a character that is no base64.
pub(crate) fn sextet(b: u8) -> Option<Option<u8>> {
    Some(Some(match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        b'=' => return None,
        b if b.is_ascii_whitespace() => return None,
        _ => return Some(None),
    }))
}

/// The bytes of the inline data of an object (`office:binary-data`): **the characters are counted and judged in one pass before anything is made** (a character that is no base64 is refused there),
/// refused past what is left of the save's room for them or past the cap of a part, then decoded once into a buffer reserved for exactly that. The standard alphabet with its padding, blanks
/// between. `None` for text that is no base64.
fn inline_bytes(data: &RawElement, room: u64) -> Option<Arc<[u8]>> {
    let mut chars = 0u64;
    for node in &data.children {
        crate::work::object_frames(1);
        if let RawNode::Text(text) = node {
            for b in text.bytes() {
                match sextet(b) {
                    None => {}
                    Some(Some(_)) => chars = chars.saturating_add(1),
                    Some(None) => return None,
                }
            }
        }
    }
    let len = chars.saturating_mul(3) / 4;
    if len > room || len > MAX_PART_BYTES {
        return None;
    }
    let len = usize::try_from(len).ok()?;
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(len).ok()?;
    let (mut acc, mut bits) = (0u32, 0u32);
    for node in &data.children {
        let RawNode::Text(text) = node else { continue };
        for b in text.bytes() {
            let Some(Some(v)) = sextet(b) else { continue };
            acc = (acc << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(u8::try_from((acc >> bits) & 0xFF).ok()?);
            }
        }
    }
    crate::work::object_bytes_copied(out.len());
    Some(Arc::from(out))
}

/// The text of a cell the shape holds, as the writer writes it.
fn cell_text<'s>(shape: &'s Shape, name: &str) -> Option<&'s str> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()).map(|v| v.text())
}

/// The attributes of an object shape's `ForeignData`: `ForeignType` `Object`, then the attributes that rode with the frame in their order, or the defaults.
fn object_attrs(shape: &Shape, foreign: &ForeignData) -> RawAttrs {
    let mut out = RawAttrs::new();
    out.push("ForeignType", "Object");
    let carried: Vec<(&str, &str)> = foreign.attrs.iter().filter(|(name, _)| *name != "ForeignType").collect();
    if !carried.is_empty() {
        for (name, value) in carried {
            out.push(name, value);
        }
        return out;
    }
    out.push("ObjectType", OBJECT_TYPE);
    out.push("ShowAsIcon", "0");
    for (attr, cell) in [("ObjectWidth", "Width"), ("ObjectHeight", "Height")] {
        if let Some(text) = cell_text(shape, cell) {
            out.push(attr, text);
        }
    }
    out
}

impl Embeddings {
    /// Judges every object frame of the model's pages, and notes the parts the frames that are not carried back name. One walk of the shapes, with a stack of its own.
    pub(crate) fn judge(doc: &Document) -> R<Embeddings> {
        Embeddings::judge_within(doc, MAX_NATIVE_BYTES, MAX_INLINE_BYTES)
    }

    /// [`Embeddings::judge`] with the room the save has for embeddings and for inline objects stated: a frame whose embedding would pass `native` bytes in all, or whose inline data would pass
    /// `inline`, is not carried back.
    pub(crate) fn judge_within(doc: &Document, native: u64, inline: u64) -> R<Embeddings> {
        let mut out = Embeddings { native, inline_max: inline, ..Embeddings::default() };
        // The parts of the model by name, made once the first frame needs it.
        let mut index: Option<HashMap<&str, &Arc<Part>>> = None;
        for page in doc.pages.iter() {
            out.walk(&page.shapes, doc, &mut index, true)?;
        }
        // A frame of a master is never carried back (an `.odg` has no master-level object, and the page relationship of R26 has no master's): what it names rides.
        for master in doc.masters.iter() {
            out.walk(&master.shapes, doc, &mut index, false)?;
        }
        Ok(out)
    }

    fn walk<'d>(&mut self, list: &ShapeList, doc: &'d Document, index: &mut Option<HashMap<&'d str, &'d Arc<Part>>>, pages: bool) -> R<()> {
        let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 0)];
        while let Some((top, at)) = stack.last_mut() {
            let Some(shape) = top.get(*at) else {
                stack.pop();
                continue;
            };
            *at += 1;
            crate::work::object_frames(1);
            if shape.foreign.is_some() && shape.ext.get().is_some() {
                self.frame(shape, doc, index, pages);
            }
            if !shape.children.is_empty() && stack.len() < MAX_SHAPE_DEPTH {
                stack.push((&shape.children, 0));
            }
        }
        Ok(())
    }

    /// Judges one shape: when it is an object frame, whether it is carried back, and what it names.
    fn frame<'d>(&mut self, shape: &Arc<Shape>, doc: &'d Document, index: &mut Option<HashMap<&'d str, &'d Arc<Part>>>, pages: bool) {
        let Some(foreign) = shape.foreign.as_ref() else { return };
        // A shape of VSDX's own, or one already written as an object, is no frame of an `.odg`.
        if shape.kind != ShapeType::Foreign || foreign.rel_id.is_some() || foreign.attr("ForeignType") == Some("Object") {
            return;
        }
        let Some(element) = object_element(shape) else { return };
        let inline = element.children.iter().find_map(|n| match n {
            RawNode::Element(e) if &*e.name == "office:binary-data" => Some(e),
            _ => None,
        });
        // The entry the frame names, found in the map of the model's parts (made once, charged by the parts it holds). An href beside inline data is ignored.
        let named: Option<&Arc<Part>> = match (inline, element.attrs.get("xlink:href")) {
            (None, Some(href)) => {
                let parts = index.get_or_insert_with(|| {
                    crate::work::object_parts(doc.parts.len());
                    doc.parts.iter().filter(|p| p.format == Format::Odf).map(|p| (&*p.name, p)).collect()
                });
                entry_of(parts, href)
            }
            _ => None,
        };
        let part_id = named.map(|p| Arc::as_ptr(p).addr());
        let ptr = Arc::as_ptr(shape).addr();
        let back = (|| {
            if !pages {
                return None;
            }
            let picture = foreign.picture.as_ref()?;
            if picture_by_bytes(&picture.bytes).is_none() || too_large(picture.bytes.len()) {
                return None;
            }
            let (identity, bytes): (Identity, Arc<[u8]>) = match (inline, named) {
                (Some(data), _) => {
                    let room = self.inline_max.saturating_sub(self.inline);
                    // An object that is no compound file is never decoded past its first bytes: the length is judged from the characters, and the bytes are made only for a frame that has a picture.
                    let bytes = inline_bytes(data, room)?;
                    (Identity::Inline(ptr), bytes)
                }
                (None, Some(part)) if eligible(part) => (Identity::Entry(Arc::as_ptr(part).addr()), Arc::clone(&part.bytes)),
                _ => return None,
            };
            if !*self.verdicts.entry(identity).or_insert_with(|| is_compound(&bytes)) {
                return None;
            }
            let key = (identity, Arc::as_ptr(picture).addr());
            let object = match self.made.get(&key) {
                Some(&object) => object,
                None => {
                    let total = self.bytes.saturating_add(bytes.len() as u64);
                    if total > self.native {
                        return None;
                    }
                    self.bytes = total;
                    if matches!(identity, Identity::Inline(_)) {
                        self.inline = self.inline.saturating_add(bytes.len() as u64);
                    }
                    self.objects.push(Object { bytes, media: Arc::clone(picture) });
                    let object = self.objects.len() - 1;
                    self.made.insert(key, object);
                    object
                }
            };
            Some(Back { object, attrs: object_attrs(shape, foreign), part: part_id })
        })();
        match back {
            Some(back) => {
                self.frames.insert(ptr, back);
            }
            None => {
                if let Some(id) = part_id {
                    self.riding.insert(id);
                }
            }
        }
    }

    /// The frame `shape` is carried back as, if it is.
    pub(crate) fn back(&self, shape: &Arc<Shape>) -> Option<&Back> {
        self.frames.get(&Arc::as_ptr(shape).addr())
    }

    /// What the carrier leaves out when the frames in `written` (by the allocation of their shapes) are written as embeddings: the parts that only those frames name, and those frames' shapes.
    pub(crate) fn natively(&self, written: &HashSet<usize>) -> Natively {
        let mut out = Natively::default();
        let mut also: HashSet<usize> = self.riding.clone();
        for (ptr, back) in &self.frames {
            if written.contains(ptr) {
                out.shapes.insert(*ptr);
                if let Some(part) = back.part {
                    out.parts.insert(part);
                }
            } else if let Some(part) = back.part {
                also.insert(part);
            }
        }
        out.parts.retain(|part| !also.contains(part));
        out
    }

    /// [`Embeddings::natively`] for a writer that writes every frame it judged.
    pub(crate) fn natively_all(&self) -> Natively {
        let written: HashSet<usize> = self.frames.keys().copied().collect();
        self.natively(&written)
    }
}

// ------------------------------------------------------------------------------------------------------------------- the model writer

/// The embeddings a model writer made parts for, by the number of the object: the name of each object part, and the row of its relationship part to its picture.
pub(super) struct Planned {
    pub emb: Embeddings,
    pub names: Vec<String>,
    pub rows: Vec<super::parts::RelRow>,
}

impl<'a> Book<'a> {
    /// Plans the object parts of the frames that are carried back: one part for each embedding, named above every `oleObject` part in use, typed by the book's own rule for `bin`, with the one `image`
    /// relationship to the picture the picture writer planned. **Planned after the pictures and before the data the carrier holds**, so that the carrier knows what went natively, and so that a
    /// `Default` for `bin` of the object type is the first the types book meets.
    pub(super) fn objects(&mut self) -> R<()> {
        let emb = Embeddings::judge(self.doc)?;
        let mut names: Vec<String> = Vec::with_capacity(emb.objects.len());
        let mut rows: Vec<super::parts::RelRow> = Vec::with_capacity(emb.objects.len());
        if !emb.objects.is_empty() {
            self.walks.object = greatest_number(self.names_in_use());
            let dir = format!("{}embeddings/", folder_of(&self.document_part));
            for (i, object) in emb.objects.iter().enumerate() {
                let name = self.fresh_name(&dir, "oleObject", ".bin", |w| &mut w.object)?;
                if !plausible(&name) {
                    return Err(WriteError::Limit("no part name was left for an embedded object"));
                }
                let picture = self
                    .media_of
                    .get(&(Arc::as_ptr(&object.media).addr()))
                    .and_then(|at| self.media.get(*at))
                    .map(|plan| plan.name.clone())
                    .ok_or(WriteError::Mismatch("the picture of an object was not planned"))?;
                let target = relative_target(&name, &picture);
                let id = self
                    .rows_of(&name)
                    .add(IMAGE, &target, false)
                    .ok_or(WriteError::Limit("no relationship id was left for the picture of an object"))?;
                self.entries.push(Entry { name: name.clone(), ct: OLE_TYPE.to_owned(), body: Body::Object(i) });
                names.push(name);
                rows.push(super::parts::RelRow { id, ty: IMAGE.to_owned(), target, external: false });
            }
        }
        self.embedded = Some(Planned { emb, names, rows });
        Ok(())
    }

    /// What the carrier leaves out, by what this writer wrote: every frame it judged.
    pub(super) fn natively(&self) -> Natively {
        self.embedded.as_ref().map(|p| p.emb.natively_all()).unwrap_or_default()
    }

    /// The lead of a frame the model writer wrote as an embedding: the row from a page part to the object part, and the attributes of the `ForeignData`.
    pub(super) fn object_lead(&self, shape: &Arc<Shape>) -> Option<ObjectLead> {
        let planned = self.embedded.as_ref()?;
        let back = planned.emb.back(shape)?;
        let name = planned.names.get(back.object)?;
        Some(ObjectLead { lead: Lead::object(name), attrs: back.attrs.clone() })
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the save into a file

/// What the save into a file keeps of the objects: the frames judged (made when the first page that needs them is), the part each embedding was written as, and the frames written.
#[derive(Default)]
pub(super) struct SpliceObjects {
    emb: Option<Embeddings>,
    parts: HashMap<usize, String>,
    /// The row of each embedding's relationship part to its picture, by the number of the object.
    rows: HashMap<usize, super::parts::RelRow>,
    walk: Option<NameWalk>,
    written: HashSet<usize>,
}

impl Plan<'_> {
    /// The lead of an object frame on a page this save makes a part for: the embedding is written the first time a frame needs it (its part, its type, its relationship part), and a second frame of
    /// the same object and picture shares it.
    pub(super) fn object_lead(&mut self, shape: &Arc<Shape>) -> R<Option<ObjectLead>> {
        if self.new_rows.objects.emb.is_none() {
            let emb = Embeddings::judge(self.doc)?;
            self.new_rows.objects.emb = Some(emb);
        }
        let Some((object, attrs, bytes, media)) = self.new_rows.objects.emb.as_ref().and_then(|emb| {
            let back = emb.back(shape)?;
            let object = emb.objects.get(back.object)?;
            Some((back.object, back.attrs.clone(), Arc::clone(&object.bytes), Arc::clone(&object.media)))
        }) else {
            return Ok(None);
        };
        let name = match self.new_rows.objects.parts.get(&object) {
            Some(name) => name.clone(),
            None => {
                let (name, row) = self.write_object(&bytes, &media)?;
                self.new_rows.objects.parts.insert(object, name.clone());
                self.new_rows.objects.rows.insert(object, row);
                name
            }
        };
        self.new_rows.objects.written.insert(Arc::as_ptr(shape).addr());
        Ok(Some(ObjectLead { lead: Lead::object(&name), attrs }))
    }

    /// Writes the object part for an embedding: the bytes the model holds, shared and not copied; its type; and its relationship part with the one `image` row to the picture. → the name of the part,
    /// and that row.
    fn write_object(&mut self, bytes: &Arc<[u8]>, media: &Arc<Media>) -> R<(String, super::parts::RelRow)> {
        let mut walk = match self.new_rows.objects.walk.take() {
            Some(walk) => walk,
            None => {
                let start = greatest_number(self.present.keys().chain(self.reserved.iter()).map(String::as_str));
                let mut walk = NameWalk::new(&format!("{}embeddings/", folder_of(&self.document_part)), "oleObject", ".bin");
                walk.n = start;
                walk
            }
        };
        let name = self.next_name(&mut walk);
        self.new_rows.objects.walk = Some(walk);
        let Some(name) = name.filter(|n| plausible(n)) else { return Err(WriteError::Limit("no part name was left for an embedded object")) };
        self.reserved.insert(diacraft_opc::name::fold(&name));
        // The picture the object part leads to is the picture writer's: a part of the file that holds it, or one made from the model's bytes.
        let Some(lead) = super::page_rows::Leads::picture(self, media)? else {
            return Err(WriteError::Mismatch("the picture of an object has no part"));
        };
        self.set_shared_part(&name, Arc::clone(bytes), None);
        // `Default` for `bin` of the object type when the file has none; an `Override` when its `Default` says another type.
        self.types.ensure_default("bin", OLE_TYPE);
        if !self.types.default_of("bin").is_some_and(|t| t.eq_ignore_ascii_case(OLE_TYPE)) {
            self.types.set_override(&name, OLE_TYPE);
        }
        let target = relative_target(&name, &lead.target);
        self.rels_mut(&name)?.add("rId1", IMAGE, &target);
        self.carried_blobs.push((name.clone(), Arc::clone(bytes)));
        Ok((name, super::parts::RelRow { id: "rId1".to_owned(), ty: IMAGE.to_owned(), target, external: false }))
    }

    /// What the carrier leaves out, by what this save wrote.
    pub(super) fn natively(&self) -> Natively {
        match &self.new_rows.objects.emb {
            Some(emb) => emb.natively(&self.new_rows.objects.written),
            None => Natively::default(),
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the baseline

/// An embedding a save made from an object of the other format: the name of its part, the bytes it holds (the model's, shared), and the rows of its relationship part.
pub(super) struct MadeEmbedding {
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub rows: Vec<super::parts::RelRow>,
}

impl Book<'_> {
    /// The embeddings this model writer made, in the order of the objects.
    pub(super) fn made_embeddings(&self) -> Vec<MadeEmbedding> {
        let Some(planned) = &self.embedded else { return Vec::new() };
        planned
            .emb
            .objects
            .iter()
            .zip(&planned.names)
            .zip(&planned.rows)
            .map(|((object, name), row)| MadeEmbedding { name: name.clone(), bytes: Arc::clone(&object.bytes), rows: vec![row.clone()] })
            .collect()
    }
}

impl Plan<'_> {
    /// The embeddings this save into a file made (for the frames of the pages it made parts for), in the order of the objects.
    pub(super) fn made_embeddings(&self) -> Vec<MadeEmbedding> {
        let Some(emb) = &self.new_rows.objects.emb else { return Vec::new() };
        let mut made: Vec<(usize, &String)> = self.new_rows.objects.parts.iter().map(|(object, name)| (*object, name)).collect();
        made.sort_unstable();
        made.into_iter()
            .filter_map(|(object, name)| {
                Some(MadeEmbedding {
                    name: name.clone(),
                    bytes: Arc::clone(&emb.objects.get(object)?.bytes),
                    rows: self.new_rows.objects.rows.get(&object).cloned().into_iter().collect(),
                })
            })
            .collect()
    }
}

/// **The parts of the baseline a save returns are the parts the file holds**, and not the model's whole: the model's parts but **the ones the save left out** (`dropped`: a macro
/// project the type cannot hold, or of the other format, by name) and **the objects' files of the other format it wrote as embeddings** (`natively`), and **the embeddings it made from them**
/// (`made`), each holding the model's bytes. Composed by name, from what the writer did: no part is read again and no bytes are compared. The baseline was the model's parts: a save into a
/// macro-free type said it held the macro project it left out, so a Save As from that file to a macro-enabled type wrote no project (the drawing and the baseline both had it) and the next edit
/// did; and a caller that became this model after a save across held an object shape that named an embedding it did not have.
pub(super) fn baseline_parts(
    model: &Document,
    natively: &[Arc<str>],
    dropped: &[String],
    made: Vec<MadeEmbedding>,
) -> diacraft_doc::SharedVec<Arc<Part>> {
    if natively.is_empty() && dropped.is_empty() && made.is_empty() {
        return model.parts.clone();
    }
    let went: HashSet<&str> = natively.iter().map(|name| &**name).collect();
    let left: HashSet<String> = dropped.iter().map(|name| diacraft_opc::name::fold(name)).collect();
    let mut parts: Vec<Arc<Part>> = made
        .into_iter()
        .map(|m| {
            let source: Arc<str> = Arc::from(m.name.as_str());
            let rels: Vec<diacraft_doc::Rel> = m
                .rows
                .into_iter()
                .map(|row| diacraft_doc::Rel {
                    source: Arc::clone(&source),
                    id: Arc::from(row.id),
                    kind: Arc::from(row.ty),
                    target: Arc::from(row.target),
                    external: row.external,
                })
                .collect();
            Arc::new(Part {
                format: Format::Vsdx,
                name: source,
                media_type: Arc::from(OLE_TYPE),
                role: PartRole::Embedding,
                manifest: RawAttrs::new(),
                rels: Arc::new(rels),
                bytes: m.bytes,
            })
        })
        .collect();
    parts.extend(
        model
            .parts
            .iter()
            .filter(|part| !(part.format != Format::Vsdx && went.contains(&*part.name)) && !left.contains(&diacraft_opc::name::fold(&part.name)))
            .cloned(),
    );
    diacraft_doc::SharedVec::from_vec(parts)
}

/// The relationship type of an object.
pub(super) fn ole_rel() -> String {
    format!("{OD}oleObject")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compound file as R9 asks one to be: 1,536 bytes, the header and a root entry.
    fn compound() -> Vec<u8> {
        let mut bytes = vec![0u8; 1536];
        bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        bytes[1024 + 0x42] = 5;
        bytes
    }

    #[test]
    fn each_of_these_turns_a_compound_file_into_something_that_is_not_one() {
        assert!(is_compound(&compound()));
        let broken: Vec<(&str, Vec<u8>)> = vec![
            ("byte 0", {
                let mut b = compound();
                b[0] = 0xD1;
                b
            }),
            ("the sector shift 12 with version 3", {
                let mut b = compound();
                b[0x1E] = 12;
                b
            }),
            ("a directory sector past the end", {
                let mut b = compound();
                b[0x30] = 2;
                b
            }),
            ("a directory sector no sector is", {
                let mut b = compound();
                b[0x30..0x34].copy_from_slice(&[0xFE, 0xFF, 0xFF, 0xFF]);
                b
            }),
            ("a root entry of another type", {
                let mut b = compound();
                b[1024 + 0x42] = 2;
                b
            }),
            ("cut to 511 bytes", compound()[..511].to_vec()),
            ("version 4 with 512-byte sectors' length", {
                let mut b = compound();
                b[0x1A] = 4;
                b[0x1E] = 12;
                b
            }),
            ("a byte order mark that is not one", {
                let mut b = compound();
                b[0x1C] = 0;
                b
            }),
        ];
        for (what, bytes) in broken {
            assert!(!is_compound(&bytes), "{what}");
        }
        // The last byte the entry needs is the last that may be missing: the entry starts at 1,024 and is 128 bytes.
        assert!(is_compound(&compound()[..1024 + 128]));
        assert!(!is_compound(&compound()[..1024 + 127]));
    }

    #[test]
    fn the_number_in_an_object_part_name_is_its_last_segments_digits() {
        assert_eq!(object_number("/visio/embeddings/oleObject12.bin"), Some(12));
        assert_eq!(object_number("/x/OLEOBJECT3.BIN"), Some(3));
        for none in [
            "/visio/embeddings/oleObject.bin",
            "/visio/embeddings/oleObject1x.bin",
            "/visio/oleObject1.bin.rels",
            "/oleObject-1.bin",
            "",
            "/oleObject1.bin/x",
        ] {
            assert_eq!(object_number(none), None, "{none}");
        }
        assert_eq!(object_number("/visio/embeddings/oleObject99999999999999999999.bin"), None, "a number that does not fit is none");
        assert_eq!(greatest_number(["/a/oleObject1.bin", "/a/oleObject7.bin", "/a/image9.png", "/a/oleObject3.bin"]), 7);
        assert_eq!(greatest_number(std::iter::empty::<&str>()), 0);
    }

    #[test]
    fn a_part_is_too_large_from_one_byte_over_the_cap_of_a_part() {
        let cap = usize::try_from(MAX_PART_BYTES).unwrap();
        assert!(!too_large(0) && !too_large(cap - 1) && !too_large(cap));
        assert!(too_large(cap + 1) && too_large(usize::MAX));
    }

    /// A frame of the other format as the `.odg` reader holds one: a `Foreign` shape that shows a PNG, with the `draw:object-ole` element beside it.
    fn frame(id: u32, href: &str, picture: u8) -> Arc<Shape> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.push(picture);
        let media = Arc::new(Media { part: Arc::from("p"), content_type: Arc::from("image/png"), bytes: Arc::from(png) });
        let foreign = ForeignData { picture: Some(media), ..ForeignData::default() };
        let mut shape = Shape { id, kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::default() };
        let object = RawElement { name: Arc::from("draw:object-ole"), attrs: RawAttrs::from_pairs([("xlink:href", href)]), children: Vec::new() };
        shape.ext.make_mut().odf.kept.kept.push(0, RawNode::Element(object));
        Arc::new(shape)
    }

    fn entry(name: &str) -> Arc<Part> {
        Arc::new(Part {
            format: Format::Odf,
            name: Arc::from(name),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(compound()),
        })
    }

    #[test]
    fn the_budgets_of_the_judge_are_kept_to_the_byte() {
        // Three frames of three objects of 1,536 bytes, each with a picture of its own.
        let mut doc = Document::new_blank();
        let id = doc.add_page(Some("P")).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        for i in 0..3u8 {
            page.shapes.push(frame(u32::from(i) + 1, &format!("./Object {i}"), i));
            doc.parts.push(entry(&format!("Object {i}")));
        }
        let made = |native: u64| {
            let emb = Embeddings::judge_within(&doc, native, 0).unwrap();
            (emb.objects.len(), emb.frames.len(), emb.riding.len())
        };
        assert_eq!(made(1536 * 3), (3, 3, 0), "room for all three");
        assert_eq!(made(1536 * 3 - 1), (2, 2, 1), "one byte short for the third: it rides, and its part with it");
        assert_eq!(made(1536 * 2), (2, 2, 1));
        assert_eq!(made(1535), (0, 0, 3), "no room: every frame rides");
        assert_eq!(made(0), (0, 0, 3));
        // Two frames of one object and one picture are one embedding however little room there is beyond it.
        let mut doc = Document::new_blank();
        let id = doc.add_page(Some("P")).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        let shared = frame(1, "./Object 0", 0);
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        let mut twin = (*shared).clone();
        twin.id = 2;
        // The same picture allocation: one embedding.
        twin.foreign = shared.foreign.clone();
        page.shapes.push(shared);
        page.shapes.push(Arc::new(twin));
        doc.parts.push(entry("Object 0"));
        let emb = Embeddings::judge_within(&doc, 1536, 0).unwrap();
        assert_eq!((emb.objects.len(), emb.frames.len()), (1, 2), "the second frame costs nothing more");
    }

    #[test]
    fn inline_objects_are_kept_to_the_room_left_for_them() {
        let base64 = |bytes: &[u8]| -> String {
            const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
                for i in 0..4 {
                    out.push(if i <= chunk.len() { char::from(A[((n >> (18 - 6 * i)) & 63) as usize]) } else { '=' });
                }
            }
            out
        };
        let mut doc = Document::new_blank();
        let id = doc.add_page(Some("P")).unwrap();
        let at = doc.pages.iter().position(|p| p.id == id).unwrap();
        let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
        let text = base64(&compound());
        for i in 0..3u8 {
            let mut shape = (*frame(u32::from(i) + 1, "", i)).clone();
            let data =
                RawElement { name: Arc::from("office:binary-data"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(text.as_str()))] };
            let object = RawElement { name: Arc::from("draw:object-ole"), attrs: RawAttrs::new(), children: vec![RawNode::Element(data)] };
            let slot = shape.ext.make_mut();
            slot.odf.kept.kept = diacraft_doc::Passthrough::new();
            slot.odf.kept.kept.push(0, RawNode::Element(object));
            page.shapes.push(Arc::new(shape));
        }
        let made = |inline: u64| Embeddings::judge_within(&doc, u64::MAX, inline).unwrap().objects.len();
        assert_eq!(made(1536 * 3), 3);
        assert_eq!(made(1536 * 3 - 1), 2, "one byte short for the third");
        assert_eq!(made(1535), 0);
    }

    #[test]
    fn inline_data_is_counted_before_it_is_made_and_decoded_once() {
        let el =
            |text: &str| RawElement { name: Arc::from("office:binary-data"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(text))] };
        assert_eq!(inline_bytes(&el("TWFu"), 100).as_deref(), Some(&b"Man"[..]));
        assert_eq!(inline_bytes(&el("  TW\n    Fu\n"), 100).as_deref(), Some(&b"Man"[..]));
        assert_eq!(inline_bytes(&el("TWE="), 100).as_deref(), Some(&b"Ma"[..]));
        assert_eq!(inline_bytes(&el("TQ=="), 100).as_deref(), Some(&b"M"[..]));
        assert_eq!(inline_bytes(&el("TWFu"), 2), None, "three bytes are more than the room of two");
        assert_eq!(inline_bytes(&el("TWFu"), 3).as_deref(), Some(&b"Man"[..]), "and exactly the room is room enough");
        assert_eq!(inline_bytes(&el("TW*u"), 100), None, "not base64");
    }
}
