//! A master body in DiaCraft's document file, and the sheets of the file's entries: written and read by the whole-model codec (`diacraft_sheetxml::model`; the
//! native-format design's 2.1). The shape layer this module held (a master, its shapes with their sheets, text, shape data, foreign data, children and kept
//! data) is the codec's now, and so are the sheets of the entries (`docsheet`, `page`, `group`, `style`): one codec, one set of tests.
//!
//! **What an element is.** A `master` of the file is the codec's own master element (`encode_master`): it declares DiaCraft's namespace itself, as a part of a `.diacraft`
//! does, holds its shapes directly (a group's members directly in the group, no `shapes` wrapper), and writes a kept element as data (an `el` element holding its name,
//! never literal XML), so that nothing a master or a shape holds is left out: the slots an `.odg` and a newer build fill come back too. A `sheet` of an entry is the codec's
//! sheet element (`encode_sheet`). The elements of the file's own (`words`, `page`, `colors`, ...) are elements of the same namespace, as they were.
//!
//! **Pictures are the file's own list.** A picture a shape of a master shows is a `blob` element of the file, which names it by a number and holds its bytes as base64 text, once
//! however many shapes show it (the codec's `BlobSink` and `BlobSource`; the design's 1.5). Nothing of a master is in another part of the package. The reader takes the list
//! first, whatever its place among the file's children, and charges each blob to the budget of pictures once, as the bytes it decodes to, before it is decoded.
//!
//! **Costs.** A decode is given the reader's whole room (its items, and the bytes those stand for at [`ITEM_BYTES`] each), is refused by the codec's own check before it builds
//! what it cannot pay for, and is charged to the reader by what it spent, whether it ended or was refused as damaged: no file is read for less than it cost, and a damaged
//! master is left out with a word and is no error. The shapes and the cells it built are counted against the document's shapes and cells afterwards. A write holds the memory
//! of the trees it is going to make before it makes them (`tree_bytes`, counted from the models), and the depth of what it writes is the codec's (the shapes nested to the
//! model's cap, and a kept element as deep as the codec writes one: at most 407 levels, which the reader parses the file at, `docfile::parse_limits`).

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Master, Sheet};
use diacraft_sheetxml::Scope;
use diacraft_sheetxml::model::cost::TREE_ITEM_BYTES;
use diacraft_sheetxml::model::{
    BlobId, BlobSink, BlobSource, Budget, CodecError, DecodeNote, MemBlobs, decode_master, decode_sheet, encode_master, encode_sheet,
};
use diacraft_sheetxml::passthrough::canonical_u32;

use crate::error::{LimitKind, OdgError};
use crate::figures::Figures;
use crate::read::{ITEM_BYTES, Reader};
use crate::write::WriteError;

/// The prefix the document file's elements are written under.
pub(crate) const P: &str = "diacraft";

// ------------------------------------------------------------------------------------------------------------------ base64

/// The standard alphabet of base64, with padding.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 of `bytes` (RFC 4648, with padding, no line breaks).
pub(crate) fn base64_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk.first().copied().unwrap_or(0)) << 16
            | u32::from(chunk.get(1).copied().unwrap_or(0)) << 8
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        let at = |shift: u32| char::from(B64[((n >> shift) & 63) as usize]);
        out.push(at(18));
        out.push(at(12));
        out.push(if chunk.len() > 1 { at(6) } else { '=' });
        out.push(if chunk.len() > 2 { at(0) } else { '=' });
    }
    out
}

/// The decoder of base64 text the reader has, over the pieces of the characters (white space is skipped).
pub(crate) fn base64_decoded(text: &str) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut pad) = (0u32, 0u32, 0usize);
    for c in text.chars().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => {
                pad += 1;
                continue;
            }
            _ => return None,
        };
        if pad > 0 {
            return None;
        }
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).ok()?);
        }
    }
    (pad <= 2).then_some(out)
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// A codec refusal, as the save's: a cap is a limit with its figure, and a tree the codec found damaged or XML cannot hold is the writer's own words.
pub(crate) fn write_refusal(e: CodecError) -> WriteError {
    match e {
        CodecError::Limit { what, max } => WriteError::Limit { what, max },
        CodecError::Malformed { why, .. } => WriteError::Unsupported(why),
        CodecError::Xml(_) => WriteError::Unsupported("a document file with a character XML cannot hold"),
    }
}

/// A sink and source for a sheet, which holds no picture.
pub(crate) struct NoBlobs;

impl BlobSink for NoBlobs {
    fn put(&mut self, _bytes: &Arc<[u8]>, _media_type: &str) -> Result<BlobId, CodecError> {
        Err(CodecError::Malformed { part: "sheet".to_owned(), why: "a sheet holds no picture" })
    }
}

impl BlobSource for NoBlobs {
    fn get(&self, _id: BlobId) -> Option<Arc<[u8]>> {
        None
    }
}

/// A master with its body, as the codec's master element: the pictures its shapes show are put in `blobs`, each distinct one once. Refused in words for a master past
/// a cap of the model (the shapes nested deeper than the model holds them, a cell name too long, too many shapes in a part).
pub(crate) fn master_el(master: &Master, blobs: &mut MemBlobs) -> Result<El, WriteError> {
    encode_master(master, blobs).map_err(write_refusal)
}

/// A sheet as the codec's sheet element, for an entry of the file.
pub(crate) fn sheet_node(sheet: &Sheet) -> Result<El, WriteError> {
    encode_sheet(sheet, &mut NoBlobs).map_err(write_refusal)
}

/// The `blob` elements of the pictures the masters showed, in the order of their numbers: the number and the bytes as base64 text.
pub(crate) fn blob_els(blobs: &MemBlobs) -> Vec<El> {
    (0..blobs.len())
        .filter_map(|at| {
            let id = BlobId(u32::try_from(at).ok()?);
            let (bytes, _) = blobs.entry(id)?;
            Some(El {
                name: format!("{P}:blob"),
                attrs: vec![("id".to_owned(), id.0.to_string())],
                children: if bytes.is_empty() { Vec::new() } else { vec![Node::Text(base64_text(bytes))] },
            })
        })
        .collect()
}

/// The most items the codec's tree has for one shape, as the figures count it: the shape element and its ten attributes, its sheet element and the five of its own, the
/// `text` element, the `foreign` element with its two attributes and the picture with its three, and the slot of the shape.
const SHAPE_ITEMS: u64 = 26;
/// For one cell: the element, its six attributes, the `extra` element and its attribute, and the cell's slot.
const CELL_ITEMS: u64 = 10;
/// For one node the figures count (a section, a row, a kept attribute, a kept node, a connection): the most of them is a section or a row, an element and five attributes, and
/// a connection (an element and six).
const NODE_ITEMS: u64 = 8;
/// For one piece of a shape's text (a run of characters, a marker, a field, a kept node): the most is a field or a kept node of four.
const PART_ITEMS: u64 = 5;
/// For one master: the element, its eight attributes, and its sheet with its own five.
const MASTER_ITEMS: u64 = 16;
/// What the file made of an item of its tree takes in bytes: the name of an element or an attribute and the marks round them, at the least and the most a name is.
const OUT_ITEM_BYTES: u64 = 48;

/// What the trees of the codec for the masters whose figures are `figures` hold at the most, in bytes, **counted from the models before any tree is made**: every unit the
/// figures count at the most items the vocabulary makes of it, each item at the codec's price for a tree item and the bytes the file made of it, the text of the models
/// three times (the tree's copy, the file's, and the slack of the buffer the file grows in), and the pictures three times as base64 (the text in the tree, the file's, and the
/// slack). What the slots of an `.odg` hold (`ext`) is no figure of the walk yet, and is a later task's to add when it fills them.
pub(crate) fn tree_bytes(figures: &Figures, masters: u64) -> u64 {
    let parts = figures.steps.saturating_sub(figures.shapes).saturating_sub(figures.cells).saturating_sub(figures.nodes);
    let items = figures
        .shapes
        .saturating_mul(SHAPE_ITEMS)
        .saturating_add(figures.cells.saturating_mul(CELL_ITEMS))
        .saturating_add(figures.nodes.saturating_mul(NODE_ITEMS))
        .saturating_add(parts.saturating_mul(PART_ITEMS))
        .saturating_add(masters.saturating_mul(MASTER_ITEMS));
    items
        .saturating_mul(TREE_ITEM_BYTES.saturating_add(OUT_ITEM_BYTES))
        .saturating_add(figures.text_bytes.saturating_mul(3))
        .saturating_add(figures.media_bytes.saturating_mul(3))
}

// ------------------------------------------------------------------------------------------------------------------ reading

/// The vocabulary of the document file: elements of DiaCraft's namespace.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ours;

impl diacraft_sheetxml::Vocabulary for Ours {
    fn reads(self, uri: Option<&str>) -> bool {
        uri.is_some_and(diacraft_doc::is_diacraft_urn)
    }
}

pub(crate) type Sc = Scope<Ours>;

/// The pictures a document file lists, by the number each is named by: what the codec asks for the bytes of a picture it reads.
#[derive(Default)]
pub(crate) struct Pictures(HashMap<u32, Arc<[u8]>>);

impl BlobSource for Pictures {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.0.get(&id.0).cloned()
    }
}

/// The bytes of the text of `el`, counted without copying it.
fn text_bytes(el: &El) -> usize {
    el.children
        .iter()
        .map(|n| match n {
            Node::Text(t) | Node::CData(t) => t.len(),
            _ => 0,
        })
        .fold(0usize, usize::saturating_add)
}

/// The bytes of picture a blob whose text is `encoded` bytes long is charged before its text is decoded: **the most its text can decode to**, three bytes for four characters
/// and the bytes the characters past the last four make (six bits each, a byte for each eight: `encoded * 3 / 4`), as the `.odg` reader charges an inline picture. A text with
/// blanks or padding decodes to fewer. (Until a later change it was a quarter of the characters, three bytes each, which left out the one or two bytes of a text with no padding.)
pub(crate) fn picture_bytes(encoded: usize) -> u64 {
    u64::try_from(encoded).unwrap_or(u64::MAX).saturating_mul(3) / 4
}

/// The `blob` children of the file's root, each charged **before it is decoded**: its element and attributes, the bytes the text stands for against the budget of pictures,
/// and the text itself by its bytes. A blob whose number was named before is read, charged, and left out (the first stands), and so is one with no number or whose text is not
/// base64: the masters that name it find no picture and say so.
pub(crate) fn read_pictures(r: &mut Reader<'_>, root: &El, sc: &Sc) -> Result<Pictures, OdgError> {
    let mut by_id: HashMap<u32, Arc<[u8]>> = HashMap::new();
    for child in root.elements() {
        if !sc.enter(child).is(child, "blob") {
            continue;
        }
        r.saw_element();
        r.take_items(1 + child.attrs.len())?;
        let encoded = text_bytes(child);
        // The bytes the text stands for are charged to the pictures' budget before they are made.
        let size = picture_bytes(encoded);
        match r.media_left.checked_sub(size) {
            Some(left) => r.media_left = left,
            None => return Err(OdgError::TooComplex { what: "bytes of pictures", max: r.limits.max_media_bytes }),
        }
        r.take_model_bytes(encoded)?;
        let Some(id) = child.attr("id").and_then(canonical_u32) else { continue };
        if by_id.contains_key(&id) {
            continue;
        }
        // The text of one node is decoded where it stands: a copy of it would be a second copy of the largest string the file holds.
        let decoded = match child.children.as_slice() {
            [Node::Text(text)] => base64_decoded(text),
            _ => base64_decoded(&child.text()),
        };
        if let Some(bytes) = decoded {
            by_id.insert(id, Arc::from(bytes));
        }
    }
    Ok(Pictures(by_id))
}

/// The room a decode of the codec is given: the reader's items, and the bytes of model they stand for. The pictures are charged where they are read, so the budget of
/// blobs of the decode is not the limit of anything.
fn room(r: &Reader<'_>) -> Budget {
    let items = u64::try_from(r.items_room()).unwrap_or(u64::MAX);
    Budget::new(items, items.saturating_mul(ITEM_BYTES as u64), u64::MAX / 2)
}

/// Charges the reader what a decode spent: the items, and the bytes of the model as the items they stand for.
fn settle(r: &mut Reader<'_>, before: &Budget, after: &Budget) -> Result<(), OdgError> {
    let items = before.items.saturating_sub(after.items);
    let bytes = before.bytes.saturating_sub(after.bytes);
    r.saw_elements(items);
    r.take_items(usize::try_from(items).unwrap_or(usize::MAX))?;
    r.take_model_bytes(usize::try_from(bytes).unwrap_or(usize::MAX))
}

/// What the codec's refusal of a decode is to the reader: its budget is the reader's items and bytes, which refuse the drawing. What it found damaged, and a cap of the
/// model it is past, is the `Ok`, the words for it.
fn refusal(r: &Reader<'_>, e: CodecError) -> Result<&'static str, OdgError> {
    match e {
        CodecError::Limit { what: "items in the drawing" | "bytes of the model in memory", .. } => {
            Err(OdgError::Limit { kind: LimitKind::Items, max: r.limits.max_items as u64 })
        }
        // A cap of the model that the master or the sheet is past (a shape nested deeper than the model holds one, a kept element past its depth) makes that one element
        // damaged, as a master the codec finds malformed is: **reading never fails the drawing for what the document file holds** (the design's 7.3), and the items and
        // the bytes of the reader's room are the only refusals.
        CodecError::Limit { .. } => Ok("it is past a limit of the model"),
        CodecError::Malformed { why, .. } => Ok(why),
        CodecError::Xml(_) => Ok("a tree that XML cannot hold"),
    }
}

/// Runs one decode of the codec within the reader's room and charges the reader what it spent. A damaged element (the codec's `Malformed`) is left out with a word, as a
/// theme that is not base64 is, and is `None`: no file is read for less than the work it was, and the drawing reads from its markup. The notes the codec gives are words.
fn decode_in<T>(
    r: &mut Reader<'_>,
    kind: &'static str,
    run: impl FnOnce(&mut Budget, &mut Vec<DecodeNote>) -> Result<T, CodecError>,
) -> Result<Option<T>, OdgError> {
    decode_in_at(r, &format!("a {kind} in DiaCraft's document file"), run)
}

/// [`decode_in`] for a fragment that is not an element of the document file (a shape's data on its own style): `what` says what it is, in the words of the note.
pub(crate) fn decode_in_at<T>(
    r: &mut Reader<'_>,
    what: &str,
    run: impl FnOnce(&mut Budget, &mut Vec<DecodeNote>) -> Result<T, CodecError>,
) -> Result<Option<T>, OdgError> {
    let mut budget = room(r);
    let before = budget.clone();
    let mut notes: Vec<DecodeNote> = Vec::new();
    let out = run(&mut budget, &mut notes);
    // The work the decode did is the file's cost, whatever it ended as.
    let spent = settle(r, &before, &budget);
    let out = match out {
        Ok(out) => out,
        Err(e) => {
            spent?;
            let why = refusal(r, e)?;
            r.warn("diacraft_file_unreadable", || format!("{what} is damaged ({why}), and is left out"));
            return Ok(None);
        }
    };
    spent?;
    for note in notes {
        match note {
            DecodeNote::PictureLost { .. } => {
                r.warn("diacraft_picture_lost", || "a picture of a master in DiaCraft's document file has no bytes, and is not shown".to_owned());
            }
            DecodeNote::Unknown { .. } => r.warn_once("diacraft_version", || {
                "DiaCraft's document file holds names this build does not know: they are left out where no slot keeps them".to_owned()
            }),
            DecodeNote::ThemeLost { .. } | DecodeNote::PartLost { .. } => {}
        }
    }
    Ok(Some(out))
}

/// A `master` element read as a master with its body, pictures from `pictures`. The shapes and cells it holds are counted against the document's, after the items and the
/// bytes it cost were charged. `None` for a master the codec found damaged.
pub(crate) fn read_master(r: &mut Reader<'_>, el: &mut El, pictures: &Pictures) -> Result<Option<Arc<Master>>, OdgError> {
    declare_namespace(r, el)?;
    let el: &El = el;
    let Some(master) = decode_in(r, "master", |budget, notes| decode_master(el, pictures, budget, notes))? else { return Ok(None) };
    let master = Arc::new(master);
    let figures = crate::figures::of_masters([&master]);
    r.count_shapes(usize::try_from(figures.shapes).unwrap_or(usize::MAX))?;
    r.count_cells(usize::try_from(figures.cells).unwrap_or(usize::MAX))?;
    Ok(Some(master))
}

/// The attribute that declares DiaCraft's namespace on `master` under the prefix it is written with, when it has none: `None` when it declares it (the codec's own master
/// element does).
pub(crate) fn missing_declaration(master: &El) -> Option<String> {
    let declaration = match master.name.split_once(':') {
        Some((prefix, _)) => format!("xmlns:{prefix}"),
        None => "xmlns".to_owned(),
    };
    (!master.attrs.iter().any(|(k, _)| *k == declaration)).then_some(declaration)
}

/// Gives `master` the declaration of DiaCraft's namespace when it has none, **in place and charged first**. A master reads as the root of a part of the codec, which declares the
/// namespace itself; a file whose root declares it for its children (the way a hand-made file does) reads the same. This is done for the master being read and for no other,
/// when it is reached (a pass over every master of the file, made before any was charged, took 8.4 seconds and 1.6 GB for 3.9 million empty ones), and
/// with no copy of the master's subtree (a copy of it was a second tree that no charge paid for): it is one attribute, an item for its pair and the bytes of its two strings.
fn declare_namespace(r: &mut Reader<'_>, master: &mut El) -> Result<(), OdgError> {
    let Some(declaration) = missing_declaration(master) else { return Ok(()) };
    r.take_items(2)?;
    r.take_bytes(declaration.len().saturating_add(super::names::URN.len()))?;
    master.attrs.push((declaration, super::names::URN.to_owned()));
    Ok(())
}

/// A `sheet` element read as a sheet, as an entry of the file holds one. Its cells are counted against the document's. `None` for a sheet the codec found damaged.
pub(crate) fn read_sheet(r: &mut Reader<'_>, el: &El) -> Result<Option<Sheet>, OdgError> {
    let Some(sheet) = decode_in(r, "sheet", |budget, notes| decode_sheet(el, &NoBlobs, budget, notes))? else { return Ok(None) };
    r.count_cells(usize::try_from(crate::figures::of_sheet(&sheet).cells).unwrap_or(usize::MAX))?;
    Ok(Some(sheet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use diacraft_doc::{Cell, CellName, ForeignData, Media, RawAttrs, Shape, ShapeList, ShapeType, Value};

    /// A root that declares DiaCraft's namespace and holds `children`.
    fn root(children: Vec<Node>) -> El {
        El { name: format!("{P}:document"), attrs: vec![(format!("xmlns:{P}"), super::super::names::URN.to_owned())], children }
    }

    fn blob(id: &str, text: &str) -> Node {
        Node::El(El { name: format!("{P}:blob"), attrs: vec![("id".to_owned(), id.to_owned())], children: vec![Node::Text(text.to_owned())] })
    }

    fn cell(text: &str) -> Cell {
        Cell::constant(Value::from_text(text))
    }

    /// Base64 is the standard alphabet with padding, in both directions: the vectors of RFC 4648 and the two characters that follow the letters and digits.
    #[test]
    fn base64_is_the_standard_alphabet_with_padding_both_ways() {
        for (plain, coded) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64_text(plain.as_bytes()), coded, "{plain:?}");
            assert_eq!(base64_decoded(coded).as_deref(), Some(plain.as_bytes()), "{coded:?}");
        }
        for (bytes, coded) in [(vec![0xFB, 0xEF, 0xBE], "++++"), (vec![0xFF, 0xFF, 0xFF], "////"), (vec![0xFB, 0xFF], "+/8="), (vec![0xFF], "/w==")] {
            assert_eq!(base64_text(&bytes), coded);
            assert_eq!(base64_decoded(coded), Some(bytes), "{coded:?}");
        }
        // Every length to forty comes back as it went.
        for len in 0u8..40 {
            let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37).wrapping_add(len).wrapping_add(200)).collect();
            assert_eq!(base64_decoded(&base64_text(&bytes)), Some(bytes), "{len}");
        }
        // White space is skipped.
        assert_eq!(base64_decoded(" Zm9v\n\tYg==\r\n ").as_deref(), Some(b"foob".as_slice()));
    }

    /// The decoder refuses a character that is no part of the alphabet, data after padding, and more than two padding characters.
    #[test]
    fn base64_that_is_not_base64_is_no_bytes() {
        for text in ["Z$==", "Zg=A", "Zg===", "=Zg=", "Zg==Zg==", "Zm9v!", "Zm9v\u{e9}"] {
            assert_eq!(base64_decoded(text), None, "{text:?}");
        }
    }

    /// The refusal of a decode is the reader's budget of items for the two budgets the codec spends the reader's items as (its items and its bytes of model), the reader's
    /// a word for an element past a cap of the model, and a word for a master the codec finds damaged.
    #[test]
    fn a_refusal_of_the_codec_is_the_readers_items_a_cap_of_the_model_or_a_damaged_element() {
        let limits = Limits { max_items: 77, ..Limits::default() };
        let r = Reader::new(&limits);
        for what in ["items in the drawing", "bytes of the model in memory"] {
            match refusal(&r, CodecError::Limit { what, max: 5 }) {
                Err(OdgError::Limit { kind: LimitKind::Items, max: 77 }) => {}
                other => panic!("{what}: {other:?}"),
            }
        }
        for what in ["levels of nested shapes", "levels of a kept element", "shapes in a part", "blobs"] {
            assert_eq!(
                refusal(&r, CodecError::Limit { what, max: 200 }).ok(),
                Some("it is past a limit of the model"),
                "{what}: a cap of the model is a damaged element"
            );
        }
        assert_eq!(refusal(&r, CodecError::Malformed { part: "master".to_owned(), why: "why" }).ok(), Some("why"));
        assert!(refusal(&r, CodecError::Xml(diacraft_container::xml::XmlWriteError::BadNode("x"))).is_ok());
    }

    /// The write side's words: a cap is a limit with its figure, and a damaged tree is the writer's own words.
    #[test]
    fn a_refusal_of_the_encoder_is_the_saves_limit_or_the_writers_words() {
        assert!(matches!(write_refusal(CodecError::Limit { what: "x", max: 3 }), WriteError::Limit { what: "x", max: 3 }));
        assert!(matches!(write_refusal(CodecError::Malformed { part: "p".to_owned(), why: "w" }), WriteError::Unsupported("w")));
    }

    /// A decode is given the reader's room and the reader is charged what the decode spent: the items, and the bytes of the model as the items they stand for.
    #[test]
    fn the_codec_is_given_the_readers_room_and_the_reader_is_charged_what_it_spent() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let budget = room(&r);
        assert_eq!((budget.items, budget.bytes), (limits.max_items as u64, limits.max_items as u64 * ITEM_BYTES as u64));
        let before = budget.clone();
        let mut after = budget;
        after.items -= 10;
        after.bytes -= 64 * 3 + 1;
        settle(&mut r, &before, &after).unwrap();
        assert_eq!(r.items_room(), limits.max_items - 10 - 4, "ten items, and 193 bytes are four items of 64");
        assert_eq!(r.report.work.elements, 10);
    }

    /// A master is read from the codec's element, and its shapes and cells are counted against the document's: a document that may hold one cell or one shape less than
    /// the master has refuses it.
    #[test]
    fn a_master_read_is_counted_in_shapes_and_cells() {
        let mut shape = Shape::new(1);
        shape.kind = ShapeType::Shape;
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Width"), cell("1"));
        let mut inner = Shape::new(2);
        Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new("Height"), cell("2"));
        shape.children = ShapeList::from_vec(vec![Arc::new(inner)]);
        let master = Master { id: 4, shapes: ShapeList::from_vec(vec![Arc::new(shape)]), ..Master::default() };
        let mut el = master_el(&master, &mut MemBlobs::new()).unwrap();
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let got = read_master(&mut r, &mut el, &Pictures::default()).unwrap().unwrap();
        assert_eq!(*got, master);
        assert_eq!(r.report.stats.cells, 2, "two cells");
        let exact = Limits { max_cells: 2, max_shapes: 2, ..Limits::default() };
        assert!(read_master(&mut Reader::new(&exact), &mut el, &Pictures::default()).is_ok(), "as many as the master has are enough");
        let tight = Limits { max_cells: 1, ..Limits::default() };
        assert!(matches!(read_master(&mut Reader::new(&tight), &mut el, &Pictures::default()), Err(OdgError::Limit { kind: LimitKind::Cells, .. })));
        let tight = Limits { max_shapes: 1, ..Limits::default() };
        assert!(matches!(read_master(&mut Reader::new(&tight), &mut el, &Pictures::default()), Err(OdgError::Limit { kind: LimitKind::Shapes, .. })));
    }

    /// The deepest tree the model holds is written: shapes nested to the model's cap, which is more than the old file held (90 levels at two elements a level), and one
    /// level more than the cap is refused in words and is no tree.
    #[test]
    fn shapes_nested_to_the_models_cap_are_written_and_one_more_is_refused_in_words() {
        let chain = |n: u32| {
            let mut s = Shape::new(n);
            for id in (1..n).rev() {
                let mut parent = Shape::new(id);
                parent.children = ShapeList::from_vec(vec![Arc::new(s)]);
                s = parent;
            }
            Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }
        };
        let most = u32::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap();
        assert!(master_el(&chain(most), &mut MemBlobs::new()).is_ok(), "{most} levels");
        match master_el(&chain(most + 1), &mut MemBlobs::new()) {
            Err(WriteError::Limit { what, max }) => assert_eq!((what, max), ("levels of nested shapes", u64::from(most))),
            other => panic!("{other:?}"),
        }
    }

    /// The pictures list: each blob is charged its element, its text and the bytes the text stands for against the budget of pictures, the first of a number stands, and
    /// text that is not base64 or a blob with no number is charged and left out.
    #[test]
    fn the_pictures_of_the_file_are_charged_and_the_first_of_a_number_stands() {
        let limits = Limits::default();
        let file = root(vec![blob("1", "Zm9vYg=="), blob("1", "AAAA"), blob("x", "AAAA"), blob("2", "not base64!"), blob("3", "")]);
        let sc = Sc::empty().enter(&file);
        let mut r = Reader::new(&limits);
        let pictures = read_pictures(&mut r, &file, &sc).unwrap();
        assert_eq!(pictures.get(BlobId(1)).as_deref(), Some(b"foob".as_slice()), "the first of a number stands");
        assert_eq!(pictures.get(BlobId(2)), None, "text that is not base64 is no picture");
        assert_eq!(pictures.get(BlobId(3)).as_deref(), Some(b"".as_slice()), "an empty blob is an empty picture");
        // 8 + 4 + 4 + 11 + 0 characters stand for 6 + 3 + 3 + 8 + 0 bytes (three for each four characters, and six bits for each one past them).
        assert_eq!(limits.max_media_bytes - r.media_left, 6 + 3 + 3 + 8);
        // The budget of pictures is exact: as many bytes as the text stands for are enough, and one fewer is refused.
        let one = root(vec![blob("1", "Zm9vYmFy")]);
        let sc = Sc::empty().enter(&one);
        let exact = Limits { max_media_bytes: 6, ..Limits::default() };
        assert!(read_pictures(&mut Reader::new(&exact), &one, &sc).is_ok());
        let short = Limits { max_media_bytes: 5, ..Limits::default() };
        assert!(matches!(read_pictures(&mut Reader::new(&short), &one, &sc), Err(OdgError::TooComplex { what: "bytes of pictures", max: 5 })));
        // An element of another namespace that is called `blob` is none of the file's.
        let stranger = El {
            name: "x:blob".to_owned(),
            attrs: vec![("xmlns:x".to_owned(), "urn:other".to_owned()), ("id".to_owned(), "9".to_owned())],
            children: Vec::new(),
        };
        let other = root(vec![Node::El(stranger)]);
        let sc = Sc::empty().enter(&other);
        let mut r = Reader::new(&limits);
        assert!(read_pictures(&mut r, &other, &sc).unwrap().0.is_empty());
        assert_eq!(r.items_room(), limits.max_items, "and is charged nothing");
    }

    /// A blob whose text is in more than one node (a comment between two runs of it, as another program's writer may leave one) is read as the text it makes together.
    #[test]
    fn a_blob_whose_text_is_in_several_nodes_is_read_as_one_text() {
        let split = Node::El(El {
            name: format!("{P}:blob"),
            attrs: vec![("id".to_owned(), "5".to_owned())],
            children: vec![Node::Text("Zm9v".to_owned()), Node::Comment("c".to_owned()), Node::Text("Yg==".to_owned())],
        });
        let file = root(vec![split]);
        let sc = Sc::empty().enter(&file);
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let pictures = read_pictures(&mut r, &file, &sc).unwrap();
        assert_eq!(pictures.get(BlobId(5)).as_deref(), Some(b"foob".as_slice()));
        assert_eq!(limits.max_media_bytes - r.media_left, 6, "charged as the eight characters it holds stand for");
    }

    /// Every text of up to eight characters, by the kinds of character the decoder tells apart (one of the alphabet, the padding, a blank, anything else: which one of a kind
    /// changes the bytes and not their number), decodes to no more than the charge for its length, and one of the alphabet alone to exactly it: what the Kani harnesses of
    /// `picture_bytes` prove for one to seven characters, for eight too.
    #[test]
    fn every_text_of_up_to_eight_characters_is_charged_no_less_than_it_decodes_to() {
        const KINDS: [char; 4] = ['A', '=', ' ', '!'];
        for len in 0..=8u32 {
            for n in 0..4usize.pow(len) {
                let text: String = (0..len).map(|i| KINDS[n / 4usize.pow(i) % 4]).collect();
                if let Some(bytes) = base64_decoded(&text) {
                    assert!(bytes.len() as u64 <= picture_bytes(text.len()), "{text:?}");
                }
            }
            let plain = "A".repeat(len as usize);
            assert_eq!(base64_decoded(&plain).map(|bytes| bytes.len() as u64), Some(picture_bytes(plain.len())), "{plain}");
        }
    }

    /// A blob whose text has no padding is charged the bytes it decodes to, before it is decoded (the harness
    /// `a_blob_is_charged_no_less_than_the_bytes_its_text_decodes_to`: a quarter of the characters, three bytes each, was charged, which for `AAA` is none of the two bytes the
    /// decoder makes of it, and two bytes short for every text whose length is three past a multiple of four).
    #[test]
    fn a_blob_whose_text_has_no_padding_is_charged_the_bytes_it_decodes_to() {
        for (text, decoded) in [("AAA", 2usize), ("AA", 1), ("A", 0), ("Zm9vYmE", 5), ("Zm9vYg", 4), ("Zm9vYmFy", 6)] {
            let file = root(vec![blob("1", text)]);
            let sc = Sc::empty().enter(&file);
            let limits = Limits::default();
            let mut r = Reader::new(&limits);
            let pictures = read_pictures(&mut r, &file, &sc).unwrap();
            assert_eq!(pictures.get(BlobId(1)).map(|bytes| bytes.len()), Some(decoded), "{text}");
            assert_eq!(limits.max_media_bytes - r.media_left, decoded as u64, "{text}: charged the bytes it decodes to");
            // A budget of one byte fewer refuses it, before it is decoded.
            if decoded > 0 {
                let short = Limits { max_media_bytes: decoded as u64 - 1, ..Limits::default() };
                assert!(
                    matches!(read_pictures(&mut Reader::new(&short), &file, &sc), Err(OdgError::TooComplex { what: "bytes of pictures", .. })),
                    "{text}"
                );
            }
        }
    }

    /// A picture written is in the list of the blobs once, and read again it is the picture of the shape that named it.
    #[test]
    fn a_picture_is_written_as_a_blob_once_and_read_again() {
        let media =
            |part: &str| Arc::new(Media { part: Arc::from(part), content_type: Arc::from("image/png"), bytes: Arc::from(vec![1u8, 2, 3, 4, 5]) });
        let shape = |id: u32, part: &str| {
            let mut s = Shape::new(id);
            s.kind = ShapeType::Foreign;
            s.foreign = Some(Arc::new(ForeignData { picture: Some(media(part)), attrs: RawAttrs::new(), ..ForeignData::default() }));
            Arc::new(s)
        };
        let master = Master { id: 1, shapes: ShapeList::from_vec(vec![shape(1, "a.png"), shape(2, "b.png")]), ..Master::default() };
        let mut sink = MemBlobs::new();
        let mut el = master_el(&master, &mut sink).unwrap();
        let blobs = blob_els(&sink);
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs.first().map(El::text).as_deref(), Some("AQIDBAU="));
        let file = root(blobs.into_iter().map(Node::El).collect());
        let sc = Sc::empty().enter(&file);
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let pictures = read_pictures(&mut r, &file, &sc).unwrap();
        let got = read_master(&mut r, &mut el, &pictures).unwrap().unwrap();
        assert_eq!(*got, master);
    }

    /// The tree an estimate is made for is never smaller than the tree: counted from the figures of a rich master before the tree is made, and held to the items of the
    /// tree that is made, at the price of an item and what the file makes of it.
    #[test]
    fn the_estimate_of_a_masters_tree_is_never_below_what_the_tree_holds() {
        let mut master = Master { id: 1, ..Master::default() };
        let mut shapes = Vec::new();
        for id in 1..=20u32 {
            let mut s = Shape::new(id);
            s.name = Some(Arc::from("n"));
            s.name_u = Some(Arc::from("u"));
            s.kind = ShapeType::Foreign;
            s.foreign = Some(Arc::new(ForeignData {
                rel_id: Some(Arc::from("r")),
                text: Some(Arc::from("t")),
                attrs: RawAttrs::from_pairs([("a", "b")]),
                picture: Some(Arc::new(Media { part: Arc::from("p"), content_type: Arc::from("c"), bytes: Arc::from(vec![9u8; 100]) })),
                ..ForeignData::default()
            }));
            let sheet = Arc::make_mut(&mut s.sheet);
            for k in 0..30 {
                sheet.cells.insert(CellName::new(&format!("C{k}")), Cell::read(Some("1+1"), Some("2"), Some("IN"), None, &mut 1_000));
            }
            s.raw = RawAttrs::from_pairs([("k", "v")]);
            shapes.push(Arc::new(s));
        }
        master.shapes = ShapeList::from_vec(shapes);
        master.raw = RawAttrs::from_pairs([("m", "n")]);
        let figures = crate::figures::of_masters([&Arc::new(master.clone())]);
        let el = master_el(&master, &mut MemBlobs::new()).unwrap();
        // The items of the tree, as the XML reader counts them: each element, attribute and text.
        let mut items = 0u64;
        let mut stack = vec![&el];
        while let Some(e) = stack.pop() {
            items += 1 + e.attrs.len() as u64;
            for c in &e.children {
                match c {
                    Node::El(child) => stack.push(child),
                    _ => items += 1,
                }
            }
        }
        let estimate = tree_bytes(&figures, 1);
        assert!(
            estimate >= items * (TREE_ITEM_BYTES + OUT_ITEM_BYTES) + figures.text_bytes * 3,
            "{estimate} bytes estimated for a tree of {items} items"
        );
    }
}
