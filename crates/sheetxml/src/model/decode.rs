//! Decoding: the trees of the vocabulary (`vocab.rs`) back to the model, within a [`Budget`].
//!
//! **Charged before it is stored.** Every element costs one item and each attribute one; a text, a name or a kept string costs its bytes; a
//! formula costs [`FORMULA_NODE_BYTES`] a node of its parse, which is limited to what is left; a picture costs the size its record states, before it is
//! fetched; each shape counts against the model's shape cap and its depth against the depth cap before anything is built for it. A cap of the model is
//! checked where the thing it counts is made, so that a model past one is never built, and the finished document is checked once more
//! ([`Document::check_limits`]), as the readers of both formats do.
//!
//! **Unknown content** (the design's 1.4). An attribute or an element that the vocabulary does not have, or that is in another namespace, is what a newer
//! build wrote. It is kept on the object it sat in, in that object's `native` slot (`ext.native`), as the attributes as written and the elements as kept
//! elements, each with the namespace declarations its names use, and each node with the number of known child elements that came before it: so that
//! it is written back, at the same place, by the same codec ([`Encoder::native_end`](super::encode::Encoder)). A record that has no slot (a colour
//! entry, a part, a relationship, a content-type row, a custom property, a definition, a held value) keeps its unknown content in the native slot of the
//! nearest object that has one, under the record's kind and key (NP7). Content that has no place at all (an attribute of a text marker, a long field of a
//! name this build does not know) is counted, and one note, [`DecodeNote::Unknown`], says how many.
//!
//! **Not recursive but for shapes.** A kept element is copied with a stack of its own (`kept.rs`), and so is an unknown one. The shape tree recurses one
//! small frame a level, to the model's cap of 200, and the depth is checked before the frame is made.

use std::borrow::{Borrow, Cow};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::XmlLimits;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{El, Node, ParseOptions, Pull, PullLimits, XmlError, XmlText, parse_size};
use diacraft_doc::{
    Cell, CellExtra, CellName, ColorEntry, Connect, Connects, DocProps, Document, Ext, FaceName, ForeignData, FormulaSlot, INHERIT_MARKER, Kept,
    Links, MAX_CELLS_PER_CONTAINER, MAX_COLOR_ENTRIES, MAX_MASTERS, MAX_NAME_BYTES, MAX_PAGES, MAX_PARTS, MAX_ROWS_PER_SECTION,
    MAX_SECTIONS_PER_SHEET, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, MAX_STYLES, Master, Media, NO_FORMULA_MARKER, Page, Part, Passthrough, RawAttrs,
    RawElement, RawNode, Row, Rows, Section, SectionName, Shape, ShapeList, ShapeType, Sheet, Style, Text, TextPart, Theme, Value, VsdxHeld,
};

use super::cost::{
    ATTR_BYTES, ATTR_PAIR_BYTES, PLACED_BYTES, RAW_NODE_BYTES, TEXT_PART_BYTES, UNKNOWN_ATTR_BYTES, UNKNOWN_COMMENT_BYTES, UNKNOWN_ELEMENT_BYTES,
    UNKNOWN_PI_BYTES, UNKNOWN_TEXT_BYTES, cells_bytes, element_bytes, push_bytes, text_bytes, tree_text_bytes, vec_bytes,
};
use super::encode::MAX_KEPT_DEPTH;
use super::escape::{ATTR_WRITTEN_MAX, attr_written_len, base64_string, comment_writable, pi_writable};
use super::kept::Rest;
use super::share::{Def, Defs, Made, SHARE_ENTRY_BYTES, SHARED_REF_BYTES, Share, Sharing, Unit};
use super::stamp::{Stamp, stamp_of_bytes};
use super::unitfp::fingerprint;
use super::vocab::{at, el};
use super::{
    BlobId, BlobRecord, BlobSource, Budget, CodecError, DecodeNote, FORMAT_PART_ITEMS, FORMULA_NODE_BYTES, OPEN_ELEMENT_ITEMS, OPEN_PART_BYTES,
    PREFIX, PartLists, PartSource, URN,
};
use crate::ns::{MAX_PREFIXES_PER_ELEMENT, Scope, Vocabulary, frame_bytes};

/// The vocabulary a reader of the codec's parts reads: the codec's namespace and no other.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ours;

impl Vocabulary for Ours {
    fn reads(self, uri: Option<&str>) -> bool {
        uri.is_some_and(diacraft_doc::is_diacraft_urn)
    }
}

/// What came of giving a unit the name it wrote ([`Dec::define`]).
enum Named {
    /// The name is the unit's.
    Yes,
    /// The unit has no name: it is read, and no `same` can take it. (The name is empty, or is another unit's already: a second unit of one name is read as a unit of
    /// no name, its shape keeps its sheet, and a `same` that names the name takes the first.)
    Unnamed,
}

/// What the entry of a named unit costs the model beside its name: the name's `String`, the pointer to the copy and the table's room.
pub const DEF_ENTRY_BYTES: u64 = 96;

/// The children of which an element holds at most one, by local name: a second is not read (see [`Dec::children`]).
const ONCE: [&str; 18] = [
    el::PROPS,
    el::SHEET,
    el::SOLUTION,
    el::VSDX_HELD,
    el::EXT,
    el::FOREIGN,
    el::TEXT,
    el::PICTURE,
    el::MANIFEST,
    el::PROPS_REST,
    el::EXTRA,
    el::SETTINGS,
    el::META,
    el::DRAWING_PAGE_STYLE,
    el::MASTER_PAGE_ATTRS,
    el::LAYOUT_PROPS,
    el::UNITS,
    el::UNMAPPED,
];

/// The most nodes the formula crate lets one formula have: below it a budget is what refused a formula.
const FORMULA_MAX_NODES: u64 = diacraft_formula::MAX_NODES as u64;

/// How many pictures of one blob (each of its own part name and content type) a decode keeps to share between shapes: more than a handful would make
/// the search for one quadratic, and a blob named with a different part name by each of a million shapes is not worth sharing.
const MEDIA_SHARED_PER_BLOB: usize = 8;

/// What gives the next child of a part's root, for [`Dec::page_with`] and [`Dec::master_with`]: it is given the decode, so that a stream can hold back the room of a child's
/// tree while the child is read, and gives `None` when there are no more.
pub(super) type NextChild<'k> = dyn FnMut(&mut Dec<'_>) -> Result<Option<Cow<'k, El>>, CodecError> + 'k;

/// What reads the fields an `s` child of an element holds, for a part read as a stream: the child, and the scope inside it.
pub(super) type LongField<'a> = dyn FnMut(&mut Dec<'_>, &El, &Scope<Ours>) -> Result<(), CodecError> + 'a;

/// What the children of an element before one leave for it: how many of them the codec knew (the place of an unknown element is the number of known children before
/// it), and which of the elements that an element holds only one of have been seen.
pub(super) struct ChildLoop {
    known: u32,
    seen: [bool; ONCE.len()],
}

impl ChildLoop {
    pub(super) fn new() -> ChildLoop {
        ChildLoop { known: 0, seen: [false; ONCE.len()] }
    }
}

/// What the reading of a unit can leave beside it ([`Dec::footprint`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Footprint {
    notes: usize,
    blobs: u64,
    keyed: (usize, usize),
}

pub(crate) struct Dec<'b> {
    pub(super) budget: &'b mut Budget,
    pub(super) notes: &'b mut Vec<DecodeNote>,
    blobs: &'b dyn BlobSource,
    /// What the document part states of each blob, when a document is being decoded: a blob is checked against its record. `None` for a fragment.
    pub(super) records: Option<HashMap<u32, BlobRecord>>,
    cache: HashMap<u32, Option<Arc<[u8]>>>,
    media: HashMap<u32, Vec<Arc<Media>>>,
    noted: HashSet<u32>,
    unknown: u64,
    shapes: usize,
    part: &'static str,
    /// The attributes the last [`Dec::fields`] did not know, as written: the object that was read takes them ([`Dec::native_attrs`]), or the next call
    /// of `fields` counts them as lost.
    pending: Vec<(String, String)>,
    /// The unknown content of the records read since the owner that collects them began, each as a kept element named by its record's kind and key
    /// (NP7). The owner (the document, a shape, a row) takes them into its native slot when it is done.
    pub(super) keyed: KeyedList,
    /// The sheets and sections read so far, so that an equal one is the copy of the first ([`super::share`]).
    share: Share,
    /// The sheets and sections of the part in hand that its elements named.
    defs: Defs,
    /// The name the unit being read gave itself, until the unit is done ([`Dec::shared`]).
    pending_def: Option<String>,
}

/// The cells of one place as they are read, to be built into a map when the place is done ([`Dec::build_cells`]).
#[derive(Default)]
struct CellList(Vec<(CellName, Cell)>);

/// The unknown content of the records of one owner, and the keys the records have had so far.
///
/// The first record of a key (in the order they are read, and then in the order they are written) is the one that takes the entry named by it: a record
/// with the same key after it is a record of the same name and its unknown content is not kept, and is counted, so that the entries are one for each name
/// and the records that take them back are the ones that had them.
#[derive(Default)]
pub(super) struct KeyedList {
    /// The kept elements, named by kind and key (NP7).
    pub(super) entries: Vec<RawElement>,
    /// The hashes of the keys seen, so that a second record of a key is known.
    seen: HashSet<u64>,
    /// The hashes added to `seen`, in order, so that the records of a part that is dropped can be taken back.
    added: Vec<u64>,
}

impl KeyedList {
    /// What to give [`KeyedList::rollback`] to take back what is added from now on.
    pub(super) fn mark(&self) -> (usize, usize) {
        (self.entries.len(), self.added.len())
    }

    /// Takes back every record added since `mark`: a record that is dropped (a part whose bytes are lost) takes its relationships' unknown content with it.
    pub(super) fn rollback(&mut self, mark: (usize, usize)) {
        self.entries.truncate(mark.0);
        while self.added.len() > mark.1 {
            if let Some(h) = self.added.pop() {
                self.seen.remove(&h);
            }
        }
    }

    /// Whether `kind` and `key` have not been seen in this owner; they are, from now on.
    pub(super) fn first(&mut self, kind: &str, key: &str) -> bool {
        let h = super::stamp::Digest::new("keyed").text(kind).text(key).finish().0;
        let first = self.seen.insert(h);
        if first {
            self.added.push(h);
        }
        first
    }
}

pub(super) fn limit(what: &'static str, max: usize) -> CodecError {
    CodecError::Limit { what, max: u64::try_from(max).unwrap_or(u64::MAX) }
}

/// Puts `native` in the slot when it holds anything.
pub(super) fn set_native<T: Default + Clone + PartialEq>(slot: &mut Ext<T>, native: Kept) {
    if native != Kept::default() {
        slot.make_mut().native = native;
    }
}

impl<'b> Dec<'b> {
    pub(crate) fn new(budget: &'b mut Budget, notes: &'b mut Vec<DecodeNote>, blobs: &'b dyn BlobSource) -> Dec<'b> {
        let share = Share::new(if budget.shares() { Sharing::Keep } else { Sharing::Off });
        Dec {
            budget,
            notes,
            blobs,
            records: None,
            cache: HashMap::new(),
            media: HashMap::new(),
            noted: HashSet::new(),
            unknown: 0,
            shapes: 0,
            part: "document",
            pending: Vec::new(),
            keyed: KeyedList::default(),
            share,
            defs: Defs::default(),
            pending_def: None,
        }
    }

    /// This decode, sharing as `mode` says: the save's check keeps no copy of what it reads ([`Sharing::Count`]).
    pub(crate) fn sharing(mut self, mode: Sharing) -> Dec<'b> {
        if self.share.mode != Sharing::Off {
            self.share = Share::new(mode);
        }
        self
    }

    pub(super) fn part(&self) -> &'static str {
        self.part
    }

    fn begin_part(&mut self, part: &'static str) {
        self.part = part;
        self.shapes = 0;
        // The names of the units of a part are the part's own.
        self.defs.clear();
        self.pending_def = None;
    }

    pub(super) fn saw_unknown(&mut self) {
        self.unknown = self.unknown.saturating_add(1);
    }

    pub(super) fn bad(&self, why: &'static str) -> CodecError {
        CodecError::malformed(self.part, why)
    }

    // ------------------------------------------------------------------------------------------------------------ charges and fields

    /// An element the codec reads: its items (itself and each attribute), and the bytes of the model it builds, by its kind and its attributes, charged before
    /// anything is built. The text of its attributes is charged by [`Dec::fields`].
    pub(super) fn charge_el(&mut self, e: &El) -> Result<(), CodecError> {
        let attrs = u64::try_from(e.attrs.len()).unwrap_or(u64::MAX);
        self.budget.charge_items(1u64.saturating_add(attrs))?;
        self.budget.charge_bytes(element_bytes(e.local()).saturating_add(attrs.saturating_mul(ATTR_BYTES)))
    }

    /// Pushes a kept node onto the list of one element's kept nodes, charging what the push adds to the list (the room of four at the first, and the header).
    pub(super) fn push_kept(&mut self, list: &mut Passthrough, after: u32, node: RawNode) -> Result<(), CodecError> {
        self.budget.charge_bytes(push_bytes(list.len(), PLACED_BYTES))?;
        list.push(after, node);
        Ok(())
    }

    /// Pushes an attribute onto the kept attributes of one element, charging what the push adds to the list. **A declaration of the namespace that earlier builds wrote**
    /// (`diacraft_doc::DIACRAFT_URN_LEGACY`) **is kept as a declaration of the current one** (APPID), so that what a build keeps whole of a file made before is what it writes.
    pub(super) fn push_attr(&mut self, raw: &mut RawAttrs, name: &str, value: &str) -> Result<(), CodecError> {
        self.budget.charge_bytes(push_bytes(raw.len(), ATTR_PAIR_BYTES))?;
        let is_declaration = name == "xmlns" || name.starts_with("xmlns:");
        raw.push(name, if is_declaration { diacraft_doc::current_namespace(value) } else { value });
        Ok(())
    }

    /// The list of the nodes inside a kept element of `n` children: the room it makes for them, charged where the element is opened.
    pub(super) fn charge_children(&mut self, n: usize) -> Result<(), CodecError> {
        self.budget.charge_bytes(vec_bytes(n, RAW_NODE_BYTES))
    }

    /// The scope inside `e`, `sc` being the one outside it. **What its namespace declarations make is charged first**, and an element that declares more
    /// prefixes than [`MAX_PREFIXES_PER_ELEMENT`] is refused before anything is made of them: a frame is a table of the prefixes,
    /// and a part of millions of them was 337 bytes each of memory that no charge had paid for.
    pub(super) fn enter(&mut self, sc: &Scope<Ours>, e: &El) -> Result<Scope<Ours>, CodecError> {
        if let Some(bytes) = frame_charge(e)? {
            self.budget.charge_bytes(bytes)?;
        }
        Ok(sc.enter(e))
    }

    /// An element nothing keeps: its items are charged, and it is counted. It builds nothing.
    pub(super) fn unknown_el(&mut self, e: &El) -> Result<(), CodecError> {
        self.budget.charge_items(1u64.saturating_add(u64::try_from(e.attrs.len()).unwrap_or(u64::MAX)))?;
        self.saw_unknown();
        Ok(())
    }

    /// Counts the attributes the last `fields` left that nobody took.
    fn flush_pending(&mut self) {
        let lost = u64::try_from(self.pending.len()).unwrap_or(u64::MAX);
        self.unknown = self.unknown.saturating_add(lost);
        self.pending.clear();
    }

    /// Reads the fields `e` holds in its attributes and in its `s` children, calling `apply` with each field's name and text (base64 already
    /// undone): `apply` says whether it knew the field. An attribute it did not know is set aside, as written, for [`Dec::native_attrs`]. The element's
    /// items and the bytes of every value are charged before a value is copied. `sc` is the scope inside `e`, which the `s` children are read in.
    pub(super) fn fields<F>(&mut self, e: &El, sc: &Scope<Ours>, mut apply: F) -> Result<(), CodecError>
    where
        F: FnMut(&mut Dec<'_>, &str, Cow<'_, str>) -> Result<bool, CodecError>,
    {
        self.fields_attrs(e, sc, &mut apply)?;
        for child in e.elements() {
            if !child.is(el::LONG) {
                continue;
            }
            let csc = self.enter(sc, child)?;
            if !csc.is_ours(child) {
                continue;
            }
            self.long_field(child, &csc, &mut apply)?;
        }
        Ok(())
    }

    /// The attributes of `e` as [`Dec::fields`] reads them, and `e` itself charged: its items and the bytes of every value, before a value is copied. A part of a
    /// stream reads its root's attributes here and each `s` child of the root, when it comes, with [`Dec::long_field`].
    pub(super) fn fields_attrs<F>(&mut self, e: &El, _sc: &Scope<Ours>, apply: &mut F) -> Result<(), CodecError>
    where
        F: FnMut(&mut Dec<'_>, &str, Cow<'_, str>) -> Result<bool, CodecError>,
    {
        self.flush_pending();
        self.charge_el(e)?;
        for (key, value) in &e.attrs {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            self.budget.charge_text(value.len())?;
            let known = match key.strip_suffix(at::B64_SUFFIX) {
                Some(base) => {
                    let text = base64_string(value).ok_or_else(|| self.bad("base64 text that is not base64 of text"))?;
                    apply(self, base, Cow::Owned(text))?
                }
                None => apply(self, key, Cow::Borrowed(value.as_str()))?,
            };
            if !known {
                // Kept, so its name is charged as its value was.
                self.budget.charge_text(key.len())?;
                self.pending.push((key.clone(), value.clone()));
            }
        }
        Ok(())
    }

    /// One field held in an `s` child (a text too long for an attribute): `child` is the element, `csc` the scope inside it, and it is the codec's own.
    pub(super) fn long_field<F>(&mut self, child: &El, _csc: &Scope<Ours>, apply: &mut F) -> Result<(), CodecError>
    where
        F: FnMut(&mut Dec<'_>, &str, Cow<'_, str>) -> Result<bool, CodecError>,
    {
        let (mut name, mut b64) = (String::new(), false);
        self.charge_el(child)?;
        for (k, v) in &child.attrs {
            if k == at::N_LONG {
                name.clone_from(v);
            } else if k == at::B64 {
                b64 = self.flag(v)?;
            }
        }
        let text = self.element_text(child)?;
        let text = if b64 { base64_string(&text).ok_or_else(|| self.bad("base64 text that is not base64 of text"))? } else { text };
        if !apply(self, &name, Cow::Owned(text))? {
            // A long field this build does not know: it cannot be kept as an attribute, and is counted.
            self.saw_unknown();
        }
        Ok(())
    }

    /// The direct text of `e`, charged by its bytes (and one item for each node of it) before it is joined.
    pub(super) fn element_text(&mut self, e: &El) -> Result<String, CodecError> {
        let mut total = 0usize;
        let mut nodes = 0u64;
        for node in &e.children {
            if let Node::Text(t) | Node::CData(t) = node {
                total = total.saturating_add(t.len());
                nodes = nodes.saturating_add(1);
            }
        }
        self.budget.charge_items(nodes)?;
        self.budget.charge_text(total)?;
        let mut out = String::new();
        out.try_reserve(total).map_err(|_| limit("bytes of the model in memory", total))?;
        for node in &e.children {
            if let Node::Text(t) | Node::CData(t) = node {
                out.push_str(t);
            }
        }
        Ok(out)
    }

    pub(super) fn u32(&self, text: &str) -> Result<u32, CodecError> {
        text.parse::<u32>().map_err(|_| self.bad("a number that is not a number"))
    }

    pub(super) fn flag(&self, text: &str) -> Result<bool, CodecError> {
        match text {
            "1" | "true" => Ok(true),
            "0" | "false" => Ok(false),
            _ => Err(self.bad("a flag that is neither 1 nor 0")),
        }
    }

    pub(super) fn arc(text: &str) -> Arc<str> {
        Arc::from(text)
    }

    /// The attributes the last `fields` did not know, as the model keeps them: as written, each prefixed name after the declaration of its prefix, taken
    /// from the scope, so that they are written back as the XML they were. `sc` is the scope inside the element they were on. Call it right after `fields`.
    pub(super) fn native_attrs(&mut self, sc: &Scope<Ours>) -> Result<RawAttrs, CodecError> {
        let mut raw = RawAttrs::new();
        let mut declared: BTreeSet<String> = BTreeSet::new();
        for (key, value) in std::mem::take(&mut self.pending) {
            // What is kept is written back as it stands, and the writer's cap is the reader's: a value that would be too long once it is spelled the way the
            // writer spells it (a quote is six bytes) is refused here, so that a drawing that opens is one that saves.
            if attr_written_len(&value) > ATTR_WRITTEN_MAX {
                return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
            }
            self.budget.charge_bytes(UNKNOWN_ATTR_BYTES)?;
            if let Some((prefix, _)) = key.split_once(':')
                && prefix != "xml"
                && declared.insert(prefix.to_owned())
                && let Some(uri) = sc.bound(prefix)
            {
                if prefix == PREFIX {
                    // The codec's own prefix is declared by every part it writes, for its own namespace: kept, it would be declared twice. Bound to another
                    // namespace it could not be written back at all, and every element of the part would change namespace with it.
                    if !diacraft_doc::is_diacraft_urn(&uri) {
                        return Err(self.bad("the codec's own prefix bound to another namespace on an attribute"));
                    }
                } else {
                    // The declaration is copied from the scope, where the reader read it as written in the file: a namespace of quotes is spelled longer by the
                    // writer than it was in the file, and one the writer cannot spell within the reader's cap would be a part that does not open.
                    if attr_written_len(&uri) > ATTR_WRITTEN_MAX {
                        return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
                    }
                    self.budget.charge_text(uri.len())?;
                    self.push_attr(&mut raw, &format!("xmlns:{prefix}"), &uri)?;
                }
            }
            self.push_attr(&mut raw, &key, &value)?;
        }
        Ok(raw)
    }

    /// An object's native content in the making: its unknown attributes, taken from the last `fields`, and no nodes yet.
    pub(super) fn start_native(&mut self, sc: &Scope<Ours>) -> Result<Kept, CodecError> {
        Ok(Kept { raw: self.native_attrs(sc)?, kept: Passthrough::new() })
    }

    /// Calls `each` with every child element of `e` that is the codec's own and is not an `s` (those are fields): `each` says whether it took the
    /// element. An element of another namespace, and one `each` did not take, is what a newer build wrote: with a `native` list it is kept there, with
    /// the number of known children before it, and without one it is charged and counted. `sc` is the scope inside `e`.
    pub(super) fn children<F>(&mut self, e: &El, sc: &Scope<Ours>, mut native: Option<&mut Passthrough>, mut each: F) -> Result<(), CodecError>
    where
        F: FnMut(&mut Dec<'_>, &El, &str, &Scope<Ours>) -> Result<bool, CodecError>,
    {
        let mut state = ChildLoop::new();
        for child in e.elements() {
            self.child_step(&mut state, child, sc, native.as_deref_mut(), &mut each, None)?;
        }
        Ok(())
    }

    /// One child of an element, as [`Dec::children`] treats each: `state` is what the elements before it left (how many known children, and which of the once-only
    /// ones were seen). A part that is read as a stream calls this for each child as it comes, with `long` to read the fields an `s` child holds (the elements of a
    /// tree have those read before, by [`Dec::fields`]).
    pub(super) fn child_step<F>(
        &mut self,
        state: &mut ChildLoop,
        child: &El,
        sc: &Scope<Ours>,
        native: Option<&mut Passthrough>,
        each: &mut F,
        long: Option<&mut LongField<'_>>,
    ) -> Result<(), CodecError>
    where
        F: FnMut(&mut Dec<'_>, &El, &str, &Scope<Ours>) -> Result<bool, CodecError>,
    {
        let csc = self.enter(sc, child)?;
        let consumed = if !csc.is_ours(child) {
            false
        } else if child.local() == el::LONG {
            if let Some(read) = long {
                read(self, child, &csc)?;
            }
            true
        } else {
            match ONCE.iter().position(|once| *once == child.local()) {
                // A second of what an element holds one of is not read: the unknown content that the records of the first kept would be left with
                // none (their entries are in the owner's list under keys the second's records would not take back).
                Some(i) if state.seen.get(i).copied().unwrap_or(false) => {
                    self.unknown_el(child)?;
                    true
                }
                Some(i) => {
                    if let Some(flag) = state.seen.get_mut(i) {
                        *flag = true;
                    }
                    each(self, child, child.local(), &csc)?
                }
                None => each(self, child, child.local(), &csc)?,
            }
        };
        if consumed {
            state.known = state.known.saturating_add(1);
            return Ok(());
        }
        match native {
            Some(list) => {
                let raw = self.unknown_element(child, sc)?;
                self.push_kept(list, state.known, RawNode::Element(raw))?;
            }
            None => self.unknown_el(child)?,
        }
        Ok(())
    }

    /// An unknown element and all that is in it, as a kept element that writes back as XML that is well-formed where it stands: every namespace declaration
    /// its names use that it does not declare itself is taken from `sc` (the scope it was read in) and put on it. The copy keeps its own stack, at most
    /// [`MAX_KEPT_DEPTH`] levels deep, and every element and string is charged before it is stored.
    pub(super) fn unknown_element(&mut self, root: &El, sc: &Scope<Ours>) -> Result<RawElement, CodecError> {
        struct Frame<'a> {
            name: String,
            attrs: RawAttrs,
            children: Vec<RawNode>,
            /// The characters read since the last node that is kept: text and CDATA, and what a dropped comment or instruction leaves between them, are one run,
            /// as a parse of what is written gives them back (two texts side by side are one when the part is read again). Made a node when another node comes.
            text: String,
            rest: std::slice::Iter<'a, Node>,
        }
        impl Frame<'_> {
            fn flush(&mut self) {
                if !self.text.is_empty() {
                    let run = std::mem::take(&mut self.text);
                    self.children.push(RawNode::Text(Arc::from(run.as_str())));
                }
            }
        }
        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut open = |this: &mut Self, e: &'_ El| -> Result<(String, RawAttrs), CodecError> {
            let attrs = u64::try_from(e.attrs.len()).unwrap_or(u64::MAX);
            this.budget.charge_items(1u64.saturating_add(attrs))?;
            this.budget.charge_bytes(UNKNOWN_ELEMENT_BYTES.saturating_add(attrs.saturating_mul(UNKNOWN_ATTR_BYTES + ATTR_BYTES)))?;
            this.budget.charge_text(e.name.len())?;
            this.charge_children(e.children.len())?;
            used.insert(e.name.split_once(':').map_or(String::new(), |(p, _)| p.to_owned()));
            let mut attrs = RawAttrs::new();
            for (k, v) in &e.attrs {
                if attr_written_len(v) > ATTR_WRITTEN_MAX {
                    return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
                }
                this.budget.charge_text(k.len().saturating_add(v.len()))?;
                if k != "xmlns"
                    && !k.starts_with("xmlns:")
                    && let Some((p, _)) = k.split_once(':')
                    && p != "xml"
                {
                    used.insert(p.to_owned());
                }
                this.push_attr(&mut attrs, k, v)?;
            }
            Ok((e.name.clone(), attrs))
        };
        let (name, attrs) = open(self, root)?;
        let mut stack: Vec<Frame<'_>> = vec![Frame { name, attrs, children: Vec::new(), text: String::new(), rest: root.children.iter() }];
        let mut done = loop {
            let step = match stack.last_mut() {
                None => return Err(self.bad("an unknown element with no frame")),
                Some(top) => top.rest.next(),
            };
            match step {
                Some(Node::El(c)) => {
                    if stack.len() >= MAX_KEPT_DEPTH {
                        return Err(CodecError::Limit { what: "levels of a kept element", max: u64::try_from(MAX_KEPT_DEPTH).unwrap_or(u64::MAX) });
                    }
                    let (name, attrs) = open(self, c)?;
                    if let Some(top) = stack.last_mut() {
                        top.flush();
                    }
                    stack.push(Frame { name, attrs, children: Vec::new(), text: String::new(), rest: c.children.iter() });
                }
                Some(Node::Text(t) | Node::CData(t)) => {
                    self.budget.charge_items(1)?;
                    self.budget.charge_bytes(UNKNOWN_TEXT_BYTES.saturating_add(text_bytes(t.len())))?;
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(t);
                    }
                }
                Some(Node::Comment(c)) => {
                    self.budget.charge_items(1)?;
                    if !comment_writable(c) {
                        // The reader took what the writer cannot write: kept, the drawing would open and could not be saved. It is counted as lost.
                        self.saw_unknown();
                        continue;
                    }
                    self.budget.charge_bytes(UNKNOWN_COMMENT_BYTES.saturating_add(text_bytes(c.len())))?;
                    if let Some(top) = stack.last_mut() {
                        top.flush();
                        top.children.push(RawNode::Comment(Self::arc(c)));
                    }
                }
                Some(Node::Pi { target, data }) => {
                    self.budget.charge_items(1)?;
                    if !pi_writable(target, data) {
                        // An instruction with no target (`<??>`) or a target that is no name (`<?1a d?>`): the reader takes it and the writer cannot write it. It is
                        // counted as lost, and the drawing saves.
                        self.saw_unknown();
                        continue;
                    }
                    self.budget.charge_bytes(UNKNOWN_PI_BYTES.saturating_add(text_bytes(target.len().saturating_add(data.len()))))?;
                    if let Some(top) = stack.last_mut() {
                        top.flush();
                        top.children.push(RawNode::Pi { target: Self::arc(target), data: Self::arc(data) });
                    }
                }
                None => {
                    let Some(mut done) = stack.pop() else { return Err(self.bad("an unknown element with no frame")) };
                    done.flush();
                    let raw = RawElement { name: Arc::from(done.name.as_str()), attrs: done.attrs, children: done.children };
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(RawNode::Element(raw)),
                        None => break raw,
                    }
                }
            }
        };
        // The declarations its names use that it does not make itself: the prefixes in the order of their names, and the default namespace for a name with
        // no prefix.
        let own: HashSet<&str> = root.attrs.iter().map(|(k, _)| k.as_str()).collect();
        let mut declarations: Vec<(String, String)> = Vec::new();
        for prefix in &used {
            let key = if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") };
            if own.contains(key.as_str()) {
                continue;
            }
            if let Some(uri) = sc.bound(prefix) {
                // Copied from the scope as the reader read it, and written as the writer spells it (a quote is six bytes): held to the reader's cap as written.
                if attr_written_len(&uri) > ATTR_WRITTEN_MAX {
                    return Err(limit("bytes of an unknown attribute as written", ATTR_WRITTEN_MAX));
                }
                self.budget.charge_text(uri.len())?;
                let uri = diacraft_doc::current_namespace(&uri).to_owned();
                declarations.push((key, uri));
            }
        }
        if !declarations.is_empty() {
            let own: Vec<(String, String)> = done.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
            declarations.extend(own);
            done.attrs = RawAttrs::from_pairs(declarations);
        }
        Ok(done)
    }

    // ------------------------------------------------------------------------------------------------------------ blobs

    /// The bytes of blob `id`: from the cache, or charged by the size its record states and fetched. `None` when they are lost: there is no record,
    /// the source has none, or what it gave is not what the record states. A fragment has no records and is charged by what the source gave.
    pub(super) fn blob(&mut self, id: u32) -> Result<Option<Arc<[u8]>>, CodecError> {
        if let Some(held) = self.cache.get(&id) {
            return Ok(held.clone());
        }
        let found = match &self.records {
            Some(records) => match records.get(&id) {
                None => None,
                Some(record) => {
                    self.budget.charge_blob(record.size)?;
                    self.blobs
                        .get(BlobId(id))
                        .filter(|bytes| u64::try_from(bytes.len()).ok() == Some(record.size) && stamp_of_bytes(bytes) == record.stamp)
                }
            },
            None => match self.blobs.get(BlobId(id)) {
                Some(bytes) => {
                    self.budget.charge_blob(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
                    Some(bytes)
                }
                None => None,
            },
        };
        self.cache.insert(id, found.clone());
        Ok(found)
    }

    /// The note of a picture whose bytes are lost: one for each blob.
    fn lost_picture(&mut self, id: u32) -> Result<(), CodecError> {
        if self.noted.insert(id) {
            self.budget.charge_items(1)?;
            self.notes.push(DecodeNote::PictureLost { blob: BlobId(id) });
        }
        Ok(())
    }

    /// The note of a part whose bytes are lost.
    pub(super) fn lost_part(&mut self, name: &str) -> Result<(), CodecError> {
        self.budget.charge_items(1)?;
        self.notes.push(DecodeNote::PartLost { name: name.to_owned() });
        Ok(())
    }

    // ------------------------------------------------------------------------------------------------------------ the document

    fn document(&mut self, root: &El, sc: &Scope<Ours>) -> Result<Document, CodecError> {
        let outer = std::mem::take(&mut self.keyed);
        let (mut version, mut title) = (Document::CURRENT_VERSION, Self::arc(""));
        self.fields(root, sc, |d, n, v| {
            match n {
                at::VERSION => version = d.u32(&v)?,
                at::TITLE => title = Self::arc(&v),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut native = self.start_native(sc)?;
        let mut props = DocProps::default();
        let mut colors: Vec<ColorEntry> = Vec::new();
        let mut fonts: Vec<FaceName> = Vec::new();
        let mut sheet: Option<Arc<Sheet>> = None;
        let mut styles: Vec<Arc<Style>> = Vec::new();
        let mut themes: Vec<Arc<Theme>> = Vec::new();
        let mut diacraft = None;
        let mut comments = diacraft_doc::Comments::new();
        let mut parts: Vec<Arc<Part>> = Vec::new();
        let mut vsdx: Option<Arc<VsdxHeld>> = None;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(root, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::PROPS => props = d.props(child, csc)?,
                el::COLOR => {
                    if colors.len() >= MAX_COLOR_ENTRIES {
                        return Err(limit("colour table entries", MAX_COLOR_ENTRIES));
                    }
                    colors.push(d.color(child, csc)?);
                }
                el::FONT => fonts.push(d.font(child, csc)?),
                el::SHEET | el::SAME => {
                    // A place holds one sheet: the first, whether it is a `sheet` or a `same` that stands for one; a second is not read and is counted.
                    if sheet.is_some() {
                        d.unknown_el(child)?;
                    } else if local == el::SHEET {
                        sheet = d.shared::<Sheet>(child, csc)?;
                    } else {
                        sheet = d.same::<Sheet>(child, csc)?;
                    }
                }
                el::STYLE => {
                    if styles.len() >= MAX_STYLES {
                        return Err(limit("styles", MAX_STYLES));
                    }
                    styles.push(Arc::new(d.style(child, csc)?));
                }
                el::THEME => {
                    if let Some(t) = d.theme(child, csc)? {
                        themes.push(Arc::new(t));
                    }
                }
                el::SOLUTION => diacraft = d.solution(child, csc)?.map(Arc::new),
                // A document holds one list of comments: the first, and a second is not read and is counted.
                el::COMMENT_LIST if !comments.is_empty() => d.unknown_el(child)?,
                el::COMMENT_LIST => comments = d.comment_list(child, csc)?,
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, odf| d.odf_doc(o, osc, odf))?,
                el::PART => {
                    if parts.len() >= MAX_PARTS {
                        return Err(limit("parts", MAX_PARTS));
                    }
                    if let Some(p) = d.package_part(child, csc)? {
                        parts.push(Arc::new(p));
                    }
                }
                el::VSDX_HELD => vsdx = Some(Arc::new(d.vsdx_held(child, csc)?)),
                // The lists were read first, by `part_lists`; each is charged here as the element it is.
                el::PAGE_REF | el::MASTER_REF | el::BLOB => {
                    // The lists were read by `part_lists` and are charged here as the elements they are. What else is on one is a newer build's, which a
                    // record with no slot cannot keep: it is counted.
                    let known: &[&str] =
                        if local == el::BLOB { &[at::ID, at::PATH, at::MEDIA_TYPE, at::SIZE, at::STAMP] } else { &[at::ID, at::PART] };
                    d.fields(child, csc, |_, n, _| Ok(known.contains(&n)))?;
                    d.children(child, csc, None, |_, _, _, _| Ok(false))?;
                }
                el::KEPT => {
                    d.rest_child(&mut rest, child, csc)?;
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        for record in std::mem::replace(&mut self.keyed, outer).entries {
            self.push_kept(&mut native.kept, 0, RawNode::Element(record))?;
        }
        set_native(&mut ext, native);
        Ok(Document {
            version,
            title,
            props: Arc::new(props),
            pages: diacraft_doc::SharedVec::new(),
            masters: diacraft_doc::SharedVec::new(),
            styles: styles.into_iter().collect(),
            themes: themes.into_iter().collect(),
            colors: Arc::new(colors),
            fonts: Arc::new(fonts),
            sheet: sheet.unwrap_or_else(|| self.share.default_sheet()),
            passthrough: rest.kept,
            diacraft,
            ext,
            parts: parts.into_iter().collect(),
            vsdx,
            comments,
        })
    }

    fn props(&mut self, e: &El, sc: &Scope<Ours>) -> Result<DocProps, CodecError> {
        let mut p = DocProps::default();
        self.fields(e, sc, |_, n, v| {
            let slot = match n {
                at::SUBJECT => &mut p.subject,
                at::CREATOR => &mut p.creator,
                at::LAST_MODIFIED_BY => &mut p.last_modified_by,
                at::KEYWORDS => &mut p.keywords,
                at::DESCRIPTION => &mut p.description,
                at::CATEGORY => &mut p.category,
                at::COMPANY => &mut p.company,
                at::MANAGER => &mut p.manager,
                at::HYPERLINK_BASE => &mut p.hyperlink_base,
                at::APP_VERSION => &mut p.app_version,
                at::CREATED => &mut p.created,
                at::MODIFIED => &mut p.modified,
                at::LAST_PRINTED => &mut p.last_printed,
                at::LANGUAGE => &mut p.language,
                _ => return Ok(false),
            };
            *slot = Some(Self::arc(&v));
            Ok(true)
        })?;
        let mut custom = Vec::new();
        self.children(e, sc, None, |d, child, local, csc| {
            if local == el::CUSTOM {
                custom.push(d.custom_prop(child, csc)?);
                return Ok(true);
            }
            Ok(false)
        })?;
        p.custom = custom;
        Ok(p)
    }

    fn color(&mut self, e: &El, sc: &Scope<Ours>) -> Result<ColorEntry, CodecError> {
        let (mut ix, mut rgb) = (0u32, Self::arc(""));
        self.fields(e, sc, |d, n, v| {
            match n {
                at::IX => ix = d.u32(&v)?,
                at::RGB => rgb = Self::arc(&v),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.record_unknowns(e, sc, "color", &ix.to_string())?;
        Ok(ColorEntry { ix, rgb })
    }

    fn font(&mut self, e: &El, sc: &Scope<Ours>) -> Result<FaceName, CodecError> {
        let mut name_u = Self::arc("");
        self.fields(e, sc, |_, n, v| {
            if n == at::NAME_U {
                name_u = Self::arc(&v);
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut native = self.start_native(sc)?;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            if local == el::EXT {
                d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?;
                return Ok(true);
            }
            if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) }
        })?;
        set_native(&mut ext, native);
        Ok(FaceName { name_u, raw: rest.raw, ext })
    }

    /// A theme element: its part name and bytes. `None` when the bytes are lost, with a note.
    fn theme(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Theme>, CodecError> {
        let (mut part, mut blob) = (Self::arc(""), None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::PART => part = Self::arc(&v),
                at::BLOB => blob = Some(d.u32(&v)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.children(e, sc, None, |_, _, _, _| Ok(false))?;
        let Some(blob) = blob else { return Err(self.bad("a theme with no blob")) };
        match self.blob(blob)? {
            Some(bytes) => Ok(Some(Theme { part, bytes, ..Theme::default() })),
            None => {
                self.budget.charge_items(1)?;
                self.notes.push(DecodeNote::ThemeLost { part: part.to_string() });
                Ok(None)
            }
        }
    }

    /// The `el` element a `solution` holds, as a kept element.
    fn solution(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<RawElement>, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut found = None;
        self.children(e, sc, None, |d, child, local, csc| {
            if local == el::KEPT_EL && found.is_none() {
                found = Some(d.raw_element(child, csc)?);
                return Ok(true);
            }
            Ok(false)
        })?;
        Ok(found)
    }

    pub(crate) fn style(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Style, CodecError> {
        let outer = std::mem::take(&mut self.keyed);
        let (mut id, mut name, mut name_u) = (0u32, None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::ID => id = d.u32(&v)?,
                at::NAME => name = Some(Self::arc(&v)),
                at::NAME_U => name_u = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut native = self.start_native(sc)?;
        let mut sheet: Option<Arc<Sheet>> = None;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::SHEET | el::SAME => {
                    // A place holds one sheet: the first, whether it is a `sheet` or a `same` that stands for one; a second is not read and is counted.
                    if sheet.is_some() {
                        d.unknown_el(child)?;
                    } else if local == el::SHEET {
                        sheet = d.shared::<Sheet>(child, csc)?;
                    } else {
                        sheet = d.same::<Sheet>(child, csc)?;
                    }
                }
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, odf| d.odf_style(o, osc, odf))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        // What a newer build wrote on the style's held values, kept under their keys (the style's own, as a shape's).
        for record in std::mem::replace(&mut self.keyed, outer).entries {
            self.push_kept(&mut native.kept, 0, RawNode::Element(record))?;
        }
        set_native(&mut ext, native);
        Ok(Style { id, name, name_u, sheet: sheet.unwrap_or_else(|| self.share.default_sheet()), raw: rest.raw, passthrough: rest.kept, ext })
    }

    // ------------------------------------------------------------------------------------------------------------ pages and masters

    pub(crate) fn page(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Page, CodecError> {
        let mut kids = e.elements();
        self.page_with(e, sc, &mut |_| Ok(kids.next().map(Cow::Borrowed)))
    }

    /// A page whose root is `head` (its attributes; the children it holds, if any, are not read from it) and whose children are what `next` gives, one at a time, until
    /// it gives none: the elements of a tree, or the children a stream of the part reads as they come ([`PartSource::page_bytes`]), each let go before the next is asked for.
    /// `next` is given the decode, so that a stream can hold back the room of the tree of a child that is more than the tree pool ([`Budget::reserve_child`]) for as long as
    /// the child is read. A field held in an `s` child is read where the child stands.
    pub(super) fn page_with<'k>(&mut self, head: &El, sc: &Scope<Ours>, next: &mut NextChild<'k>) -> Result<Page, CodecError> {
        self.begin_part("page");
        let (mut id, mut name, mut name_u, mut background, mut back_page, mut rel_id, mut part_name) =
            (0u32, Self::arc(""), None, false, None, None, None);
        let mut apply = |d: &mut Dec<'_>, n: &str, v: Cow<'_, str>| -> Result<bool, CodecError> {
            match n {
                at::ID => id = d.u32(&v)?,
                at::NAME => name = Self::arc(&v),
                at::NAME_U => name_u = Some(Self::arc(&v)),
                at::BACKGROUND => background = d.flag(&v)?,
                at::BACK_PAGE => back_page = Some(d.u32(&v)?),
                at::REL_ID => rel_id = Some(Self::arc(&v)),
                at::PART_NAME => part_name = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        };
        self.fields_attrs(head, sc, &mut apply)?;
        let mut native = self.start_native(sc)?;
        let mut sheet: Option<Arc<Sheet>> = None;
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        let mut connects: Vec<Connect> = Vec::new();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        let mut each = |d: &mut Dec<'_>, child: &El, local: &str, csc: &Scope<Ours>| -> Result<bool, CodecError> {
            match local {
                el::SHEET | el::SAME => {
                    // A place holds one sheet: the first, whether it is a `sheet` or a `same` that stands for one; a second is not read and is counted.
                    if sheet.is_some() {
                        d.unknown_el(child)?;
                    } else if local == el::SHEET {
                        sheet = d.shared::<Sheet>(child, csc)?;
                    } else {
                        sheet = d.same::<Sheet>(child, csc)?;
                    }
                }
                el::SHAPE => shapes.push(Arc::new(d.shape(child, csc, 1)?)),
                el::CONNECT => connects.push(d.connect(child, csc)?),
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, odf| d.odf_page(o, osc, odf))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        };
        let mut state = ChildLoop::new();
        let mut long = |d: &mut Dec<'_>, child: &El, csc: &Scope<Ours>| d.long_field(child, csc, &mut apply);
        while let Some(child) = next(self)? {
            self.child_step(&mut state, &child, sc, Some(&mut native.kept), &mut each, Some(&mut long))?;
        }
        set_native(&mut ext, native);
        Ok(Page {
            id,
            name,
            name_u,
            background,
            back_page,
            rel_id,
            sheet: sheet.unwrap_or_else(|| self.share.default_sheet()),
            shapes: ShapeList::from_vec(shapes),
            connects: Connects::from_vec(connects),
            raw: rest.raw,
            passthrough: rest.kept,
            ext,
            part_name,
        })
    }

    pub(crate) fn master(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Master, CodecError> {
        let mut kids = e.elements();
        self.master_with(e, sc, &mut |_| Ok(kids.next().map(Cow::Borrowed)))
    }

    /// A master whose root is `head` and whose children are what `next` gives ([`Dec::page_with`]).
    pub(super) fn master_with<'k>(&mut self, head: &El, sc: &Scope<Ours>, next: &mut NextChild<'k>) -> Result<Master, CodecError> {
        self.begin_part("master");
        let (mut id, mut name, mut name_u, mut unique_id, mut pattern_flags, mut rel_id, mut icon, mut part_name) =
            (0u32, None, None, None, None, None, None, None);
        let mut apply = |d: &mut Dec<'_>, n: &str, v: Cow<'_, str>| -> Result<bool, CodecError> {
            match n {
                at::ID => id = d.u32(&v)?,
                at::NAME => name = Some(Self::arc(&v)),
                at::NAME_U => name_u = Some(Self::arc(&v)),
                at::UNIQUE_ID => unique_id = Some(Self::arc(&v)),
                at::PATTERN_FLAGS => pattern_flags = Some(d.u32(&v)?),
                at::REL_ID => rel_id = Some(Self::arc(&v)),
                at::ICON => icon = Some(Self::arc(&v)),
                at::PART_NAME => part_name = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        };
        self.fields_attrs(head, sc, &mut apply)?;
        let mut native = self.start_native(sc)?;
        let mut sheet: Option<Arc<Sheet>> = None;
        let mut shapes: Vec<Arc<Shape>> = Vec::new();
        let mut connects: Vec<Connect> = Vec::new();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        let mut each = |d: &mut Dec<'_>, child: &El, local: &str, csc: &Scope<Ours>| -> Result<bool, CodecError> {
            match local {
                el::SHEET | el::SAME => {
                    // A place holds one sheet: the first, whether it is a `sheet` or a `same` that stands for one; a second is not read and is counted.
                    if sheet.is_some() {
                        d.unknown_el(child)?;
                    } else if local == el::SHEET {
                        sheet = d.shared::<Sheet>(child, csc)?;
                    } else {
                        sheet = d.same::<Sheet>(child, csc)?;
                    }
                }
                el::SHAPE => shapes.push(Arc::new(d.shape(child, csc, 1)?)),
                el::CONNECT => connects.push(d.connect(child, csc)?),
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        };
        let mut state = ChildLoop::new();
        let mut long = |d: &mut Dec<'_>, child: &El, csc: &Scope<Ours>| d.long_field(child, csc, &mut apply);
        while let Some(child) = next(self)? {
            self.child_step(&mut state, &child, sc, Some(&mut native.kept), &mut each, Some(&mut long))?;
        }
        set_native(&mut ext, native);
        Ok(Master {
            id,
            name,
            name_u,
            unique_id,
            pattern_flags,
            rel_id,
            icon,
            sheet: sheet.unwrap_or_else(|| self.share.default_sheet()),
            shapes: ShapeList::from_vec(shapes),
            connects: Connects::from_vec(connects),
            raw: rest.raw,
            passthrough: rest.kept,
            ext,
            part_name,
        })
    }

    /// A page part's tree, read: its root is the page element, the page is decoded, and its `ID` is the one the document part's list gives.
    pub(super) fn read_page(&mut self, tree: &El, id: u32) -> Result<Page, CodecError> {
        let sc = part_scope(self, tree, el::PAGE, "page")?;
        let page = self.page(tree, &sc)?;
        if page.id != id {
            return Err(CodecError::malformed("page", "a page part whose ID is not the one the list gives"));
        }
        Ok(page)
    }

    /// A page part's bytes, read **as a stream of the children of its root**: the root's start tag and then each child, whole, one at a time, so that the tree held is one
    /// child's and not the part's ([`stream_pull`]). What is read is what [`Dec::read_page`] reads of the tree of the same bytes, charged the same.
    pub(super) fn read_page_bytes(&mut self, bytes: &[u8], id: u32) -> Result<Page, CodecError> {
        let text = stream_text(bytes, "page")?;
        let (mut pull, head) = stream_pull(&text, "page")?;
        let sc = part_scope(self, &head, el::PAGE, "page")?;
        let mut held = 0u64;
        let page = self.page_with(&head, &sc, &mut |d| stream_child(d, &mut pull, &mut held, "page"))?;
        pull.finish().map_err(|e| xml_error("page", &e))?;
        if page.id != id {
            return Err(CodecError::malformed("page", "a page part whose ID is not the one the list gives"));
        }
        Ok(page)
    }

    /// A master part's tree, read, as [`Dec::read_page`] reads a page's.
    pub(super) fn read_master(&mut self, tree: &El, id: u32) -> Result<Master, CodecError> {
        let sc = part_scope(self, tree, el::MASTER, "master")?;
        let master = self.master(tree, &sc)?;
        if master.id != id {
            return Err(CodecError::malformed("master", "a master part whose ID is not the one the list gives"));
        }
        Ok(master)
    }

    /// A master part's bytes, read as a stream ([`Dec::read_page_bytes`]).
    pub(super) fn read_master_bytes(&mut self, bytes: &[u8], id: u32) -> Result<Master, CodecError> {
        let text = stream_text(bytes, "master")?;
        let (mut pull, head) = stream_pull(&text, "master")?;
        let sc = part_scope(self, &head, el::MASTER, "master")?;
        let mut held = 0u64;
        let master = self.master_with(&head, &sc, &mut |d| stream_child(d, &mut pull, &mut held, "master"))?;
        pull.finish().map_err(|e| xml_error("master", &e))?;
        if master.id != id {
            return Err(CodecError::malformed("master", "a master part whose ID is not the one the list gives"));
        }
        Ok(master)
    }

    fn connect(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Connect, CodecError> {
        let (mut from_sheet, mut from_cell, mut from_part, mut to_sheet, mut to_cell, mut to_part) = (0u32, None, None, 0u32, None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::FROM_SHEET => from_sheet = d.u32(&v)?,
                at::FROM_CELL => from_cell = Some(Self::arc(&v)),
                at::FROM_PART => from_part = Some(d.u32(&v)?),
                at::TO_SHEET => to_sheet = d.u32(&v)?,
                at::TO_CELL => to_cell = Some(Self::arc(&v)),
                at::TO_PART => to_part = Some(d.u32(&v)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut native = self.start_native(sc)?;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            if local == el::EXT {
                d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?;
                return Ok(true);
            }
            if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) }
        })?;
        set_native(&mut ext, native);
        Ok(Connect { from_sheet, from_cell, from_part, to_sheet, to_cell, to_part, raw: rest.raw, ext })
    }

    // ------------------------------------------------------------------------------------------------------------ shapes

    /// A shape and what is under it. `depth` is 1 for a shape of a page or master. The depth and the part's shape count are checked before anything is
    /// built or charged for the shape; the recursion is one small frame for each level, and at most 200.
    pub(crate) fn shape(&mut self, e: &El, sc: &Scope<Ours>, depth: usize) -> Result<Shape, CodecError> {
        if depth > MAX_SHAPE_DEPTH {
            return Err(limit("levels of nested shapes", MAX_SHAPE_DEPTH));
        }
        self.shapes = self.shapes.saturating_add(1);
        if self.shapes > MAX_SHAPES_PER_PAGE {
            return Err(limit("shapes in a part", MAX_SHAPES_PER_PAGE));
        }
        let outer = std::mem::take(&mut self.keyed);
        let mut f = ShapeFields::default();
        self.fields(e, sc, |d, n, v| f.apply(d, n, &v))?;
        let mut native = self.start_native(sc)?;
        let mut sheet: Option<Arc<Sheet>> = None;
        let mut text = None;
        let mut foreign = None;
        let mut children: Vec<Arc<Shape>> = Vec::new();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::SHEET | el::SAME => {
                    // A place holds one sheet: the first, whether it is a `sheet` or a `same` that stands for one; a second is not read and is counted.
                    if sheet.is_some() {
                        d.unknown_el(child)?;
                    } else if local == el::SHEET {
                        sheet = d.shared::<Sheet>(child, csc)?;
                    } else {
                        sheet = d.same::<Sheet>(child, csc)?;
                    }
                }
                el::TEXT => text = Some(Arc::new(d.text(child, csc)?)),
                el::FOREIGN => foreign = Some(Arc::new(d.foreign(child, csc)?)),
                el::SHAPE => children.push(Arc::new(d.shape(child, csc, depth.saturating_add(1))?)),
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, odf| d.odf_shape(o, osc, odf))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        for record in std::mem::replace(&mut self.keyed, outer).entries {
            self.push_kept(&mut native.kept, 0, RawNode::Element(record))?;
        }
        set_native(&mut ext, native);
        Ok(Shape {
            id: f.id,
            name: f.name,
            name_u: f.name_u,
            kind: f.kind,
            master: f.master,
            master_shape: f.master_shape,
            del: f.del,
            sheet: sheet.unwrap_or_else(|| self.share.default_sheet()),
            text,
            data1: f.data1,
            data2: f.data2,
            data3: f.data3,
            foreign,
            children: ShapeList::from_vec(children),
            raw: rest.raw,
            passthrough: rest.kept,
            ext,
        })
    }

    fn text(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Text, CodecError> {
        let outer = std::mem::take(&mut self.keyed);
        self.fields(e, sc, |_, _, _| Ok(false))?;
        // The attributes of the text element that no build here knows are the slot's native attributes; the unknown elements among its children are parts
        // (`NativeRaw`), and those inside its `ext` element are the slot's native nodes.
        let mut native = self.start_native(sc)?;
        let mut ext = Ext::default();
        let mut ext_seen = false;
        let mut parts: Vec<TextPart> = Vec::new();
        for node in &e.children {
            match node {
                Node::Text(t) | Node::CData(t) => {
                    self.budget.charge_items(1)?;
                    self.budget.charge_bytes(TEXT_PART_BYTES.saturating_add(text_bytes(t.len())))?;
                    parts.push(TextPart::Chars(Self::arc(t)));
                }
                // The codec writes neither; one a tool added is layout.
                Node::Comment(_) | Node::Pi { .. } => {}
                Node::El(child) => {
                    let csc = self.enter(sc, child)?;
                    if !csc.is_ours(child) {
                        parts.push(TextPart::NativeRaw(Box::new(RawNode::Element(self.unknown_element(child, sc)?))));
                        continue;
                    }
                    match child.local() {
                        l if l == el::CHARS => parts.push(TextPart::Chars(Self::arc(&self.text_node(child, &csc)?))),
                        l if l == el::CP => parts.push(TextPart::Cp(self.marker(child, &csc)?)),
                        l if l == el::PP => parts.push(TextPart::Pp(self.marker(child, &csc)?)),
                        l if l == el::TP => parts.push(TextPart::Tp(self.marker(child, &csc)?)),
                        l if l == el::FLD => {
                            let mut ix = 0u32;
                            let text = self.text_node_with(child, &csc, |d, n, v| {
                                if n == at::IX {
                                    ix = d.u32(&v)?;
                                    return Ok(true);
                                }
                                Ok(false)
                            })?;
                            parts.push(TextPart::Fld { ix, text: Self::arc(&text) });
                        }
                        l if l == el::KEPT => {
                            if let (_, Some(node)) = self.kept_node_of(child, &csc)? {
                                parts.push(TextPart::Raw(node));
                            }
                        }
                        // A newer build's text, comment or instruction, in data form.
                        l if l == el::KEPT_TEXT || l == el::KEPT_COMMENT || l == el::KEPT_PI => {
                            if let Some(node) = self.node_of(child, &csc)? {
                                parts.push(TextPart::NativeRaw(Box::new(node)));
                            }
                        }
                        l if l == el::ODF_RAW => {
                            if let Some(node) = self.wrapped_node(child, &csc)? {
                                parts.push(TextPart::OdfRaw(Box::new(node)));
                            }
                        }
                        // The first `ext` is the text's slot; another is not read.
                        l if l == el::EXT && ext_seen => self.unknown_el(child)?,
                        l if l == el::EXT => {
                            ext_seen = true;
                            self.ext_child_native(child, &csc, &mut ext, &mut native.kept, |d, o, osc, odf| d.odf_text(o, osc, odf))?;
                        }
                        // A long field of a text is read with its fields, which a text has none of.
                        l if l == el::LONG => {}
                        // An element of the codec's own namespace that this build does not know: what a newer build wrote inside the text.
                        _ => parts.push(TextPart::NativeRaw(Box::new(RawNode::Element(self.unknown_element(child, sc)?)))),
                    }
                }
            }
        }
        for record in std::mem::replace(&mut self.keyed, outer).entries {
            self.push_kept(&mut native.kept, 0, RawNode::Element(record))?;
        }
        set_native(&mut ext, native);
        Ok(Text { parts, ext })
    }

    /// The node an `odf-raw` wrapper holds: its one `el`, `txt`, `comment` or `pi` child.
    fn wrapped_node(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<RawNode>, CodecError> {
        self.fields(e, sc, |_, _, _| Ok(false))?;
        let mut found = None;
        self.children(e, sc, None, |d, child, _, csc| {
            if found.is_none()
                && let Some(n) = d.node_of(child, csc)?
            {
                found = Some(n);
                return Ok(true);
            }
            Ok(false)
        })?;
        Ok(found)
    }

    fn marker(&mut self, e: &El, sc: &Scope<Ours>) -> Result<u32, CodecError> {
        let mut ix = 0u32;
        self.fields(e, sc, |d, n, v| {
            if n == at::IX {
                ix = d.u32(&v)?;
                return Ok(true);
            }
            Ok(false)
        })?;
        self.children(e, sc, None, |_, _, _, _| Ok(false))?;
        Ok(ix)
    }

    fn foreign(&mut self, e: &El, sc: &Scope<Ours>) -> Result<ForeignData, CodecError> {
        let (mut rel_id, mut text) = (None, None);
        self.fields(e, sc, |_, n, v| {
            match n {
                at::REL_ID => rel_id = Some(Self::arc(&v)),
                at::TEXT => text = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut native = self.start_native(sc)?;
        let mut picture = None;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::PICTURE => picture = d.picture(child, csc)?,
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        set_native(&mut ext, native);
        Ok(ForeignData { attrs: rest.raw, rel_id, picture, text, passthrough: rest.kept, ext })
    }

    fn picture(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Arc<Media>>, CodecError> {
        let (mut part, mut content_type, mut blob) = (String::new(), String::new(), None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::PART => part = v.into_owned(),
                at::TYPE => content_type = v.into_owned(),
                at::BLOB => blob = Some(d.u32(&v)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.children(e, sc, None, |_, _, _, _| Ok(false))?;
        let Some(blob) = blob else { return Err(self.bad("a picture with no blob")) };
        let Some(bytes) = self.blob(blob)? else {
            self.lost_picture(blob)?;
            return Ok(None);
        };
        if let Some(shared) = self.media.get(&blob).and_then(|list| list.iter().find(|m| *m.part == *part && *m.content_type == *content_type)) {
            return Ok(Some(Arc::clone(shared)));
        }
        let media = Arc::new(Media { part: Self::arc(&part), content_type: Self::arc(&content_type), bytes });
        let list = self.media.entry(blob).or_default();
        if list.len() < MEDIA_SHARED_PER_BLOB {
            list.push(Arc::clone(&media));
        }
        Ok(Some(media))
    }

    // ------------------------------------------------------------------------------------------------------------ sheets

    pub(crate) fn sheet(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Sheet, CodecError> {
        let mut links = Links::default();
        self.fields(e, sc, |d, n, v| {
            match n {
                at::LINE_STYLE => links.line = Some(d.u32(&v)?),
                at::FILL_STYLE => links.fill = Some(d.u32(&v)?),
                at::TEXT_STYLE => links.text = Some(d.u32(&v)?),
                at::DEF => d.named(v.into_owned()),
                at::FP => {}
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let mut native = self.start_native(sc)?;
        let mut cells = CellList::default();
        let mut sections: Vec<Arc<Section>> = Vec::new();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::CELL => d.put_cell(&mut cells, child, csc)?,
                el::SECTION => {
                    if sections.len() >= MAX_SECTIONS_PER_SHEET {
                        return Err(limit("sections in a sheet", MAX_SECTIONS_PER_SHEET));
                    }
                    sections.extend(d.shared::<Section>(child, csc)?);
                }
                el::SAME => {
                    if sections.len() >= MAX_SECTIONS_PER_SHEET {
                        return Err(limit("sections in a sheet", MAX_SECTIONS_PER_SHEET));
                    }
                    sections.extend(d.same::<Section>(child, csc)?);
                }
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        set_native(&mut ext, native);
        let cells = self.build_cells(cells)?;
        Ok(Sheet { links, cells, sections, raw: rest.raw, passthrough: rest.kept, ext })
    }

    fn section(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Section, CodecError> {
        let (mut name, mut ix, mut del) = (String::new(), None, false);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::N => name = v.into_owned(),
                at::SECTION_IX => ix = Some(d.u32(&v)?),
                at::SECTION_DEL => del = d.flag(&v)?,
                at::DEF => d.named(v.into_owned()),
                at::FP => {}
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        self.check_name(&name)?;
        let mut native = self.start_native(sc)?;
        let mut cells = CellList::default();
        let mut rows: Vec<Arc<Row>> = Vec::new();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::CELL => d.put_cell(&mut cells, child, csc)?,
                el::ROW => {
                    if rows.len() >= MAX_ROWS_PER_SECTION {
                        return Err(limit("rows in a section", MAX_ROWS_PER_SECTION));
                    }
                    rows.push(Arc::new(d.row(child, csc)?));
                }
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        set_native(&mut ext, native);
        let cells = self.build_cells(cells)?;
        Ok(Section { name: SectionName::new(&name), ix, del, cells, rows: Rows::from(rows), raw: rest.raw, passthrough: rest.kept, ext })
    }

    fn row(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Row, CodecError> {
        let outer = std::mem::take(&mut self.keyed);
        let (mut name, mut ix, mut local_name, mut row_type, mut del) = (None, None, None, None, false);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::N => name = Some(Self::arc(&v)),
                at::SECTION_IX => ix = Some(d.u32(&v)?),
                at::LOCAL_NAME => local_name = Some(Self::arc(&v)),
                at::ROW_T => row_type = Some(Self::arc(&v)),
                at::SECTION_DEL => del = d.flag(&v)?,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        if let Some(name) = &name {
            self.check_name(name)?;
        }
        let mut native = self.start_native(sc)?;
        let mut cells = CellList::default();
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            match local {
                el::CELL => d.put_cell(&mut cells, child, csc)?,
                el::EXT => d.ext_child(child, csc, &mut ext, |d, o, osc, odf| d.odf_row(o, osc, odf))?,
                _ => return d.rest_child(&mut rest, child, csc),
            }
            Ok(true)
        })?;
        for record in std::mem::replace(&mut self.keyed, outer).entries {
            self.push_kept(&mut native.kept, 0, RawNode::Element(record))?;
        }
        set_native(&mut ext, native);
        let cells = self.build_cells(cells)?;
        Ok(Row { name, ix, local_name, row_type, del, cells, raw: rest.raw, passthrough: rest.kept, ext })
    }

    /// A unit (a sheet or a section) as a shared copy ([`super::share`]): a unit that is the same as one read before (its key is known **and its stream is the
    /// same**, byte for byte: the key only finds the candidate) is not decoded again, and costs what it cost the items (the comparison is as long as the unit and
    /// that is what the items stand for), a reference in bytes, and its unknown content; the first of a stream is decoded as it always was and kept for the next, at the
    /// price of its entry and its stream.
    pub(super) fn shared<T: Unit + DecodeUnit>(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Arc<T>>, CodecError> {
        // What the element before this one left unread is its own to count, and not the unit's: the count a unit makes is the unit's alone, whether it is decoded or
        // is a reference to one that was.
        self.flush_pending();
        // A unit that names itself (`def`) is read in full and is no unit that another is compared with: what the part says of it is its name, and a name is given
        // once. What the unit that holds it names is its own, so the name an outer unit set is put aside while this one is read.
        let outer_def = self.pending_def.take();
        let (items, unknown, names) = (self.budget.items, self.unknown, self.defs.mark());
        let made = if self.share.mode == Sharing::Off { Arc::new(T::decode(self, e, sc)?) } else { self.shared_by_stream::<T>(e, sc)? };
        let mine = std::mem::replace(&mut self.pending_def, outer_def);
        let Some(name) = mine else { return Ok(Some(made)) };
        let (spent, lost) = (items.saturating_sub(self.budget.items), self.unknown.saturating_sub(unknown));
        match self.define::<T>(name, e, &made, spent, lost)? {
            Named::Yes => Ok(Some(made)),
            // A name that no `same` can take (a unit that was edited by a program that did not know it, a name that is empty, or a name that another unit has): the unit is
            // what it says and is read, the names that were given inside it are as stale as it is, and the name that is not given is counted.
            Named::Unnamed => {
                self.defs.rollback(names);
                self.saw_unknown();
                Ok(Some(made))
            }
        }
    }

    /// The unit in hand gives itself a name. **The name is not an item**: a unit costs the same whether it names itself or not, so that a part that says a unit once
    /// is charged what the part that says it in full is (the `same` that stands for it is charged the unit, and the element of the `same` is where it stands).
    fn named(&mut self, name: String) {
        self.pending_def = Some(name);
        self.budget.items = self.budget.items.saturating_add(1);
    }

    /// Gives the unit read (`e`, which made `made`) the `name` that a `same` of the part can stand for it by, and what reading it cost the items and counted as lost.
    /// **A name that cannot be given is not damage** (the format's 3.19): a name that is empty, or whose unit no longer matches the fingerprint it was written with
    /// (the unit was edited by a program that kept the `same` elements that stand for it), gives no name; a name that another unit has is given to none.
    fn define<T: Unit>(&mut self, name: String, e: &El, made: &Arc<T>, items: u64, unknown: u64) -> Result<Named, CodecError> {
        if name.is_empty() || self.defs.get(&name).is_some() {
            return Ok(Named::Unnamed);
        }
        // The fingerprint of the unit as it reads, from the tree that was read and the units of the part that it names, and the one it was written with.
        let defs = &self.defs;
        let fp = fingerprint(e, &|n| defs.fingerprint_of(n));
        if e.attr(at::FP).is_some_and(|written| written != fp.text()) {
            return Ok(Named::Unnamed);
        }
        self.budget.charge_bytes(DEF_ENTRY_BYTES.saturating_add(text_bytes(name.len())))?;
        let value = (self.share.mode != Sharing::Count).then(|| Arc::clone(made));
        self.defs.insert(name, T::named(Def { value, items, unknown, fp }));
        Ok(Named::Yes)
    }

    /// A `same` element: the unit of the kind this place holds that an earlier element of the part named. It costs the element, a reference, and the name's bytes,
    /// and what it holds (it holds nothing) is counted as lost.
    pub(super) fn same<T: Unit>(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Option<Arc<T>>, CodecError> {
        let before = self.budget.items;
        let mut of: Option<String> = None;
        self.fields(e, sc, |_, n, v| {
            if n == at::OF {
                of = Some(v.into_owned());
                Ok(true)
            } else {
                Ok(false)
            }
        })?;
        self.children(e, sc, None, |_, _, _, _| Ok(false))?;
        // **A `same` that names no unit is not damage** (3.19): no earlier element of the part named a unit of the kind this place holds by that name (a program that
        // does not know 3.19 moved the unit, deleted the shape that held it, or edited it), and it is not read and is counted.
        // A `same` with no `of` is no `same`: it is damage, as any element without what it must have.
        let Some(of) = of else { return Err(self.bad("a same that names nothing")) };
        let found = Some(of.as_str()).and_then(|name| self.defs.get(name)).and_then(T::of_kind).map(|d| (d.items, d.unknown, d.value.clone()));
        let Some((items, unknown, copy)) = found else {
            self.saw_unknown();
            return Ok(None);
        };
        self.budget.charge_bytes(SHARED_REF_BYTES)?;
        // What a copy of the unit would have cost the items and counted as lost, as a unit that an equal one is the same as is charged (the element of the `same` is
        // part of it: the unit stands where the element stands, so what the element was charged is not charged twice).
        let own = before.saturating_sub(self.budget.items);
        self.budget.charge_items(items.saturating_sub(own))?;
        self.unknown = self.unknown.saturating_add(unknown);
        Ok(Some(copy.unwrap_or_else(|| T::placeholder(&self.share))))
    }

    /// What the decode has made of the things a unit's reading can leave beside the unit: the notes it has, the bytes of blobs it has been charged and the records of
    /// unknown content it has kept for the owner above. A unit that leaves any of them different is not a function of its text alone.
    fn footprint(&self) -> Footprint {
        Footprint { notes: self.notes.len(), blobs: self.budget.blobs_charged(), keyed: self.keyed.mark() }
    }

    /// A unit read as a copy that an equal one read before shares ([`super::share`]).
    fn shared_by_stream<T: Unit + DecodeUnit>(&mut self, e: &El, sc: &Scope<Ours>) -> Result<Arc<T>, CodecError> {
        let (key, len, relative) = self.share.key_len(e, sc);
        // A unit that gives a name, or that names one, is read where it stands and is no unit that another is the same as.
        if relative {
            return Ok(Arc::new(T::decode(self, e, sc)?));
        }
        if let Some(hit) = self.share.find::<T>(key, e, sc) {
            let (items, unknown, copy) = (hit.items, hit.unknown, hit.value.clone());
            self.budget.charge_items(items)?;
            self.budget.charge_bytes(SHARED_REF_BYTES)?;
            self.unknown = self.unknown.saturating_add(unknown);
            return Ok(copy.unwrap_or_else(|| T::placeholder(&self.share)));
        }
        let (items, unknown, before) = (self.budget.items, self.unknown, self.footprint());
        let made = Arc::new(T::decode(self, e, sc)?);
        // What a notes, a blob or the records of an owner above make of a unit is not a function of its text alone: such a unit is never shared.
        if self.footprint() == before && self.share.has_room::<T>(key, len) {
            let spent_items = items.saturating_sub(self.budget.items);
            // The entry, its stream, and the room of a chunk of streams the stream has to make: all of it before any of it is made.
            self.budget.charge_bytes(SHARE_ENTRY_BYTES.saturating_add(len).saturating_add(self.share.chunk_slack::<T>(len)))?;
            let value = (self.share.mode == Sharing::Keep).then(|| Arc::clone(&made));
            let unit = Made { items: spent_items, unknown: self.unknown.saturating_sub(unknown), value };
            self.share.enter_walked::<T>(key, len, (e, sc), unit);
        }
        Ok(made)
    }

    fn check_name(&self, name: &str) -> Result<(), CodecError> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err(self.bad("a cell, section or row name that is empty or too long"));
        }
        Ok(())
    }

    /// Reads the `Cell` element `e` into `cells`: at the cap of cells in one place the document is refused. The map the cells go into is built when the place
    /// is done ([`Dec::build_cells`]), from the cells sorted, and what each cell adds to it is charged here: the first cell of a place makes a leaf of eleven
    /// slots, and so does every twelfth after it.
    fn put_cell(&mut self, cells: &mut CellList, e: &El, sc: &Scope<Ours>) -> Result<(), CodecError> {
        let have = cells.0.len();
        if have >= MAX_CELLS_PER_CONTAINER {
            return Err(limit("cells in one place", MAX_CELLS_PER_CONTAINER));
        }
        self.budget.charge_bytes(cells_bytes(have.saturating_add(1)).saturating_sub(cells_bytes(have)))?;
        let (name, cell) = self.cell(e, sc)?;
        self.check_name(&name)?;
        cells.0.push((CellName::new(&name), cell));
        Ok(())
    }

    /// The map of the cells of one place, built from the cells in one go (`BTreeMap::from_iter` sorts them and fills every node, where one insert at a time
    /// in the order of the file leaves leaves half empty). A name twice in one place refuses the document.
    fn build_cells(&self, list: CellList) -> Result<diacraft_doc::Cells, CodecError> {
        let n = list.0.len();
        let cells: diacraft_doc::Cells = list.0.into_iter().collect();
        if cells.len() != n {
            return Err(self.bad("two cells of one name in one place"));
        }
        Ok(cells)
    }

    /// A `Cell` element. What a newer build wrote on it is kept in the native slot of the cell's extra (a cell has no slot of its own, so that it costs
    /// nothing): an extra is made for it when the cell has none.
    fn cell(&mut self, e: &El, sc: &Scope<Ours>) -> Result<(String, Cell), CodecError> {
        let mut name = None;
        let (mut value, mut formula, mut unit, mut error) = (None, FormulaSlot::Absent, None, None);
        self.fields(e, sc, |d, n, v| {
            match n {
                at::N => name = Some(v.into_owned()),
                at::V => value = Some(Value::from_text(&v)),
                at::F => formula = d.formula(&v, false)?,
                at::FL => formula = d.formula(&v, true)?,
                at::U => unit = Some(Self::arc(&v)),
                at::E => error = Some(Self::arc(&v)),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        let Some(name) = name else { return Err(self.bad("a Cell with no N")) };
        let mut native = self.start_native(sc)?;
        let mut extra: Option<CellExtra> = None;
        let mut rest = Rest::default();
        self.children(e, sc, Some(&mut native.kept), |d, child, local, csc| {
            if local == el::EXTRA {
                extra = Some(d.extra(child, csc)?);
                return Ok(true);
            }
            if local == el::ATTR { d.rest_child(&mut rest, child, csc) } else { Ok(false) }
        })?;
        if native != Kept::default() {
            set_native(&mut extra.get_or_insert_with(CellExtra::default).ext, native);
        }
        Ok((name, Cell { formula, value, unit, error, raw: rest.raw, extra: extra.map(Arc::new) }))
    }

    fn extra(&mut self, e: &El, sc: &Scope<Ours>) -> Result<CellExtra, CodecError> {
        let mut text = None;
        self.fields(e, sc, |_, n, v| {
            if n == at::TEXT {
                text = Some(Self::arc(&v));
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut ext = Ext::default();
        let mut rest = Rest::default();
        self.children(e, sc, None, |d, child, local, csc| match local {
            el::KEPT => d.rest_child(&mut rest, child, csc),
            el::EXT => {
                d.ext_child(child, csc, &mut ext, |d, o, osc, k| d.odf_kept(o, osc, k))?;
                Ok(true)
            }
            _ => Ok(false),
        })?;
        Ok(CellExtra { text, passthrough: rest.kept, ext })
    }

    /// A formula's text as a slot: the two markers, unless `literal`, else a parse that may use at most what the budget has left. A formula the budget
    /// cannot afford refuses the document: the codec gives the model back exactly or not at all.
    fn formula(&mut self, text: &str, literal: bool) -> Result<FormulaSlot, CodecError> {
        if !literal {
            if text == INHERIT_MARKER {
                return Ok(FormulaSlot::Inherit);
            }
            if text == NO_FORMULA_MARKER {
                return Ok(FormulaSlot::NoFormula);
            }
        }
        let afford = self.budget.room() / FORMULA_NODE_BYTES;
        let max_nodes = usize::try_from(afford).unwrap_or(usize::MAX);
        let formula = diacraft_formula::parse_with_budget(text, max_nodes);
        // A parse that ran out of nodes because the budget was small is the budget's refusal; one that ran out at the formula crate's own cap is the
        // formula's.
        if afford < FORMULA_MAX_NODES && matches!(&formula.ast, Err(e) if e.kind == diacraft_formula::ParseErrorKind::TooManyNodes) {
            return Err(self.budget.bytes_refusal());
        }
        let used = u64::try_from(formula.node_count()).unwrap_or(u64::MAX);
        // The nodes, and the strings the tree holds (its literals and names, copies of parts of the text the source holds besides), as text.
        self.budget.charge_bytes(used.saturating_mul(FORMULA_NODE_BYTES).saturating_add(text_bytes(tree_text_bytes(&formula))))?;
        Ok(FormulaSlot::Local(Arc::new(formula)))
    }

    /// What a record that has no slot of its own and no child of its own keeps that no field of it reads: its unknown attributes (from the last `fields`) and
    /// unknown child elements, as one kept element named by the record's `kind` and `key` (NP7), for the nearest owner to take into its native slot. A record
    /// with nothing unknown keeps nothing. `sc` is the scope inside the record.
    pub(super) fn record_unknowns(&mut self, e: &El, sc: &Scope<Ours>, kind: &str, key: &str) -> Result<(), CodecError> {
        let attrs = self.native_attrs(sc)?;
        let mut nodes = Passthrough::new();
        self.children(e, sc, Some(&mut nodes), |_, _, _, _| Ok(false))?;
        self.finish_record(attrs, nodes, kind, key);
        Ok(())
    }
}

/// What a shape's attributes hold, gathered as they are read.
struct ShapeFields {
    id: u32,
    name: Option<Arc<str>>,
    name_u: Option<Arc<str>>,
    kind: ShapeType,
    master: Option<u32>,
    master_shape: Option<u32>,
    del: bool,
    data1: Option<Arc<str>>,
    data2: Option<Arc<str>>,
    data3: Option<Arc<str>>,
}

impl Default for ShapeFields {
    fn default() -> ShapeFields {
        ShapeFields {
            id: 0,
            name: None,
            name_u: None,
            kind: ShapeType::Unspecified,
            master: None,
            master_shape: None,
            del: false,
            data1: None,
            data2: None,
            data3: None,
        }
    }
}

impl ShapeFields {
    fn apply(&mut self, d: &mut Dec<'_>, name: &str, value: &str) -> Result<bool, CodecError> {
        match name {
            at::ID => self.id = d.u32(value)?,
            at::NAME => self.name = Some(Arc::from(value)),
            at::NAME_U => self.name_u = Some(Arc::from(value)),
            at::TYPE => {
                self.kind = match value {
                    "Shape" => ShapeType::Shape,
                    "Group" => ShapeType::Group,
                    "Foreign" => ShapeType::Foreign,
                    "Guide" => ShapeType::Guide,
                    _ => return Err(d.bad("a shape type the vocabulary does not list")),
                }
            }
            at::TYPE_OTHER => self.kind = ShapeType::Other(Arc::from(value)),
            at::MASTER => self.master = Some(d.u32(value)?),
            at::MASTER_SHAPE => self.master_shape = Some(d.u32(value)?),
            at::DEL => self.del = d.flag(value)?,
            at::DATA1 => self.data1 = Some(Arc::from(value)),
            at::DATA2 => self.data2 = Some(Arc::from(value)),
            at::DATA3 => self.data3 = Some(Arc::from(value)),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------------------------------------------------- the public functions

/// The scope of a fragment's root: the root is the codec's own, under whatever prefix (or none) it was written, and declares what it declares.
fn fragment_scope(dec: &mut Dec<'_>, root: &El) -> Result<Scope<Ours>, CodecError> {
    let declaration = match root.name.split_once(':') {
        Some((prefix, _)) => format!("xmlns:{prefix}"),
        None => "xmlns".to_owned(),
    };
    let above = El { name: "x".to_owned(), attrs: vec![(declaration, URN.to_owned())], children: Vec::new() };
    dec.enter(&Scope::<Ours>::empty().enter(&above), root)
}

/// What the frame of `e` costs, when it declares a namespace or a prefix: the frame, each prefix, and the bytes of the names (kept in the frame and in the table
/// of open frames) and of the namespaces. `None` for an element that declares nothing, which makes no frame. An element that declares more prefixes than
/// [`MAX_PREFIXES_PER_ELEMENT`] is refused, as the count is made and before any table is.
fn frame_charge(e: &El) -> Result<Option<u64>, CodecError> {
    let (mut any, mut prefixes, mut bytes) = (false, 0usize, 0u64);
    for (k, v) in &e.attrs {
        if k == "xmlns" {
            any = true;
            bytes = bytes.saturating_add(u64::try_from(v.len()).unwrap_or(u64::MAX));
        } else if let Some(prefix) = k.strip_prefix("xmlns:") {
            any = true;
            prefixes = prefixes.saturating_add(1);
            if prefixes > MAX_PREFIXES_PER_ELEMENT {
                return Err(limit("namespace prefixes declared by one element", MAX_PREFIXES_PER_ELEMENT));
            }
            bytes = bytes.saturating_add(u64::try_from(prefix.len().saturating_mul(2).saturating_add(v.len())).unwrap_or(u64::MAX));
        }
    }
    Ok(any.then(|| frame_bytes(prefixes, bytes)))
}

/// The scope inside `e`, for a part read with no budget: the cap on what one element declares holds, and nothing is charged.
fn enter_checked(sc: &Scope<Ours>, e: &El) -> Result<Scope<Ours>, CodecError> {
    frame_charge(e)?;
    Ok(sc.enter(e))
}

/// The scope inside the root of a part, which declares the namespace itself, and whether the root is the element `local`.
fn part_scope(dec: &mut Dec<'_>, root: &El, local: &'static str, part: &'static str) -> Result<Scope<Ours>, CodecError> {
    let sc = dec.enter(&Scope::<Ours>::empty(), root)?;
    if !sc.is(root, local) {
        return Err(CodecError::malformed(part, "the root element is not the one this part holds"));
    }
    Ok(sc)
}

/// The limits a part is read at as a stream of the children of its root: **a child's own tree** is at most [`OPEN_ELEMENT_ITEMS`] items and as deep as the container
/// reads, and the whole part at most [`FORMAT_PART_ITEMS`] (the format's limit of a part, 6.1; the budget's items bound the drawing). The part's bytes are held beside the child that is made
/// of them, as they are beside a whole tree.
pub(super) fn stream_limits() -> PullLimits {
    let max_nodes = usize::try_from(OPEN_ELEMENT_ITEMS).unwrap_or(usize::MAX);
    let max_document_nodes = usize::try_from(FORMAT_PART_ITEMS).unwrap_or(usize::MAX);
    PullLimits { xml: XmlLimits { max_depth: XML_DEPTH_CEILING, max_nodes, ..XmlLimits::default() }, max_document_nodes }
}

/// The next child of a part read as a stream, whole, and its tree's room held back. The child before it is let go, and its room given back ([`Budget::release_tree`]);
/// the child may have what the pool of one tree holds and what the model's room holds beside it ([`Budget::child_items`]), which is told to the stream before the child is read
/// and so before its tree is made; and the items of the tree that was made beyond the tree pool are held back from the model's pool for as long as the child is held
/// ([`Budget::reserve_child`]), so that the model and the tree together never pass the pools.
fn stream_child<'k>(dec: &mut Dec<'_>, pull: &mut Pull<'_>, held: &mut u64, part: &str) -> Result<Option<Cow<'k, El>>, CodecError> {
    dec.budget.release_tree(std::mem::take(held));
    pull.set_child_items(usize::try_from(dec.budget.child_items()).unwrap_or(usize::MAX));
    let child = pull.next_child().map_err(|e| child_error(dec.budget, part, &e))?;
    if child.is_some() {
        *held = dec.budget.reserve_child(u64::try_from(pull.last_child_items()).unwrap_or(u64::MAX))?;
    }
    Ok(child.map(Cow::Owned))
}

/// What an XML error of a child of a part read as a stream is: past the child's own limit it is that, in the words of a child; past what the pool's room allowed it (the limit
/// the stream was given was less than a child may have) it is the pool's; else it is as [`xml_error`] says.
fn child_error(budget: &Budget, part: &str, e: &XmlError) -> CodecError {
    if let XmlError::TooManyNodes { limit } = e
        && let Ok(limit) = u64::try_from(*limit)
    {
        if limit < budget.child_items {
            return budget.bytes_refusal();
        }
        if limit == budget.child_items {
            return CodecError::Limit { what: "XML items in one element of a part", max: limit };
        }
    }
    xml_error(part, e)
}

/// What an XML error of a part read as a stream is, as the codec says it: a limit in words (its figure is the XML reader's), and a part that is not well-formed as
/// damage. A caller that wants the XML reader's own class words reads the part itself ([`PartSource::page`]).
pub(super) fn xml_error(part: &str, e: &XmlError) -> CodecError {
    match e {
        XmlError::TooManyNodes { limit } => {
            let (what, max) = if u64::try_from(*limit).is_ok_and(|l| l == OPEN_ELEMENT_ITEMS) {
                ("XML items in one element of a part", OPEN_ELEMENT_ITEMS)
            } else {
                ("XML items in one part", u64::try_from(*limit).unwrap_or(u64::MAX))
            };
            CodecError::Limit { what, max }
        }
        XmlError::TooDeep { limit } => {
            CodecError::Limit { what: "levels of nested elements in one part", max: u64::try_from(*limit).unwrap_or(u64::MAX) }
        }
        XmlError::AttributeTooLarge { limit } => {
            CodecError::Limit { what: "bytes in one XML attribute", max: u64::try_from(*limit).unwrap_or(u64::MAX) }
        }
        XmlError::EntityRefused { .. } => CodecError::malformed(part, "XML that holds a DTD or an entity, which is refused"),
        XmlError::Malformed { .. } => CodecError::malformed(part, "XML that is not well-formed"),
    }
}

/// The text of a part's bytes, for a stream: refused past what a part may be ([`OPEN_PART_BYTES`], and the copy that UTF-16 makes of its text beside the bytes, held
/// with them: twice that).
fn stream_text<'b>(bytes: &'b [u8], part: &str) -> Result<XmlText<'b>, CodecError> {
    let held = u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(parse_size(bytes).copy_bytes);
    if held > OPEN_PART_BYTES.saturating_mul(2) || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > OPEN_PART_BYTES {
        return Err(CodecError::Limit { what: "bytes in one part", max: OPEN_PART_BYTES });
    }
    XmlText::new(bytes).map_err(|e| xml_error(part, &e))
}

/// The root's start tag of a part and the stream of its children ([`stream_limits`]).
fn stream_pull<'t>(text: &'t XmlText<'_>, part: &str) -> Result<(Pull<'t>, El), CodecError> {
    text.pull(&stream_limits(), ParseOptions::default()).map_err(|e| xml_error(part, &e))
}

fn finish(mut dec: Dec<'_>) -> Result<(), CodecError> {
    dec.flush_pending();
    if dec.unknown > 0 {
        dec.budget.charge_items(1)?;
        dec.notes.push(DecodeNote::Unknown { count: dec.unknown });
    }
    Ok(())
}

/// What the document part lists: the part of each page and each master, and each blob. Read without a budget: the lists are capped (a page list at the
/// model's pages, a master list at its masters, the blobs at a package's entries), so what is made is bounded by the part's own cap.
pub fn part_lists(document: &El) -> Result<PartLists, CodecError> {
    let sc = enter_checked(&Scope::<Ours>::empty(), document)?;
    if !sc.is(document, el::DOCUMENT) {
        return Err(CodecError::malformed("document", "the root element is not the one this part holds"));
    }
    let mut pages: Vec<(u32, String)> = Vec::new();
    let mut masters: Vec<(u32, String)> = Vec::new();
    let mut blobs: Vec<BlobRecord> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    for child in document.elements() {
        let csc = enter_checked(&sc, child)?;
        if !csc.is_ours(child) {
            continue;
        }
        let local = child.local();
        if local != el::PAGE_REF && local != el::MASTER_REF && local != el::BLOB {
            continue;
        }
        let mut id = 0u32;
        let mut part = String::new();
        let (mut path, mut media_type, mut size, mut stamp) = (String::new(), String::new(), 0u64, None);
        let bad = |why: &'static str| CodecError::malformed("document", why);
        let mut fields = |k: &str, v: &str| -> Result<(), CodecError> {
            match k {
                at::ID => id = v.parse().map_err(|_| bad("a number that is not a number"))?,
                at::PART => part = v.to_owned(),
                at::PATH => path = v.to_owned(),
                at::MEDIA_TYPE => media_type = v.to_owned(),
                at::SIZE => size = v.parse().map_err(|_| bad("a number that is not a number"))?,
                at::STAMP => stamp = Some(Stamp::parse(v).ok_or_else(|| bad("a stamp that is not 16 lowercase hex digits"))?),
                _ => {}
            }
            Ok(())
        };
        for (k, v) in &child.attrs {
            if k == "xmlns" || k.starts_with("xmlns:") {
                continue;
            }
            match k.strip_suffix(at::B64_SUFFIX) {
                Some(base) => fields(base, &base64_string(v).ok_or_else(|| bad("base64 text that is not base64 of text"))?)?,
                None => fields(k, v)?,
            }
        }
        for long in child.elements() {
            let lsc = enter_checked(&csc, long)?;
            if !long.is(el::LONG) || !lsc.is_ours(long) {
                continue;
            }
            let name = long.attr(at::N_LONG).unwrap_or("");
            let b64 = long.attr(at::B64).is_some_and(|f| f == "1" || f == "true");
            let text: String =
                long.children.iter().filter_map(|n| if let Node::Text(t) | Node::CData(t) = n { Some(t.as_str()) } else { None }).collect();
            let text = if b64 { base64_string(&text).ok_or_else(|| bad("base64 text that is not base64 of text"))? } else { text };
            fields(name, &text)?;
        }
        match local {
            l if l == el::PAGE_REF => {
                if pages.len() >= MAX_PAGES {
                    return Err(limit("pages", MAX_PAGES));
                }
                pages.push((id, part));
            }
            l if l == el::MASTER_REF => {
                if masters.len() >= MAX_MASTERS {
                    return Err(limit("masters", MAX_MASTERS));
                }
                masters.push((id, part));
            }
            _ => {
                if blobs.len() >= diacraft_container::limits::MAX_PARTS {
                    return Err(limit("blobs", diacraft_container::limits::MAX_PARTS));
                }
                if !seen.insert(id) {
                    return Err(bad("a blob number used twice"));
                }
                let Some(stamp) = stamp else { return Err(bad("a blob with no stamp")) };
                blobs.push(BlobRecord { id: BlobId(id), path, media_type, size, stamp });
            }
        }
    }
    blobs.sort_by_key(|r| r.id);
    Ok(PartLists { pages, masters, blobs })
}

/// The parts of a document that a caller already holds, as references: what [`decode`] and [`check_decodes`] give [`decode_from`].
struct Slices<'a> {
    pages: &'a [&'a El],
    masters: &'a [&'a El],
}

impl<'a> PartSource for Slices<'a> {
    type Tree = &'a El;

    fn page(&mut self, index: usize, _name: &str) -> Result<&'a El, CodecError> {
        self.pages.get(index).copied().ok_or_else(|| CodecError::malformed("document", "the lists name other parts than were given"))
    }

    fn master(&mut self, index: usize, _name: &str) -> Result<&'a El, CodecError> {
        self.masters.get(index).copied().ok_or_else(|| CodecError::malformed("document", "the lists name other parts than were given"))
    }

    fn counts(&self) -> Option<(usize, usize)> {
        Some((self.pages.len(), self.masters.len()))
    }
}

/// A document from its parts: the document part, the pages' parts and the masters' parts, in the order `part_lists` names them, with the blobs from `blobs`.
/// A lost blob is a note and not a refusal; anything else the vocabulary does not allow, and anything past `budget`, is an error and builds nothing that
/// is kept.
pub fn decode(
    document: &El,
    pages: &[&El],
    masters: &[&El],
    blobs: &dyn BlobSource,
    budget: &mut Budget,
    notes: &mut Vec<DecodeNote>,
) -> Result<Document, CodecError> {
    decode_from(document, &mut Slices { pages, masters }, blobs, budget, notes)
}

/// [`decode`], with the parts asked for one at a time from `parts` ([`PartSource`]): each part's tree is let go before the next is asked for, so a decode holds the
/// document part, one part and the model it is making, and not every part's tree.
pub fn decode_from<S: PartSource>(
    document: &El,
    parts: &mut S,
    blobs: &dyn BlobSource,
    budget: &mut Budget,
    notes: &mut Vec<DecodeNote>,
) -> Result<Document, CodecError> {
    decode_parts(document, parts, blobs, budget, notes, true)
}

/// A decode that keeps nothing of the pages and masters it reads: each is read, charged to `budget`, and let go before the next, so that what it holds at the
/// most is the document and one part. What it says is what [`decode`] would say of the same parts and the same budget: the save's pre-flight, which refuses a
/// model that an open would refuse (`crate::model::preflight`).
pub fn check_decodes(
    document: &El,
    pages: &[&El],
    masters: &[&El],
    blobs: &dyn BlobSource,
    budget: &mut Budget,
    notes: &mut Vec<DecodeNote>,
) -> Result<(), CodecError> {
    check_decodes_from(document, &mut Slices { pages, masters }, blobs, budget, notes)
}

/// [`check_decodes`], with the parts asked for one at a time from `parts`.
pub fn check_decodes_from<S: PartSource>(
    document: &El,
    parts: &mut S,
    blobs: &dyn BlobSource,
    budget: &mut Budget,
    notes: &mut Vec<DecodeNote>,
) -> Result<(), CodecError> {
    decode_parts(document, parts, blobs, budget, notes, false).map(drop)
}

/// How a decode shares what it reads: it keeps the copy of each unit when it makes the document (an open), and the cost of each and nothing more when it only
/// checks (the save's check).
fn sharing_for(keep: bool) -> Sharing {
    if keep { Sharing::Keep } else { Sharing::Count }
}

fn decode_parts<S: PartSource>(
    document: &El,
    parts: &mut S,
    blobs: &dyn BlobSource,
    budget: &mut Budget,
    notes: &mut Vec<DecodeNote>,
    keep: bool,
) -> Result<Document, CodecError> {
    let lists = part_lists(document)?;
    if let Some((pages, masters)) = parts.counts()
        && (lists.pages.len() != pages || lists.masters.len() != masters)
    {
        return Err(CodecError::malformed("document", "the lists name other parts than were given"));
    }
    let mut dec = Dec::new(budget, notes, blobs).sharing(sharing_for(keep));
    dec.records = Some(lists.blobs.iter().map(|r| (r.id.0, r.clone())).collect());
    // The document part's tree is held by the caller for as long as the pages are read: what it has beyond what is free is held back from the model's pool.
    let held_back = dec.budget.reserve_tree(super::save::items_and_depth(document).0)?;
    dec.begin_part("document");
    let sc = part_scope(&mut dec, document, el::DOCUMENT, "document")?;
    let mut doc = dec.document(document, &sc)?;
    let mut page_list: Vec<Arc<Page>> = Vec::with_capacity(if keep { lists.pages.len() } else { 0 });
    for (index, (id, name)) in lists.pages.iter().enumerate() {
        // The tree is let go before the next part is asked for: a decode holds one part's tree at a time.
        let page = match parts.page_bytes(index, name)? {
            // A source that holds the part as bytes is read as a stream of the children of its root, one child's tree at a time.
            Some(bytes) => dec.read_page_bytes(bytes.as_ref(), *id)?,
            None => {
                let tree = parts.page(index, name)?;
                dec.read_page(tree.borrow(), *id)?
            }
        };
        if keep {
            page_list.push(Arc::new(page));
        }
    }
    let mut master_list: Vec<Arc<Master>> = Vec::with_capacity(if keep { lists.masters.len() } else { 0 });
    for (index, (id, name)) in lists.masters.iter().enumerate() {
        let master = match parts.master_bytes(index, name)? {
            Some(bytes) => dec.read_master_bytes(bytes.as_ref(), *id)?,
            None => {
                let tree = parts.master(index, name)?;
                dec.read_master(tree.borrow(), *id)?
            }
        };
        if keep {
            master_list.push(Arc::new(master));
        }
    }
    dec.budget.release_tree(held_back);
    finish(dec)?;
    if keep {
        doc.pages = page_list.into_iter().collect();
        doc.masters = master_list.into_iter().collect();
        doc.check_limits()?;
    }
    Ok(doc)
}

/// A decode that keeps nothing, run a part at a time as the parts are made: the save's check ([`super::save_parts`]). It spends the budget by the steps
/// [`decode_from`] does, in the same order (the document part first, then each page, then each master), and keeps no unit of what it reads ([`Sharing::Count`]).
pub(super) struct Check<'b> {
    dec: Dec<'b>,
    lists: PartLists,
    /// What the document part's tree takes of the model's pool beyond what is free, held back for as long as the pages and masters are read ([`Budget::reserve_tree`]), as an
    /// open holds it: a check that did not would pass a drawing the open refuses.
    held_back: u64,
}

impl<'b> Check<'b> {
    /// Begins the check with the document part, which is read, charged and let go.
    pub(super) fn begin(
        document: &El,
        blobs: &'b dyn BlobSource,
        budget: &'b mut Budget,
        notes: &'b mut Vec<DecodeNote>,
    ) -> Result<Check<'b>, CodecError> {
        let lists = part_lists(document)?;
        let mut dec = Dec::new(budget, notes, blobs).sharing(Sharing::Count);
        dec.records = Some(lists.blobs.iter().map(|r| (r.id.0, r.clone())).collect());
        let held_back = dec.budget.reserve_tree(super::save::items_and_depth(document).0)?;
        dec.begin_part("document");
        let sc = part_scope(&mut dec, document, el::DOCUMENT, "document")?;
        drop(dec.document(document, &sc)?);
        Ok(Check { dec, lists, held_back })
    }

    /// What the document part lists.
    pub(super) fn lists(&self) -> &PartLists {
        &self.lists
    }

    /// Reads and lets go the page part `tree`, which is the page whose `ID` is `id`.
    pub(super) fn page(&mut self, tree: &El, id: u32) -> Result<(), CodecError> {
        self.dec.read_page(tree, id).map(drop)
    }

    /// Reads and lets go the master part `tree`.
    pub(super) fn master(&mut self, tree: &El, id: u32) -> Result<(), CodecError> {
        self.dec.read_master(tree, id).map(drop)
    }

    /// Reads and lets go the page part whose bytes are `bytes`, as a stream of the children of its root (what an open does of a source that holds the part as bytes).
    pub(super) fn page_bytes(&mut self, bytes: &[u8], id: u32) -> Result<(), CodecError> {
        self.dec.read_page_bytes(bytes, id).map(drop)
    }

    /// Reads and lets go the master part whose bytes are `bytes`.
    pub(super) fn master_bytes(&mut self, bytes: &[u8], id: u32) -> Result<(), CodecError> {
        self.dec.read_master_bytes(bytes, id).map(drop)
    }

    /// Ends the check: what was lost, counted, is one more item.
    pub(super) fn finish(self) -> Result<(), CodecError> {
        self.dec.budget.release_tree(self.held_back);
        finish(self.dec)
    }
}

macro_rules! fragment {
    ($(#[$doc:meta])* $name:ident, $method:ident, $ret:ty) => {
        $(#[$doc])*
        pub fn $name(el: &El, blobs: &dyn BlobSource, budget: &mut Budget, notes: &mut Vec<DecodeNote>) -> Result<$ret, CodecError> {
            let mut dec = Dec::new(budget, notes, blobs);
            let sc = fragment_scope(&mut dec, el)?;
            let out = dec.$method(el, &sc)?;
            finish(dec)?;
            Ok(out)
        }
    };
}

fragment!(
    /// A style, from the element [`encode_style`](super::encode_style) made, within `budget`.
    decode_style,
    style,
    Style
);
fragment!(
    /// A sheet, from the element [`encode_sheet`](super::encode_sheet) made, within `budget`.
    decode_sheet,
    sheet,
    Sheet
);

/// A held value, from the element [`encode_held`](super::encode_held) made, within `budget`. What the value's record kept unknown has no owner here to be
/// kept in, so it is counted as unknown.
pub fn decode_held(el: &El, blobs: &dyn BlobSource, budget: &mut Budget, notes: &mut Vec<DecodeNote>) -> Result<diacraft_doc::Held, CodecError> {
    let mut dec = Dec::new(budget, notes, blobs);
    dec.begin_part("held");
    let sc = fragment_scope(&mut dec, el)?;
    let out = dec.held(el, &sc, &mut super::ext::Ordinals::default())?;
    let lost = u64::try_from(dec.keyed.entries.len()).unwrap_or(u64::MAX);
    dec.unknown = dec.unknown.saturating_add(lost);
    finish(dec)?;
    Ok(out)
}

/// A shape and what is under it, from the element [`encode_shape`](super::encode_shape) made, within `budget`. The depth is counted from this shape as the
/// first level.
pub fn decode_shape(el: &El, blobs: &dyn BlobSource, budget: &mut Budget, notes: &mut Vec<DecodeNote>) -> Result<Shape, CodecError> {
    let mut dec = Dec::new(budget, notes, blobs);
    dec.begin_part("shape");
    let sc = fragment_scope(&mut dec, el)?;
    let out = dec.shape(el, &sc, 1)?;
    finish(dec)?;
    Ok(out)
}

/// A page, from a part's tree made by [`encode_page`](super::encode_page), within `budget`.
pub fn decode_page(el: &El, blobs: &dyn BlobSource, budget: &mut Budget, notes: &mut Vec<DecodeNote>) -> Result<Page, CodecError> {
    let mut dec = Dec::new(budget, notes, blobs);
    let sc = part_scope(&mut dec, el, super::vocab::el::PAGE, "page")?;
    let out = dec.page(el, &sc)?;
    finish(dec)?;
    Ok(out)
}

/// A master, from a part's tree made by [`encode_master`](super::encode_master), within `budget`.
pub fn decode_master(el: &El, blobs: &dyn BlobSource, budget: &mut Budget, notes: &mut Vec<DecodeNote>) -> Result<Master, CodecError> {
    let mut dec = Dec::new(budget, notes, blobs);
    let sc = part_scope(&mut dec, el, super::vocab::el::MASTER, "master")?;
    let out = dec.master(el, &sc)?;
    finish(dec)?;
    Ok(out)
}

/// A unit of a sheet that the decoder can read: a sheet or a section.
pub(super) trait DecodeUnit: Sized {
    fn decode(dec: &mut Dec<'_>, e: &El, sc: &Scope<Ours>) -> Result<Self, CodecError>;
}

impl DecodeUnit for Sheet {
    fn decode(dec: &mut Dec<'_>, e: &El, sc: &Scope<Ours>) -> Result<Sheet, CodecError> {
        dec.sheet(e, sc)
    }
}

impl DecodeUnit for Section {
    fn decode(dec: &mut Dec<'_>, e: &El, sc: &Scope<Ours>) -> Result<Section, CodecError> {
        dec.section(e, sc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemBlobs;

    const HUGE: u64 = u64::MAX / 4;

    #[test]
    fn the_check_reads_a_master_part_as_an_open_does_from_its_bytes_and_charges_it() {
        // The second pass of a save checks each master part's bytes as it checks each page's.
        use diacraft_doc::{Master, Shape, ShapeList};
        let mut doc = diacraft_doc::Document::empty();
        let mut master = Master { id: 10, ..Master::default() };
        master.shapes = ShapeList::from_vec((1..=20).map(|i| Arc::new(Shape::new(i))).collect());
        doc.masters.push(Arc::new(master));
        let encoded = crate::model::encode(&doc, &mut MemBlobs::new()).expect("a document and a master");
        let (id, tree) = encoded.masters.first().expect("one master part");
        let bytes = diacraft_container::xml::to_bytes(tree, &[], &[], true).expect("the bytes of the part");
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut check = Check::begin(&encoded.document, &blobs, &mut budget, &mut notes).expect("a check");
        let before = check.dec.budget.bytes;
        check.master_bytes(&bytes, *id).expect("the master part is read");
        let charged = before - check.dec.budget.bytes;
        assert!(charged > 20 * 100, "twenty shapes are charged: {charged}");
        // A master whose ID is not the list's is damage, as it is of a tree; a page's bytes are no master.
        let wrong = check.master_bytes(&bytes, id + 1);
        assert_eq!(wrong, Err(CodecError::malformed("master", "a master part whose ID is not the one the list gives")));
        let page = check.master_bytes(b"<page/>", *id);
        assert!(matches!(page, Err(CodecError::Malformed { .. })), "{page:?}");
    }

    #[test]
    fn a_unit_that_leaves_a_note_a_blob_charge_or_a_record_beside_it_is_no_unit_to_share() {
        // Each of the three is enough, on its own, to make a unit's reading differ from the reading of an equal one.
        let before = Footprint { notes: 2, blobs: 100, keyed: (3, 4) };
        assert_eq!(before, Footprint { notes: 2, blobs: 100, keyed: (3, 4) });
        assert_ne!(before, Footprint { notes: 3, ..before }, "a note");
        assert_ne!(before, Footprint { blobs: 101, ..before }, "a blob that was charged");
        assert_ne!(before, Footprint { keyed: (4, 4), ..before }, "a record kept");
        assert_ne!(before, Footprint { keyed: (3, 5), ..before }, "a key seen");
        // And the decode's own footprint moves with what it holds.
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let dec = Dec::new(&mut budget, &mut notes, &blobs);
        let start = dec.footprint();
        dec.budget.charge_blob(7).unwrap();
        assert_eq!(dec.footprint(), Footprint { blobs: start.blobs + 7, ..start });
        dec.notes.push(DecodeNote::PartLost { name: "x".to_owned() });
        assert_eq!(dec.footprint(), Footprint { blobs: start.blobs + 7, notes: start.notes + 1, ..start });
    }

    /// A part as bytes: a root of `children` children that each hold `inside` empty elements (that many items and one more).
    fn part_of(children: usize, inside: usize) -> Vec<u8> {
        let mut text = String::from("<page>");
        for _ in 0..children {
            text.push_str("<shape>");
            for _ in 0..inside {
                text.push_str("<x/>");
            }
            text.push_str("</shape>");
        }
        text.push_str("</page>");
        text.into_bytes()
    }

    /// A document whose own sheet is a path of `rows` rows of one cell each: a document part of many items (about eight a row).
    fn document_with_rows(rows: u32) -> diacraft_doc::Document {
        use diacraft_doc::{Cell, Row, Section, Value};
        let mut doc = diacraft_doc::Document::empty();
        let mut section = Section::new("Geometry", Some(0));
        for i in 0..rows {
            let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(i + 1), ..Row::default() };
            row.cells.insert("X".into(), Cell::constant(Value::from_text(&i.to_string())));
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut doc.sheet).sections.push(Arc::new(section));
        doc
    }

    #[test]
    fn the_check_holds_back_the_document_parts_tree_beyond_what_is_free_as_the_open_does_and_gives_it_back_when_it_is_done() {
        // The second pass of a save reads the parts as an open does, and an open holds the document part's tree while the pages are read; a check that
        // did not passed drawings whose model, with that tree, is past the pool that the open then refuses.
        let doc = document_with_rows(60_000);
        let encoded = crate::model::encode(&doc, &mut MemBlobs::new()).expect("a document part");
        let items = crate::model::save::items_and_depth(&encoded.document).0;
        assert!(items > crate::model::OPEN_DOCUMENT_FREE_ITEMS + 100_000, "a document part of more items than are free: {items}");
        let held = (items - crate::model::OPEN_DOCUMENT_FREE_ITEMS) * crate::model::cost::TREE_ITEM_BYTES;
        let blobs = MemBlobs::new();
        // What the document costs the model, without a tree held back: the open's budget after the document part.
        let (mut open, mut notes) = (Budget::new(HUGE, HUGE, 0), Vec::new());
        crate::model::decode(&encoded.document, &[], &[], &blobs, &mut open, &mut notes).expect("an open");
        let cost = HUGE - open.bytes;
        // The check of the same document at a pool that holds the model and not the tree: the open refuses it, and so must the check.
        let tight = |pool: u64| {
            let mut budget = Budget::new(HUGE, pool, 0);
            let mut notes = Vec::new();
            let opened = crate::model::decode(&encoded.document, &[], &[], &blobs, &mut budget, &mut notes).map(drop);
            let mut budget = Budget::new(HUGE, pool, 0);
            let mut notes = Vec::new();
            let checked = Check::begin(&encoded.document, &blobs, &mut budget, &mut notes).and_then(Check::finish);
            (opened, checked)
        };
        let edge = cost + held;
        assert_eq!(tight(edge), (Ok(()), Ok(())), "a pool of the model and the tree holds both");
        let (opened, checked) = tight(edge - 1);
        assert!(matches!(opened, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "the open: {opened:?}");
        assert_eq!(checked, opened, "one byte short of the model and the tree, the check says what the open does");
        // The tree is held for as long as the check runs and given back when it is done.
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, 0), Vec::new());
        let check = Check::begin(&encoded.document, &blobs, &mut budget, &mut notes).expect("a check");
        assert_eq!(check.dec.budget.bytes - check.dec.budget.room(), held, "the tree is held back beyond what is free");
        check.finish().expect("done");
        assert_eq!(budget.bytes, budget.room(), "given back");
    }

    /// What reading the part's children one at a time, at the limits of a stream, ends in.
    fn read_as_stream(bytes: &[u8]) -> Result<u64, CodecError> {
        let limits = stream_limits();
        let text = XmlText::new(bytes).map_err(|e| xml_error("page", &e))?;
        let (mut pull, _) = text.pull(&limits, ParseOptions::default()).map_err(|e| xml_error("page", &e))?;
        let mut children = 0u64;
        while pull.next_child().map_err(|e| xml_error("page", &e))?.is_some() {
            children += 1;
        }
        Ok(children)
    }

    #[test]
    fn a_part_read_as_a_stream_is_held_to_the_items_the_format_allows_a_part_in_all_though_each_child_is_small() {
        // 12,000,000 items in all is the format's limit (6.1): a part of a few thousand children of a few thousand items each is read, and one item more is refused, in
        // the words of the part and not of a child.
        assert_eq!(u64::try_from(stream_limits().max_document_nodes), Ok(FORMAT_PART_ITEMS));
        // The root, then 300 shapes of 40,000 elements and the shape: 12,000,301 items is over by 301, so that 299 shapes are the most the limit allows.
        assert_eq!(read_as_stream(&part_of(299, 40_000)), Ok(299));
        let refused = read_as_stream(&part_of(300, 40_000));
        assert_eq!(refused, Err(CodecError::Limit { what: "XML items in one part", max: FORMAT_PART_ITEMS }), "{refused:?}");
    }

    /// The scope inside `root`, in a part that declares our namespace on it.
    fn scope_of(root: &El) -> Scope<Ours> {
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        fragment_scope(&mut dec, root).expect("a scope")
    }

    /// A flat call of `args` arguments and one more: that many nodes and a few.
    fn flat_call(args: usize) -> String {
        format!("SUM({}1)", "1,".repeat(args))
    }

    #[test]
    fn a_formula_past_the_formula_crates_cap_is_the_budgets_refusal_only_when_the_budget_could_not_have_paid_for_the_crates_cap() {
        let text = flat_call(diacraft_formula::MAX_NODES);
        let blobs = MemBlobs::new();
        let cap = FORMULA_MAX_NODES * FORMULA_NODE_BYTES;
        // Exactly what the crate's most nodes cost: the budget could have paid for them, so the refusal is the formula's own and the text is kept as it is.
        let (mut budget, mut notes) = (Budget::new(HUGE, cap, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        assert!(matches!(dec.formula(&text, false), Ok(FormulaSlot::Local(_))));
        // A node's cost less: the budget's.
        let (mut budget, mut notes) = (Budget::new(HUGE, cap - FORMULA_NODE_BYTES, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        assert!(matches!(dec.formula(&text, false), Err(CodecError::Limit { what: "bytes of the model in memory", .. })));
    }

    #[test]
    fn a_formula_the_budget_cannot_afford_is_refused_with_the_most_the_budget_was_made_with_and_not_what_was_left() {
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, 50_000, HUGE), Vec::new());
        budget.charge_bytes(49_000).expect("most of it spent");
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        let out = dec.formula(&flat_call(100), false);
        assert!(matches!(out, Err(CodecError::Limit { what: "bytes of the model in memory", max: 50_000 })), "{out:?}");
    }

    #[test]
    fn a_part_holds_the_most_shapes_the_model_does_and_no_more() {
        let shape = El { name: "d:shape".to_owned(), attrs: vec![("id".to_owned(), "1".to_owned())], children: Vec::new() };
        let sc = scope_of(&shape);
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        dec.shapes = MAX_SHAPES_PER_PAGE - 1;
        assert!(dec.shape(&shape, &sc, 1).is_ok(), "the last shape the model allows");
        assert!(matches!(dec.shape(&shape, &sc, 1), Err(CodecError::Limit { what: "shapes in a part", .. })));
        assert!(matches!(dec.shape(&shape, &sc, 1), Err(CodecError::Limit { what: "shapes in a part", .. })));
        // And the depth: the 200th level is the last. A part begins its count of shapes again.
        dec.begin_part("page");
        assert_eq!(dec.shapes, 0);
        assert!(dec.shape(&shape, &sc, MAX_SHAPE_DEPTH).is_ok());
        assert!(matches!(dec.shape(&shape, &sc, MAX_SHAPE_DEPTH + 1), Err(CodecError::Limit { what: "levels of nested shapes", .. })));
        assert_eq!(dec.part(), "page");
    }

    /// A long field is an element with an attribute and a text node: each costs what it is, and the budget refuses the part one item short.
    #[test]
    fn a_long_field_child_is_charged_as_an_element_with_its_attributes_and_its_text() {
        let long =
            |text: &str| El { name: "d:s".to_owned(), attrs: vec![("n".to_owned(), "x".to_owned())], children: vec![Node::Text(text.to_owned())] };
        let e =
            El { name: "d:shape".to_owned(), attrs: Vec::new(), children: vec![Node::El(long("ab")), Node::El(long("cd")), Node::El(long("ef"))] };
        let sc = scope_of(&e);
        let blobs = MemBlobs::new();
        // The element: 1. Each long child, its element and its attribute 2 and its one text node 1: 3.
        for (items, charged) in [(10u64, true), (9, false)] {
            let (mut budget, mut notes) = (Budget::new(items, HUGE, HUGE), Vec::new());
            let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
            assert_eq!(dec.fields(&e, &sc, |_, _, _| Ok(true)).is_ok(), charged, "{items} items");
        }
    }

    #[test]
    fn the_attributes_one_element_left_that_nobody_took_are_counted_and_not_given_to_the_next() {
        let element = |name: &str, attr: &str| El { name: name.to_owned(), attrs: vec![(attr.to_owned(), "1".to_owned())], children: Vec::new() };
        let (first, second) = (element("d:a", "future"), element("d:b", "id"));
        let sc = scope_of(&first);
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        dec.fields(&first, &sc, |_, n, _| Ok(n == "id")).expect("the first element");
        dec.fields(&second, &sc, |_, n, _| Ok(n == "id")).expect("the second element");
        assert_eq!(dec.unknown, 1, "the first's attribute is counted when the next element is read");
        assert_eq!(dec.native_attrs(&sc).expect("native attributes").iter().count(), 0, "and is not the second's");
    }

    // ----------------------------------------------------------------------------------------------------------------------------- a collision of keys

    /// A page of `shapes` shapes whose sheet is one of `kinds` different ones, in turn: each has a cell of its own and a section that is the same for all of its kind.
    fn page_of_kinds(kinds: usize, shapes: usize) -> El {
        let body: String = (0..shapes)
            .map(|i| {
                let k = i % kinds;
                format!(
                    "<d:shape id=\"{}\"><d:sheet><d:Cell N=\"PinX\" V=\"{k}\"/><d:Section N=\"User\"><d:Cell N=\"A\" V=\"{k}\"/></d:Section></d:sheet></d:shape>",
                    i + 1
                )
            })
            .collect();
        parsed(&format!("<d:page xmlns:d=\"{URN}\" id=\"1\">{body}</d:page>"))
    }

    /// The page `tree` read by a decode that shares as `share` says, and what it spent: its bytes and its items.
    fn read_with(share: Share, tree: &El) -> (Page, u64, u64) {
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        dec.share = share;
        let page = dec.read_page(tree, 1).expect("a page");
        (page, HUGE - budget.bytes, HUGE - budget.items)
    }

    #[test]
    fn a_decode_that_makes_the_document_keeps_the_copy_of_each_unit_and_a_check_keeps_the_cost_of_each_and_nothing_more() {
        assert_eq!(sharing_for(true), Sharing::Keep);
        assert_eq!(sharing_for(false), Sharing::Count);
        // The check of a save is the second: what it reads, it keeps no unit of (a copy of the model beside the model).
        let tree = page_of_kinds(2, 6);
        let document = document_of(&[1], &[]);
        let blobs = MemBlobs::new();
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes).unwrap();
        assert_eq!(check.dec.share.mode, Sharing::Count);
        check.page(&tree, 1).unwrap();
        assert!(check.dec.share.sheets.len() > 0 && check.dec.share.sections.len() > 0, "the units were entered");
        assert!(check.dec.share.sheets.values_kept() == 0 && check.dec.share.sections.values_kept() == 0, "and none of them is held");
        // The decode of the same page that keeps units holds each of them.
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let mut dec = Dec::new(&mut budget, &mut notes, &blobs);
        dec.read_page(&tree, 1).unwrap();
        assert_eq!(dec.share.mode, Sharing::Keep);
        assert_eq!(dec.share.sheets.values_kept(), dec.share.sheets.len());
        assert_eq!(dec.share.sections.values_kept(), dec.share.sections.len());
    }

    #[test]
    fn units_whose_keys_collide_are_told_apart_by_their_streams_and_the_equal_ones_are_still_shared() {
        // A hasher whose keys every unit shares (what a target without a secret for its hasher can be made to give): three kinds of sheet, each twelve times, and one
        // kind of section for each, all under one key. No shape is given another's sheet, and what is the same is the same copy, at the price it has when keys differ.
        let tree = page_of_kinds(3, 36);
        let (kept, bytes, items) = read_with(Share::new(Sharing::Keep), &tree);
        let (collided, collided_bytes, collided_items) = read_with(Share::colliding(Sharing::Keep), &tree);
        assert_eq!(collided, kept, "the same page");
        assert_eq!((collided_bytes, collided_items), (bytes, items), "at the same price");
        for i in 0..36 {
            let (a, b) = (collided.shapes.get(i).unwrap(), collided.shapes.get(i % 3).unwrap());
            assert!(Arc::ptr_eq(&a.sheet, &b.sheet), "shape {i} has the copy of its kind");
            assert_eq!(a.sheet.cells.get("PinX").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()), Some((i % 3).to_string()), "shape {i}");
        }
        assert!(!Arc::ptr_eq(&collided.shapes.get(0).unwrap().sheet, &collided.shapes.get(1).unwrap().sheet));
    }

    #[test]
    fn more_different_units_under_one_key_than_a_key_holds_are_decoded_as_they_are_and_none_is_given_another_s_copy() {
        // Twelve kinds, past what one key holds: the ones that were entered are shared, and the others are each their own, and every shape has its own kind's sheet.
        let tree = page_of_kinds(12, 48);
        let (plain, ..) = read_with(Share::new(Sharing::Off), &tree);
        for mode in [Sharing::Keep, Sharing::Count] {
            let (page, ..) = read_with(Share::colliding(mode), &tree);
            if mode == Sharing::Keep {
                assert_eq!(page, plain, "{mode:?}: the page is the page");
            }
        }
        let (page, ..) = read_with(Share::colliding(Sharing::Keep), &tree);
        for i in 0..48 {
            let shape = page.shapes.get(i).unwrap();
            assert_eq!(
                shape.sheet.cells.get("PinX").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()),
                Some((i % 12).to_string()),
                "shape {i}"
            );
        }
    }

    #[test]
    fn a_check_and_an_open_spend_the_same_when_every_key_collides() {
        let tree = page_of_kinds(3, 36);
        let (_, keep_bytes, keep_items) = read_with(Share::colliding(Sharing::Keep), &tree);
        let (_, count_bytes, count_items) = read_with(Share::colliding(Sharing::Count), &tree);
        assert_eq!((count_bytes, count_items), (keep_bytes, keep_items));
    }

    // ----------------------------------------------------------------------------------------------------------------------------- the save's check

    fn parsed(xml: &str) -> El {
        diacraft_container::xml::parse_with(xml.as_bytes(), &diacraft_container::xml::XmlLimits { max_depth: 512, ..Default::default() }).unwrap()
    }

    fn document_of(page_ids: &[u32], master_ids: &[u32]) -> El {
        let d = "diacraft";
        let pages: String = page_ids.iter().map(|id| format!("<{d}:page-ref id=\"{id}\" part=\"pages/page{id}.xml\"/>")).collect();
        let masters: String = master_ids.iter().map(|id| format!("<{d}:master-ref id=\"{id}\" part=\"masters/master{id}.xml\"/>")).collect();
        parsed(&format!("<{d}:document xmlns:{d}=\"{URN}\">{pages}{masters}</{d}:document>"))
    }

    fn part(kind: &str, id: u32, inner: &str) -> El {
        parsed(&format!("<d:{kind} xmlns:d=\"{URN}\" id=\"{id}\">{inner}</d:{kind}>"))
    }

    /// What a check of the page `tree` and the master `master` costs a budget with everything to spend: its items and its bytes.
    fn cost_of_check(document: &El, page: &El, master: &El) -> (u64, u64) {
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let blobs = MemBlobs::new();
        let mut check = Check::begin(document, &blobs, &mut budget, &mut notes).unwrap();
        check.page(page, 1).unwrap();
        check.master(master, 2).unwrap();
        check.finish().unwrap();
        (HUGE - budget.items, HUGE - budget.bytes)
    }

    #[test]
    fn a_check_reads_each_page_and_master_charges_the_budget_for_it_and_refuses_what_does_not_fit() {
        let document = document_of(&[1], &[2]);
        let page = part("page", 1, "<d:shape id=\"1\"/><d:shape id=\"2\"/>");
        let master = part("master", 2, "<d:shape id=\"3\"/>");
        let (items, bytes) = cost_of_check(&document, &page, &master);
        let attempt = |items: u64, bytes: u64| -> Result<(), CodecError> {
            let (mut budget, mut notes) = (Budget::new(items, bytes, HUGE), Vec::new());
            let blobs = MemBlobs::new();
            let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes)?;
            check.page(&page, 1)?;
            check.master(&master, 2)?;
            check.finish()
        };
        assert_eq!(attempt(items, bytes), Ok(()), "what it costs is enough");
        // One item or one byte short is refused, and by the budget in words: the page and the master are each charged, so that neither is a call that does nothing.
        assert!(matches!(attempt(items - 1, bytes), Err(CodecError::Limit { what: "items in the drawing", .. })));
        assert!(matches!(attempt(items, bytes - 1), Err(CodecError::Limit { what: "bytes of the model in memory", .. })));
        // The page alone is charged what two shapes cost, and the master alone what one costs.
        let bytes_after = |with_page: bool| {
            let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
            let blobs = MemBlobs::new();
            {
                let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes).unwrap();
                if with_page {
                    check.page(&page, 1).unwrap();
                }
            }
            HUGE - budget.bytes
        };
        let page_only = bytes_after(true) - bytes_after(false);
        assert!(page_only >= 2 * element_bytes(el::SHAPE), "{page_only}");
    }

    #[test]
    fn a_check_refuses_a_page_or_a_master_that_is_not_the_one_the_list_names() {
        let document = document_of(&[1], &[2]);
        let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
        let blobs = MemBlobs::new();
        let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes).unwrap();
        assert_eq!(check.lists().pages, vec![(1, "pages/page1.xml".to_owned())]);
        assert_eq!(check.lists().masters, vec![(2, "masters/master2.xml".to_owned())]);
        assert_eq!(
            check.page(&part("page", 9, ""), 1).err(),
            Some(CodecError::malformed("page", "a page part whose ID is not the one the list gives"))
        );
        assert_eq!(
            check.master(&part("master", 9, ""), 2).err(),
            Some(CodecError::malformed("master", "a master part whose ID is not the one the list gives"))
        );
        // A part that is not the kind asked for is refused too.
        assert!(check.page(&part("master", 1, ""), 1).is_err());
        assert!(check.master(&part("page", 2, ""), 2).is_err());
    }

    #[test]
    fn what_a_check_lost_is_counted_by_the_end_as_an_item_and_a_note_and_is_refused_when_the_budget_has_no_item_for_it() {
        // A second `ext` in a shape is not read and is counted as unknown.
        let document = document_of(&[1], &[]);
        let page = part("page", 1, "<d:shape id=\"1\"><d:ext/><d:ext/></d:shape>");
        let run = |items: u64| {
            let (mut budget, mut notes) = (Budget::new(items, HUGE, HUGE), Vec::new());
            let blobs = MemBlobs::new();
            let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes)?;
            check.page(&page, 1)?;
            check.finish()?;
            Ok::<_, CodecError>(notes)
        };
        let ok = run(HUGE).unwrap();
        assert_eq!(ok, vec![DecodeNote::Unknown { count: 1 }]);
        // The items of the document and the page, and then of the whole check: the note is the one more.
        let items_of = |end: bool| {
            let (mut budget, mut notes) = (Budget::new(HUGE, HUGE, HUGE), Vec::new());
            let blobs = MemBlobs::new();
            let mut check = Check::begin(&document, &blobs, &mut budget, &mut notes).unwrap();
            check.page(&page, 1).unwrap();
            if end {
                check.finish().unwrap();
            } else {
                drop(check);
            }
            HUGE - budget.items
        };
        let before_end = items_of(false);
        assert_eq!(items_of(true), before_end + 1, "the note is an item");
        assert!(matches!(run(before_end), Err(CodecError::Limit { what: "items in the drawing", .. })), "no item left for the note");
        assert!(run(before_end + 1).is_ok());
    }
}
