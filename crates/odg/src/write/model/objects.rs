//! A VSDX OLE object written as an ODF object frame (the native-format design's §4.5, Owner 9).
//!
//! A shape of a drawing that came from a VSDX file is a **carried object** when all four hold: its `ForeignType` is `Object`; its `Rel` names an internal `oleObject` relationship from the
//! page's part; the target part is a compound file; and the relationship part beside it names exactly one internal picture, whose bytes (the shape's replacement picture, which the VSDX
//! reader loaded) are one of the pictures DiaCraft tells by their bytes. It is then written as the frame R20 describes: **`draw:object-ole`** with `xlink:href="./Object N"`, the three link
//! attributes and `draw:class-id` (the compound file's class, bare, left out when null), then **`draw:image`** naming `./ObjectReplacements/Object N`; and the package gets the two entries
//! of R21: `Object N`, the part's bytes unchanged, typed `application/vnd.sun.star.oleobject`, and `ObjectReplacements/Object N`, the replacement picture's bytes unchanged, typed by what
//! the picture is. N is the lowest number above every `Object N` in use. Nothing is written for `ShowAsIcon` or a draw aspect: the attributes of the `ForeignData` other than the type
//! are the shape's tier 2.
//!
//! This is done to the **model** before the splice (the frame is the shape's own kept markup, which the new-shape path writes back, and the entries are package entries of the model's):
//! [`carry`] gives the model with each such shape rewritten as the object frame an `.odg` reader would have made of it, and with the VSDX parts it carried taken out. **An object that is not
//! carried is left as it is**, and the plan refuses it in words, as it refuses every VSDX part an `.odg` has no place for yet. Every object looked at is a step, charged
//! before it is looked at; the walk of a page's shapes is bounded by the model's own depth.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, Format, MAX_SHAPE_DEPTH, Part, PartRole, RawAttrs, RawElement, RawNode, Shape, ShapeList, ShapeType};

use crate::picturetype::picture_kind;
use crate::write::{WriteError, WriteWork};

/// The media type R21 gives an object entry.
const OBJECT_MEDIA: &str = "application/vnd.sun.star.oleobject";

/// The namespace of `xlink:`.
const XLINK: &str = "http://www.w3.org/1999/xlink";

/// What [`carry`] made.
pub(crate) struct Carried {
    /// The model with the carried shapes rewritten, the entries added and the VSDX parts it carried taken out.
    pub doc: Document,
    /// The relationships that named the objects (source and `Id`), which the plan does not count as held VSDX data.
    pub rels: HashSet<(String, String)>,
}

/// The class of a compound file's root, as R22 writes it: `Some(None)` for a null class, `Some(Some("D1A0C0DE-..."))` for one, and `None` when the bytes are no compound file by R9
/// (the signature, the byte order mark, version 3 with sector shift 9 or version 4 with 12, a directory sector inside the bytes and a root entry of type 5).
pub(crate) fn compound_class(bytes: &[u8]) -> Option<Option<String>> {
    const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    if bytes.get(..8)? != SIGNATURE || bytes.len() < 512 {
        return None;
    }
    let u16_at = |at: usize| -> Option<u16> { Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?])) };
    let u32_at =
        |at: usize| -> Option<u32> { Some(u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, *bytes.get(at + 3)?])) };
    let (version, order, shift) = (u16_at(0x1A)?, u16_at(0x1C)?, u16_at(0x1E)?);
    if order != 0xFFFE || !matches!((version, shift), (3, 9) | (4, 12)) {
        return None;
    }
    let sector = 1usize << shift;
    let first = usize::try_from(u32_at(0x30)?).ok()?;
    // Sector n starts at (n + 1) * the sector size; the root is the first entry of the first directory sector.
    let at = first.checked_add(1)?.checked_mul(sector)?;
    let entry = bytes.get(at..at.checked_add(128)?)?;
    if *entry.get(0x42)? != 5 {
        return None;
    }
    let clsid = entry.get(0x50..0x60)?;
    if clsid.iter().all(|b| *b == 0) {
        return Some(None);
    }
    // A CLSID is stored as a 4-byte, a 2-byte and a 2-byte little-endian number, then 8 bytes in order.
    let d1 = u32::from_le_bytes([clsid[0], clsid[1], clsid[2], clsid[3]]);
    let d2 = u16::from_le_bytes([clsid[4], clsid[5]]);
    let d3 = u16::from_le_bytes([clsid[6], clsid[7]]);
    Some(Some(format!(
        "{d1:08X}-{d2:04X}-{d3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        clsid[8], clsid[9], clsid[10], clsid[11], clsid[12], clsid[13], clsid[14], clsid[15]
    )))
}

/// The part name a relationship target names, from the part the relationship comes from: absolute from the package's root, or relative to the source's folder, `.` and `..` resolved.
fn resolve(source: &str, target: &str) -> Option<String> {
    let mut path: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        path.extend(source.split('/').filter(|s| !s.is_empty()));
        path.pop();
    }
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                path.pop()?;
            }
            other => path.push(other),
        }
    }
    Some(format!("/{}", path.join("/")))
}

/// The numbers `N` in the names of the model's own `Object N` entries (a folder or a file), and those the save has made.
fn numbers_in_use(doc: &Document) -> HashSet<u64> {
    doc.parts
        .iter()
        .filter(|p| p.format == Format::Odf)
        .filter_map(|p| {
            let name = p.name.strip_prefix("ObjectReplacements/").unwrap_or(&p.name);
            let rest = name.strip_prefix("Object ")?;
            rest.split(['/', '.']).next()?.parse::<u64>().ok()
        })
        .collect()
}

/// An element made by hand: `name` with `attrs` and no children.
fn element(name: &str, attrs: &[(&str, &str)]) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() })
}

/// Everything [`carry`] needs while it walks, and what it makes.
struct Walk<'a> {
    doc: &'a Document,
    used: HashSet<u64>,
    entries: Vec<Arc<Part>>,
    taken: HashSet<String>,
    rels: HashSet<(String, String)>,
}

impl Walk<'_> {
    /// The object frame of the shape `shape` of the page whose part is `page_part`, when it is a carried object: the shape rewritten, with its two entries made.
    fn object(&mut self, shape: &Shape, page_part: &str) -> Option<Shape> {
        if shape.kind != ShapeType::Foreign {
            return None;
        }
        let foreign = shape.foreign.as_deref()?;
        if foreign.attr("ForeignType") != Some("Object") {
            return None;
        }
        let (rel_id, picture) = (foreign.rel_id.as_deref()?, foreign.picture.as_deref()?);
        let held = self.doc.vsdx.as_ref()?;
        let rel = held
            .rels
            .iter()
            .find(|r| &*r.source == page_part && r.id.eq_ignore_ascii_case(rel_id) && !r.external && r.kind.ends_with("/oleObject"))?;
        let name = resolve(page_part, &rel.target)?;
        let part = self.doc.parts.iter().find(|p| p.format == Format::Vsdx && p.name.eq_ignore_ascii_case(&name) && p.role == PartRole::Embedding)?;
        let class = compound_class(&part.bytes)?;
        // The relationship part beside the object names exactly one internal picture, and the shape's replacement is a picture DiaCraft tells by its bytes.
        let mut images = part.rels.iter().filter(|r| r.kind.ends_with("/image"));
        let (Some(image), None) = (images.next(), images.next()) else { return None };
        if image.external {
            return None;
        }
        let kind = picture_kind(&picture.bytes)?;
        // N is the lowest number above every one in use.
        let number = self.used.iter().max().map_or(1, |m| m.saturating_add(1));
        self.used.insert(number);
        let (object_name, replacement_name) = (format!("Object {number}"), format!("ObjectReplacements/Object {number}"));
        let part_entry = |name: String, media: &str, role: PartRole, bytes: Arc<[u8]>| {
            Arc::new(Part {
                format: Format::Odf,
                name: Arc::from(name),
                media_type: Arc::from(media),
                role,
                manifest: RawAttrs::new(),
                rels: Arc::new(Vec::new()),
                bytes,
            })
        };
        self.entries.push(part_entry(object_name.clone(), OBJECT_MEDIA, PartRole::Embedding, Arc::clone(&part.bytes)));
        self.entries.push(part_entry(replacement_name.clone(), kind.media, PartRole::ObjectReplacement, Arc::clone(&picture.bytes)));
        self.taken.insert(part.name.to_string());
        self.rels.insert((page_part.to_owned(), rel_id.to_owned()));
        // The frame's children, as an `.odg` reader keeps them: the object first, then the replacement picture. The hrefs spell the space raw.
        let href = format!("./{object_name}");
        let image_href = format!("./{replacement_name}");
        let mut links = vec![
            ("xmlns:xlink", XLINK),
            ("xlink:href", href.as_str()),
            ("xlink:type", "simple"),
            ("xlink:show", "embed"),
            ("xlink:actuate", "onLoad"),
        ];
        if let Some(class) = class.as_deref() {
            links.push(("draw:class-id", class));
        }
        let mut image_links = vec![
            ("xmlns:xlink", XLINK),
            ("xlink:href", image_href.as_str()),
            ("xlink:type", "simple"),
            ("xlink:show", "embed"),
            ("xlink:actuate", "onLoad"),
        ];
        image_links.sort_by_key(|(k, _)| *k != "xmlns:xlink");
        let mut copy = shape.clone();
        let slot = copy.ext.make_mut();
        slot.odf.kept.kept.push(0, element("draw:object-ole", &links));
        slot.odf.kept.kept.push(0, element("draw:image", &image_links));
        Some(copy)
    }

    /// The shapes of a page's list (or of a group's), rewritten where one is a carried object: `None` when none changed.
    fn shapes(&mut self, list: &ShapeList, page_part: &str, depth: usize, work: &mut WriteWork) -> Result<Option<ShapeList>, WriteError> {
        if depth > MAX_SHAPE_DEPTH {
            return Ok(None);
        }
        let mut changed: Option<Vec<Arc<Shape>>> = None;
        for (at, shape) in list.iter().enumerate() {
            work.charge_steps(1)?;
            let replaced: Option<Arc<Shape>> = if shape.children.is_empty() {
                self.object(shape, page_part).map(Arc::new)
            } else {
                match self.shapes(&shape.children, page_part, depth + 1, work)? {
                    Some(children) => {
                        let mut copy = (**shape).clone();
                        copy.children = children;
                        Some(Arc::new(copy))
                    }
                    None => None,
                }
            };
            let Some(new) = replaced else { continue };
            let all = match &mut changed {
                Some(all) => all,
                None => {
                    let mut all: Vec<Arc<Shape>> = Vec::new();
                    all.try_reserve_exact(list.len())
                        .map_err(|_| WriteError::Limit { what: "memory for the shapes of a page", max: list.len() as u64 })?;
                    all.extend(list.iter().cloned());
                    changed.insert(all)
                }
            };
            if let Some(slot) = all.get_mut(at) {
                *slot = new;
            }
        }
        Ok(changed.map(ShapeList::from_vec))
    }
}

/// The model with each carried object of its pages written as an object frame (see the module documentation), or `None` when it holds none.
pub(super) fn carry(doc: &Document, work: &mut WriteWork) -> Result<Option<Carried>, WriteError> {
    if doc.vsdx.is_none() || !doc.parts.iter().any(|p| p.format == Format::Vsdx && p.role == PartRole::Embedding) {
        return Ok(None);
    }
    let mut walk = Walk { doc, used: numbers_in_use(doc), entries: Vec::new(), taken: HashSet::new(), rels: HashSet::new() };
    let mut out = doc.clone();
    let mut any = false;
    for (at, page) in doc.pages.iter().enumerate() {
        let Some(part) = page.part_name.as_deref() else { continue };
        if page.background {
            continue;
        }
        if let Some(shapes) = walk.shapes(&page.shapes, part, 0, work)?
            && let Some(slot) = out.pages.get_mut(at)
        {
            Arc::make_mut(slot).shapes = shapes;
            any = true;
        }
    }
    if !any {
        return Ok(None);
    }
    // The VSDX parts that were carried are out; the model's entries for the frames are in.
    let kept: Vec<Arc<Part>> = out.parts.iter().filter(|p| !walk.taken.contains(&*p.name)).cloned().collect();
    let mut parts = kept;
    parts
        .try_reserve(walk.entries.len())
        .map_err(|_| WriteError::Limit { what: "memory for the package entries", max: walk.entries.len() as u64 })?;
    parts.extend(walk.entries.iter().cloned());
    out.parts = diacraft_doc::SharedVec::from_vec(parts);
    Ok(Some(Carried { doc: out, rels: walk.rels }))
}

/// **`doc` as the model writer's file holds it of the VSDX data that rides**: the VSDX parts [`carry`] takes out (a carried object's embedding is the file's own `Object N`, written as an
/// object frame), and the relationships that named them, out of the drawing; its shapes and the entries of the frames are left as they are. The drawing a person has keeps the embedding, and a save
/// into the file the model writer wrote must be given it the same way, or the splice finds a VSDX part riding that the file does not carry, counts it as an edit and writes the object a second time
/// in DiaCraft's own folder. `None` when the drawing holds no carried object. The walk is [`carry`]'s, charged to `work`.
pub(super) fn without_carried(doc: &Document, work: &mut WriteWork) -> Result<Option<Document>, WriteError> {
    let Some(carried) = carry(doc, work)? else { return Ok(None) };
    // The VSDX parts that are no longer in the model `carry` made are the ones it took.
    let kept: HashSet<&str> = carried.doc.parts.iter().filter(|p| p.format == Format::Vsdx).map(|p| &*p.name).collect();
    let mut out = doc.clone();
    out.parts =
        diacraft_doc::SharedVec::from_vec(doc.parts.iter().filter(|p| p.format != Format::Vsdx || kept.contains(&*p.name)).cloned().collect());
    if let Some(held) = out.vsdx.as_mut() {
        Arc::make_mut(held).rels.retain(|r| !carried.rels.contains(&(r.source.to_string(), r.id.to_string())));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A compound file as R9 asks one to be: the header, and a root entry of the class.
    pub(crate) fn compound(class: [u8; 16]) -> Vec<u8> {
        let mut bytes = vec![0u8; 1536];
        bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        bytes[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        bytes[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&1u32.to_le_bytes());
        // Sector 1 starts at 1024: the root entry.
        bytes[1024 + 0x42] = 5;
        bytes[1024 + 0x50..1024 + 0x60].copy_from_slice(&class);
        bytes
    }

    /// V1 of the spec: the class of a compound file is the root's CLSID, written as the number it is and not as its bytes.
    #[test]
    fn the_class_of_a_compound_file_is_its_roots_clsid_in_the_form_writes() {
        let class = [0xDE, 0xC0, 0xA0, 0xD1, 0x01, 0x00, 0x00, 0x40, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBE, 0xEF];
        assert_eq!(compound_class(&compound(class)), Some(Some("D1A0C0DE-0001-4000-8000-00000000BEEF".to_owned())));
        assert_eq!(compound_class(&compound([0; 16])), Some(None), "a null class is left out");
        // Not a compound file: a ZIP, a short file, a header with the wrong shift for its version, a directory sector past the end, a root entry of another type.
        let mut bad = compound(class);
        assert!(compound_class(b"PK\x03\x04").is_none() && compound_class(&bad[..511]).is_none());
        bad[0x1E] = 12;
        assert!(compound_class(&bad).is_none(), "version 3 with sector shift 12");
        let mut far = compound(class);
        far[0x30..0x34].copy_from_slice(&99u32.to_le_bytes());
        assert!(compound_class(&far).is_none(), "a directory sector past the end");
        let mut kind = compound(class);
        kind[1024 + 0x42] = 2;
        assert!(compound_class(&kind).is_none(), "a root entry of another type");
    }

    /// A target is resolved from the folder of its source, `..` and `.` taken out, and an absolute one as it is; a path that leaves the root is none.
    #[test]
    fn a_relationship_target_is_resolved_from_the_folder_of_its_source() {
        assert_eq!(resolve("/visio/pages/page1.xml", "../embeddings/oleObject1.bin").as_deref(), Some("/visio/embeddings/oleObject1.bin"));
        assert_eq!(resolve("/visio/pages/page1.xml", "/visio/embeddings/a.bin").as_deref(), Some("/visio/embeddings/a.bin"));
        assert_eq!(resolve("/visio/pages/page1.xml", "./x.bin").as_deref(), Some("/visio/pages/x.bin"));
        assert_eq!(resolve("/visio/pages/page1.xml", "../../../x.bin"), None);
    }
}
