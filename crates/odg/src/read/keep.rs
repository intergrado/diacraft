//! What the reader keeps of the package and of the document's own parts, as data in the model (the native-format design's §3.4 and §4.2): the font faces, `meta.xml`,
//! `settings.xml`, the roots of the parts, `mimetype`, the manifest's rows and the package entries no model field reads. A `.diacraft` file keeps no original, so **whatever the model
//! does not hold here is lost the day a drawing opened from an `.odg` is saved as one**. The model holds this data **as the file had it when it was read**: it follows the file
//! where a model writer needs it, by [`crate::read_package_data`] and [`crate::PackageData::put`] (`package_data.rs`), since a splice save writes some of it anew.
//!
//! # What goes where
//!
//! * **Font faces** (`Document.fonts`): one [`FaceName`] for each distinct `style:font-face` of the two parts' `office:font-face-decls`, in the order read (`styles.xml` first). Its `name_u`
//!   is the family the face names (`svg:font-family`, quotes taken off, which is what a run's font cell holds, so the engine's lookup by family finds it), or the declaration's
//!   `style:name` when it names no family. **The declaration is held exactly**, in the face's `odf` slot: every attribute, the namespace declarations among them, in order, and every child
//!   (`svg:font-face-src`). A declaration equal to one held, attribute for attribute and child for child, is the same face and is held once (a producer declares a face in both parts). At most
//!   [`MAX_FACES`], and the face past it refuses the drawing.
//! * **`meta.xml`**: `dc:title` is [`Document::title`]; the fields both formats say are [`DocProps`] (`dc:description`, `dc:subject`, the first `meta:keyword`, `meta:initial-creator` as the
//!   creator, `dc:creator` as the last modifier, `meta:creation-date`, `dc:date` as the modified time, `meta:print-date`, `dc:language`); each `meta:user-defined` is a
//!   [`CustomProp`] of [`Format::Odf`]; **everything else** (the generator, the statistics, the editing cycles, a second title, an element with attributes of its own, a comment) is
//!   kept whole in `OdfDoc.meta`, each node with the number of typed fields before it. A field is typed only while it is a plain text element with no attribute and the first of its name: the
//!   writer puts back what the model says and what is kept, so no byte of a field is lost. **A `meta.xml` that is not of that shape** (past the cap of a small part, an `office:meta` of
//!   attributes of its own, a root child that is not `office:meta`) is not read into fields at all: it is a package entry, kept as bytes ([`Part`]).
//! * **`settings.xml`**: the whole tree, `OdfDoc.settings`. One past the small-part cap is a package entry of bytes, as `meta.xml` is.
//! * **The roots**: the attributes (the namespace declarations among them) of the root of `content.xml`, `styles.xml`, `meta.xml`, `settings.xml` and the manifest, in `OdfDoc.roots` by
//!   the entry's name; `mimetype` is `OdfDoc.mimetype`.
//! * **The manifest's rows**: a row for an entry that is a [`Part`] is on the part (its media type, its other attributes); every other row (the root's, a row for `content.xml`, for a
//!   picture the model holds, for an entry the package lacks, a second row for one name, a row with no name, which is held under the empty name) is in `OdfDoc.manifest_rows`, by the
//!   name it gives, with all its attributes but that one.
//! * **The package entries**: every file and folder the model does not read as something else is a [`Part`] of [`Format::Odf`] in the order of the archive's directory, its bytes **shared with
//!   the package** (never a copy), its role by its name ([`role_of`]). What the model reads as something else, and so is no part: `mimetype`, the manifest, `content.xml`, `styles.xml`,
//!   `meta.xml` and `settings.xml` when they are read into fields, DiaCraft's own document file, and a picture a frame shows.
//!
//! # Cost
//!
//! Every value is **charged to the read's items before it is made** ([`charge`]), by the sizes of its own type and the bytes of its text, as the rest of the model's data is. The package
//! entries' bytes are charged to the media budget (`Limits::max_media_bytes`) twice over: once by the package, from the sizes its directory states, before the entry is inflated
//! ([`Package::read_kept`]), and again here as the part is made, so that an entry the package inflated for the reader's own sake (an oversize `meta.xml`) is charged too.
//! The work is counted in the reader's own units (`work.elements` and `work.attributes`: every node and attribute walked, to hash, to compare or to hold).

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    CustomProp, DocProps, Document, ExtData, FaceName, Format, Kept, MAX_PARTS, Part, PartRole, Passthrough, Placed, RawAttrs, RawElement, RawNode,
};

use super::cost::{held, priced, slot};
use super::names::{Names, Ns};
use super::shapes::{element_cost, raw_element};
use super::{ITEM_BYTES, Reader};
use crate::error::OdgError;
use crate::package::{MANIFEST_PATH, MIMETYPE_PATH, Package};

/// Labelled choice: the most font faces a read holds, the cap the model gives its named definitions (`diacraft_doc::MAX_DEFINITIONS`). A real drawing declares
/// about ten; the face past this refuses the drawing, and nothing is left out.
pub(crate) const MAX_FACES: usize = 65_536;

/// The counts in front of what an `Arc` holds.
const ARC: usize = 16;

/// Items a font face costs beside its declaration's own markup: the [`FaceName`], the slot behind its pointer, its entry in the table that finds a declaration held before (an entry
/// of 64 bytes at most, with the list it keeps), and what tells the parts apart: its byte of [`Faces::parts`] and its name's entry in [`Faces::names`] (a hash, in a
/// set that keeps room for twice what it holds: 32 bytes at most).
const FACE: usize = (size_of::<FaceName>() + ARC + size_of::<ExtData<Kept>>() + ARC + 64 + 1 + 32).div_ceil(ITEM_BYTES);

/// Items a user-defined property costs beside the bytes of its strings: the [`CustomProp`] in the document's list of them, which grows by doubling, and the three
/// allocations of its text, by the law of the memory tests a quarter above ([`super::cost::priced`]). It was the property and three counts (two items);
/// counted, a property kept 216 bytes in 3 allocations and was charged 3 items, where the law asks 315.
const CUSTOM: usize = priced(slot(size_of::<CustomProp>()) + 3 * held(ARC));

/// Items a package entry costs beside the bytes of its names: the [`Part`] behind its pointer, the list of relationships it points to (none, which is an allocation of its
/// own), its place in the list of parts and the three allocations of its name, its media type and its bytes, by the law of the memory tests a quarter above
/// ([`super::cost::priced`]). It was three items; counted, an entry kept 253 bytes in 5 allocations and was charged 4 items and its bytes, where the law asks
/// 405.
const PART: usize =
    priced(held(ARC + size_of::<Part>()) + held(ARC + size_of::<Vec<diacraft_doc::Rel>>()) + slot(size_of::<Arc<Part>>()) + 3 * held(ARC));

/// Items a manifest row, a root or a typed field of `meta.xml` costs beside its strings: a pair of an `Arc<str>` and a list of attributes, and a place in a list.
const ENTRY: usize = (size_of::<(Arc<str>, RawAttrs)>() + ARC + size_of::<usize>()).div_ceil(ITEM_BYTES);

/// Charges the read `items` and `bytes` of the model's memory, in whole items, before what they pay for is made.
fn charge(r: &mut Reader<'_>, items: usize, bytes: usize) -> Result<(), OdgError> {
    r.take_items(items.saturating_add(super::cost::text(bytes).div_ceil(ITEM_BYTES)))
}

/// What attributes `(name, value)` cost: two items a pair, and the bytes of both strings.
fn pairs_cost(pairs: &[(String, String)]) -> (usize, usize) {
    let bytes = pairs.iter().fold(0usize, |sum, (k, v)| sum.saturating_add(k.len()).saturating_add(v.len()));
    (pairs.len().saturating_mul(2), bytes)
}

/// The attributes `pairs` as the model keeps them.
fn raw_attrs(pairs: &[(String, String)]) -> RawAttrs {
    RawAttrs::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}

// ---- the font faces --------------------------------------------------------------------------------------------------------------------

/// The faces a read has held, and how to find a declaration it has held already.
#[derive(Default)]
pub(crate) struct Faces {
    list: Vec<FaceName>,
    /// The places in `list` of the faces whose declaration has this hash (a hash of the whole declaration, so a place is a candidate and not yet a match).
    by_hash: HashMap<u64, Vec<usize>>,
    /// The parts each face of `list` was declared in, by place: a bit for each ([`part_bit`]).
    parts: Vec<u8>,
    /// The names (`style:name`) declared so far, by a hash of the name: a second declaration of a name that is not the first's is one the font table cannot hold apart.
    names: HashSet<u64>,
}

/// The bit of the part a declaration is in, for [`Faces::parts`].
fn part_bit(file: super::styles::File) -> u8 {
    match file {
        super::styles::File::Styles => 1,
        super::styles::File::Content => 2,
    }
}

/// A hash of a declaration's `style:name`, for [`Faces::names`].
fn name_hash(name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

/// A hash of a whole declaration: names, attributes and children, in order, and how many elements and attributes were walked to make it (the work, in the reader's units). The walk
/// keeps a stack of the children still to see and nothing deeper than the tree is, so no input makes it recurse.
fn declaration_hash(root: &El) -> (u64, u64, u64) {
    let mut hasher = DefaultHasher::new();
    let (mut elements, mut attrs) = (1u64, root.attrs.len() as u64);
    let open = |hasher: &mut DefaultHasher, el: &El| {
        1u8.hash(hasher);
        el.name.hash(hasher);
        el.attrs.len().hash(hasher);
        for (k, v) in &el.attrs {
            k.hash(hasher);
            v.hash(hasher);
        }
    };
    open(&mut hasher, root);
    let mut stack = vec![root.children.iter()];
    while let Some(top) = stack.last_mut() {
        match top.next() {
            Some(Node::El(child)) => {
                elements = elements.saturating_add(1);
                attrs = attrs.saturating_add(child.attrs.len() as u64);
                open(&mut hasher, child);
                stack.push(child.children.iter());
            }
            // A text and a CDATA section are one kind of node in the model.
            Some(Node::Text(t) | Node::CData(t)) => {
                2u8.hash(&mut hasher);
                t.hash(&mut hasher);
            }
            Some(Node::Comment(c)) => {
                3u8.hash(&mut hasher);
                c.hash(&mut hasher);
            }
            Some(Node::Pi { target, data }) => {
                4u8.hash(&mut hasher);
                target.hash(&mut hasher);
                data.hash(&mut hasher);
            }
            None => {
                0u8.hash(&mut hasher);
                stack.pop();
            }
        }
    }
    (hasher.finish(), elements, attrs)
}

/// Whether the child nodes `a` are the nodes `b`, every one to the last byte (a text and a CDATA section are alike), and the elements and attributes walked doing it.
fn same_nodes(a: &[Node], b: &[&RawNode]) -> (bool, u64, u64) {
    let (mut elements, mut attrs) = (0u64, 0u64);
    let mut stack: Vec<(Vec<&Node>, Vec<&RawNode>)> = vec![(a.iter().collect(), b.to_vec())];
    while let Some((left, right)) = stack.pop() {
        if left.len() != right.len() {
            return (false, elements, attrs);
        }
        for (x, y) in left.into_iter().zip(right) {
            match (x, y) {
                (Node::El(e), RawNode::Element(k)) => {
                    elements = elements.saturating_add(1);
                    attrs = attrs.saturating_add(e.attrs.len() as u64);
                    let same_attrs = e.attrs.len() == k.attrs.len() && e.attrs.iter().map(|(n, v)| (n.as_str(), v.as_str())).eq(k.attrs.iter());
                    if &*k.name != e.name.as_str() || !same_attrs {
                        return (false, elements, attrs);
                    }
                    stack.push((e.children.iter().collect(), k.children.iter().collect()));
                }
                (Node::Text(t) | Node::CData(t), RawNode::Text(u)) if t.as_str() == &**u => {}
                (Node::Comment(c), RawNode::Comment(d)) if c.as_str() == &**d => {}
                (Node::Pi { target, data }, RawNode::Pi { target: t, data: d }) if target.as_str() == &**t && data.as_str() == &**d => {}
                _ => return (false, elements, attrs),
            }
        }
    }
    (true, elements, attrs)
}

impl Faces {
    /// The place of the face held for the declaration `el`, when it is one held already: the candidates are those of its hash, and one is the face when its attributes and children are the
    /// declaration's.
    fn find(&self, hash: u64, el: &El) -> (Option<usize>, u64, u64) {
        let (mut elements, mut attrs) = (0u64, 0u64);
        // A declaration with no attribute and no child is a slot of defaults, which is no slot (`Ext::get`).
        let nothing = Kept::default();
        for &at in self.by_hash.get(&hash).map_or(&[][..], Vec::as_slice) {
            let Some(face) = self.list.get(at) else { continue };
            let odf = face.ext.get().map_or(&nothing, |d| &d.odf);
            let nodes: Vec<&RawNode> = odf.kept.iter().map(|p| &p.node).collect();
            let same_attrs = el.attrs.len() == odf.raw.len() && el.attrs.iter().map(|(n, v)| (n.as_str(), v.as_str())).eq(odf.raw.iter());
            attrs = attrs.saturating_add(el.attrs.len() as u64);
            if !same_attrs {
                continue;
            }
            let (same, e, a) = same_nodes(&el.children, &nodes);
            elements = elements.saturating_add(e);
            attrs = attrs.saturating_add(a);
            if same {
                return (Some(at), elements, attrs);
            }
        }
        (None, elements, attrs)
    }

    /// How many faces are held.
    pub(crate) fn len(&self) -> usize {
        self.list.len()
    }

    /// The faces held, to be the document's font table.
    pub(crate) fn into_table(self) -> Vec<FaceName> {
        self.list
    }
}

/// The name a face is known by: the family it declares, quotes off (what a font cell holds), else the name of the declaration.
fn face_name<'t>(names: &Names<'t>, el: &'t El) -> String {
    let attrs = names.attrs(el);
    let family = attrs.peek(Ns::Svg, "font-family").map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"')).filter(|f| !f.is_empty());
    family.or_else(|| attrs.peek(Ns::Style, "name")).unwrap_or_default().to_owned()
}

/// Holds the declaration `el` (a `style:font-face` of the part `file`) as a face of the font table, unless an equal one is held. Charged first.
///
/// **What the one table cannot hold apart is counted** for the open's note: a declaration equal to one the same part declared already (the table holds it once, and a
/// copy declares it once), and a declaration of a `style:name` an earlier declaration that is not the same one gave (a copy declares both in each part, so that a part finds the other
/// part's under the name). A declaration the two parts make alike is one face of both, as the programs that write both write it, and is no loss.
pub(crate) fn face<'t>(r: &mut Reader<'_>, names: &Names<'t>, el: &'t El, file: super::styles::File) -> Result<(), OdgError> {
    let (hash, elements, attrs) = declaration_hash(el);
    r.saw_elements(elements);
    r.saw_attrs(usize::try_from(attrs).unwrap_or(usize::MAX));
    let (found, elements, attrs) = r.faces.find(hash, el);
    r.saw_elements(elements);
    r.saw_attrs(usize::try_from(attrs).unwrap_or(usize::MAX));
    let bit = part_bit(file);
    if let Some(at) = found {
        let repeat = match r.faces.parts.get_mut(at) {
            Some(parts) => {
                let seen = *parts & bit != 0;
                *parts |= bit;
                seen
            }
            None => false,
        };
        if repeat {
            r.not_kept("font_declaration", 1);
        }
        return Ok(());
    }
    if r.faces.len() >= MAX_FACES {
        return Err(OdgError::TooComplex { what: "font faces", max: MAX_FACES as u64 });
    }
    // A name of a face another declaration gave: the table holds both, and cannot say which part gave which.
    let declared = names.attrs(el).peek(Ns::Style, "name").map(name_hash);
    if let Some(key) = declared
        && !r.faces.names.insert(key)
    {
        r.not_kept("font_declaration", 1);
    }
    let name = face_name(names, el);
    charge(r, FACE + element_cost(el), name.len())?;
    let made = raw_element(el);
    let kept = Kept { raw: made.attrs, kept: Passthrough::from_nodes(made.children.into_iter().map(|node| Placed { after: 0, node }).collect()) };
    let mut face = FaceName { name_u: Arc::from(name.as_str()), raw: RawAttrs::new(), ext: Default::default() };
    face.ext.make_mut().odf = kept;
    r.faces.by_hash.entry(hash).or_default().push(r.faces.list.len());
    r.faces.list.push(face);
    r.faces.parts.push(bit);
    Ok(())
}

// ---- meta.xml --------------------------------------------------------------------------------------------------------------------------

/// What `meta.xml` gave: the title, the fields of [`DocProps`] it says, and the nodes kept whole.
#[derive(Default)]
pub(crate) struct Meta {
    title: Option<Arc<str>>,
    props: DocProps,
    kept: Passthrough,
}

/// The fields of `meta.xml` both formats say, in the order of [`Meta::set`]'s slots.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Description,
    Subject,
    Keyword,
    InitialCreator,
    Creator,
    CreationDate,
    Date,
    PrintDate,
    Language,
}

impl Field {
    const COUNT: usize = 10;

    fn of(ns: Ns, local: &str) -> Option<Field> {
        Some(match (ns, local) {
            (Ns::Dc, "title") => Field::Title,
            (Ns::Dc, "description") => Field::Description,
            (Ns::Dc, "subject") => Field::Subject,
            (Ns::Meta, "keyword") => Field::Keyword,
            (Ns::Meta, "initial-creator") => Field::InitialCreator,
            (Ns::Dc, "creator") => Field::Creator,
            (Ns::Meta, "creation-date") => Field::CreationDate,
            (Ns::Dc, "date") => Field::Date,
            (Ns::Meta, "print-date") => Field::PrintDate,
            (Ns::Dc, "language") => Field::Language,
            _ => return None,
        })
    }

    fn slot(self) -> usize {
        self as usize
    }
}

/// The text of `el` when it is a plain text element: no attribute (a namespace declaration is one) and nothing but text in it.
fn plain_text(el: &El) -> Option<String> {
    if !el.attrs.is_empty() || el.children.iter().any(|n| !matches!(n, Node::Text(_) | Node::CData(_))) {
        return None;
    }
    Some(el.text())
}

/// Whether `node` is white space between elements, which is the file's layout and no content.
fn is_layout(node: &Node) -> bool {
    matches!(node, Node::Text(t) if t.chars().all(char::is_whitespace))
}

impl Meta {
    fn set(&mut self, field: Field, text: &str) {
        let value: Arc<str> = Arc::from(text);
        let p = &mut self.props;
        match field {
            Field::Title => self.title = Some(value),
            Field::Description => p.description = Some(value),
            Field::Subject => p.subject = Some(value),
            Field::Keyword => p.keywords = Some(value),
            Field::InitialCreator => p.creator = Some(value),
            Field::Creator => p.last_modified_by = Some(value),
            Field::CreationDate => p.created = Some(value),
            Field::Date => p.modified = Some(value),
            Field::PrintDate => p.last_printed = Some(value),
            Field::Language => p.language = Some(value),
        }
    }
}

/// Reads `meta.xml`'s tree into [`Meta`], or says it is not of the shape the fields can hold (`None`: the part is kept as bytes). The tree's root is `office:document-meta` (the caller
/// has read its generator, which refuses any other root). Each value is charged before it is made.
pub(crate) fn meta(r: &mut Reader<'_>, tree: &El) -> Result<Option<Meta>, OdgError> {
    // The shape: one `office:meta` and nothing else but layout under the root, and nothing on `office:meta` itself that a field would lose.
    let mut names = Names::default();
    let root_mark = names.enter(tree);
    let mut holder: Option<&El> = None;
    let mut shaped = true;
    for node in &tree.children {
        match node {
            Node::El(el) if holder.is_none() && names.is(el, Ns::Office, "meta") && el.attrs.is_empty() => holder = Some(el),
            other if is_layout(other) => {}
            _ => shaped = false,
        }
    }
    let Some(holder) = holder.filter(|_| shaped) else {
        names.leave(root_mark);
        return Ok(None);
    };
    let out = meta_fields(r, &mut names, holder)?;
    names.leave(root_mark);
    Ok(Some(out))
}

/// `office:meta` of a flat drawing into [`Meta`] as `meta.xml`'s is (a flat file's `office:meta` is `meta.xml`'s): the same fields, kept the same way. `root`
/// is the flat document's root, whose namespace declarations the fields' names are read under. `None` when the element has attributes of its own (a namespace declaration is one), which a
/// package's `meta.xml` has no more than a flat one: the fields it holds stay in the file's own bytes, and the caller says so.
pub(crate) fn flat_meta<'t>(r: &mut Reader<'_>, root: &'t El, holder: &'t El) -> Result<Option<Meta>, OdgError> {
    if !holder.attrs.is_empty() {
        return Ok(None);
    }
    let mut names = Names::default();
    let root_mark = names.enter(root);
    let out = meta_fields(r, &mut names, holder);
    names.leave(root_mark);
    out.map(Some)
}

/// The fields of the `office:meta` element `holder`, whose names are read through `names`.
fn meta_fields<'t>(r: &mut Reader<'_>, names: &mut Names<'t>, holder: &'t El) -> Result<Meta, OdgError> {
    let mut out = Meta::default();
    let mut seen = [false; Field::COUNT];
    let mut typed = 0u32;
    for node in &holder.children {
        if is_layout(node) {
            continue;
        }
        let Node::El(child) = node else {
            // A comment, an instruction or text that is no layout: kept where it stood.
            r.saw_element();
            charge(r, 2, node_bytes(node))?;
            out.kept.push(typed, raw_node_of(node));
            continue;
        };
        r.saw_element();
        let mark = names.enter(child);
        let (ns, local) = names.element(child);
        let attrs = names.attrs(child);
        r.saw_attrs(child.attrs.len());
        let mut done = false;
        if let Some(field) = Field::of(ns, local)
            && let Some(slot) = seen.get_mut(field.slot())
            && !*slot
            && let Some(text) = plain_text(child).filter(|t| !t.is_empty())
        {
            charge(r, ENTRY, text.len())?;
            *slot = true;
            out.set(field, &text);
            done = true;
        } else if ns == Ns::Meta && local == "user-defined" && child.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) {
            let name = attrs.iter().find(|a| a.ns == Ns::Meta && a.local == "name").map(|a| (a.name, a.value));
            let kind = attrs.iter().find(|a| a.ns == Ns::Meta && a.local == "value-type").map(|a| (a.name, a.value));
            if let Some((name_key, name)) = name {
                let rest: Vec<(String, String)> = child
                    .attrs
                    .iter()
                    .filter(|(k, _)| k != name_key && kind.is_none_or(|(key, _)| k != key))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let text = child.text();
                let (items, bytes) = pairs_cost(&rest);
                let words = name.len().saturating_add(kind.map_or(0, |(_, v)| v.len())).saturating_add(text.len()).saturating_add(bytes);
                charge(r, CUSTOM + items, words)?;
                out.props.custom.push(CustomProp {
                    format: Format::Odf,
                    name: Arc::from(name),
                    value_type: Arc::from(kind.map_or("", |(_, v)| v)),
                    value: Arc::from(text.as_str()),
                    raw: raw_attrs(&rest),
                });
                done = true;
            }
        }
        if done {
            typed = typed.saturating_add(1);
        } else {
            // Kept whole, tied to the number of typed fields before it.
            let held = element_cost(child);
            charge(r, held.saturating_add(1), 0)?;
            r.saw_elements(count_elements(child));
            out.kept.push(typed, RawNode::Element(raw_element(child)));
        }
        names.leave(mark);
    }
    Ok(out)
}

/// The bytes of a node that is no element, for its charge.
fn node_bytes(node: &Node) -> usize {
    match node {
        Node::Text(t) | Node::CData(t) | Node::Comment(t) => t.len(),
        Node::Pi { target, data } => target.len().saturating_add(data.len()),
        Node::El(_) => 0,
    }
}

/// A node that is no element, as the model keeps it.
fn raw_node_of(node: &Node) -> RawNode {
    match node {
        Node::El(el) => RawNode::Element(raw_element(el)),
        Node::Text(t) | Node::CData(t) => RawNode::Text(Arc::from(t.as_str())),
        Node::Comment(c) => RawNode::Comment(Arc::from(c.as_str())),
        Node::Pi { target, data } => RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) },
    }
}

/// The elements in `el`, itself counted, walked with a stack of their own.
fn count_elements(el: &El) -> u64 {
    let mut count = 0u64;
    let mut stack = vec![el];
    while let Some(here) = stack.pop() {
        count = count.saturating_add(1);
        stack.extend(here.elements());
    }
    count
}

// ---- settings.xml and the roots --------------------------------------------------------------------------------------------------------

/// Reads `settings.xml`'s tree whole, charged before it is made.
pub(crate) fn settings(r: &mut Reader<'_>, tree: &El) -> Result<Arc<RawElement>, OdgError> {
    charge(r, element_cost(tree).saturating_add(1), 0)?;
    r.saw_elements(count_elements(tree));
    Ok(Arc::new(raw_element(tree)))
}

/// A flat drawing's `office:settings` as the tree of a `settings.xml` (a flat file's `office:settings` is `settings.xml`'s): an
/// `office:document-settings` root, written under the prefix the flat document's own root has and bound by the namespace declarations that root has, holding the element whole. The model
/// holds it as it holds a package's, so a package written from the model writes the settings the flat file had. Charged before it is made.
pub(crate) fn flat_settings(r: &mut Reader<'_>, root: &El, holder: &El) -> Result<Arc<RawElement>, OdgError> {
    let prefix = root.name.split_once(':').map_or("", |(prefix, _)| prefix);
    let name = if prefix.is_empty() { "document-settings".to_owned() } else { format!("{prefix}:document-settings") };
    let declarations: Vec<(String, String)> = root.attrs.iter().filter(|(k, _)| k == "xmlns" || k.starts_with("xmlns:")).cloned().collect();
    let (items, bytes) = pairs_cost(&declarations);
    charge(r, element_cost(holder).saturating_add(items).saturating_add(ENTRY), bytes.saturating_add(name.len()))?;
    r.saw_elements(count_elements(holder).saturating_add(1));
    r.saw_attrs(declarations.len());
    Ok(Arc::new(RawElement {
        name: Arc::from(name.as_str()),
        attrs: raw_attrs(&declarations),
        children: vec![RawNode::Element(raw_element(holder))],
    }))
}

/// What the read found of the document's own parts as it went, to be put in the model when it is made.
#[derive(Default)]
pub(crate) struct Held {
    pub(crate) meta: Option<Meta>,
    pub(crate) settings: Option<Arc<RawElement>>,
    roots: Vec<(Arc<str>, RawAttrs)>,
}

impl Held {
    /// Holds the attributes of the root of the part `name` (the namespace declarations among them), charged first.
    pub(crate) fn root(&mut self, r: &mut Reader<'_>, name: &str, attrs: &[(String, String)]) -> Result<(), OdgError> {
        let (items, bytes) = pairs_cost(attrs);
        charge(r, ENTRY + items, name.len().saturating_add(bytes))?;
        r.saw_attrs(attrs.len());
        self.roots.push((Arc::from(name), raw_attrs(attrs)));
        Ok(())
    }
}

// ---- the package ---------------------------------------------------------------------------------------------------------------------------

/// What an entry of the package is for, by its name: a macro project, a signature, an embedded object, its replacement picture, the thumbnail or an embedded font,
/// and anything else. A name ending in `/` is a folder of the same role as the files in it.
pub(crate) fn role_of(name: &str) -> PartRole {
    let object = name.strip_prefix("Object ").is_some_and(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest.get(digits..).is_some_and(|tail| tail.starts_with('/'))
    });
    if name.starts_with("Thumbnails/") && !name.ends_with('/') {
        PartRole::Thumbnail
    } else if name.starts_with("Basic/") || name.starts_with("Scripts/") {
        PartRole::MacroProject
    } else if name == "META-INF/macrosignatures.xml" {
        PartRole::MacroSignature
    } else if name == "META-INF/documentsignatures.xml" {
        PartRole::Signature
    } else if name.starts_with("ObjectReplacements/") {
        PartRole::ObjectReplacement
    } else if object {
        PartRole::ObjectFile
    } else if name.starts_with("Fonts/") {
        PartRole::Font
    } else {
        PartRole::Other
    }
}

/// What a package entry the model keeps as a part is not: the entries the model reads as something else.
fn is_read_elsewhere(name: &str, meta_held: bool, settings_held: bool, pictures: &HashSet<&str>) -> bool {
    match name {
        MIMETYPE_PATH | MANIFEST_PATH | "content.xml" | "styles.xml" => true,
        "meta.xml" => meta_held,
        "settings.xml" => settings_held,
        // DiaCraft's own entries (its document file and the entries its items name) are read by the reader of the items, and are no part of the OpenDocument package.
        _ => {
            name.get(..crate::ext::items::FOLDER.len()).is_some_and(|head| head.eq_ignore_ascii_case(crate::ext::items::FOLDER))
                || pictures.contains(name)
        }
    }
}

/// Puts what `meta.xml` (or a flat drawing's `office:meta`) gave in the model: the title, the properties and the fields kept whole.
fn put_meta(doc: &mut Document, meta: Option<Meta>) {
    if let Some(meta) = meta {
        if let Some(title) = meta.title {
            doc.title = title;
        }
        doc.props = Arc::new(meta.props);
        doc.ext.make_mut().odf.meta = meta.kept;
    }
}

/// Puts what the read kept in the model: the font table, the title and the properties, `meta.xml`'s other fields, `settings.xml`, the roots, `mimetype`, the manifest's rows and the package
/// entries as [`Document::parts`]. `package` is `None` for a flat drawing, which has no package: its faces are all that it gives. `pictures` are the names of the entries the model holds
/// as pictures. Everything is charged before it is made.
pub(crate) fn apply(
    r: &mut Reader<'_>,
    doc: &mut Document,
    mut held: Held,
    package: Option<&Package>,
    pictures: &HashSet<&str>,
) -> Result<(), OdgError> {
    doc.fonts = Arc::new(std::mem::take(&mut r.faces).into_table());
    let Some(package) = package else {
        // A flat drawing has no package: its faces, its `office:meta` and `office:settings`, held as a package's `meta.xml` and `settings.xml` are, and the attributes of its one
        // root, held as the roots of `content.xml` and `styles.xml` (the namespace declarations the markup it keeps needs), are all that it gives.
        put_meta(doc, held.meta.take());
        let ext = doc.ext.make_mut();
        if let Some(settings) = held.settings.take() {
            ext.odf.settings = Some(settings);
        }
        ext.odf.roots = std::mem::take(&mut held.roots);
        return Ok(());
    };
    // Which of `meta.xml` and `settings.xml` were read into fields: the others are package entries.
    let (meta_held, settings_held) = (held.meta.is_some(), held.settings.is_some());
    put_meta(doc, held.meta.take());
    let manifest = package.manifest();
    // The roots: the manifest's own, beside those of the parts the read parsed.
    held.root(r, MANIFEST_PATH, &manifest.root_attrs)?;
    let ext = doc.ext.make_mut();
    if let Some(text) = package.mimetype_text() {
        charge(r, 1, text.len())?;
        ext.odf.mimetype = Some(Arc::from(text));
    }
    if let Some(settings) = held.settings.take() {
        ext.odf.settings = Some(settings);
    }
    ext.odf.roots = std::mem::take(&mut held.roots);
    // What the containers of the parts hold that no field reads: their attributes after the roots', and their children.
    std::mem::take(&mut r.unread).put(&mut ext.odf);
    // The entries, in the order of the directory, and the rows they take with them.
    let mut consumed = vec![false; manifest.entries.len()];
    let mut parts: Vec<Arc<Part>> = Vec::new();
    for entry in package.entries() {
        let name = entry.name.as_str();
        if is_read_elsewhere(name, meta_held, settings_held, pictures) {
            continue;
        }
        let bytes: Arc<[u8]> = if entry.folder {
            Arc::from(&[][..])
        } else {
            match package.part_shared(name) {
                Some(bytes) => bytes,
                // Damaged, and named by the package's warning: there are no bytes to keep.
                None => continue,
            }
        };
        if parts.len() >= MAX_PARTS {
            return Err(OdgError::TooComplex { what: "package entries", max: MAX_PARTS as u64 });
        }
        let row = manifest.position(name).and_then(|at| manifest.entries.get(at).map(|row| (at, row)));
        let (media_type, other): (&str, Vec<(String, String)>) = match row {
            Some((_, row)) => (
                row.media_type.as_deref().unwrap_or_default(),
                row.attrs.iter().enumerate().filter(|(at, _)| Some(*at) != row.media_type_at).map(|(_, p)| p.clone()).collect(),
            ),
            None => ("", Vec::new()),
        };
        let (items, pair_bytes) = pairs_cost(&other);
        let len = bytes.len() as u64;
        // The names and the media type are charged by their bytes, and the entry's own bytes to the media budget before the part is made.
        charge(r, PART + items, name.len().saturating_add(media_type.len()).saturating_add(pair_bytes))?;
        r.media_left =
            r.media_left.checked_sub(len).ok_or(OdgError::TooComplex { what: "bytes of package entries kept", max: r.limits.max_media_bytes })?;
        r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(len);
        r.saw_attrs(other.len());
        if let Some((at, _)) = row
            && let Some(slot) = consumed.get_mut(at)
        {
            *slot = true;
        }
        parts.push(Arc::new(Part {
            format: Format::Odf,
            name: Arc::from(name),
            media_type: Arc::from(media_type),
            role: role_of(name),
            manifest: raw_attrs(&other),
            rels: Arc::new(Vec::new()),
            bytes,
        }));
    }
    // Every row no part took is held by the name it gives (a row with no name under the empty one).
    let ext = doc.ext.make_mut();
    for (row, taken) in manifest.entries.iter().zip(&consumed) {
        if !*taken {
            let (items, bytes) = pairs_cost(&row.attrs);
            charge(r, ENTRY + items, row.full_path.len().saturating_add(bytes))?;
            r.saw_attrs(row.attrs.len());
            ext.odf.manifest_rows.push((Arc::from(row.full_path.as_str()), raw_attrs(&row.attrs)));
        }
    }
    for attrs in &manifest.unnamed_rows {
        let (items, bytes) = pairs_cost(attrs);
        charge(r, ENTRY + items, bytes)?;
        r.saw_attrs(attrs.len());
        ext.odf.manifest_rows.push((Arc::from(""), raw_attrs(attrs)));
    }
    for part in parts {
        doc.parts.push(part);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    #[test]
    fn a_name_says_what_an_entry_is_for() {
        for (name, role) in [
            ("Thumbnails/thumbnail.png", PartRole::Thumbnail),
            ("Thumbnails/", PartRole::Other),
            ("Basic/Standard/Module1.xml", PartRole::MacroProject),
            ("Basic/", PartRole::MacroProject),
            ("Scripts/python/x.py", PartRole::MacroProject),
            ("META-INF/macrosignatures.xml", PartRole::MacroSignature),
            ("META-INF/documentsignatures.xml", PartRole::Signature),
            ("ObjectReplacements/Object 1", PartRole::ObjectReplacement),
            ("Object 1/content.xml", PartRole::ObjectFile),
            ("Object 12/", PartRole::ObjectFile),
            ("Object /content.xml", PartRole::Other),
            ("Object 1", PartRole::Other),
            ("Object x/y", PartRole::Other),
            ("Fonts/a.ttf", PartRole::Font),
            ("Configurations2/accelerator/current.xml", PartRole::Other),
            ("manifest.rdf", PartRole::Other),
            ("", PartRole::Other),
        ] {
            assert_eq!(role_of(name), role, "{name:?}");
        }
    }

    fn el(text: &str) -> El {
        diacraft_container::xml::parse(text.as_bytes()).expect("well-formed")
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    /// Attributes cost two items a pair and the bytes of both strings; none cost nothing; the sums saturate.
    #[test]
    fn attributes_cost_two_items_a_pair_and_the_bytes_of_both_strings() {
        assert_eq!(pairs_cost(&[]), (0, 0));
        assert_eq!(pairs_cost(&[pair("a", "bc"), pair("de", "")]), (4, 5));
        // A thousand pairs of 1 MiB each (the cap of an attribute is 1 MiB) are the sum, and no more than the sum.
        let long = "x".repeat(1 << 20);
        let many: Vec<(String, String)> = (0..1_000).map(|_| pair("k", &long)).collect();
        assert_eq!(pairs_cost(&many), (2_000, 1_000 * ((1 << 20) + 1)));
    }

    /// A charge is the items and the bytes in whole items, taken from the read's items before anything is made; a charge no room is left for is refused and takes nothing; the sum never
    /// wraps.
    #[test]
    fn a_charge_is_items_and_whole_items_of_bytes_and_a_charge_past_the_room_is_refused() {
        let limits = Limits { max_items: 10, ..Limits::default() };
        let mut r = Reader::new(&limits);
        assert!(charge(&mut r, 3, 65).is_ok(), "3 items and 65 bytes, which are 2 items");
        assert_eq!(r.report.work.items, 5);
        assert!(charge(&mut r, 0, 1).is_ok(), "a byte is an item");
        assert_eq!(r.report.work.items, 6);
        assert!(charge(&mut r, 5, 0).is_err(), "5 more of the 4 left");
        assert_eq!(r.report.work.items, 6, "a refused charge takes nothing");
        assert!(charge(&mut r, usize::MAX, usize::MAX).is_err(), "saturates and is refused, never wraps to a small number");
        // Text is charged at nine eighths of its length, rounded up: 227 bytes are 256, the four items left.
        assert!(charge(&mut r, 0, 227).is_ok(), "the four left, in bytes");
    }

    /// The caps summed over their items: the most faces, each of the least a declaration is, are a small part of the items a read has, so the faces' own cap and the read's cap are
    /// two different things, and each is the one that binds where it should.
    #[test]
    fn the_most_faces_of_the_least_size_fit_the_items_of_a_read() {
        let least = FACE + 3 + 2 * 2 + 1;
        assert!(MAX_FACES * least < crate::limits::MAX_ITEMS / 10, "{} items for the most faces", MAX_FACES * least);
        // And the face past the cap is the one the cap refuses, whatever the items left.
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        r.faces.list.resize(MAX_FACES, FaceName { name_u: Arc::from(""), raw: RawAttrs::new(), ext: Default::default() });
        let names = Names::default();
        let tree = el(r#"<style:font-face xmlns:style="urn:x" style:name="one more"/>"#);
        match face(&mut r, &names, &tree, crate::read::styles::File::Styles) {
            Err(OdgError::TooComplex { what: "font faces", max }) => assert_eq!(max, MAX_FACES as u64),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.report.work.items, 0, "nothing was charged for the face that was refused");
    }

    /// A declaration's hash tells two declarations apart by any attribute, child, text or comment, and says the same for equal ones; the walk is counted.
    #[test]
    fn a_hash_tells_declarations_apart_by_anything_they_hold_and_counts_the_walk() {
        let base = r#"<a x="1"><b y="2">t</b><!--c--><?p d?></a>"#;
        let (hash, elements, attrs) = declaration_hash(&el(base));
        assert_eq!((elements, attrs), (2, 2), "an element and a child, and an attribute each");
        assert_eq!(declaration_hash(&el(base)).0, hash);
        // A text and a CDATA section are one kind of node.
        assert_eq!(declaration_hash(&el(r#"<a><b>t</b></a>"#)).0, declaration_hash(&el(r#"<a><b><![CDATA[t]]></b></a>"#)).0);
        for other in [
            r#"<a x="2"><b y="2">t</b><!--c--><?p d?></a>"#,
            r#"<a x="1"><b y="3">t</b><!--c--><?p d?></a>"#,
            r#"<a x="1"><b y="2">u</b><!--c--><?p d?></a>"#,
            r#"<a x="1"><b y="2">t</b><!--d--><?p d?></a>"#,
            r#"<a x="1"><b y="2">t</b><!--c--><?p e?></a>"#,
            r#"<a x="1"><c y="2">t</c><!--c--><?p d?></a>"#,
            r#"<a x="1"><b y="2">t</b><!--c--></a>"#,
            r#"<a x="1" z="0"><b y="2">t</b><!--c--><?p d?></a>"#,
            r#"<a x="1"><b y="2">t</b></a><!--c-->"#,
            r#"<a x="1"><b y="2"><i/>t</b><!--c--><?p d?></a>"#,
        ] {
            assert_ne!(declaration_hash(&el(other)).0, hash, "{other}");
        }
    }

    /// Nodes are the same only when every byte is, at every depth, and a different count is different; the walk is counted.
    #[test]
    fn nodes_are_the_same_only_when_every_byte_of_them_is() {
        let held = |text: &str| raw_element(&el(text));
        let same = |x: &str, y: &str| {
            let (tree, kept) = (el(x), held(y));
            let nodes: Vec<&RawNode> = kept.children.iter().collect();
            same_nodes(&tree.children, &nodes)
        };
        let base = r#"<a><b y="2"><c>t</c></b>u<!--k--></a>"#;
        assert_eq!(same(base, base), (true, 2, 1));
        for other in [
            r#"<a><b y="3"><c>t</c></b>u<!--k--></a>"#,
            r#"<a><b y="2"><c>x</c></b>u<!--k--></a>"#,
            r#"<a><b y="2"><d>t</d></b>u<!--k--></a>"#,
            r#"<a><b y="2"><c>t</c></b>v<!--k--></a>"#,
            r#"<a><b y="2"><c>t</c></b>u</a>"#,
            r#"<a><b y="2"><c>t</c></b>u<!--k--><e/></a>"#,
            r#"<a><b y="2" z="1"><c>t</c></b>u<!--k--></a>"#,
        ] {
            assert!(!same(base, other).0, "{other}");
        }
    }

    /// A plain text element has no attribute and nothing but text; layout is white space between elements.
    #[test]
    fn a_plain_field_is_text_alone_and_layout_is_white_space() {
        assert_eq!(plain_text(&el("<a>x y</a>")).as_deref(), Some("x y"));
        assert_eq!(plain_text(&el("<a/>")).as_deref(), Some(""));
        assert_eq!(plain_text(&el("<a><![CDATA[c]]>d</a>")).as_deref(), Some("cd"));
        for not in [r#"<a k="v">x</a>"#, r#"<a xmlns:x="urn:x">x</a>"#, "<a>x<b/></a>", "<a><!--c-->x</a>"] {
            assert_eq!(plain_text(&el(not)), None, "{not}");
        }
        assert!(is_layout(&Node::Text(" \n\t".to_owned())) && is_layout(&Node::Text(String::new())));
        assert!(!is_layout(&Node::Text(" x ".to_owned())) && !is_layout(&Node::Comment(" ".to_owned())));
    }

    /// Every field of `meta.xml` has a slot of its own in the array of those seen.
    #[test]
    fn every_field_has_a_slot_of_its_own() {
        let fields = [
            Field::Title,
            Field::Description,
            Field::Subject,
            Field::Keyword,
            Field::InitialCreator,
            Field::Creator,
            Field::CreationDate,
            Field::Date,
            Field::PrintDate,
            Field::Language,
        ];
        assert_eq!(fields.len(), Field::COUNT);
        let mut seen = [false; Field::COUNT];
        for f in fields {
            assert!(!std::mem::replace(&mut seen[f.slot()], true), "two fields in one slot");
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(Field::of(Ns::Dc, "title").map(Field::slot), Some(0));
        assert!(Field::of(Ns::Meta, "title").is_none() && Field::of(Ns::Other, "title").is_none());
    }
}
