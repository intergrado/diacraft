//! The markup a drawing kept of the file it was read from, checked before the model writer puts it in a part (the security scan's findings): its names
//! are qualified names of XML Namespaces 1.0, its declarations are ones that standard allows, and the prefixes it uses that it does not declare itself are found, so that a writer puts it
//! only where each of them stands for the namespace the markup was read in.
//!
//! **What the model knows of the namespace of a kept name.** The reader keeps a name as written, prefix and all. It keeps the declarations of the roots of the parts and of their containers
//! (`OdfDoc::roots`) and those a kept element makes itself, and **not** a declaration a page, a group or a shape makes on its own start tag. So where the model knows the namespace of a kept
//! name, it is the one the declarations it holds bind its prefix to above the place it was read: the model writer writes those declarations on the root and the containers of the part it
//! makes, so the binding in force on the element that holds the pages there (`office:drawing`, `office:master-styles`) is the one a shape's kept name was read with, and the binding in force
//! on a container is the one an element kept in it was. **A prefix those do not bind is one whose namespace the model does not know**: the markup is left out and said, never written where
//! its prefix is unbound or bound to a namespace it may not have had (a declaration a sibling's kept element makes binds nothing where the shape is).
//!
//! Every walk here keeps its own stack (no input makes it recurse) and counts what it walks, for the caller to charge; the trees it walks were bounded by the caller first.

use std::collections::{BTreeSet, HashMap};

use diacraft_container::xml::{El, Node, is_name};
use diacraft_doc::{RawAttrs, RawElement, RawNode};

/// The XML namespace, which the prefix `xml` is bound to without a declaration (XML Namespaces 1.0, section 3).
pub(crate) const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";

/// The namespace of the declarations themselves, to which no prefix may be bound.
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// The bindings in force at a place, by prefix (`""` for the default namespace).
pub(crate) type Bindings = HashMap<String, String>;

/// Whether `s` is an NCName: an XML name with no colon.
pub(crate) fn is_ncname(s: &str) -> bool {
    !s.is_empty() && !s.contains(':') && is_name(s)
}

/// Whether `name` is a qualified name of XML Namespaces 1.0: an NCName, or two of them joined by one colon. An XML name with two colons, or one that starts or ends with a colon, is not
/// one: no namespace-aware reader can read it.
pub(crate) fn is_qname(name: &str) -> bool {
    match name.split_once(':') {
        None => is_ncname(name),
        Some((prefix, local)) => is_ncname(prefix) && is_ncname(local),
    }
}

/// The prefix the attribute `name` declares: `Some("")` for `xmlns`, the default namespace, the prefix for `xmlns:p`, `None` for an attribute that declares nothing.
pub(crate) fn declared_by(name: &str) -> Option<&str> {
    if name == "xmlns" { Some("") } else { name.strip_prefix("xmlns:") }
}

/// Whether XML Namespaces 1.0 allows a declaration that binds `prefix` (`""`, the default namespace) to `uri`: the prefix `xml` only to the XML namespace and no other prefix to it, no
/// prefix to the namespace of the declarations nor `xmlns` itself, and a prefix never to the empty name (which only the default namespace may be bound to, to undo it).
pub(crate) fn declaration_ok(prefix: &str, uri: &str) -> bool {
    if prefix.is_empty() {
        return uri != XML_URI && uri != XMLNS_URI;
    }
    is_ncname(prefix) && prefix != "xmlns" && !uri.is_empty() && uri != XMLNS_URI && ((prefix == "xml") == (uri == XML_URI))
}

/// The prefix of an element's name: `""` for one with none, which is in the default namespace in force.
fn element_prefix(name: &str) -> &str {
    name.split_once(':').map_or("", |(prefix, _)| prefix)
}

/// What [`free_of`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Free {
    /// The prefixes the names use where no element of the markup declares them (the element's own declarations count), in order: `""` stands for the default namespace of an element
    /// name with no prefix (an attribute with none is in no namespace, and needs nothing). `xml` is never one: it is bound everywhere.
    pub prefixes: BTreeSet<String>,
    /// The elements, attributes and other nodes walked: each a step of the save, for the caller to charge.
    pub steps: usize,
}

/// The prefixes an element of the name `name`, the attributes `attrs` and the children `children` uses that it does not declare where it uses them (see [`Free`]), or `None` when a name is
/// not a qualified name or a declaration is not one XML Namespaces allows ([`declaration_ok`]): such markup cannot be written so that a namespace-aware reader reads it.
pub(crate) fn free_of<'a>(name: &'a str, attrs: &'a RawAttrs, children: impl Iterator<Item = &'a RawNode>) -> Option<Free> {
    let mut free = Free::default();
    let mut scope: HashMap<&'a str, usize> = HashMap::new();
    // The root's declarations stay in force for all of its children.
    enter(name, attrs, &mut scope, &mut free)?;
    for child in children {
        match child {
            RawNode::Element(el) => subtree(el, &mut scope, &mut free)?,
            _ => free.steps = free.steps.saturating_add(1),
        }
    }
    Some(free)
}

/// [`free_of`] for a kept element.
pub(crate) fn free_prefixes(el: &RawElement) -> Option<Free> {
    free_of(&el.name, &el.attrs, el.children.iter())
}

/// Walks the element `el` and everything in it with a stack of its own, taking its declarations into `scope` while inside it and out again after.
fn subtree<'a>(el: &'a RawElement, scope: &mut HashMap<&'a str, usize>, free: &mut Free) -> Option<()> {
    let declared = enter(&el.name, &el.attrs, scope, free)?;
    // (element, the place of its next child, the prefixes it declared)
    let mut stack: Vec<(&'a RawElement, usize, Vec<&'a str>)> = vec![(el, 0, declared)];
    while let Some(last) = stack.last_mut() {
        let here: &'a RawElement = last.0;
        let at = last.1;
        last.1 = at.saturating_add(1);
        match here.children.get(at) {
            Some(RawNode::Element(child)) => {
                let declared = enter(&child.name, &child.attrs, scope, free)?;
                stack.push((child, 0, declared));
            }
            Some(_) => free.steps = free.steps.saturating_add(1),
            None => {
                if let Some((_, _, declared)) = stack.pop() {
                    for prefix in declared {
                        if let Some(count) = scope.get_mut(prefix) {
                            *count = count.saturating_sub(1);
                        }
                    }
                }
            }
        }
    }
    Some(())
}

/// Enters an element: checks its names and declarations, puts its declarations in `scope`, and notes the prefixes it uses that nothing in scope binds. Gives the prefixes it declared.
fn enter<'a>(name: &'a str, attrs: &'a RawAttrs, scope: &mut HashMap<&'a str, usize>, free: &mut Free) -> Option<Vec<&'a str>> {
    free.steps = free.steps.saturating_add(1).saturating_add(attrs.len());
    let mut declared = Vec::new();
    for (key, value) in attrs.iter() {
        if !is_qname(key) {
            return None;
        }
        if let Some(prefix) = declared_by(key) {
            if !declaration_ok(prefix, value) {
                return None;
            }
            let count = scope.entry(prefix).or_insert(0);
            *count = count.saturating_add(1);
            declared.push(prefix);
        }
    }
    if !is_qname(name) {
        return None;
    }
    let element = element_prefix(name);
    // An element is never in the namespace of the declarations.
    if element == "xmlns" {
        return None;
    }
    let bound = |scope: &HashMap<&str, usize>, prefix: &str| prefix == "xml" || scope.get(prefix).is_some_and(|n| *n > 0);
    if !bound(scope, element) {
        free.prefixes.insert(element.to_owned());
    }
    for (key, _) in attrs.iter() {
        if declared_by(key).is_some() {
            continue;
        }
        if let Some((prefix, _)) = key.split_once(':')
            && !bound(scope, prefix)
        {
            free.prefixes.insert(prefix.to_owned());
        }
    }
    Some(declared)
}

/// The URI the element `el` itself binds `prefix` to (`""`: its default namespace), when it declares it.
pub(crate) fn own_binding<'a>(attrs: &'a RawAttrs, prefix: &str) -> Option<&'a str> {
    attrs.iter().find(|(key, _)| declared_by(key) == Some(prefix)).map(|(_, uri)| uri)
}

/// Puts the declarations among `attrs` into `bindings`, over what it holds.
pub(crate) fn declare_into<'a>(bindings: &mut Bindings, attrs: impl Iterator<Item = (&'a str, &'a str)>) {
    for (key, value) in attrs {
        if let Some(prefix) = declared_by(key) {
            bindings.insert(prefix.to_owned(), value.to_owned());
        }
    }
}

/// The local name of an element name as written.
fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, l)| l)
}

/// The bindings in force inside the element of `root` at `path` (the local names of the elements from the root down, each the first child of that name of the one before; the
/// root's own children are found by the first): each element's declarations over its parent's. A path that leaves the tree gives what was in force where it left.
pub(crate) fn in_force(root: &El, path: &[&str]) -> Bindings {
    let mut bindings = Bindings::new();
    let mut here = Some(root);
    let mut rest = path.iter();
    while let Some(el) = here {
        declare_into(&mut bindings, el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        here = rest.next().and_then(|name| {
            el.children.iter().find_map(|n| match n {
                Node::El(child) if local(&child.name) == *name => Some(child),
                _ => None,
            })
        });
    }
    bindings
}

/// Whether `bindings` bind `prefix`: `xml` always, `""` always (an element name with no prefix is in the default namespace in force, or in none, as it was where it was read), and any
/// other prefix when it is bound to a namespace (a binding to the empty name undoes one).
pub(crate) fn binds(bindings: &Bindings, prefix: &str) -> bool {
    prefix.is_empty() || prefix == "xml" || bindings.get(prefix).is_some_and(|uri| !uri.is_empty())
}

/// Whether the kept node `node` can be put where `bindings` are in force: every prefix it uses and does not declare itself is bound there ([`binds`]). With the steps the walk took;
/// `None` when a name or a declaration in it is not one XML Namespaces allows. A node that is no element is always placeable.
pub(crate) fn placeable(node: &RawNode, bindings: &Bindings) -> Option<(bool, usize)> {
    let RawNode::Element(el) = node else { return Some((true, 1)) };
    let free = free_prefixes(el)?;
    Some((free.prefixes.iter().all(|p| binds(bindings, p)), free.steps))
}

/// The text of the kept element `el` as it stands in a part, with `declare` (prefix and URI, `""` the default namespace) declared on its start tag before its own attributes: `None`
/// when it holds what XML cannot (a name that is no XML name, a character XML 1.0 does not allow, an attribute twice).
pub(crate) fn element_text(el: &RawElement, declare: &[(String, String)]) -> Option<String> {
    let mut tree = diacraft_sheetxml::passthrough::to_tree(el);
    if !declare.is_empty() {
        let mut attrs: Vec<(String, String)> =
            declare.iter().map(|(p, uri)| (if p.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{p}") }, uri.clone())).collect();
        attrs.append(&mut tree.attrs);
        tree.attrs = attrs;
    }
    let mut out: Vec<u8> = Vec::new();
    diacraft_container::xml::write_element(&mut out, &tree).ok()?;
    String::from_utf8(out).ok()
}

/// What a prefix of a new shape's kept markup needs where the shape is written ([`need_of`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    /// The place binds it to the namespace it was read in, this one: it is written as it is.
    Bound(String),
    /// The place binds it otherwise: it is written with a declaration of its own, binding it to the namespace it was read in, this one.
    Declare(String),
    /// Its namespace is not known, or it cannot be declared where it is used: the markup is left out and said.
    Unknown,
}

/// What a prefix needs, from the namespace it was read in (`read`: `None` when the model does not know it), the one the place binds it to (`here`: `None` for no binding), and whether
/// it is one of the writer's own prefixes **on the start tag the writer writes** (`writer_on_tag`: a declaration there would bind the writer's own names too). **A prefix bound at the
/// place to another namespace than the one it was read in is never taken as the same**: it is declared again, or, where it cannot be, the markup is left out.
pub(crate) fn need_of(read: Option<&str>, here: Option<&str>, writer_on_tag: bool) -> Need {
    let Some(read) = read else { return Need::Unknown };
    if here == Some(read) {
        Need::Bound(read.to_owned())
    } else if writer_on_tag {
        Need::Unknown
    } else {
        Need::Declare(read.to_owned())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
        RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
    }

    fn free(e: &RawElement) -> Vec<String> {
        free_prefixes(e).unwrap().prefixes.into_iter().collect()
    }

    /// A qualified name is an NCName or two joined by one colon: two colons, an empty side or a name that is no XML name is not one.
    #[test]
    fn a_qualified_name_has_at_most_one_colon_between_two_ncnames() {
        for good in ["a", "draw:rect", "xml:id", "xmlns", "xmlns:my", "a-b.c_d", "é:ü"] {
            assert!(is_qname(good), "{good}");
        }
        for bad in ["", ":a", "a:", "a:b:c", "1a", "a b", "my:a=\"1\" my:b", "a>b", "my:1x", "::"] {
            assert!(!is_qname(bad), "{bad}");
        }
    }

    /// A declaration binds the default namespace to anything but the two reserved ones, `xml` only to its own, no other prefix to it, and no prefix to the empty name or as `xmlns`.
    #[test]
    fn a_declaration_is_one_xml_namespaces_allows() {
        assert!(declaration_ok("", "") && declaration_ok("", "urn:a") && declaration_ok("my", "urn:a") && declaration_ok("xml", XML_URI));
        assert!(!declaration_ok("", XML_URI) && !declaration_ok("", XMLNS_URI));
        assert!(!declaration_ok("my", "") && !declaration_ok("my", XML_URI) && !declaration_ok("my", XMLNS_URI));
        assert!(!declaration_ok("xml", "urn:a") && !declaration_ok("xmlns", "urn:a") && !declaration_ok("a b", "urn:a"));
        assert_eq!((declared_by("xmlns"), declared_by("xmlns:my"), declared_by("my:x")), (Some(""), Some("my"), None));
    }

    /// The prefixes an element uses and does not declare where it uses them are the free ones: its own declarations bind its names and its descendants', a descendant's binds only
    /// its own subtree, `xml` is always bound, and an element name with no prefix needs the default namespace (an attribute with none needs nothing).
    #[test]
    fn the_free_prefixes_are_the_ones_used_where_nothing_inside_declares_them() {
        let own = el("my:a", &[("xmlns:my", "urn:m"), ("my:k", "v"), ("xml:lang", "en"), ("plain", "1")], vec![]);
        assert_eq!(free(&own), Vec::<String>::new());
        // A descendant's declaration binds its subtree only: the sibling after it, and the attribute of the root, are free.
        let inner = el("my:b", &[("xmlns:my", "urn:m")], vec![RawNode::Element(el("my:c", &[], vec![]))]);
        let tree =
            el("x:a", &[("y:k", "v")], vec![RawNode::Element(inner), RawNode::Element(el("my:d", &[], vec![])), RawNode::Text(Arc::from("t"))]);
        assert_eq!(free(&tree), ["my", "x", "y"]);
        // An element name with no prefix needs the default namespace, unless the element or one above it in the markup declares it.
        assert_eq!(free(&el("a", &[], vec![])), [""]);
        assert_eq!(free(&el("a", &[("xmlns", "urn:d")], vec![RawNode::Element(el("b", &[], vec![]))])), Vec::<String>::new());
        // Each element, attribute and other node is a step.
        let counted = free_prefixes(&tree).unwrap();
        assert_eq!(counted.steps, (1 + 1) + (1 + 1) + 1 + 1 + 1, "{counted:?}");
    }

    /// Markup that holds a name that is no qualified name, or a declaration XML Namespaces does not allow, anywhere in it, has no free prefixes: it cannot be written.
    #[test]
    fn markup_with_a_name_or_a_declaration_namespaces_do_not_allow_is_refused_at_any_depth() {
        let deep = |bad: RawElement| el("a", &[], vec![RawNode::Element(el("b", &[], vec![RawNode::Element(bad)]))]);
        assert!(free_prefixes(&deep(el("c:d:e", &[], vec![]))).is_none());
        assert!(free_prefixes(&deep(el("c", &[("k:l:m", "v")], vec![]))).is_none());
        assert!(free_prefixes(&deep(el("c", &[("xmlns:p", "")], vec![]))).is_none());
        assert!(free_prefixes(&deep(el("xmlns:c", &[], vec![]))).is_none());
        assert!(free_prefixes(&deep(el("c", &[("xmlns:", "urn:x")], vec![]))).is_none());
    }

    /// A tree deeper than the writer lets kept markup be is walked with a stack of its own (the model drops a tree by recursion, so the test stays at a depth that drop takes).
    #[test]
    fn a_deep_tree_is_walked_with_a_stack_of_its_own() {
        let mut node = el("my:leaf", &[], vec![]);
        for _ in 0..2_000 {
            node = el("a", &[], vec![RawNode::Element(node)]);
        }
        assert_eq!(free(&node), ["", "my"]);
    }

    /// The bindings in force on an element of a tree are its own declarations over those of the elements above it; a path that leaves the tree stops where it left.
    #[test]
    fn the_bindings_in_force_are_each_element_s_over_its_parent_s() {
        let drawing = El { name: "o:drawing".to_owned(), attrs: vec![("xmlns:my".to_owned(), "urn:two".to_owned())], children: Vec::new() };
        let body = El { name: "o:body".to_owned(), attrs: vec![("xmlns:b".to_owned(), "urn:b".to_owned())], children: vec![Node::El(drawing)] };
        let root = El {
            name: "o:root".to_owned(),
            attrs: vec![
                ("xmlns:o".to_owned(), "urn:o".to_owned()),
                ("xmlns:my".to_owned(), "urn:one".to_owned()),
                ("o:version".to_owned(), "1.4".to_owned()),
            ],
            children: vec![Node::El(body)],
        };
        let at_root = in_force(&root, &[]);
        assert_eq!(at_root.get("my").map(String::as_str), Some("urn:one"));
        assert!(!at_root.contains_key("b"));
        let at_drawing = in_force(&root, &["body", "drawing"]);
        assert_eq!(at_drawing.get("my").map(String::as_str), Some("urn:two"));
        assert_eq!(at_drawing.get("b").map(String::as_str), Some("urn:b"));
        assert_eq!(in_force(&root, &["body", "nothing"]).get("b").map(String::as_str), Some("urn:b"));
        assert!(binds(&at_drawing, "b") && binds(&at_drawing, "") && binds(&at_drawing, "xml") && !binds(&at_drawing, "q"));
        let undone: Bindings = [("q".to_owned(), String::new())].into_iter().collect();
        assert!(!binds(&undone, "q"), "a prefix bound to the empty name is bound to nothing");
    }

    /// A kept element can be put where every prefix it does not declare itself is bound; its text carries the declarations it is given before its own attributes.
    #[test]
    fn a_kept_element_is_placeable_where_its_free_prefixes_are_bound_and_written_with_what_it_is_given() {
        let node = RawNode::Element(el("my:a", &[("other:k", "v")], vec![]));
        let both: Bindings = [("my".to_owned(), "urn:m".to_owned()), ("other".to_owned(), "urn:o".to_owned())].into_iter().collect();
        let one: Bindings = [("my".to_owned(), "urn:m".to_owned())].into_iter().collect();
        assert_eq!(placeable(&node, &both).map(|p| p.0), Some(true));
        assert_eq!(placeable(&node, &one).map(|p| p.0), Some(false));
        assert_eq!(placeable(&RawNode::Text(Arc::from("t")), &Bindings::new()), Some((true, 1)));
        let RawNode::Element(e) = &node else { panic!() };
        let text = element_text(e, &[("other".to_owned(), "urn:o".to_owned()), (String::new(), "urn:d".to_owned())]).unwrap();
        assert_eq!(text, r#"<my:a xmlns:other="urn:o" xmlns="urn:d" other:k="v"/>"#);
        assert_eq!(own_binding(&RawAttrs::from_pairs([("xmlns:my", "urn:m")]), "my"), Some("urn:m"));
        assert_eq!(own_binding(&RawAttrs::from_pairs([("xmlns", "urn:d")]), ""), Some("urn:d"));
        assert!(element_text(&el("a", &[("k", "\u{1}")], vec![]), &[]).is_none(), "a character XML cannot hold");
    }

    /// **A prefix bound where the markup is written to another namespace than it was read in is never taken as the same** (the scan's finding 2): it is declared again on the markup,
    /// or, on the writer's own start tag, where a declaration would bind the writer's own names too, the markup is left out; a prefix whose namespace is not known is left out.
    #[test]
    fn a_prefix_bound_elsewhere_where_it_is_written_is_declared_again_or_left_out_and_never_taken_as_the_same() {
        assert_eq!(need_of(Some("urn:one"), Some("urn:one"), false), Need::Bound("urn:one".to_owned()));
        assert_eq!(need_of(Some("urn:one"), Some("urn:one"), true), Need::Bound("urn:one".to_owned()));
        assert_eq!(need_of(Some("urn:one"), Some("urn:two"), false), Need::Declare("urn:one".to_owned()));
        assert_eq!(need_of(Some("urn:one"), None, false), Need::Declare("urn:one".to_owned()));
        assert_eq!(need_of(Some("urn:one"), Some("urn:two"), true), Need::Unknown);
        assert_eq!(
            need_of(None, Some("urn:one"), false),
            Need::Unknown,
            "a binding at the place is not the one the markup was read in when that is not known"
        );
        assert_eq!(need_of(None, None, false), Need::Unknown);
    }
}
