//! What the model has no typed place for, and how the reader keeps it: unknown attributes, elements, comments and processing
//! instructions.
//!
//! # The rules the model's fields follow
//!
//! * **A typed field holds a value only if it can hand the file's text back.** An `ID` of `5` is the number 5 and needs nothing more. An
//!   `ID` of `007`, `+5` or `4294967296` is not text that the number 7, 5 or 4294967295 would give back, so the **attribute is kept in
//!   `raw` as it was and wins**: the typed field holds the nearest number its type allows, and a writer gives the `raw` text. The same
//!   holds for a flag (`Del="0"`, `Background="true"`) and a style link (`LineStyle=""`). So nothing is carried in two places without
//!   one of them being the truth: `raw` wins wherever it holds the attribute.
//! * **Where an element's unknown children go** (`Placed::after`): `after` is the number of the element's *typed* child elements that
//!   came before the node, a typed child being one the reader put in the model (a `Cell` or `Section` of a sheet, a `Row` of a section,
//!   the `Text`, `ForeignData` and `Shapes` of a shape, ...), so a writer that puts typed children back in file order puts the node back
//!   after that many. A sheet's `Trigger` elements go to `Sheet::passthrough`, and any other node (a foreign element, an unknown VSDX
//!   element, a comment, an instruction, text that is not blank) to the passthrough of the element that holds the sheet where it has one
//!   (`Shape`, `Style`, `Page`, `Master`, `Document`), and to the sheet's own where it has not (`PageSheet`, `DocumentSheet`).
//! * **Layout is not content.** Blank text between the child elements of an element that holds elements only is not kept: a write at the
//!   tree level patches the original tree, which has it. Text that is not blank, a `Cell`'s text, `Text` and
//!   `fld` content and the content of an unknown element are kept exactly.
//! * **CDATA is text.** `RawNode` has no CDATA variant, so a CDATA section is kept as the text it holds.
//!
//! The elements are copied from the container's tree, whose depth is at most `XML_DEPTH_CEILING` (512), so the recursion of these functions is
//! bounded by it, and a test runs the deepest on a small stack.

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Passthrough, RawAttrs, RawElement, RawNode};

/// `after` for a count of typed children, as the model stores it.
pub fn after(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Every attribute of `el`, in order, as the model's `RawAttrs`.
pub fn all_attrs(el: &El) -> RawAttrs {
    RawAttrs::from_pairs(el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}

/// An element and everything in it, as the model keeps an unknown element.
pub fn raw_element(el: &El) -> RawElement {
    RawElement { name: Arc::from(el.name.as_str()), attrs: all_attrs(el), children: el.children.iter().map(raw_node).collect() }
}

/// A node of the tree as the model keeps it. CDATA is text.
pub fn raw_node(node: &Node) -> RawNode {
    match node {
        Node::El(el) => RawNode::Element(raw_element(el)),
        Node::Text(t) | Node::CData(t) => RawNode::Text(Arc::from(t.as_str())),
        Node::Comment(c) => RawNode::Comment(Arc::from(c.as_str())),
        Node::Pi { target, data } => RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) },
    }
}

/// Whether text is only the blanks XML allows between elements.
pub fn is_blank(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}

/// The items an element costs a document that keeps it: itself, each attribute, and each node in it, all the way down. The walk keeps its own
/// stack and does not recurse.
pub fn element_cost(el: &El) -> usize {
    let mut total = 0usize;
    let mut stack: Vec<&El> = vec![el];
    while let Some(e) = stack.pop() {
        total = total.saturating_add(1).saturating_add(e.attrs.len());
        for node in &e.children {
            match node {
                Node::El(child) => stack.push(child),
                _ => total = total.saturating_add(1),
            }
        }
    }
    total
}

/// The bytes of text an element costs a document that keeps it: the names of the element and of each element in it, every attribute's name and value, and the
/// text, comments and instructions all the way down. A document that keeps the element charges [`element_cost`] items for the nodes and these bytes (at
/// whatever the document's bytes to an item are) for what the nodes hold, before it copies it. The walk keeps its own stack and does not recurse.
pub fn element_bytes(el: &El) -> usize {
    let mut total = 0usize;
    let mut stack: Vec<&El> = vec![el];
    while let Some(e) = stack.pop() {
        total = total.saturating_add(e.name.len());
        for (k, v) in &e.attrs {
            total = total.saturating_add(k.len()).saturating_add(v.len());
        }
        for node in &e.children {
            let len = match node {
                Node::El(child) => {
                    stack.push(child);
                    0
                }
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => t.len(),
                Node::Pi { target, data } => target.len().saturating_add(data.len()),
            };
            total = total.saturating_add(len);
        }
    }
    total
}

/// Keeps `node` after `typed` typed children.
pub fn keep(into: &mut Passthrough, typed: usize, node: RawNode) {
    into.push(after(typed), node);
}

/// The number an attribute holds when it is the text of that number: no sign, no leading zeros, no blanks, in 0 to 4294967295.
pub fn canonical_u32(text: &str) -> Option<u32> {
    let n: u32 = text.parse().ok()?;
    (n.to_string() == text).then_some(n)
}

/// A number-valued attribute, read for a typed field: the number, and whether the attribute must also be kept in `raw` because the number
/// cannot give its text back (the module documentation). A text that is all digits but too large reads as `u32::MAX`; any other text reads as
/// no number.
pub fn read_u32(text: &str) -> (Option<u32>, bool) {
    if let Some(n) = canonical_u32(text) {
        return (Some(n), false);
    }
    let digits = text.trim().trim_start_matches('+');
    let nearest =
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) { Some(digits.parse::<u32>().unwrap_or(u32::MAX)) } else { None };
    (nearest, true)
}

/// A flag attribute (`Del`, `Background`): its value, and whether it must also be kept in `raw`. `1` is true and needs nothing more; every
/// other text, `0` included, is not what the typed field gives back (a false flag is an absent attribute).
pub fn read_flag(text: &str) -> (bool, bool) {
    match text {
        "1" => (true, false),
        "true" => (true, true),
        _ => (false, true),
    }
}

/// An element as the container's tree holds it, back from the model's copy of it: for a writer, and for the test that a read and a re-emit give
/// the same text. Its names, attributes and children are as the model has them.
pub fn to_tree(raw: &RawElement) -> El {
    El {
        name: raw.name.to_string(),
        attrs: raw.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        children: raw.children.iter().map(node_to_tree).collect(),
    }
}

/// A [`RawNode`] as a node of the container's tree.
pub fn node_to_tree(node: &RawNode) -> Node {
    match node {
        RawNode::Element(el) => Node::El(to_tree(el)),
        RawNode::Text(t) => Node::Text(t.to_string()),
        RawNode::Comment(c) => Node::Comment(c.to_string()),
        RawNode::Pi { target, data } => Node::Pi { target: target.to_string(), data: data.to_string() },
    }
}
