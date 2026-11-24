//! The text of an element the writer makes new (a new style, a new shape): written under the prefixes the
//! place it goes in binds to ODF's own namespaces, and under a prefix of its own, declared on the new element, for one it does not bind (plan Ruling
//! A name is never written under a prefix the file binds to another URI, so a file that binds `svg` to the W3C's has its new `svg:` names
//! under `svg1`).

use super::WriteWork;
use super::scan::{DRAW_URI, FO_URI, STYLE_URI, SVG_URI, Scopes, TEXT_URI};
use super::splice::escape_value;
use crate::read::names::Ns;

/// The prefixes a new element is written under, and the declarations its root tag carries for those the place does not bind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pfx {
    pub draw: String,
    pub svg: String,
    pub fo: String,
    pub style: String,
    pub text: String,
    /// ` xmlns:svg1="..."` for each prefix of the five that is new, to put on the first tag of the new element.
    pub declare: String,
}

impl Pfx {
    /// The prefixes for an element put where the namespaces in `scope` are in force.
    pub(crate) fn of(scopes: &mut Scopes, scope: u32, work: &mut WriteWork) -> Pfx {
        let mut declare = String::new();
        let mut one = |scopes: &mut Scopes, uri: &str, base: &str| -> String {
            match scopes.prefix_for(scope, uri, work) {
                Some(p) => p,
                None => {
                    let fresh = scopes.fresh_prefix(base, work);
                    declare.push_str(&format!(" xmlns:{fresh}=\"{uri}\""));
                    fresh
                }
            }
        };
        let draw = one(scopes, DRAW_URI, "draw");
        let svg = one(scopes, SVG_URI, "svg");
        let fo = one(scopes, FO_URI, "fo");
        let style = one(scopes, STYLE_URI, "style");
        let text = one(scopes, TEXT_URI, "text");
        Pfx { draw, svg, fo, style, text, declare }
    }

    /// The prefix of the namespace `ns` of an attribute the writer writes.
    pub(crate) fn of_ns(&self, ns: Ns) -> &str {
        match ns {
            Ns::Svg => &self.svg,
            Ns::Fo => &self.fo,
            Ns::Style => &self.style,
            Ns::Text => &self.text,
            // LibreOffice's extension (`loext:shadow-blur`), declared on the element that holds it (`super::fx::LOEXT_URI`); no other name is written under it.
            Ns::Other => "loext",
            _ => &self.draw,
        }
    }

    /// ` prefix:local="value"`, the value escaped. **A `local` that is no NCName writes nothing**: it is never written into the markup, whatever the caller meant (an attribute name with a
    /// quote, a space or a `>` in it would put markup of its own into the part); a caller whose names come from a file uses [`Pfx::attr_checked`] and refuses in words.
    pub(crate) fn attr(&self, ns: Ns, local: &str, value: &str) -> String {
        if !super::kept::is_ncname(local) {
            return String::new();
        }
        format!(" {}:{local}=\"{}\"", self.of_ns(ns), escape_value(value, b'"'))
    }

    /// [`Pfx::attr`] for a name and a value that come from a file: refused in words when the name is no NCName or the value holds a character XML 1.0 does not allow.
    pub(crate) fn attr_checked(&self, ns: Ns, local: &str, value: &str) -> Result<String, super::WriteError> {
        if !super::kept::is_ncname(local) || super::splice::has_bad_char(value) {
            return Err(super::WriteError::Unsupported(super::splice::WORDS_XML_CHAR));
        }
        Ok(self.attr(ns, local, value))
    }

    /// The same declarations, kept for the first tag only: later tags of one new element write none.
    pub(crate) fn without_declarations(&self) -> Pfx {
        Pfx { declare: String::new(), ..self.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local name that is no NCName writes nothing, and one that comes from a file is refused in words with a value XML does not allow.
    #[test]
    fn a_local_name_that_is_no_name_is_never_written() {
        let mut work = WriteWork::default();
        let (mut scopes, scope) = Scopes::with_scope(&[], Some("draw"), None);
        let p = Pfx::of(&mut scopes, scope, &mut work);
        for bad in ["a=\"1\" b", "x>", "x y", "", "a:b", "1x", "x\"", "<x"] {
            assert_eq!(p.attr(Ns::Svg, bad, "v"), "", "{bad:?}");
            assert!(p.attr_checked(Ns::Svg, bad, "v").is_err(), "{bad:?}");
        }
        assert!(p.attr_checked(Ns::Svg, "x", "a\u{1}b").is_err(), "a value with a control character");
        assert_eq!(p.attr_checked(Ns::Svg, "x", "1cm").unwrap(), format!(" {}:x=\"1cm\"", p.svg));
    }

    #[test]
    fn a_name_is_written_under_the_prefix_the_place_binds_to_odf_s_namespace_and_one_of_its_own_otherwise() {
        let mut work = WriteWork::default();
        // `svg` bound to the W3C's namespace is no prefix for ODF's: `svg1` is declared. The rest are as the place binds them.
        let (mut scopes, scope) = Scopes::with_scope(&[("svg", Ns::Svg)], Some("draw"), None);
        let p = Pfx::of(&mut scopes, scope, &mut work);
        assert_eq!((p.draw.as_str(), p.svg.as_str()), ("draw", "svg1"));
        assert_eq!((p.fo.as_str(), p.style.as_str(), p.text.as_str()), ("fo1", "style1", "text1"));
        assert!(p.declare.contains(&format!("xmlns:svg1=\"{SVG_URI}\"")) && !p.declare.contains("xmlns:draw"), "{}", p.declare);
        assert_eq!(p.attr(Ns::Svg, "x", "a\"b"), " svg1:x=\"a&quot;b\"");
        assert_eq!(p.without_declarations().declare, "");
        assert_eq!(p.of_ns(Ns::Draw), "draw");
        assert_eq!(p.of_ns(Ns::Fo), "fo1");
        assert_eq!(p.of_ns(Ns::Style), "style1");
        assert_eq!(p.of_ns(Ns::Text), "text1");
        assert_eq!(p.attr(Ns::Style, "name", "gr1"), " style1:name=\"gr1\"");
        assert_eq!(p.attr(Ns::Text, "style-name", "P1"), " text1:style-name=\"P1\"");
    }
}
