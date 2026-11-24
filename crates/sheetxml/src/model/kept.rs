//! What the model keeps without a field, as data: `attr` children for [`RawAttrs`], `kept` children for a [`Passthrough`], and a kept element as an
//! `el` element that holds its name as an attribute (the design's 2.2).
//!
//! ```text
//! kept := <kept after="N"> NODE </kept>                      (after is absent when 0)
//! NODE := <el name="v:foo"> <attr name="x" value="1"/>* NODE* </el>
//!       | <txt>characters</txt> | <comment>characters</comment> | <pi target="t">data</pi>
//! ```
//!
//! A node is never literal XML, so a kept prefix cannot clash with the codec's and a kept attribute cannot clash with a typed one. A string with a
//! character XML forbids is in base64 under a `b64="1"` flag. A kept element is copied with a stack of its own, in both directions, at most
//! [`MAX_KEPT_DEPTH`] levels, and every element and byte of it is charged once.

use std::borrow::Cow;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Passthrough, RawAttrs, RawElement, RawNode};

use super::CodecError;
use super::cost::{ATTR_PAIR_BYTES, push_bytes};
use super::decode::{Dec, Ours};
use super::encode::{Encoder, MAX_KEPT_DEPTH};
use super::escape::{base64_encode, base64_string, needs_base64};
use super::vocab::{at, el};
use crate::ns::Scope;

// ---------------------------------------------------------------------------------------------------------------------- encoding

impl Encoder<'_> {
    /// `raw` as `attr` children, in order.
    pub(super) fn raw_attrs(&mut self, parent: &mut El, raw: &RawAttrs) -> Result<(), CodecError> {
        for (name, value) in raw.iter() {
            let mut a = self.elem(el::ATTR);
            self.put_req(&mut a, at::NAME, name)?;
            self.put_req(&mut a, at::VALUE, value)?;
            self.add(parent, a)?;
        }
        Ok(())
    }

    /// `kept` as `kept` children, in order.
    pub(super) fn kept(&mut self, parent: &mut El, kept: &Passthrough) -> Result<(), CodecError> {
        for placed in kept.iter() {
            let k = self.kept_wrapper(placed.after, &placed.node)?;
            self.add(parent, k)?;
        }
        Ok(())
    }

    /// One `kept` element: the node, and how many typed children it followed.
    pub(super) fn kept_wrapper(&mut self, after: u32, node: &RawNode) -> Result<El, CodecError> {
        let mut k = self.elem(el::KEPT);
        Self::put_u32(&mut k, at::AFTER, after, 0);
        let n = self.kept_node(node)?;
        self.add(&mut k, n)?;
        Ok(k)
    }

    pub(super) fn kept_node(&mut self, node: &RawNode) -> Result<El, CodecError> {
        match node {
            RawNode::Element(e) => self.raw_element_el(e),
            RawNode::Text(t) => self.text_element(el::KEPT_TEXT, t),
            RawNode::Comment(c) => self.text_element(el::KEPT_COMMENT, c),
            RawNode::Pi { target, data } => {
                let mut p = self.elem(el::KEPT_PI);
                self.put_req(&mut p, at::TARGET, target)?;
                let content = if needs_base64(data) {
                    p.attrs.push((at::B64.to_owned(), "1".to_owned()));
                    base64_encode(data.as_bytes())
                } else {
                    data.to_string()
                };
                if !content.is_empty() {
                    self.add_text(&mut p, content)?;
                }
                Ok(p)
            }
        }
    }

    /// A kept element and everything in it, as an `el` tree. The copy keeps its own stack, and the depth is held to [`MAX_KEPT_DEPTH`].
    pub(super) fn raw_element_el(&mut self, root: &RawElement) -> Result<El, CodecError> {
        struct Frame<'r> {
            el: El,
            rest: std::slice::Iter<'r, RawNode>,
        }
        enum Step<'r> {
            Open(&'r RawElement),
            Leaf(&'r RawNode),
            Close,
        }
        let mut stack: Vec<Frame<'_>> = vec![Frame { el: self.open_raw(root)?, rest: root.children.iter() }];
        loop {
            let step = match stack.last_mut() {
                None => return Err(CodecError::malformed("document", "a kept element with no frame")),
                Some(top) => match top.rest.next() {
                    Some(RawNode::Element(e)) => Step::Open(e),
                    Some(other) => Step::Leaf(other),
                    None => Step::Close,
                },
            };
            match step {
                Step::Open(e) => {
                    if stack.len() >= MAX_KEPT_DEPTH {
                        return Err(CodecError::Limit { what: "levels of a kept element", max: u64::try_from(MAX_KEPT_DEPTH).unwrap_or(u64::MAX) });
                    }
                    let el = self.open_raw(e)?;
                    stack.push(Frame { el, rest: e.children.iter() });
                }
                Step::Leaf(node) => {
                    let leaf = self.kept_node(node)?;
                    if let Some(top) = stack.last_mut() {
                        self.add(&mut top.el, leaf)?;
                    }
                }
                Step::Close => {
                    let Some(done) = stack.pop() else { return Err(CodecError::malformed("document", "a kept element with no frame")) };
                    match stack.last_mut() {
                        Some(parent) => self.add(&mut parent.el, done.el)?,
                        None => return Ok(done.el),
                    }
                }
            }
        }
    }

    /// The `el` element of `e`: its name and attributes, and none of its children.
    fn open_raw(&mut self, e: &RawElement) -> Result<El, CodecError> {
        let mut el = self.elem(el::KEPT_EL);
        self.put_req(&mut el, at::NAME, &e.name)?;
        self.raw_attrs(&mut el, &e.attrs)?;
        Ok(el)
    }
}

// ---------------------------------------------------------------------------------------------------------------------- decoding

/// What an object keeps without a field, gathered from its children as they are read.
#[derive(Default)]
pub(super) struct Rest {
    pub raw: RawAttrs,
    pub kept: Passthrough,
}

impl Dec<'_> {
    /// Takes `child` into `rest` if it is an `attr` or a `kept` element: true when it was. `csc` is the scope inside `child`.
    pub(super) fn rest_child(&mut self, rest: &mut Rest, child: &El, csc: &Scope<Ours>) -> Result<bool, CodecError> {
        match child.local() {
            l if l == el::ATTR => {
                let (name, value) = self.attr_pair(child, csc)?;
                self.push_attr(&mut rest.raw, &name, &value)?;
                Ok(true)
            }
            l if l == el::KEPT => {
                let (after, node) = self.kept_node_of(child, csc)?;
                if let Some(node) = node {
                    self.push_kept(&mut rest.kept, after, node)?;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// The name and value of an `attr` element.
    pub(super) fn attr_pair(&mut self, child: &El, csc: &Scope<Ours>) -> Result<(String, String), CodecError> {
        let (mut name, mut value) = (String::new(), String::new());
        self.fields(child, csc, |_, n, v| {
            match n {
                at::NAME => name = v.into_owned(),
                at::VALUE => value = v.into_owned(),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        // A kept element's declaration of the namespace that builds made before the id moved to `diacraft.app` wrote is held as one of the current namespace (APPID).
        if (name == "xmlns" || name.starts_with("xmlns:")) && value == diacraft_doc::DIACRAFT_URN_LEGACY {
            value = diacraft_doc::DIACRAFT_URN.to_owned();
        }
        Ok((name, value))
    }

    /// A `kept` element: its `after` and the node in it. `None` when it holds no node this codec knows.
    pub(super) fn kept_node_of(&mut self, k: &El, csc: &Scope<Ours>) -> Result<(u32, Option<RawNode>), CodecError> {
        let mut after = 0u32;
        self.fields(k, csc, |d, n, v| {
            if n == at::AFTER {
                after = d.u32(&v)?;
                return Ok(true);
            }
            Ok(false)
        })?;
        let mut node = None;
        for child in k.elements() {
            let ksc = self.enter(csc, child)?;
            if !ksc.is_ours(child) {
                self.saw_unknown();
            } else if child.local() == el::LONG {
                // Read with the fields above.
            } else if node.is_some() {
                self.saw_unknown();
            } else {
                node = self.node_of(child, &ksc)?;
                if node.is_none() {
                    self.saw_unknown();
                }
            }
        }
        Ok((after, node))
    }

    /// The node an element of the data form is: an `el` tree, or a `txt`, `comment` or `pi`. `None` for any other element. `csc` is the scope inside `child`.
    pub(super) fn node_of(&mut self, child: &El, csc: &Scope<Ours>) -> Result<Option<RawNode>, CodecError> {
        Ok(match child.local() {
            l if l == el::KEPT_EL => Some(RawNode::Element(self.raw_element(child, csc)?)),
            l if l == el::KEPT_TEXT => Some(RawNode::Text(Arc::from(self.text_node(child, csc)?.as_str()))),
            l if l == el::KEPT_COMMENT => Some(RawNode::Comment(Arc::from(self.text_node(child, csc)?.as_str()))),
            l if l == el::KEPT_PI => {
                let mut target = String::new();
                let data = self.text_node_with(child, csc, |_, n, v| {
                    if n == at::TARGET {
                        target = v.into_owned();
                        return Ok(true);
                    }
                    Ok(false)
                })?;
                Some(RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) })
            }
            _ => None,
        })
    }

    /// The characters of a `txt`, `comment`, `t` or `fld` element: its text, or what the base64 in it holds when the flag says so.
    pub(super) fn text_node(&mut self, e: &El, csc: &Scope<Ours>) -> Result<String, CodecError> {
        self.text_node_with(e, csc, |_, _, _| Ok(false))
    }

    /// [`Dec::text_node`], with `apply` for the attributes the element has besides the flag.
    pub(super) fn text_node_with<F>(&mut self, e: &El, csc: &Scope<Ours>, mut apply: F) -> Result<String, CodecError>
    where
        F: FnMut(&mut Dec<'_>, &str, Cow<'_, str>) -> Result<bool, CodecError>,
    {
        let mut b64 = false;
        self.fields(e, csc, |d, n, v| {
            if n == at::B64 {
                b64 = d.flag(&v)?;
                return Ok(true);
            }
            apply(d, n, v)
        })?;
        let text = self.element_text(e)?;
        if b64 {
            return base64_string(&text).ok_or_else(|| CodecError::malformed(self.part(), "base64 text that is not base64 of text"));
        }
        Ok(text)
    }

    /// A kept element and everything in it, as a [`RawElement`]. The copy keeps its own stack, at most [`MAX_KEPT_DEPTH`] levels deep, and every
    /// element and string is charged once, before it is stored. `csc` is the scope inside `root`.
    pub(super) fn raw_element(&mut self, root: &El, csc: &Scope<Ours>) -> Result<RawElement, CodecError> {
        struct Frame<'a> {
            name: String,
            attrs: RawAttrs,
            children: Vec<RawNode>,
            rest: std::slice::Iter<'a, Node>,
            sc: Scope<Ours>,
        }
        enum Step<'a> {
            Open(&'a El, Scope<Ours>),
            Attr(&'a El, Scope<Ours>),
            Leaf(RawNode),
            Skip,
            Ignore,
            Close,
        }
        let first_name = self.element_name(root, csc)?;
        self.charge_children(root.elements().count())?;
        let first = Frame { name: first_name, attrs: RawAttrs::new(), children: Vec::new(), rest: root.children.iter(), sc: csc.clone() };
        let mut stack: Vec<Frame<'_>> = vec![first];
        loop {
            let step = {
                let Some(top) = stack.last_mut() else { return Err(CodecError::malformed(self.part(), "a kept element with no frame")) };
                match top.rest.next() {
                    Some(Node::El(c)) => {
                        let ksc = self.enter(&top.sc, c)?;
                        if !ksc.is_ours(c) {
                            Step::Skip
                        } else if c.local() == el::ATTR {
                            Step::Attr(c, ksc)
                        } else if c.local() == el::KEPT_EL {
                            Step::Open(c, ksc)
                        } else if c.local() == el::LONG {
                            // Read with the fields of the element's own attributes.
                            Step::Ignore
                        } else {
                            match self.node_of(c, &ksc)? {
                                Some(n) => Step::Leaf(n),
                                None => Step::Skip,
                            }
                        }
                    }
                    // The text between the elements of the data form is layout, which the codec never writes.
                    Some(_) => Step::Ignore,
                    None => Step::Close,
                }
            };
            match step {
                Step::Skip => self.saw_unknown(),
                Step::Ignore => {}
                Step::Attr(c, ksc) => {
                    let (name, value) = self.attr_pair(c, &ksc)?;
                    // What the push adds to the list of attributes of the open element is charged before it is made.
                    let have = stack.last().map_or(0, |top| top.attrs.len());
                    self.budget.charge_bytes(push_bytes(have, ATTR_PAIR_BYTES))?;
                    if let Some(top) = stack.last_mut() {
                        top.attrs.push(&name, &value);
                    }
                }
                Step::Leaf(n) => {
                    if let Some(top) = stack.last_mut() {
                        top.children.push(n);
                    }
                }
                Step::Open(c, ksc) => {
                    if stack.len() >= MAX_KEPT_DEPTH {
                        return Err(CodecError::Limit { what: "levels of a kept element", max: u64::try_from(MAX_KEPT_DEPTH).unwrap_or(u64::MAX) });
                    }
                    let name = self.element_name(c, &ksc)?;
                    self.charge_children(c.elements().count())?;
                    stack.push(Frame { name, attrs: RawAttrs::new(), children: Vec::new(), rest: c.children.iter(), sc: ksc });
                }
                Step::Close => {
                    let Some(done) = stack.pop() else { return Err(CodecError::malformed(self.part(), "a kept element with no frame")) };
                    let raw = RawElement { name: Arc::from(done.name.as_str()), attrs: done.attrs, children: done.children };
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(RawNode::Element(raw)),
                        None => return Ok(raw),
                    }
                }
            }
        }
    }

    /// The name an `el` element holds, with its items charged.
    fn element_name(&mut self, e: &El, csc: &Scope<Ours>) -> Result<String, CodecError> {
        let mut name = String::new();
        self.fields(e, csc, |_, n, v| {
            if n == at::NAME {
                name = v.into_owned();
                return Ok(true);
            }
            Ok(false)
        })?;
        Ok(name)
    }
}
