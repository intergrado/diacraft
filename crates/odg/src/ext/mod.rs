//! DiaCraft's own data in a drawing: tier 2 on a shape's own automatic style, and the one document-level file (the design's decisions 5, 6 and 7; P9).
//!
//! * [`names`]: every name DiaCraft's extension uses, each an `NCName` in `urn:app.diacraft:extension`, and the escaping of a cell's or
//!   a section's name into one.
//! * [`payload`]: what a shape's style carries as a value ([`payload::Tier2`]), its encoding in attributes (cells, sections and text as XML fragments
//!   written by the codec of the ShapeSheet), the reader of them and how they are put over what the reader rebuilt from the native markup.
//!
//! **Tier 2 is only what differs.** The reader rebuilds a shape from its native markup, and a cell, a row or a section
//! is tier 2 only when the rebuild would not give it as the model holds it. Reading
//! puts the carried data over the rebuild: first the removals, then the replacements.

pub(crate) mod body;
pub(crate) mod docfile;
pub(crate) mod fingerprint;
pub(crate) mod items;
pub(crate) mod names;
pub(crate) mod native;
pub(crate) mod odf14;
pub(crate) mod payload;
pub(crate) mod shell;
pub(crate) mod tier3;

#[cfg(test)]
mod model_tags;

/// Whether two shapes' texts are the same text to a save: the same parts. **The `.odg` data in a text's slot** (the style each paragraph and run named, with its stamps)
/// **is no part of it**: a text the model made has none, and the same text read from a file has some, and a save writes what the parts say. The markup nodes the reader kept whole
/// (`TextPart::OdfRaw`) are parts, and are compared.
pub(crate) fn text_same(a: Option<&std::sync::Arc<diacraft_doc::Text>>, b: Option<&std::sync::Arc<diacraft_doc::Text>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y) || x.parts == y.parts,
        _ => false,
    }
}
