//! `settings.xml` written from the model (the design's §3.4 row for it): **the tree the model holds, whole** (`OdfDoc::settings`, as the file had it), or, for a model that holds none, the
//! default part of [`crate::base`]. A tree larger than the reader reads as fields (a small part) is refused in words: it would read back as a package entry of bytes and not as it is
//! written, and the model that holds one holds it as bytes already ([`diacraft_doc::Part`]).
//!
//! DiaCraft's own items of the settings (the drawing scale, snap and grid) have no item of their own here: the scale of a page is carried by the document file with the page's cells, as the
//! splice writer carries it, and nothing of the snap options is written into the part. A later task writes them natively.

use diacraft_doc::Document;

use super::{WORDS_NAMES, WORDS_SMALL_PART, bounded_element, ensure_declarations, set_version};
use crate::base::VERSION;
use crate::limits::Limits;
use crate::write::{WriteError, WriteWork};

const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

/// The name of the part.
pub(crate) const PART: &str = "settings.xml";

/// `settings.xml` as the model holds it, or `None` when the model holds no tree.
pub(super) fn write(doc: &Document, limits: &Limits, work: &mut WriteWork) -> Result<Option<Vec<u8>>, WriteError> {
    let Some(tree) = doc.ext.get().and_then(|d| d.odf.settings.as_ref()) else { return Ok(None) };
    let cap = usize::try_from(limits.max_small_part_bytes).unwrap_or(usize::MAX);
    if !bounded_element(tree, cap) {
        return Err(WriteError::Unsupported(WORDS_SMALL_PART));
    }
    let mut root = diacraft_sheetxml::passthrough::to_tree(tree);
    // The part says the version the package says (`set_version`).
    let declared = ensure_declarations(&mut root.attrs, &[("office", OFFICE_URI)])?;
    set_version(&mut root.attrs, &declared, OFFICE_URI, VERSION);
    let bytes = diacraft_container::xml::to_bytes(&root, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", b"\n", true)
        .map_err(|_| WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_copy(bytes.len(), limits)?;
    if bytes.len() as u64 > limits.max_small_part_bytes {
        return Err(WriteError::Unsupported(WORDS_SMALL_PART));
    }
    Ok(Some(bytes))
}
