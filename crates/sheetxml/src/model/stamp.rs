//! Stamps: the fingerprint of the model's content, one part at a time.
//!
//! A [`Stamp`] is the FNV-1a hash over 64 bits that the fingerprints use, each field of a [`Digest`] prefixed by its length as eight little-endian
//! bytes, so that two ways of cutting the same bytes into fields are two stamps (NP3). Both types are the document crate's (`diacraft_doc::stamp`), which
//! the readers and writers share with this codec.
//!
//! [`content_stamp`] is the fingerprint of a whole [`Document`]: each part the codec makes of it (the document, each page, each master),
//! walked and hashed one at a time, the tree dropped before the next is built, then the table of blobs by what each holds (size and the stamp of
//! its bytes), so that a picture replaced by other bytes of the same size is another content, and an undo that puts the old bytes back is the
//! first one again ("Blobs: bytes stored once"; the plan's `content_stamp_encodes_one_part_at_a_time`).
//!
//! **The stamp is over the content and not over how a part says it** (the spec's 3.17 and 5.5). A part that names a sheet or a section once and refers to it after
//! (`def` and `same`, 3.19) is walked **as the part of version 1.0 would have said it**: each `same` is the element it names, and no `def` is there, so that a drawing has
//! the stamp it had when each place held its own copy, and a stamp that was stored with a drawing is good for the file that says it once. The walk goes over the element
//! a `same` names each time it is met, without making a tree of it, so what it costs in time is what the drawing is once nothing is said once, and the items it
//! charges are those it walks.

use std::collections::HashMap;

use diacraft_container::limits::MAX_XML_NODES;
use diacraft_container::xml::{El, Node};
use diacraft_doc::Document;
pub use diacraft_doc::stamp::{Digest, Stamp};

use super::blobs::MemBlobs;
use super::encode::Encoder;
use super::stream::{ChildCap, PartSink};
use super::vocab::{at, el};
use super::{Budget, CodecError, PREFIX};

/// The stamp of a byte string: its length and its bytes, in a digest of their own.
pub fn stamp_of_bytes(bytes: &[u8]) -> Stamp {
    Digest::new("bytes").bytes(bytes).finish()
}

/// What a [`content_stamp_counted`] cost: the parts it made, the items of the codec's trees summed, and the largest tree held at one time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StampCost {
    /// Parts made: the document, each page and each master.
    pub parts: u64,
    /// The items of those parts' trees, summed (as the XML reader counts them).
    pub items: u64,
    /// The items of the largest part. A part is walked a child of its root at a time, so what is held at one time is one child's tree and no more.
    pub largest_part_items: u64,
}

/// The local name of an element of the codec's own.
fn ours(e: &El) -> Option<&str> {
    e.name.strip_prefix(PREFIX).and_then(|rest| rest.strip_prefix(':'))
}

/// Whether `e` is an element of the codec's own that gives itself a name (`def`): a `sheet` or a `Section`.
fn names_itself(e: &El) -> bool {
    matches!(ours(e), Some(el::SHEET | el::SECTION)) && e.attrs.iter().any(|(k, _)| k == at::DEF)
}

/// The element a `same` of the codec's own stands for, if this is a place where one stands and the table has a unit of the kind the place holds: in a `sheet` it
/// stands for a section, and in the document, a style, a page, a master or a shape, for a sheet. A `same` anywhere else is an element like any other, as the decoder
/// reads it.
fn named_by<'a>(parent: &El, e: &El, defs: &HashMap<&'a str, &'a El>) -> Option<&'a El> {
    if ours(e) != Some(el::SAME) {
        return None;
    }
    let want = match ours(parent)? {
        el::SHEET => el::SECTION,
        el::DOCUMENT | el::STYLE | el::PAGE | el::MASTER | el::SHAPE => el::SHEET,
        _ => return None,
    };
    let of = e.attrs.iter().find(|(k, _)| k == at::OF).map(|(_, v)| v.as_str())?;
    let named = defs.get(of).copied()?;
    (ours(named) == Some(want)).then_some(named)
}

/// Feeds the tree `root` into `digest` **as the part would be written if it said no unit once**: every element's name, attributes and children in order, text,
/// comments and instructions by their kind and bytes, each `same` as the element it names (walked again where it stands), and no `def` and no `fp`. The walk keeps its
/// own stack and does not recurse. A name is given **when its element is closed**, as the decoder gives it, so that a `same` names a unit that is complete and no walk
/// of a unit can come to itself: the walk is a tree of what was named before, however a hostile part names. It charges `budget` an item for each element,
/// attribute and node it feeds, before it feeds it, and gives how many that was.
fn feed_tree(digest: &mut Digest, root: &El, budget: &mut Budget) -> Result<u64, CodecError> {
    let mut fed = 0u64;
    let mut defs: HashMap<&str, &El> = HashMap::new();
    let mut stack: Vec<(&El, usize)> = Vec::new();
    open(digest, root, budget, &mut fed)?;
    stack.push((root, 0));
    while let Some((e, idx)) = stack.pop() {
        let Some(node) = e.children.get(idx) else {
            digest.text("/");
            if names_itself(e)
                && let Some((_, name)) = e.attrs.iter().find(|(k, _)| k == at::DEF)
            {
                defs.entry(name.as_str()).or_insert(e);
            }
            continue;
        };
        stack.push((e, idx.saturating_add(1)));
        match node {
            Node::El(child) => {
                // A `same` is the element it names, fed where it stands; one that names nothing is an element like any other (a part that this codec did not write).
                let shown = named_by(e, child, &defs).unwrap_or(child);
                open(digest, shown, budget, &mut fed)?;
                stack.push((shown, 0));
            }
            Node::Text(t) | Node::CData(t) => {
                budget.charge_items(1)?;
                fed = fed.saturating_add(1);
                digest.text("t").text(t);
            }
            Node::Comment(c) => {
                budget.charge_items(1)?;
                fed = fed.saturating_add(1);
                digest.text("c").text(c);
            }
            Node::Pi { target, data } => {
                budget.charge_items(1)?;
                fed = fed.saturating_add(1);
                digest.text("p").text(target).text(data);
            }
        }
    }
    Ok(fed)
}

/// An element's name, the number of its attributes that are fed (all of them but the `def` and the `fp` of a unit that names itself), and those attributes; charged
/// to `budget` and counted in `fed` as the items of the element and of the attributes it feeds.
fn open(digest: &mut Digest, e: &El, budget: &mut Budget, fed: &mut u64) -> Result<(), CodecError> {
    let skip = names_itself(e);
    let fed_attrs = |k: &str| !(skip && (k == at::DEF || k == at::FP));
    let attrs = e.attrs.iter().filter(|(k, _)| fed_attrs(k)).count();
    let items = 1u64.saturating_add(u64::try_from(attrs).unwrap_or(u64::MAX));
    budget.charge_items(items)?;
    *fed = fed.saturating_add(items);
    digest.text("<").text(&e.name).bytes(&u64::try_from(attrs).unwrap_or(u64::MAX).to_le_bytes());
    for (k, v) in &e.attrs {
        if fed_attrs(k) {
            digest.text(k).text(&as_version_1_0(v));
        }
    }
    Ok(())
}

/// A value as a stamp or a fingerprint feeds it: **the namespace is fed as the writer of version 1.0 wrote it** (APPID, and 5.5 of the format: a later version keeps every stamp
/// already stored). A value that is the current namespace of DiaCraft, or begins with it and a colon (the type of a relationship to a carried part), is fed with the earlier
/// namespace in its place, so that the stamp of a drawing is the one that a build before the namespace moved gave it, whichever of the two its parts say.
pub(super) fn as_version_1_0(value: &str) -> std::borrow::Cow<'_, str> {
    match value.strip_prefix(diacraft_doc::DIACRAFT_URN) {
        Some(rest) if rest.is_empty() || rest.starts_with(':') => std::borrow::Cow::Owned([diacraft_doc::DIACRAFT_URN_LEGACY, rest].concat()),
        _ => std::borrow::Cow::Borrowed(value),
    }
}

/// What a page or a master is fed to the digest through: its root's start tag, then each child, **as the part would be written if it said no unit once** (the encoder that
/// feeds it shares nothing, so there is no `def` and no `same` to expand). The walk of a child is [`feed_tree`]'s, and the child is let go before the next is made, so that what is
/// held is one child's tree and not the part's.
struct StampSink<'d, 'b> {
    digest: &'d mut Digest,
    budget: &'b mut Budget,
    fed: u64,
}

impl PartSink for StampSink<'_, '_> {
    fn head(&mut self, root: &El) -> Result<(), CodecError> {
        open(self.digest, root, self.budget, &mut self.fed)
    }

    fn child(&mut self, node: Node) -> Result<(), CodecError> {
        match node {
            Node::El(e) => {
                let items = feed_tree(self.digest, &e, self.budget)?;
                self.fed = self.fed.saturating_add(items);
            }
            Node::Text(t) | Node::CData(t) => {
                self.budget.charge_items(1)?;
                self.fed = self.fed.saturating_add(1);
                self.digest.text("t").text(&t);
            }
            Node::Comment(c) => {
                self.budget.charge_items(1)?;
                self.fed = self.fed.saturating_add(1);
                self.digest.text("c").text(&c);
            }
            Node::Pi { target, data } => {
                self.budget.charge_items(1)?;
                self.fed = self.fed.saturating_add(1);
                self.digest.text("p").text(&target).text(&data);
            }
        }
        Ok(())
    }
}

/// The content stamp of `doc`, and what it cost. See the module documentation. `budget` pays for the items of every element walked and for the bytes
/// of every distinct blob hashed (a blob that many shapes name is hashed once). **A page and a master are made a child of the root at a time**, each child
/// walked and let go, by an encoder that writes every unit in full: what is held is one child's tree, whatever the number of shapes, and the stamp is the one the walk of the
/// part that says no unit once has always given.
pub fn content_stamp_counted(doc: &Document, budget: &mut Budget) -> Result<(Stamp, StampCost), CodecError> {
    let mut cost = StampCost::default();
    let mut digest = Digest::new("content");
    let mut blobs = MemBlobs::new();
    let mut enc = Encoder::new(&mut blobs).without_sharing();
    enc.check_document(doc)?;
    let mut count = |items: u64| {
        cost.parts = cost.parts.saturating_add(1);
        cost.items = cost.items.saturating_add(items);
        cost.largest_part_items = cost.largest_part_items.max(items);
    };
    let (mut el, _) = enc.document_part(doc)?;
    enc.finish_document(&mut el, doc)?;
    count(feed_tree(&mut digest, &el, budget)?);
    drop(el);
    // The stamp walks a drawing the model holds, a child at a time: a child is held to the items of a tree that a container reads (the figure the whole tree of a part was held to before
    // pages were streams), so that a drawing that a save writes is a drawing that is stamped.
    let items = MAX_XML_NODES as u64;
    let child = ChildCap { items, what: "XML items in one element of a part", max: items, part: u64::MAX };
    for page in &doc.pages {
        let mut sink = StampSink { digest: &mut digest, budget: &mut *budget, fed: 0 };
        enc.page_stream(page, Some(child), &mut sink)?;
        sink.digest.text("/");
        let fed = sink.fed;
        count(fed);
    }
    for master in &doc.masters {
        let mut sink = StampSink { digest: &mut digest, budget: &mut *budget, fed: 0 };
        enc.master_stream(master, Some(child), &mut sink)?;
        sink.digest.text("/");
        let fed = sink.fed;
        count(fed);
    }
    for record in enc.records() {
        budget.charge_blob(record.size)?;
        digest.bytes(&u64::from(record.id.0).to_le_bytes()).text(&record.media_type).bytes(&record.size.to_le_bytes()).text(&record.stamp.text());
    }
    Ok((digest.finish(), cost))
}

/// The content stamp of `doc`: a fingerprint of what the document holds, whatever the file it came from or will go to. See the module
/// documentation.
pub fn content_stamp(doc: &Document, budget: &mut Budget) -> Result<Stamp, CodecError> {
    content_stamp_counted(doc, budget).map(|(stamp, _)| stamp)
}

#[cfg(test)]
mod tests {
    use diacraft_container::xml::{XmlLimits, parse_with};

    use super::*;
    use crate::model::URN;

    /// The body with the codec's own prefix (the walk reads the parts the codec writes).
    fn ours_prefix(body: &str) -> String {
        body.replace("<d:", "<diacraft:").replace("</d:", "</diacraft:")
    }

    fn walk(body: &str, items: u64) -> Result<u64, CodecError> {
        let text = format!("<diacraft:page xmlns:diacraft=\"{URN}\">{}</diacraft:page>", ours_prefix(body));
        let tree = parse_with(text.as_bytes(), &XmlLimits { max_depth: 512, ..XmlLimits::default() }).unwrap();
        let mut budget = Budget::new(items, u64::MAX, u64::MAX);
        feed_tree(&mut Digest::new("t"), &tree, &mut budget)
    }

    #[test]
    fn a_hostile_page_has_a_bounded_walk_a_name_is_given_when_its_unit_is_closed_and_a_same_stands_only_where_a_unit_does() {
        // The review's input: a section that holds a `same` for a name that a later unit gives, and a `same` for the section: no walk comes round to itself, at any
        // budget (the walk of the part is a tree of what was named before).
        let cycle = "<d:shape id=\"1\"><d:sheet><d:Section N=\"U\" def=\"5\"><d:same of=\"1\"/></d:Section></d:sheet></d:shape>\
                     <d:shape id=\"2\"><d:sheet def=\"1\"><d:same of=\"5\"/></d:sheet></d:shape>\
                     <d:shape id=\"3\"><d:sheet><d:same of=\"5\"/></d:sheet></d:shape>";
        let full = walk(cycle, u64::MAX).expect("a bounded walk");
        // Every budget below what the walk is refuses it, in words, and the walk at that budget is that many items and no more.
        assert_eq!(walk(cycle, full), Ok(full));
        assert!(matches!(walk(cycle, full - 1), Err(CodecError::Limit { what: "items in the drawing", .. })));
        // A unit that holds itself by name: the name is given when the unit is closed, so the `same` inside it names nothing and is an element like any other.
        let itself = "<d:shape id=\"1\"><d:sheet def=\"1\"><d:Section N=\"U\" def=\"2\"><d:same of=\"2\"/></d:Section></d:sheet></d:shape>\
                      <d:shape id=\"2\"><d:same of=\"1\"/></d:shape>";
        let (inside, once) = (
            walk(itself, u64::MAX).unwrap(),
            walk("<d:shape id=\"1\"><d:sheet def=\"1\"><d:Section N=\"U\" def=\"2\"><d:same of=\"2\"/></d:Section></d:sheet></d:shape>", u64::MAX)
                .unwrap(),
        );
        assert!(inside > once, "the `same` of the second shape is the whole sheet, walked again: {inside} against {once}");
        // A `same` stands where a unit does and for a unit of the kind the place holds, as the decoder reads one: not for a section where a sheet stands, not in a row.
        let kind = "<d:shape id=\"1\"><d:sheet><d:Section N=\"U\" def=\"1\"/></d:sheet></d:shape><d:shape id=\"2\"><d:same of=\"1\"/></d:shape>";
        let plain = "<d:shape id=\"1\"><d:sheet><d:Section N=\"U\" def=\"1\"/></d:sheet></d:shape><d:shape id=\"2\"/>";
        assert!(
            walk(kind, u64::MAX).unwrap() > walk(plain, u64::MAX).unwrap(),
            "an element like any other, one item and its attribute, is still walked"
        );
        let in_a_row = "<d:shape id=\"1\"><d:sheet def=\"1\"/></d:shape><d:shape id=\"2\"><d:sheet><d:Section N=\"U\"><d:Row><d:same of=\"1\"/></d:Row></d:Section></d:sheet></d:shape>";
        let row_without = "<d:shape id=\"1\"><d:sheet def=\"1\"/></d:shape><d:shape id=\"2\"><d:sheet><d:Section N=\"U\"><d:Row><d:other of=\"1\"/></d:Row></d:Section></d:sheet></d:shape>";
        assert_eq!(walk(in_a_row, u64::MAX).unwrap(), walk(row_without, u64::MAX).unwrap(), "a `same` in a row is not a unit's place");
        // A unit's `fp` is no attribute of the content, as its `def` is not.
        let plain_unit = "<d:shape id=\"1\"><d:sheet><d:Cell N=\"A\"/></d:sheet></d:shape>";
        let named = "<d:shape id=\"1\"><d:sheet def=\"1\" fp=\"0123456789abcdef\"><d:Cell N=\"A\"/></d:sheet></d:shape>";
        let (a, b) = (walk(plain_unit, u64::MAX).unwrap(), walk(named, u64::MAX).unwrap());
        assert_eq!(a, b);
        // Only a unit that names itself drops them: an attribute that is called `fp` or `def` on a shape, on a sheet that gives no name, on a section that has an `fp` and no `def`, and a
        // `def` that no `fp` follows, are what they say, and are walked (one item for the element and one for each attribute).
        let items = |body: &str| walk(body, u64::MAX).unwrap();
        let base = items("<d:shape id=\"1\"/>");
        assert_eq!(items("<d:shape id=\"1\" fp=\"0123456789abcdef\"/>"), base + 1, "an attribute of a shape that is called `fp`");
        assert_eq!(items("<d:shape id=\"1\"><d:sheet fp=\"0123456789abcdef\"/></d:shape>"), base + 2, "a sheet that gives no name has its `fp`");
        assert_eq!(
            items("<d:shape id=\"1\"><d:sheet><d:Section N=\"U\" fp=\"0\"/></d:sheet></d:shape>"),
            base + 4,
            "a section that gives no name has its `fp`"
        );
        assert_eq!(
            items("<d:shape id=\"1\"><d:sheet def=\"1\"/></d:shape>"),
            base + 1,
            "a unit that names itself is walked as the element, and not its name"
        );
        assert_eq!(items("<d:shape id=\"1\"><d:sheet def=\"1\" fp=\"0\" other=\"x\"/></d:shape>"), base + 2, "and the attribute it has besides");
        let digest_of = |body: &str| {
            let text = format!("<diacraft:page xmlns:diacraft=\"{URN}\">{}</diacraft:page>", ours_prefix(body));
            let tree = parse_with(text.as_bytes(), &XmlLimits { max_depth: 512, ..XmlLimits::default() }).unwrap();
            let mut digest = Digest::new("t");
            feed_tree(&mut digest, &tree, &mut Budget::new(u64::MAX, u64::MAX, u64::MAX)).unwrap();
            digest.finish()
        };
        assert_eq!(digest_of(plain_unit), digest_of(named), "the same stamp with and without a name and a fingerprint");
    }
}
