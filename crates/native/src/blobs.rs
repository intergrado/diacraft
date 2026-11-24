//! Blobs: every byte string the model holds, stored once (the public spec's 2.6 and 3.16).
//!
//! **Numbering.** A blob has a number, the order of its first use while the parts are written: the document part, then the pages in order, then the masters in order
//! (2.9). The same model always gets the same numbers, and a part is named by model ID, so that deleting one page renames no other page's part.
//!
//! **Equal bytes are one blob.** A writer finds equal byte strings by **size, then stamp, then the bytes themselves**, so that a stamp that happens to be equal never
//! joins two different strings (FNV over 64 bits is easy to collide on purpose). The media type takes no part in this: a byte string used under two media types is one
//! blob, and its record carries the media type of its first use. [`BlobTable::with_hash`] is the seam a test uses to make every stamp equal.
//!
//! **Storing.** A blob whose first bytes are a PNG, JPEG or GIF signature or `PK\x03\x04` is stored (method 0), since these are compressed already; any other blob is
//! deflated at the package's one level ([`stored`]). **A blob is never parsed, opened as a package or run** (1.3, 2.6): a macro project or an embedded object that a
//! drawing holds is copied as bytes and nothing else.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::limits::MAX_PARTS;
use diacraft_sheetxml::model::{BlobId, BlobSink, BlobSource, CodecError, Stamp, stamp_of_bytes};

/// Whether a blob is stored and not deflated: its first bytes are a PNG signature, a JPEG signature, a GIF signature, or `PK\x03\x04`.
pub fn stored(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"PK\x03\x04")
}

/// The blobs of a model being written: each distinct byte string once, in the order of first use, and the numbers the codec names them by. Both the sink the codec
/// puts byte strings into and the source its check of the save reads them from.
pub struct BlobTable {
    entries: Vec<(Arc<[u8]>, String)>,
    by_stamp: HashMap<(usize, Stamp), Vec<u32>>,
    by_pointer: HashMap<(usize, usize), u32>,
    hash: fn(&[u8]) -> Stamp,
}

impl Default for BlobTable {
    fn default() -> BlobTable {
        BlobTable::new()
    }
}

impl BlobTable {
    /// An empty table that finds equal bytes by the stamp of the format (FNV-1a over 64 bits, 3.17).
    pub fn new() -> BlobTable {
        BlobTable::with_hash(stamp_of_bytes)
    }

    /// An empty table that finds equal bytes by `hash` in the place of the stamp: a seam for the tests that a stamp equal for two byte strings joins neither, since
    /// the bytes are compared after it. Never used by a writer.
    pub fn with_hash(hash: fn(&[u8]) -> Stamp) -> BlobTable {
        BlobTable { entries: Vec::new(), by_stamp: HashMap::new(), by_pointer: HashMap::new(), hash }
    }

    /// How many blobs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bytes of blob `id` and the media type of its first use.
    pub fn entry(&self, id: BlobId) -> Option<(&Arc<[u8]>, &str)> {
        let at = usize::try_from(id.0).ok()?;
        self.entries.get(at).map(|(bytes, media)| (bytes, media.as_str()))
    }

    /// The very allocation held for blob `id`: what a part that names it must have named when it was read, for it to be copied as it is (Review Focus 1).
    pub fn arc(&self, id: u32) -> Option<&Arc<[u8]>> {
        self.entry(BlobId(id)).map(|(bytes, _)| bytes)
    }
}

fn pointer_key(bytes: &Arc<[u8]>) -> (usize, usize) {
    (Arc::as_ptr(bytes).cast::<u8>().addr(), bytes.len())
}

impl BlobSink for BlobTable {
    fn put(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError> {
        // The address of an `Arc` this table holds is its own for as long as it holds it: no other allocation can have it.
        if let Some(id) = self.by_pointer.get(&pointer_key(bytes)) {
            return Ok(BlobId(*id));
        }
        let key = (bytes.len(), (self.hash)(bytes));
        if let Some(ids) = self.by_stamp.get(&key) {
            for id in ids {
                if let Some((held, _)) = usize::try_from(*id).ok().and_then(|at| self.entries.get(at))
                    && **held == **bytes
                {
                    // Not entered by address: this table does not hold `bytes`, and an allocation the model lets go of may be given to other bytes.
                    return Ok(BlobId(*id));
                }
            }
        }
        if self.entries.len() >= MAX_PARTS {
            return Err(CodecError::Limit { what: "blobs", max: u64::try_from(MAX_PARTS).unwrap_or(u64::MAX) });
        }
        let id = u32::try_from(self.entries.len()).map_err(|_| CodecError::Limit { what: "blobs", max: u64::from(u32::MAX) })?;
        self.entries.push((Arc::clone(bytes), media_type.to_owned()));
        self.by_stamp.entry(key).or_default().push(id);
        self.by_pointer.insert(pointer_key(bytes), id);
        Ok(BlobId(id))
    }
}

impl BlobSource for BlobTable {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.arc(id.0).cloned()
    }
}
