//! What the reader's own objects cost, in bytes: the shapes, text, pictures, styles, masters and pages it builds, and the XML trees it holds
//! while it builds them. The cells, rows, sections and kept markup of a sheet are priced by `diacraft_sheetxml::price`, whose module documentation says what a
//! price is; this file prices the rest the same way, from the sizes of the model's types and the allocator's rounding, and `tests/it/memory_price.rs` holds
//! each figure against a process that made the object. `tests/it/memory_counted.rs` holds each kind's charge to what its model keeps, **counted** by the test binary's
//! allocator, with 24 bytes for each allocation: a process's growth does not see a vector's room that nothing has touched.
//!
//! **Everything the read holds at once is one budget** (`Limits::max_memory`): the file the caller holds, the package inflated, the XML tree of every part
//! being read (held while it is read: [`TREE_ITEM_BYTES`] an item at the most, and the room its text can take), and the model that is built (charged before it is
//! built). What is left of it when the file and the package are counted is the **pool** the trees and the model are charged to: **each is counted before it is
//! made, and a read that would pass the pool is refused in words** (`too_much_memory`) where it is counted. **What no charge counts is the margin**, the 0.47 GB that
//! the budget leaves of the 3.07 GB of an address-space limit of `ulimit -v 3000000` (the allocator's slack on what is freed and made again, the process's own 143 MB,
//! and the walks of a tree once it is parsed): the cases of `tests/it/memory_price.rs` reach 85 percent of the limit at the most, and that is measured, not proved.

use std::mem::size_of;
use std::sync::Arc;

use diacraft_doc::{Connect, ContentTypeRow, CustomProp, ExtData, ForeignData, Kept, Master, Media, Page, Part, Rel, Shape, Style, Text, TextPart};
use diacraft_opc::xml::ParseSize;
use diacraft_sheetxml::model::cost::{chunk, push_bytes};
use diacraft_sheetxml::price::{PAIR_BYTES, POINTER_BYTES, SHEET_BYTES, arc_str, block, heap, raw_attr_bytes};

/// What a list that grows holds beside itself while it grows: the allocator makes the new list, copies the old into it and frees the old, so for a moment both are
/// there, and the price of a push is the growth and not the two. The old list is at most the 32 MiB the allocator serves from its arenas (a larger one is moved by
/// the system without a copy), the same again is the table of a map that grows, and one list grows at a time: **64 MiB, kept back from the pool** for the one
/// moment. (A read of a file of 500,000 connects, a list of 37.7 MB, held 20 MB more than it was charged, which is this.)
pub const GROWTH_BYTES: u64 = 64 * 1024 * 1024;

/// What the structure of the tree of a part can cost for each item it holds (an element, an attribute, a text, a comment or an instruction), **at the worst the
/// parser can make it**, which is what a parse is bounded by before the tree is made. The characters in the strings are not in it (they are counted from the
/// bytes of the part: [`tree_bound`]). Of each string the allocator makes a chunk of at most 32 bytes more than it holds. A list of children has room for four at its
/// first child (288 bytes, 304 as a chunk), so an element that has one child holds that for it; with that child's own name it is **336 an element in a chain of
/// elements** (the dearest tree of elements: 335 measured, in chains of 190), 336 for a text or a comment that is the only child, **368 for an instruction that is
/// the only child** (its two strings, 64, and the 304) and 272 for an element's one attribute (a list of attributes with room for four, 208, and the two strings, 64).
/// A list that has more has less for each item. Once a tree is made, what it holds is counted from the tree (`unheld::tree_bytes`), which is about 140 an item for
/// every tree a drawing is. (The container's parser makes lists with room for four and for what doubling gives, and a tree it made with less would be priced the
/// same: when its lists are made to the size they need, this is what to re-derive, with the test of `unheld.rs` that holds every kind of tree to it.)
pub const TREE_ITEM_BYTES: u64 = 368;

/// The most the tree of a part can hold, whatever the part is, from what is known of it before it is parsed: `items` items at [`TREE_ITEM_BYTES`] each, and the
/// characters in its strings, which are at most the text the parse reads (`size.text_bytes`) and which a text that is made of many pieces holds at up to twice what
/// it ends with (a string grows by doubling). Every kind of tree is held to this by `unheld.rs`.
#[must_use]
pub fn tree_bound(items: u64, size: ParseSize) -> u64 {
    items.saturating_mul(TREE_ITEM_BYTES).saturating_add(size.text_bytes.saturating_mul(2))
}

/// What a parse of a part holds that does not depend on the items of its tree: the room its strings can take ([`tree_bound`]'s twice the text) and the copy of the text
/// that it makes and keeps until it is done (a part in UTF-16). A parse is bounded to the items the pool can hold when this is taken out of it.
#[must_use]
pub fn parse_fixed_bytes(size: ParseSize) -> u64 {
    size.text_bytes.saturating_mul(2).saturating_add(size.copy_bytes)
}

/// An `Arc<Shape>` and the `Arc<Sheet>` it holds, and the entry in the table of the part's shape `ID`s (a number and a flag, at its least full). A shape's place in
/// the list that holds it is [`push_bytes`] of [`POINTER_BYTES`], and its strings are [`arc_str`] each.
pub const SHAPE_BYTES: u64 = chunk(16 + size_of::<Shape>()) as u64 + SHEET_BYTES + 32;

/// An `Arc<Text>`; its list of parts is made whole from the part's children and holds one [`TextPart`] for each.
pub const TEXT_BYTES: u64 = chunk(16 + size_of::<Text>()) as u64;

/// An `Arc<ForeignData>`.
pub const FOREIGN_BYTES: u64 = chunk(16 + size_of::<ForeignData>()) as u64;

/// A picture: the `Arc<Media>`, and its entry in the table of the pictures loaded (the folded name, the slot and the pointer). The picture's own bytes and the
/// two names are [`arc_str`] each.
pub const MEDIA_BYTES: u64 = chunk(16 + size_of::<Media>()) as u64 + 192;

/// An `Arc<Style>`, its sheet, its place in the list, and the `ID`s noted of it (the set of the document, the table of the check of links and its three walks).
pub const STYLE_BYTES: u64 = chunk(16 + size_of::<Style>()) as u64 + SHEET_BYTES + 8 + 160;

/// An `Arc<Master>`, its sheet and its place in the list, and what the read notes of it: its place by `ID`, its `UniqueID`, and the part it parsed.
pub const MASTER_BYTES: u64 = chunk(16 + size_of::<Master>()) as u64 + SHEET_BYTES + 8 + 256;

/// An `Arc<Page>`, its sheet and its place in the list, and the `ID` noted of it and the count of its shapes.
pub const PAGE_BYTES: u64 = chunk(16 + size_of::<Page>()) as u64 + SHEET_BYTES + 8 + 64;

/// A part parsed once however many rows name it: its entry in the table of the parts (a key and the lists it shares), and its relationships' map.
pub const PARSED_BYTES: u64 = 256;

/// A row of a relationships part: the three strings it holds, and its entry in the map of the part's `Id`s.
pub const REL_ROW_BYTES: u64 = 512;

/// A `ColorEntry` or a `FaceName` and its place in the table.
pub const TABLE_ROW_BYTES: u64 = 128;

/// What putting one more shape in a list of `have` costs: the shape ([`SHAPE_BYTES`]) and the growth of the list.
#[must_use]
pub fn shape_bytes(have: usize) -> u64 {
    SHAPE_BYTES.saturating_add(push_bytes(have, POINTER_BYTES))
}

/// What one more `Connect` costs a list of `have`: the row, and the two names it holds (counted at their length).
#[must_use]
pub fn connect_bytes(have: usize) -> u64 {
    push_bytes(have, size_of::<Connect>())
}

/// A text part that holds `len` bytes of characters, a comment or a marker's text.
#[must_use]
pub fn text_part_bytes(len: usize) -> u64 {
    arc_str(len)
}

/// The list of parts of a text of `n` children (made whole from the children, one [`TextPart`] each).
#[must_use]
pub fn text_parts_bytes(n: usize) -> u64 {
    heap(n.saturating_mul(size_of::<TextPart>()))
}

/// What `all_attrs` makes of `n` attributes of an element kept whole: the `Arc`, the list made whole, and the two names of each.
#[must_use]
pub fn all_attrs_bytes<'a>(attrs: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
    let mut n = 0usize;
    let mut names = 0u64;
    for (k, v) in attrs {
        n += 1;
        names = names.saturating_add(arc_str(k.len())).saturating_add(arc_str(v.len()));
    }
    if n == 0 { 0 } else { block(16 + 24).saturating_add(heap(n.saturating_mul(PAIR_BYTES))).saturating_add(names) }
}

/// What a list of attributes made by pushes costs (`RawAttrs::push`, one at a time, as a face's are): its `Arc`, a list with room for four at the first and twice
/// as much each time it is full, and the two names of each ([`raw_attr_bytes`] of each in turn). [`all_attrs_bytes`] is the list made whole, which holds what it
/// has (a face of one attribute was priced at the whole list of one, 48 bytes, and holds the room of four, 128).
#[must_use]
pub fn pushed_attrs_bytes<'a>(attrs: impl Iterator<Item = (&'a str, &'a str)>) -> u64 {
    attrs.enumerate().fold(0u64, |sum, (have, (k, v))| sum.saturating_add(raw_attr_bytes(have, k.len(), v.len())))
}

/// The bytes a picture of `size` bytes costs once copied: the bytes, the `Arc<Media>`, the two names and the entry in the table.
#[must_use]
pub fn picture_bytes(size: usize, part: usize, content_type: usize) -> u64 {
    // The bytes themselves are charged as they are (`Reader::take_exact`), and what is built around them with the margin: pass 0 for them.
    (if size == 0 { 0 } else { arc_str(size) }).saturating_add(MEDIA_BYTES).saturating_add(arc_str(part)).saturating_add(arc_str(content_type))
}

/// Whether `value` is the text of a number a typed field gives back as it is (`read_u32`: no sign, no leading zeros).
fn canonical(value: &str) -> bool {
    diacraft_sheetxml::passthrough::canonical_u32(value).is_some()
}

/// What the attributes of an element cost the model, besides `base`, the objects the element makes: **an attribute a typed field holds costs what `typed` says of it**
/// (nothing for a number or a flag that reads as it is written, a string for a name), **and any other is kept**, as a pair of strings in the element's list of kept
/// attributes, which has room for four at its first.
pub(crate) fn attrs_bytes(base: u64, attrs: &[(String, String)], typed: impl Fn(&str, &str) -> Option<u64>) -> u64 {
    let mut bytes = base;
    let mut kept = 0usize;
    for (k, v) in attrs {
        let more = typed(k, v).unwrap_or_else(|| {
            kept += 1;
            raw_attr_bytes(kept - 1, k.len(), v.len())
        });
        bytes = bytes.saturating_add(more);
    }
    bytes
}

/// The attributes of a `Shape` that go to a typed field: its `ID`, its style links and (in a page part) its `Master` and `MasterShape` when they read as written, its
/// names, its type, and a `Del` that is `1`.
pub(crate) fn shape_attr(is_master: bool) -> impl Fn(&str, &str) -> Option<u64> {
    move |k, v| match k {
        "ID" | "LineStyle" | "FillStyle" | "TextStyle" if canonical(v) => Some(0),
        "Master" | "MasterShape" if canonical(v) && !is_master => Some(0),
        "Name" | "NameU" => Some(arc_str(v.len())),
        "Type" => Some(0),
        "Del" if v == "1" => Some(0),
        _ => None,
    }
}

/// The attributes of a `StyleSheet` that go to a typed field: its `ID` and style links when they read as written, and its names.
pub(crate) fn style_attr(k: &str, v: &str) -> Option<u64> {
    match k {
        "ID" | "LineStyle" | "FillStyle" | "TextStyle" if canonical(v) => Some(0),
        "Name" | "NameU" => Some(arc_str(v.len())),
        _ => None,
    }
}

/// The attributes of a `Master` row that go to a typed field: its `ID` and `PatternFlags` when they read as written, its names, and its `UniqueID`, which is held twice
/// (in the model, and in the set that finds a repeat).
pub(crate) fn master_attr(k: &str, v: &str) -> Option<u64> {
    match k {
        "ID" | "PatternFlags" if canonical(v) => Some(0),
        "Name" | "NameU" => Some(arc_str(v.len())),
        "UniqueID" => Some(arc_str(v.len()).saturating_add(heap(v.len()))),
        _ => None,
    }
}

/// The attributes of a `Page` row that go to a typed field: its `ID` and `BackPage` when they read as written, its names, and a `Background` that is `1`.
pub(crate) fn page_attr(k: &str, v: &str) -> Option<u64> {
    match k {
        "ID" | "BackPage" if canonical(v) => Some(0),
        "Name" | "NameU" => Some(arc_str(v.len())),
        "Background" if v == "1" => Some(0),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------------------------ what is held

/// The record of an extension slot (`ExtData<T>`), made when the first thing is put in the slot: the allocation and the counts of its `Arc`. `T` is the slot's
/// own type (`OdfShape` for a shape's, `Kept` for a master's), whose size the record holds.
#[must_use]
pub const fn ext_data_bytes<T>() -> u64 {
    chunk(16 + size_of::<ExtData<T>>()) as u64
}

/// What one more entry costs a slot's list of the VSDX elements the model has no field for (`VsdxRest`), which holds `have`: the growth of the list, and the
/// key, which is an `Arc<str>` of its own. What the entry holds is `diacraft_sheetxml::price`'s, charged as it is kept.
#[must_use]
pub fn rest_entry_bytes(have: usize, key: usize) -> u64 {
    push_bytes(have, size_of::<(Arc<str>, Kept)>()).saturating_add(arc_str(key))
}

/// A held part (`Part`): the `Arc` and what it holds. Its name, its content type, its relationships and its bytes are charged beside it, and its place in the
/// list of the document is `push_bytes` of `POINTER_BYTES`.
pub const HELD_PART_BYTES: u64 = chunk(16 + size_of::<Part>()) as u64;

/// The `Arc<Vec<Rel>>` of a held part's relationships, when it has any.
pub const HELD_RELS_BYTES: u64 = chunk(16 + size_of::<Vec<Rel>>()) as u64;

/// What one more relationship row costs a list that holds `have`: the growth of the list, and the three strings of the row. The source's own name is shared by
/// the rows of one source, and charged once for them.
#[must_use]
pub fn held_rel_bytes(have: usize, id: usize, kind: usize, target: usize) -> u64 {
    push_bytes(have, size_of::<Rel>()).saturating_add(arc_str(id)).saturating_add(arc_str(kind)).saturating_add(arc_str(target))
}

/// What one more content-type row costs a list that holds `have`: the growth of the list, and the two strings of the row.
#[must_use]
pub fn held_type_row_bytes(have: usize, key: usize, content_type: usize) -> u64 {
    push_bytes(have, size_of::<ContentTypeRow>()).saturating_add(arc_str(key)).saturating_add(arc_str(content_type))
}

/// What one more custom property costs a list that holds `have`: the growth of the list, and the strings of the property. Its other attributes are kept
/// attributes, charged as such.
#[must_use]
pub fn custom_prop_bytes(have: usize, name: usize, value_type: usize, value: usize) -> u64 {
    push_bytes(have, size_of::<CustomProp>()).saturating_add(arc_str(name)).saturating_add(arc_str(value_type)).saturating_add(arc_str(value))
}
