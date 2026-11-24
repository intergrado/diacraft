//! DiaCraft's own data in the document: the `Data` element of the DiaCraft solution part.
//!
//! Puts DiaCraft-only data in a solution part named `DiaCraft`, whose payload is one `Data` element in
//! [`DIACRAFT_URN`] with a `version` attribute in that namespace; R28 says it holds only what has no place in the ShapeSheet, that an unknown version
//! or an unknown child is kept and not interpreted, and that a file with no such data gets no part. [`Document::diacraft`] is that element as read
//! (the reader of a file fills it; the writer writes it), and these are the ways to read and change one child of it: the engine keeps the Snap & Glue
//! options that no specification gives a bit for in a child called `snap` (the file's own `SnapSettings`, `GlueSettings` and
//! `SnapExtensions` carry no bit DiaCraft made up).

use std::sync::Arc;

use crate::document::Document;
use crate::raw::{RawAttrs, RawElement, RawNode};

/// The namespace of the payload of the DiaCraft part, and of DiaCraft's own markup in every format: the one every writer writes.
/// It must not vary once a release has written it. (APPID: it is based on the domain `diacraft.app`, as the application's id `app.diacraft` is.)
pub const DIACRAFT_URN: &str = "urn:app.diacraft:extension";

/// The namespace that builds made before the id moved to the domain `diacraft.app` wrote (APPID, owner 2026-11-23). **It is read as DiaCraft's own and never written**:
/// a reader asks [`is_diacraft_urn`] and not `==` [`DIACRAFT_URN`], so that a file made before opens as it did.
pub const DIACRAFT_URN_LEGACY: &str = "urn:io.github.one_bit.diacraft:extension";

/// Whether `uri` is one of DiaCraft's own namespaces: [`DIACRAFT_URN`], or [`DIACRAFT_URN_LEGACY`] that earlier builds wrote.
pub fn is_diacraft_urn(uri: &str) -> bool {
    uri == DIACRAFT_URN || uri == DIACRAFT_URN_LEGACY
}

/// The namespace a writer writes for `uri`: [`DIACRAFT_URN`] for [`DIACRAFT_URN_LEGACY`], and any other URI as it is.
pub fn current_namespace(uri: &str) -> &str {
    if uri == DIACRAFT_URN_LEGACY { DIACRAFT_URN } else { uri }
}

/// Whether an attribute called `name` is a namespace declaration (`xmlns` or `xmlns:prefix`).
fn is_declaration(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

/// Makes every declaration of [`DIACRAFT_URN_LEGACY`] in `element` and everything inside it one of [`DIACRAFT_URN`]: the markup a model keeps whole, read from a file
/// that an earlier build wrote, is then what a build of this one writes (APPID). Nothing else changes: another namespace, another attribute and every name are as they were,
/// and an element that declares no such namespace is not copied. A walk with no recursion, one step for each element.
pub fn use_current_namespace(element: &mut RawElement) {
    let mut open: Vec<&mut RawElement> = vec![element];
    while let Some(e) = open.pop() {
        let RawElement { attrs, children, .. } = e;
        if attrs.iter().any(|(name, value)| is_declaration(name) && value == DIACRAFT_URN_LEGACY) {
            let moved = attrs.iter().map(|(name, value)| (name, if is_declaration(name) { current_namespace(value) } else { value }));
            *attrs = RawAttrs::from_pairs(moved.collect::<Vec<_>>());
        }
        for child in children.iter_mut() {
            if let RawNode::Element(inner) = child {
                open.push(inner);
            }
        }
    }
}

/// Makes `element` declare, as [`DIACRAFT_URN`], each prefix (or the default namespace) that it or what it holds uses and that `outer` (the declarations of the elements it stands in: a
/// flat drawing's root) binds to [`DIACRAFT_URN_LEGACY`], where the element does not declare that prefix itself. The markup a model keeps whole, read from a file that an earlier build wrote, is
/// then in the current namespace wherever the model is written (an element that relied on the root's binding would be written in the old namespace under the root's declaration, which a save
/// that writes only the current one must not do). A declaration put there goes first among the element's attributes. A walk with no recursion, one step for each element.
pub fn declare_inherited_namespace<'a>(element: &mut RawElement, outer: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let bound: Vec<&str> =
        outer.into_iter().filter(|(name, value)| is_declaration(name) && *value == DIACRAFT_URN_LEGACY).map(|(name, _)| name).collect();
    if bound.is_empty() {
        return;
    }
    let wanted: Vec<String> = {
        // The prefixes the element or what it holds uses (`""` for the default namespace, which an element with no prefix is in; an attribute with none is in no namespace), found in one walk,
        // and the names the element declares itself: each is looked up once for each declaration of the outer elements, which may be many.
        let mut used: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut open: Vec<&RawElement> = vec![&*element];
        while let Some(e) = open.pop() {
            used.insert(prefix(&e.name).unwrap_or(""));
            for (name, _) in e.attrs.iter() {
                if let Some(p) = prefix(name).filter(|p| *p != "xmlns" && *p != "xml") {
                    used.insert(p);
                }
            }
            for child in &e.children {
                if let RawNode::Element(inner) = child {
                    open.push(inner);
                }
            }
        }
        let own: std::collections::HashSet<&str> = element.attrs.iter().map(|(name, _)| name).collect();
        bound.into_iter().filter(|name| used.contains(name.strip_prefix("xmlns:").unwrap_or("")) && !own.contains(name)).map(str::to_owned).collect()
    };
    if wanted.is_empty() {
        return;
    }
    let own = element.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned()));
    let all: Vec<(String, String)> = wanted.into_iter().map(|name| (name, DIACRAFT_URN.to_owned())).chain(own).collect();
    element.attrs = RawAttrs::from_pairs(all);
}

/// The `version` this build reads and writes. Data of another version is kept as it is and not interpreted.
pub const DIACRAFT_VERSION: &str = "1";

/// The prefix a `Data` element made here binds to [`DIACRAFT_URN`].
const PREFIX: &str = "dc";

/// The child of the data that holds the Page Breaks box (View, Show, Page Breaks) where the drawing's window does not: its attribute `on` is `1` for a box
/// turned on. Puts the box in the drawing window's `ShowPageBreaks` where a drawing has a VSDX place for it ([`crate::WindowFlags::page_breaks`]),
/// and reads the window first.
pub const PAGE_BREAKS_CHILD: &str = "page_breaks";

/// An element or attribute name without its prefix.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// The prefix of a name, if it has one.
fn prefix(name: &str) -> Option<&str> {
    name.split_once(':').map(|(prefix, _)| prefix)
}

/// The `version` the data states: the attribute called `version` in the prefix of the element's own name.
fn version(data: &RawElement) -> Option<&str> {
    let want = prefix(&data.name);
    data.attrs.iter().find(|(name, _)| local(name) == "version" && prefix(name) == want).map(|(_, value)| value)
}

impl Document {
    /// What the data says of the Page Breaks box ([`PAGE_BREAKS_CHILD`]): `Some(true)` for `on` `1` or `true`, `Some(false)` for `0` or `false` (blanks round them aside),
    /// `None` when it holds no such child or another value.
    pub fn page_breaks_in_data(&self) -> Option<bool> {
        crate::window_flags::value_of(self.diacraft_child(PAGE_BREAKS_CHILD)?.attrs.get("on")?)
    }

    /// The child of the DiaCraft data called `local` (by its name without the prefix), if the document has the data, the data is of the version this
    /// build reads, and the child is there. Data of another version reads as holding nothing: it is not interpreted.
    pub fn diacraft_child(&self, local_name: &str) -> Option<&RawElement> {
        let data = self.diacraft.as_deref()?;
        if version(data) != Some(DIACRAFT_VERSION) {
            return None;
        }
        data.children.iter().find_map(|c| match c {
            RawNode::Element(el) if local(&el.name) == local_name => Some(el),
            _ => None,
        })
    }

    /// Makes the child of the DiaCraft data called `local` hold exactly these attributes (`None` takes it out). The data element is made when there is
    /// none and a child is to be set; a child is replaced where it is, and a new one goes last, in the prefix of the data element. Nothing else in the
    /// element changes, and a call that changes nothing leaves the element as it was. Taking out the last child takes the element out too: data with nothing
    /// in it is none.
    ///
    /// → `false` when the data is of a version this build does not read: it is kept as it is and nothing is changed.
    pub fn set_diacraft_child(&mut self, local_name: &str, attrs: Option<&[(&str, &str)]>) -> bool {
        let Some(current) = self.diacraft.as_deref() else {
            let Some(attrs) = attrs else { return true };
            let data = RawElement {
                name: Arc::from(format!("{PREFIX}:Data")),
                attrs: RawAttrs::from_pairs([
                    (format!("xmlns:{PREFIX}"), DIACRAFT_URN.to_owned()),
                    (format!("{PREFIX}:version"), DIACRAFT_VERSION.to_owned()),
                ]),
                children: vec![RawNode::Element(made(&format!("{PREFIX}:{local_name}"), attrs))],
            };
            self.diacraft = Some(Arc::new(data));
            return true;
        };
        if version(current) != Some(DIACRAFT_VERSION) {
            return false;
        }
        let at = current.children.iter().position(|c| matches!(c, RawNode::Element(el) if local(&el.name) == local_name));
        let mut next = current.clone();
        let mut emptied = false;
        match (at, attrs) {
            (Some(at), Some(attrs)) => {
                if let Some(RawNode::Element(el)) = next.children.get_mut(at) {
                    *el = made(&el.name, attrs);
                }
            }
            (Some(at), None) => {
                next.children.remove(at);
                // Data with nothing left in it is no data: a file with none gets no part. Blanks between children are nothing.
                emptied = next.children.iter().all(|c| matches!(c, RawNode::Text(t) if t.trim().is_empty()));
            }
            (None, Some(attrs)) => {
                let name = match prefix(&next.name) {
                    Some(p) => format!("{p}:{local_name}"),
                    None => local_name.to_owned(),
                };
                next.children.push(RawNode::Element(made(&name, attrs)));
            }
            (None, None) => {}
        }
        if emptied {
            self.diacraft = None;
        } else if &next != current {
            self.diacraft = Some(Arc::new(next));
        }
        true
    }
}

/// An element called `name` with these attributes and nothing in it.
fn made(name: &str, attrs: &[(&str, &str)]) -> RawElement {
    RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() }
}
