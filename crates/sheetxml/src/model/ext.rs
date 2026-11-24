//! What the model holds of the two formats beyond its own fields (the types): the `ext` slot of each object, the parts, relationships and
//! content-type rows of a package, the custom properties, and each ODF type with its held values, as the elements of `vocab.rs` and back.
//!
//! **The slot** ([`Ext`]) is one `ext` child of its object. It holds an `odf` element (the object's ODF data, which the owner's type says how to write)
//! and a `vsdx` element (the VSDX elements the object had no slot for, a `rest` for each, by its key), and nothing when the slot holds only defaults.
//! **What a newer build wrote** (`ext.native`) is not in the `ext` element: it is the object's own unknown attributes and child elements, back where they
//! were, written as the XML they were read as (the names with the declarations they use), so that a newer build reads them as its own. A slot's
//! `native` content that no known child follows is written after them; one that follows `n` known children is written after `n` of them.
//!
//! **A record with no slot** (a colour entry, a part, a relationship, a content-type row, a custom property, a definition, a held value) keeps its unknown
//! content in the native slot of its owner (the document, or the shape or row that holds the value), as a kept element named `kind[key]` (NP7). The
//! record takes it back when it is written; one whose record is gone is dropped.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    ContentTypeRow, CustomProp, DefFamily, Definition, Ext, ExtData, Format, Held, HeldGroup, HeldStyleValue, Kept, KeptStyle, MAX_DEFINITIONS,
    MAX_LIST_LEVELS, OdfDoc, OdfListLevel, OdfPage, OdfParagraph, OdfRow, OdfRun, OdfShape, OdfStyle, OdfText, OdfTextStyle, Part, PartRole,
    Passthrough, RawAttrs, RawElement, RawNode, Rel, TextGroup, VsdxHeld, VsdxRest,
};

use super::decode::{Dec, Ours, limit};
use super::encode::{Encoder, MAX_KEPT_DEPTH};
use super::escape::{ATTR_WRITTEN_MAX, attr_written_len, comment_writable, pi_writable};
use super::kept::Rest;
use super::stamp::Stamp;
use super::vocab::{at, el};
use super::{CodecError, PREFIX};
use crate::ns::Scope;

/// The name a record's unknown content is kept under in its owner's native slot: `kind[key]` (NP7). An XML name has no `[`, so the name tells such an entry
/// from an unknown element of the owner's own.
pub(super) fn keyed_name(kind: &str, key: &str) -> String {
    format!("{kind}[{key}]")
}

pub(super) fn is_keyed(name: &str) -> bool {
    name.ends_with(']') && name.contains('[')
}

/// The unknown content of records, by key, for the records of one owner to take back as they are written.
pub(super) type Keyed<'k> = HashMap<&'k str, &'k RawElement>;

/// How many values of each group a list has held so far, for the ordinal that keys the next one's unknown content (Call 24).
///
/// NP7 keys a record by what it is ("never its position") when it has an identity of its own: a shape by its ID, a row by its name or `IX`, a part by its
/// name. Several held values of one group have none, and the model allows them (an `OdfShape` holds a list), so the key of a value is its group and its
/// **ordinal within its owner's list of that group**: `held[fill.0]` and `held[fill.1]` are two values, and each keeps what a newer build wrote on it. The same
/// holds for the other lists with no identity: the paragraphs of a text, the runs of a paragraph (keyed by their ordinals) and the held values of a style.
#[derive(Default)]
pub(super) struct Ordinals(HashMap<&'static str, usize>);

impl Ordinals {
    /// The ordinal of the next value of `group`, which is then counted.
    pub(super) fn next(&mut self, group: &'static str) -> usize {
        let count = self.0.entry(group).or_insert(0);
        let ordinal = *count;
        *count = count.saturating_add(1);
        ordinal
    }
}

const FORMATS: [(&str, Format); 3] = [("vsdx", Format::Vsdx), ("odf", Format::Odf), ("native", Format::Native)];
const ROLES: [(&str, PartRole); 11] = [
    ("thumbnail", PartRole::Thumbnail),
    ("macro-project", PartRole::MacroProject),
    ("macro-signature", PartRole::MacroSignature),
    ("macro-data", PartRole::MacroData),
    ("embedding", PartRole::Embedding),
    ("object-file", PartRole::ObjectFile),
    ("object-replacement", PartRole::ObjectReplacement),
    ("signature", PartRole::Signature),
    ("custom-xml", PartRole::CustomXml),
    ("font", PartRole::Font),
    ("other", PartRole::Other),
];
const GROUPS: [(&str, HeldGroup); 8] = [
    ("fill", HeldGroup::Fill),
    ("line", HeldGroup::Line),
    ("line-ends", HeldGroup::LineEnds),
    ("shadow", HeldGroup::Shadow),
    ("text", HeldGroup::Text),
    ("frame", HeldGroup::Frame),
    ("glue", HeldGroup::Glue),
    ("geometry", HeldGroup::Geometry),
];
const FAMILIES: [(&str, DefFamily); 6] = [
    ("marker", DefFamily::Marker),
    ("gradient", DefFamily::Gradient),
    ("opacity", DefFamily::Opacity),
    ("hatch", DefFamily::Hatch),
    ("stroke-dash", DefFamily::StrokeDash),
    ("fill-image", DefFamily::FillImage),
];

/// The place of a family among the six, and the words `DocError` names its cap in (`Document::check_limits` gives the same refusal at the door).
fn family_cap(family: DefFamily) -> (usize, &'static str) {
    match family {
        DefFamily::Marker => (0, "marker definitions"),
        DefFamily::Gradient => (1, "gradient definitions"),
        DefFamily::Opacity => (2, "opacity definitions"),
        DefFamily::Hatch => (3, "hatch definitions"),
        DefFamily::StrokeDash => (4, "stroke dash definitions"),
        DefFamily::FillImage => (5, "fill image definitions"),
    }
}

fn word_of<T: Copy + PartialEq>(table: &[(&'static str, T)], value: T) -> &'static str {
    table.iter().find(|(_, v)| *v == value).map_or("", |(w, _)| *w)
}

fn value_of<T: Copy>(table: &[(&'static str, T)], word: &str) -> Option<T> {
    table.iter().find(|(w, _)| *w == word).map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------------------------------------------------- encoding

impl Encoder<'_> {
    /// A wrapper element of `raw`'s `attr` children, when there are any.
    fn bag(&mut self, parent: &mut El, wrapper: &str, raw: &RawAttrs) -> Result<(), CodecError> {
        if raw.is_empty() {
            return Ok(());
        }
        let mut w = self.elem(wrapper);
        self.raw_attrs(&mut w, raw)?;
        self.add(parent, w)
    }

    fn kept_into(&mut self, e: &mut El, k: &Kept) -> Result<(), CodecError> {
        self.raw_attrs(e, &k.raw)?;
        self.kept(e, &k.kept)
    }

    /// A kept element in a wrapper of its own, when there is one.
    fn element_in(&mut self, parent: &mut El, wrapper: &str, element: Option<&RawElement>) -> Result<(), CodecError> {
        if let Some(inner) = element {
            let mut w = self.elem(wrapper);
            let e = self.raw_element_el(inner)?;
            self.add(&mut w, e)?;
            self.add(parent, w)?;
        }
        Ok(())
    }

    /// The `ext` child of an object whose slot holds something: the ODF data (written by `odf` into the `odf` element it is given), and the VSDX elements
    /// the object had no slot for. What a newer build wrote is not here: [`Encoder::native_end`] writes it, last.
    pub(super) fn ext_slot<T>(
        &mut self,
        e: &mut El,
        slot: &Ext<T>,
        odf: impl FnOnce(&mut Self, &T, &mut El) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
    {
        self.ext_slot_with(e, slot, odf, false)
    }

    /// [`Encoder::ext_slot`] for an object whose own children are all its content (a text's parts): what a newer build wrote beside them is in the `ext`
    /// element, as unknown elements after as many of its `odf` and `vsdx` children as they followed, and the `ext` element is written when it holds only that.
    pub(super) fn ext_slot_native_inside<T>(
        &mut self,
        e: &mut El,
        slot: &Ext<T>,
        odf: impl FnOnce(&mut Self, &T, &mut El) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
    {
        self.ext_slot_with(e, slot, odf, true)
    }

    fn ext_slot_with<T>(
        &mut self,
        e: &mut El,
        slot: &Ext<T>,
        odf: impl FnOnce(&mut Self, &T, &mut El) -> Result<(), CodecError>,
        native_inside: bool,
    ) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
    {
        let Some(data) = slot.get() else { return Ok(()) };
        let ExtData { odf: odf_data, vsdx, native } = data;
        let mut x = self.elem(el::EXT);
        if *odf_data != T::default() {
            let mut o = self.elem(el::ODF);
            odf(self, odf_data, &mut o)?;
            self.add(&mut x, o)?;
        }
        if !vsdx.0.is_empty() {
            let mut v = self.elem(el::VSDX);
            for (key, k) in &vsdx.0 {
                let mut r = self.elem(el::REST);
                self.put_req(&mut r, at::KEY, key)?;
                self.kept_into(&mut r, k)?;
                self.add(&mut v, r)?;
            }
            self.add(&mut x, v)?;
        }
        if native_inside {
            self.native_nodes_end(&mut x, Some(native))?;
        }
        if !x.children.is_empty() {
            self.add(e, x)?;
        }
        Ok(())
    }

    /// The slot of an `Ext<Kept>`: its ODF side is a bag of attributes and nodes.
    pub(super) fn ext_kept(&mut self, e: &mut El, slot: &Ext<Kept>) -> Result<(), CodecError> {
        self.ext_slot(e, slot, |enc, k, o| enc.kept_into(o, k))
    }

    // ------------------------------------------------------------------------------------------------------------ what a newer build wrote

    /// A node of unknown content as the XML it was read as (its names, with the declarations they use, are in it).
    pub(super) fn literal(&mut self, node: &RawNode) -> Result<Node, CodecError> {
        match node {
            RawNode::Text(t) => {
                self.guard_bytes(t.len())?;
                self.count(1)?;
                self.count_bytes(u64::try_from(t.len()).unwrap_or(u64::MAX))?;
                Ok(Node::Text(t.to_string()))
            }
            RawNode::Comment(c) => {
                if !comment_writable(c) {
                    return Err(CodecError::malformed("document", "an unknown comment that XML cannot write"));
                }
                self.guard_bytes(c.len())?;
                self.count(1)?;
                self.count_bytes(u64::try_from(c.len()).unwrap_or(u64::MAX))?;
                Ok(Node::Comment(c.to_string()))
            }
            RawNode::Pi { target, data } => {
                if !pi_writable(target, data) {
                    return Err(CodecError::malformed("document", "an unknown processing instruction that XML cannot write"));
                }
                self.guard_bytes(target.len().saturating_add(data.len()))?;
                self.count(1)?;
                self.count_bytes(u64::try_from(target.len().saturating_add(data.len())).unwrap_or(u64::MAX))?;
                Ok(Node::Pi { target: target.to_string(), data: data.to_string() })
            }
            RawNode::Element(root) => self.literal_element(root).map(Node::El),
        }
    }

    /// An unknown element and all that is in it, with a stack of its own, at most [`MAX_KEPT_DEPTH`] levels deep.
    fn literal_element(&mut self, root: &RawElement) -> Result<El, CodecError> {
        struct Frame<'r> {
            el: El,
            rest: std::slice::Iter<'r, RawNode>,
        }
        fn open(e: &RawElement) -> Result<Frame<'_>, CodecError> {
            if e.attrs.iter().any(|(_, v)| attr_written_len(v) > ATTR_WRITTEN_MAX) {
                return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
            }
            Ok(Frame {
                el: El { name: e.name.to_string(), attrs: e.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(), children: Vec::new() },
                rest: e.children.iter(),
            })
        }
        let mut stack: Vec<Frame<'_>> = vec![open(root)?];
        loop {
            let step = match stack.last_mut() {
                None => return Err(CodecError::malformed("document", "an unknown element with no frame")),
                Some(top) => top.rest.next(),
            };
            match step {
                Some(RawNode::Element(e)) => {
                    if stack.len() >= MAX_KEPT_DEPTH {
                        return Err(CodecError::Limit { what: "levels of a kept element", max: u64::try_from(MAX_KEPT_DEPTH).unwrap_or(u64::MAX) });
                    }
                    stack.push(open(e)?);
                }
                Some(other) => {
                    let n = self.literal(other)?;
                    if let Some(top) = stack.last_mut() {
                        top.el.children.push(n);
                    }
                }
                None => {
                    let Some(done) = stack.pop() else { return Err(CodecError::malformed("document", "an unknown element with no frame")) };
                    self.count(1u64.saturating_add(u64::try_from(done.el.attrs.len()).unwrap_or(u64::MAX)))?;
                    self.count_bytes(super::encode::element_floor(&done.el))?;
                    match stack.last_mut() {
                        Some(parent) => parent.el.children.push(Node::El(done.el)),
                        None => return Ok(done.el),
                    }
                }
            }
        }
    }

    /// What a newer build wrote on this object and this build does not know, back where it was: the attributes after the ones the codec wrote, and each node
    /// after as many of the object's known child elements as it followed. Called last, when every known child is in place. A keyed entry (the unknown content of
    /// a record, NP7) is not written here: the record takes it.
    pub(super) fn native_end(&mut self, e: &mut El, native: Option<&Kept>) -> Result<(), CodecError> {
        self.native_attrs_end(e, native)?;
        self.native_nodes_end(e, native)
    }

    /// The attributes of [`Encoder::native_end`] alone.
    pub(super) fn native_attrs_end(&mut self, e: &mut El, native: Option<&Kept>) -> Result<(), CodecError> {
        let Some(k) = native else { return Ok(()) };
        for (name, value) in k.raw.iter() {
            self.push_native_attr(e, name, value)?;
        }
        Ok(())
    }

    /// An unknown attribute put on an element of the codec's own, as the XML it was: its value held to what the reader allows an attribute as written (a
    /// model another reader made can hold any size), and the declaration of the codec's own prefix not repeated, for the part declares it once (bound to
    /// another namespace it is refused: the part could not be written, and read back its elements would be in that namespace).
    pub(super) fn push_native_attr(&self, e: &mut El, name: &str, value: &str) -> Result<(), CodecError> {
        if name == format!("xmlns:{PREFIX}") {
            return if diacraft_doc::is_diacraft_urn(value) {
                Ok(())
            } else {
                Err(CodecError::malformed("document", "the codec's own prefix bound to another namespace on an attribute"))
            };
        }
        if attr_written_len(value) > ATTR_WRITTEN_MAX {
            return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
        }
        e.attrs.push((name.to_owned(), value.to_owned()));
        Ok(())
    }

    /// The nodes of [`Encoder::native_end`] alone.
    pub(super) fn native_nodes_end(&mut self, e: &mut El, native: Option<&Kept>) -> Result<(), CodecError> {
        let Some(k) = native else { return Ok(()) };
        let mut nodes: Vec<(u32, &RawNode)> =
            k.kept.iter().filter(|p| !matches!(&p.node, RawNode::Element(r) if is_keyed(&r.name))).map(|p| (p.after, &p.node)).collect();
        if nodes.is_empty() {
            return Ok(());
        }
        nodes.sort_by_key(|(after, _)| *after);
        let old = std::mem::take(&mut e.children);
        let mut out: Vec<Node> = Vec::with_capacity(old.len().saturating_add(nodes.len()));
        let mut next = nodes.into_iter().peekable();
        let mut elements = 0u32;
        for child in old {
            if matches!(child, Node::El(_)) {
                while let Some((_, n)) = next.next_if(|(after, _)| *after <= elements) {
                    out.push(self.literal(n)?);
                }
                elements = elements.saturating_add(1);
            }
            out.push(child);
        }
        for (_, n) in next {
            out.push(self.literal(n)?);
        }
        e.children = out;
        Ok(())
    }

    /// The keyed unknown content of an owner's native slot, by key: what the records of that owner take back as they are written.
    pub(super) fn keyed_unknowns(native: Option<&Kept>) -> Keyed<'_> {
        let mut map = HashMap::new();
        if let Some(k) = native {
            for p in k.kept.iter() {
                if let RawNode::Element(r) = &p.node
                    && is_keyed(&r.name)
                {
                    map.entry(&*r.name).or_insert(r);
                }
            }
        }
        map
    }

    /// Puts a record's keyed unknown content back into the record: its attributes after its own, its nodes after its own children.
    pub(super) fn take_keyed(&mut self, e: &mut El, keyed: &mut Keyed<'_>, kind: &str, key: &str) -> Result<(), CodecError> {
        if keyed.is_empty() {
            return Ok(());
        }
        let Some(r) = keyed.remove(keyed_name(kind, key).as_str()) else { return Ok(()) };
        for (name, value) in r.attrs.iter() {
            self.push_native_attr(e, name, value)?;
        }
        for node in &r.children {
            let n = self.literal(node)?;
            e.children.push(n);
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------------------------ the package-level records

    pub(super) fn rel(&mut self, r: &Rel, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let Rel { source, id, kind, target, external } = r;
        let mut e = self.elem(el::REL);
        self.put_req(&mut e, at::SOURCE, source)?;
        self.put_req(&mut e, at::ID, id)?;
        self.put_req(&mut e, at::KIND, kind)?;
        self.put_req(&mut e, at::TARGET, target)?;
        Self::put_flag(&mut e, at::EXTERNAL, *external);
        self.take_keyed(&mut e, keyed, "rel", &format!("{source}|{id}"))?;
        Ok(e)
    }

    fn content_type_row(&mut self, row: &ContentTypeRow, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let mut e = self.elem(el::CONTENT_TYPE);
        match row {
            ContentTypeRow::Default { extension, content_type } => {
                self.put_str(&mut e, at::EXTENSION, extension)?;
                self.put_req(&mut e, at::CONTENT_TYPE, content_type)?;
                self.take_keyed(&mut e, keyed, "content-type", &format!("default:{extension}"))?;
            }
            ContentTypeRow::Override { part, content_type } => {
                self.put_str(&mut e, at::PART, part)?;
                self.put_req(&mut e, at::CONTENT_TYPE, content_type)?;
                self.take_keyed(&mut e, keyed, "content-type", &format!("override:{part}"))?;
            }
        }
        Ok(e)
    }

    pub(super) fn vsdx_held(&mut self, h: &VsdxHeld, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let VsdxHeld { rels, content_types, props_rest, windows } = h;
        let mut e = self.elem(el::VSDX_HELD);
        for r in rels {
            let r = self.rel(r, keyed)?;
            self.add(&mut e, r)?;
        }
        for c in content_types {
            let c = self.content_type_row(c, keyed)?;
            self.add(&mut e, c)?;
        }
        if !props_rest.is_empty() {
            let mut p = self.elem(el::PROPS_REST);
            for (key, k) in props_rest {
                let mut r = self.elem(el::REST);
                self.put_req(&mut r, at::KEY, key)?;
                self.kept_into(&mut r, k)?;
                self.add(&mut p, r)?;
            }
            self.add(&mut e, p)?;
        }
        self.element_in(&mut e, el::WINDOWS, windows.as_deref())?;
        Ok(e)
    }

    pub(super) fn part(&mut self, p: &Part, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let Part { format, name, media_type, role, manifest, rels, bytes } = p;
        let mut e = self.elem(el::PART);
        e.attrs.push((at::FORMAT.to_owned(), word_of(&FORMATS, *format).to_owned()));
        self.put_req(&mut e, at::NAME, name)?;
        self.put_req(&mut e, at::MEDIA_TYPE, media_type)?;
        e.attrs.push((at::ROLE.to_owned(), word_of(&ROLES, *role).to_owned()));
        let id = self.blob(bytes, media_type)?;
        e.attrs.push((at::BLOB.to_owned(), id.0.to_string()));
        self.bag(&mut e, el::MANIFEST, manifest)?;
        for r in rels.iter() {
            let r = self.rel(r, keyed)?;
            self.add(&mut e, r)?;
        }
        self.take_keyed(&mut e, keyed, "part", name)?;
        Ok(e)
    }

    pub(super) fn custom_prop(&mut self, c: &CustomProp, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let CustomProp { format, name, value_type, value, raw } = c;
        let mut e = self.elem(el::CUSTOM);
        e.attrs.push((at::FORMAT.to_owned(), word_of(&FORMATS, *format).to_owned()));
        self.put_req(&mut e, at::NAME, name)?;
        self.put_req(&mut e, at::VALUE_TYPE, value_type)?;
        self.put_req(&mut e, at::VALUE, value)?;
        self.raw_attrs(&mut e, raw)?;
        self.take_keyed(&mut e, keyed, "custom", &format!("{}:{name}", word_of(&FORMATS, *format)))?;
        Ok(e)
    }

    // ------------------------------------------------------------------------------------------------------------ ODF

    pub(super) fn held(&mut self, h: &Held, keyed: &mut Keyed<'_>, ordinal: usize) -> Result<El, CodecError> {
        let Held { group, stamp, attrs, kept } = h;
        let mut e = self.elem(el::HELD);
        e.attrs.push((at::GROUP.to_owned(), word_of(&GROUPS, *group).to_owned()));
        // The frame's value and a circle's centre form have no stamp: absent, as it is in the model.
        if let Some(stamp) = stamp {
            e.attrs.push((at::STAMP.to_owned(), stamp.text()));
        }
        self.raw_attrs(&mut e, attrs)?;
        self.kept(&mut e, kept)?;
        self.take_keyed(&mut e, keyed, "held", &format!("{}.{ordinal}", word_of(&GROUPS, *group)))?;
        Ok(e)
    }

    fn definition(&mut self, d: &Definition, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let Definition { family, name, element, bytes } = d;
        let mut e = self.elem(el::DEFINITION);
        e.attrs.push((at::FAMILY.to_owned(), word_of(&FAMILIES, *family).to_owned()));
        self.put_req(&mut e, at::NAME, name)?;
        if let Some(b) = bytes {
            let id = self.blob(b, "application/octet-stream")?;
            e.attrs.push((at::BLOB.to_owned(), id.0.to_string()));
        }
        let inner = self.raw_element_el(element)?;
        self.add(&mut e, inner)?;
        self.take_keyed(&mut e, keyed, "definition", &format!("{}:{name}", word_of(&FAMILIES, *family)))?;
        Ok(e)
    }

    fn kept_style(&mut self, s: &KeptStyle, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let KeptStyle { family, name, element } = s;
        let mut e = self.elem(el::KEPT_STYLE);
        self.put_req(&mut e, at::FAMILY, family)?;
        self.put_req(&mut e, at::NAME, name)?;
        let inner = self.raw_element_el(element)?;
        self.add(&mut e, inner)?;
        self.take_keyed(&mut e, keyed, "kept-style", &format!("{family}:{name}"))?;
        Ok(e)
    }

    pub(super) fn odf_doc(&mut self, d: &OdfDoc, o: &mut El, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfDoc { kept, mimetype, roots, manifest_rows, meta, settings, definitions, layers, styles } = d;
        self.put_opt(o, at::MIMETYPE, mimetype.as_ref())?;
        self.kept_into(o, kept)?;
        for (name, attrs) in roots {
            let mut r = self.elem(el::ODF_ROOT);
            self.put_req(&mut r, at::NAME, name)?;
            self.raw_attrs(&mut r, attrs)?;
            self.take_keyed(&mut r, keyed, "root", name)?;
            self.add(o, r)?;
        }
        for (name, attrs) in manifest_rows {
            let mut r = self.elem(el::MANIFEST_ROW);
            self.put_req(&mut r, at::NAME, name)?;
            self.raw_attrs(&mut r, attrs)?;
            self.take_keyed(&mut r, keyed, "manifest-row", name)?;
            self.add(o, r)?;
        }
        if !meta.is_empty() {
            let mut m = self.elem(el::META);
            self.kept(&mut m, meta)?;
            self.add(o, m)?;
        }
        self.element_in(o, el::SETTINGS, settings.as_deref())?;
        for def in definitions {
            let e = self.definition(def, keyed)?;
            self.add(o, e)?;
        }
        for layer in layers {
            self.element_in(o, el::LAYER, Some(layer))?;
        }
        for s in styles {
            let e = self.kept_style(s, keyed)?;
            self.add(o, e)?;
        }
        Ok(())
    }

    pub(super) fn odf_page(&mut self, p: &OdfPage, o: &mut El) -> Result<(), CodecError> {
        let OdfPage { kept, master_page, layout, master_page_attrs, layout_props, drawing_page_style, units, stamp } = p;
        self.put_opt(o, at::MASTER_PAGE, master_page.as_ref())?;
        self.put_opt(o, at::LAYOUT, layout.as_ref())?;
        if let Some(s) = stamp {
            o.attrs.push((at::STAMP.to_owned(), s.text()));
        }
        self.kept_into(o, kept)?;
        self.bag(o, el::MASTER_PAGE_ATTRS, master_page_attrs)?;
        self.bag(o, el::LAYOUT_PROPS, layout_props)?;
        self.element_in(o, el::DRAWING_PAGE_STYLE, drawing_page_style.as_deref())?;
        self.bag(o, el::UNITS, units)
    }

    pub(super) fn odf_style(&mut self, s: &OdfStyle, o: &mut El, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfStyle { kept, name, display_name, parent, family, unmapped, units, held, text_held } = s;
        self.put_opt(o, at::NAME, name.as_ref())?;
        self.put_opt(o, at::DISPLAY_NAME, display_name.as_ref())?;
        self.put_opt(o, at::PARENT, parent.as_ref())?;
        self.put_opt(o, at::FAMILY, family.as_ref())?;
        self.kept_into(o, kept)?;
        self.bag(o, el::UNMAPPED, unmapped)?;
        self.bag(o, el::UNITS, units)?;
        let mut ordinals = Ordinals::default();
        for h in held {
            let ordinal = ordinals.next(word_of(&GROUPS, h.group));
            let e = self.held(h, keyed, ordinal)?;
            self.add(o, e)?;
        }
        // The values of the style's text properties are keyed by `style` and their group and ordinal (a paragraph's or a run's by its own ordinals).
        let mut text_ordinals = Ordinals::default();
        for value in text_held {
            let e = self.held_style(value, "style", &mut text_ordinals, keyed)?;
            self.add(o, e)?;
        }
        Ok(())
    }

    pub(super) fn odf_shape(&mut self, s: &OdfShape, o: &mut El, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfShape { kept, style_name, held, units } = s;
        self.put_opt(o, at::STYLE_NAME, style_name.as_ref())?;
        self.kept_into(o, kept)?;
        let mut ordinals = Ordinals::default();
        for h in held {
            let ordinal = ordinals.next(word_of(&GROUPS, h.group));
            let e = self.held(h, keyed, ordinal)?;
            self.add(o, e)?;
        }
        self.bag(o, el::UNITS, units)
    }

    pub(super) fn odf_row(&mut self, r: &OdfRow, o: &mut El, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfRow { kept, style_name, held } = r;
        self.put_opt(o, at::STYLE_NAME, style_name.as_ref())?;
        self.kept_into(o, kept)?;
        if let Some(h) = held {
            // A row holds one value: it is the first of its group.
            let e = self.held(h, keyed, 0)?;
            self.add(o, e)?;
        }
        Ok(())
    }

    /// A text's ODF data: what its markup kept, and its paragraphs by ordinal. The unknown content of a record with no slot is kept under its kind and a
    /// key made of the ordinals (`paragraph[p]`, `run[p.r]`, `text-style[p]` or `text-style[p.r]`, `held-style[p.group.n]` or `held-style[p.r.group.n]`, where `n` is the ordinal of the value among those of its group in its style: Call 24, see [`Ordinals`]).
    pub(super) fn odf_text(&mut self, t: &OdfText, o: &mut El, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfText { kept, paragraphs } = t;
        self.kept_into(o, kept)?;
        for (p, paragraph) in paragraphs.iter().enumerate() {
            let OdfParagraph { style, characters, runs, element, levels } = paragraph;
            let key = p.to_string();
            let mut e = self.elem(el::PARAGRAPH);
            if let Some(stamp) = characters {
                e.attrs.push((at::CHARACTERS.to_owned(), stamp.text()));
            }
            self.text_style(&mut e, style, &key, keyed)?;
            self.element_in(&mut e, el::ELEMENT, element.as_deref())?;
            for level in levels {
                let OdfListLevel { list, item } = level;
                let mut l = self.elem(el::LEVEL);
                self.element_in(&mut l, el::LIST, list.as_deref())?;
                self.element_in(&mut l, el::ITEM, item.as_deref())?;
                self.add(&mut e, l)?;
            }
            for (r, run) in runs.iter().enumerate() {
                let OdfRun { from, to, style, element } = run;
                let key = format!("{p}.{r}");
                let mut re = self.elem(el::RUN);
                Self::put_u32(&mut re, at::FROM, *from, 0);
                Self::put_u32(&mut re, at::TO, *to, 0);
                self.text_style(&mut re, style, &key, keyed)?;
                self.element_in(&mut re, el::ELEMENT, element.as_deref())?;
                self.take_keyed(&mut re, keyed, "run", &key)?;
                self.add(&mut e, re)?;
            }
            self.take_keyed(&mut e, keyed, "paragraph", &key)?;
            self.add(o, e)?;
        }
        Ok(())
    }

    /// The `text-style` child of a paragraph or a run, when the style names something, has a stamp, holds a value or has unknown content kept for it.
    fn text_style(&mut self, parent: &mut El, style: &OdfTextStyle, key: &str, keyed: &mut Keyed<'_>) -> Result<(), CodecError> {
        let OdfTextStyle { name, stamp, held } = style;
        if *style == OdfTextStyle::default() && !keyed.contains_key(keyed_name("text-style", key).as_str()) {
            return Ok(());
        }
        let mut e = self.elem(el::TEXT_STYLE);
        self.put_opt(&mut e, at::NAME, name.as_ref())?;
        if let Some(stamp) = stamp {
            e.attrs.push((at::STAMP.to_owned(), stamp.text()));
        }
        let mut ordinals = Ordinals::default();
        for value in held {
            let h = self.held_style(value, key, &mut ordinals, keyed)?;
            self.add(&mut e, h)?;
        }
        self.take_keyed(&mut e, keyed, "text-style", key)?;
        self.add(parent, e)
    }

    /// A `held-style` element: one value of a text style, with the group of cells it rests on, its stamp and the attributes and nodes as the file had them. `key` is
    /// its owner's (`p`, `p.r` or `style`), and its unknown content is kept under that, its group and its ordinal among the values of the group.
    fn held_style(&mut self, value: &HeldStyleValue, key: &str, ordinals: &mut Ordinals, keyed: &mut Keyed<'_>) -> Result<El, CodecError> {
        let HeldStyleValue { group, stamp, attrs, kept } = value;
        let ordinal = ordinals.next(group.id());
        let mut h = self.elem(el::HELD_STYLE);
        h.attrs.push((at::GROUP.to_owned(), group.id().to_owned()));
        if let Some(stamp) = stamp {
            h.attrs.push((at::STAMP.to_owned(), stamp.text()));
        }
        self.raw_attrs(&mut h, attrs)?;
        self.kept(&mut h, kept)?;
        self.take_keyed(&mut h, keyed, "held-style", &format!("{key}.{}.{ordinal}", group.id()))?;
        Ok(h)
    }
}

// ---------------------------------------------------------------------------------------------------------------------- decoding

impl Dec<'_> {
    /// The kept element a wrapper holds (`settings`, `windows`, `drawing-page-style`, `layer`): its one `el` child.
    fn wrapped_element(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Arc<RawElement>>, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut found = None;
        self.children(e, sc, None, |d, child, local, csc| {
            if local == el::KEPT_EL && found.is_none() {
                found = Some(Arc::new(d.raw_element(child, csc)?));
                return Ok(true);
            }
            Ok(false)
        })?;
        Ok(found)
    }

    /// The attributes a wrapper element holds as `attr` children (`units`, `manifest`, `unmapped`, ...).
    fn bag_of(&mut self, e: &El, sc: &Scope<Ours>) -> Result<RawAttrs, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut rest = Rest::default();
        self.children(e, sc, None, |d, child, local, csc| if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) })?;
        Ok(rest.raw)
    }

    /// The `kept` children of a wrapper (`meta`).
    fn kept_list(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Passthrough, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut rest = Rest::default();
        self.children(e, sc, None, |d, child, local, csc| if local == el::KEPT { d.rest_child(&mut rest, child, csc) } else { Ok(false) })?;
        Ok(rest.kept)
    }

    /// A `rest` element: its key and what it keeps.
    fn rest_entry(&mut self, e: &El, sc: &Scope<Ours>) -> Result<(Arc<str>, Kept), CodecError> {
        let mut key = String::new();
        self.fields(e, sc, |_, n, v| {
            if n == at::KEY {
                key = v.into_owned();
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut rest = Rest::default();
        self.children(e, sc, None, |d, child, _, csc| d.rest_child(&mut rest, child, csc))?;
        Ok((Arc::from(key.as_str()), Kept { raw: rest.raw, kept: rest.kept }))
    }

    /// The entries of a wrapper of `rest` elements.
    fn rest_entries(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Vec<(Arc<str>, Kept)>, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut entries = Vec::new();
        self.children(e, sc, None, |d, child, local, csc| {
            if local == el::REST {
                entries.push(d.rest_entry(child, csc)?);
                return Ok(true);
            }
            Ok(false)
        })?;
        Ok(entries)
    }

    /// The `ext` child of an object: its ODF data (read by `odf`, which is given the `odf` element and its scope), and its VSDX rest.
    pub(super) fn ext_child<T, F>(&mut self, e: &El, sc: &Scope<Ours>, slot: &mut Ext<T>, odf: F) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
        F: FnMut(&mut Dec<'_>, &El, &Scope<Ours>, &mut T) -> Result<(), CodecError>,
    {
        self.ext_child_with(e, sc, slot, None, odf)
    }

    /// [`Dec::ext_child`] for an owner whose own children are its content (a text): an element in the `ext` that this build does not know is what a newer
    /// build wrote beside them, and goes to `native` with the number of known children before it.
    pub(super) fn ext_child_native<T, F>(
        &mut self,
        e: &El,
        sc: &Scope<Ours>,
        slot: &mut Ext<T>,
        native: &mut Passthrough,
        odf: F,
    ) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
        F: FnMut(&mut Dec<'_>, &El, &Scope<Ours>, &mut T) -> Result<(), CodecError>,
    {
        self.ext_child_with(e, sc, slot, Some(native), odf)
    }

    fn ext_child_with<T, F>(
        &mut self,
        e: &El,
        sc: &Scope<Ours>,
        slot: &mut Ext<T>,
        native: Option<&mut Passthrough>,
        mut odf: F,
    ) -> Result<(), CodecError>
    where
        T: Default + Clone + PartialEq,
        F: FnMut(&mut Dec<'_>, &El, &Scope<Ours>, &mut T) -> Result<(), CodecError>,
    {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let (mut odf_seen, mut vsdx_seen) = (false, false);
        self.children(e, sc, native, |d, child, local, csc| {
            match local {
                // The first of each is read: the unknown content of the records of one that another replaces would be left with none.
                el::ODF | el::VSDX if (local == el::ODF && odf_seen) || (local == el::VSDX && vsdx_seen) => d.unknown_el(child)?,
                el::ODF => {
                    odf_seen = true;
                    odf(d, child, csc, &mut slot.make_mut().odf)?;
                }
                el::VSDX => {
                    vsdx_seen = true;
                    slot.make_mut().vsdx = VsdxRest(d.rest_entries(child, csc)?);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        slot.tidy();
        Ok(())
    }

    /// The `odf` element of an `Ext<Kept>`: attributes and nodes, kept.
    pub(super) fn odf_kept(&mut self, e: &El, sc: &Scope<Ours>, out: &mut Kept) -> Result<(), CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut rest = Rest::default();
        self.children(e, sc, None, |d, child, _, csc| d.rest_child(&mut rest, child, csc))?;
        *out = Kept { raw: rest.raw, kept: rest.kept };
        Ok(())
    }

    /// Takes what a record kept unknown (its attributes from the last `fields`, and the nodes) into the owner's list, under the record's kind and key.
    pub(super) fn finish_record(&mut self, attrs: RawAttrs, nodes: Passthrough, kind: &str, key: &str) {
        // Every record counts, so that a later record of the same key is known for one.
        let first = self.keyed.first(kind, key);
        if attrs.is_empty() && nodes.is_empty() {
            return;
        }
        if !first {
            self.saw_unknown();
            return;
        }
        let children = nodes.iter().map(|p| p.node.clone()).collect();
        self.keyed.entries.push(RawElement { name: Arc::from(keyed_name(kind, key).as_str()), attrs, children });
    }

    // ------------------------------------------------------------------------------------------------------------ the package-level records

    pub(super) fn rel(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Rel, CodecError> {
        let (mut source, mut id, mut kind, mut target, mut external) = (String::new(), String::new(), String::new(), String::new(), false);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::SOURCE => source = v.into_owned(),
                at::ID => id = v.into_owned(),
                at::KIND => kind = v.into_owned(),
                at::TARGET => target = v.into_owned(),
                at::EXTERNAL => external = d.flag(&v)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.record_unknowns(e, sc, "rel", &format!("{source}|{id}"))?;
        Ok(Rel {
            source: Arc::from(source.as_str()),
            id: Arc::from(id.as_str()),
            kind: Arc::from(kind.as_str()),
            target: Arc::from(target.as_str()),
            external,
        })
    }

    fn content_type_row(&mut self, e: &El, sc: &Scope<Ours>) -> Result<ContentTypeRow, CodecError> {
        let (mut extension, mut part, mut content_type) = (None::<String>, None::<String>, String::new());
        self.fields(e, sc, |_, n, v| {
            match n {
                at::EXTENSION => extension = Some(v.into_owned()),
                at::PART => part = Some(v.into_owned()),
                at::CONTENT_TYPE => content_type = v.into_owned(),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let content_type: Arc<str> = Arc::from(content_type.as_str());
        match (extension, part) {
            (Some(extension), None) => {
                self.record_unknowns(e, sc, "content-type", &format!("default:{extension}"))?;
                Ok(ContentTypeRow::Default { extension: Arc::from(extension.as_str()), content_type })
            }
            (None, Some(part)) => {
                self.record_unknowns(e, sc, "content-type", &format!("override:{part}"))?;
                Ok(ContentTypeRow::Override { part: Arc::from(part.as_str()), content_type })
            }
            _ => Err(self.bad("a content-type row that is neither a default nor an override")),
        }
    }

    pub(super) fn vsdx_held(&mut self, e: &El, sc: &Scope<Ours>) -> Result<VsdxHeld, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let (mut rels, mut content_types, mut props_rest, mut windows) = (Vec::new(), Vec::new(), Vec::new(), None);
        self.children(e, sc, None, |d, child, local, csc| {
            match local {
                el::REL => rels.push(d.rel(child, csc)?),
                el::CONTENT_TYPE => content_types.push(d.content_type_row(child, csc)?),
                el::PROPS_REST => props_rest = d.rest_entries(child, csc)?,
                el::WINDOWS => windows = d.wrapped_element(child, csc)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(VsdxHeld { rels, content_types, props_rest, windows })
    }

    /// A `part` element. `None` when its bytes are lost, with a note.
    pub(super) fn package_part(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Part>, CodecError> {
        let (mut format, mut name, mut media_type, mut role, mut blob) = (None, String::new(), String::new(), None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::FORMAT => format = Some(value_of(&FORMATS, &v).ok_or_else(|| d.bad("a part of a format the vocabulary does not list"))?),
                at::NAME => name = v.into_owned(),
                at::MEDIA_TYPE => media_type = v.into_owned(),
                at::ROLE => role = Some(value_of(&ROLES, &v).ok_or_else(|| d.bad("a part of a role the vocabulary does not list"))?),
                at::BLOB => blob = Some(d.u32(&v)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let (Some(format), Some(role), Some(blob)) = (format, role, blob) else { return Err(self.bad("a part with no format, role or blob")) };
        let mut manifest = RawAttrs::new();
        let mut rels: Vec<Rel> = Vec::new();
        let mut nodes = Passthrough::new();
        let mark = self.keyed.mark();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            match local {
                el::MANIFEST => manifest = d.bag_of(child, csc)?,
                el::REL => rels.push(d.rel(child, csc)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let Some(bytes) = self.blob(blob)? else {
            // A part that is gone takes the unknown content of its relationships with it, and its own is not kept.
            self.keyed.rollback(mark);
            self.lost_part(&name)?;
            return Ok(None);
        };
        self.finish_record(attrs, nodes, "part", &name);
        Ok(Some(Part {
            format,
            name: Arc::from(name.as_str()),
            media_type: Arc::from(media_type.as_str()),
            role,
            manifest,
            rels: Arc::new(rels),
            bytes,
        }))
    }

    pub(super) fn custom_prop(&mut self, e: &El, sc: &Scope<Ours>) -> Result<CustomProp, CodecError> {
        let (mut format, mut name, mut value_type, mut value) = (None, String::new(), String::new(), String::new());
        self.fields(e, sc, |d, n, v| {
            match n {
                at::FORMAT => format = Some(value_of(&FORMATS, &v).ok_or_else(|| d.bad("a property of a format the vocabulary does not list"))?),
                at::NAME => name = v.into_owned(),
                at::VALUE_TYPE => value_type = v.into_owned(),
                at::VALUE => value = v.into_owned(),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let Some(format) = format else { return Err(self.bad("a custom property with no format")) };
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(
            e,
            sc,
            Some(&mut nodes),
            |d, child, local, csc| if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) },
        )?;
        self.finish_record(attrs, nodes, "custom", &format!("{}:{name}", word_of(&FORMATS, format)));
        Ok(CustomProp {
            format,
            name: Arc::from(name.as_str()),
            value_type: Arc::from(value_type.as_str()),
            value: Arc::from(value.as_str()),
            raw: rest.raw,
        })
    }

    // ------------------------------------------------------------------------------------------------------------ ODF

    pub(super) fn held(&mut self, e: &El, sc: &Scope<Ours>, ordinals: &mut Ordinals) -> Result<Held, CodecError> {
        let (mut group, mut stamp) = (None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::GROUP => group = Some(value_of(&GROUPS, &v).ok_or_else(|| d.bad("a held value of a group the vocabulary does not list"))?),
                at::STAMP => stamp = Some(Stamp::parse(&v).ok_or_else(|| d.bad("a stamp that is not 16 lowercase hex digits"))?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs_unknown = self.native_attrs(sc)?;
        let Some(group) = group else { return Err(self.bad("a held value with no group")) };
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, _, csc| d.rest_child(&mut rest, child, csc))?;
        let ordinal = ordinals.next(word_of(&GROUPS, group));
        self.finish_record(attrs_unknown, nodes, "held", &format!("{}.{ordinal}", word_of(&GROUPS, group)));
        Ok(Held { group, stamp, attrs: rest.raw, kept: rest.kept })
    }

    fn definition(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Definition, CodecError> {
        let (mut family, mut name, mut blob) = (None, String::new(), None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::FAMILY => family = Some(value_of(&FAMILIES, &v).ok_or_else(|| d.bad("a definition of a family the vocabulary does not list"))?),
                at::NAME => name = v.into_owned(),
                at::BLOB => blob = Some(d.u32(&v)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let Some(family) = family else { return Err(self.bad("a definition with no family")) };
        let mut element = None;
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            if local == el::KEPT_EL && element.is_none() {
                element = Some(Arc::new(d.raw_element(child, csc)?));
                return Ok(true);
            }
            Ok(false)
        })?;
        self.finish_record(attrs, nodes, "definition", &format!("{}:{name}", word_of(&FAMILIES, family)));
        let Some(element) = element else { return Err(self.bad("a definition with no element")) };
        let bytes = match blob {
            Some(id) => {
                let found = self.blob(id)?;
                if found.is_none() {
                    self.lost_part(&name)?;
                }
                found
            }
            None => None,
        };
        Ok(Definition { family, name: Arc::from(name.as_str()), element, bytes })
    }

    fn kept_style(&mut self, e: &El, sc: &Scope<Ours>) -> Result<KeptStyle, CodecError> {
        let (mut family, mut name) = (String::new(), String::new());
        self.fields(e, sc, |_, n, v| {
            match n {
                at::FAMILY => family = v.into_owned(),
                at::NAME => name = v.into_owned(),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let mut element = None;
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            if local == el::KEPT_EL && element.is_none() {
                element = Some(Arc::new(d.raw_element(child, csc)?));
                return Ok(true);
            }
            Ok(false)
        })?;
        self.finish_record(attrs, nodes, "kept-style", &format!("{family}:{name}"));
        let Some(element) = element else { return Err(self.bad("a style with no element")) };
        Ok(KeptStyle { family: Arc::from(family.as_str()), name: Arc::from(name.as_str()), element })
    }

    /// A record named by `name` whose attributes are `attr` children (`root`, `manifest-row`): its name and attributes.
    fn named_bag(&mut self, e: &El, sc: &Scope<Ours>, kind: &str) -> Result<(Arc<str>, RawAttrs), CodecError> {
        let mut name = String::new();
        self.fields(e, sc, |_, n, v| {
            if n == at::NAME {
                name = v.into_owned();
                return Ok(true);
            }
            Ok(false)
        })?;
        let attrs = self.native_attrs(sc)?;
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(
            e,
            sc,
            Some(&mut nodes),
            |d, child, local, csc| if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) },
        )?;
        self.finish_record(attrs, nodes, kind, &name);
        Ok((Arc::from(name.as_str()), rest.raw))
    }

    pub(super) fn odf_doc(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfDoc) -> Result<(), CodecError> {
        let mut mimetype = None;
        self.fields(o, sc, |_, n, v| {
            if n == at::MIMETYPE {
                mimetype = Some(Self::arc(&v));
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut rest = Rest::default();
        let (mut roots, mut manifest_rows, mut meta, mut settings) = (Vec::new(), Vec::new(), Passthrough::new(), None);
        let (mut definitions, mut layers, mut styles): (Vec<Definition>, Vec<Arc<RawElement>>, Vec<KeptStyle>) = (Vec::new(), Vec::new(), Vec::new());
        let mut per_family = [0usize; FAMILIES.len()];
        self.children(o, sc, None, |d, child, local, csc| {
            match local {
                el::ODF_ROOT => roots.push(d.named_bag(child, csc, "root")?),
                el::MANIFEST_ROW => manifest_rows.push(d.named_bag(child, csc, "manifest-row")?),
                el::META => meta = d.kept_list(child, csc)?,
                el::SETTINGS => settings = d.wrapped_element(child, csc)?,
                el::DEFINITION => {
                    // The cap is each family's (`DocError`'s words for it), counted as the definitions are read: the one that passes it is the last read, and
                    // the six families together can never pass six times the cap.
                    let definition = d.definition(child, csc)?;
                    let (at, what) = family_cap(definition.family);
                    if let Some(count) = per_family.get_mut(at) {
                        *count = count.saturating_add(1);
                        if *count > MAX_DEFINITIONS {
                            return Err(limit(what, MAX_DEFINITIONS));
                        }
                    }
                    definitions.push(definition);
                }
                el::LAYER => layers.extend(d.wrapped_element(child, csc)?),
                el::KEPT_STYLE => styles.push(d.kept_style(child, csc)?),
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        *out = OdfDoc { kept: Kept { raw: rest.raw, kept: rest.kept }, mimetype, roots, manifest_rows, meta, settings, definitions, layers, styles };
        Ok(())
    }

    pub(super) fn odf_page(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfPage) -> Result<(), CodecError> {
        let (mut master_page, mut layout, mut stamp) = (None, None, None);
        self.fields(o, sc, |d, n, v| {
            match n {
                at::MASTER_PAGE => master_page = Some(Self::arc(&v)),
                at::LAYOUT => layout = Some(Self::arc(&v)),
                at::STAMP => stamp = Some(Stamp::parse(&v).ok_or_else(|| d.bad("a stamp that is not 16 lowercase hex digits"))?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut rest = Rest::default();
        let (mut master_page_attrs, mut layout_props, mut drawing_page_style, mut units) = (RawAttrs::new(), RawAttrs::new(), None, RawAttrs::new());
        self.children(o, sc, None, |d, child, local, csc| {
            match local {
                el::MASTER_PAGE_ATTRS => master_page_attrs = d.bag_of(child, csc)?,
                el::LAYOUT_PROPS => layout_props = d.bag_of(child, csc)?,
                el::DRAWING_PAGE_STYLE => drawing_page_style = d.wrapped_element(child, csc)?,
                el::UNITS => units = d.bag_of(child, csc)?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        *out = OdfPage {
            kept: Kept { raw: rest.raw, kept: rest.kept },
            master_page,
            layout,
            master_page_attrs,
            layout_props,
            drawing_page_style,
            units,
            stamp,
        };
        Ok(())
    }

    pub(super) fn odf_style(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfStyle) -> Result<(), CodecError> {
        let (mut name, mut display_name, mut parent, mut family) = (None, None, None, None);
        self.fields(o, sc, |_, n, v| {
            match n {
                at::NAME => name = Some(Self::arc(&v)),
                at::DISPLAY_NAME => display_name = Some(Self::arc(&v)),
                at::PARENT => parent = Some(Self::arc(&v)),
                at::FAMILY => family = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut rest = Rest::default();
        let (mut unmapped, mut units) = (RawAttrs::new(), RawAttrs::new());
        let (mut held, mut text_held): (Vec<Held>, Vec<HeldStyleValue>) = (Vec::new(), Vec::new());
        let (mut ordinals, mut text_ordinals) = (Ordinals::default(), Ordinals::default());
        self.children(o, sc, None, |d, child, local, csc| {
            match local {
                el::UNMAPPED => unmapped = d.bag_of(child, csc)?,
                el::UNITS => units = d.bag_of(child, csc)?,
                el::HELD => held.push(d.held(child, csc, &mut ordinals)?),
                el::HELD_STYLE => text_held.push(d.held_style("style", child, csc, &mut text_ordinals)?),
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        *out = OdfStyle { kept: Kept { raw: rest.raw, kept: rest.kept }, name, display_name, parent, family, unmapped, units, held, text_held };
        Ok(())
    }

    pub(super) fn odf_shape(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfShape) -> Result<(), CodecError> {
        let mut style_name = None;
        self.fields(o, sc, |_, n, v| {
            if n == at::STYLE_NAME {
                style_name = Some(Self::arc(&v));
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut rest = Rest::default();
        let (mut held, mut units) = (Vec::new(), RawAttrs::new());
        let mut ordinals = Ordinals::default();
        self.children(o, sc, None, |d, child, local, csc| {
            match local {
                el::HELD => held.push(d.held(child, csc, &mut ordinals)?),
                el::UNITS => units = d.bag_of(child, csc)?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        *out = OdfShape { kept: Kept { raw: rest.raw, kept: rest.kept }, style_name, held, units };
        Ok(())
    }

    pub(super) fn odf_row(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfRow) -> Result<(), CodecError> {
        let mut style_name = None;
        self.fields(o, sc, |_, n, v| {
            if n == at::STYLE_NAME {
                style_name = Some(Self::arc(&v));
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut rest = Rest::default();
        let mut held = None;
        let mut ordinals = Ordinals::default();
        self.children(o, sc, None, |d, child, local, csc| {
            if local == el::HELD {
                if held.is_some() {
                    // A row holds one value. A second is what a newer build wrote: kept as the markup it is, after the nodes kept before it, so that neither
                    // it nor the first (whose unknown content is under its own key) loses anything.
                    let raw = d.unknown_element(child, csc)?;
                    d.push_kept(&mut rest.kept, 0, RawNode::Element(raw))?;
                    return Ok(true);
                }
                held = Some(d.held(child, csc, &mut ordinals)?);
                return Ok(true);
            }
            d.rest_child(&mut rest, child, csc)
        })?;
        *out = OdfRow { kept: Kept { raw: rest.raw, kept: rest.kept }, style_name, held };
        Ok(())
    }

    /// The `odf` element of a text. The ordinals of its paragraphs and runs make the keys of the unknown content its records kept (see [`Encoder::odf_text`]).
    pub(super) fn odf_text(&mut self, o: &El, sc: &Scope<Ours>, out: &mut OdfText) -> Result<(), CodecError> {
        self.fields(o, sc, |_, _, _| Ok(false))?;
        // What a newer build wrote on the element is kept with what the data form keeps: its attributes first, and its elements after the nodes the data form kept.
        let mut rest = Rest { raw: self.native_attrs(sc)?, ..Rest::default() };
        let mut paragraphs: Vec<OdfParagraph> = Vec::new();
        let mut unknown = Passthrough::new();
        self.children(o, sc, Some(&mut unknown), |d, child, local, csc| {
            if local == el::PARAGRAPH {
                let p = paragraphs.len();
                paragraphs.push(d.paragraph(p, child, csc)?);
                return Ok(true);
            }
            d.rest_child(&mut rest, child, csc)
        })?;
        if !unknown.is_empty() {
            rest.kept.nodes_mut().append(unknown.nodes_mut());
        }
        *out = OdfText { kept: Kept { raw: rest.raw, kept: rest.kept }, paragraphs };
        Ok(())
    }

    fn paragraph(&mut self, p: usize, e: &El, sc: &Scope<Ours>) -> Result<OdfParagraph, CodecError> {
        let mut characters = None;
        self.fields(e, sc, |d, n, v| {
            if n == at::CHARACTERS {
                characters = Some(Stamp::parse(&v).ok_or_else(|| d.bad("a stamp that is not 16 lowercase hex digits"))?);
                return Ok(true);
            }
            Ok(false)
        })?;
        let attrs = self.native_attrs(sc)?;
        let key = p.to_string();
        let mut style = OdfTextStyle::default();
        let mut style_seen = false;
        let mut runs: Vec<OdfRun> = Vec::new();
        let mut element: Option<Arc<RawElement>> = None;
        let mut element_seen = false;
        let mut levels: Vec<OdfListLevel> = Vec::new();
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            match local {
                // The first `text-style` is the paragraph's; another is not read, or the unknown content of the first's records would be left with none.
                el::TEXT_STYLE if style_seen => d.unknown_el(child)?,
                el::TEXT_STYLE => {
                    style_seen = true;
                    style = d.text_style(&key, child, csc)?;
                }
                // The first `element` is the paragraph's, as the first `text-style` is.
                el::ELEMENT if element_seen => d.unknown_el(child)?,
                el::ELEMENT => {
                    element_seen = true;
                    element = d.wrapped_element(child, csc)?;
                }
                el::LEVEL if levels.len() >= MAX_LIST_LEVELS => return Err(limit("levels of lists round a paragraph", MAX_LIST_LEVELS)),
                el::LEVEL => levels.push(d.list_level(child, csc)?),
                el::RUN => {
                    let r = runs.len();
                    runs.push(d.run(p, r, child, csc)?);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.finish_record(attrs, nodes, "paragraph", &key);
        Ok(OdfParagraph { style, characters, runs, element, levels })
    }

    /// One `level` of a paragraph: the list and the item it opens at that depth, each a wrapper of one `el`, the first of each read.
    fn list_level(&mut self, e: &El, sc: &Scope<Ours>) -> Result<OdfListLevel, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let (mut list, mut item): (Option<Arc<RawElement>>, Option<Arc<RawElement>>) = (None, None);
        let (mut list_seen, mut item_seen) = (false, false);
        self.children(e, sc, None, |d, child, local, csc| {
            match local {
                el::LIST if !list_seen => {
                    list_seen = true;
                    list = d.wrapped_element(child, csc)?;
                }
                el::ITEM if !item_seen => {
                    item_seen = true;
                    item = d.wrapped_element(child, csc)?;
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(OdfListLevel { list, item })
    }

    fn run(&mut self, p: usize, r: usize, e: &El, sc: &Scope<Ours>) -> Result<OdfRun, CodecError> {
        let (mut from, mut to) = (0u32, 0u32);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::FROM => from = d.u32(&v)?,
                at::TO => to = d.u32(&v)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let key = format!("{p}.{r}");
        let mut style = OdfTextStyle::default();
        let mut style_seen = false;
        let mut element: Option<Arc<RawElement>> = None;
        let mut element_seen = false;
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            if local == el::TEXT_STYLE {
                // As in a paragraph: the first is the run's.
                if style_seen {
                    d.unknown_el(child)?;
                } else {
                    style_seen = true;
                    style = d.text_style(&key, child, csc)?;
                }
                return Ok(true);
            }
            if local == el::ELEMENT {
                if element_seen {
                    d.unknown_el(child)?;
                } else {
                    element_seen = true;
                    element = d.wrapped_element(child, csc)?;
                }
                return Ok(true);
            }
            Ok(false)
        })?;
        self.finish_record(attrs, nodes, "run", &key);
        Ok(OdfRun { from, to, style, element })
    }

    fn text_style(&mut self, key: &str, e: &El, sc: &Scope<Ours>) -> Result<OdfTextStyle, CodecError> {
        let (mut name, mut stamp) = (None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::NAME => name = Some(Self::arc(&v)),
                at::STAMP => stamp = Some(Stamp::parse(&v).ok_or_else(|| d.bad("a stamp that is not 16 lowercase hex digits"))?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs = self.native_attrs(sc)?;
        let mut held: Vec<HeldStyleValue> = Vec::new();
        let mut ordinals = Ordinals::default();
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, local, csc| {
            if local == el::HELD_STYLE {
                held.push(d.held_style(key, child, csc, &mut ordinals)?);
                return Ok(true);
            }
            Ok(false)
        })?;
        self.finish_record(attrs, nodes, "text-style", key);
        Ok(OdfTextStyle { name, stamp, held })
    }

    fn held_style(&mut self, key: &str, e: &El, sc: &Scope<Ours>, ordinals: &mut Ordinals) -> Result<HeldStyleValue, CodecError> {
        let (mut group, mut stamp) = (None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::GROUP => {
                    group = Some(
                        TextGroup::ALL
                            .iter()
                            .copied()
                            .find(|g| g.id() == &*v)
                            .ok_or_else(|| d.bad("a held style value of a group the vocabulary does not list"))?,
                    )
                }
                at::STAMP => stamp = Some(Stamp::parse(&v).ok_or_else(|| d.bad("a stamp that is not 16 lowercase hex digits"))?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let attrs_unknown = self.native_attrs(sc)?;
        let Some(group) = group else { return Err(self.bad("a held style value with no group")) };
        let mut rest = Rest::default();
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |d, child, _, csc| d.rest_child(&mut rest, child, csc))?;
        let ordinal = ordinals.next(group.id());
        self.finish_record(attrs_unknown, nodes, "held-style", &format!("{key}.{}.{ordinal}", group.id()));
        Ok(HeldStyleValue { group, stamp, attrs: rest.raw, kept: rest.kept })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyed_name_is_a_kind_and_a_key_in_brackets_and_nothing_else_is() {
        assert_eq!(keyed_name("color", "30"), "color[30]");
        assert_eq!(keyed_name("rel", "/a|r"), "rel[/a|r]");
        assert!(is_keyed("color[30]"));
        assert!(is_keyed("a[]"));
        // An XML name has no bracket, so a name with one at the end only or at the start only is no key's.
        for name in ["", "x", "x]", "[x", "]x[", "diacraft:future", "x[y"] {
            assert!(!is_keyed(name), "{name:?}");
        }
    }
}
