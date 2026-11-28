//! The attributes of one start tag, found and changed in place (an attribute of a changed element that no
//! changed cell feeds keeps its bytes; an attribute is rewritten only when its cells changed). A value is replaced between its quotes, an
//! attribute taken out is taken out with the white space before it, and one added is put after the last attribute of the tag, as the file's
//! own attributes are written: nothing else of the tag moves.
//!
//! **Names are found by namespace and not by prefix**, as the reader finds them (`read::names::Attrs`): the first attribute of the tag that is
//! `local` in the namespace `ns`, whatever its prefix, so a file that binds `svg` to the W3C URI has its `svg:x` found
//! and its value replaced under the name it has. **A name the writer adds is never written under a prefix the file binds to another URI**
//! The least prefix bound in scope to ODF's own namespace is used; where there is none, ODF's namespace is declared on the
//! tag under a prefix of its own, `svg1`, `svg2`, ..., and the file's own bindings are left as they are. **The declaration is made once for a tag**, however many edits of
//! it add a name (a connector's ends and then its path are two edits, each with a `TagEdit` of its own): the splicer keeps the declarations the edits made
//! ([`Splicer::declare`]). **Only attributes the ownership table owns are written** ([`owns`](crate::write::own::owns)).

use diacraft_container::xml_scan::{AttrSpan, TagParts, tag_attributes};

use super::scan::{DRAW_URI, SVG_URI, Scopes};
use super::splice::{Splicer, escape_value, escaped_len};
use super::{WriteError, WriteWork};
use crate::read::names::Ns;
use crate::write::own::owns;

/// A fault the unit tests inject into what the editor writes, to show that the check of the bytes made catches it: a value the
/// reader refuses, or an attribute written twice. Nothing of it is in a build that ships.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Fault {
        Off,
        /// Every value written is `garbled`, which no parser of the reader's reads.
        Unreadable,
        /// Every attribute written is written again after it, under the same name.
        Repeated,
        /// The declaration of a prefix of the writer's own, made on a tag for a namespace the place does not bind, is written twice.
        DeclaredTwice,
    }

    thread_local! {
        pub(crate) static INJECTED: Cell<Fault> = const { Cell::new(Fault::Off) };
    }

    /// The text to write in place of `text` under the fault now injected.
    pub(crate) fn text(text: &str) -> String {
        if INJECTED.get() == Fault::Unreadable { "garbled".to_owned() } else { text.to_owned() }
    }

    pub(crate) fn repeated() -> bool {
        INJECTED.get() == Fault::Repeated
    }

    pub(crate) fn declared_twice() -> bool {
        INJECTED.get() == Fault::DeclaredTwice
    }
}

/// The attributes of a start tag.
pub(crate) struct TagEdit<'a> {
    bytes: &'a [u8],
    /// Where the start tag begins in the part.
    open_at: usize,
    scope: u32,
    spans: Vec<AttrSpan>,
    parts: TagParts,
    /// The prefixes this tag has been given for namespaces it did not bind: the URI and the prefix.
    declared: Vec<(&'static str, String)>,
    /// Whether the attributes written are noted for the check of the bytes made as written on a shape element (the default): the tag of a style
    /// or a page layout is not a shape element, and is checked by what it names ([`TagEdit::untracked`]).
    track: bool,
    /// The namespace each attribute that [`TagEdit::find`] found was found in: what the ownership of an attribute is judged by.
    found: std::cell::RefCell<Vec<(usize, Ns)>>,
}

/// The namespace of an attribute the writer owns, from its local name: the SVG namespace's when the table owns `svg:` of it, else the drawing's.
fn owned_ns(local: &str) -> Ns {
    if owns(&format!("svg:{local}")).is_some() {
        Ns::Svg
    } else if owns(&format!("fo:{local}")).is_some() {
        Ns::Fo
    } else if owns(&format!("draw:{local}")).is_some() {
        Ns::Draw
    } else if owns(&format!("text:{local}")).is_some() {
        Ns::Text
    } else {
        Ns::Style
    }
}

/// The conventional prefix and the URI of a namespace the writer writes attributes of.
fn conventional(ns: Ns) -> Option<(&'static str, &'static str)> {
    match ns {
        Ns::Draw => Some(("draw", DRAW_URI)),
        Ns::Svg => Some(("svg", SVG_URI)),
        Ns::Fo => Some(("fo", super::scan::FO_URI)),
        Ns::Style => Some(("style", super::scan::STYLE_URI)),
        Ns::Text => Some(("text", super::scan::TEXT_URI)),
        Ns::Diacraft => Some((crate::ext::names::PREFIX, super::scan::DIACRAFT_URI)),
        _ => None,
    }
}

impl<'a> TagEdit<'a> {
    /// The attributes of the start tag at `open` of `bytes`, in the scope `scope`. Charged to the work as bytes of attributes looked at.
    pub(crate) fn new(bytes: &'a [u8], open: (usize, usize), scope: u32, work: &mut WriteWork) -> Result<TagEdit<'a>, WriteError> {
        work.charge_attribute_bytes(open.1 - open.0)?;
        let mut spans = Vec::new();
        let parts =
            tag_attributes(bytes, open.0, open.1, &mut spans).ok_or(WriteError::Mismatch("a start tag the writer cannot read the attributes of"))?;
        Ok(TagEdit { bytes, open_at: open.0, scope, spans, parts, declared: Vec::new(), track: true, found: std::cell::RefCell::new(Vec::new()) })
    }

    /// [`TagEdit::new`] for a tag that is not a shape element: what is written on it is not noted as written on a shape.
    pub(crate) fn untracked(bytes: &'a [u8], open: (usize, usize), scope: u32, work: &mut WriteWork) -> Result<TagEdit<'a>, WriteError> {
        let mut tag = TagEdit::new(bytes, open, scope, work)?;
        tag.track = false;
        Ok(tag)
    }

    fn name(&self, a: &AttrSpan) -> &str {
        std::str::from_utf8(self.bytes.get(a.name.0..a.name.1).unwrap_or_default()).unwrap_or_default()
    }

    /// The index of the first attribute that is `local` in `ns`.
    pub(crate) fn find(&self, scopes: &mut Scopes, ns: Ns, local: &str, work: &mut WriteWork) -> Option<usize> {
        let at = self.spans.iter().position(|a| {
            let name = self.name(a);
            if name == "xmlns" || name.starts_with("xmlns:") {
                return false;
            }
            match name.split_once(':') {
                Some((prefix, l)) => l == local && scopes.ns_of_prefix(self.scope, prefix, work) == ns,
                None => ns == Ns::None && name == local,
            }
        })?;
        self.found.borrow_mut().push((at, ns));
        Some(at)
    }

    /// The attributes of the tag that are in `ns`, as their places and local names, in the order written. (Each is noted as found in `ns`, so that the editor's
    /// ownership check judges it by that namespace.)
    pub(crate) fn in_namespace(&self, scopes: &mut Scopes, ns: Ns, work: &mut WriteWork) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        for (i, a) in self.spans.iter().enumerate() {
            let name = self.name(a);
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            if let Some((prefix, local)) = name.split_once(':')
                && scopes.ns_of_prefix(self.scope, prefix, work) == ns
            {
                self.found.borrow_mut().push((i, ns));
                out.push((i, local.to_owned()));
            }
        }
        out
    }

    /// The text of the attribute at `at`, as the file writes it between its quotes, references resolved.
    pub(crate) fn text(&self, at: usize) -> String {
        self.spans.get(at).map_or_else(String::new, |a| crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default()))
    }

    /// The value of the attribute at `at` **as the reader reads it** (XML 1.0 section 3.3.3, `diacraft_container::xml`): each literal tab, line feed and carriage return
    /// (a `CR LF` is one) is a space, and the references are then resolved, so a tab written as `&#9;` stays one. [`TagEdit::text`] keeps a literal tab a tab, which is
    /// the text of the bytes and not the name the reader gives the shape (the milestone-1: an id written with a tab was `&#9;`, which the reader keeps, and
    /// the shape's own `id<TAB>9` was `id 9` to it). The work is the value's length, which the tag's scan counted.
    pub(crate) fn read_text(&self, at: usize) -> String {
        let raw = self.spans.get(at).and_then(|a| self.bytes.get(a.value.0..a.value.1)).unwrap_or_default();
        let mut spaced: Vec<u8> = Vec::with_capacity(raw.len());
        let mut bytes = raw.iter().copied().peekable();
        while let Some(b) = bytes.next() {
            match b {
                b'\r' if bytes.peek() == Some(&b'\n') => {
                    bytes.next();
                    spaced.push(b' ');
                }
                b'\t' | b'\n' | b'\r' => spaced.push(b' '),
                _ => spaced.push(b),
            }
        }
        crate::package::unescape(&spaced)
    }

    /// Replaces the value of the attribute at `at`, in its own quotes. The attribute is the file's own, and keeps the name and the binding it has:
    /// where the file binds `svg` to the W3C namespace, a `svg:width` is replaced under that binding (this edits what the file already
    /// says, and governs only the attributes the writer adds, [`TagEdit::add`]).
    pub(crate) fn set(&self, splicer: &mut Splicer<'_>, at: usize, text: &str) -> Result<(), WriteError> {
        let a = self.spans.get(at).ok_or(WriteError::Mismatch("an attribute that is not on the tag"))?;
        self.check_owned(at, a)?;
        #[cfg(test)]
        let text = fault::text(text);
        #[cfg(test)]
        let text = text.as_str();
        splicer.replace(a.value.0, a.value.1, escape_value(text, a.quote));
        #[cfg(test)]
        if fault::repeated() {
            splicer.insert(self.parts.attrs_end, format!(" {}=\"1cm\"", self.name(a)));
        }
        let local = self.name(a).rsplit(':').next().unwrap_or_default();
        if self.track {
            splicer.note_written(self.open_at, owned_ns(local), local);
        }
        Ok(())
    }

    /// The bytes [`TagEdit::set`] writes for `text` as the value of the attribute at `at`, in the quotes the attribute has: its escaped length, which the splicer keeps
    /// until the part is written. 0 for an attribute that is not on the tag (`set` refuses it).
    pub(crate) fn written_len(&self, at: usize, text: &str) -> u64 {
        self.spans.get(at).map_or(0, |a| escaped_len(text, a.quote))
    }

    /// Takes the attribute at `at` out, with the white space before it.
    pub(crate) fn remove(&self, splicer: &mut Splicer<'_>, at: usize) -> Result<(), WriteError> {
        let a = self.spans.get(at).ok_or(WriteError::Mismatch("an attribute that is not on the tag"))?;
        self.check_owned(at, a)?;
        splicer.remove(a.lead, a.end);
        Ok(())
    }

    /// Refuses an attribute the table does not own: the name written as the table spells it, by ODF's conventional prefix.
    fn check_owned(&self, at: usize, a: &AttrSpan) -> Result<(), WriteError> {
        let name = self.name(a);
        let local = name.rsplit(':').next().unwrap_or_default();
        // By the namespace the attribute was found in, and, for one that was not looked for, by the conventional prefix it is written under.
        let found = self.found.borrow().iter().find(|(i, _)| *i == at).map(|(_, ns)| *ns);
        let prefix = match found {
            Some(ns) => conventional(ns).map(|(p, _)| p),
            None => name.split_once(':').map(|(p, _)| p).filter(|p| ["svg", "draw", "fo", "style", "text"].contains(p)),
        };
        let owned = match found {
            // DiaCraft's own attributes: the writer owns the names `ext::names::owned` lists, and keeps every other (the last row of "Styles").
            Some(Ns::Diacraft) => crate::ext::names::owned(local),
            _ => prefix.is_some_and(|p| owns(&format!("{p}:{local}")).is_some()),
        };
        if owned { Ok(()) } else { Err(WriteError::Mismatch("an attribute the ownership table does not own")) }
    }

    /// Adds `xml:id` valued `text` after the tag's last attribute (the prefix `xml` is bound without a declaration), unless the tag has one.
    pub(crate) fn add_xml_id(&mut self, splicer: &mut Splicer<'_>, text: &str) -> Result<(), WriteError> {
        let (spans, _) = (&self.spans, ());
        let has = spans.iter().any(|a| self.bytes.get(a.name.0..a.name.1) == Some(b"xml:id".as_slice()));
        if has {
            return Err(WriteError::Mismatch("an attribute the tag already has is not added again"));
        }
        splicer.insert(self.parts.attrs_end, format!(" xml:id=\"{}\"", escape_value(text, b'"')));
        Ok(())
    }

    /// Adds the attribute `local` of the namespace `ns` (the drawing's or the SVG one), valued `text`, after the tag's last attribute, under the
    /// least prefix the scope binds to ODF's own namespace, or under a prefix declared on the tag for it when there is none.
    pub(crate) fn add(
        &mut self,
        splicer: &mut Splicer<'_>,
        scopes: &mut Scopes,
        work: &mut WriteWork,
        ns: Ns,
        local: &str,
        text: &str,
    ) -> Result<(), WriteError> {
        let (conventional_prefix, uri) = conventional(ns).ok_or(WriteError::Mismatch("a namespace the writer does not write attributes of"))?;
        let table_owns = if ns == Ns::Diacraft { crate::ext::names::owned(local) } else { owns(&format!("{conventional_prefix}:{local}")).is_some() };
        if !table_owns {
            return Err(WriteError::Mismatch("an attribute the ownership table does not own"));
        }
        // An attribute the tag already has, whatever its value, is replaced and never added again: a tag that names one twice is not XML.
        if self.find(scopes, ns, local, work).is_some() {
            return Err(WriteError::Mismatch("an attribute the tag already has is not added again"));
        }
        let prefix = match self.declared.iter().find(|(u, _)| *u == uri) {
            Some((_, p)) => p.clone(),
            None => match scopes.prefix_for(self.scope, uri, work) {
                Some(p) => p,
                None => {
                    let fresh = scopes.fresh_prefix(conventional_prefix, work);
                    // **Once for the tag**: the editor makes one `TagEdit` for each edit of a shape, and the splicer knows what the edits before this one declared on the tag.
                    if splicer.declare(self.open_at, self.parts.attrs_end, &fresh, uri) {
                        work.attributes_written = work.attributes_written.saturating_add(1);
                        #[cfg(test)]
                        if fault::declared_twice() {
                            splicer.insert(self.parts.attrs_end, format!(" xmlns:{fresh}=\"{uri}\""));
                        }
                    }
                    self.declared.push((uri, fresh.clone()));
                    fresh
                }
            },
        };
        #[cfg(test)]
        let text = fault::text(text);
        #[cfg(test)]
        let text = text.as_str();
        splicer.insert(self.parts.attrs_end, format!(" {prefix}:{local}=\"{}\"", escape_value(text, b'"')));
        #[cfg(test)]
        if fault::repeated() {
            splicer.insert(self.parts.attrs_end, format!(" {prefix}:{local}=\"1cm\""));
        }
        if self.track {
            splicer.note_written(self.open_at, ns, local);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    /// The scope a tag nested in elements declaring `svg`, `draw` and `other` sees.
    fn scoped() -> (Scopes, u32) {
        Scopes::with_scope(&[("svg", Ns::Svg), ("draw", Ns::Draw), ("other", Ns::Other)], Some("draw"), Some("svg"))
    }

    fn edit<'a>(tag: &'a [u8], scope: u32, work: &mut WriteWork) -> TagEdit<'a> {
        match TagEdit::new(tag, (0, tag.len()), scope, work) {
            Ok(edit) => edit,
            Err(e) => panic!("the tag did not read: {e}"),
        }
    }

    /// The attributes of a namespace are listed in the order written, and a declaration and an attribute with no prefix are none of them and cost no step: a
    /// step is the look of one prefix.
    #[test]
    fn the_attributes_of_a_namespace_are_listed_and_a_declaration_costs_no_step() {
        let (mut scopes, scope) = scoped();
        let mut work = WriteWork::default();
        let tag = br#"<d:e xmlns:foo="urn:f" xmlns="urn:g" svg:x="1" x="2" draw:y="3" other:z="4" svg:w="5"/>"#;
        let tag = edit(tag, scope, &mut work);
        let before = work.scope_steps;
        assert_eq!(tag.in_namespace(&mut scopes, Ns::Svg, &mut work), vec![(2, "x".to_owned()), (6, "w".to_owned())]);
        assert_eq!(
            work.scope_steps - before,
            4,
            "a step for each of the four attributes with a prefix, none for the two declarations and the one with none"
        );
    }

    #[test]
    fn an_attribute_is_found_by_its_namespace_and_never_by_its_prefix_or_by_a_declaration() {
        let (mut scopes, scope) = scoped();
        let mut work = WriteWork::default();
        let tag = br#"<d:e xmlns:foo="urn:f" foo="9" other:x="3" svg:x="1" draw:x="2" x="4" svg:y="5"/>"#;
        let tag = edit(tag, scope, &mut work);
        let mut find = |ns, local: &str| tag.find(&mut scopes, ns, local, &mut work);
        assert_eq!(find(Ns::Svg, "x"), Some(3));
        assert_eq!(find(Ns::Draw, "x"), Some(4));
        assert_eq!(find(Ns::None, "x"), Some(5));
        assert_eq!(find(Ns::Svg, "y"), Some(6));
        assert_eq!(find(Ns::Svg, "z"), None);
        assert_eq!(find(Ns::Draw, "y"), None);
        // A declaration is no attribute: `xmlns:foo` names the prefix `foo` and is not the attribute `foo`.
        assert_eq!(find(Ns::None, "foo"), Some(1));
        assert_eq!(find(Ns::None, "xmlns"), None);
        assert_eq!(find(Ns::None, "xmlns:foo"), None);
        // An attribute with no prefix is in no namespace, and is not found as one of the SVG's even when it comes first.
        let (mut scopes, scope) = scoped();
        let tag = br#"<d:e x="4" svg:x="1"/>"#;
        let tag = edit(tag, scope, &mut work);
        assert_eq!(tag.find(&mut scopes, Ns::Svg, "x", &mut work), Some(1));
        assert_eq!(tag.find(&mut scopes, Ns::None, "x", &mut work), Some(0));
        assert_eq!(tag.find(&mut scopes, Ns::None, "svg", &mut work), None);
    }

    /// The value of an attribute as the reader reads it (XML 1.0 section 3.3.3): each literal tab, line feed and carriage return a space (a `CR LF` one), the references
    /// resolved after, so `&#9;` stays a tab; `text` keeps a literal tab a tab, which is not the name the reader gives.
    #[test]
    fn an_attribute_is_read_as_the_reader_reads_it_white_space_made_spaces_and_references_kept() {
        let mut work = WriteWork::default();
        let (_, scope) = scoped();
        let tag = b"<d:e a=\"x\ty\" b=\"x\nz\" c=\"x\r\nz\" d=\"x\rz\" e=\"x&#9;y&#10;z&#13;w\" f=\"a&amp;b&lt;&#x41;\" g=\"\" h=\"\t\t\"/>";
        let tag = edit(tag, scope, &mut work);
        let read: Vec<String> = (0..8).map(|at| tag.read_text(at)).collect();
        assert_eq!(read, ["x y", "x z", "x z", "x z", "x\ty\nz\rw", "a&b<A", "", "  "]);
        // `text` is the text of the bytes: a literal tab stays one.
        assert_eq!(tag.text(0), "x\ty");
        assert_eq!(tag.read_text(99), "", "an attribute the tag does not have");
    }

    #[test]
    fn an_attribute_the_tag_already_has_is_never_added_again() {
        let (mut scopes, scope) = scoped();
        let mut work = WriteWork::default();
        let bytes = br#"<d:e svg:x="oops" draw:transform="t"/>"#;
        let mut tag = edit(bytes, scope, &mut work);
        let mut splicer = Splicer::new(bytes);
        for (ns, local) in [(Ns::Svg, "x"), (Ns::Draw, "transform")] {
            assert!(matches!(tag.add(&mut splicer, &mut scopes, &mut work, ns, local, "1cm"), Err(WriteError::Mismatch(_))), "{local}");
        }
        assert!(tag.add(&mut splicer, &mut scopes, &mut work, Ns::Svg, "y", "1cm").is_ok(), "an attribute it lacks is added");
    }

    /// What the splice of `bytes` makes after the edits `edits` (each a `TagEdit` of its own on the one start tag, as `Editor::tag` makes one for each edit of a shape): the tag, as text.
    fn spliced(bytes: &[u8], scope: u32, scopes: &mut Scopes, edits: &[&[(Ns, &str)]]) -> String {
        let mut work = WriteWork::default();
        let mut splicer = Splicer::new(bytes);
        for names in edits {
            let mut tag = edit(bytes, scope, &mut work);
            for (ns, local) in *names {
                tag.add(&mut splicer, scopes, &mut work, *ns, local, "1").unwrap_or_else(|e| panic!("{local}: {e}"));
            }
        }
        String::from_utf8(splicer.apply(&mut work, &Limits::default()).unwrap()).unwrap()
    }

    /// A name the writer adds in an ODF namespace that the place binds under no prefix is written under a prefix of its own, declared on the tag; **the tag
    /// is edited by one `TagEdit` for each edit made to it** (a connector's ends, then its path), and each declared the prefix again, so the tag held `xmlns:svg1` twice (not XML).
    /// The declaration is made once for a tag, whichever of the six namespaces it is for and however many edits add names to the tag.
    #[test]
    fn a_prefix_declared_on_a_tag_for_a_namespace_the_place_does_not_bind_is_declared_once_however_many_edits_add_names() {
        let bytes = br#"<d:e a="1"/>"#;
        let cases: [(Ns, &str, &str, &str); 5] = [
            (Ns::Svg, "x", "y", "svg1"),
            (Ns::Draw, "transform", "z-index", "draw1"),
            (Ns::Fo, "color", "font-size", "fo1"),
            (Ns::Style, "display-name", "font-name", "style1"),
            (Ns::Diacraft, "id", "name", crate::ext::names::PREFIX),
        ];
        for (ns, first, second, prefix) in cases {
            let (mut scopes, scope) = Scopes::with_scope(&[], None, None);
            let after = spliced(bytes, scope, &mut scopes, &[&[(ns, first)], &[(ns, second)]]);
            assert_eq!(after.matches(&format!("xmlns:{prefix}=")).count(), 1, "{ns:?}: {after}");
            for local in [first, second] {
                assert_eq!(after.matches(&format!(" {prefix}:{local}=\"1\"")).count(), 1, "{ns:?} {local}: {after}");
            }
            let tag = after.replace("/>", "></d:e>");
            assert!(diacraft_container::xml::parse(tag.as_bytes()).is_ok(), "{ns:?}: {after}");
        }
        // Three edits, the names of two namespaces among them: one declaration for each prefix, and each name once.
        let (mut scopes, scope) = Scopes::with_scope(&[], None, None);
        let edits: [&[(Ns, &str)]; 3] =
            [&[(Ns::Svg, "x1"), (Ns::Draw, "transform")], &[(Ns::Svg, "d"), (Ns::Svg, "viewBox")], &[(Ns::Draw, "z-index"), (Ns::Svg, "y1")]];
        let after = spliced(bytes, scope, &mut scopes, &edits);
        for prefix in ["svg1", "draw1"] {
            assert_eq!(after.matches(&format!("xmlns:{prefix}=")).count(), 1, "{after}");
        }
        for name in ["svg1:x1", "svg1:y1", "svg1:d", "svg1:viewBox", "draw1:transform", "draw1:z-index"] {
            assert_eq!(after.matches(&format!(" {name}=")).count(), 1, "{name}: {after}");
        }
        let tag = after.replace("/>", "></d:e>");
        assert!(diacraft_container::xml::parse(tag.as_bytes()).is_ok(), "{after}");
    }

    #[test]
    fn only_an_attribute_the_table_owns_is_replaced_or_taken_out() {
        let (_, scope) = scoped();
        let mut work = WriteWork::default();
        let bytes = br#"<d:e draw:name="n" svg:x="1" draw:transform="t" svg:width="2" xlink:href="h" draw:style-name="s"/>"#;
        let tag = edit(bytes, scope, &mut work);
        // `draw:name` is a page's (a page renamed); `xlink:href` is the file's own.
        for (at, owned) in [(0, true), (1, true), (2, true), (3, true), (4, false), (5, true)] {
            let mut splicer = Splicer::new(bytes);
            assert_eq!(tag.set(&mut splicer, at, "9").is_ok(), owned, "set {at}");
            assert_eq!(tag.remove(&mut splicer, at).is_ok(), owned, "remove {at}");
        }
        let mut splicer = Splicer::new(bytes);
        assert!(matches!(tag.set(&mut splicer, 99, "9"), Err(WriteError::Mismatch(_))), "an attribute the tag does not have");
        assert!(matches!(tag.remove(&mut splicer, 99), Err(WriteError::Mismatch(_))));
    }
}
