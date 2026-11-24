//! What a read (or a write) remembers of the package it made a model from, so that the next write can copy what did not change (the public spec's 2.9).
//!
//! **A save that changed nothing writes the file it read, and a save that changed one page rewrites that page's part and no other.** For that the write needs, for
//! each page and master, the very `Arc` the model holds for it when the file was read or written, the entry that held it (its name, size and checksum, as the
//! archive states and the reader verified), and the blobs its part names, each with the `Arc` of its bytes: a part is copied as it is **only when its `Arc` is still
//! the one decoded and every blob it names is the very allocation it was** (Review Focus 1: blobs are numbered by first use, so adding a picture to page 1 renumbers
//! the pictures, and a page 3 that is copied as it is would show the wrong one). A stamp or a size is never trusted for this, since a hash over 64 bits is easy to
//! collide on purpose.
//!
//! A [`Decoded`] is cheap to hold (reference counts and a few strings for each part) and says nothing a caller can read but how many parts it remembers: it is
//! given back to [`write`](crate::write) beside the bytes it was made from, and it is only ever used with them.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::{El, Node, XmlText};
use diacraft_doc::{Master, Page};
use diacraft_sheetxml::model::{Digest, Stamp};

/// One entry of a package as its archive states it, and as a read verified it (the size is what inflated, and the checksum is the bytes').
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryRecord {
    pub name: String,
    pub crc: u32,
    pub size: u64,
}

/// What a part's bytes are, as the three numbers a write compares them by: the stamp of the bytes, their length and their CRC-32 (a checksum is made to collide on purpose, and a stamp over
/// the same bytes does not collide with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Print {
    pub stamp: Stamp,
    pub size: u64,
    pub crc: u32,
}

/// A page or a master part: the model object as decoded, the entry that held it, and the numbers of the blobs its part names.
#[derive(Clone, Debug)]
pub(crate) struct PartRecord<T> {
    pub id: u32,
    pub entry: EntryRecord,
    pub object: Arc<T>,
    /// The blob numbers the part's elements name, sorted and without repeats; `None` when the part names more than a package has blobs (nothing is then copied).
    pub blobs: Option<Vec<u32>>,
}

/// A blob as read or written: the entry that held it and the allocation its bytes are held in.
#[derive(Clone, Debug)]
pub(crate) struct BlobRecord {
    pub entry: EntryRecord,
    pub bytes: Arc<[u8]>,
}

/// A part of a role this version does not know: kept by its name, the entry that held it and the allocation of its bytes.
#[derive(Clone, Debug)]
pub(crate) struct ExtraRecord {
    pub entry: EntryRecord,
    pub bytes: Arc<[u8]>,
}

/// What was read or written of a package, for the next write to copy from. See the [module](self).
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub(crate) document: Option<EntryRecord>,
    /// The stamp of the document part's bytes (3.17), taken beside its checksum: a write copies the preview only when both are what they were.
    pub(crate) document_stamp: Option<Stamp>,
    /// What the document part's bytes would be if every spelling of the namespace of the builds before the id moved to `diacraft.app` in it were the current one, when it holds one:
    /// a write puts the current namespace in the part, so that a save of a drawing that did not change finds the file's part as it would write it, and copies the preview.
    pub(crate) document_as_current: Option<Print>,
    pub(crate) pages: Vec<PartRecord<Page>>,
    pub(crate) masters: Vec<PartRecord<Master>>,
    pub(crate) blobs: HashMap<u32, BlobRecord>,
    pub(crate) extras: Vec<ExtraRecord>,
    pub(crate) preview: Option<EntryRecord>,
}

impl Decoded {
    /// How many page parts are remembered.
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// How many master parts are remembered.
    pub fn masters(&self) -> usize {
        self.masters.len()
    }

    /// How many blobs are remembered: those a part of the package named and whose bytes were found.
    pub fn blobs(&self) -> usize {
        self.blobs.len()
    }

    /// How many parts of an unknown role are remembered.
    pub fn extras(&self) -> usize {
        self.extras.len()
    }

    /// Whether the package remembered holds a preview picture: a write that has none to give copies this one when the model is as it was, and writes none when the package had none (the
    /// engine asks, to make a picture for a package that has none to copy).
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
}

/// The most distinct blob numbers one part may be remembered as naming: a package's most entries. A part that names more is never copied as it is.
const MAX_NAMED: usize = diacraft_container::limits::MAX_PARTS;

/// The numbers of the blobs a part's elements name: the value of every attribute named `blob`, whatever the element, sorted and without repeats. **It reads more
/// than it must** (an unknown element of a newer version that has an attribute of that name is counted), which is the safe side: a part that is thought to name a
/// blob it does not is copied less often, and one that names a blob is never thought to name none. `None` when there are more than [`MAX_NAMED`]. **Only the tests use it now**: a part is
/// read as a stream and scanned ([`blobs_named_bytes`]), and the tests hold the scan to what a tree says.
#[cfg(test)]
pub(crate) fn blobs_named(tree: &El) -> Option<Vec<u32>> {
    let mut named: Vec<u32> = Vec::new();
    let mut stack: Vec<&El> = vec![tree];
    while let Some(el) = stack.pop() {
        for (name, value) in &el.attrs {
            if name == "blob" {
                match value.parse::<u32>() {
                    Ok(id) => named.push(id),
                    // A value that is no number names no blob the decoder would find, and the part is not one to copy as it is.
                    Err(_) => return None,
                }
            }
        }
        stack.extend(el.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }));
        if named.len() > 4 * MAX_NAMED {
            named.sort_unstable();
            named.dedup();
            if named.len() > MAX_NAMED {
                return None;
            }
        }
    }
    named.sort_unstable();
    named.dedup();
    (named.len() <= MAX_NAMED).then_some(named)
}

/// The numbers of the blobs a part's **bytes** name, as [`blobs_named`] says them of its tree, **found by a scan of the text and not a parse of it** (a page part is read as a stream of
/// its children, and no tree of it is ever made): every attribute named `blob`, wherever it stands, sorted and without repeats. **It reads more than it must, and never less:**
/// a text that holds ` blob="7"` (in a comment, a value or an element's text) is counted as an attribute, a value that is no number is `None` (the part is not copied as it is),
/// and an attribute named `blob` is found whatever the whitespace around its `=` and whichever quote it uses. The text is the decoded one, so a part in UTF-16 is scanned as the
/// text it is. `None` when the bytes are no text, or the part names more than a package has blobs.
pub(crate) fn blobs_named_bytes(bytes: &[u8]) -> Option<Vec<u32>> {
    let text = XmlText::new(bytes).ok()?;
    let text = text.as_str().as_bytes();
    let mut named: Vec<u32> = Vec::new();
    let mut from = 0usize;
    while let Some(found) = find(text, b"blob", from) {
        from = found + 4;
        // An attribute's name stands after whitespace, and is exactly `blob`: the byte after it is whitespace or `=`.
        if found == 0 || !text.get(found - 1).is_some_and(u8::is_ascii_whitespace) {
            continue;
        }
        let mut at = found + 4;
        while text.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if text.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while text.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let Some(&quote) = text.get(at).filter(|q| **q == b'"' || **q == b'\'') else { continue };
        let rest = text.get(at + 1..)?;
        let end = rest.iter().position(|b| *b == quote)?;
        let value = std::str::from_utf8(rest.get(..end)?).ok()?;
        named.push(value.parse::<u32>().ok()?);
        if named.len() > 4 * MAX_NAMED {
            named.sort_unstable();
            named.dedup();
            if named.len() > MAX_NAMED {
                return None;
            }
        }
        from = at + 1 + end;
    }
    named.sort_unstable();
    named.dedup();
    (named.len() <= MAX_NAMED).then_some(named)
}

/// The position of `needle` in `text` at or after `from`.
fn find(text: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    text.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// The items of a tree as the XML reader counts them: each element, attribute, text, comment and instruction.
pub(crate) fn items_of(tree: &El) -> u64 {
    let mut total = 0u64;
    let mut stack: Vec<&El> = vec![tree];
    while let Some(el) = stack.pop() {
        total = total.saturating_add(1).saturating_add(u64::try_from(el.attrs.len()).unwrap_or(u64::MAX));
        for node in &el.children {
            match node {
                Node::El(child) => stack.push(child),
                _ => total = total.saturating_add(1),
            }
        }
    }
    total
}

/// The print of `bytes`, which the tests hold [`print_as_current`] to (the write takes its stamp from the part it makes, once).
#[cfg(test)]
fn print_of(bytes: &[u8]) -> Print {
    Print {
        stamp: diacraft_sheetxml::model::stamp_of_bytes(bytes),
        size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        crc: crc32fast::hash(bytes),
    }
}

/// The print `bytes` would have if every spelling of the namespace that builds made before the id moved to `diacraft.app` wrote (`diacraft_doc::DIACRAFT_URN_LEGACY`) in it were the
/// current one, which is the part a write of the same drawing makes: `None` when the bytes hold none, and when they are no UTF-8 text (a part in another coding is not searched, which
/// only keeps a save from finding it unchanged, as it was). **Nothing is copied**: the text is searched twice, in time linear in its length, and its pieces are fed to the stamp and the
/// checksum where they stand, with the current spelling between them.
pub(crate) fn print_as_current(bytes: &[u8]) -> Option<Print> {
    let (old, new) = (diacraft_doc::DIACRAFT_URN_LEGACY, diacraft_doc::DIACRAFT_URN);
    let text = std::str::from_utf8(bytes).ok()?;
    let count = u64::try_from(text.matches(old).count()).ok()?;
    if count == 0 {
        return None;
    }
    // The current spelling is the shorter of the two, so the part is not longer for it; the length is the part's less what each spelling gives up.
    let shorter = u64::try_from(old.len().checked_sub(new.len())?).ok()?;
    let size = u64::try_from(bytes.len()).ok()?.checked_sub(count.checked_mul(shorter)?)?;
    let pieces = || {
        text.split(old).enumerate().flat_map(|(at, piece)| {
            let lead: Option<&[u8]> = (at > 0).then_some(new.as_bytes());
            lead.into_iter().chain(std::iter::once(piece.as_bytes()))
        })
    };
    let stamp = Digest::new("bytes").bytes_in_pieces(size, pieces()).finish();
    let mut crc = crc32fast::Hasher::new();
    for piece in pieces() {
        crc.update(piece);
    }
    Some(Print { stamp, size, crc: crc.finalize() })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_container::xml::{XmlLimits, parse_with};

    use super::*;

    #[test]
    fn the_print_as_current_is_the_print_of_the_part_with_the_spelling_replaced() {
        let (old, new) = (diacraft_doc::DIACRAFT_URN_LEGACY, diacraft_doc::DIACRAFT_URN);
        for text in [
            format!("<d:document xmlns:d=\"{old}\"/>"),
            old.to_owned(),
            format!("{old}{old}"),
            format!("a{old}b{old}c{old}"),
            format!("<a x=\"{old}:blob\">{old}</a>"),
            format!("{} {}", &old[..old.len() - 1], &old[1..]),
        ] {
            let replaced = text.replace(old, new);
            let held = print_as_current(text.as_bytes());
            if text.contains(old) {
                assert_eq!(held, Some(print_of(replaced.as_bytes())), "{text}");
            } else {
                assert_eq!(held, None, "{text}");
            }
        }
        // None when there is none, when the text is no text, and for nothing at all.
        assert_eq!(print_as_current(format!("<d xmlns:d=\"{new}\"/>").as_bytes()), None);
        assert_eq!(print_as_current(&[0xFF, 0xFE, 0x00]), None);
        assert_eq!(print_as_current(b""), None);
        // The spelling is the shorter one after, and a part that holds many has the length it would have.
        let many = old.repeat(1000);
        assert_eq!(print_as_current(many.as_bytes()).map(|p| p.size), Some(u64::try_from(new.len()).unwrap() * 1000));
    }

    /// What the scan of the bytes says of `xml`, and what the names of its tree say.
    fn both(xml: &str) -> (Option<Vec<u32>>, Option<Vec<u32>>) {
        let tree = parse_with(xml.as_bytes(), &XmlLimits::default()).unwrap();
        (blobs_named_bytes(xml.as_bytes()), blobs_named(&tree))
    }

    #[test]
    fn the_scan_of_a_parts_bytes_finds_the_blob_numbers_its_tree_names_whatever_the_quotes_and_the_blanks() {
        for xml in [
            "<a/>",
            "<a blob=\"3\"/>",
            "<a><b blob='7'/><c blob = \"2\"/><d\n blob\t=\n'9'/><e blob=\"3\"/></a>",
            "<a x=\"1\" blob=\"4294967295\"><b blob=\"0\"/></a>",
            // A name that only ends in or starts with `blob` is no attribute named it.
            "<a data-blob=\"5\" blobs=\"6\" blob2=\"7\" xblob=\"8\"/>",
            // A value that is no number: the part is not one to copy as it is, whichever way it is read.
            "<a blob=\"x\"/>",
            "<a blob=\"-1\"/>",
            "<a blob=\"\"/>",
            "<a blob=\"4294967296\"/>",
        ] {
            let (scanned, tree) = both(xml);
            assert_eq!(scanned, tree, "{xml}");
        }
    }

    #[test]
    fn the_scan_reads_more_than_the_tree_and_never_less() {
        // A comment or a text that holds what looks like an attribute is counted, which only keeps a part from being copied; one that names nothing is not.
        assert_eq!(blobs_named_bytes(b"<a><!-- <b blob=\"4\"/> --></a>"), Some(vec![4]));
        assert_eq!(blobs_named_bytes(b"<a>x blob=\"6\" y</a>"), Some(vec![6]));
        assert_eq!(blobs_named_bytes(b"<a>blob=\"6\"</a>"), Some(Vec::new()), "no blank before the name: not an attribute");
        // An unterminated value is no part to copy.
        assert_eq!(blobs_named_bytes(b"<a blob=\"6/>"), None);
        assert_eq!(blobs_named_bytes(b"<a blob=6/>"), Some(Vec::new()), "no quote: no value");
        // Bytes that are no text name no blob.
        assert_eq!(blobs_named_bytes(&[0xFF, 0xFE, 0x00]), None);
    }

    #[test]
    fn a_part_in_utf_16_is_scanned_as_the_text_it_is() {
        let text = "<a><b blob=\"11\"/><c blob='12'/></a>";
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(blobs_named_bytes(&utf16), Some(vec![11, 12]));
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(text.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(blobs_named_bytes(&be), Some(vec![11, 12]));
    }

    #[test]
    fn more_blob_numbers_than_a_package_has_entries_name_no_blobs_the_part_could_be_copied_by() {
        let many: String = (0..MAX_NAMED as u32 + 1).map(|i| format!("<b blob=\"{i}\"/>")).collect();
        assert_eq!(blobs_named_bytes(format!("<a>{many}</a>").as_bytes()), None);
        let fits: String = (0..MAX_NAMED as u32).map(|i| format!("<b blob=\"{i}\"/>")).collect();
        assert_eq!(blobs_named_bytes(format!("<a>{fits}</a>").as_bytes()).map(|v| v.len()), Some(MAX_NAMED));
    }
}
