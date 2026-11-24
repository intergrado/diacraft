//! The names of the entries of a package, and the judge that holds a read to the rules of names (the public spec's 2.1, 2.2 and 2.5; the plan's NP4).
//!
//! **The rules of a name** (2.1): UTF-8, relative, `/` between segments; not empty, no leading `/`, no `\`, no empty segment (so no trailing `/` on a file), no segment
//! `.` or `..`, at most 1,024 bytes; a name that is not ASCII has the UTF-8 flag (bit 11) set; no two entries have one name, compared byte for byte. The container's
//! [`Plain`] judge holds some of these (traversal, empty, length, duplicates), and this adds the rest. **A folder is ignored** (2.1): its row keeps its place in the
//! layout check, and is never named, read or listed.

use diacraft_container::ContainerError;
use diacraft_container::limits::Limits;
use diacraft_container::work::Work;
use diacraft_container::zip::{Entry, Judge, Plain, Verdict};

/// The first entry: the media type of the file.
pub(crate) const MIMETYPE: &str = "mimetype";
/// The manifest.
pub(crate) const MANIFEST: &str = "manifest.xml";
/// The document part.
pub(crate) const DOCUMENT: &str = "document.xml";
/// The state part.
pub(crate) const STATE: &str = "state.xml";
/// The preview picture.
pub(crate) const PREVIEW: &str = "preview.png";

/// The roles of the entries this version knows (2.4): the words a manifest's `role` holds.
pub(crate) mod role {
    pub(crate) const DOCUMENT: &str = "document";
    pub(crate) const PAGE: &str = "page";
    pub(crate) const MASTER: &str = "master";
    pub(crate) const BLOB: &str = "blob";
    pub(crate) const STATE: &str = "state";
    pub(crate) const PREVIEW: &str = "preview";

    /// Whether `word` is the role of an entry this version reads as itself.
    pub(crate) fn is_known(word: &str) -> bool {
        matches!(word, DOCUMENT | PAGE | MASTER | BLOB | STATE | PREVIEW)
    }
}

/// The bit of a ZIP entry's flags that says its name is UTF-8.
const FLAG_UTF8_NAME: u16 = 0x0800;

/// The most bytes the media type entry is allowed (the longest media type is 33).
const MIMETYPE_MAX: u64 = 64;

/// Whether a name is one of an entry of a package (2.1), for a name of a file: the rules above, and no character XML 1.0 forbids, since a manifest names it.
pub(crate) fn valid_entry_name(name: &str, max_bytes: usize) -> bool {
    if name.is_empty() || name.len() > max_bytes || name.starts_with('/') || name.contains('\\') {
        return false;
    }
    if diacraft_container::xml::first_bad_char(name).is_some() {
        return false;
    }
    name.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Whether an entry named `name` holds a part of XML by the rules of the format: the manifest, the document part, the state, and the page and master parts under
/// their folders. A part the document part lists under another name is held to the same cap by the reader, from the size its entry states.
fn is_xml_entry(name: &str) -> bool {
    name == MANIFEST || name == DOCUMENT || name == STATE || name.starts_with("pages/") || name.starts_with("masters/")
}

/// The judge of a read: the container's plain names, and the rules above, with the cap of each entry (an XML part is held to the part cap, the media type to 64
/// bytes, anything else to what the container allows of an entry).
pub(crate) struct NativeJudge {
    plain: Plain,
    xml_cap: u64,
    entry_cap: u64,
    max_name: usize,
}

impl NativeJudge {
    /// The judge for one read within `limits`, holding an XML part to `xml_cap` bytes.
    pub(crate) fn new(limits: &Limits, xml_cap: u64) -> NativeJudge {
        NativeJudge {
            plain: Plain::new(limits),
            xml_cap: xml_cap.min(limits.max_part_bytes),
            entry_cap: limits.max_part_bytes,
            max_name: limits.max_part_name_bytes,
        }
    }
}

impl Judge for NativeJudge {
    type Error = ContainerError;

    fn entry(&mut self, entry: &Entry, kept: &[Entry]) -> Result<Verdict, ContainerError> {
        if entry.folder {
            return Ok(Verdict::Drop);
        }
        // The container's names first (a name that would leave the package, an empty one, a long one, a Unicode path that disagrees), then the rest of 2.1.
        let verdict = self.plain.entry(entry, kept)?;
        let invalid = |reason: &'static str| ContainerError::InvalidPartName { name: entry.name.chars().take(64).collect(), reason };
        if entry.name.split('/').any(|segment| segment.is_empty() || segment == ".") {
            return Err(invalid("an empty segment or a segment that is a dot"));
        }
        if !entry.name.is_ascii() && entry.flags & FLAG_UTF8_NAME == 0 {
            return Err(invalid("a name that is not ASCII and does not say it is UTF-8"));
        }
        if entry.name.len() > self.max_name {
            return Err(invalid("longer than the limit"));
        }
        let Verdict::Keep { rename, counted, .. } = verdict else { return Ok(verdict) };
        let cap = if entry.name == MIMETYPE {
            MIMETYPE_MAX
        } else if is_xml_entry(&entry.name) {
            self.xml_cap
        } else {
            self.entry_cap
        };
        Ok(Verdict::Keep { rename, cap, counted })
    }

    fn listed(&mut self, kept: &[Entry], work: &mut Work) -> Result<(), ContainerError> {
        self.plain.listed(kept, work)
    }
}
