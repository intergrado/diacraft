//! Format tags, the extension slot, and the package-level data the model holds beside its cells (the native-format design's §3).
//!
//! **Why this exists.** The model is the only record of a drawing that came from a foreign file, so everything that file holds and the model does not
//! rebuild from its own fields needs a home here, as data: each element, attribute, part and relationship. A construct goes to the first home that fits
//! (a cell, a held value beside stand-in cells, a typed slot, tagged kept data), and **each format's data is tagged** and sits beside the cells without
//! ever overriding them. These are plain data: no parsing, no I/O.
//!
//! **The tags.** The existing `raw` and `passthrough` fields (and [`crate::TextPart::Raw`]) stay the VSDX format's. ODF's kept markup and typed held values
//! are in the [`odf`](crate::odf) slot of each object, and what a newer DiaCraft wrote and this build does not know is in the `native` slot, which no writer
//! of either format emits. A part of the package is tagged by [`Format`].
//!
//! **One slot per object.** [`Ext`] is that one slot: a single pointer, null when nothing is held, so an object that carries none pays 8 bytes and a
//! clone takes one reference count. Its equality is by hand: **a slot holding only defaults equals an absent one**, so a reader that took the slot for a
//! write and added nothing cannot break `decode(encode(m)) == m`.
//!
//! **Which reader fills what, and which writer reads it** (§3.4): the VSDX reader fills [`Format::Vsdx`] parts, [`VsdxHeld`], the `vsdx` side of each
//! slot and the part names; the `.odg` reader fills [`Format::Odf`] parts and the `odf` data of each slot (`odf.rs`); fills [`Format::Native`]
//! parts and the `native` side. Each model writer writes its own format's data natively and sends the other's to its carrier.

use std::sync::Arc;

use crate::raw::{Passthrough, RawAttrs, RawElement};

/// The most parts a document holds in [`crate::Document::parts`]: 50,000, the package cap on entries. A project rule (the native format's global
/// constraints); checked at the door by [`crate::Document::check_limits`].
pub const MAX_PARTS: usize = 50_000;

/// Which format's rules a part, a record or a piece of kept data belongs to. A writer emits its own format's and sends the other's to its carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// A VSDX package's. Filled by the VSDX reader; written by the VSDX writer, carried by the `.odg` writer.
    Vsdx,
    /// An `.odg` package's. Filled by the `.odg` reader; written by the `.odg` writer, carried by the VSDX writer.
    Odf,
    /// An unknown-role part of a newer `.diacraft` (placement note 11, Call 19). Filled by and written back by it; no foreign
    /// writer emits it, and the carriers route it as DiaCraft data like other content the target cannot say, so it survives every Save As.
    Native,
}

/// Attributes and child nodes the model has no field for: the pair [`RawAttrs`] and [`Passthrough`] that `raw` and `passthrough` fields hold, kept together
/// under one tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// The attributes, in file order.
    pub raw: RawAttrs,
    /// The child nodes, each at its place among its siblings.
    pub kept: Passthrough,
}

/// What the VSDX reader found on an object that the model has no field for, by the element's local name (and `/<IX>` for a colour entry). Filled by
/// the VSDX reader, written back by the VSDX writer, carried by the `.odg` writer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VsdxRest(pub Vec<(Arc<str>, Kept)>);

/// What the slot of an object holds: its ODF data (`T`, a type of `odf.rs`), the VSDX elements it had no slot for, and unknown `.diacraft` content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtData<T> {
    /// The object's ODF data: filled by the `.odg` reader, written by the `.odg` writer.
    pub odf: T,
    /// The VSDX elements the object had no slot for: filled by the VSDX reader, written by the VSDX writer.
    pub vsdx: VsdxRest,
    /// Unknown content of a newer `.diacraft`: filled by the decoder, never written into either foreign format.
    pub native: Kept,
}

/// The extension slot of an object: one pointer, null when it holds nothing. See the module documentation.
///
/// `PartialEq` is by hand, and the one rule is: **the slot is equal to what it holds when that is not all defaults, and to an absent one when it is**.
/// [`Ext::get`], [`Ext::is_empty`] and [`Ext::tidy`] follow the same rule.
#[derive(Clone, Debug)]
pub struct Ext<T>(Option<Arc<ExtData<T>>>);

impl<T> Default for Ext<T> {
    fn default() -> Ext<T> {
        Ext(None)
    }
}

impl<T: Default + Clone + PartialEq> Ext<T> {
    /// What the slot holds, or `None` when it holds nothing but defaults (an untidied empty record is nothing). The one place that rule is decided.
    fn live(&self) -> Option<&Arc<ExtData<T>>> {
        self.0.as_ref().filter(|data| ***data != ExtData::default())
    }

    /// What the slot holds, or `None` when it is empty.
    pub fn get(&self) -> Option<&ExtData<T>> {
        self.live().map(|data| &**data)
    }

    /// The data to write to, made if there is none; copied first if a clone of the object shares it. A reader that calls this and adds nothing leaves a
    /// slot that equals an empty one; [`Ext::tidy`] returns it to none.
    pub fn make_mut(&mut self) -> &mut ExtData<T> {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(ExtData::default())))
    }

    /// Back to none when every field is default, so that an object that took the slot for a write and wrote nothing costs 8 bytes again.
    pub fn tidy(&mut self) {
        if self.live().is_none() {
            self.0 = None;
        }
    }

    /// Whether the slot holds nothing (no record, or a record of defaults).
    pub fn is_empty(&self) -> bool {
        self.live().is_none()
    }
}

impl<T: Default + Clone + PartialEq> PartialEq for Ext<T> {
    fn eq(&self, other: &Ext<T>) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            // Two records are the same record, or are equal as data: if they differ, at most one of them is all defaults, so they are not equal.
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || **a == **b,
            (Some(_), None) => self.live().is_none(),
            (None, Some(_)) => other.live().is_none(),
        }
    }
}

impl<T: Default + Clone + Eq> Eq for Ext<T> {}

/// What a part of the package is for. A writer that knows the role may treat the part specially (a macro project only into a macro-enabled type, a
/// thumbnail made again when the drawing changed); one that does not carries the bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartRole {
    /// The package's thumbnail picture.
    Thumbnail,
    /// A macro project, kept as bytes: nothing parses or runs it.
    MacroProject,
    /// A macro project's signature.
    MacroSignature,
    /// A macro project's data part.
    MacroData,
    /// An embedded object's package or file.
    Embedding,
    /// A subdocument of an embedded object, as its own package entry.
    ObjectFile,
    /// The picture standing for an embedded object.
    ObjectReplacement,
    /// A digital signature.
    Signature,
    /// A custom XML item or its properties part.
    CustomXml,
    /// An embedded font.
    Font,
    /// Any other part: kept as bytes.
    Other,
}

/// One relationship of the package: where it comes from, its ID, type, target and whether the target is outside the package. Filled by the VSDX reader
/// (a package's and its parts' relationships) and the `.odg` reader (a manifest's rows), written back by the writer of the same format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rel {
    /// The part the relationship comes from (the package itself is `/`).
    pub source: Arc<str>,
    /// Its ID (`rId1`), compared ASCII case-insensitively as OPC compares them.
    pub id: Arc<str>,
    /// Its type (a URI), verbatim.
    pub kind: Arc<str>,
    /// Its target, verbatim.
    pub target: Arc<str>,
    /// `TargetMode="External"`.
    pub external: bool,
}

/// One part of the package that no model field reads, as it came: `Document.parts` holds one for each. Filled by the readers (the VSDX reader's unheld
/// parts, the `.odg` reader's other package entries, the unknown roles); written back by the writer of the same [`Format`], and carried by
/// the other format's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Whose rules the part belongs to.
    pub format: Format,
    /// The part's name in its package, verbatim.
    pub name: Arc<str>,
    /// Its media type (a content type, or a manifest's `manifest:media-type`).
    pub media_type: Arc<str>,
    /// What it is for.
    pub role: PartRole,
    /// The other attributes of the manifest row that named it, verbatim (an `.odg`'s; none for a VSDX part).
    pub manifest: RawAttrs,
    /// The relationships from this part.
    pub rels: Arc<Vec<Rel>>,
    /// Its bytes, as stored once inflated; shared by every clone.
    pub bytes: Arc<[u8]>,
}

/// One row of `[Content_Types].xml`. Filled by the VSDX reader, written back by the VSDX writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentTypeRow {
    /// A default for an extension.
    Default {
        /// The extension, without its dot.
        extension: Arc<str>,
        /// The content type.
        content_type: Arc<str>,
    },
    /// An override for one part.
    Override {
        /// The part's name.
        part: Arc<str>,
        /// The content type.
        content_type: Arc<str>,
    },
}

/// What a VSDX package holds at the document level that the model has no other field for. Filled by the VSDX reader, written back by the VSDX writer,
/// carried by the `.odg` writer. `None` on a document that came from no VSDX file and whose window settings nothing set; **one that holds `windows` alone**
/// is a drawing of no VSDX file whose window settings were set (the model holds them as the windows part holds them), and
/// holds nothing of a package ([`VsdxHeld::holds_windows_alone`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VsdxHeld {
    /// The relationships the writer would not make itself (the package's, the document's, each page's and master's), by source part.
    pub rels: Vec<Rel>,
    /// The content-type rows beyond what the parts need.
    pub content_types: Vec<ContentTypeRow>,
    /// The elements of the properties parts (`core`, `app`) that no field of the properties reads, by element name.
    pub props_rest: Vec<(Arc<str>, Kept)>,
    /// `windows.xml`, whole: the window reads its settings from here ([`crate::WindowFlags`]), and the command that sets them writes them here.
    pub windows: Option<Arc<RawElement>>,
}

impl VsdxHeld {
    /// Whether this holds a windows part and nothing else: no relationship row, no content-type row and no element of a property part. **A model read from a package
    /// never does**: no field says the package's own rows (to the document part and the property parts), so the reader holds them. One that holds a windows part alone
    /// came from no package: its window settings were set in DiaCraft, and the VSDX model writer writes it as a drawing made new, with that windows part.
    pub fn holds_windows_alone(&self) -> bool {
        self.windows.is_some() && self.rels.is_empty() && self.content_types.is_empty() && self.props_rest.is_empty()
    }
}

/// A user-defined property of the document: VSDX's `custom.xml` and ODF's `meta:user-defined`, which share this shape. Filled by either reader, written by
/// the writer of the same format; the other format's writer writes it where it has a place for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomProp {
    /// The format the property came from.
    pub format: Format,
    /// Its name.
    pub name: Arc<str>,
    /// Its type, as the format spells it (`lpwstr`, `string`).
    pub value_type: Arc<str>,
    /// Its value, as text.
    pub value: Arc<str>,
    /// Its other attributes, verbatim.
    pub raw: RawAttrs,
}
