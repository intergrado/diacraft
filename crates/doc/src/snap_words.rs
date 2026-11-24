//! The Snap & Glue words of `DocumentSettings`: where real files keep the document's snap and glue settings.
//!
//! `DocumentSettings` holds `GlueSettings`, `SnapSettings`, `SnapExtensions`, `SnapAngles` and `DynamicGridEnabled`, in
//! that order, in every sample. The element is one the model has no typed field for, so it lives in [`Document::passthrough`] as the file had it
//! **There is no second copy of the words:** [`Document::snap_words`] reads them from that element, and
//! [`Document::set_snap_words`] changes them in it, and changes nothing else in it: the attributes, an unknown child, a blank between two
//! children and a word that is not changed stay as they were. What the words mean (which bit is which option) is the engine's business
//! (`diacraft-engine`'s `snap::settings`); here a word is the text the file holds.

use std::sync::Arc;

use crate::document::Document;
use crate::raw::{Passthrough, Placed, RawAttrs, RawElement, RawNode};

/// The element that holds the words.
const SETTINGS: &str = "DocumentSettings";

/// The children of `DocumentSettings` that files write for snapping, in the order they write them. `SnapAngles` is not a
/// word: it holds a list, and nothing here changes it.
const GLUE: &str = "GlueSettings";
const SNAP: &str = "SnapSettings";
const EXTENSIONS: &str = "SnapExtensions";
const ANGLES: &str = "SnapAngles";
const DYNAMIC: &str = "DynamicGridEnabled";
const ORDER: [&str; 5] = [GLUE, SNAP, EXTENSIONS, ANGLES, DYNAMIC];

/// The text of the Snap & Glue children of `DocumentSettings`, each as the file holds it (trimmed), or `None` for a child the file lacks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapWords {
    /// `GlueSettings`: the options of Glue to (a bit mask in files).
    pub glue: Option<Arc<str>>,
    /// `SnapSettings`: the options of Snap to (a bit mask in files).
    pub snap: Option<Arc<str>>,
    /// `SnapExtensions`: the options of the extension lines (a bit mask in files). Kept, not changed by any command yet.
    pub extensions: Option<Arc<str>>,
    /// `DynamicGridEnabled`: 0 or 1 in files.
    pub dynamic_grid: Option<Arc<str>>,
}

/// An element name without its prefix.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// The namespaces of the target application's drawing files (the writer's `Ns`): an element in either, or in none, is the file's own.
const NS_2012: &str = "http://schemas.microsoft.com/office/visio/2012/main"; // brand-ok: a namespace the format defines
const NS_2011: &str = "http://schemas.microsoft.com/office/visio/2011/1/core"; // brand-ok: a namespace the format defines

/// The name of the attribute that binds the prefix of an element name (`xmlns` for a name with none, `xmlns:p` for `p:name`).
pub fn namespace_attribute(name: &str) -> String {
    name.split_once(':').map_or_else(|| "xmlns".to_owned(), |(prefix, _)| format!("xmlns:{prefix}"))
}

/// Whether a namespace a start tag binds its own name to is one the target application's files do not use. **The one rule by which the model and the writer
/// agree on which element is a word**: a `SnapSettings` that declares `xmlns="urn:x"` is another application's element, whatever its
/// local name. An element whose prefix is bound out of its own tag (on the root, say) cannot be judged here; the writer, which resolves every prefix, checks
/// that its choice is the model's and leaves the words alone, with a warning, when it is not.
pub fn is_other_namespace(uri: &str) -> bool {
    !uri.is_empty() && uri != NS_2012 && uri != NS_2011
}

/// Whether `el` is the element called `name`: that local name, and not a namespace of another application declared on the element itself.
fn is_named(el: &RawElement, name: &str) -> bool {
    local(&el.name) == name && !el.attrs.get(&namespace_attribute(&el.name)).is_some_and(is_other_namespace)
}

fn is_settings(node: &RawNode) -> Option<&RawElement> {
    match node {
        RawNode::Element(el) if is_named(el, SETTINGS) => Some(el),
        _ => None,
    }
}

/// The text a word child holds: its text nodes together.
fn text_of(el: &RawElement) -> String {
    el.children
        .iter()
        .filter_map(|n| match n {
            RawNode::Text(t) => Some(&**t),
            _ => None,
        })
        .collect()
}

/// The first child element called `name` (by its local name) and its text, trimmed.
fn word(settings: &RawElement, name: &str) -> Option<Arc<str>> {
    settings.children.iter().find_map(|c| match c {
        RawNode::Element(el) if is_named(el, name) => Some(Arc::from(text_of(el).trim())),
        _ => None,
    })
}

/// Where a child called `name` goes in `children`: after the last known child that files write before it, else before the first they write
/// after it, else at the end.
fn place_for(children: &[RawNode], name: &str) -> usize {
    let rank = |n: &str| ORDER.iter().position(|o| *o == n);
    let Some(mine) = rank(name) else { return children.len() };
    let known = |c: &RawNode| match c {
        RawNode::Element(el) if !el.attrs.get(&namespace_attribute(&el.name)).is_some_and(is_other_namespace) => rank(local(&el.name)),
        _ => None,
    };
    if let Some(at) = children.iter().rposition(|c| known(c).is_some_and(|r| r < mine)) {
        return at + 1;
    }
    children.iter().position(|c| known(c).is_some_and(|r| r > mine)).unwrap_or(children.len())
}

/// Makes the word `name` of `settings` say `value` (`None` takes the child out). Does nothing when it already says that.
fn put_word(settings: &mut RawElement, name: &str, value: Option<&Arc<str>>) {
    let at = settings.children.iter().position(|c| matches!(c, RawNode::Element(el) if is_named(el, name)));
    match (at, value) {
        (Some(at), Some(v)) => {
            if let Some(RawNode::Element(el)) = settings.children.get_mut(at)
                && text_of(el).trim() != &**v
            {
                el.children = vec![RawNode::Text(Arc::clone(v))];
            }
        }
        (Some(at), None) => {
            settings.children.remove(at);
        }
        (None, Some(v)) => {
            let at = place_for(&settings.children, name);
            let child = RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::clone(v))] };
            settings.children.insert(at, RawNode::Element(child));
        }
        (None, None) => {}
    }
}

/// Whether a child element is one of the words.
fn is_word(node: &RawNode) -> bool {
    matches!(node, RawNode::Element(el) if [GLUE, SNAP, EXTENSIONS, DYNAMIC].iter().any(|w| is_named(el, w)))
}

/// Whether a node is a `DocumentSettings` that holds the words alone: no attribute, and no child but a word or a blank.
fn words_only(node: &RawNode) -> bool {
    let Some(settings) = is_settings(node) else { return false };
    settings.attrs.is_empty() && settings.children.iter().all(|c| is_word(c) || matches!(c, RawNode::Text(t) if t.trim().is_empty()))
}

/// Whether two unknown nodes are the same, the words of a `DocumentSettings` aside.
fn same_node(a: &RawNode, b: &RawNode) -> bool {
    match (is_settings(a), is_settings(b)) {
        (Some(x), Some(y)) => {
            x.name == y.name && x.attrs == y.attrs && x.children.iter().filter(|c| !is_word(c)).eq(y.children.iter().filter(|c| !is_word(c)))
        }
        (None, None) => a == b,
        _ => false,
    }
}

impl Document {
    /// The Snap & Glue words of `DocumentSettings`, as the file holds them. Every one is `None` for a document with no such element.
    pub fn snap_words(&self) -> SnapWords {
        let Some(settings) = self.passthrough.iter().find_map(|p| is_settings(&p.node)) else { return SnapWords::default() };
        SnapWords {
            glue: word(settings, GLUE),
            snap: word(settings, SNAP),
            extensions: word(settings, EXTENSIONS),
            dynamic_grid: word(settings, DYNAMIC),
        }
    }

    /// Makes the Snap & Glue words of `DocumentSettings` `words`: a word that is the same is not touched, one that differs is replaced where it
    /// is, one the element lacks goes in where files put it, and one that is `None` is taken out. The element is made, first in the document, when
    /// there is none and a word is to be written. Nothing else in the element, or in the document, changes.
    pub fn set_snap_words(&mut self, words: &SnapWords) {
        let wanted = [(GLUE, &words.glue), (SNAP, &words.snap), (EXTENSIONS, &words.extensions), (DYNAMIC, &words.dynamic_grid)];
        if self.snap_words() == *words {
            return;
        }
        let list = self.passthrough.nodes_mut();
        let at = list.iter().position(|p| is_settings(&p.node).is_some());
        let at = match at {
            Some(at) => at,
            None => {
                if wanted.iter().all(|(_, v)| v.is_none()) {
                    return;
                }
                let made = RawElement { name: Arc::from(SETTINGS), attrs: RawAttrs::new(), children: Vec::new() };
                list.insert(0, Placed { after: 0, node: RawNode::Element(made) });
                0
            }
        };
        if let Some(Placed { node: RawNode::Element(settings), .. }) = list.get_mut(at) {
            for (name, value) in wanted {
                put_word(settings, name, value.as_ref());
            }
        }
    }

    /// Whether the unknown nodes of this document and of `other` are the same but for the Snap & Glue words, compared in place: nothing is copied (the
    /// writer asks this of the two models at every save, and a document's unknown nodes may be many). A `DocumentSettings` that
    /// holds the words alone (no attribute, no other child) is not counted, as in [`Document::passthrough_but_snap_words`].
    pub fn same_but_snap_words(&self, other: &Document) -> bool {
        let counted = |p: &&Placed| !words_only(&p.node);
        let (mut a, mut b) = (self.passthrough.iter().filter(counted), other.passthrough.iter().filter(counted));
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) if x.after == y.after && same_node(&x.node, &y.node) => {}
                _ => return false,
            }
        }
    }

    /// Whether the document has unknown nodes other than the Snap & Glue words (a `DocumentSettings` that holds the words alone is none).
    pub fn holds_more_than_snap_words(&self) -> bool {
        self.passthrough.iter().any(|p| !words_only(&p.node))
    }

    /// The document's unknown nodes without the Snap & Glue words: what the writer compares to know that the rest of them is as the file had it.
    /// A `DocumentSettings` that is nothing once the words are out (no attribute, no other child) is left out too, so that a document whose
    /// settings were made by the model, for the words alone, has none.
    pub fn passthrough_but_snap_words(&self) -> Passthrough {
        let mut kept: Vec<Placed> = Vec::new();
        for placed in self.passthrough.iter() {
            let Some(settings) = is_settings(&placed.node) else {
                kept.push(placed.clone());
                continue;
            };
            let mut rest = settings.clone();
            rest.children.retain(|c| !is_word(c));
            let blank = rest.children.iter().all(|c| matches!(c, RawNode::Text(t) if t.trim().is_empty()));
            if rest.attrs.is_empty() && blank {
                continue;
            }
            kept.push(Placed { after: placed.after, node: RawNode::Element(rest) });
        }
        Passthrough::from_nodes(kept)
    }
}
