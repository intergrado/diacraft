//! Encoding: the model as trees of the vocabulary (`vocab.rs`), with every byte string sent to a [`BlobSink`].
//!
//! The encoder counts what it makes as the XML reader will count it (one item for each element, attribute and text node), and refuses a part that
//! would pass the reader's cap, and a model that passes a cap of the model, so that no file is written that cannot be opened. The depth of shapes
//! and of kept elements is held to their caps for the same reason. Nothing here recurses on anything but the shape tree (at most 200 levels, one small
//! frame each); a kept element is copied with a stack of its own (`kept.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_container::limits::{MAX_XML_NODES, XML_DEPTH_CEILING};
use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    Cell, CellExtra, ColorEntry, Connect, DocProps, Document, FaceName, ForeignData, FormulaSlot, Kept, Links, MAX_CELLS_PER_CONTAINER,
    MAX_COLOR_ENTRIES, MAX_MASTERS, MAX_NAME_BYTES, MAX_PAGES, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE,
    MAX_STYLES, Master, Media, Page, RawNode, Row, Section, Shape, ShapeType, Sheet, Style, Text, TextPart, Theme,
};

use super::escape::{ATTR_MAX_BYTES, base64_encode, needs_base64};
use super::stamp::stamp_of_bytes;
use super::stream::TreeSink;
use super::vocab::{at, el};
use super::{BlobId, BlobRecord, BlobSink, CodecError, Encoded, OPEN_PART_BYTES, OPEN_TREE_ITEMS, PREFIX, URN, blob_path};

/// The most items one part may hold: what the XML reader will parse.
const MAX_PART_ITEMS: u64 = MAX_XML_NODES as u64;

/// The deepest kept element the codec writes (the XML reader the model's own caps follow: 200). With the shapes' 200 and the few elements around
/// them it stays inside the depth the codec parses its parts at ([`XML_DEPTH_CEILING`]).
pub(crate) const MAX_KEPT_DEPTH: usize = 200;

fn limit(what: &'static str, max: usize) -> CodecError {
    CodecError::Limit { what, max: u64::try_from(max).unwrap_or(u64::MAX) }
}

/// What makes the trees: the sink, the blobs seen so far, and the items of the part in hand.
pub(crate) struct Encoder<'b> {
    pub(super) blobs: &'b mut dyn BlobSink,
    records: Vec<BlobRecord>,
    recorded: HashSet<u32>,
    /// A blob by the pointer and length of the `Arc` that was given: the model holds every `Arc` it names for as long as it is encoded, so an
    /// address is its own.
    seen: HashMap<(usize, usize), BlobId>,
    pub(super) items: u64,
    pub(super) cap: u64,
    /// What a part that passes `cap` is refused as: the words and the figure.
    pub(super) cap_error: (&'static str, u64),
    /// The bytes of the part in hand, **at the least** as it will be written: the names, the values and the text that were made (the part's exact size is
    /// [`super::written_len`]'s, once it is whole). A part is refused while it is made at the byte that passes `byte_cap`, as it is at the item that passes `cap`: a
    /// model that shares one long text among many shapes writes it for each of them, and a tree held to its items alone would be the text times the shapes.
    bytes: u64,
    byte_cap: u64,
    byte_error: (&'static str, u64),
    shapes: usize,
    /// The sheets and sections of the part in hand that more than one place of the model holds (the one allocation), and which of them have been written.
    pub(super) units: Units,
    /// Whether equal units are written once and said again (`def` and `same`): on, unless [`Encoder::without_sharing`] says otherwise.
    pub(super) shares: bool,
}

/// The units of a part that the model shares, **by value**: how many places hold each class of equal units, and the name each class was given when it was first
/// written. A class is named by the address of its first allocation ([`super::byvalue`]), and every allocation of the class is looked up by it. A unit that one place holds
/// is written in full and has no name; one that several hold (as one allocation or as equal ones) is written in full once, with the name (`def`), and as a `same` that names
/// it everywhere after, so that what the model holds once, or says in many places alike, the file says once.
#[derive(Default)]
pub(super) struct Units {
    /// The class of an allocation that is not the first of its class.
    class: HashMap<usize, usize>,
    /// The places that hold each shared class (two or more).
    held: HashMap<usize, u32>,
    /// The name each shared class was given when it was written.
    named: HashMap<usize, u32>,
    /// The names given so far.
    given: u32,
    /// The fingerprint of each unit that was named, by its name: what a `same` inside a later unit is to the fingerprint of that unit ([`super::unitfp`]).
    fps: HashMap<String, diacraft_doc::stamp::Stamp>,
}

impl Units {
    /// The units of `sheets` (a part's own sheets and its shapes' sheets), found by a walk of the shapes with a stack of its own, and the sections of each, in classes of
    /// equal units.
    pub(super) fn scan<'a>(own: impl Iterator<Item = &'a Arc<Sheet>>, lists: &[&'a diacraft_doc::ShapeList]) -> Units {
        let mut classes = super::byvalue::Classifier::new();
        for s in own {
            classes.place(s);
        }
        classes.shapes(lists);
        let super::byvalue::Classes { class, held } = classes.finish();
        Units { class, held, named: HashMap::new(), given: 0, fps: HashMap::new() }
    }

    /// The class of the unit at `addr`: its own address when it is the first of its class, or a unit that was not met.
    fn class_of(&self, addr: usize) -> usize {
        self.class.get(&addr).copied().unwrap_or(addr)
    }
}

/// The caps of the trees an encode makes: of the items and the bytes of the parts it holds at once, and of each part.
#[derive(Clone, Copy)]
pub(super) struct Caps {
    /// Items between the parts held at once, and what a part that passes the room left is refused as.
    pub(super) total_items: u64,
    pub(super) total_items_error: (&'static str, u64),
    /// Items of one part.
    pub(super) part_items: u64,
    /// Bytes between the parts held at once, and what a part that passes the room left is refused as.
    pub(super) total_bytes: u64,
    pub(super) total_bytes_error: (&'static str, u64),
    /// Bytes of one part as written.
    pub(super) part_bytes: u64,
}

/// The bytes an element has of its own, **at the least** as it is written (an element with no children is written `<name/>`, and one with children has its
/// closing tag besides, which is counted when the part is measured): its name and the marks round it, and its attributes' names and values and the marks round them.
/// Its children are their own.
pub(super) fn element_floor(e: &El) -> u64 {
    let mut total = (e.name.len() as u64).saturating_add(3);
    for (k, v) in &e.attrs {
        total = total.saturating_add(k.len() as u64).saturating_add(v.len() as u64).saturating_add(4);
    }
    total
}

/// What an encoder knows of the blobs it has numbered, which a second encoder of the same model can start from: the second pass of a save makes the parts again
/// without a sink, every blob it meets being one the first pass numbered.
pub(crate) struct EncState {
    records: Vec<BlobRecord>,
    recorded: HashSet<u32>,
    seen: HashMap<(usize, usize), BlobId>,
}

/// A sink for the second pass of an encode: every blob is known already, and one that is not is a bug of the encoder, refused in words.
pub(crate) struct NoSink;

impl BlobSink for NoSink {
    fn put(&mut self, _bytes: &Arc<[u8]>, _media_type: &str) -> Result<BlobId, CodecError> {
        Err(CodecError::malformed("document", "a byte string that the first pass of the save did not meet"))
    }
}

impl<'b> Encoder<'b> {
    pub(crate) fn new(blobs: &'b mut dyn BlobSink) -> Encoder<'b> {
        Encoder {
            blobs,
            records: Vec::new(),
            recorded: HashSet::new(),
            seen: HashMap::new(),
            items: 0,
            cap: MAX_PART_ITEMS,
            cap_error: ("XML items in one part", MAX_PART_ITEMS),
            bytes: 0,
            byte_cap: OPEN_PART_BYTES,
            byte_error: ("bytes in one part as written", OPEN_PART_BYTES),
            shapes: 0,
            units: Units::default(),
            shares: true,
        }
    }

    /// This encoder, writing every unit in full where it stands (the part of version 1.0 of the vocabulary): what the content stamp walks, which is over the content and not
    /// over how a part says it.
    pub(super) fn without_sharing(mut self) -> Encoder<'b> {
        self.shares = false;
        self
    }

    /// The units of a part whose own sheets are `own` and whose shapes are in `lists`: the classes of equal ones, or none when this encoder shares nothing.
    pub(super) fn units_of<'a>(&self, own: impl Iterator<Item = &'a Arc<Sheet>>, lists: &[&'a diacraft_doc::ShapeList]) -> Units {
        if self.shares { Units::scan(own, lists) } else { Units::default() }
    }

    /// The most items the next part may have, and what it is refused as when it passes them. A part is refused **while it is made**, at the item that passes the
    /// figure, and not once it is whole: the trees an encoder holds are what it is allowed to make.
    pub(super) fn set_cap(&mut self, items: u64, what: &'static str, max: u64) {
        self.cap = items;
        self.cap_error = (what, max);
    }

    /// The most bytes the next part may have, and what it is refused as when it passes them: refused **while it is made**, before the string that passes the
    /// figure is copied.
    pub(super) fn set_byte_cap(&mut self, bytes: u64, what: &'static str, max: u64) {
        self.byte_cap = bytes;
        self.byte_error = (what, max);
    }

    /// The caps of the part that follows `items` and `bytes` of the parts held before it, under `caps`.
    pub(super) fn cap_for(&mut self, caps: &Caps, items: u64, bytes: u64) {
        let room = caps.total_items.saturating_sub(items);
        if room < caps.part_items {
            self.set_cap(room, caps.total_items_error.0, caps.total_items_error.1);
        } else {
            self.set_cap(caps.part_items, "XML items in one part", caps.part_items);
        }
        let room = caps.total_bytes.saturating_sub(bytes);
        if room < caps.part_bytes {
            self.set_byte_cap(room, caps.total_bytes_error.0, caps.total_bytes_error.1);
        } else {
            self.set_byte_cap(caps.part_bytes, "bytes in one part as written", caps.part_bytes);
        }
    }

    /// An encoder that goes on from `state` (the blobs another has numbered) with `blobs`.
    pub(crate) fn resume(blobs: &'b mut dyn BlobSink, state: EncState) -> Encoder<'b> {
        let mut enc = Encoder::new(blobs);
        (enc.records, enc.recorded, enc.seen) = (state.records, state.recorded, state.seen);
        enc
    }

    /// What this encoder knows of the blobs, for [`Encoder::resume`].
    pub(crate) fn into_state(self) -> EncState {
        EncState { records: self.records, recorded: self.recorded, seen: self.seen }
    }

    /// The blobs seen, in the order of their numbers.
    pub(crate) fn records(&self) -> &[BlobRecord] {
        &self.records
    }

    pub(super) fn begin_part(&mut self) {
        self.items = 0;
        self.bytes = 0;
        self.shapes = 0;
        // The units of a part are the part's own: the names are given again in each.
        self.units = Units::default();
    }

    /// The bytes of the part in hand so far ([`Encoder::count_bytes`]).
    pub(crate) fn part_bytes(&self) -> u64 {
        self.bytes
    }

    /// Counts `n` bytes of the part in hand, and refuses the part that passes its cap.
    pub(super) fn count_bytes(&mut self, n: u64) -> Result<(), CodecError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.byte_cap {
            return Err(CodecError::Limit { what: self.byte_error.0, max: self.byte_error.1 });
        }
        Ok(())
    }

    /// Refuses a part that `n` more bytes would take past its cap, **before** the string of `n` bytes is made.
    pub(super) fn guard_bytes(&self, n: usize) -> Result<(), CodecError> {
        if self.bytes.saturating_add(u64::try_from(n).unwrap_or(u64::MAX)) > self.byte_cap {
            return Err(CodecError::Limit { what: self.byte_error.0, max: self.byte_error.1 });
        }
        Ok(())
    }

    pub(super) fn count(&mut self, n: u64) -> Result<(), CodecError> {
        self.items = self.items.saturating_add(n);
        if self.items > self.cap {
            return Err(CodecError::Limit { what: self.cap_error.0, max: self.cap_error.1 });
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------------------------ building

    /// A new element of the vocabulary; it is counted when it is added to its parent.
    pub(super) fn elem(&self, local: &str) -> El {
        El { name: format!("{PREFIX}:{local}"), attrs: Vec::new(), children: Vec::new() }
    }

    /// Adds `child` to `parent`, counting the child's own items: the element and each attribute.
    pub(super) fn add(&mut self, parent: &mut El, child: El) -> Result<(), CodecError> {
        self.count(1u64.saturating_add(u64::try_from(child.attrs.len()).unwrap_or(u64::MAX)))?;
        self.count_bytes(element_floor(&child))?;
        parent.children.push(Node::El(child));
        Ok(())
    }

    /// Adds a text node.
    pub(super) fn add_text(&mut self, parent: &mut El, text: String) -> Result<(), CodecError> {
        self.count(1)?;
        self.count_bytes(u64::try_from(text.len()).unwrap_or(u64::MAX))?;
        parent.children.push(Node::Text(text));
        Ok(())
    }

    /// The root of a part: the element with the namespace declared on it.
    pub(super) fn root(&self, local: &str) -> El {
        let mut e = self.elem(local);
        e.attrs.push((format!("xmlns:{PREFIX}"), URN.to_owned()));
        e
    }

    /// Ends a part: counts the root and gives it with the items of the whole part.
    fn finish_part(&mut self, root: El) -> Result<(El, u64), CodecError> {
        self.count(1u64.saturating_add(u64::try_from(root.attrs.len()).unwrap_or(u64::MAX)))?;
        self.count_bytes(element_floor(&root))?;
        Ok((root, self.items))
    }

    /// A text field: an attribute when it is short and plain, the same in base64 under the name with the suffix when it has a character XML forbids,
    /// and the text of an `s` child when it is long.
    pub(super) fn put_str(&mut self, e: &mut El, name: &str, value: &str) -> Result<(), CodecError> {
        self.guard_bytes(value.len())?;
        let bad = needs_base64(value);
        if value.len() <= ATTR_MAX_BYTES {
            if bad {
                e.attrs.push((format!("{name}{}", at::B64_SUFFIX), base64_encode(value.as_bytes())));
            } else {
                e.attrs.push((name.to_owned(), value.to_owned()));
            }
            return Ok(());
        }
        let mut long = self.elem(el::LONG);
        long.attrs.push((at::N_LONG.to_owned(), name.to_owned()));
        let text = if bad {
            long.attrs.push((at::B64.to_owned(), "1".to_owned()));
            base64_encode(value.as_bytes())
        } else {
            value.to_owned()
        };
        self.add_text(&mut long, text)?;
        self.add(e, long)
    }

    /// An optional text field: written when there is one, empty or not.
    pub(super) fn put_opt(&mut self, e: &mut El, name: &str, value: Option<&Arc<str>>) -> Result<(), CodecError> {
        match value {
            Some(v) => self.put_str(e, name, v),
            None => Ok(()),
        }
    }

    /// A text field whose default is the empty text: written when it is not.
    pub(super) fn put_req(&mut self, e: &mut El, name: &str, value: &str) -> Result<(), CodecError> {
        if value.is_empty() { Ok(()) } else { self.put_str(e, name, value) }
    }

    /// A number field with a default.
    pub(super) fn put_u32(e: &mut El, name: &str, value: u32, default: u32) {
        if value != default {
            e.attrs.push((name.to_owned(), value.to_string()));
        }
    }

    /// An optional number field.
    pub(super) fn put_opt_u32(e: &mut El, name: &str, value: Option<u32>) {
        if let Some(v) = value {
            e.attrs.push((name.to_owned(), v.to_string()));
        }
    }

    /// A flag: `1` when on, and nothing when off.
    pub(super) fn put_flag(e: &mut El, name: &str, on: bool) {
        if on {
            e.attrs.push((name.to_owned(), "1".to_owned()));
        }
    }

    // ------------------------------------------------------------------------------------------------------------ blobs

    /// The number of the blob `bytes`: stored once by the sink, and recorded the first time it is seen.
    pub(super) fn blob(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError> {
        let key = (Arc::as_ptr(bytes).cast::<u8>().addr(), bytes.len());
        if let Some(id) = self.seen.get(&key) {
            return Ok(*id);
        }
        let id = self.blobs.put(bytes, media_type)?;
        self.seen.insert(key, id);
        if self.recorded.insert(id.0) {
            let record = BlobRecord {
                id,
                path: blob_path(id, media_type),
                media_type: media_type.to_owned(),
                size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                stamp: stamp_of_bytes(bytes),
            };
            // A sink numbers blobs in order of first use, so this is the end of the list; one that does not still gets a list in order of number.
            let at = self.records.partition_point(|r| r.id < id);
            self.records.insert(at, record);
        }
        Ok(id)
    }

    // ------------------------------------------------------------------------------------------------------------ the document

    /// The names of the parts of `pages` or `masters` of the given folder and prefix: `pages/page<ID>.xml`, and for an ID that is used again,
    /// `pages/page<ID>-<k>.xml` for its k-th use after the first, so that no two parts share a name.
    fn part_names(ids: impl Iterator<Item = u32>, folder: &str, stem: &str) -> Vec<String> {
        let mut uses: HashMap<u32, u32> = HashMap::new();
        ids.map(|id| {
            let used = uses.entry(id).or_insert(0);
            let name = if *used == 0 { format!("{folder}/{stem}{id}.xml") } else { format!("{folder}/{stem}{id}-{used}.xml") };
            *used = used.saturating_add(1);
            name
        })
        .collect()
    }

    /// The model caps of the document itself, which the parts do not see.
    pub(crate) fn check_document(&self, doc: &Document) -> Result<(), CodecError> {
        if doc.pages.len() > MAX_PAGES {
            return Err(limit("pages", MAX_PAGES));
        }
        if doc.styles.len() > MAX_STYLES {
            return Err(limit("styles", MAX_STYLES));
        }
        if doc.masters.len() > MAX_MASTERS {
            return Err(limit("masters", MAX_MASTERS));
        }
        if doc.colors.len() > MAX_COLOR_ENTRIES {
            return Err(limit("colour table entries", MAX_COLOR_ENTRIES));
        }
        Ok(())
    }

    /// The document part, without the list of blobs (which is complete only when every page and master has been made): and its items.
    pub(crate) fn document_part(&mut self, doc: &Document) -> Result<(El, u64), CodecError> {
        // Every field is named, so that a field the model gains is a compile error here until the codec says what to do with it.
        let Document {
            version,
            title,
            props,
            pages,
            masters,
            styles,
            themes,
            colors,
            fonts,
            sheet,
            passthrough,
            diacraft,
            ext,
            parts,
            vsdx,
            comments,
        } = doc;
        self.begin_part();
        self.units = self.units_of(std::iter::once(sheet).chain(styles.iter().map(|s| &s.sheet)), &[]);
        let mut keyed = Self::keyed_unknowns(ext.get().map(|d| &d.native));
        let mut root = self.root(el::DOCUMENT);
        Self::put_u32(&mut root, at::VERSION, *version, Document::CURRENT_VERSION);
        self.put_req(&mut root, at::TITLE, title)?;
        if **props != DocProps::default() {
            let e = self.props(props, &mut keyed)?;
            self.add(&mut root, e)?;
        }
        for c in colors.iter() {
            let ColorEntry { ix, rgb } = c;
            let mut e = self.elem(el::COLOR);
            Self::put_u32(&mut e, at::IX, *ix, 0);
            self.put_req(&mut e, at::RGB, rgb)?;
            self.take_keyed(&mut e, &mut keyed, "color", &ix.to_string())?;
            self.add(&mut root, e)?;
        }
        for f in fonts.iter() {
            let e = self.font(f)?;
            self.add(&mut root, e)?;
        }
        if !is_default_sheet(sheet) {
            let e = self.sheet(sheet)?;
            self.add(&mut root, e)?;
        }
        for style in styles {
            let e = self.style(style)?;
            self.add(&mut root, e)?;
        }
        for theme in themes {
            let e = self.theme(theme)?;
            self.add(&mut root, e)?;
        }
        if let Some(solution) = diacraft {
            let mut e = self.elem(el::SOLUTION);
            let inner = self.raw_element_el(solution)?;
            self.add(&mut e, inner)?;
            self.add(&mut root, e)?;
        }
        if !comments.is_empty() {
            let e = self.comment_list(comments, &mut keyed)?;
            self.add(&mut root, e)?;
        }
        self.kept(&mut root, passthrough)?;
        self.ext_slot(&mut root, ext, |enc, o, el| enc.odf_doc(o, el, &mut keyed))?;
        for p in parts {
            let e = self.part(p, &mut keyed)?;
            self.add(&mut root, e)?;
        }
        if let Some(h) = vsdx {
            let e = self.vsdx_held(h, &mut keyed)?;
            self.add(&mut root, e)?;
        }
        for (page, name) in pages.iter().zip(Self::part_names(pages.iter().map(|p| p.id), "pages", "page")) {
            let mut e = self.elem(el::PAGE_REF);
            Self::put_u32(&mut e, at::ID, page.id, 0);
            self.put_str(&mut e, at::PART, &name)?;
            self.add(&mut root, e)?;
        }
        for (master, name) in masters.iter().zip(Self::part_names(masters.iter().map(|m| m.id), "masters", "master")) {
            let mut e = self.elem(el::MASTER_REF);
            Self::put_u32(&mut e, at::ID, master.id, 0);
            self.put_str(&mut e, at::PART, &name)?;
            self.add(&mut root, e)?;
        }
        self.finish_part(root)
    }

    /// What a newer build wrote on the document element and this build does not know, back in place, once the blobs are listed: the last thing a document
    /// part is given. Gives the items of the part so far.
    pub(crate) fn finish_document(&mut self, root: &mut El, doc: &Document) -> Result<u64, CodecError> {
        let native = doc.ext.get().map(|d| &d.native);
        self.count(native.map_or(0, |k| u64::try_from(k.raw.len()).unwrap_or(u64::MAX)))?;
        self.native_end(root, native)?;
        Ok(self.items)
    }

    /// The list of blobs, added to a document part whose items were `items`.
    pub(crate) fn add_blob_list(&mut self, document: &mut El, items: u64, bytes: u64) -> Result<(), CodecError> {
        self.items = items;
        self.bytes = bytes;
        let records = self.records.clone();
        for r in &records {
            let mut e = self.elem(el::BLOB);
            Self::put_u32(&mut e, at::ID, r.id.0, 0);
            self.put_req(&mut e, at::PATH, &r.path)?;
            self.put_req(&mut e, at::MEDIA_TYPE, &r.media_type)?;
            if r.size != 0 {
                e.attrs.push((at::SIZE.to_owned(), r.size.to_string()));
            }
            e.attrs.push((at::STAMP.to_owned(), r.stamp.text()));
            self.add(document, e)?;
        }
        Ok(())
    }

    fn props(&mut self, p: &DocProps, keyed: &mut super::ext::Keyed<'_>) -> Result<El, CodecError> {
        let DocProps {
            subject,
            creator,
            last_modified_by,
            keywords,
            description,
            category,
            company,
            manager,
            hyperlink_base,
            app_version,
            created,
            modified,
            last_printed,
            language,
            custom,
        } = p;
        let mut e = self.elem(el::PROPS);
        for (name, value) in [
            (at::SUBJECT, subject),
            (at::CREATOR, creator),
            (at::LAST_MODIFIED_BY, last_modified_by),
            (at::KEYWORDS, keywords),
            (at::DESCRIPTION, description),
            (at::CATEGORY, category),
            (at::COMPANY, company),
            (at::MANAGER, manager),
            (at::HYPERLINK_BASE, hyperlink_base),
            (at::APP_VERSION, app_version),
            (at::CREATED, created),
            (at::MODIFIED, modified),
            (at::LAST_PRINTED, last_printed),
            (at::LANGUAGE, language),
        ] {
            self.put_opt(&mut e, name, value.as_ref())?;
        }
        for c in custom {
            let c = self.custom_prop(c, keyed)?;
            self.add(&mut e, c)?;
        }
        Ok(e)
    }

    fn font(&mut self, f: &FaceName) -> Result<El, CodecError> {
        let FaceName { name_u, raw, ext } = f;
        let mut e = self.elem(el::FONT);
        self.put_req(&mut e, at::NAME_U, name_u)?;
        self.raw_attrs(&mut e, raw)?;
        self.ext_kept(&mut e, ext)?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    fn theme(&mut self, t: &Theme) -> Result<El, CodecError> {
        // What a theme holds besides its part name and bytes is derived from the bytes when a document is opened, and is never stored.
        let Theme {
            part,
            bytes,
            colors: _,
            slots: _,
            major_font: _,
            minor_font: _,
            color_scheme_id: _,
            font_scheme_id: _,
            effect_scheme_id: _,
            connector_scheme_id: _,
            theme_scheme_id: _,
            entries: _,
        } = t;
        let mut e = self.elem(el::THEME);
        self.put_req(&mut e, at::PART, part)?;
        let id = self.blob(bytes, THEME_MEDIA_TYPE)?;
        e.attrs.push((at::BLOB.to_owned(), id.0.to_string()));
        Ok(e)
    }

    pub(crate) fn style(&mut self, s: &Style) -> Result<El, CodecError> {
        let Style { id, name, name_u, sheet, raw, passthrough, ext } = s;
        let mut e = self.elem(el::STYLE);
        Self::put_u32(&mut e, at::ID, *id, 0);
        self.put_opt(&mut e, at::NAME, name.as_ref())?;
        self.put_opt(&mut e, at::NAME_U, name_u.as_ref())?;
        if !is_default_sheet(sheet) {
            let sheet = self.sheet(sheet)?;
            self.add(&mut e, sheet)?;
        }
        self.raw_attrs(&mut e, raw)?;
        self.kept(&mut e, passthrough)?;
        let mut keyed = Self::keyed_unknowns(ext.get().map(|d| &d.native));
        self.ext_slot(&mut e, ext, |enc, o, el| enc.odf_style(o, el, &mut keyed))?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    // ------------------------------------------------------------------------------------------------------------ pages and masters

    /// A page as the tree of its part, and the items of the part: [`Encoder::page_stream`] into a tree.
    pub(crate) fn page_part(&mut self, p: &Page) -> Result<(El, u64), CodecError> {
        let mut sink = TreeSink::default();
        let items = self.page_stream(p, None, &mut sink)?;
        Ok((sink.into_tree()?, items))
    }

    /// A master as the tree of its part, and the items of the part.
    pub(crate) fn master_part(&mut self, m: &Master) -> Result<(El, u64), CodecError> {
        let mut sink = TreeSink::default();
        let items = self.master_stream(m, None, &mut sink)?;
        Ok((sink.into_tree()?, items))
    }

    pub(super) fn connect(&mut self, c: &Connect) -> Result<El, CodecError> {
        let Connect { from_sheet, from_cell, from_part, to_sheet, to_cell, to_part, raw, ext } = c;
        let mut e = self.elem(el::CONNECT);
        Self::put_u32(&mut e, at::FROM_SHEET, *from_sheet, 0);
        self.put_opt(&mut e, at::FROM_CELL, from_cell.as_ref())?;
        Self::put_opt_u32(&mut e, at::FROM_PART, *from_part);
        Self::put_u32(&mut e, at::TO_SHEET, *to_sheet, 0);
        self.put_opt(&mut e, at::TO_CELL, to_cell.as_ref())?;
        Self::put_opt_u32(&mut e, at::TO_PART, *to_part);
        self.raw_attrs(&mut e, raw)?;
        self.ext_kept(&mut e, ext)?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    // ------------------------------------------------------------------------------------------------------------ shapes

    /// A shape and the shapes under it. `depth` is 1 for a shape of a page or master. The depth and the part's shape count are held to their caps
    /// before anything is made for the shape, and the recursion is one frame a level of at most 200.
    pub(crate) fn shape(&mut self, s: &Shape, depth: usize) -> Result<El, CodecError> {
        let Shape { id, name, name_u, kind, master, master_shape, del, sheet, text, data1, data2, data3, foreign, children, raw, passthrough, ext } =
            s;
        if depth > MAX_SHAPE_DEPTH {
            return Err(limit("levels of nested shapes", MAX_SHAPE_DEPTH));
        }
        self.shapes = self.shapes.saturating_add(1);
        if self.shapes > MAX_SHAPES_PER_PAGE {
            return Err(limit("shapes in a part", MAX_SHAPES_PER_PAGE));
        }
        let mut e = self.elem(el::SHAPE);
        Self::put_u32(&mut e, at::ID, *id, 0);
        self.put_opt(&mut e, at::NAME, name.as_ref())?;
        self.put_opt(&mut e, at::NAME_U, name_u.as_ref())?;
        match kind {
            ShapeType::Shape => e.attrs.push((at::TYPE.to_owned(), "Shape".to_owned())),
            ShapeType::Group => e.attrs.push((at::TYPE.to_owned(), "Group".to_owned())),
            ShapeType::Foreign => e.attrs.push((at::TYPE.to_owned(), "Foreign".to_owned())),
            ShapeType::Guide => e.attrs.push((at::TYPE.to_owned(), "Guide".to_owned())),
            ShapeType::Unspecified => {}
            ShapeType::Other(text) => self.put_str(&mut e, at::TYPE_OTHER, text)?,
        }
        Self::put_opt_u32(&mut e, at::MASTER, *master);
        Self::put_opt_u32(&mut e, at::MASTER_SHAPE, *master_shape);
        Self::put_flag(&mut e, at::DEL, *del);
        self.put_opt(&mut e, at::DATA1, data1.as_ref())?;
        self.put_opt(&mut e, at::DATA2, data2.as_ref())?;
        self.put_opt(&mut e, at::DATA3, data3.as_ref())?;
        if !is_default_sheet(sheet) {
            let sheet = self.sheet(sheet)?;
            self.add(&mut e, sheet)?;
        }
        if let Some(t) = text {
            let t = self.text(t)?;
            self.add(&mut e, t)?;
        }
        if let Some(f) = foreign {
            let f = self.foreign(f)?;
            self.add(&mut e, f)?;
        }
        self.raw_attrs(&mut e, raw)?;
        self.kept(&mut e, passthrough)?;
        let mut keyed = Self::keyed_unknowns(ext.get().map(|d| &d.native));
        self.ext_slot(&mut e, ext, |enc, o, el| enc.odf_shape(o, el, &mut keyed))?;
        for child in children {
            let c = self.shape(child, depth.saturating_add(1))?;
            self.add(&mut e, c)?;
        }
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    fn text(&mut self, t: &Text) -> Result<El, CodecError> {
        let Text { parts, ext } = t;
        let mut e = self.elem(el::TEXT);
        // Whether the last node written is a bare text node: a run of characters that follows one would read back as part of it.
        let mut after_text = false;
        for part in parts {
            let mut was_text = false;
            match part {
                TextPart::Chars(chars) => {
                    if !chars.is_empty() && !needs_base64(chars) && !after_text {
                        self.add_text(&mut e, chars.to_string())?;
                        was_text = true;
                    } else {
                        let c = self.text_element(el::CHARS, chars)?;
                        self.add(&mut e, c)?;
                    }
                }
                TextPart::Cp(ix) => self.marker(&mut e, el::CP, *ix)?,
                TextPart::Pp(ix) => self.marker(&mut e, el::PP, *ix)?,
                TextPart::Tp(ix) => self.marker(&mut e, el::TP, *ix)?,
                TextPart::Fld { ix, text } => {
                    let mut f = self.text_element(el::FLD, text)?;
                    if *ix != 0 {
                        f.attrs.insert(0, (at::IX.to_owned(), ix.to_string()));
                    }
                    self.add(&mut e, f)?;
                }
                TextPart::Raw(node) => {
                    let k = self.kept_wrapper(0, node)?;
                    self.add(&mut e, k)?;
                }
                TextPart::OdfRaw(node) => {
                    let mut w = self.elem(el::ODF_RAW);
                    let n = self.kept_node(node)?;
                    self.add(&mut w, n)?;
                    self.add(&mut e, w)?;
                }
                // What a newer build wrote inside the text, as the XML it was read as, among the parts where it stood.
                // An element as the XML it was; a text, a comment or an instruction in data form (`txt`, `comment`, `pi`), which a bare node would merge with
                // the characters beside it or lose.
                TextPart::NativeRaw(node) => match &**node {
                    RawNode::Element(_) => {
                        let n = self.literal(node)?;
                        e.children.push(n);
                    }
                    other => {
                        let n = self.kept_node(other)?;
                        self.add(&mut e, n)?;
                    }
                },
            }
            after_text = was_text;
        }
        // The text's slot is the last child. What a newer build wrote on the text element itself is its attributes, written on it; what it wrote among the
        // text's children is in the text's parts (`NativeRaw`), and what the model keeps of it beside those is inside the `ext` element, since a text's own
        // children are its parts.
        let mut keyed = Self::keyed_unknowns(ext.get().map(|d| &d.native));
        self.ext_slot_native_inside(&mut e, ext, |enc, o, el| enc.odf_text(o, el, &mut keyed))?;
        self.native_attrs_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    fn marker(&mut self, parent: &mut El, local: &str, ix: u32) -> Result<(), CodecError> {
        let mut m = self.elem(local);
        Self::put_u32(&mut m, at::IX, ix, 0);
        self.add(parent, m)
    }

    /// An element whose content is `chars`, in base64 with the flag when it has a character XML forbids. The content of an empty text is nothing.
    pub(super) fn text_element(&mut self, local: &str, chars: &str) -> Result<El, CodecError> {
        self.guard_bytes(chars.len())?;
        let mut e = self.elem(local);
        let content = if needs_base64(chars) {
            e.attrs.push((at::B64.to_owned(), "1".to_owned()));
            base64_encode(chars.as_bytes())
        } else {
            chars.to_owned()
        };
        if !content.is_empty() {
            self.add_text(&mut e, content)?;
        }
        Ok(e)
    }

    fn foreign(&mut self, f: &ForeignData) -> Result<El, CodecError> {
        let ForeignData { attrs, rel_id, picture, text, passthrough, ext } = f;
        let mut e = self.elem(el::FOREIGN);
        self.put_opt(&mut e, at::REL_ID, rel_id.as_ref())?;
        self.put_opt(&mut e, at::TEXT, text.as_ref())?;
        self.raw_attrs(&mut e, attrs)?;
        if let Some(m) = picture {
            let Media { part, content_type, bytes } = &**m;
            let mut p = self.elem(el::PICTURE);
            self.put_req(&mut p, at::PART, part)?;
            self.put_req(&mut p, at::TYPE, content_type)?;
            let id = self.blob(bytes, content_type)?;
            p.attrs.push((at::BLOB.to_owned(), id.0.to_string()));
            self.add(&mut e, p)?;
        }
        self.kept(&mut e, passthrough)?;
        self.ext_kept(&mut e, ext)?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    // ------------------------------------------------------------------------------------------------------------ sheets

    /// A sheet: its links, its cells, its sections, what it keeps. The caps of a sheet are held here as the reader holds them, so that what is made
    /// reads back.
    pub(crate) fn sheet(&mut self, sh: &Sheet) -> Result<El, CodecError> {
        let addr = self.units.class_of(std::ptr::from_ref(sh).addr());
        if let Some(name) = self.units.named.get(&addr) {
            return Ok(self.same(*name));
        }
        let def_name = self.name_unit(addr);
        let Sheet { links, cells, sections, raw, passthrough, ext } = sh;
        let Links { line, fill, text } = links;
        if cells.len() > MAX_CELLS_PER_CONTAINER {
            return Err(limit("cells in one place", MAX_CELLS_PER_CONTAINER));
        }
        if sections.len() > MAX_SECTIONS_PER_SHEET {
            return Err(limit("sections in a sheet", MAX_SECTIONS_PER_SHEET));
        }
        let mut e = self.elem(el::SHEET);
        Self::put_opt_u32(&mut e, at::LINE_STYLE, *line);
        Self::put_opt_u32(&mut e, at::FILL_STYLE, *fill);
        Self::put_opt_u32(&mut e, at::TEXT_STYLE, *text);
        for (name, cell) in cells {
            let c = self.cell(name.as_str(), cell)?;
            self.add(&mut e, c)?;
        }
        for section in sections {
            let s = self.section(section)?;
            self.add(&mut e, s)?;
        }
        self.raw_attrs(&mut e, raw)?;
        self.kept(&mut e, passthrough)?;
        self.ext_kept(&mut e, ext)?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        self.give_name(&mut e, def_name);
        Ok(e)
    }

    /// The `same` element that stands for the unit given `name`.
    fn same(&self, name: u32) -> El {
        let mut e = self.elem(el::SAME);
        e.attrs.push((at::OF.to_owned(), name.to_string()));
        e
    }

    /// The name a unit is given when it is written first, if the model holds it in more than one place of the part: the next number. A unit that one place holds has none.
    fn name_unit(&mut self, addr: usize) -> Option<u32> {
        if !self.units.held.contains_key(&addr) {
            return None;
        }
        self.units.given = self.units.given.saturating_add(1);
        self.units.named.insert(addr, self.units.given);
        Some(self.units.given)
    }

    /// Puts the name on the element of a unit that was given one, and the fingerprint of the unit as it is written: the stamp of what the unit says, with what a `same`
    /// inside it stands for, which a reader of 1.1 compares with the unit it reads (a program of 1.0 that edits the unit keeps the name and the fingerprint as they were).
    fn give_name(&mut self, e: &mut El, name: Option<u32>) {
        if let Some(name) = name {
            let text = name.to_string();
            let fps = &self.units.fps;
            let fp = super::unitfp::fingerprint(e, &|n| fps.get(n).copied());
            self.units.fps.insert(text.clone(), fp);
            e.attrs.push((at::DEF.to_owned(), text));
            e.attrs.push((at::FP.to_owned(), fp.text()));
        }
    }

    fn section(&mut self, s: &Section) -> Result<El, CodecError> {
        let addr = self.units.class_of(std::ptr::from_ref(s).addr());
        if let Some(name) = self.units.named.get(&addr) {
            return Ok(self.same(*name));
        }
        let def_name = self.name_unit(addr);
        let Section { name, ix, del, cells, rows, raw, passthrough, ext } = s;
        check_name(name.as_str())?;
        if cells.len() > MAX_CELLS_PER_CONTAINER {
            return Err(limit("cells in one place", MAX_CELLS_PER_CONTAINER));
        }
        if rows.len() > MAX_ROWS_PER_SECTION {
            return Err(limit("rows in a section", MAX_ROWS_PER_SECTION));
        }
        let mut e = self.elem(el::SECTION);
        self.put_req(&mut e, at::N, name.as_str())?;
        Self::put_opt_u32(&mut e, at::SECTION_IX, *ix);
        Self::put_flag(&mut e, at::SECTION_DEL, *del);
        for (name, cell) in cells {
            let c = self.cell(name.as_str(), cell)?;
            self.add(&mut e, c)?;
        }
        for row in rows {
            let r = self.row(row)?;
            self.add(&mut e, r)?;
        }
        self.raw_attrs(&mut e, raw)?;
        self.kept(&mut e, passthrough)?;
        self.ext_kept(&mut e, ext)?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        self.give_name(&mut e, def_name);
        Ok(e)
    }

    fn row(&mut self, r: &Row) -> Result<El, CodecError> {
        let Row { name, ix, local_name, row_type, del, cells, raw, passthrough, ext } = r;
        if let Some(name) = name {
            check_name(name)?;
        }
        if cells.len() > MAX_CELLS_PER_CONTAINER {
            return Err(limit("cells in one place", MAX_CELLS_PER_CONTAINER));
        }
        let mut e = self.elem(el::ROW);
        self.put_opt(&mut e, at::N, name.as_ref())?;
        Self::put_opt_u32(&mut e, at::SECTION_IX, *ix);
        self.put_opt(&mut e, at::LOCAL_NAME, local_name.as_ref())?;
        self.put_opt(&mut e, at::ROW_T, row_type.as_ref())?;
        Self::put_flag(&mut e, at::SECTION_DEL, *del);
        for (name, cell) in cells {
            let c = self.cell(name.as_str(), cell)?;
            self.add(&mut e, c)?;
        }
        self.raw_attrs(&mut e, raw)?;
        self.kept(&mut e, passthrough)?;
        let mut keyed = Self::keyed_unknowns(ext.get().map(|d| &d.native));
        self.ext_slot(&mut e, ext, |enc, o, el| enc.odf_row(o, el, &mut keyed))?;
        self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        Ok(e)
    }

    fn cell(&mut self, name: &str, c: &Cell) -> Result<El, CodecError> {
        let Cell { formula, value, unit, error, raw, extra } = c;
        check_name(name)?;
        let mut e = self.elem(el::CELL);
        self.put_str(&mut e, at::N, name)?;
        if let Some(v) = value {
            self.put_str(&mut e, at::V, v.text())?;
        }
        match formula {
            FormulaSlot::Absent => {}
            FormulaSlot::Inherit => self.put_str(&mut e, at::F, diacraft_doc::INHERIT_MARKER)?,
            FormulaSlot::NoFormula => self.put_str(&mut e, at::F, diacraft_doc::NO_FORMULA_MARKER)?,
            FormulaSlot::Local(f) => {
                // A formula whose text is a marker's would read back as the marker.
                let marker = &*f.source == diacraft_doc::INHERIT_MARKER || &*f.source == diacraft_doc::NO_FORMULA_MARKER;
                self.put_str(&mut e, if marker { at::FL } else { at::F }, &f.source)?;
            }
        }
        self.put_opt(&mut e, at::U, unit.as_ref())?;
        self.put_opt(&mut e, at::E, error.as_ref())?;
        self.raw_attrs(&mut e, raw)?;
        if let Some(extra) = extra {
            let CellExtra { text, passthrough, ext } = &**extra;
            // The `extra` element holds what the extra has of its own. One that is empty is still written, since an empty extra is not an absent one; one
            // that holds nothing but what a newer build wrote has no element, and that content is on the cell.
            let own = text.is_some() || !passthrough.is_empty() || ext.get().is_some_and(|d| d.odf != Kept::default() || !d.vsdx.0.is_empty());
            if own || ext.get().is_none() {
                let mut x = self.elem(el::EXTRA);
                self.put_opt(&mut x, at::TEXT, text.as_ref())?;
                self.kept(&mut x, passthrough)?;
                self.ext_kept(&mut x, ext)?;
                self.add(&mut e, x)?;
            }
            self.native_end(&mut e, ext.get().map(|d| &d.native))?;
        }
        Ok(e)
    }
}

/// The media type a theme part's blob is stored under.
const THEME_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// Whether a sheet is the empty one, which has no element: nothing in it, and nothing in its slot.
pub(super) fn is_default_sheet(sheet: &Sheet) -> bool {
    crate::build::sheet_is_empty(sheet) && sheet.ext.is_empty()
}

fn check_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(CodecError::malformed("document", "a cell, section or row name that is empty or too long"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------- the public functions

/// The parts of `doc`: the document, each page and each master, as trees, with every byte string put in `blobs`. A model past a cap, or a part the XML
/// reader would refuse, is [`CodecError::Limit`], and nothing is made of it.
///
/// **The trees are all held at once**, so they are held to [`OPEN_TREE_ITEMS`] items between them, as the tree of one part is: a drawing past that is refused in
/// words **while the part that passes it is made**, and is saved a part at a time ([`super::save_parts`]). A model that shares what is equal (a decode makes
/// one) has a sheet that many shapes hold, and each of them writes it in full: the items are what the model writes and not what it holds, and a model of a few
/// hundred megabytes is the parts of many gigabytes.
pub fn encode(doc: &Document, blobs: &mut dyn BlobSink) -> Result<Encoded, CodecError> {
    encode_capped(doc, blobs, held_at_once(MAX_PART_ITEMS, OPEN_PART_BYTES))
}

/// The caps of trees held at once: the items one tree may have, and the bytes one part may be, between them all; and `part_items` and `part_bytes` for each.
pub(super) fn held_at_once(part_items: u64, part_bytes: u64) -> Caps {
    Caps {
        total_items: OPEN_TREE_ITEMS,
        total_items_error: ("XML items in the parts held at once", OPEN_TREE_ITEMS),
        part_items,
        // Two parts' worth: the trees held at once are held to the items one tree may have, and their strings to what two parts may be, so that the part that
        // passes one part's bytes is refused as such, whatever the document part had.
        total_bytes: OPEN_PART_BYTES.saturating_mul(2),
        total_bytes_error: ("bytes in the parts held at once", OPEN_PART_BYTES.saturating_mul(2)),
        part_bytes,
    }
}

/// [`encode`], refusing as soon as the parts made so far have more items or bytes than `caps` allow between them (and saying what `caps` say when they do), and no
/// part of more than a part's own: so that a drawing that is too large is refused at the item or the byte that passes the figure, and not after the part that has
/// it, or after every part.
pub(super) fn encode_capped(doc: &Document, blobs: &mut dyn BlobSink, caps: Caps) -> Result<Encoded, CodecError> {
    let mut enc = Encoder::new(blobs);
    enc.check_document(doc)?;
    enc.cap_for(&caps, 0, 0);
    let (mut document, doc_items) = enc.document_part(doc)?;
    let doc_bytes = enc.part_bytes();
    let (mut total, mut held) = (doc_items, doc_bytes);
    let mut pages = Vec::with_capacity(doc.pages.len());
    for page in &doc.pages {
        enc.cap_for(&caps, total, held);
        let (e, items) = enc.page_part(page)?;
        total = total.saturating_add(items);
        held = held.saturating_add(enc.part_bytes());
        pages.push((page.id, e));
    }
    let mut masters = Vec::with_capacity(doc.masters.len());
    for master in &doc.masters {
        enc.cap_for(&caps, total, held);
        let (e, items) = enc.master_part(master)?;
        total = total.saturating_add(items);
        held = held.saturating_add(enc.part_bytes());
        masters.push((master.id, e));
    }
    // The list of blobs and the native attributes of the document element are counted from the document part's own items and bytes, under the cap it was made with.
    enc.cap_for(&caps, 0, 0);
    enc.add_blob_list(&mut document, doc_items, doc_bytes)?;
    enc.finish_document(&mut document, doc)?;
    Ok(Encoded { document, pages, masters })
}

/// One page as a part's tree.
pub fn encode_page(page: &Page, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    Encoder::new(blobs).page_part(page).map(|(e, _)| e)
}

/// One master as a part's tree.
pub fn encode_master(master: &Master, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    Encoder::new(blobs).master_part(master).map(|(e, _)| e)
}

/// One shape and the shapes under it, as an element that declares the namespace, for a carrier that holds it.
pub fn encode_shape(shape: &Shape, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    let mut enc = Encoder::new(blobs);
    enc.begin_part();
    let mut e = enc.shape(shape, 1)?;
    e.attrs.insert(0, (format!("xmlns:{PREFIX}"), URN.to_owned()));
    Ok(e)
}

/// One held value of an ODF object, as an element that declares the namespace. What its record kept unknown is not in it: that is in the slot of the object
/// that owns the record (NP7), which a fragment of the value alone has not.
pub fn encode_held(held: &diacraft_doc::Held, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    let mut enc = Encoder::new(blobs);
    enc.begin_part();
    let mut e = enc.held(held, &mut HashMap::new(), 0)?;
    e.attrs.insert(0, (format!("xmlns:{PREFIX}"), URN.to_owned()));
    Ok(e)
}

/// One style, as an element that declares the namespace.
pub fn encode_style(style: &Style, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    let mut enc = Encoder::new(blobs);
    enc.begin_part();
    let mut e = enc.style(style)?;
    e.attrs.insert(0, (format!("xmlns:{PREFIX}"), URN.to_owned()));
    Ok(e)
}

/// One sheet, as an element that declares the namespace.
pub fn encode_sheet(sheet: &Sheet, blobs: &mut dyn BlobSink) -> Result<El, CodecError> {
    let mut enc = Encoder::new(blobs);
    enc.begin_part();
    let mut e = enc.sheet(sheet)?;
    e.attrs.insert(0, (format!("xmlns:{PREFIX}"), URN.to_owned()));
    Ok(e)
}

const _: () = assert!(XML_DEPTH_CEILING >= 2 * MAX_SHAPE_DEPTH, "the shapes and a kept element at their caps must fit the depth ceiling");

#[cfg(test)]
mod tests {
    use diacraft_doc::{Page, Shape, ShapeList};

    use super::*;
    use crate::model::MemBlobs;

    /// A page of `n` shapes that each have a name: a few items each.
    fn page_of(n: u32) -> Page {
        let mut page = Page::new(1, "P");
        let shapes = (1..=n)
            .map(|i| {
                let mut s = Shape::new(i);
                s.name = Some(Arc::from("n"));
                Arc::new(s)
            })
            .collect();
        page.shapes = ShapeList::from_vec(shapes);
        page
    }

    fn items_of(page: &Page) -> u64 {
        let mut blobs = MemBlobs::new();
        Encoder::new(&mut blobs).page_part(page).map(|(_, items)| items).unwrap_or(0)
    }

    /// An encode of `doc` held to `max` items between the parts and `part` for each, with no cap of bytes that matters.
    fn items_caps(doc: &Document, max: u64, part: u64) -> Result<Encoded, CodecError> {
        let caps = Caps {
            total_items: max,
            total_items_error: ("items held at once", max),
            part_items: part,
            total_bytes: u64::MAX,
            total_bytes_error: ("bytes held at once", u64::MAX),
            part_bytes: u64::MAX,
        };
        encode_capped(doc, &mut MemBlobs::new(), caps)
    }

    #[test]
    fn the_trees_held_at_once_are_refused_while_the_part_that_passes_the_total_is_made_and_the_document_part_is_finished_under_its_own_cap() {
        // A total of items over every part, and a cap for one part. The cap of a part is what is left of the total, and the document part, whose
        // native attributes are counted when it is finished, is finished under the cap it was made with and not the room that is left for another part.
        let mut doc = Document::empty();
        let page = page_of(6);
        // As many native attributes on the document element as the page has items, so that the document part, finished, is as many items as the total allows and
        // more than the room that is left for a part after the last page.
        let native: Vec<(String, String)> = (0..items_of(&page)).map(|i| (format!("f{i}"), "1".to_owned())).collect();
        doc.pages.push(Arc::new(page));
        doc.ext.make_mut().native.raw = diacraft_doc::RawAttrs::from_pairs(native);
        let mut blobs = MemBlobs::new();
        let (document_items, page_items) = {
            let mut enc = Encoder::new(&mut blobs);
            let (_, d) = enc.document_part(&doc).unwrap();
            let (_, p) = enc.page_part(&doc.pages.get(0).unwrap().clone()).unwrap();
            (d, p)
        };
        let total = document_items + page_items;
        let held = |max: u64, part: u64| items_caps(&doc, max, part);
        // The total is enough, and one item fewer is refused at the page that passes it, in the words given and with the total as the figure.
        assert!(held(total, 1_000_000).is_ok(), "the total of the parts");
        assert_eq!(held(total - 1, 1_000_000).err(), Some(CodecError::Limit { what: "items held at once", max: total - 1 }));
        // A cap for one part refuses the part that passes it, whatever the total.
        assert_eq!(held(u64::MAX, page_items - 1).err(), Some(CodecError::Limit { what: "XML items in one part", max: page_items - 1 }));
        assert!(held(u64::MAX, page_items.max(document_items + page_items)).is_ok());
    }

    #[test]
    fn a_total_that_is_the_cap_of_one_part_says_the_part_when_the_document_part_passes_it() {
        // When what is left of the total is exactly what one part may have, the part is what is refused, and it is said so (the less is the total's words).
        let mut doc = Document::empty();
        let colors: Vec<diacraft_doc::ColorEntry> = (0..40).map(|ix| diacraft_doc::ColorEntry { ix, rgb: Arc::from("#000000") }).collect();
        doc.colors = Arc::new(colors);
        let out = items_caps(&doc, 60, 60);
        assert_eq!(out.err(), Some(CodecError::Limit { what: "XML items in one part", max: 60 }));
        let out = items_caps(&doc, 60, 61);
        assert_eq!(out.err(), Some(CodecError::Limit { what: "items held at once", max: 60 }), "a part may have more than is left of the total");
    }

    #[test]
    fn an_element_a_text_and_a_string_count_toward_the_bytes_a_part_may_have_and_the_part_is_refused_past_the_figure_and_not_at_it() {
        let words = |max: u64| CodecError::Limit { what: "bytes in one part as written", max };
        let made = |cap: u64| {
            let mut blobs = MemBlobs::new();
            let mut enc = Encoder::new(&mut blobs);
            enc.set_byte_cap(cap, "bytes in one part as written", cap);
            let mut parent = enc.elem("x");
            let mut child = enc.elem("y");
            child.attrs.push(("k".to_owned(), "v".repeat(60)));
            // The element's own bytes at the least: its name and the marks round it, and each attribute's name, value and the marks.
            let own = element_floor(&child);
            assert_eq!(own, (child.name.len() + 3 + 1 + 60 + 4) as u64);
            let added = enc.add(&mut parent, child);
            (own, added, enc.part_bytes())
        };
        let (own, added, counted) = made(u64::MAX);
        assert_eq!((added, counted), (Ok(()), own));
        // At the figure the element is added, and a byte under it is refused, in the words given and with the figure.
        assert_eq!(made(own).1, Ok(()));
        assert_eq!(made(own - 1).1, Err(words(own - 1)));
        // A text counts its bytes, and a string is guarded before it is copied.
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        enc.set_byte_cap(10, "bytes in one part as written", 10);
        let mut parent = enc.elem("x");
        assert_eq!(enc.add_text(&mut parent, "0123456789".to_owned()), Ok(()));
        assert_eq!(enc.add_text(&mut parent, "x".to_owned()), Err(words(10)));
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        enc.set_byte_cap(10, "bytes in one part as written", 10);
        let mut e = enc.elem("x");
        assert_eq!(enc.put_str(&mut e, "n", &"s".repeat(11)), Err(words(10)), "a string past the figure is refused before it is copied");
        assert!(e.attrs.is_empty(), "and nothing was made of it");
        assert_eq!(enc.put_str(&mut e, "n", &"s".repeat(10)), Ok(()));
    }

    #[test]
    fn the_bytes_of_the_parts_held_at_once_bind_when_what_is_left_of_the_total_is_less_than_a_part_may_be_and_a_part_cap_binds_otherwise() {
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page_of(6)));
        let mut second = page_of(6);
        second.id = 2;
        doc.pages.push(Arc::new(second));
        let caps = |total: u64, part: u64| Caps {
            total_items: u64::MAX,
            total_items_error: ("items held at once", u64::MAX),
            part_items: u64::MAX,
            total_bytes: total,
            total_bytes_error: ("bytes held at once", total),
            part_bytes: part,
        };
        let held = |total: u64, part: u64| encode_capped(&doc, &mut MemBlobs::new(), caps(total, part)).err();
        // The least total that holds every part (the part's own cap not in the way): one byte less is refused as the total, with the figure given.
        let (mut lo, mut hi) = (0u64, 1u64 << 22);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if held(mid, u64::MAX).is_none() { hi = mid } else { lo = mid + 1 }
        }
        let total = lo;
        assert!(total > 0 && held(total, u64::MAX).is_none());
        assert_eq!(held(total - 1, u64::MAX), Some(CodecError::Limit { what: "bytes held at once", max: total - 1 }));
        // The bytes of the document part and of a page, as they are counted.
        let (document, page) = {
            let mut blobs = MemBlobs::new();
            let mut enc = Encoder::new(&mut blobs);
            enc.document_part(&doc).unwrap();
            let document = enc.part_bytes();
            enc.page_part(&doc.pages.get(0).unwrap().clone()).unwrap();
            (document, enc.part_bytes())
        };
        // A part that may be no more than a page less one byte, in a total that has exactly that much room for the first page: it is the part's cap that refuses it,
        // said as a part's, and not the total.
        let part = page - 1;
        assert_eq!(held(document + part, part), Some(CodecError::Limit { what: "bytes in one part as written", max: part }));
        // And a part that may be as much as a page, in a total with room for less, is refused as the total.
        assert_eq!(held(document + page - 1, page), Some(CodecError::Limit { what: "bytes held at once", max: document + page - 1 }));
    }

    #[test]
    fn a_part_is_refused_past_the_item_cap_and_not_at_it() {
        let page = page_of(20);
        let items = items_of(&page);
        assert!(items > 40, "{items}");
        // Every cap below what the part holds refuses it, whatever the steps the items are counted in, and the cap at what it holds does not.
        for cap in 1..=items {
            let mut blobs = MemBlobs::new();
            let mut enc = Encoder::new(&mut blobs);
            enc.set_cap(cap, "XML items in one part", cap);
            let out = enc.page_part(&page);
            if cap < items {
                assert!(matches!(out, Err(CodecError::Limit { what: "XML items in one part", max }) if max == cap), "cap {cap} of {items}");
            } else {
                assert!(out.is_ok(), "cap {cap}");
            }
        }
    }

    #[test]
    fn each_part_starts_its_count_and_its_shapes_again() {
        let page = page_of(20);
        let items = items_of(&page);
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        enc.cap = items;
        for _ in 0..3 {
            assert_eq!(enc.page_part(&page).map(|(_, n)| n).ok(), Some(items));
        }
        // The shapes of a part are counted from none: a part that comes after one that used the cap up is not refused.
        enc.shapes = MAX_SHAPES_PER_PAGE;
        enc.cap = u64::MAX;
        assert!(enc.page_part(&page).is_ok());
    }

    #[test]
    fn a_part_holds_the_most_shapes_the_model_does_and_no_more() {
        let one = Shape::new(1);
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        enc.begin_part();
        enc.shapes = MAX_SHAPES_PER_PAGE - 1;
        assert!(enc.shape(&one, 1).is_ok(), "the last shape the model allows");
        assert!(matches!(enc.shape(&one, 1), Err(CodecError::Limit { what: "shapes in a part", .. })));
        assert!(matches!(enc.shape(&one, 1), Err(CodecError::Limit { what: "shapes in a part", .. })));
        // And the depth: the 200th level is the last.
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        assert!(enc.shape(&one, MAX_SHAPE_DEPTH).is_ok());
        assert!(matches!(enc.shape(&one, MAX_SHAPE_DEPTH + 1), Err(CodecError::Limit { what: "levels of nested shapes", .. })));
    }

    /// A master of `n` shapes and a document of a master and a page: each part is counted from its own start, whatever came before it in the encoder.
    #[test]
    fn a_master_and_a_document_part_start_their_count_afresh() {
        let page = page_of(20);
        let master = Master { shapes: page.shapes.clone(), ..Master::default() };
        let mut blobs = MemBlobs::new();
        let master_items = Encoder::new(&mut blobs).master_part(&master).map(|(_, n)| n).unwrap_or(0);
        assert!(master_items > 40, "{master_items}");
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page.clone()));
        let doc_items = Encoder::new(&mut blobs).document_part(&doc).map(|(_, n)| n).unwrap_or(0);
        assert!(doc_items > 1, "{doc_items}");
        // A part that comes after one that used a good deal of the cap, with a cap of exactly what the part holds: not refused.
        let mut enc = Encoder::new(&mut blobs);
        enc.cap = u64::MAX;
        assert!(enc.page_part(&page).is_ok());
        enc.cap = master_items;
        assert_eq!(enc.master_part(&master).map(|(_, n)| n).ok(), Some(master_items));
        let mut enc = Encoder::new(&mut blobs);
        enc.cap = u64::MAX;
        assert!(enc.page_part(&page).is_ok());
        enc.cap = doc_items;
        assert_eq!(enc.document_part(&doc).map(|(_, n)| n).ok(), Some(doc_items));
    }

    /// What a newer build wrote on the document element is written back as items: a text, a comment and an instruction one each, and an element one and one
    /// for each attribute, with what is in it.
    #[test]
    fn each_kept_node_the_native_slot_writes_back_is_an_item_of_the_part() {
        use diacraft_doc::{Passthrough, Placed, RawElement, RawNode};
        let element = |attrs: &[(&str, &str)], children: Vec<RawNode>| {
            RawNode::Element(RawElement { name: Arc::from("q:e"), attrs: diacraft_doc::RawAttrs::from_pairs(attrs.iter().copied()), children })
        };
        let nodes = [
            (RawNode::Text(Arc::from("t")), 1),
            (RawNode::Comment(Arc::from("c")), 1),
            (RawNode::Pi { target: Arc::from("p"), data: Arc::from("d") }, 1),
            (element(&[], Vec::new()), 1),
            (element(&[("xmlns:q", "urn:q"), ("a", "1")], Vec::new()), 3),
            (element(&[("a", "1")], vec![RawNode::Text(Arc::from("t")), element(&[], Vec::new())]), 4),
        ];
        for (node, cost) in nodes {
            let mut doc = Document::empty();
            doc.ext.make_mut().native.kept = Passthrough::from_nodes(vec![Placed { after: 0, node }]);
            for (room, fits) in [(cost - 1, false), (cost, true)] {
                let mut blobs = MemBlobs::new();
                let mut enc = Encoder::new(&mut blobs);
                let (mut root, items) = enc.document_part(&doc).expect("a document part");
                enc.cap = items + room;
                assert_eq!(enc.finish_document(&mut root, &doc).is_ok(), fits, "{doc:?} with room for {room}");
            }
        }
    }

    #[test]
    fn the_native_attributes_of_the_document_element_are_items_of_its_part_and_the_cap_holds_them() {
        let mut doc = Document::empty();
        doc.ext.make_mut().native.raw = diacraft_doc::RawAttrs::from_pairs([("a", "1"), ("b", "2"), ("c", "3")]);
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        let (mut root, items) = enc.document_part(&doc).expect("a document part");
        // Three attributes more than the part held: the cap one short refuses them, the cap at them does not.
        enc.cap = items + 2;
        assert!(matches!(enc.finish_document(&mut root, &doc), Err(CodecError::Limit { what: "XML items in one part", .. })));
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        let (mut root, items) = enc.document_part(&doc).expect("a document part");
        enc.cap = items + 3;
        assert_eq!(enc.finish_document(&mut root, &doc).ok(), Some(items + 3));
    }
}
