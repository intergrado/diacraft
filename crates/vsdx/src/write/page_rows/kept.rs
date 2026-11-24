//! **Relationship ids in the markup a shape keeps whole**. A `ForeignData` on a shape whose `Type` is not `Foreign` is not read: it is
//! kept as an unknown element of the shape (`Shape::passthrough`), and its `Rel` still names, by its `r:id`, a row of the relationships of the part the shape is in. A part that a save makes
//! for a page that had none (`page_rows.rs`: a copy of a page) must have that row as it has the rows of the `ForeignData`s that were read, or the copy names a row that is not there and shows
//! nothing. **Any relationship id in a shape's kept markup is such a name**, so each gets its row, whatever element holds it.
//!
//! **What is a relationship id.** An attribute whose local name is `id` and whose prefix is bound to the relationships namespace where it is written: by the nearest declaration of the
//! prefix (on the kept element or on one inside it on the way down, on the shape and the groups around it, on the root of the part as the page keeps it), and, for `r` with none, by the root
//! the writer makes, which binds `r` to it (`model::root_attrs`). The spelling is the file's: a file may bind another prefix to the namespace, and `r` to something else.
//!
//! **Costs** (the security scan). The declarations in force are a chain of [`Frame`]s, each holding only what its element declares and a pointer to the frame around it:
//! an element that declares nothing shares its parent's frame, and an element's frame is made when the walk takes it, so the walk's stack holds a pointer for each element waiting and never
//! a list of the declarations in force (a wide element under deep declarations copied them for each child). What a frame holds is the model's own text, shared. A prefix is looked up in at
//! most [`MAX_LOOKUP`] declarations, the innermost. In a kept node, at most [`MAX_TRIES_PER_NODE`] attributes that may be ids are looked up and at most [`MAX_IDS_PER_NODE`] ids get rows,
//! and the walk of the node ends there; a kept `ForeignData`'s own `Rel` is looked at first, so it is never past them. Each element, attribute and declaration looked at and each entry copied is counted (`work::kept_steps`, `work::kept_entries_copied`). A kept element
//! is written again only where the part gives one of its ids another, every attribute of that id at once, to the depth a part is read to.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_container::limits::XML_DEPTH_CEILING;
use diacraft_doc::{Passthrough, Placed, RawAttrs, RawElement, RawNode};

use crate::ns::NS_REL;

/// The most declarations a prefix is looked up in: the innermost. A file declares its prefixes on the root of a part, a handful; a prefix declared farther out than this many is read as one
/// that is not declared, so `r` is the one the root binds (a labelled choice: such a file is a hostile one).
pub(super) const MAX_LOOKUP: usize = 64;

/// The most relationship ids one kept node names that get rows: a `ForeignData` names one, and a node that names more than this many is a hostile one (a labelled choice), whose other ids are
/// written as they are.
pub(super) const MAX_IDS_PER_NODE: usize = 16;

/// The most attributes of one kept node that may be relationship ids (`p:id`) that are looked up: past them the walk of the node ends (a labelled choice, four times the ids that get rows).
pub(super) const MAX_TRIES_PER_NODE: usize = 4 * MAX_IDS_PER_NODE;

/// The attributes of an element as the model holds them, each name and value shared.
fn pairs(attrs: &RawAttrs) -> impl Iterator<Item = &(Arc<str>, Arc<str>)> {
    attrs.shared_list().into_iter().flat_map(|list| list.iter())
}

/// The declarations of one element and the frame of the element around it: **a chain**, so an element that declares nothing has its parent's frame and one that declares holds its own
/// declarations only, as the model holds them (`xmlns:p` and the namespace, shared).
#[derive(Default)]
pub(super) struct Frame {
    own: Vec<(Arc<str>, Arc<str>)>,
    parent: Option<Rc<Frame>>,
}

/// What a prefix is bound to.
enum Bound<'a> {
    To(&'a str),
    /// No declaration of it is among the [`MAX_LOOKUP`] innermost.
    Undeclared,
}

impl Frame {
    /// The frame of an element with `attrs` inside `parent`: `parent` itself when the element declares nothing.
    pub(super) fn enter(parent: &Rc<Frame>, attrs: &RawAttrs) -> Rc<Frame> {
        let own: Vec<(Arc<str>, Arc<str>)> =
            pairs(attrs).filter(|(name, _)| name.starts_with("xmlns:")).map(|(n, v)| (Arc::clone(n), Arc::clone(v))).collect();
        if own.is_empty() {
            return Rc::clone(parent);
        }
        crate::work::kept_copy(own.len());
        Rc::new(Frame { own, parent: Some(Rc::clone(parent)) })
    }

    /// What `prefix` is bound to by its nearest declaration, among the [`MAX_LOOKUP`] innermost.
    fn bound(&self, prefix: &str) -> Bound<'_> {
        let mut left = MAX_LOOKUP;
        let mut frame = self;
        loop {
            for (name, uri) in frame.own.iter().rev() {
                if left == 0 {
                    return Bound::Undeclared;
                }
                left -= 1;
                crate::work::kept_step(1);
                if name.strip_prefix("xmlns:") == Some(prefix) {
                    return Bound::To(uri);
                }
            }
            match &frame.parent {
                Some(parent) => frame = parent,
                None => return Bound::Undeclared,
            }
        }
    }
}

/// Whether an attribute's name may be a relationship id: a prefix that is not `xmlns`, and the local name `id`.
fn may_be_id(name: &str) -> bool {
    matches!(name.split_once(':'), Some((prefix, "id")) if prefix != "xmlns")
}

/// Whether the attribute `name` is a relationship id where `frame` is in force: its prefix is bound to the relationships namespace, or it is `r` and declared nowhere, which the root a part is
/// written with binds.
fn is_rel_id(frame: &Frame, name: &str) -> bool {
    let Some((prefix, "id")) = name.split_once(':') else { return false };
    if prefix == "xmlns" {
        return false;
    }
    match frame.bound(prefix) {
        Bound::To(uri) => uri == NS_REL,
        Bound::Undeclared => prefix == "r",
    }
}

/// A relationship id of a shape's kept markup: the place of the node it is in among the shape's kept nodes, and the id, as the model holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeptId {
    pub at: usize,
    pub id: Arc<str>,
}

/// The ids found in one kept node, and what its walk has spent.
struct Found<'k> {
    at: usize,
    seen: HashSet<&'k str>,
    tries: usize,
}

impl<'k> Found<'k> {
    /// Looks at the attributes of an element whose frame is `frame`, and puts the relationship ids among them in `out`. → whether the node's caps are reached, which ends its walk.
    fn look(&mut self, attrs: &'k RawAttrs, frame: &Frame, out: &mut Vec<KeptId>) -> bool {
        for (name, value) in pairs(attrs) {
            if !may_be_id(name) {
                continue;
            }
            if self.seen.len() >= MAX_IDS_PER_NODE || self.tries >= MAX_TRIES_PER_NODE {
                return true;
            }
            self.tries += 1;
            if is_rel_id(frame, name) && self.seen.insert(value) {
                out.push(KeptId { at: self.at, id: Arc::clone(value) });
            }
        }
        false
    }
}

/// The local part of an element's name.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The relationship ids of the kept nodes of a shape, `outer` in force around them: each id once for each node (the first place it has), at most [`MAX_IDS_PER_NODE`] of a node, in the order of
/// the nodes. **A kept `ForeignData`'s own `Rel` comes first**, the first child `Rel` that names one, as the reader takes it of one it reads: no markup beside it, however much, puts it
/// past the caps of its node.
pub(super) fn ids_of(kept: &Passthrough, outer: &Rc<Frame>) -> Vec<KeptId> {
    let mut out: Vec<KeptId> = Vec::new();
    for (at, placed) in kept.iter().enumerate() {
        let RawNode::Element(top) = &placed.node else { continue };
        let mut found = Found { at, seen: HashSet::new(), tries: 0 };
        crate::work::kept_step(1 + top.attrs.len());
        let top_frame = Frame::enter(outer, &top.attrs);
        if local(&top.name) == "ForeignData" {
            for child in &top.children {
                crate::work::kept_step(1);
                let RawNode::Element(rel) = child else { continue };
                if local(&rel.name) != "Rel" {
                    continue;
                }
                let before = out.len();
                let frame = Frame::enter(&top_frame, &rel.attrs);
                found.look(&rel.attrs, &frame, &mut out);
                if out.len() > before {
                    break;
                }
            }
        }
        if found.look(&top.attrs, &top_frame, &mut out) {
            continue;
        }
        // Each element waits with the frame of its parent, and its own is made when it is taken: the stack holds pointers, never lists of declarations.
        let mut stack: Vec<(&RawElement, Rc<Frame>)> =
            top.children.iter().rev().filter_map(|c| if let RawNode::Element(c) = c { Some((c, Rc::clone(&top_frame))) } else { None }).collect();
        while let Some((el, parent)) = stack.pop() {
            crate::work::kept_step(1 + el.attrs.len());
            let frame = Frame::enter(&parent, &el.attrs);
            if found.look(&el.attrs, &frame, &mut out) {
                break;
            }
            // The children last first, so that they are taken in order.
            for child in el.children.iter().rev() {
                if let RawNode::Element(c) = child {
                    stack.push((c, Rc::clone(&frame)));
                }
            }
        }
    }
    out
}

/// The ids a kept element may name a relationship by: an attribute `p:id` with any prefix but `xmlns`, at most [`MAX_IDS_PER_NODE`], as the model holds them. **More than the ids `ids_of`
/// finds** (no binding is looked at): what a page a copy came from holds is matched by the ids the copy names, which are the ones that count.
pub(super) fn prefixed_ids(el: &RawElement) -> Vec<Arc<str>> {
    let mut out: Vec<Arc<str>> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&RawElement> = vec![el];
    while let Some(e) = stack.pop() {
        crate::work::kept_step(1 + e.attrs.len());
        for (name, value) in pairs(&e.attrs) {
            if may_be_id(name) && seen.insert(value) {
                out.push(Arc::clone(value));
                if out.len() >= MAX_IDS_PER_NODE {
                    return out;
                }
            }
        }
        stack.extend(e.children.iter().filter_map(|c| match c {
            RawNode::Element(c) => Some(c),
            _ => None,
        }));
    }
    out
}

/// The new ids of the relationship ids of some kept nodes: by the place of the node, the old id and the new one.
pub(super) type Renames = HashMap<usize, HashMap<Arc<str>, Arc<str>>>;

/// The kept nodes with the relationship ids of each node `renames` names changed as it says, `outer` in force around them: every attribute of an old id in that node, to the depth a part
/// is read to. The nodes it names are copied; the others are shared.
pub(super) fn renamed(kept: &Passthrough, renames: &Renames, outer: &Rc<Frame>) -> Passthrough {
    let nodes: Vec<Placed> = kept
        .iter()
        .enumerate()
        .map(|(at, placed)| match (&placed.node, renames.get(&at)) {
            (RawNode::Element(el), Some(mine)) => Placed { after: placed.after, node: RawNode::Element(rename_in(el, mine, outer, 0)) },
            _ => placed.clone(),
        })
        .collect();
    Passthrough::from_nodes(nodes)
}

fn rename_in(el: &RawElement, renames: &HashMap<Arc<str>, Arc<str>>, parent: &Rc<Frame>, depth: usize) -> RawElement {
    let frame = Frame::enter(parent, &el.attrs);
    crate::work::kept_copy(el.attrs.len());
    let attrs = RawAttrs::from_pairs(pairs(&el.attrs).map(|(name, value)| match renames.get(value) {
        Some(new) if is_rel_id(&frame, name) => (&**name, &**new),
        _ => (&**name, &**value),
    }));
    let children = if depth >= XML_DEPTH_CEILING {
        el.children.clone()
    } else {
        el.children
            .iter()
            .map(|c| match c {
                RawNode::Element(c) => RawNode::Element(rename_in(c, renames, &frame, depth.saturating_add(1))),
                other => other.clone(),
            })
            .collect()
    };
    RawElement { name: Arc::clone(&el.name), attrs, children }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawElement>) -> RawElement {
        RawElement {
            name: Arc::from(name),
            attrs: RawAttrs::from_pairs(attrs.iter().copied()),
            children: children.into_iter().map(RawNode::Element).collect(),
        }
    }

    fn kept(nodes: Vec<RawElement>) -> Passthrough {
        Passthrough::from_nodes(nodes.into_iter().map(|e| Placed { after: 0, node: RawNode::Element(e) }).collect())
    }

    fn foreign(rel: &[(&str, &str)]) -> RawElement {
        el("ForeignData", &[("ForeignType", "Bitmap")], vec![el("Rel", rel, Vec::new())])
    }

    fn root_with(attrs: &[(&str, &str)]) -> Rc<Frame> {
        Frame::enter(&Rc::new(Frame::default()), &RawAttrs::from_pairs(attrs.iter().copied()))
    }

    fn ids(k: &Passthrough, outer: &Rc<Frame>) -> Vec<(usize, String)> {
        ids_of(k, outer).into_iter().map(|k| (k.at, k.id.to_string())).collect()
    }

    #[test]
    fn an_id_is_a_relationship_id_by_the_binding_of_its_prefix_and_r_is_bound_by_the_root() {
        let none = Rc::new(Frame::default());
        // `r` with no declaration is the root's.
        assert_eq!(ids(&kept(vec![foreign(&[("r:id", "rId1")])]), &none), [(0, "rId1".to_owned())]);
        // Another prefix bound on the element, or around it, names one too; `r` bound elsewhere does not.
        let own = foreign(&[("rel:id", "rId2"), ("xmlns:rel", NS_REL)]);
        assert_eq!(ids(&kept(vec![own]), &none), [(0, "rId2".to_owned())]);
        let outer = root_with(&[("xmlns:rel", NS_REL), ("xmlns:r", "urn:other")]);
        assert_eq!(ids(&kept(vec![foreign(&[("rel:id", "rId3")])]), &outer), [(0, "rId3".to_owned())]);
        assert!(ids(&kept(vec![foreign(&[("r:id", "rId4")])]), &outer).is_empty(), "r is bound elsewhere around the shape");
        // The nearest declaration wins: an element that binds `r` again inside markup that bound it elsewhere.
        let inside = el("x:Wrap", &[("xmlns:r", "urn:other")], vec![el("Rel", &[("xmlns:r", NS_REL), ("r:id", "rId5")], Vec::new())]);
        assert_eq!(ids(&kept(vec![inside]), &none), [(0, "rId5".to_owned())]);
        // Not ids: an attribute with no prefix, a declaration, another local name.
        let plain = el("ForeignData", &[("id", "a"), ("xmlns:id", NS_REL), ("r:ids", "b")], Vec::new());
        assert!(ids(&kept(vec![plain]), &none).is_empty());
    }

    #[test]
    fn each_node_names_an_id_once_in_the_order_of_the_nodes_and_deep_ones_are_found() {
        let deep = (0..40).fold(el("Rel", &[("r:id", "rId9")], Vec::new()), |inner, _| el("x:In", &[], vec![inner]));
        let k = kept(vec![foreign(&[("r:id", "rId1")]), el("Two", &[("r:id", "rId1")], vec![el("Rel", &[("r:id", "rId1")], Vec::new())]), deep]);
        assert_eq!(ids(&k, &Rc::new(Frame::default())), [(0, "rId1".to_owned()), (1, "rId1".to_owned()), (2, "rId9".to_owned())]);
    }

    #[test]
    fn a_rename_changes_the_relationship_ids_of_one_node_and_nothing_else() {
        let k = kept(vec![
            el("ForeignData", &[("Note", "rId1")], vec![el("Rel", &[("r:id", "rId1")], Vec::new()), el("Rel", &[("r:id", "rId2")], Vec::new())]),
            foreign(&[("r:id", "rId1")]),
        ]);
        let renames: Renames = HashMap::from([(0, HashMap::from([(Arc::from("rId1"), Arc::from("rId7"))]))]);
        let out = renamed(&k, &renames, &Rc::new(Frame::default()));
        let first = match &out.iter().next().unwrap().node {
            RawNode::Element(e) => e.clone(),
            _ => panic!("an element"),
        };
        assert_eq!(first.attrs.get("Note"), Some("rId1"), "an attribute that is no relationship id keeps its text");
        let rels: Vec<Option<&str>> = first
            .children
            .iter()
            .map(|c| match c {
                RawNode::Element(e) => e.attrs.get("r:id"),
                _ => None,
            })
            .collect();
        assert_eq!(rels, [Some("rId7"), Some("rId2")]);
        assert_eq!(out.iter().nth(1), k.iter().nth(1), "the other node is as it was");
        assert_eq!(ids_of(&out, &Rc::new(Frame::default())).len(), 3);
    }

    #[test]
    fn a_prefix_is_looked_up_in_the_innermost_declarations_and_a_node_is_looked_at_to_its_caps() {
        // 100 prefixes bound to the namespace, on two frames: the 64 innermost are looked up, and a prefix declared farther out is no relationships prefix.
        let decls: Vec<(String, String)> = (0..100).map(|n| (format!("xmlns:p{n}"), NS_REL.to_owned())).collect();
        let outer = root_with(&decls.iter().take(50).map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>());
        let frame = Frame::enter(&outer, &RawAttrs::from_pairs(decls.iter().skip(50).map(|(a, b)| (a.as_str(), b.as_str()))));
        assert!(is_rel_id(&frame, "p99:id") && is_rel_id(&frame, "p36:id"), "within the 64 innermost");
        assert!(!is_rel_id(&frame, "p35:id") && !is_rel_id(&frame, "p0:id"), "past them, a prefix is read as one that is not declared");
        assert!(is_rel_id(&frame, "r:id"), "r is declared nowhere, and the root binds it");
        let far = Frame::enter(&root_with(&[("xmlns:r", "urn:other")]), &RawAttrs::from_pairs(decls.iter().map(|(a, b)| (a.as_str(), b.as_str()))));
        assert!(is_rel_id(&far, "r:id"), "a binding of r past the 64 innermost is not looked at");
        // A node with 40 ids gets rows for 16, and one with 100 attributes that may be ids and are not is looked at for 64.
        let attrs: Vec<(String, String)> = (0..40).map(|n| (format!("r{n}:id"), format!("rId{n}"))).collect();
        let mut with_decls = attrs.clone();
        with_decls.extend((0..40).map(|n| (format!("xmlns:r{n}"), NS_REL.to_owned())));
        let wide = RawElement {
            name: Arc::from("x:Wide"),
            attrs: RawAttrs::from_pairs(with_decls.iter().map(|(a, b)| (a.as_str(), b.as_str()))),
            children: Vec::new(),
        };
        assert_eq!(ids_of(&kept(vec![wide.clone()]), &Rc::new(Frame::default())).len(), MAX_IDS_PER_NODE);
        assert_eq!(prefixed_ids(&wide).len(), MAX_IDS_PER_NODE);
        let not_ids: Vec<(String, String)> = (0..100).map(|n| (format!("q{n}:id"), format!("v{n}"))).collect();
        let tail = el("Rel", &[("r:id", "rId1")], Vec::new());
        let busy = RawElement {
            name: Arc::from("x:Busy"),
            attrs: RawAttrs::from_pairs(not_ids.iter().map(|(a, b)| (a.as_str(), b.as_str()))),
            children: vec![RawNode::Element(tail)],
        };
        assert!(ids_of(&kept(vec![busy]), &Rc::new(Frame::default())).is_empty(), "the node's walk ends after 64 attributes that are not ids");
    }

    #[test]
    fn a_kept_element_names_any_prefixed_id() {
        let e = el("x:A", &[("p:id", "1"), ("xmlns:p", "urn:x")], vec![el("B", &[("q:id", "2"), ("id", "3"), ("p:id", "1")], Vec::new())]);
        let got: Vec<String> = prefixed_ids(&e).iter().map(|v| v.to_string()).collect();
        assert_eq!(got, ["1", "2"]);
    }
}
