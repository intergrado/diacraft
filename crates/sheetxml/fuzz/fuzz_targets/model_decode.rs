//! `cargo fuzz run model_decode`: the whole-model codec's decoder on any part. The first byte chooses the kind of part (0 the document part, 1 a
//! page part, 2 a master part) and the rest is parsed as XML by the container's bounded parser, at the native depth ceiling and a smaller cap of items so that an
//! input stays quick; a tree that parses is decoded with a budget of its own. A decode gives an error or a document, and a document **holds the model's caps**
//! (`check_limits`), **encodes again**, and what that makes **decodes to the same document** (the codec's contract), whatever bytes it came from; and the second
//! encoding is the first's bytes (a fixed point). A page or a master is put in a document of its own for that. A document part names pages and masters: each is
//! given as an honest part of the ID the list gives, so that the lists drive the decode and not a refusal for a missing part. Anything else, a panic, a hang or
//! a document the codec cannot give back, is a crash the fuzzer reports. The save's pre-flight is held to the open: at the limits of a package
//! of a mebibyte, a document is saved and then decoded within the budget of those limits, or refused in words; never saved and then refused. **A part is read and written as a
//! stream of the children of its root**: the bytes of every part are read by a source that holds them as bytes, and what the stream reads is what the tree of the
//! same bytes reads, the same document, notes and budget; the save of bytes hands over the bytes the writer makes of the trees of the other save, and a refusal is in words.
#![no_main]

use std::borrow::Cow;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{El, XmlLimits, parse_with, to_bytes};
use diacraft_doc::Document;
use diacraft_sheetxml::model::{
    BlobId, BlobSource, Budget, CodecError, DecodeNote, MemBlobs, PartRole, PartSource, SavedBytes, decode, decode_from, decode_master, decode_page,
    encode, encode_for_save, part_lists, save_parts, save_parts_bytes,
};
use libfuzzer_sys::fuzz_target;

/// Gives blobs 0 to 3, small, and loses the rest.
struct Blobs;

impl BlobSource for Blobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        (id.0 < 4).then(|| Arc::from(vec![id.0 as u8; (id.0 as usize + 1) * 3]))
    }
}

fn budget() -> Budget {
    Budget::new(4_000_000, 256 << 20, 64 << 20)
}

fn limits() -> XmlLimits {
    XmlLimits { max_depth: XML_DEPTH_CEILING, max_nodes: 300_000, max_attr_bytes: 1 << 20 }
}

/// An honest page part of ID `id`.
fn page_part(id: u32) -> El {
    El {
        name: "d:page".to_owned(),
        attrs: vec![("xmlns:d".to_owned(), diacraft_sheetxml::model::URN.to_owned()), ("id".to_owned(), id.to_string())],
        children: Vec::new(),
    }
}

fn master_part(id: u32) -> El {
    El { name: "d:master".to_owned(), ..page_part(id) }
}

/// What the codec gives of `doc`: its parts' bytes and its blobs. **No write error is excused**: a document that decoded (and held the model's caps) writes, and
/// what it writes is XML a writer can write (a cap on a write was once excused, which hid a drawing that opened and could never be saved).
fn bytes_of(doc: &Document) -> (Vec<Vec<u8>>, MemBlobs) {
    let mut blobs = MemBlobs::new();
    let e = encode(doc, &mut blobs).expect("a document that decoded encodes");
    let mut parts = vec![to_bytes(&e.document, &[], &[], true).expect("the document part is XML a writer can write")];
    for (_, p) in e.pages.iter().chain(&e.masters) {
        parts.push(to_bytes(p, &[], &[], true).expect("a part is XML a writer can write"));
    }
    (parts, blobs)
}

fn decode_parts(parts: &[Vec<u8>], pages: usize, blobs: &MemBlobs) -> Document {
    let l = XmlLimits { max_depth: XML_DEPTH_CEILING, ..XmlLimits::default() };
    let trees: Vec<El> = parts.iter().map(|b| parse_with(b, &l).expect("what the codec wrote parses")).collect();
    let (document, rest) = trees.split_first().expect("a document part");
    let (p, m) = rest.split_at(pages);
    let (p, m): (Vec<&El>, Vec<&El>) = (p.iter().collect(), m.iter().collect());
    let mut big = Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4);
    decode(document, &p, &m, blobs, &mut big, &mut Vec::new()).expect("what the codec wrote decodes")
}

/// A document as text, without the places of unknown elements: `after: 3` where it is written.
fn without_places(doc: &Document) -> String {
    let text = format!("{doc:?}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("after: ") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 7..].bytes().take_while(u8::is_ascii_digit).count();
        rest = &rest[at + 7 + digits..];
    }
    out.push_str(rest);
    out
}

/// The limits of an open of a package of a mebibyte: small enough that a document a fuzz input makes is refused by them now and then.
fn small_limits() -> Limits {
    let mut limits = Limits { max_package_bytes: 1 << 20, ..Limits::default() };
    limits.xml.max_depth = XML_DEPTH_CEILING;
    limits.xml.max_nodes = usize::try_from(Budget::for_open(&limits).items).unwrap_or(usize::MAX);
    limits
}

/// The parts of a package a save a part at a time handed over, as bytes: asked for one at a time by an open (`decode_from`).
struct Handed {
    pages: Vec<Vec<u8>>,
    masters: Vec<Vec<u8>>,
    limits: XmlLimits,
}

impl PartSource for Handed {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        let bytes = self.pages.get(index).ok_or(CodecError::Malformed { part: "page".to_owned(), why: "no such part" })?;
        parse_with(bytes, &self.limits).map_err(|_| CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" })
    }

    fn master(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        let bytes = self.masters.get(index).ok_or(CodecError::Malformed { part: "master".to_owned(), why: "no such part" })?;
        parse_with(bytes, &self.limits).map_err(|_| CodecError::Malformed { part: "master".to_owned(), why: "a part the reader refuses" })
    }
}

/// A source that holds each part as bytes, so that a decode reads it as a stream of the children of its root.
struct Streamed {
    pages: Vec<Vec<u8>>,
    masters: Vec<Vec<u8>>,
}

impl PartSource for Streamed {
    type Tree = El;

    fn page(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "page".to_owned(), why: "the tree of a part held as bytes was asked for" })
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "the tree of a part held as bytes was asked for" })
    }

    fn page_bytes(&mut self, index: usize, _name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        Ok(self.pages.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }

    fn master_bytes(&mut self, index: usize, _name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        Ok(self.masters.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }
}

/// **What a stream reads is what the tree of the same bytes reads**: the same document, the same notes of what was lost, and the same budget left.
fn streams_read_what_trees_read(parts: &[Vec<u8>], pages: usize, blobs: &MemBlobs) {
    let l = XmlLimits { max_depth: XML_DEPTH_CEILING, ..XmlLimits::default() };
    let (first, rest) = parts.split_first().expect("a document part");
    let document = parse_with(first, &l).expect("what the codec wrote parses");
    let (p, m) = rest.split_at(pages);
    let trees: Vec<El> = rest.iter().map(|b| parse_with(b, &l).expect("what the codec wrote parses")).collect();
    let (tp, tm) = trees.split_at(pages);
    let (tp, tm): (Vec<&El>, Vec<&El>) = (tp.iter().collect(), tm.iter().collect());
    let (mut a, mut b) = (budget_big(), budget_big());
    let (mut notes_a, mut notes_b) = (Vec::new(), Vec::new());
    let streamed = decode_from(&document, &mut Streamed { pages: p.to_vec(), masters: m.to_vec() }, blobs, &mut a, &mut notes_a);
    let treed = decode(&document, &tp, &tm, blobs, &mut b, &mut notes_b);
    match (streamed, treed) {
        (Ok(x), Ok(y)) => {
            assert!(x == y, "the stream and the tree of one part read two documents");
            assert_eq!(notes_a, notes_b, "the same notes");
            assert_eq!((a.items, a.bytes, a.blob_bytes), (b.items, b.bytes, b.blob_bytes), "the same budget spent");
        }
        (Err(x), Err(y)) => assert_eq!(x.to_string(), y.to_string(), "the same refusal"),
        (x, y) => panic!("the stream says {:?} and the tree says {:?}", x.err(), y.err()),
    }
}

fn budget_big() -> Budget {
    Budget::new(u64::MAX / 4, u64::MAX / 4, u64::MAX / 4)
}

/// The save of bytes: the bytes the writer makes of the trees of the save of every part, or a refusal in words that is a limit; and a package it hands over is read as a stream.
fn saves_bytes_or_is_refused_in_words(doc: &Document, held: Option<&diacraft_sheetxml::model::Encoded>) {
    let limits = small_limits();
    let mut blobs = MemBlobs::new();
    let (mut document, mut pages, mut masters) = (Vec::new(), Vec::new(), Vec::new());
    let out = save_parts_bytes(doc, &mut blobs, &limits, &mut |part: SavedBytes<'_>| {
        match part.role {
            PartRole::Document => document = part.bytes.to_vec(),
            PartRole::Page { .. } => pages.push(part.bytes.to_vec()),
            PartRole::Master { .. } => masters.push(part.bytes.to_vec()),
        }
        Ok(())
    });
    match (out, held) {
        (Ok(()), Some(e)) => {
            // The bytes are the writer's bytes of the trees of the other save.
            assert_eq!(document, to_bytes(&e.document, &[], &[], true).expect("writes"), "the document part");
            for (bytes, (_, tree)) in pages.iter().zip(&e.pages) {
                assert_eq!(bytes, &to_bytes(tree, &[], &[], true).expect("writes"), "a page part");
            }
            for (bytes, (_, tree)) in masters.iter().zip(&e.masters) {
                assert_eq!(bytes, &to_bytes(tree, &[], &[], true).expect("writes"), "a master part");
            }
            let mut parts = vec![document];
            parts.extend(pages);
            parts.extend(masters);
            streams_read_what_trees_read(&parts, e.pages.len(), &blobs);
        }
        // The stream holds a page to the items of one child, so it may save what the tree of a part cannot hold.
        (Ok(()), None) => {}
        (Err(CodecError::Limit { .. }), None) => {}
        (Err(e), Some(_)) => panic!("the save of bytes refuses what the save of trees takes: {e}"),
        (Err(other), _) => panic!("a save of bytes that is refused for a reason that is not a limit: {other}"),
    }
}

/// A save that is made is a package the open takes: the parts decode within the budget of the limits it was checked against. A save that is refused is refused in
/// words, as a limit. **The save a part at a time says what the save that holds every part says**, and the package it hands over is read by an open that asks for
/// a part at a time and gives the document back.
fn saves_or_is_refused_in_words(doc: &Document) {
    let limits = small_limits();
    let mut blobs = MemBlobs::new();
    let held = encode_for_save(doc, &mut blobs, &limits);
    let mut streamed_blobs = MemBlobs::new();
    let (mut document, mut pages, mut masters) = (Vec::new(), Vec::new(), Vec::new());
    let streamed = save_parts(doc, &mut streamed_blobs, &limits, &mut |part| {
        let bytes = to_bytes(part.tree, &[], &[], true)?;
        match part.role {
            PartRole::Document => document = bytes,
            PartRole::Page { .. } => pages.push(bytes),
            PartRole::Master { .. } => masters.push(bytes),
        }
        Ok(())
    });
    match (&held, &streamed) {
        (Ok(_), Ok(())) => {}
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "the two saves refuse in the same words"),
        (a, b) => panic!("the save of every part says {:?} and the save a part at a time says {:?}", a.as_ref().err(), b.as_ref().err()),
    }
    saves_bytes_or_is_refused_in_words(doc, held.as_ref().ok());
    match held {
        Ok(e) => {
            let pages_of: Vec<&El> = e.pages.iter().map(|(_, p)| p).collect();
            let masters_of: Vec<&El> = e.masters.iter().map(|(_, p)| p).collect();
            let out = decode(&e.document, &pages_of, &masters_of, &blobs, &mut Budget::for_open(&limits), &mut Vec::new());
            assert!(out.is_ok(), "a save the pre-flight passed is refused by the open: {:?}", out.err());
            let l = XmlLimits { max_depth: XML_DEPTH_CEILING, ..XmlLimits::default() };
            let root = parse_with(&document, &l).expect("the document part the save handed over parses");
            let opened =
                decode_from(&root, &mut Handed { pages, masters, limits: l }, &streamed_blobs, &mut Budget::for_open(&limits), &mut Vec::new())
                    .expect("a package a save a part at a time handed over is opened by an open that asks for a part at a time");
            assert!(opened == out.expect("checked"), "the two packages open to one document");
        }
        Err(CodecError::Limit { .. }) => {}
        Err(other) => panic!("a save that is refused for a reason that is not a limit: {other}"),
    }
}

fn exercise(doc: Document) {
    assert!(doc.check_limits().is_ok(), "a decoded document holds the model's caps");
    saves_or_is_refused_in_words(&doc);
    let (first, blobs) = bytes_of(&doc);
    streams_read_what_trees_read(&first, doc.pages.len(), &blobs);
    let back = decode_parts(&first, doc.pages.len(), &blobs);
    // What was read comes back, but for the places of unknown elements: one that followed a known child the codec does not write again (a theme whose bytes
    // are lost, a second `sheet`, a part with no content) is closed up against the ones that are written.
    assert!(without_places(&back) == without_places(&doc), "the codec gives the document back");
    let (second, blobs) = bytes_of(&back);
    // From there on the document is as the codec writes it, and comes back exactly.
    let again = decode_parts(&second, back.pages.len(), &blobs);
    assert!(again == back, "a document the codec wrote comes back as it is");
    // And the encoding is a fixed point **once a file has been read and written**: the first write spells units as the codec does (the namespaces they are read in, what
    // an unknown element carried, what a decode lost), which makes equal units that the first read found different by their text, and the second read shares them
    // and the write says them once. The first and second encodings may differ in that; the second and the third do not.
    let (third, _) = bytes_of(&again);
    assert_eq!(second, third, "the encoding is a fixed point after one read and one write");
}

fuzz_target!(|data: &[u8]| {
    let Some((&kind, rest)) = data.split_first() else { return };
    let Ok(el) = parse_with(rest, &limits()) else { return };
    let mut notes: Vec<DecodeNote> = Vec::new();
    match kind % 3 {
        0 => {
            let Ok(lists) = part_lists(&el) else { return };
            let pages: Vec<El> = lists.pages.iter().map(|(id, _)| page_part(*id)).collect();
            let masters: Vec<El> = lists.masters.iter().map(|(id, _)| master_part(*id)).collect();
            let (p, m): (Vec<&El>, Vec<&El>) = (pages.iter().collect(), masters.iter().collect());
            if let Ok(doc) = decode(&el, &p, &m, &Blobs, &mut budget(), &mut notes) {
                exercise(doc);
            }
        }
        1 => {
            if let Ok(page) = decode_page(&el, &Blobs, &mut budget(), &mut notes) {
                let mut doc = Document::empty();
                doc.pages.push(Arc::new(page));
                exercise(doc);
            }
        }
        _ => {
            if let Ok(master) = decode_master(&el, &Blobs, &mut budget(), &mut notes) {
                let mut doc = Document::empty();
                doc.masters.push(Arc::new(master));
                exercise(doc);
            }
        }
    }
});
