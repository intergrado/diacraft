//! Blobs held in memory: the names of their entries, and a sink and source that keep them in a list.
//!
//! [`blob_path`] is the one place that says what the entry of a blob is called (`blobs/<n>.<ext>`, the design's 1.1 and the plan's NP4), so that
//! the package and the codec cannot disagree. [`MemBlobs`] stores each distinct byte string once, numbered by first use, and is both a
//! [`BlobSink`] and a [`BlobSource`]: the tests of the codec and the stamp of a model's content use it, and a package may use it before it
//! writes the entries.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::stamp::{Stamp, stamp_of_bytes};
use super::{BlobId, BlobSink, BlobSource, CodecError};

/// The extension of an entry that holds bytes of `media_type`: from a fixed list of those the model holds, and `bin` for any other, so that no
/// name comes from a file's own words.
pub fn blob_extension(media_type: &str) -> &'static str {
    let essence = media_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match essence.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        other if other.ends_with("+xml") || other == "application/xml" || other == "text/xml" => "xml",
        _ => "bin",
    }
}

/// The name of the entry that holds blob `id` of `media_type`: `blobs/<n>.<ext>`.
pub fn blob_path(id: BlobId, media_type: &str) -> String {
    format!("blobs/{}.{}", id.0, blob_extension(media_type))
}

/// Most blobs a [`MemBlobs`] holds: a package's most entries.
const MAX_BLOBS: usize = diacraft_container::limits::MAX_PARTS;

/// A list of blobs in memory: each distinct byte string once, in the order of first use. Equal bytes are found by size, then by the stamp of the
/// bytes, then by the bytes themselves, so that a hash collision never joins two byte strings.
#[derive(Debug, Default)]
pub struct MemBlobs {
    blobs: Vec<(Arc<[u8]>, String)>,
    by_stamp: HashMap<(usize, Stamp), Vec<u32>>,
    by_pointer: HashMap<(usize, usize), u32>,
    lost: HashSet<u32>,
}

impl MemBlobs {
    /// An empty list.
    pub fn new() -> MemBlobs {
        MemBlobs::default()
    }

    /// How many blobs.
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// The bytes of blob `id` and its media type.
    pub fn entry(&self, id: BlobId) -> Option<(&Arc<[u8]>, &str)> {
        let at = usize::try_from(id.0).ok()?;
        self.blobs.get(at).map(|(bytes, media)| (bytes, media.as_str()))
    }

    /// Loses blob `id`, as a damaged package would: [`BlobSource::get`] then gives `None`. For the tests of a lost blob.
    pub fn lose(&mut self, id: BlobId) {
        self.lost.insert(id.0);
    }

    /// Puts other bytes in the place of blob `id`'s, which a source that lies would give. For the tests of a record that disagrees with its entry.
    pub fn replace(&mut self, id: BlobId, bytes: Arc<[u8]>) {
        if let Some(slot) = usize::try_from(id.0).ok().and_then(|at| self.blobs.get_mut(at)) {
            slot.0 = bytes;
            // What was found by pointer or by stamp is not what is held any more.
            self.by_stamp.clear();
            self.by_pointer.clear();
        }
    }
}

fn pointer_key(bytes: &Arc<[u8]>) -> (usize, usize) {
    (Arc::as_ptr(bytes).cast::<u8>().addr(), bytes.len())
}

impl BlobSink for MemBlobs {
    fn put(&mut self, bytes: &Arc<[u8]>, media_type: &str) -> Result<BlobId, CodecError> {
        // The pointer of an `Arc` this list holds is its own for as long as it holds it: no other allocation can have the address.
        if let Some(id) = self.by_pointer.get(&pointer_key(bytes)) {
            return Ok(BlobId(*id));
        }
        let stamp = stamp_of_bytes(bytes);
        let key = (bytes.len(), stamp);
        if let Some(ids) = self.by_stamp.get(&key) {
            for id in ids {
                if let Some((held, _)) = usize::try_from(*id).ok().and_then(|at| self.blobs.get(at))
                    && **held == **bytes
                {
                    return Ok(BlobId(*id));
                }
            }
        }
        if self.blobs.len() >= MAX_BLOBS {
            return Err(CodecError::Limit { what: "blobs", max: u64::try_from(MAX_BLOBS).unwrap_or(u64::MAX) });
        }
        let id = u32::try_from(self.blobs.len()).map_err(|_| CodecError::Limit { what: "blobs", max: u64::from(u32::MAX) })?;
        self.blobs.push((Arc::clone(bytes), media_type.to_owned()));
        self.by_stamp.entry(key).or_default().push(id);
        self.by_pointer.insert(pointer_key(bytes), id);
        Ok(BlobId(id))
    }
}

impl BlobSource for MemBlobs {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        if self.lost.contains(&id.0) {
            return None;
        }
        self.entry(id).map(|(bytes, _)| Arc::clone(bytes))
    }
}
