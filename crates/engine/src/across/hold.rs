//! What a drawing holds of the two things a person may choose to leave out of a save across: its **macro project** and its **embedded objects** (harvested from the shelved the `across/hold.rs`;  the native-format design's Owner 9 and Call 17).
//!
//! **Looked for in the model.** An earlier change asked the package (`diacraft_vsdx::holds` read a file's bytes), because the model then held none of it. The model holds every part now (`Document::parts`, each
//! tagged with its format and role) and the shapes that show an object, so what a save would carry is what the model holds, and an object a person has deleted since the file was read no longer asks for
//! anything. Nothing is parsed and nothing is run: a part is bytes, and a macro project or an object is named by its role and its name alone.
//!
//! * **A macro project** is a part whose role is `MacroProject`, `MacroSignature` or `MacroData`, or whose name is the `.odg`'s (`Basic/`, `Scripts/` and `META-INF/macrosignatures.xml`).
//! * **An embedded object** is a part that is an embedding, an object's own file or the picture that stands for it (by role, or by the names of `Object N` entries and folders and of
//!   `ObjectReplacements/`), and a shape that shows one: a `Foreign` shape of `ForeignType` `Object` or `Ink`, or a frame whose kept markup holds an object element.
//!
//! **`drop_objects`** ([`without_objects`]): the objects **of the other format** are left out of the file, and **what is left of their shapes is the picture they show** (the design: "its shape shows
//! its replacement picture natively in both cases"; the spec's question Q3, which asked whether the shape should go with the object, is answered for 0.2 as the design says, since a picture dropped
//! with the object would be data lost that the person did not ask to lose). The drawing keeps them: this works on a copy.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{
    Document, Format, MAX_SHAPE_DEPTH, Page, Part, PartRole, Passthrough, Placed, RawAttrs, RawNode, Shape, ShapeList, SharedVec, VsdxHeld,
};

use super::{MAX_STEPS, TOO_BIG};

/// What a drawing holds. See the [module documentation](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hold {
    /// How many parts are of a macro project.
    pub macro_parts: usize,
    /// How many parts are of embedded objects (an embedding, an object's file, a replacement picture).
    pub object_parts: usize,
    /// How many shapes show an object.
    pub object_shapes: u64,
}

/// The elements of a frame that make it an object an `.odg` keeps, by local name.
const ODF_OBJECT_ELEMENTS: [&str; 6] = ["object", "object-ole", "applet", "plugin", "floating-frame", "table"];

/// The folders of a VSDX package that hold embedded objects and ActiveX controls, lower case.
const VSDX_OBJECT_FOLDERS: [&str; 2] = ["/visio/embeddings/", "/visio/activex/"]; // brand-ok: part names the format defines

/// Whether a part belongs to a macro project: by its role, or by the name an `.odg` gives such an entry (either says it is).
pub(crate) fn is_macro_part(part: &Part) -> bool {
    if matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) {
        return true;
    }
    part.format == Format::Odf && {
        let folded = part.name.to_ascii_lowercase();
        folded.starts_with("basic/") || folded.starts_with("scripts/") || folded == "meta-inf/macrosignatures.xml"
    }
}

/// Whether a part is a file of an embedded object: an embedding, an object's own file or its replacement picture, by role or by name.
pub(crate) fn is_object_part(part: &Part) -> bool {
    if matches!(part.role, PartRole::Embedding | PartRole::ObjectFile | PartRole::ObjectReplacement) {
        return true;
    }
    match part.format {
        Format::Odf => {
            part.name.starts_with("ObjectReplacements/")
                || part.name.strip_prefix("Object ").is_some_and(|rest| {
                    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                    digits > 0 && rest.get(digits..).is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
                })
        }
        Format::Vsdx => {
            let folded = part.name.to_ascii_lowercase();
            VSDX_OBJECT_FOLDERS.iter().any(|f| folded.starts_with(f))
        }
        Format::Native => false,
    }
}

/// Whether a node is an element that makes a frame an object (by local name, as the prefix is the file's).
fn is_object_node(node: &RawNode) -> bool {
    matches!(node, RawNode::Element(el) if {
        let local = el.name.rsplit_once(':').map_or(&*el.name, |(_, local)| local);
        ODF_OBJECT_ELEMENTS.contains(&local)
    })
}

/// Whether the shape shows a VSDX object: a `Foreign` shape of `ForeignType` `Object` (an OLE object) or `Ink` (whose target is an OLE part).
pub(crate) fn is_vsdx_object(shape: &Shape) -> bool {
    shape.foreign.as_ref().is_some_and(|f| matches!(f.attr("ForeignType"), Some("Object" | "Ink")))
}

/// Whether the shape is a frame of an `.odg` that holds an object: one of its kept nodes (the `.odg` reader's, in the `odf` side of the slot) is an object element.
pub(crate) fn is_odf_object(shape: &Shape) -> bool {
    shape.ext.get().is_some_and(|data| data.odf.kept.kept.iter().any(|placed| is_object_node(&placed.node)))
}

/// Each shape of the drawing once, in order, children after their parent: the pages' and, when `masters`, the masters'. The walk has a stack of its own and the steps it takes are counted: past
/// [`MAX_STEPS`] shapes, or past the model's depth, it ends with [`TOO_BIG`]. → the shapes visited.
pub(crate) fn each_shape<'d>(doc: &'d Document, masters: bool, visit: impl FnMut(&'d Shape)) -> Result<u64, &'static str> {
    each_shape_within(doc, masters, MAX_STEPS, visit)
}

/// [`each_shape`] with the most shapes it may visit given.
pub(crate) fn each_shape_within<'d>(doc: &'d Document, masters: bool, max: u64, mut visit: impl FnMut(&'d Shape)) -> Result<u64, &'static str> {
    let mut steps: u64 = 0;
    let roots = doc.pages.iter().map(|p| &p.shapes).chain(doc.masters.iter().filter(|_| masters).map(|m| &m.shapes));
    for root in roots {
        let mut stack: Vec<(&'d ShapeList, usize)> = vec![(root, 0)];
        while let Some(top) = stack.last_mut() {
            let list: &'d ShapeList = top.0;
            let Some(shape) = list.get(top.1) else {
                stack.pop();
                continue;
            };
            top.1 = top.1.saturating_add(1);
            steps = steps.saturating_add(1);
            if steps > max {
                return Err(TOO_BIG);
            }
            visit(shape);
            if !shape.children.is_empty() {
                if stack.len() >= MAX_SHAPE_DEPTH {
                    return Err(TOO_BIG);
                }
                stack.push((&shape.children, 0));
            }
        }
    }
    Ok(steps)
}

/// What the drawing holds: its macro parts, its object parts and the shapes that show an object (of either format).
pub(crate) fn hold(doc: &Document) -> Result<Hold, &'static str> {
    hold_within(doc, MAX_STEPS)
}

/// [`hold`] with the most steps it may take given.
pub(crate) fn hold_within(doc: &Document, max: u64) -> Result<Hold, &'static str> {
    let mut found = Hold::default();
    if u64::try_from(doc.parts.len()).unwrap_or(u64::MAX) > max {
        return Err(TOO_BIG);
    }
    for part in doc.parts.iter() {
        found.macro_parts = found.macro_parts.saturating_add(usize::from(is_macro_part(part)));
        found.object_parts = found.object_parts.saturating_add(usize::from(is_object_part(part)));
    }
    let room = max.saturating_sub(doc.parts.len() as u64);
    each_shape_within(doc, true, room, |shape| {
        found.object_shapes = found.object_shapes.saturating_add(u64::from(is_vsdx_object(shape) || is_odf_object(shape)));
    })?;
    Ok(found)
}

/// What [`without_objects`] left out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Left {
    /// The shapes that showed an object of the other format and now show their picture.
    pub shapes: u64,
    /// The parts of such objects.
    pub parts: usize,
}

/// The part a relationship's target names, from the part the relationship comes from (absolute from the package's root, or relative to the source's folder, `.` and `..` resolved).
fn resolve(source: &str, target: &str) -> Option<String> {
    let mut path: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        path.extend(source.split('/').filter(|s| !s.is_empty()));
        path.pop();
    }
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                path.pop()?;
            }
            other => path.push(other),
        }
    }
    Some(format!("/{}", path.join("/")))
}

/// The type a `ForeignType` of a picture says, by its first bytes: `EnhMetaFile` for an EMF, `Bitmap` for the other pictures the model draws, and `None` for bytes that are no picture.
pub(crate) fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    const BITMAPS: [&[u8]; 5] = [b"\x89PNG\r\n\x1a\n", b"\xFF\xD8\xFF", b"GIF8", b"BM", b"II*\0"];
    if bytes.get(40..44) == Some(b" EMF".as_slice()) {
        Some("EnhMetaFile")
    } else if BITMAPS.iter().any(|sig| bytes.starts_with(sig)) || bytes.starts_with(b"MM\0*") {
        Some("Bitmap")
    } else {
        None
    }
}

/// The shapes of `list` with `change` applied to each (a copy of each it changes, group members included), or `None` when it changes none. The walk is counted in `steps` and bounded by the model's
/// depth; a group's members are looked at whether or not the group itself changed.
pub(crate) fn rewrite_list(
    list: &ShapeList,
    change: &mut dyn FnMut(&Shape) -> Option<Shape>,
    steps: &mut u64,
    depth: usize,
) -> Result<Option<ShapeList>, &'static str> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(TOO_BIG);
    }
    let mut kept: Vec<Arc<Shape>> = Vec::new();
    let mut changed = false;
    for shape in list.iter() {
        *steps = steps.saturating_add(1);
        if *steps > MAX_STEPS {
            return Err(TOO_BIG);
        }
        let mut copy = change(shape);
        let members = match copy.as_ref().map_or(&shape.children, |own| &own.children) {
            children if children.is_empty() => None,
            children => rewrite_list(children, change, steps, depth.saturating_add(1))?,
        };
        if let Some(members) = members {
            let own = copy.get_or_insert_with(|| Shape::clone(shape));
            own.children = members;
        }
        match copy {
            Some(own) => {
                changed = true;
                kept.push(Arc::new(own));
            }
            None => kept.push(Arc::clone(shape)),
        }
    }
    Ok(changed.then(|| ShapeList::from_vec(kept)))
}

/// The drawing with `change` applied to each shape of its pages (and, when `masters`, of its masters), or `None` when it changes none. The copy shares everything it does not change.
pub(crate) fn rewrite_shapes(
    doc: &Document,
    masters: bool,
    change: &mut dyn FnMut(&Shape) -> Option<Shape>,
    steps: &mut u64,
) -> Result<Option<Document>, &'static str> {
    let mut out: Option<Document> = None;
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        if let Some(shapes) = rewrite_list(&page.shapes, change, steps, 0)? {
            let mut copy = Page::clone(page);
            copy.shapes = shapes;
            // (`at` is a place in the list.)
            let _ = out.get_or_insert_with(|| doc.clone()).pages.set(at, Arc::new(copy));
        }
    }
    if masters {
        for at in 0..doc.masters.len() {
            let Some(master) = doc.masters.get(at) else { continue };
            if let Some(shapes) = rewrite_list(&master.shapes, change, steps, 0)? {
                let mut copy = diacraft_doc::Master::clone(master);
                copy.shapes = shapes;
                let _ = out.get_or_insert_with(|| doc.clone()).masters.set(at, Arc::new(copy));
            }
        }
    }
    Ok(out)
}

/// What a shape of the other format that shows an object becomes: for an `.odg`'s frame written as a VSDX file, the shape without the element that names the object's entry (it shows its picture);
/// for a VSDX object written as an `.odg`, the shape as a picture of what it showed (its type is its picture's, and it names no relationship). `None` for a shape that shows none.
fn strip_one(shape: &Shape, target: Format) -> Option<Shape> {
    match target {
        Format::Vsdx if is_odf_object(shape) => {
            let mut own = Shape::clone(shape);
            let nodes: Vec<Placed> = shape
                .ext
                .get()
                .map(|data| data.odf.kept.kept.iter().filter(|placed| !is_object_node(&placed.node)).cloned().collect())
                .unwrap_or_default();
            own.ext.make_mut().odf.kept.kept = Passthrough::from_nodes(nodes);
            own.ext.tidy();
            Some(own)
        }
        Format::Odf if is_vsdx_object(shape) => {
            let mut own = Shape::clone(shape);
            if let Some(foreign) = &shape.foreign {
                let mut data = (**foreign).clone();
                let kind = data.picture.as_ref().and_then(|m| picture_type(&m.bytes));
                let pairs: Vec<(String, String)> = data
                    .attrs
                    .iter()
                    .filter(|(name, _)| *name != "ForeignType" || kind.is_some())
                    .map(|(name, value)| {
                        (name.to_owned(), if name == "ForeignType" { kind.map_or(value, |k| k).to_owned() } else { value.to_owned() })
                    })
                    .collect();
                data.attrs = RawAttrs::from_pairs(pairs);
                data.rel_id = None;
                own.foreign = Some(Arc::new(data));
            }
            Some(own)
        }
        _ => None,
    }
}

/// The drawing **without the objects of the other format** (`drop_objects`), and what was left out; `None` when it holds none. `target` is the format of the file being written: `Format::Vsdx` leaves
/// out the objects of an `.odg` (its object parts, and the elements of the frames that name them), `Format::Odf` those of a VSDX file (its embeddings, the relationships that name them, and the
/// type of the shapes that show them). **What is left of a shape is its picture.** The copy shares everything it does not change.
pub(crate) fn without_objects(doc: &Document, target: Format) -> Result<Option<(Document, Left)>, &'static str> {
    let other = if target == Format::Vsdx { Format::Odf } else { Format::Vsdx };
    let mut out = doc.clone();
    let mut left = Left::default();
    let mut steps = u64::try_from(doc.parts.len()).unwrap_or(u64::MAX);
    if steps > MAX_STEPS {
        return Err(TOO_BIG);
    }
    // The parts: the other format's object parts go.
    let gone: Vec<&Arc<Part>> = doc.parts.iter().filter(|p| p.format == other && is_object_part(p)).collect();
    if !gone.is_empty() {
        left.parts = gone.len();
        out.parts = SharedVec::from_vec(doc.parts.iter().filter(|p| !(p.format == other && is_object_part(p))).cloned().collect());
        // The relationships that name a part of a VSDX package that went (the document's own, the pages' and the masters').
        if other == Format::Vsdx
            && let Some(held) = &doc.vsdx
        {
            let names: HashSet<String> = gone.iter().map(|p| p.name.to_ascii_lowercase()).collect();
            steps = steps.saturating_add(held.rels.len() as u64);
            let rows: Vec<_> = held
                .rels
                .iter()
                .filter(|r| r.external || !resolve(&r.source, &r.target).is_some_and(|name| names.contains(&name.to_ascii_lowercase())))
                .cloned()
                .collect();
            if rows.len() != held.rels.len() {
                out.vsdx = Some(Arc::new(VsdxHeld { rels: rows, ..VsdxHeld::clone(held) }));
            }
        }
    }
    // The shapes of the pages and the masters.
    let mut shapes = 0u64;
    let mut change = |shape: &Shape| {
        let own = strip_one(shape, target);
        shapes = shapes.saturating_add(u64::from(own.is_some()));
        own
    };
    if let Some(stripped) = rewrite_shapes(&out, true, &mut change, &mut steps)? {
        out = stripped;
    }
    left.shapes = shapes;
    Ok((left != Left::default()).then_some((out, left)))
}
