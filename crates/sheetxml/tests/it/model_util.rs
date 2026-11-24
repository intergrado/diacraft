//! Helpers of the codec's tests: a model through the codec and the XML bytes the way a package would hold it, and back.

use diacraft_container::Limits;
use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_container::xml::{El, XmlLimits, parse_with, to_bytes};
use diacraft_doc::Document;
use diacraft_sheetxml::model::{BlobSource, Budget, CodecError, DecodeNote, Encoded, MemBlobs, decode, encode};

/// The limits a native part is parsed at: the container's depth ceiling, not the default 200.
pub fn native_limits() -> XmlLimits {
    XmlLimits { max_depth: XML_DEPTH_CEILING, ..XmlLimits::default() }
}

pub fn bytes_of_el(e: &El) -> Vec<u8> {
    to_bytes(e, &[], &[], true).unwrap()
}

/// What a package would hold of a model: each part's bytes, and the blobs.
pub struct Packed {
    pub document: Vec<u8>,
    pub pages: Vec<Vec<u8>>,
    pub masters: Vec<Vec<u8>>,
    pub blobs: MemBlobs,
}

impl Packed {
    /// Every part's bytes, in the order a package stores them.
    pub fn all_bytes(&self) -> Vec<&[u8]> {
        let mut out: Vec<&[u8]> = vec![&self.document];
        out.extend(self.pages.iter().map(Vec::as_slice));
        out.extend(self.masters.iter().map(Vec::as_slice));
        out
    }
}

pub fn pack_encoded(e: &Encoded, blobs: MemBlobs) -> Packed {
    Packed {
        document: bytes_of_el(&e.document),
        pages: e.pages.iter().map(|(_, el)| bytes_of_el(el)).collect(),
        masters: e.masters.iter().map(|(_, el)| bytes_of_el(el)).collect(),
        blobs,
    }
}

pub fn pack(doc: &Document) -> Packed {
    let mut blobs = MemBlobs::new();
    let e = encode(doc, &mut blobs).unwrap();
    pack_encoded(&e, blobs)
}

/// Decodes `p` through the real XML reader at native limits, with `budget`.
pub fn unpack_with(p: &Packed, blobs: &dyn BlobSource, budget: &mut Budget) -> Result<(Document, Vec<DecodeNote>), CodecError> {
    let limits = native_limits();
    let document = parse_with(&p.document, &limits).unwrap();
    let pages: Vec<El> = p.pages.iter().map(|b| parse_with(b, &limits).unwrap()).collect();
    let masters: Vec<El> = p.masters.iter().map(|b| parse_with(b, &limits).unwrap()).collect();
    let page_refs: Vec<&El> = pages.iter().collect();
    let master_refs: Vec<&El> = masters.iter().collect();
    let mut notes = Vec::new();
    let doc = decode(&document, &page_refs, &master_refs, blobs, budget, &mut notes)?;
    Ok((doc, notes))
}

pub fn unpack(p: &Packed) -> Result<(Document, Vec<DecodeNote>), CodecError> {
    unpack_with(p, &p.blobs, &mut Budget::for_open(&Limits::default()))
}

/// The model through encode, bytes, parse and decode.
pub fn round_trip(doc: &Document) -> Document {
    let (out, notes) = unpack(&pack(doc)).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    out
}
