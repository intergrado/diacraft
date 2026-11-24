//! The fingerprint of a named unit (the format's 3.19): a `def` carries the stamp of the unit it names, **as 1.0 would say it**, so that a program that
//! edits the unit and keeps the `same` elements that stand for it is found out when the file is read again.
//!
//! A newer writer stamps what it stores with a fingerprint of the object, and an older program that edits the object keeps the stamp exactly as it was; the newer
//! reader computes the fingerprint again and **drops what no longer matches** (5.4). Here the stored data is the name a `same` takes, the object is the unit, and what is
//! dropped is the name: the unit still reads as itself, and the `same` elements that named it name nothing, and are counted.
//!
//! The fingerprint is of the unit's subtree as written, in order: the local name of each element, each attribute that is not a declaration, the text that is not
//! layout, and for a `same` inside the unit **the fingerprint of the unit it names** (a unit that holds one is what that one is). The `def` and the `fp` of the unit
//! itself are not in it. The writer and the reader walk the same tree: the one the writer made, and the one the reader parsed.

use diacraft_container::xml::{El, Node};
use diacraft_doc::stamp::{Digest, Stamp};

use super::vocab::{at, el};

/// The local name of an element or attribute name: what follows the prefix.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// Feeds an element's name and attributes. A declaration is not an attribute of the content (a program that writes the part again may declare namespaces where it
/// likes), and a unit's own name and fingerprint are not in its fingerprint.
fn open(digest: &mut Digest, e: &El, root: bool) {
    digest.text("<").text(local(&e.name));
    for (k, v) in &e.attrs {
        if k == "xmlns" || k.starts_with("xmlns:") || (root && (k == at::DEF || k == at::FP)) {
            continue;
        }
        digest.text(k).text(&super::stamp::as_version_1_0(v));
    }
}

/// The fingerprint of the unit `root`. `named` gives the fingerprint of a unit that an earlier element of the part named, by its name. Not recursive: the stack is
/// at most as deep as the tree, which the container holds to its ceiling.
pub(super) fn fingerprint(root: &El, named: &dyn Fn(&str) -> Option<Stamp>) -> Stamp {
    let mut digest = Digest::new("unit");
    open(&mut digest, root, true);
    let mut stack: Vec<(&El, usize)> = vec![(root, 0)];
    while let Some((e, at_child)) = stack.pop() {
        let Some(node) = e.children.get(at_child) else {
            digest.text("/");
            continue;
        };
        stack.push((e, at_child.saturating_add(1)));
        match node {
            Node::El(child) => {
                if local(&child.name) == el::SAME {
                    // What it stands for, and not what it says: a `same` that names no unit is `?`.
                    digest.text("same");
                    match child.attr(at::OF).and_then(named) {
                        Some(fp) => digest.text(&fp.text()),
                        None => digest.text("?"),
                    };
                    continue;
                }
                open(&mut digest, child, false);
                stack.push((child, 0));
            }
            Node::Text(t) | Node::CData(t) => {
                if !t.trim().is_empty() {
                    digest.text("t").text(t);
                }
            }
            // A comment and an instruction are not content a reader keeps outside kept data.
            Node::Comment(_) | Node::Pi { .. } => {}
        }
    }
    digest.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
        El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
    }

    fn none(_: &str) -> Option<Stamp> {
        None
    }

    #[test]
    fn the_fingerprint_of_a_unit_is_of_its_content_and_not_of_its_name_its_prefix_its_declarations_or_its_layout() {
        let cell = |v: &str| Node::El(el("d:Cell", &[("N", "A"), ("V", v)], vec![]));
        let unit = el("d:Section", &[("N", "U")], vec![cell("1")]);
        let base = fingerprint(&unit, &none);
        // The unit's own name and fingerprint, a prefix, a declaration, layout and a comment are no part of it.
        let named = el("d:Section", &[("N", "U"), ("def", "7"), ("fp", "0123456789abcdef"), ("xmlns:z", "urn:z")], vec![cell("1")]);
        assert_eq!(fingerprint(&named, &none), base);
        let other_prefix = el(
            "x:Section",
            &[("N", "U")],
            vec![Node::El(el("x:Cell", &[("N", "A"), ("V", "1")], vec![])), Node::Text("\n  ".to_owned()), Node::Comment("c".to_owned())],
        );
        assert_eq!(fingerprint(&other_prefix, &none), base);
        // What is in it: a value, an attribute, a child, an order, a text, a name of a nested element's own.
        for changed in [
            el("d:Section", &[("N", "U")], vec![cell("2")]),
            el("d:Section", &[("N", "V")], vec![cell("1")]),
            el("d:Section", &[("N", "U")], vec![cell("1"), cell("1")]),
            el("d:Section", &[("N", "U")], vec![]),
            el("d:Section", &[("N", "U")], vec![cell("1"), Node::Text("x".to_owned())]),
            el("d:Section", &[("N", "U"), ("IX", "1")], vec![cell("1")]),
            el("d:Row", &[("N", "U")], vec![cell("1")]),
        ] {
            assert_ne!(fingerprint(&changed, &none), base, "{changed:?}");
        }
        // The `def` of a unit inside the unit is content.
        let inner = |name: &str| el("d:sheet", &[], vec![Node::El(el("d:Section", &[("N", "U"), ("def", name)], vec![]))]);
        assert_ne!(fingerprint(&inner("1"), &none), fingerprint(&inner("2"), &none));
    }

    #[test]
    fn a_same_inside_a_unit_is_the_fingerprint_of_the_unit_it_names_and_a_name_that_is_not_known_is_another_thing() {
        let same = |of: &str| el("d:sheet", &[], vec![Node::El(el("d:same", &[("of", of)], vec![]))]);
        let (a, b) = (Digest::new("x").text("a").finish(), Digest::new("x").text("b").finish());
        let table = [("1", a), ("2", b), ("3", a)];
        let by = |n: &str| table.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
        let fp = |of: &str| fingerprint(&same(of), &by);
        assert_eq!(fp("1"), fp("3"), "two names for one unit's fingerprint are one thing");
        assert_ne!(fp("1"), fp("2"), "a unit that names another is what that one is");
        assert_ne!(fp("1"), fp("9"), "a name that names nothing is not a unit");
        assert_eq!(fp("8"), fp("9"));
    }
}
