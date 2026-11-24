//! A page or a master part made as **a stream of the children of its root** (item 1).
//!
//! A part is its root and a list of children: the page's sheet, its shapes, its connects, and what it keeps. The encoder made the whole tree before it gave it away, so that a
//! page of a hundred thousand shapes was held as a tree of millions of items beside the model it came from. Here the head of the root is made first (its attributes, and the
//! fields too long for an attribute), then **each child is made, handed to a [`PartSink`] and let go**: what is held is the child in hand, whatever the number of shapes.
//! The sink is what the part goes to: a tree ([`TreeSink`], what [`Encoder::page_part`] gives), or the bytes of the part ([`ByteSink`]), which is what a package stores and
//! what the save's check reads as an open does.
//!
//! **The same part, either way.** The tree and the bytes are made by the same code, so the bytes are what the writer makes of the tree (a test holds them to it, over generated
//! drawings). What a newer build wrote and this build keeps is put among the children by the number of known elements it followed, as the writer of the whole tree puts it, and
//! the attributes of the root it keeps are on its head, which is written before the children.
//!
//! **What a child may have** ([`ChildCap`]): a part read as a stream is read a child at a time ([`super::OPEN_ELEMENT_ITEMS`]), so the encoder holds each child, the head and
//! the tail (what the page keeps) to the items of one tree, and refuses the one that passes them in words, **while it is made**. A part made as a tree is held to its own cap as
//! it always was.

use diacraft_container::xml::{El, Node, RootWriter};
use diacraft_doc::{Connect, Kept, Master, Page, RawNode};

use super::CodecError;
use super::encode::{Encoder, element_floor, is_default_sheet};
use super::vocab::{at, el};

/// Where the children of a part go as they are made.
pub(super) trait PartSink {
    /// The root's start tag: its name and its attributes. Called once, first.
    fn head(&mut self, root: &El) -> Result<(), CodecError>;
    /// A child of the root, whole, in order.
    fn child(&mut self, node: Node) -> Result<(), CodecError>;
}

/// A part collected as a tree, for a caller that holds every part ([`Encoder::page_part`]).
#[derive(Default)]
pub(super) struct TreeSink {
    root: Option<El>,
}

impl TreeSink {
    pub(super) fn into_tree(self) -> Result<El, CodecError> {
        self.root.ok_or_else(|| CodecError::malformed("document", "a part with no root"))
    }
}

impl PartSink for TreeSink {
    fn head(&mut self, root: &El) -> Result<(), CodecError> {
        self.root = Some(El { name: root.name.clone(), attrs: root.attrs.clone(), children: Vec::new() });
        Ok(())
    }

    fn child(&mut self, node: Node) -> Result<(), CodecError> {
        match self.root.as_mut() {
            Some(root) => {
                root.children.push(node);
                Ok(())
            }
            None => Err(CodecError::malformed("document", "a child of a part before its root")),
        }
    }
}

/// A part written as the bytes of an XML document, held to the bytes a part may have: refused at the child that passes them, before the next is made.
pub(super) struct ByteSink {
    writer: Option<RootWriter>,
    cap: u64,
    error: (&'static str, u64),
}

impl ByteSink {
    /// A sink for a part of at most `cap` bytes, refused as `error` says.
    pub(super) fn new(cap: u64, error: (&'static str, u64)) -> ByteSink {
        ByteSink { writer: None, cap, error }
    }

    /// The part's bytes, once every child is given.
    pub(super) fn finish(self) -> Result<Vec<u8>, CodecError> {
        let bytes = self.writer.ok_or_else(|| CodecError::malformed("document", "a part with no root"))?.finish();
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.cap {
            return Err(CodecError::Limit { what: self.error.0, max: self.error.1 });
        }
        Ok(bytes)
    }

    fn within(&self) -> Result<(), CodecError> {
        let held = self.writer.as_ref().map_or(0, RootWriter::len);
        if u64::try_from(held).unwrap_or(u64::MAX) > self.cap { Err(CodecError::Limit { what: self.error.0, max: self.error.1 }) } else { Ok(()) }
    }
}

impl PartSink for ByteSink {
    fn head(&mut self, root: &El) -> Result<(), CodecError> {
        self.writer = Some(RootWriter::begin(root, true)?);
        self.within()
    }

    fn child(&mut self, node: Node) -> Result<(), CodecError> {
        match self.writer.as_mut() {
            Some(writer) => writer.child(&node)?,
            None => return Err(CodecError::malformed("document", "a child of a part before its root")),
        }
        self.within()
    }
}

/// What one child of the root of a part may have, when the part is made as a stream: the items of a tree of the reader's, and the words it is refused in; and what the whole
/// part may have, which is the format's limit of items in one part (the spec's 6.1: a writer does not write a part that passes it, however it is made).
#[derive(Clone, Copy)]
pub(super) struct ChildCap {
    pub(super) items: u64,
    pub(super) what: &'static str,
    pub(super) max: u64,
    /// The items the part may have in all, the head and every child; `u64::MAX` for a walk that refuses nothing (the content stamp).
    pub(super) part: u64,
}

/// The unknown nodes of an element that a newer build wrote, put among its children by the number of known elements they followed: as the writer of the whole tree does it
/// ([`Encoder::native_nodes_end`]), a node is written before the known element that is the one after its count, and what is left is written last.
struct Interleave<'k> {
    nodes: std::iter::Peekable<std::vec::IntoIter<(u32, &'k RawNode)>>,
    elements: u32,
}

impl<'k> Interleave<'k> {
    fn new(native: Option<&'k Kept>) -> Interleave<'k> {
        let mut nodes: Vec<(u32, &RawNode)> = native
            .map(|k| {
                k.kept
                    .iter()
                    .filter(|p| !matches!(&p.node, RawNode::Element(r) if super::ext::is_keyed(&r.name)))
                    .map(|p| (p.after, &p.node))
                    .collect()
            })
            .unwrap_or_default();
        nodes.sort_by_key(|(after, _)| *after);
        Interleave { nodes: nodes.into_iter().peekable(), elements: 0 }
    }

    /// Gives `node` to the sink, after the unknown nodes that belong before it. **Each unknown node is a child of its own** (`cap`): its items are counted against the cap of a
    /// child and added to the part's, as those of every other child are, and not against the part's as a whole.
    fn push(&mut self, enc: &mut Encoder<'_>, sink: &mut dyn PartSink, cap: Option<ChildCap>, node: Node) -> Result<(), CodecError> {
        if matches!(node, Node::El(_)) {
            while let Some((_, n)) = self.nodes.next_if(|(after, _)| *after <= self.elements) {
                let literal = enc.one_child(cap, |enc| enc.literal(n))?;
                sink.child(literal)?;
            }
            self.elements = self.elements.saturating_add(1);
        }
        sink.child(node)
    }

    /// The unknown nodes that followed more known elements than the part has.
    fn finish(self, enc: &mut Encoder<'_>, sink: &mut dyn PartSink, cap: Option<ChildCap>) -> Result<(), CodecError> {
        for (_, n) in self.nodes {
            let literal = enc.one_child(cap, |enc| enc.literal(n))?;
            sink.child(literal)?;
        }
        Ok(())
    }
}

impl Encoder<'_> {
    /// Runs `make` as one child of a part made as a stream: its items are counted from nothing against the cap of one child (`cap`), and added to the part's when it is done.
    /// A part made as a tree (`cap` is `None`) counts every item against its own cap.
    fn one_child<T>(&mut self, cap: Option<ChildCap>, make: impl FnOnce(&mut Self) -> Result<T, CodecError>) -> Result<T, CodecError> {
        let Some(cap) = cap else { return make(self) };
        let (items, limit, error) = (self.items, self.cap, self.cap_error);
        self.items = 0;
        self.cap = cap.items;
        self.cap_error = (cap.what, cap.max);
        let made = make(self);
        let used = self.items;
        self.items = items.saturating_add(used);
        self.cap = limit;
        self.cap_error = error;
        if made.is_ok() && self.items > cap.part {
            return Err(CodecError::Limit { what: "XML items in one part", max: cap.part });
        }
        made
    }

    /// The tail of a part: what its object keeps (the attributes and nodes the model has no field for, and its extension slot), as children in order.
    fn tail_children(&mut self, fill: impl FnOnce(&mut Self, &mut El) -> Result<(), CodecError>) -> Result<Vec<Node>, CodecError> {
        let mut scratch = self.elem("tail");
        fill(self, &mut scratch)?;
        Ok(scratch.children)
    }

    /// A page as a stream of the children of its root: the head, then each child, to `sink`. Gives the items of the part. With `cap` each child is held to the items of one
    /// tree ([`ChildCap`]); without it the whole part is counted against the part's cap, as a tree is.
    pub(super) fn page_stream(&mut self, p: &Page, cap: Option<ChildCap>, sink: &mut dyn PartSink) -> Result<u64, CodecError> {
        let Page { id, name, name_u, background, back_page, rel_id, sheet, shapes, connects, raw, passthrough, ext, part_name } = p;
        self.begin_part();
        self.units = self.units_of(std::iter::once(sheet), &[shapes]);
        let native = ext.get().map(|d| &d.native);
        let mut root = self.root(el::PAGE);
        self.one_child(cap, |enc| {
            Self::put_u32(&mut root, at::ID, *id, 0);
            enc.put_req(&mut root, at::NAME, name)?;
            enc.put_opt(&mut root, at::NAME_U, name_u.as_ref())?;
            Self::put_flag(&mut root, at::BACKGROUND, *background);
            Self::put_opt_u32(&mut root, at::BACK_PAGE, *back_page);
            enc.put_opt(&mut root, at::REL_ID, rel_id.as_ref())?;
            enc.put_opt(&mut root, at::PART_NAME, part_name.as_ref())?;
            enc.native_attrs_end(&mut root, native)?;
            // A part made as a stream counts its root where it is made, before its children; a part made as a tree counts it last, when it is whole.
            if cap.is_some() { enc.count_head(&root) } else { Ok(()) }
        })?;
        let longs = std::mem::take(&mut root.children);
        sink.head(&root)?;
        let mut among = Interleave::new(native);
        for node in longs {
            among.push(self, sink, cap, node)?;
        }
        if !is_default_sheet(sheet) {
            let e = self.one_child(cap, |enc| enc.child_el(|enc| enc.sheet(sheet)))?;
            among.push(self, sink, cap, Node::El(e))?;
        }
        for shape in shapes {
            let e = self.one_child(cap, |enc| enc.child_el(|enc| enc.shape(shape, 1)))?;
            among.push(self, sink, cap, Node::El(e))?;
        }
        self.connects_stream(connects, cap, &mut among, sink)?;
        let tail = self.one_child(cap, |enc| {
            enc.tail_children(|enc, e| {
                enc.raw_attrs(e, raw)?;
                enc.kept(e, passthrough)?;
                enc.ext_slot(e, ext, |enc, o, el| enc.odf_page(o, el))
            })
        })?;
        for node in tail {
            among.push(self, sink, cap, node)?;
        }
        among.finish(self, sink, cap)?;
        if cap.is_none() {
            self.count_head(&root)?;
        }
        Ok(self.items)
    }

    /// A master as a stream of the children of its root ([`Encoder::page_stream`]).
    pub(super) fn master_stream(&mut self, m: &Master, cap: Option<ChildCap>, sink: &mut dyn PartSink) -> Result<u64, CodecError> {
        let Master { id, name, name_u, unique_id, pattern_flags, rel_id, icon, sheet, shapes, connects, raw, passthrough, ext, part_name } = m;
        self.begin_part();
        self.units = self.units_of(std::iter::once(sheet), &[shapes]);
        let native = ext.get().map(|d| &d.native);
        let mut root = self.root(el::MASTER);
        self.one_child(cap, |enc| {
            Self::put_u32(&mut root, at::ID, *id, 0);
            enc.put_opt(&mut root, at::NAME, name.as_ref())?;
            enc.put_opt(&mut root, at::NAME_U, name_u.as_ref())?;
            enc.put_opt(&mut root, at::UNIQUE_ID, unique_id.as_ref())?;
            Self::put_opt_u32(&mut root, at::PATTERN_FLAGS, *pattern_flags);
            enc.put_opt(&mut root, at::REL_ID, rel_id.as_ref())?;
            enc.put_opt(&mut root, at::ICON, icon.as_ref())?;
            enc.put_opt(&mut root, at::PART_NAME, part_name.as_ref())?;
            enc.native_attrs_end(&mut root, native)?;
            // A part made as a stream counts its root where it is made, before its children; a part made as a tree counts it last, when it is whole.
            if cap.is_some() { enc.count_head(&root) } else { Ok(()) }
        })?;
        let longs = std::mem::take(&mut root.children);
        sink.head(&root)?;
        let mut among = Interleave::new(native);
        for node in longs {
            among.push(self, sink, cap, node)?;
        }
        if !is_default_sheet(sheet) {
            let e = self.one_child(cap, |enc| enc.child_el(|enc| enc.sheet(sheet)))?;
            among.push(self, sink, cap, Node::El(e))?;
        }
        for shape in shapes {
            let e = self.one_child(cap, |enc| enc.child_el(|enc| enc.shape(shape, 1)))?;
            among.push(self, sink, cap, Node::El(e))?;
        }
        self.connects_stream(connects, cap, &mut among, sink)?;
        let tail = self.one_child(cap, |enc| {
            enc.tail_children(|enc, e| {
                enc.raw_attrs(e, raw)?;
                enc.kept(e, passthrough)?;
                enc.ext_kept(e, ext)
            })
        })?;
        for node in tail {
            among.push(self, sink, cap, node)?;
        }
        among.finish(self, sink, cap)?;
        if cap.is_none() {
            self.count_head(&root)?;
        }
        Ok(self.items)
    }

    fn connects_stream<'c>(
        &mut self,
        connects: impl IntoIterator<Item = &'c Connect>,
        cap: Option<ChildCap>,
        among: &mut Interleave<'_>,
        sink: &mut dyn PartSink,
    ) -> Result<(), CodecError> {
        for c in connects {
            let e = self.one_child(cap, |enc| enc.child_el(|enc| enc.connect(c)))?;
            among.push(self, sink, cap, Node::El(e))?;
        }
        Ok(())
    }

    /// The element `make` gives, counted as a child of the root: its own items and its bytes, as [`Encoder::add`] counts what it puts in a parent.
    fn child_el(&mut self, make: impl FnOnce(&mut Self) -> Result<El, CodecError>) -> Result<El, CodecError> {
        let e = make(self)?;
        self.count_child(&e)?;
        Ok(e)
    }

    /// The root of a part, counted: its element, its attributes and the bytes of its start tag, as the part is counted when it is whole.
    fn count_head(&mut self, root: &El) -> Result<(), CodecError> {
        self.count_child(root)
    }

    /// The items and the bytes an element adds to its parent: itself and each attribute.
    fn count_child(&mut self, e: &El) -> Result<(), CodecError> {
        self.count(1u64.saturating_add(u64::try_from(e.attrs.len()).unwrap_or(u64::MAX)))?;
        self.count_bytes(element_floor(e))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Page, Shape, ShapeList};

    use super::*;
    use crate::model::MemBlobs;

    fn page_of(n: u32) -> Page {
        let mut page = Page::new(1, "P");
        page.shapes = ShapeList::from_vec(
            (1..=n)
                .map(|i| {
                    let mut s = Shape::new(i);
                    s.name = Some(Arc::from("n"));
                    Arc::new(s)
                })
                .collect(),
        );
        page
    }

    /// A page made as a stream with at most `part` items in all and `child` in one child: its items, or the refusal.
    fn made(page: &Page, child: u64, part: u64) -> Result<u64, CodecError> {
        let mut blobs = MemBlobs::new();
        let mut enc = Encoder::new(&mut blobs);
        let cap = ChildCap { items: child, what: "XML items in one element of a part", max: child, part };
        let mut sink = ByteSink::new(u64::MAX, ("bytes in one part as written", u64::MAX));
        enc.page_stream(page, Some(cap), &mut sink)
    }

    #[test]
    fn a_part_made_as_a_stream_is_held_to_the_items_a_part_may_have_in_all_though_each_child_is_small() {
        // The format allows a part so many items (6.1), and a page that a reader opens child by child is still one part: its children are small, and it is refused when
        // their sum passes the part's cap, in the words of the part.
        let page = page_of(200);
        let items = made(&page, 1_000, u64::MAX).expect("a page with no cap for the part");
        assert!(items > 400, "each shape is an element with its attributes: {items}");
        assert_eq!(made(&page, 1_000, items), Ok(items), "at exactly the items it has, it is made");
        assert_eq!(
            made(&page, 1_000, items - 1),
            Err(CodecError::Limit { what: "XML items in one part", max: items - 1 }),
            "one item fewer, it is refused"
        );
        // The child's cap is the child's: a part of many small children is not refused by it.
        assert!(made(&page, 40, u64::MAX).is_ok());
        assert_eq!(made(&page, 1, u64::MAX), Err(CodecError::Limit { what: "XML items in one element of a part", max: 1 }));
    }
}
