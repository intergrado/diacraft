//! Namespaces: the URIs of the ODF vocabularies this crate looks for, and a walk of a tree that resolves each element's and attribute's
//! prefix to its URI as XML Namespaces 1.0 does (a prefix is bound by the nearest `xmlns:p` on the element or above it, an unprefixed
//! element by the nearest `xmlns`, and an unprefixed attribute is in no namespace).
//!
//! The container's tree keeps names as written and resolves nothing (`diacraft_container::xml`). ODF names are matched by namespace and not
//! by prefix (a producer may bind any prefix), so the manifest and the macro scan resolve them here.
//!
//! **What it costs**: one visit for each element, and for each attribute one look in a map, so a tree that declares many prefixes on one
//! element and uses them in many below costs what it holds and not their product. The walk keeps an explicit stack, so no tree makes it
//! recurse.

use std::collections::HashMap;

use diacraft_container::xml::{El, Node};

/// The manifest's namespace (ODF 1.4 Part 2, the manifest schema).
pub(crate) const MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
/// The `office` namespace (ODF Part 3): `office:script`, `office:event-listeners`.
pub(crate) const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
/// The `script` namespace (ODF Part 3): `script:event-listener`.
pub(crate) const SCRIPT: &str = "urn:oasis:names:tc:opendocument:xmlns:script:1.0";
/// The `presentation` namespace (ODF Part 3): `presentation:event-listener`, which binds an event to an action of the presentation
/// vocabulary and not to a script.
pub(crate) const PRESENTATION: &str = "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0";
/// The namespace the prefix `xml` is bound to without a declaration (XML Namespaces 1.0, section 3).
pub(crate) const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// The bindings in scope at one point of a walk: each prefix (`""` for the default namespace) to the stack of URIs bound to it, the
/// innermost last. An empty URI unbinds.
#[derive(Default)]
pub(crate) struct Scopes<'t> {
    bound: HashMap<&'t str, Vec<&'t str>>,
}

/// A name as written, split at its first colon: `(prefix, local)`, with `""` for no prefix.
fn split(name: &str) -> (&str, &str) {
    name.split_once(':').unwrap_or(("", name))
}

impl<'t> Scopes<'t> {
    /// The URI `prefix` is bound to here, or `None`.
    fn uri(&self, prefix: &str) -> Option<&'t str> {
        if prefix == "xml" {
            return Some(XML);
        }
        self.bound.get(prefix).and_then(|stack| stack.last()).copied().filter(|uri| !uri.is_empty())
    }

    /// The namespace and local name of an element named `name` here.
    pub(crate) fn element<'n>(&self, name: &'n str) -> (Option<&'t str>, &'n str) {
        let (prefix, local) = split(name);
        (self.uri(prefix), local)
    }

    /// The namespace and local name of an attribute named `name` here: an unprefixed attribute is in no namespace.
    pub(crate) fn attribute<'n>(&self, name: &'n str) -> (Option<&'t str>, &'n str) {
        match name.split_once(':') {
            Some((prefix, local)) => (self.uri(prefix), local),
            None => (None, name),
        }
    }

    /// The value of the attribute of `el` that is `local` in `namespace`, the first if there are two.
    pub(crate) fn attr<'e>(&self, el: &'e El, namespace: &str, local: &str) -> Option<&'e str> {
        el.attrs.iter().find(|(k, _)| !is_declaration(k) && self.attribute(k) == (Some(namespace), local)).map(|(_, v)| v.as_str())
    }

    /// Declares the bindings `el` makes, and gives how many prefixes it pushed onto `declared`.
    fn enter(&mut self, el: &'t El, declared: &mut Vec<&'t str>) -> usize {
        let mut pushed = 0;
        for (key, value) in &el.attrs {
            let prefix = if key == "xmlns" {
                ""
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                prefix
            } else {
                continue;
            };
            self.bound.entry(prefix).or_default().push(value.as_str());
            declared.push(prefix);
            pushed += 1;
        }
        pushed
    }

    /// Undoes the last `count` declarations.
    fn leave(&mut self, count: usize, declared: &mut Vec<&'t str>) {
        for _ in 0..count {
            if let Some(prefix) = declared.pop()
                && let Some(stack) = self.bound.get_mut(prefix)
            {
                stack.pop();
            }
        }
    }
}

/// Whether an attribute written `key` declares a namespace.
pub(crate) fn is_declaration(key: &str) -> bool {
    key == "xmlns" || key.starts_with("xmlns:")
}

/// What a visit of an element tells the walk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Go on into the element's children.
    Into,
    /// End the walk.
    Stop,
}

/// Walks `root` and every element in it in document order, with the bindings in scope at each, and calls `visit` with the element, its
/// depth (0 for the root) and the bindings. Gives the elements visited.
pub(crate) fn walk<'t>(root: &'t El, mut visit: impl FnMut(&'t El, usize, &Scopes<'t>) -> Flow) -> u64 {
    enum Step<'t> {
        Enter(&'t El, usize),
        Leave(usize),
    }
    let mut scopes = Scopes::default();
    let mut declared: Vec<&'t str> = Vec::new();
    let mut stack = vec![Step::Enter(root, 0)];
    let mut visited = 0u64;
    while let Some(step) = stack.pop() {
        match step {
            Step::Leave(count) => scopes.leave(count, &mut declared),
            Step::Enter(el, depth) => {
                visited = visited.saturating_add(1);
                let pushed = scopes.enter(el, &mut declared);
                match visit(el, depth, &scopes) {
                    Flow::Stop => break,
                    Flow::Into => {
                        stack.push(Step::Leave(pushed));
                        let deeper = depth.saturating_add(1);
                        stack.extend(el.children.iter().rev().filter_map(|n| match n {
                            Node::El(child) => Some(Step::Enter(child, deeper)),
                            _ => None,
                        }));
                    }
                }
            }
        }
    }
    visited
}
