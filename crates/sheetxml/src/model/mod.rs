//! The whole-model codec: a [`Document`] as XML in DiaCraft's namespace, and back, so that `decode(encode(m)) == m` for every model the engine can
//! hold (the design's section 2).
//!
//! **What it makes.** [`encode`] gives the parts a `.diacraft` package stores as trees: the document part, one part for each page and one for
//! each master ([`Encoded`]). A fragment of the model has its own pair of functions ([`encode_shape`] and [`decode_shape`], [`encode_page`] and
//! [`decode_page`], and the rest), for the carriers inside a VSDX file or an `.odg`, and for `crates/odg/src/ext/body.rs`, whose shape layer this
//! module's was ported from. A picture or a theme or any other byte string the model holds is not in the XML: it goes to a [`BlobSink`], which
//! numbers it and stores it once, and the element names the [`BlobId`] (the design's 1.5).
//!
//! **The vocabulary** is [`vocab`]: one table of every element and attribute, with the default the decoder assumes when it is absent. Elements are
//! in `urn:app.diacraft:extension` under the prefix `diacraft`; a reader matches the namespace and never the prefix.
//!
//! **Exact.** What the codec reads back is what it was given, by value. The rules that make it so:
//!
//! * a typed field is an attribute, named after the field; a field equal to its stated default is not written;
//! * what the model has no field for ([`RawAttrs`], [`Passthrough`]) is data: `attr` and `kept` children, and a kept element is an `el` element
//!   holding its name as an attribute, never literal XML, so that its prefixes cannot clash with the codec's and a kept attribute cannot clash with
//!   a typed one;
//! * a string that holds a character XML 1.0 forbids is written in base64 (an attribute takes the suffix `-b64` on its name, a `txt`, `comment`,
//!   `pi` or `t` element a `b64="1"` flag), and a string longer than [`escape::ATTR_MAX_BYTES`] goes in a child element `s` of its own: the XML
//!   reader caps an attribute by the bytes as written, which escaping makes six times the value;
//! * text keeps every blank, and every run of characters is an element of its own where a bare text node would merge with its neighbour or vanish;
//! * a number is its shortest decimal text, `-0.0` is kept, and a value that is not finite is a named token ([`escape::f64_text`]);
//! * an absent value and an empty one stay apart, and so do an absent `extra` and an empty one;
//! * each object that has an extension slot ([`diacraft_doc::Ext`]) has one `ext` child for what the slot holds of ODF and of VSDX, and writes nothing
//!   for a slot that holds only defaults;
//! * **what a newer build wrote** (an attribute or an element this build's vocabulary does not have, or that is in another namespace) is kept in the
//!   object's `native` slot, and written back where it was, as the XML it was read as, with the namespace declarations its names use, so that the same
//!   or a newer build reads it as its own; a record that has no slot (a colour entry, a part, a relationship, a content-type row, a custom property, a
//!   definition, a held value) keeps its unknown content in its owner's native slot under `kind[key]` (NP7). **The key** is the record's identity where it
//!   has one of its own (a colour's index, a part's name, a relationship's source and ID, a shape's ID, a row's name or `IX`): never its position. Within a
//!   list that has none, **the ordinal within the owner's list is the key** (Call 24): the held values of one group (`held[fill.0]`, `held[fill.1]`), the
//!   paragraphs of a text (`paragraph[p]`), the runs of a paragraph (`run[p.r]`) and the held values of a style (`held-style[p.group.n]`), so that every
//!   value keeps what a newer build wrote on it. The place of an unknown element is the number
//!   of known children that came before it, and a write puts it after as many as it writes: a document read from a file that held a known child the codec
//!   does not write again (a theme whose bytes are lost, a second `sheet`, a default `props`) is written with the place closed up, and from that write on
//!   it is exact; the bytes of a second write are the first's.
//!
//! **Never crash.** A decode is charged to a [`Budget`] before it allocates: one item for each element and attribute, text by its bytes, a formula by its
//! nodes, a picture by the size its blob record states, each shape against the shape cap, and the depth of shapes before the shape is built. **What an object
//! costs is what it costs where it stands**: a map of cells allocates a leaf of eleven slots at its first cell, and a list of kept nodes room
//! for four at its first, so a place that holds one thing costs what a place of four does, and the decoder charges the nodes of each map and the room of each
//! list as it makes them ([`cost`]); the frame of an element that declares a namespace is charged before it is made, and an element declares at most
//! [`ns::MAX_PREFIXES_PER_ELEMENT`] prefixes. An encode counts the items of each part and refuses what the XML reader would refuse, so that no file is written
//! that cannot be opened. The trees the codec makes keep to the container's depth ceiling, and a kept element is copied with a stack of its own.
//!
//! **One budget for an open and for a save.** [`Budget::for_open`] is what a decode of a package may spend, and a save asks the same first: each part as written
//! within the part cap and the package cap ([`written_len`] counts the bytes the writer makes, without making them), and the parts decoded as an open decodes
//! them, within the same budget ([`check_decodes`], each page let go as it is read). A save that passes is a package an open takes. [`save_parts`] holds one
//! part's tree at a time and [`encode_for_save`] every part's, held to what one tree may have ([`OPEN_TREE_ITEMS`]); a decode asks its [`PartSource`] for one
//! part at a time, and lets the tree go before it asks for the next ([`decode_from`]).
//!
//! **Equal sheets and sections are one allocation** (`share`). A decode keeps one copy of each sheet and each section it reads and gives every later equal one that
//! copy, charged as a reference; a write makes its own copy of what it changes, as the model always has. **What an edit unshares is charged by the edit, not by the
//! open**: a drawing of a million shapes that hold one sheet is a sheet and a million pointers until each shape is edited, and the open charges what it holds.
//! The engine's edit path refuses an edit that would make the model held pass [`OPEN_MODEL_BYTES`], in words (`Session::edit_if`, counting with
//! [`held_bytes_until`]), and a save counts the model it holds, each allocation once, before it makes a tree. **A file says a shared sheet or section once**
//! (`same`, the vocabulary's 3.19): the encoder writes a unit that the model holds in more than one place of a part in full the first time, with a name, and as a
//! `same` that names it after, so that the file is what the model holds and the tree of a part is no more than that.
//!
//! **Capacity** (measured on beast, 2026-11-21; `tests/it/model_capacity.rs`, and the readers' own drawings in `crates/odg/tests/it/native_capacity.rs` and
//! `crates/vsdx/tests/it/native_capacity.rs`). The pools, the file and the one tree being read fit under 3 GB together (the constants say so at compile time,
//! [`OPEN_WORST_BYTES`]; a save's, [`SAVE_WORST_BYTES`], and the items of its trees come down as the model it holds grows, [`save_tree_items`]): **a tree of 2 million items, a model of
//! 960 MiB (with the document part's tree beyond 256,000 items in it), blobs of 384 MiB, a package of 512 MiB.** A page or a master is read and written **as a stream of the children
//! of its root** (`PartSource::page_bytes`, `save_parts_bytes`): what bounds it is the bytes of the part (128 MiB), a child (a shape and what it holds) of 4 million items (the first 2 million in the pool of one tree, and what is beyond held back from the model's pool while the child is held), and the format's own limit of 12,000,000 items for a part in all
//! ([`FORMAT_PART_ITEMS`], the spec's 6.1, which a writer does not pass and a reader refuses: a page of `.odg` rectangles that each have a sheet of their own is 121 items a shape, and the 76,000
//! of them that the reader opens are a page of 9.2 million); a part held whole is at most 2 million items. A file says a shared sheet or section **once** (`same`), by value as well as by address, and a decode shares what is
//! equal, so a drawing of shapes that hold one path holds one path. What an edit unshares is charged by the edit (the engine's `Session::edit_if`, which refuses an edit that would leave
//! more than [`EDIT_MODEL_BYTES`], 1.5 GiB), and an open counts what a file holds. **What binds is the pool of the model** (960 MiB, 1,344 MiB when the blobs are few), or the items a part may have: the figures are in `docs/gaps.md`, F31.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, XmlWriteError};

mod blobs;
pub(crate) mod byvalue;
mod comments;
pub mod cost;
mod decode;
mod encode;
pub mod escape;
mod ext;
mod kept;
mod save;
mod share;
mod stamp;
mod stream;
mod unitfp;
pub mod vocab;
mod weigh;

pub use blobs::{MemBlobs, blob_extension, blob_path};
pub use decode::{
    DEF_ENTRY_BYTES, check_decodes, check_decodes_from, decode, decode_from, decode_held, decode_master, decode_page, decode_shape, decode_sheet,
    decode_style, part_lists,
};
pub use encode::{encode, encode_held, encode_master, encode_page, encode_shape, encode_sheet, encode_style};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use save::expected_bytes;
pub use save::{
    PartRole, SaveThreads, SavedBytes, SavedOwned, SavedPart, encode_for_save, grown_bytes_until, held_bytes, held_bytes_until, preflight,
    save_child_items, save_parts, save_parts_bytes, save_parts_owned, save_tree_items, written_len,
};
pub use share::{BUCKET_MAX, SECTION_STREAM_BYTES, SHARE_ENTRY_BYTES, SHARED_REF_BYTES, SHEET_STREAM_BYTES};
#[cfg(kani)]
pub(crate) use share::{chunk_for, stream_in};
pub use stamp::{Digest, Stamp, StampCost, content_stamp, content_stamp_counted, stamp_of_bytes};
pub use weigh::{Added, Freed, Released, allocations_walked, grown, held_bytes_of, whole_walks};

/// The namespace every element of the codec is in (`diacraft_doc::DIACRAFT_URN`).
pub const URN: &str = diacraft_doc::DIACRAFT_URN;
/// The prefix the codec writes the namespace under. A reader never relies on it.
pub const PREFIX: &str = "diacraft";

/// The number of a blob: the order of its first use, from 0 (the document, then the pages, then the masters).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobId(pub u32);

/// Where the encoder puts the byte strings of the model: it stores each distinct one once and numbers it. The numbering is the order of first
/// use, so that the same model always gets the same numbers.
pub trait BlobSink {
    /// Stores `bytes` (of media type `media_type`) unless an equal byte string is held already, and gives its number.
    fn put(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError>;
}

/// Where the decoder finds the byte strings of the model.
pub trait BlobSource {
    /// The bytes of blob `id`, or `None` when they are lost (a damaged or missing entry): a lost blob is a note and a picture that is not shown
    /// (the design's 1.5), never a refusal.
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>>;
}

/// Where a decode finds the trees of the parts a document part lists, **one at a time**: it asks for a part, reads it, lets the tree go, and then asks for the
/// next, so that the trees it holds at once are the document's and one part's, whatever the number of pages. A caller that reads a package gives
/// each part by parsing it when it is asked for, at limits whose depth and items it holds one tree to ([`open_limits`]) and with the part's bytes let go once the tree
/// is made, and one that already holds the trees ([`decode`]) gives references to them.
pub trait PartSource {
    /// What a part is when it is given: the tree itself, or a reference to it.
    type Tree: std::borrow::Borrow<El>;

    /// The tree of the page part listed at `index` (counted from 0, in the order of the document part's list) under `name`. Asked once for each page, in order.
    fn page(&mut self, index: usize, name: &str) -> Result<Self::Tree, CodecError>;

    /// The tree of the master part listed at `index` under `name`. Asked once for each master, after every page.
    fn master(&mut self, index: usize, name: &str) -> Result<Self::Tree, CodecError>;

    /// How many pages and masters this source holds, when it holds them all: the lists are then checked against it before anything is read. `None` for a source
    /// that reads a part when it is asked.
    fn counts(&self) -> Option<(usize, usize)> {
        None
    }

    /// The bytes of the page part at `index`, **when the source holds the part as bytes and would have the decoder read it as a stream**: the decoder reads the root's
    /// start tag and then each child of the root, whole, one at a time, so that what it holds of the part is one child's tree and the part's bytes, and a page of any
    /// number of shapes is read (a page held as a tree is at most [`OPEN_TREE_ITEMS`] items). `Ok(None)`, the default, has the decoder ask for the tree
    /// ([`PartSource::page`]). A part's bytes are held to [`OPEN_PART_BYTES`], and what a stream refuses is said in the codec's words and not as the XML reader's classes.
    /// The bytes are let go once the part is read.
    fn page_bytes(&mut self, index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        let _ = (index, name);
        Ok(None)
    }

    /// [`PartSource::page_bytes`] for the master part at `index`.
    fn master_bytes(&mut self, index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        let _ = (index, name);
        Ok(None)
    }
}

/// What a decode may still spend: each charge is made before what it pays for is stored, and a charge that does not fit is refused as
/// [`CodecError::Limit`]. The fields are what is left.
///
/// **What an item and a byte are.** An item is an element, an attribute, a text, comment or instruction node, or a note. A byte is a byte of the model in
/// memory: the text it keeps (a value, a name, a kept text), the objects each element builds (a shape, a cell, a row: [`cost::element_bytes`], measured), and a
/// formula at [`FORMULA_NODE_BYTES`] for each node of its parse, the figure the formula crate gives (a tree costs 128 bytes a node). An element that costs one
/// item can build hundreds of bytes (an empty `<shape/>` is 8 bytes of file and 338 of model), which is why an item is not what bounds memory. A blob is
/// charged to `blob_bytes` by the size its record states, before it is fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Items left.
    pub items: u64,
    /// Bytes of the model in memory left (text, the objects built, formula trees).
    pub bytes: u64,
    /// Bytes of blobs left.
    pub blob_bytes: u64,
    start: (u64, u64, u64),
    /// Whether the decode shares equal sheets and sections (`share`): on, unless [`Budget::without_sharing`] says otherwise.
    sharing: bool,
    /// Bytes of the model's pool that a tree held beside the model takes ([`Budget::reserve_tree`]): a charge is refused when it would leave less than this.
    reserved: u64,
    /// Whether the model may take the room of the blobs' pool that the blobs have not spent ([`Budget::with_lent_blobs`]).
    lend: bool,
    /// The bytes [`Budget::charge_blob`] has taken: what a blob cost, which `blob_bytes` alone does not say once the model may take the room of that pool.
    blobs_taken: u64,
    /// The items of a tree that the tree pool holds without the model's help: [`OPEN_TREE_ITEMS`], or what the limits a caller gave say ([`Budget::for_open`]). A child of a part
    /// read as a stream that has more is held beside the model by what it has beyond this ([`Budget::reserve_child`]).
    tree_items: u64,
    /// The most items one child of a part read as a stream may have, whatever room there is: [`OPEN_ELEMENT_ITEMS`], or the limits' own tree when a caller gave less than a tree.
    child_items: u64,
}

/// What a node of a parsed formula costs the budget, in bytes.
pub const FORMULA_NODE_BYTES: u64 = 128;
/// The items [`Budget::for_open`] allows, summed over every part of the package: 64 million. **An item takes no memory of the budget's** (the bytes are charged apart,
/// and a tree is let go before the next part is read): this is the time a decode may take, at the 8 million items or so a second it makes, and the work of the
/// parse beside it. **A `same` and a unit that an equal one is the same as are charged the items of the unit they stand for**, so that a file of references is
/// charged what the file that says each unit in full would be (a diagram of 350,000 rectangles of 167 items is 58 million); what it takes in time is a pointer
/// for each, and the file's bytes bound that (a part is at most [`OPEN_PART_BYTES`]), so the figure is what the drawing would be and not what the open does.
pub const OPEN_ITEMS: u64 = 64_000_000;
/// The items one tree may have: 2 million, the limit [`open_limits`] reads each part at, and a child of the root of a part read as a stream. A decode holds one such tree at a time
/// ([`PartSource`]), and the document part's tree beside it, which is charged to the model's pool by what it is ([`Budget::reserve_tree`]). It is what is left of the address space
/// for a tree once the pools and the file are held ([`OPEN_WORST_BYTES`]). (It was 4 million until a later change, which made the room for a larger pool of the model.)
pub const OPEN_TREE_ITEMS: u64 = 2_000_000;
/// The items of the document part's tree that are in no one's charge: 256,000, a tree of 41 MB. A document part is held by the caller, as a tree, beside the model while the pages are
/// read; **what it has beyond this is taken from the model's pool** ([`Budget::reserve_tree`]) for as long as the decode runs, so that the tree and the model together never pass the
/// pool, and a document part of ordinary size costs the drawing nothing. A document part that is read as a stream holds none.
pub const OPEN_DOCUMENT_FREE_ITEMS: u64 = 256_000;
/// The items **one child of the root of a part read as a stream** may have, whatever room there is (a shape and what it holds, a connect, a sheet): 4,000,000, what a part
/// was held to before pages were streams, so that a group of tens of thousands of shapes or a path of hundreds of thousands of rows that fit then fit now. **A child of up to
/// [`OPEN_TREE_ITEMS`] items is held in the pool of one tree; what it has beyond that is held back from the model's pool for as long as the child is held**
/// ([`Budget::reserve_child`], as [`Budget::reserve_tree`] does for the document part), so that a child the room of the model cannot hold beside it is refused in the words of the
/// pool, before its tree is made. A part read as a stream has as many children as it has bytes for.
pub const OPEN_ELEMENT_ITEMS: u64 = 4_000_000;
/// The XML items a part of a `.diacraft` may have **in all**: 12,000,000, the format's own limit (the spec's 6.1; 1.0 had 6,000,000, and 1.1 raised it so that a page of the
/// most the `.odg` reader opens is a part of the format). A writer does not write a part that passes it, however the part is made, and a part read as a stream is refused past it.
/// It is not the cap of a tree held whole, which is [`OPEN_TREE_ITEMS`], and not the container's cap of a tree for the other formats (`MAX_XML_NODES`, which the readers of those
/// keep): a part of this many items is read a child at a time and is never a tree.
pub const FORMAT_PART_ITEMS: u64 = 12_000_000;
/// The bytes of one **XML part** an open reads, inflated: 128 MiB. The part's bytes are held while it is parsed, beside the tree that is being made of them. A save
/// refuses a part past it ([`save_parts`]), and a package reader gives each XML entry this cap (a judge of the entry, which may be smaller than [`open_limits`]'s
/// 256 MiB, the cap of every entry: a picture is an entry too, and the blobs are held to their own pool).
pub const OPEN_PART_BYTES: u64 = 128 << 20;
/// The bytes of model [`Budget::for_open`] allows: 960 MiB (it was 640 MiB until a later change, when the tree of a part came down from 4 million items to 2 million and a part
/// was read a child at a time). It is what [`OPEN_WORST_BYTES`] leaves once the file, one tree, the parts' bytes, the blobs and what a process has are held.
pub const OPEN_MODEL_BYTES: u64 = 960 << 20;
/// The items of a tree the tree pool holds at `limits`: what the limits say of one tree, and never more than [`OPEN_TREE_ITEMS`].
fn tree_items_of(limits: &Limits) -> u64 {
    u64::try_from(limits.xml.max_nodes).unwrap_or(u64::MAX).min(OPEN_TREE_ITEMS)
}

/// The most items one child of a part read as a stream may have at `limits`, whatever room there is: [`OPEN_ELEMENT_ITEMS`], or the limits' own tree when a caller that has
/// less than a tree says so (a small machine's limits: a child is no larger than what it holds a tree to).
pub(crate) fn child_items_of(limits: &Limits) -> u64 {
    let tree = u64::try_from(limits.xml.max_nodes).unwrap_or(u64::MAX);
    if tree < OPEN_TREE_ITEMS { tree } else { OPEN_ELEMENT_ITEMS }
}

/// The bytes of blobs [`Budget::for_open`] allows: 384 MiB.
pub const OPEN_BLOB_BYTES: u64 = 384 << 20;
/// The bytes of the model and of the blobs **together** that [`Budget::for_open`] allows: 1,344 MiB, the two pools. The blobs are at most [`OPEN_BLOB_BYTES`] of it and the model at most
/// all of it, so that a drawing with no pictures has a model of this much and one with 384 MiB of pictures has [`OPEN_MODEL_BYTES`] ([`Budget::with_lent_blobs`]).
pub const OPEN_POOL_BYTES: u64 = OPEN_MODEL_BYTES + OPEN_BLOB_BYTES;
/// What the model of a drawing may hold **as an edit leaves it**: 1.5 GiB. It is not the pool of an open (a file the open takes is at most [`OPEN_MODEL_BYTES`]): a
/// drawing another program's file made (the `.odg` reader makes 1.4 GB of 350,000 rectangles) is edited, and an edit of it makes a copy of what it changes. It fits the
/// address space with the blobs and what a process has (the constant below says so at compile time), which a model of more would not: that is what the engine's
/// `Session::edit_if` refuses an edit past (the session's held count, `held_bytes_of`).
pub const EDIT_MODEL_BYTES: u64 = 3 << 29;
/// The inflated bytes of a package an open reads, XML parts and blobs together: 512 MiB. Only one part is in memory at a time, so what the package holds in all is a
/// matter of time and not of memory; **the file is in memory**, as a slice, while the container reads it ([`OPEN_FILE_BYTES`]).
pub const OPEN_PACKAGE_BYTES: u64 = 512 << 20;
/// The bytes of the file an open reads, which the caller holds as a slice while the container reads it: the package's cap and 64 MiB (the most a file may be above what
/// it holds, which the spec's 6.4 says: the directory of the archive and the headers of its entries). The file of a package is no more than it, and a caller refuses a
/// larger one before it reads it.
pub const OPEN_FILE_BYTES: u64 = OPEN_PACKAGE_BYTES + (64 << 20);
/// What the budget is held to: 3,000,000 KiB, the address space the tests give a process that opens a hostile drawing (`ulimit -v 3000000`).
pub const ADDRESS_SPACE_BYTES: u64 = 3_000_000 << 10;

/// What a process may hold in the worst case that an open of a package at [`open_limits`] and the budget of [`Budget::for_open`] allow: **the file** (a slice, as the
/// container reads it: [`OPEN_FILE_BYTES`]), **the tree of one part, and the document part's tree up to what is in no one's charge** ([`OPEN_DOCUMENT_FREE_ITEMS`]; the rest of it is
/// in the model's pool), the bytes of that part twice (as inflated, and as the strings of its tree, which exceed the bytes a tree is counted at an item with), the blobs, the model
/// (and the document part's tree beyond what is free), and what a process has before it opens anything.
pub const OPEN_WORST_BYTES: u64 = OPEN_FILE_BYTES
    + OPEN_TREE_ITEMS * cost::TREE_ITEM_BYTES
    + OPEN_DOCUMENT_FREE_ITEMS * cost::TREE_ITEM_BYTES
    + 2 * OPEN_PART_BYTES
    + OPEN_BLOB_BYTES
    + OPEN_MODEL_BYTES
    + cost::BASE_BYTES;

/// What a process may hold in the worst case **while a drawing is saved a part at a time** ([`save_parts_bytes`]): the model of the drawing as it is held (the figure that
/// the save refuses a drawing past, [`OPEN_MODEL_BYTES`]), the tables of streams that the check of the save keeps ([`SECTION_STREAM_BYTES`]), the tree of the child in hand
/// at the most items a child may have ([`OPEN_ELEMENT_ITEMS`]), its strings (the bytes a part may be), the blobs, and what a process has. The caller's file is not held: the
/// parts are handed over as they are made. It is arithmetic and not only a measurement: the constants say at compile time that it fits.
pub const SAVE_WORST_BYTES: u64 =
    OPEN_MODEL_BYTES + SECTION_STREAM_BYTES + OPEN_ELEMENT_ITEMS * cost::TREE_ITEM_BYTES + OPEN_PART_BYTES + OPEN_BLOB_BYTES + cost::BASE_BYTES;
// A drawing of a larger model is saved with a tree of fewer items ([`save_tree_items`]): at the most a save holds (an edit's figure) it still has a million.
const _: () = assert!(
    (ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20 - EDIT_MODEL_BYTES - SECTION_STREAM_BYTES - OPEN_PART_BYTES - OPEN_BLOB_BYTES - cost::BASE_BYTES)
        / cost::TREE_ITEM_BYTES
        >= 1_000_000,
    "a save of the largest model has no room for a tree"
);

// The open budget is sized so that its pools and the one tree of the part being read fit under [`ADDRESS_SPACE_BYTES`] together, at their worst, with a twentieth of
// it to spare. A figure moved so that this fails does not compile.
const _: () = assert!(OPEN_WORST_BYTES <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20, "the open budget does not fit the address space");
const _: () = assert!(SAVE_WORST_BYTES <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20, "the save does not fit the address space");
const _: () = assert!(OPEN_BLOB_BYTES <= OPEN_PACKAGE_BYTES, "the blobs are a part of the package");
// What the model may hold as an edit leaves it, with the blobs and a process's own, fits the address space with a twentieth to spare.
const _: () = assert!(
    EDIT_MODEL_BYTES + OPEN_BLOB_BYTES + cost::BASE_BYTES <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20,
    "the model of an edited drawing does not fit the address space"
);
const _: () = assert!(OPEN_PART_BYTES <= OPEN_PACKAGE_BYTES, "a part is a part of the package");

/// The container limits an open reads a package at, for the budget [`Budget::for_open`] makes: the package is [`OPEN_PACKAGE_BYTES`], an entry is the container's
/// 256 MiB (an XML part is held to [`OPEN_PART_BYTES`] besides, by the caller's judge of the entry), a tree is [`OPEN_TREE_ITEMS`] items, and as deep as the
/// container reads. **A caller reads one part at a time** ([`PartSource`]): it parses a part at these limits when the decode asks for it, and lets the part's bytes
/// go once the tree is made.
pub fn open_limits() -> Limits {
    let mut limits = Limits { max_package_bytes: OPEN_PACKAGE_BYTES, ..Limits::default() };
    limits.xml.max_nodes = usize::try_from(OPEN_TREE_ITEMS).unwrap_or(usize::MAX);
    // A part of the codec is as deep as 200 nested shapes and a kept element at its own cap of 200 levels beside them, which the container's default of 200
    // levels refuses: the open reads at the container's ceiling, the deepest the container reads at all.
    limits.xml.max_depth = diacraft_container::limits::XML_DEPTH_CEILING;
    limits
}

impl Budget {
    /// A budget of these amounts: also what [`CodecError::Limit`] names as the most, whatever the fields are changed to afterwards.
    pub fn new(items: u64, bytes: u64, blob_bytes: u64) -> Budget {
        Budget {
            items,
            bytes,
            blob_bytes,
            start: (items, bytes, blob_bytes),
            sharing: true,
            reserved: 0,
            lend: false,
            blobs_taken: 0,
            tree_items: OPEN_TREE_ITEMS,
            child_items: OPEN_ELEMENT_ITEMS,
        }
    }

    /// This budget, for a decode that shares nothing: every sheet and section is built for the shape that has it. For the tests that measure what each kind of
    /// element costs one by one (a thousand equal ones would be one), and for a caller that wants the model as the file spells it.
    #[must_use]
    pub fn without_sharing(mut self) -> Budget {
        self.sharing = false;
        self
    }

    /// This budget, with the model allowed **the room of the blobs' pool that the blobs have not spent**: the model and the blobs together are at most what the two pools are
    /// (the pools are sized as a sum that fits the address space, [`OPEN_WORST_BYTES`]), the blobs at most their own, and a drawing with no pictures has the whole for its model.
    /// What the model takes of it the blobs do not have. [`Budget::for_open`] is made so.
    #[must_use]
    pub fn with_lent_blobs(mut self) -> Budget {
        self.lend = true;
        self
    }

    /// Whether a decode under this budget shares equal sheets and sections.
    pub(crate) fn shares(&self) -> bool {
        self.sharing
    }

    /// The budget of one open, and of the pre-flight of a save (a save refuses what this would refuse): a labelled choice, no spec states one. **A decode that
    /// passes it stays under [`ADDRESS_SPACE_BYTES`] with the tree of the part it is reading held beside the model it makes**, which the constants say at compile
    /// time ([`OPEN_WORST_BYTES`]) and the tests of `model_pools.rs` hold with every pool at its limit at once (a child under `ulimit -v 3000000`).
    ///
    /// * **Items: [`OPEN_ITEMS`]**, 64 million (or the package's bytes over eight, when that is less: each item of a file that says every unit in full takes at least three bytes
    ///   of it, and a file that says a unit once stands for more items than it has bytes, and what it stands for is charged). They are the time of the decode and take no memory: a part's tree is let go before the next is read, and what a tree costs is bounded by
    ///   [`OPEN_TREE_ITEMS`] and [`cost::TREE_ITEM_BYTES`] for the one in hand.
    /// * **Bytes: [`OPEN_MODEL_BYTES`]**, 960 MiB, and the room of the blobs' pool that the blobs have not spent ([`OPEN_POOL_BYTES`], 1,344 MiB between the two; at most 128 bytes for each byte
    ///   of the package, when that is less): the model in memory, each object
    ///   charged by what it costs, whatever place it is in ([`cost`]: a place that holds one cell costs a leaf of eleven), and a sheet or a section that an
    ///   earlier one equals charged as a reference (`share`), so that the model a file builds is at most this and a file of millions of small elements is
    ///   refused where it would have aborted the process.
    /// * **Blobs: [`OPEN_BLOB_BYTES`]**, 384 MiB (or the package's bytes when that is less).
    ///
    /// The pool of the model, the blobs, the one tree and the two copies of its bytes, and the [`cost::BASE_BYTES`] a process has beside them are
    /// [`OPEN_WORST_BYTES`] (2,911,096,832 bytes) at their worst, **all of which one file can reach**: a pool is not another's slack. A caller with less memory
    /// (the web build, a small machine) passes smaller limits, and one with more passes a [`Budget::new`] of its own.
    pub fn for_open(limits: &Limits) -> Budget {
        let package = limits.max_package_bytes;
        let mut budget = Budget::new((package / 8).min(OPEN_ITEMS), package.saturating_mul(128).min(OPEN_MODEL_BYTES), package.min(OPEN_BLOB_BYTES))
            .with_lent_blobs();
        budget.tree_items = tree_items_of(limits);
        budget.child_items = child_items_of(limits);
        budget
    }

    /// The most items the next child of a part read as a stream may have: what the pool of one tree holds and what the model's room can hold beside it, at
    /// [`cost::TREE_ITEM_BYTES`] an item, and never more than the child limit ([`OPEN_ELEMENT_ITEMS`]).
    pub fn child_items(&self) -> u64 {
        self.tree_items.saturating_add(self.room() / cost::TREE_ITEM_BYTES).min(self.child_items)
    }

    /// Holds back the room of the tree of a child of a part read as a stream, which the decode holds beside the model while the child is read: the items it has beyond the tree
    /// pool's ([`Budget::for_open`]'s `tree_items`) are taken from the model's pool by [`cost::TREE_ITEM_BYTES`] an item. Refused, in the words of the pool, when they do not fit;
    /// [`Budget::release_tree`] gives them back, as it does for [`Budget::reserve_tree`].
    pub fn reserve_child(&mut self, items: u64) -> Result<u64, CodecError> {
        self.reserve_beyond(items, self.tree_items)
    }

    /// This budget, with the items of a tree that its tree pool holds set to `tree_items` and a child of a part read as a stream limited as `limits` limit a tree (a caller that
    /// sizes its pools from the memory it has, as the package reader does): a child that has more items than the pool is held back from the model's pool by what it has beyond them
    /// ([`Budget::reserve_child`]), and one past [`OPEN_ELEMENT_ITEMS`], or past the tree the limits hold when it is less than a tree of an open, is refused.
    #[must_use]
    pub fn with_tree_pool(mut self, tree_items: u64, limits: &Limits) -> Budget {
        self.tree_items = tree_items;
        self.child_items = child_items_of(limits);
        self
    }

    /// Takes `n` items.
    pub fn charge_items(&mut self, n: u64) -> Result<(), CodecError> {
        self.items = self.items.checked_sub(n).ok_or(CodecError::Limit { what: "items in the drawing", max: self.start.0 })?;
        Ok(())
    }

    /// What the model may still be charged: its own pool and, when the blobs' room is lent, what the blobs have not spent, less what a tree held back takes ([`Budget::reserve_tree`]).
    pub fn room(&self) -> u64 {
        let lent = if self.lend { self.blob_bytes } else { 0 };
        self.bytes.saturating_add(lent).saturating_sub(self.reserved)
    }

    /// Takes text of `len` bytes: **nine eighths of its length, rounded up** ([`cost::text_bytes`]), for the size classes of the app's allocator,
    /// which the bytes alone fall short of. Every name, value, formula, run and kept string the decoder charges by its length is charged so.
    pub fn charge_text(&mut self, len: usize) -> Result<(), CodecError> {
        self.charge_bytes(cost::text_bytes(len))
    }

    /// Takes `n` bytes of the model: a price the decoder worked out ([`cost`]), or text already charged as [`Budget::charge_text`] charges it.
    pub fn charge_bytes(&mut self, n: u64) -> Result<(), CodecError> {
        let lent = if self.lend { self.blob_bytes } else { 0 };
        let room = self.bytes.saturating_add(lent);
        match room.checked_sub(n) {
            Some(left) if left >= self.reserved => {}
            _ => return Err(self.bytes_refusal()),
        }
        // The model's own pool first, and then the room the blobs have not spent.
        let own = n.min(self.bytes);
        self.bytes -= own;
        self.blob_bytes -= n - own;
        Ok(())
    }

    /// Holds back the room of a tree of `items` that the caller holds beside the model for as long as the decode runs (the document part's tree): what it has beyond
    /// [`OPEN_DOCUMENT_FREE_ITEMS`] is taken from the pool by [`cost::TREE_ITEM_BYTES`] an item, so that no charge leaves the model and the tree past the pool. Refused, in the words
    /// of the pool, when the tree alone does not fit it. [`Budget::release_tree`] gives it back.
    pub fn reserve_tree(&mut self, items: u64) -> Result<u64, CodecError> {
        self.reserve_beyond(items, OPEN_DOCUMENT_FREE_ITEMS)
    }

    /// Holds back the room of the items of a tree that are more than `free`.
    fn reserve_beyond(&mut self, items: u64, free: u64) -> Result<u64, CodecError> {
        let bytes = items.saturating_sub(free).saturating_mul(cost::TREE_ITEM_BYTES);
        let room = self.bytes.saturating_add(if self.lend { self.blob_bytes } else { 0 });
        if room < self.reserved.saturating_add(bytes) {
            return Err(self.bytes_refusal());
        }
        self.reserved = self.reserved.saturating_add(bytes);
        Ok(bytes)
    }

    /// Gives back what [`Budget::reserve_tree`] held back, which it returned.
    pub fn release_tree(&mut self, bytes: u64) {
        self.reserved = self.reserved.saturating_sub(bytes);
    }

    /// What a charge that does not fit is refused as: the most the budget was made with, and not what is left of it.
    pub(crate) fn bytes_refusal(&self) -> CodecError {
        // The pool of the model, and the room of the blobs' that it may take when it is lent.
        let lent = if self.lend { self.start.2 } else { 0 };
        CodecError::Limit { what: "bytes of the model in memory", max: self.start.1.saturating_add(lent) }
    }

    /// The bytes of blobs this budget has been charged ([`Budget::charge_blob`]), and not what the model took of the room of that pool: a decode that asks whether a unit
    /// cost a blob compares this, and not `blob_bytes`, which the model's own charges lower once it has spent its own pool.
    pub fn blobs_charged(&self) -> u64 {
        self.blobs_taken
    }

    /// Takes `n` bytes of blobs. **When the model may take the room of this pool** ([`Budget::with_lent_blobs`]), a blob is not charged what the model and a tree held back
    /// ([`Budget::reserve_tree`]) count on: the room that is left of the two pools, less what the blob takes, is at least what the tree holds back. A budget whose pools are apart
    /// keeps them apart, and a tree is held back against the model's alone.
    pub fn charge_blob(&mut self, n: u64) -> Result<(), CodecError> {
        let refused = CodecError::Limit { what: "bytes of pictures and parts in the drawing", max: self.start.2 };
        let left = self.blob_bytes.checked_sub(n).ok_or_else(|| refused.clone())?;
        if self.lend && self.bytes.saturating_add(left) < self.reserved {
            return Err(refused);
        }
        self.blob_bytes = left;
        self.blobs_taken = self.blobs_taken.saturating_add(n);
        Ok(())
    }
}

/// The parts of a model as trees, in the order a package stores them: `pages` and `masters` each with the ID of its model object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    /// The document part, with the lists naming the other parts and the blobs.
    pub document: El,
    /// Each page's part, in page order, with the page's `ID`.
    pub pages: Vec<(u32, El)>,
    /// Each master's part, in master order, with the master's `ID`.
    pub masters: Vec<(u32, El)>,
}

/// What a document part lists: the part of each page and of each master, in order, and each blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartLists {
    /// Each page's `ID` and the name of its part.
    pub pages: Vec<(u32, String)>,
    /// Each master's `ID` and the name of its part.
    pub masters: Vec<(u32, String)>,
    /// Each blob, in order of number.
    pub blobs: Vec<BlobRecord>,
}

/// One blob of the list in a document part: its number, the name of its entry, its media type, and what it holds (its size and the stamp of its
/// bytes), by which a reader knows a blob that lies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobRecord {
    pub id: BlobId,
    /// `blobs/<n>.<ext>` ([`blob_path`]).
    pub path: String,
    pub media_type: String,
    pub size: u64,
    pub stamp: Stamp,
}

/// What a decode lost or did not understand, for the note a person is shown. Each is counted by what it names, never listed by a name from the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeNote {
    /// A picture's bytes are lost (no entry, or one that does not match its record): the shape shows none. One for each blob.
    PictureLost { blob: BlobId },
    /// A theme's bytes are lost: the theme is left out.
    ThemeLost { part: String },
    /// A part's bytes are lost: the part is left out.
    PartLost { name: String },
    /// Elements and attributes the codec does not know, which no slot held, `count` of them.
    Unknown { count: u64 },
}

/// Why a model could not be encoded or decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// A cap of the model, of the XML reader, or of the budget, and the number it passes.
    Limit { what: &'static str, max: u64 },
    /// A part that is not what the vocabulary says: `part` names it (`document`, `page`, ...), and `why` the damage, in words.
    Malformed { part: String, why: &'static str },
    /// A tree the XML writer refused.
    Xml(XmlWriteError),
}

impl CodecError {
    /// The class word: `model_too_complex`, `model_malformed` or `model_xml`.
    pub fn class(&self) -> &'static str {
        match self {
            CodecError::Limit { .. } => "model_too_complex",
            CodecError::Malformed { .. } => "model_malformed",
            CodecError::Xml(_) => "model_xml",
        }
    }

    pub(crate) fn malformed(part: &str, why: &'static str) -> CodecError {
        CodecError::Malformed { part: part.to_owned(), why }
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Limit { what, max } => write!(f, "the drawing holds more than {max} {what}"),
            CodecError::Malformed { part, why } => write!(f, "the {part} part is damaged: {why}"),
            CodecError::Xml(e) => write!(f, "a part cannot be written: {e}"),
        }
    }
}

impl std::error::Error for CodecError {}

impl From<XmlWriteError> for CodecError {
    fn from(e: XmlWriteError) -> CodecError {
        CodecError::Xml(e)
    }
}

impl From<diacraft_doc::DocError> for CodecError {
    fn from(e: diacraft_doc::DocError) -> CodecError {
        match e {
            diacraft_doc::DocError::Limit { what, max } => CodecError::Limit { what, max: u64::try_from(max).unwrap_or(u64::MAX) },
            diacraft_doc::DocError::TooManyPages => {
                CodecError::Limit { what: "pages", max: u64::try_from(diacraft_doc::MAX_PAGES).unwrap_or(u64::MAX) }
            }
            diacraft_doc::DocError::BadName(_) => CodecError::malformed("document", "a name that is empty or too long"),
            _ => CodecError::malformed("document", "a model that breaks the model's own rules"),
        }
    }
}
