//! Names by namespace, as the reader walks a tree: each element's and attribute's prefix resolved to the vocabulary it is bound to, as XML
//! Namespaces 1.0 does (a prefix is bound by the nearest `xmlns:p` on the element or above it, an unprefixed element by the nearest `xmlns`,
//! and an unprefixed attribute is in no namespace). ODF names are matched by namespace and never by prefix.
//!
//! The walk is the reader's own recursive descent, so the bindings are a stack the reader pushes on entering an element ([`Names::enter`])
//! and pops on leaving it ([`Names::leave`]): one map of each prefix to its stack of bindings, so that a lookup is one hash probe however
//! deep the element is and however many prefixes are declared above it.
//!
//! **The two pre-standard URIs.** OpenOffice.org-line files of the census (vector 7) bind
//! `fo` and `svg` to the W3C XSL-FO and SVG namespaces, where ODF has its own compatible ones. Project rule (reported as a spec
//! question): the reader reads those two URIs as ODF's `fo` and `svg`, so that such a file's frames and formatting are read, not ignored.
//!
//! **What it costs**: [`Names::attrs`] walks an element's attributes once, and a lookup in what it gives is a scan of a short list, or one
//! hash probe for an element of more than [`MAP_FROM`] attributes, so an element of a million attributes costs a million steps, not a
//! million for each property the reader asks for.

use std::collections::HashMap;

use diacraft_container::xml::El;

/// The vocabularies the reader reads, by namespace URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ns {
    Office,
    Style,
    Text,
    Draw,
    Fo,
    Svg,
    Xlink,
    Meta,
    /// Dublin Core, `http://purl.org/dc/elements/1.1/`: the elements of `meta.xml` that both formats say (`dc:title`, `dc:date`).
    Dc,
    Config,
    Table,
    Dr3d,
    /// The prefix `xml`, bound without a declaration (XML Namespaces 1.0, section 3).
    Xml,
    /// DiaCraft's own extension, `urn:app.diacraft:extension`, **and the namespace that builds made before the id moved to `diacraft.app` wrote**
    /// (`urn:io.github.one_bit.diacraft:extension`, APPID): both are DiaCraft's own, read alike. A writer writes the first only.
    Diacraft,
    /// LibreOffice's pre-standard office namespace, `http://openoffice.org/2009/office`: a drawing it saves holds a page's comments as `officeooo:annotation`.
    Officeooo,
    /// A namespace the reader does not read: a producer's extension, or anything else.
    Other,
    /// No namespace: an unprefixed attribute, or an element under no default namespace, or a prefix bound to nothing.
    None,
}

/// The URI of LibreOffice's pre-standard office namespace ([`Ns::Officeooo`]).
pub(crate) const OFFICEOOO_URI: &str = "http://openoffice.org/2009/office";

/// The vocabulary a namespace URI names, for the vocabularies the reader matches: ODF 1.4 Part 3's (§1.5, Tables 1 to 4), XLink's and
/// DiaCraft's. Any other URI, the empty one that unbinds a prefix included, is one the reader reads nothing in.
pub(crate) fn ns_of_uri(uri: &str) -> Ns {
    const ODF: &str = "urn:oasis:names:tc:opendocument:xmlns:";
    if let Some(rest) = uri.strip_prefix(ODF) {
        return match rest {
            "office:1.0" => Ns::Office,
            "style:1.0" => Ns::Style,
            "text:1.0" => Ns::Text,
            "drawing:1.0" => Ns::Draw,
            "xsl-fo-compatible:1.0" => Ns::Fo,
            "svg-compatible:1.0" => Ns::Svg,
            "meta:1.0" => Ns::Meta,
            "config:1.0" => Ns::Config,
            "table:1.0" => Ns::Table,
            "dr3d:1.0" => Ns::Dr3d,
            _ => Ns::Other,
        };
    }
    match uri {
        "http://www.w3.org/1999/xlink" => Ns::Xlink,
        "http://purl.org/dc/elements/1.1/" => Ns::Dc,
        OFFICEOOO_URI => Ns::Officeooo,
        // The pre-standard URIs (module documentation).
        "http://www.w3.org/1999/XSL/Format" => Ns::Fo,
        "http://www.w3.org/2000/svg" => Ns::Svg,
        _ if diacraft_doc::is_diacraft_urn(uri) => Ns::Diacraft,
        _ => Ns::Other,
    }
}

/// The namespace of LibreOffice's extension (`loext`), in which LibreOffice writes a gradient's stops (`loext:gradient-stop`) and a shadow's blur. The reader reads it as
/// [`Ns::Other`], and tells it by [`Names::uri_of`] where it reads one of its names.
pub(crate) const LOEXT_URI: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";

/// Elements past this many attributes are looked in through a map.
const MAP_FROM: usize = 16;

/// The bindings in scope, as a stack the walk pushes and pops.
#[derive(Default)]
pub(crate) struct Names<'t> {
    /// Each prefix's bindings, innermost last: the vocabulary and the URI as written (the URI tells a producer's extension the reader reads one element of, LibreOffice's
    /// `loext:gradient-stop`, from any other name of [`Ns::Other`]), and the place of the declaration among the ones made (its number in [`Names::declared`] when it was made).
    bound: HashMap<&'t str, Vec<(Ns, &'t str, usize)>>,
    declared: Vec<&'t str>,
    /// How many declarations were made when the model's own were all made (the root's, and the containers' down to the one that holds the pages: [`Names::hold`]): a binding made
    /// after that is **local**, one the model does not hold, and markup kept whole that uses it must carry the declaration itself ([`Names::local_binding`]). `None` before the pages.
    floor: Option<usize>,
}

impl<'t> Names<'t> {
    /// Declares the bindings `el` makes: what [`Names::leave`] is given to undo them.
    pub(crate) fn enter(&mut self, el: &'t El) -> usize {
        let mut pushed = 0;
        for (key, value) in &el.attrs {
            let prefix = if key == "xmlns" {
                ""
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                prefix
            } else {
                continue;
            };
            self.bound.entry(prefix).or_default().push((ns_of_uri(value), value.as_str(), self.declared.len()));
            self.declared.push(prefix);
            pushed += 1;
        }
        pushed
    }

    /// Undoes the last `count` declarations.
    pub(crate) fn leave(&mut self, count: usize) {
        for _ in 0..count {
            if let Some(prefix) = self.declared.pop()
                && let Some(stack) = self.bound.get_mut(prefix)
            {
                stack.pop();
            }
        }
    }

    /// The vocabulary `prefix` is bound to here.
    fn prefix(&self, prefix: &str) -> Ns {
        if prefix == "xml" {
            return Ns::Xml;
        }
        self.bound.get(prefix).and_then(|stack| stack.last()).map_or(Ns::None, |(ns, _, _)| *ns)
    }

    /// The URI the prefix of `name` (an element's or an attribute's name as written) is bound to here: `None` for a name with no prefix, and for a prefix bound to nothing.
    pub(crate) fn uri_of(&self, name: &str) -> Option<&'t str> {
        let (prefix, _) = name.split_once(':')?;
        self.uri_of_prefix(prefix)
    }

    /// The URI `prefix` is bound to here: `None` for a prefix bound to nothing.
    pub(crate) fn uri_of_prefix(&self, prefix: &str) -> Option<&'t str> {
        self.bound.get(prefix).and_then(|stack| stack.last()).map(|(_, uri, _)| *uri)
    }

    /// Says that the declarations made so far are the model's own (a package: the root of the part and the containers down to the one that holds the pages, whose attributes the model
    /// holds as the roots; a flat drawing: its one root): from here on a binding is local. Only the first call counts, so that a flat drawing, which holds the root alone, is not made
    /// to hold the containers below it too.
    /// Gives whether this call did it, which is the one that may [`Names::release`].
    pub(crate) fn hold(&mut self) -> bool {
        let first = self.floor.is_none();
        self.floor.get_or_insert(self.declared.len());
        first
    }

    /// Ends [`Names::hold`]: nothing is local any more.
    pub(crate) fn release(&mut self) {
        self.floor = None;
    }

    /// Whether any binding made after [`Names::hold`] is in force here: the quick answer that no markup read here needs a declaration of its own.
    pub(crate) fn has_locals(&self) -> bool {
        self.floor.is_some_and(|floor| self.declared.len() > floor)
    }

    /// The namespace `prefix` (`""` the default namespace) stands for by **a declaration the model does not hold**, when that is not the one the model's own declarations bind it to:
    /// markup kept whole that uses the prefix without declaring it must declare it itself, or it is written where the prefix means another namespace, or none. `None` for a
    /// prefix the model's declarations bind (as it was read), for one nothing binds, and for a local declaration that binds what the model's do.
    pub(crate) fn local_binding(&self, prefix: &str) -> Option<&'t str> {
        let floor = self.floor?;
        let stack = self.bound.get(prefix)?;
        let (_, uri, serial) = stack.last()?;
        if *serial < floor {
            return None;
        }
        let held = stack.iter().rev().find(|(_, _, serial)| *serial < floor).map(|(_, uri, _)| *uri);
        (held != Some(*uri)).then_some(*uri)
    }

    /// The vocabulary and local name of an element here.
    pub(crate) fn element<'n>(&self, el: &'n El) -> (Ns, &'n str) {
        match el.name.split_once(':') {
            Some((prefix, local)) => (self.prefix(prefix), local),
            None => (self.prefix(""), el.name.as_str()),
        }
    }

    /// Whether `el` is `local` in `ns` here.
    pub(crate) fn is(&self, el: &El, ns: Ns, local: &str) -> bool {
        self.element(el) == (ns, local)
    }

    /// The attributes of `el`, namespace declarations left out, resolved here.
    pub(crate) fn attrs(&self, el: &'t El) -> Attrs<'t> {
        let mut items = Vec::with_capacity(el.attrs.len());
        for (key, value) in &el.attrs {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            let (ns, local) = match key.split_once(':') {
                Some((prefix, local)) => (self.prefix(prefix), local),
                None => (Ns::None, key.as_str()),
            };
            items.push(Attr { ns, local, name: key.as_str(), value: value.as_str() });
        }
        let map = (items.len() > MAP_FROM).then(|| {
            let mut map = HashMap::with_capacity(items.len());
            for (at, a) in items.iter().enumerate() {
                map.entry((a.ns, a.local)).or_insert(at);
            }
            map
        });
        let used = vec![false; items.len()];
        Attrs { items, used, map }
    }
}

/// One attribute, resolved.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attr<'t> {
    pub ns: Ns,
    pub local: &'t str,
    /// The name as written, prefix and all.
    pub name: &'t str,
    pub value: &'t str,
}

/// The attributes of one element, resolved, with a mark on each the reader took.
pub(crate) struct Attrs<'t> {
    items: Vec<Attr<'t>>,
    used: Vec<bool>,
    map: Option<HashMap<(Ns, &'t str), usize>>,
}

impl<'t> Attrs<'t> {
    fn position(&self, ns: Ns, local: &str) -> Option<usize> {
        match &self.map {
            Some(map) => map.get(&(ns, local)).copied(),
            None => self.items.iter().position(|a| a.ns == ns && a.local == local),
        }
    }

    /// The value of the attribute `local` in `ns`, the first if the element has two, marked as taken.
    pub(crate) fn take(&mut self, ns: Ns, local: &str) -> Option<&'t str> {
        let at = self.position(ns, local)?;
        if let Some(used) = self.used.get_mut(at) {
            *used = true;
        }
        self.items.get(at).map(|a| a.value)
    }

    /// The value of the attribute `local` in `ns`, not marked.
    pub(crate) fn peek(&self, ns: Ns, local: &str) -> Option<&'t str> {
        self.position(ns, local).and_then(|at| self.items.get(at)).map(|a| a.value)
    }

    /// How many attributes, declarations left out.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// Every attribute, in the order written.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Attr<'t>> {
        self.items.iter()
    }

    /// The attributes the reader did not take, in the order written.
    pub(crate) fn untaken(&self) -> impl Iterator<Item = &Attr<'t>> {
        self.items.iter().zip(&self.used).filter(|(_, used)| !**used).map(|(a, _)| a)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn tree(text: &str) -> El {
        diacraft_container::xml::parse(text.as_bytes()).unwrap()
    }

    /// **A binding made after the model's own are held is local**: one the model holds, or one that binds what the model's do, is none, and a prefix nothing binds has none.
    #[test]
    fn a_binding_made_after_the_models_own_are_held_is_local_unless_it_says_what_they_do() {
        let root = tree(
            r#"<a xmlns:p="urn:one" xmlns:held="urn:held"><b xmlns:p="urn:two" xmlns:q="urn:three" xmlns="urn:default"><c xmlns:p="urn:one"/></b></a>"#,
        );
        let mut names = Names::default();
        let a = names.enter(&root);
        assert!(!names.has_locals() && names.local_binding("p").is_none(), "nothing is local before the model's declarations are held");
        assert!(names.hold(), "the first call holds");
        assert!(!names.hold(), "a second call is not the one that may release");
        assert_eq!((names.local_binding("p"), names.has_locals()), (None, false), "the root's own declaration is the model's");
        let b = root.elements().next().unwrap();
        let mark_b = names.enter(b);
        assert!(names.has_locals());
        assert_eq!(names.local_binding("p"), Some("urn:two"), "rebound below the held ones");
        assert_eq!(names.local_binding("q"), Some("urn:three"));
        assert_eq!(names.local_binding(""), Some("urn:default"), "the default namespace too");
        assert_eq!(names.local_binding("held"), None, "a prefix only the model's declarations bind");
        assert_eq!(names.local_binding("nowhere"), None, "a prefix nothing binds");
        let c = b.elements().next().unwrap();
        let mark_c = names.enter(c);
        assert_eq!(names.local_binding("p"), None, "bound again to what the model's declarations bind");
        assert_eq!(names.local_binding("q"), Some("urn:three"), "what the element above still binds");
        names.leave(mark_c);
        assert_eq!(names.local_binding("p"), Some("urn:two"));
        names.leave(mark_b);
        assert_eq!((names.local_binding("p"), names.local_binding("q"), names.has_locals()), (None, None, false), "left, a binding is none");
        names.release();
        names.leave(a);
    }

    #[test]
    fn nothing_is_local_after_release_or_in_a_walk_that_never_held() {
        let root = tree(r#"<a><b xmlns:p="urn:two"/></a>"#);
        let mut names = Names::default();
        let a = names.enter(&root);
        let b = root.elements().next().unwrap();
        let mark = names.enter(b);
        assert_eq!((names.local_binding("p"), names.has_locals()), (None, false), "a walk that never held has no local binding");
        names.leave(mark);
        assert!(names.hold());
        let mark = names.enter(b);
        assert_eq!(names.local_binding("p"), Some("urn:two"));
        names.release();
        assert_eq!((names.local_binding("p"), names.has_locals()), (None, false));
        names.leave(mark);
        names.leave(a);
    }
}
