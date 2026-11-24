//! Which of an element's names are DiaCraft's: the namespace test of the manifest and of `state.xml` (the public spec's 3.1).
//!
//! **A reader matches the namespace and never the prefix**: a part may use another prefix, or make the namespace the default. The container's tree resolves
//! nothing, so this reads the declarations of an element and of its parent, which is all the two small parts of a package need (a root and its children).
//! Every declaration scan walks each element's own attributes once, and at most [`MAX_DECLARATIONS`] prefixed declarations are kept for one element.

use diacraft_container::xml::El;

/// The most prefixed declarations one element may make: more is a part that no writer made.
pub(crate) const MAX_DECLARATIONS: usize = 64;

/// The declarations one element makes by itself: the default namespace and the prefixed ones.
#[derive(Debug, Default)]
pub(crate) struct Declarations<'a> {
    default: Option<&'a str>,
    prefixed: Vec<(&'a str, &'a str)>,
}

impl<'a> Declarations<'a> {
    /// The declarations in the attributes of `el`, or `None` when it makes more than [`MAX_DECLARATIONS`] prefixed ones.
    pub(crate) fn of(el: &'a El) -> Option<Declarations<'a>> {
        let mut out = Declarations::default();
        for (name, value) in &el.attrs {
            if name == "xmlns" {
                out.default = Some(value);
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                if out.prefixed.len() >= MAX_DECLARATIONS {
                    return None;
                }
                out.prefixed.push((prefix, value));
            }
        }
        Some(out)
    }

    fn lookup(&self, prefix: &str) -> Option<&'a str> {
        if prefix.is_empty() {
            return self.default;
        }
        self.prefixed.iter().rev().find(|(p, _)| *p == prefix).map(|(_, uri)| *uri)
    }
}

/// Whether the element `el`, whose own declarations are `own` and whose parent's are `parent`, is in DiaCraft's namespace: its prefix (or the default namespace
/// when it has none) is bound by the nearest declaration to the namespace every writer writes or to the one that builds made before the id moved to `diacraft.app` wrote
/// (`diacraft_doc::is_diacraft_urn`, APPID). The reserved prefix `xml` is bound to no namespace of ours.
pub(crate) fn is_ours(el: &El, own: &Declarations<'_>, parent: Option<&Declarations<'_>>) -> bool {
    let prefix = el.prefix();
    if prefix == "xml" || prefix == "xmlns" {
        return false;
    }
    own.lookup(prefix).or_else(|| parent.and_then(|p| p.lookup(prefix))).is_some_and(diacraft_doc::is_diacraft_urn)
}

/// The value of the attribute `name` of `el`, which is in no namespace: an attribute with a prefix is another attribute, and a declaration is none.
pub(crate) fn attr<'a>(el: &'a El, name: &str) -> Option<&'a str> {
    el.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Whether `name` is the name of an attribute that is in no namespace: it has no prefix and is no declaration.
pub(crate) fn is_plain_attr(name: &str) -> bool {
    !name.contains(':') && name != "xmlns"
}
