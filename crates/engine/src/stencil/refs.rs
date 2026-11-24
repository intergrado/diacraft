//! Whether a kept attribute or element names a part of the stencil's package by a relationship (S5, S7, S8).
//!
//! The model keeps what it has no field for as the file had it, and **names are kept as written, prefix and all** (`passthrough.rs` of
//! `crates/vsdx`): `r:id` and `rel:id` are the same attribute when both prefixes are bound to the relationships namespace. A copy of a master
//! must not carry such a reference to a drawing that does not have the part, so the namespace is judged by its URI and not by the letter `r`:
//!
//! * a prefix bound by an `xmlns:` attribute on the element itself or on an element above it **inside the kept text** is judged by what it is
//!   bound to: the relationships namespace ([`NS_REL`], and the Strict form of it, [`NS_REL_STRICT`]) makes every attribute under it a
//!   relationship, and any other URI makes none;
//! * **a declaration on an outer element only adds**: the typed element the kept text sits in (a shape, a section, a row, a cell, ...)
//!   keeps its declarations as attributes, but nothing makes the writer give them back at the same element, and a kept `r:id` whose `r` was rebound
//!   there would be written under the part's root, where `r` is the relationships namespace, and dangle. So a binding there to the relationships
//!   namespace makes the attributes under it relationships, and a binding there to **any other namespace is not believed**: the prefix is judged
//!   as if unbound, by the next item. Only a declaration on the kept element itself, or on one above it inside the kept text (both are copied
//!   verbatim), lifts it ([`Scope::enter_outer`] and [`Scope::enter`]);
//! * **a prefix bound nowhere in the kept text** was bound above it, by the part's root or by an element the model has a field for, and **the
//!   reader keeps no such binding** (the model holds a part's shapes and not its root). It is judged **closed**: the prefix `r`, which every VSDX
//!   part's root binds to the relationships namespace and which the part a copy is written into binds there too, makes **every** attribute under
//!   it a relationship, whatever its local name; any other unbound prefix makes a relationship of each attribute whose local name is one the
//!   namespace defines ([`REL_LOCALS`]). So an attribute bound out of sight to another namespace under one of those names is refused too: the
//!   refusal is in words and costs one master.
//!
//! **The work is bounded by what the kept text holds, however it is shaped** (S7). The bindings in scope are **one stack for a whole walk**, each
//! declaration pushed when its element is entered and taken back when it is left (so an element of K declarations and N children costs K and N,
//! not K times N), looked up in a hash map by prefix (so an attribute costs one lookup, not a scan of the bindings), and **capped**: an element
//! declares at most [`MAX_XMLNS_PER_ELEMENT`] prefixes and at most [`MAX_XMLNS_IN_SCOPE`] are in scope at once, which no real part comes near (a
//! VSDX part declares one or two). Past either the master is refused ([`TooManyBindings`]), before anything is pushed.
//!
//! **The attributes of the relationships namespace** are the fourteen that `shared-relationshipReference` of ECMA-376 declares from its 2nd
//! edition on: the ten of the 1st edition, `id`, `embed`, `link`, `dm`, `lo`, `qs`, `cs`, `blip`, `pict` and `href`, and four the 2nd added, `topLeft`,
//! `topRight`, `bottomLeft` and `bottomRight`. Part 1 §22.8 says every attribute in the namespace names a relationship, whatever its local name.
//! The namespace has two URIs: Transitional, `http://schemas.openxmlformats.org/officeDocument/2006/relationships` ([`NS_REL`]), the 1st edition's
//! only one; and Strict, `http://purl.oclc.org/ooxml/officeDocument/relationships` ([`NS_REL_STRICT`]), which Part 1 uses from the 3rd edition on.
//! **Cites each of these** (the editions read 2026-11-07: the prefix `r` is not always this
//! namespace, so an attribute is judged by the URI its prefix is bound to). The ten of the 1st edition were first fetched from the Apache POI copy
//! of the schema, <https://sources.debian.org/src/libapache-poi-java/4.0.1-4/ooxml-xsds/shared-relationshipReference.xsd>.
//!
//! **The four of the 2nd edition** ([`REL_LOCALS_PENDING`], a name kept from when they were not yet cited) are refused under an unbound prefix as
//! the ten are. The list is no longer pending a check: R33 is the spec for it.

use std::collections::HashMap;

use diacraft_doc::{Ext, Kept, Master, OdfPage, Page, Passthrough, RawAttrs, RawElement, RawNode};

/// The namespace of `r:id` and its kin (ECMA-376 Part 1, relationships, Transitional).
pub const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The same namespace in the Strict form of the standard, which Part 1 uses from the 3rd edition on.
pub const NS_REL_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// The local names of the attributes the relationships namespace declares in the 1st edition of ECMA-376.
pub const REL_LOCALS: [&str; 10] = ["id", "embed", "link", "dm", "lo", "qs", "cs", "blip", "pict", "href"];

/// Four more local names the namespace declares from the 2nd edition on (WordprocessingML's art page borders use them), refused under an
/// unbound prefix as [`REL_LOCALS`] are. The name is a leftover from when they were pending a check, which R33 now is. A false refusal costs one
/// master and a word of explanation, and a missed reference would be written into a drawing that does not have the part.
pub const REL_LOCALS_PENDING: [&str; 4] = ["topLeft", "topRight", "bottomLeft", "bottomRight"];

/// The most prefixes one kept element may declare. Real parts declare one or two.
pub const MAX_XMLNS_PER_ELEMENT: usize = 64;

/// The most prefixes in scope at once in kept text, the declarations of an element and of all above it counted.
pub const MAX_XMLNS_IN_SCOPE: usize = 256;

/// An element declared more prefixes than a copy reads, or the text nested more than it reads at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyBindings;

// What the tests count of the work: bindings pushed on a scope, for this thread. A test states the work as a count, which does not move with the
// load of the machine.
#[cfg(test)]
thread_local! {
    pub(crate) static BINDINGS_PUSHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether `uri` is the relationships namespace, in either form of the standard.
fn is_rel_namespace(uri: &str) -> bool {
    let uri = uri.trim();
    uri == NS_REL || uri == NS_REL_STRICT
}

/// What a prefix is bound to, and whether the copy carries the declaration with the kept text.
#[derive(Clone, Copy, Debug)]
struct Binding<'a> {
    uri: &'a str,
    /// Declared on an element of the kept text itself (the kept element, or one above it inside the kept text), or on the attributes the judgment
    /// is of: the copy gives the declaration back at the same element. Not so for an **outer** declaration, on the element the model has a field
    /// for that the kept text sits in.
    carried: bool,
}

/// The prefixes bound at the place a walk has reached: **one stack and one map for the whole walk**.
#[derive(Debug, Default)]
pub struct Scope<'a> {
    /// What each prefix is bound to, the nearest last.
    by_prefix: HashMap<&'a str, Vec<Binding<'a>>>,
    /// The prefixes in the order they were bound, so that leaving an element takes back exactly what it declared.
    log: Vec<&'a str>,
}

impl<'a> Scope<'a> {
    pub fn new() -> Scope<'a> {
        Scope::default()
    }

    /// A place to cut back to: leaving the element entered after this call takes back what it declared.
    pub fn mark(&self) -> usize {
        self.log.len()
    }

    /// Takes back every binding made since `mark`.
    pub fn cut_back(&mut self, mark: usize) {
        while self.log.len() > mark {
            let Some(prefix) = self.log.pop() else { break };
            if let Some(stack) = self.by_prefix.get_mut(prefix) {
                stack.pop();
                if stack.is_empty() {
                    self.by_prefix.remove(prefix);
                }
            }
        }
    }

    /// The `xmlns:` declarations of one element, counted (one pass over its attributes, nothing pushed).
    fn declarations(attrs: &RawAttrs) -> usize {
        attrs.iter().filter(|(name, _)| name.strip_prefix("xmlns:").is_some_and(|p| !p.is_empty())).count()
    }

    /// Binds the prefixes `attrs` declares, on an element of the kept text or on the attributes being judged: the copy carries the declaration, so
    /// a binding to another namespace is believed. Past [`MAX_XMLNS_PER_ELEMENT`] on the element, or [`MAX_XMLNS_IN_SCOPE`] in scope with it,
    /// nothing is bound.
    pub fn enter(&mut self, attrs: &'a RawAttrs) -> Result<(), TooManyBindings> {
        self.push(attrs, true)
    }

    /// Binds the prefixes `attrs` declares, on an **outer** element, one the model has a field for that the kept text sits in. Nothing makes the
    /// writer give such a declaration back at the same element, so **it may add a relationship binding and never lift the closed default**: a prefix
    /// bound there to another namespace is judged as if unbound (`r`, and the names the namespace defines, stay relationships) unless a nearer
    /// binding of the kept text says otherwise.
    pub fn enter_outer(&mut self, attrs: &'a RawAttrs) -> Result<(), TooManyBindings> {
        self.push(attrs, false)
    }

    fn push(&mut self, attrs: &'a RawAttrs, carried: bool) -> Result<(), TooManyBindings> {
        let count = Scope::declarations(attrs);
        if count == 0 {
            return Ok(());
        }
        if count > MAX_XMLNS_PER_ELEMENT || self.log.len().saturating_add(count) > MAX_XMLNS_IN_SCOPE {
            return Err(TooManyBindings);
        }
        for (name, value) in attrs.iter() {
            if let Some(prefix) = name.strip_prefix("xmlns:").filter(|p| !p.is_empty()) {
                #[cfg(test)]
                BINDINGS_PUSHED.with(|c| c.set(c.get().saturating_add(1)));
                self.by_prefix.entry(prefix).or_default().push(Binding { uri: value, carried });
                self.log.push(prefix);
            }
        }
        Ok(())
    }

    /// Whether the attribute `name` is in the relationships namespace at this place (see the module documentation for the prefix bound nowhere).
    pub fn is_relationship(&self, name: &str) -> bool {
        let Some((prefix, local)) = name.split_once(':') else {
            return false;
        };
        if prefix == "xmlns" || prefix == "xml" {
            return false;
        }
        let closed = prefix == "r" || REL_LOCALS.contains(&local) || REL_LOCALS_PENDING.contains(&local);
        match self.by_prefix.get(prefix).and_then(|bindings| bindings.last()) {
            Some(binding) if is_rel_namespace(binding.uri) => true,
            // Another namespace, said where the copy carries the declaration: believed.
            Some(binding) if binding.carried => false,
            // Another namespace, said by an outer element: not believed.
            _ => closed,
        }
    }

    /// Whether any attribute of `attrs` is a relationship here (the element's own declarations are bound first by [`Scope::enter`]).
    pub fn any_relationship(&self, attrs: &RawAttrs) -> bool {
        attrs.iter().any(|(name, _)| self.is_relationship(name))
    }
}

/// Whether any of the attributes `attrs` of one element is a relationship, the element's own declarations counting. One pass to count the
/// declarations, and past the caps nothing more: [`TooManyBindings`].
pub fn attrs_refer(attrs: &RawAttrs) -> Result<bool, TooManyBindings> {
    if attrs.is_empty() {
        return Ok(false);
    }
    let mut scope = Scope::new();
    scope.enter(attrs)?;
    Ok(scope.any_relationship(attrs))
}

/// How deep a kept element is followed when what the slot keeps is made again without its references: deeper than any kept markup a file has, and the bound of the walk (a
/// subtree past it, in an entry that holds a reference, is left out and counted with the references).
const MAX_STRIP_DEPTH: usize = 256;

/// **A master that leaves its package, made fit for another** (the second security review; the one step every caller takes: a master brought from a stencil, and a
/// master a paste puts back). It is `Master::leave_package`, which takes out what names a place in the package it was read from, and then **every attribute of the relationships namespace
/// among what its slot keeps is taken out**: the nodes of its part's `Shapes` and `Connects`, the prefixes the root declared, its ODF data and unknown `.diacraft` content. Such an
/// attribute names a row of the part the master had, which the package it is written into does not hold for it (the master is given a part of its own, with no rows), so it would dangle
/// or lead to another part. It is judged as a copy judges kept text (the root's prefixes are in scope for the other entries, an element's own
/// declarations for it and what it holds). **Taken out, not refused**: the rest of what the master keeps stays, and the caller says how many were taken out. An entry whose declarations
/// are past the caps cannot be judged and is left out whole, counted as one. Nothing is copied when the slot holds no reference. → how many were taken out.
pub fn leave_package_for_another(master: &mut Master) -> usize {
    master.leave_package();
    strip_slot(&mut master.ext, "MasterContents", |odf| odf, |odf| odf)
}

/// **A page that is copied inside its drawing's package** (`page.duplicate`): the same step for a page as [`leave_package_for_another`] is for a master. It is
/// `Page::leave_package`, and then **every attribute of the relationships namespace among what the page's slot keeps is taken out**: the nodes of its part's `Shapes` and `Connects`,
/// the prefixes the root declared, its ODF markup and unknown `.diacraft` content. Such an attribute names a row of the part the page had, and the copy is given a part of its own with
/// no rows, so it would dangle. **Taken out, not refused**, and judged as the master's are. → how many were taken out.
pub fn leave_package_for_copy(page: &mut Page) -> usize {
    page.leave_package();
    strip_slot(&mut page.ext, "PageContents", |odf: &OdfPage| &odf.kept, |odf: &mut OdfPage| &mut odf.kept)
}

/// The work both steps share: the relationship attributes of every entry a slot keeps, taken out. `root_key` is the entry the part's root is kept under (`MasterContents`,
/// `PageContents`), whose prefixes are in scope for the other entries; `kept` and `kept_mut` give the markup the slot's ODF data holds (the data itself for a master).
fn strip_slot<T: Default + Clone + PartialEq>(ext: &mut Ext<T>, root_key: &str, kept: fn(&T) -> &Kept, kept_mut: fn(&mut T) -> &mut Kept) -> usize {
    let Some(slot) = ext.get() else { return 0 };
    let root = slot.vsdx.0.iter().find(|(key, _)| &**key == root_key).map(|(_, held)| held.raw.clone());
    let mut changes: Vec<(usize, Option<Kept>, usize)> = Vec::new();
    for (at, (key, held)) in slot.vsdx.0.iter().enumerate() {
        let outer = if &**key == root_key { None } else { root.as_ref() };
        if let Some(change) = stripped(held, outer) {
            changes.push((at, change.0, change.1));
        }
    }
    let odf = stripped(kept(&slot.odf), None);
    let native = stripped(&slot.native, None);
    if changes.is_empty() && odf.is_none() && native.is_none() {
        return 0;
    }
    let mut count = 0_usize;
    let data = ext.make_mut();
    // From the last, so that an entry left out does not move the places of the ones before it.
    for (at, held, n) in changes.into_iter().rev() {
        count = count.saturating_add(n);
        match held {
            Some(held) => {
                if let Some(entry) = data.vsdx.0.get_mut(at) {
                    entry.1 = held;
                }
            }
            None => {
                if at < data.vsdx.0.len() {
                    data.vsdx.0.remove(at);
                }
            }
        }
    }
    if let Some((held, n)) = odf {
        count = count.saturating_add(n);
        *kept_mut(&mut data.odf) = held.unwrap_or_default();
    }
    if let Some((held, n)) = native {
        count = count.saturating_add(n);
        data.native = held.unwrap_or_default();
    }
    ext.tidy();
    count
}

/// `kept` made again without its relationship attributes, and how many were taken out: `None` when it holds none (nothing to change), `Some((None, 1))` when its declarations are past
/// the caps and it is left out whole.
fn stripped(kept: &Kept, outer: Option<&RawAttrs>) -> Option<(Option<Kept>, usize)> {
    if kept.raw.is_empty() && kept.kept.is_empty() {
        return None;
    }
    let mut scope = Scope::new();
    let judged = (|| -> Result<Option<(Kept, usize)>, TooManyBindings> {
        if let Some(outer) = outer {
            scope.enter_outer(outer)?;
        }
        scope.enter(&kept.raw)?;
        let mut found = kept.raw.iter().filter(|(name, _)| scope.is_relationship(name)).count();
        for placed in kept.kept.iter() {
            found = found.saturating_add(references(&mut scope, &placed.node, 0)?);
        }
        if found == 0 {
            return Ok(None);
        }
        let mut count = 0_usize;
        let raw = without_references(&scope, &kept.raw, &mut count);
        let mut nodes = Passthrough::new();
        for placed in kept.kept.iter() {
            if let Some(node) = rebuilt(&mut scope, &placed.node, 0, &mut count)? {
                nodes.push(placed.after, node);
            }
        }
        Ok(Some((Kept { raw, kept: nodes }, count)))
    })();
    match judged {
        Ok(None) => None,
        Ok(Some((kept, count))) => Some((Some(kept), count)),
        Err(TooManyBindings) => Some((None, 1)),
    }
}

/// How many relationship attributes `node` and what it holds carry, in `scope` (left as it was). A subtree past [`MAX_STRIP_DEPTH`] counts as one, as [`rebuilt`] leaves it out.
fn references<'a>(scope: &mut Scope<'a>, node: &'a RawNode, depth: usize) -> Result<usize, TooManyBindings> {
    let RawNode::Element(element) = node else { return Ok(0) };
    if depth >= MAX_STRIP_DEPTH {
        return Ok(1);
    }
    let mark = scope.mark();
    scope.enter(&element.attrs)?;
    let mut found = element.attrs.iter().filter(|(name, _)| scope.is_relationship(name)).count();
    for child in &element.children {
        found = found.saturating_add(references(scope, child, depth + 1)?);
    }
    scope.cut_back(mark);
    Ok(found)
}

/// `node` made again without its relationship attributes, in `scope` (left as it was), counting what is taken out in `count`; `None` for a subtree past [`MAX_STRIP_DEPTH`].
fn rebuilt<'a>(scope: &mut Scope<'a>, node: &'a RawNode, depth: usize, count: &mut usize) -> Result<Option<RawNode>, TooManyBindings> {
    let RawNode::Element(element) = node else { return Ok(Some(node.clone())) };
    if depth >= MAX_STRIP_DEPTH {
        *count = count.saturating_add(1);
        return Ok(None);
    }
    let mark = scope.mark();
    scope.enter(&element.attrs)?;
    let attrs = without_references(scope, &element.attrs, count);
    let mut children = Vec::with_capacity(element.children.len());
    for child in &element.children {
        if let Some(child) = rebuilt(scope, child, depth + 1, count)? {
            children.push(child);
        }
    }
    scope.cut_back(mark);
    Ok(Some(RawNode::Element(RawElement { name: element.name.clone(), attrs, children })))
}

/// `attrs` without those `scope` judges to be relationships, counting them in `count`.
fn without_references(scope: &Scope<'_>, attrs: &RawAttrs, count: &mut usize) -> RawAttrs {
    let before = *count;
    let kept: Vec<(&str, &str)> = attrs
        .iter()
        .filter(|(name, _)| {
            let goes = scope.is_relationship(name);
            if goes {
                *count = count.saturating_add(1);
            }
            !goes
        })
        .collect();
    if *count == before { attrs.clone() } else { RawAttrs::from_pairs(kept) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> RawAttrs {
        RawAttrs::from_pairs(pairs.iter().copied())
    }

    fn rel(pairs: &[(&str, &str)]) -> bool {
        attrs_refer(&attrs(pairs)).unwrap()
    }

    #[test]
    fn a_relationship_attribute_is_found_by_its_namespace_and_not_its_prefix() {
        // Bound here: any prefix, in either form of the standard.
        assert!(rel(&[("xmlns:rel", NS_REL), ("rel:id", "rId1")]));
        assert!(rel(&[("xmlns:q", NS_REL), ("q:embed", "rId1")]));
        assert!(rel(&[("xmlns:s", NS_REL_STRICT), ("s:id", "rId1")]));
        assert!(rel(&[("xmlns:q", " http://purl.oclc.org/ooxml/officeDocument/relationships "), ("q:anything", "x")]));
        // Bound here to another namespace: not a relationship, even as `r:id`.
        assert!(!rel(&[("xmlns:r", "urn:other"), ("r:id", "7")]));
        // Not namespaced, or a declaration.
        assert!(!rel(&[("id", "5"), ("xmlns:r", NS_REL), ("xml:space", "preserve")]));
    }

    #[test]
    fn a_prefix_bound_out_of_sight_is_judged_closed() {
        // `r` is the prefix the part's root and the part a copy is written into bind to the relationships namespace: whatever it is called after,
        // an attribute under it is a relationship.
        for local in ["id", "embed", "somethingelse", "x"] {
            assert!(rel(&[(&format!("r:{local}"), "v")]), "{local}");
        }
        // Another prefix: by the names the namespace defines, all ten.
        for local in REL_LOCALS {
            assert!(rel(&[(&format!("rel:{local}"), "v")]), "{local}");
        }
        assert!(!rel(&[("rel:name", "v"), ("foo:other", "v")]));
    }

    #[test]
    fn the_four_names_of_the_second_edition_are_refused_under_an_unbound_prefix() {
        // `topLeft`, `topRight`, `bottomLeft` and `bottomRight`: the 2nd edition of ECMA-376 added them to the ten of the 1st.
        for local in ["topLeft", "topRight", "bottomLeft", "bottomRight"] {
            assert!(rel(&[(&format!("rel:{local}"), "v")]), "{local}, unbound");
            assert!(rel(&[("xmlns:q", NS_REL_STRICT), (&format!("q:{local}"), "v")]), "{local}, bound to the Strict namespace");
            assert!(!rel(&[("xmlns:q", "urn:other"), (&format!("q:{local}"), "v")]), "{local}, bound to another namespace");
        }
        assert!(REL_LOCALS_PENDING.iter().all(|name| !REL_LOCALS.contains(name)), "they are a list of their own: the ten are the 1st edition's");
    }

    #[test]
    fn the_bindings_are_one_stack_taken_back_on_leaving_and_looked_up_by_prefix() {
        let outer = attrs(&[("xmlns:q", NS_REL)]);
        let inner = attrs(&[("xmlns:q", "urn:other")]);
        let mut scope = Scope::new();
        scope.enter(&outer).unwrap();
        assert!(scope.is_relationship("q:x"));
        let mark = scope.mark();
        scope.enter(&inner).unwrap();
        assert!(!scope.is_relationship("q:x"), "the nearest binding wins");
        scope.cut_back(mark);
        assert!(scope.is_relationship("q:x"), "and is taken back on leaving");
        scope.cut_back(0);
        assert!(!scope.is_relationship("q:x"));
    }

    #[test]
    fn a_declaration_of_an_outer_element_only_adds_a_relationship_binding_and_never_lifts_the_closed_default() {
        // The element the model has a field for is written by a writer that need not give its declaration back at the same element, so what it
        // binds is good for adding a relationship, never for clearing one.
        let to_rel = attrs(&[("xmlns:q", NS_REL)]);
        let to_other_r = attrs(&[("xmlns:r", "urn:other"), ("xmlns:q", "urn:other")]);
        let mut scope = Scope::new();
        scope.enter_outer(&to_rel).unwrap();
        assert!(scope.is_relationship("q:custom"), "an outer binding to the namespace adds one");
        scope.cut_back(0);
        scope.enter_outer(&to_other_r).unwrap();
        for name in ["r:id", "r:whatever", "r:embed", "q:id", "q:link", "q:topLeft"] {
            assert!(scope.is_relationship(name), "{name}: bound to another namespace by an outer element, it is still the closed default");
        }
        assert!(!scope.is_relationship("q:custom") && !scope.is_relationship("foo:x"), "and nothing else becomes one");
        // The kept element's own declaration, which the copy carries, does lift it, and is taken back with its element.
        let own = attrs(&[("xmlns:r", "urn:third")]);
        let mark = scope.mark();
        scope.enter(&own).unwrap();
        assert!(!scope.is_relationship("r:id"), "declared on the kept element itself");
        scope.cut_back(mark);
        assert!(scope.is_relationship("r:id"));
        // An outer binding to the namespace is cleared by a nearer carried one.
        scope.cut_back(0);
        scope.enter_outer(&to_rel).unwrap();
        let nearer = attrs(&[("xmlns:q", "urn:other")]);
        scope.enter(&nearer).unwrap();
        assert!(!scope.is_relationship("q:custom"));
    }

    #[test]
    fn the_declarations_are_capped_per_element_and_in_scope_before_anything_is_pushed() {
        let many: Vec<(String, String)> = (0..=MAX_XMLNS_PER_ELEMENT).map(|k| (format!("xmlns:p{k}"), format!("urn:{k}"))).collect();
        let too_many = RawAttrs::from_pairs(many.iter().map(|(a, b)| (a.as_str(), b.as_str())));
        assert_eq!(attrs_refer(&too_many), Err(TooManyBindings));
        let few: Vec<(String, String)> = (0..MAX_XMLNS_PER_ELEMENT).map(|k| (format!("xmlns:p{k}"), format!("urn:{k}"))).collect();
        let exactly = RawAttrs::from_pairs(few.iter().map(|(a, b)| (a.as_str(), b.as_str())));
        assert_eq!(attrs_refer(&exactly), Ok(false));
        // In scope: four elements of sixty-four fill 256, and the fifth is refused.
        let mut scope = Scope::new();
        for _ in 0..MAX_XMLNS_IN_SCOPE / MAX_XMLNS_PER_ELEMENT {
            scope.enter(&exactly).unwrap();
        }
        assert_eq!(scope.enter(&exactly), Err(TooManyBindings));
        assert_eq!(scope.mark(), MAX_XMLNS_IN_SCOPE, "nothing was pushed by the refused element");
    }
}
