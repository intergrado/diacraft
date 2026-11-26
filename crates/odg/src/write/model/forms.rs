//! **The properties of a style the model holds whole, in the form ODF 1.4 takes**.
//!
//! The styles the `.odg` reader keeps whole are written whole ([`super::document`]): the automatic styles of the graphic, paragraph and text families that nothing names, the
//! default styles, and the styles of the other families. A file labelled 1.0 or 1.1 passes the schema of its own version, which takes any attribute on a style's properties, and the part
//! written is labelled 1.4, whose schema does not. The full gate's schema check found two classes in what a visit wrote of such styles: `text:enable-numbering` on the paragraph properties of a style
//! no paragraph names, and a weight no version of the list has (`fo:font-weight="250"`). **A style a paragraph or a run names is written with the same treatment** (`write/text/model.rs`: the
//! attribute rides in `diacraft:older-enable-numbering`), and this module gives it to the styles written whole, on their `style:paragraph-properties` and `style:text-properties`:
//!
//! * an attribute of the standard's own namespaces other than `fo` and `style` that the schemas of 1.2 and 1.4 have no place for ([`crate::ext::odf14::refused_on_properties`]) **moves into the
//!   attribute of DiaCraft's own that carries it**, where it has one (`text:enable-numbering`), in the place it stood, and is **left out and counted** where it has none;
//! * a weight, a slant or a variant that 1.4 lists no value for ([`crate::ext::odf14::form_14`]) is written as the value it lists nearest, and **the value it had rides in an attribute of
//!   DiaCraft's own at the end of the same element** (`diacraft:older-font-weight`, [`crate::ext::odf14::listed`]).
//!
//! The reader puts each back where it stood (`read::odfdoc`), so a model read from the file written is the one read from the file visited. The attribute of DiaCraft's own is declared on the
//! properties element when nothing in force binds its prefix to DiaCraft's namespace.
//!
//! **Cost.** One step for each child of the style and one for each attribute of each properties element, charged before the walk (the style was bounded and charged when it was copied,
//! [`super::document`]'s `tree`); nothing is copied but the values of the attributes that change.

use std::collections::HashSet;

use diacraft_container::xml::{El, Node};

use crate::ext::names::PREFIX;
use crate::ext::odf14;
use crate::write::kept::{Bindings, declare_into, declared_by};
use crate::write::{WriteError, WriteWork};

/// What an attribute of a properties element becomes.
enum Change {
    /// The attribute stays, with this value (the value it had rides in the carrier `.0`).
    Value(&'static str, &'static str),
    /// The attribute becomes the carrier `.0`, in its place.
    Move(&'static str),
    /// No version of the schema has a place for it, and no carrier is made for it.
    Drop,
}

/// The local name of a name as written.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// The prefixes in force on a properties element: its own declarations, the style's, and the container's.
struct Scope<'a> {
    own: Bindings,
    style: &'a Bindings,
    outer: &'a Bindings,
}

impl Scope<'_> {
    /// The namespace `prefix` stands for there (`None` for one nothing binds and for a binding to the empty name).
    fn uri(&self, prefix: &str) -> Option<&str> {
        self.own.get(prefix).or_else(|| self.style.get(prefix)).or_else(|| self.outer.get(prefix)).map(String::as_str).filter(|uri| !uri.is_empty())
    }
}

/// What the attribute `name`, `value`, of the properties element `holder` (its local name) becomes in `scope`; `None` for one that is as it is.
fn change_of(name: &str, value: &str, holder: &str, scope: &Scope<'_>) -> Option<Change> {
    if declared_by(name).is_some() {
        return None;
    }
    let (prefix, local) = name.split_once(':')?;
    let uri = scope.uri(prefix)?;
    match odf14::space_of(uri) {
        // `fo` and `style` are written as they always were, but for the values 1.4 lists (a text's own).
        Some(space) => {
            let (kind, carrier) = odf14::listed(space, local).filter(|_| holder == "text-properties")?;
            odf14::form_14(kind, value).map(|form| Change::Value(carrier, form))
        }
        None if odf14::refused_on_properties(holder, uri, local) => {
            Some(odf14::carrier_on_properties(holder, uri, local).map_or(Change::Drop, Change::Move))
        }
        None => None,
    }
}

/// The properties elements of the style `style` (written whole, in a container where `outer` bindings are in force) in the form ODF 1.4 takes, as the [module documentation](self) says. The
/// number of attributes left out, or whose value could not be carried, because no version of the schema has a place for them and no carrier is made for them or the carrier's prefix means
/// another namespace there; the save is marked as one that writes a form of an earlier version in 1.4's when a value was changed or moved (`WriteWork::forms_1_4`).
pub(super) fn in_1_4_form(style: &mut El, outer: &Bindings, work: &mut WriteWork) -> Result<usize, WriteError> {
    work.charge_steps(style.children.len())?;
    let mut style_own = Bindings::new();
    declare_into(&mut style_own, style.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let mut left = 0usize;
    for child in style.children.iter_mut() {
        let Node::El(props) = child else { continue };
        let holder = local(&props.name).to_owned();
        if !matches!(holder.as_str(), "paragraph-properties" | "text-properties") {
            continue;
        }
        work.charge_steps(props.attrs.len())?;
        left = left.saturating_add(properties_in_1_4_form(props, &holder, &style_own, outer, work));
    }
    Ok(left)
}

/// Whether `name` is an attribute of DiaCraft's own under the prefix [`PREFIX`].
fn is_diacraft(name: &str) -> bool {
    name.split_once(':').is_some_and(|(prefix, _)| prefix == PREFIX)
}

/// One properties element `holder` in the form ODF 1.4 takes: see [`in_1_4_form`]. The attributes left out.
fn properties_in_1_4_form(props: &mut El, holder: &str, style_own: &Bindings, outer: &Bindings, work: &mut WriteWork) -> usize {
    let mut own = Bindings::new();
    declare_into(&mut own, props.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let scope = Scope { own, style: style_own, outer };
    if !props.attrs.iter().any(|(k, v)| change_of(k, v, holder, &scope).is_some()) {
        return 0;
    }
    // The carriers are written with the prefix of DiaCraft's namespace: bound so where the element is, or declared on it, which is no change to any attribute of the element when none uses
    // the prefix for another namespace.
    let bound_here = scope.uri(PREFIX) == Some(diacraft_doc::DIACRAFT_URN);
    let can_carry = bound_here || scope.uri(PREFIX).is_none() || !props.attrs.iter().any(|(k, _)| is_diacraft(k));
    let carrier_name = |carrier: &str| format!("{PREFIX}:{carrier}");
    // The names the element holds, and then the carriers written: a set, so that each carrier is looked up in one step however many attributes the element holds.
    let mut taken: HashSet<String> = props.attrs.iter().map(|(k, _)| k.clone()).collect();
    let mut out: Vec<(String, String)> = Vec::with_capacity(props.attrs.len().saturating_add(2));
    let mut later: Vec<(&'static str, String)> = Vec::new();
    let (mut left, mut carried) = (0usize, false);
    for (name, value) in std::mem::take(&mut props.attrs) {
        match change_of(&name, &value, holder, &scope) {
            None => out.push((name, value)),
            Some(Change::Value(carrier, form)) => {
                if can_carry {
                    later.push((carrier, value));
                } else {
                    left = left.saturating_add(1);
                }
                out.push((name, form.to_owned()));
            }
            // In the place the attribute stood, so that what a read puts back is where it was; a name the element holds already is left alone, and the attribute with it (counted).
            Some(Change::Move(carrier)) if can_carry && !taken.contains(&carrier_name(carrier)) => {
                carried = true;
                taken.insert(carrier_name(carrier));
                out.push((carrier_name(carrier), value));
            }
            Some(Change::Move(_) | Change::Drop) => left = left.saturating_add(1),
        }
    }
    // The carriers of the values at the end, once each, where the name is free.
    for (carrier, value) in later {
        if taken.insert(carrier_name(carrier)) {
            carried = true;
            out.push((carrier_name(carrier), value));
        } else {
            left = left.saturating_add(1);
        }
    }
    if carried && !bound_here {
        let declaration = format!("xmlns:{PREFIX}");
        out.retain(|(n, _)| *n != declaration);
        out.insert(0, (declaration, diacraft_doc::DIACRAFT_URN.to_owned()));
    }
    props.attrs = out;
    if carried {
        work.forms_1_4 = true;
    }
    left
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const DIA: &str = diacraft_doc::DIACRAFT_URN;

    fn bindings(pairs: &[(&str, &str)]) -> Bindings {
        pairs.iter().map(|(p, u)| ((*p).to_owned(), (*u).to_owned())).collect()
    }

    fn standard() -> Bindings {
        bindings(&[
            ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
            ("fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
            ("text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
            ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
        ])
    }

    fn style(holder: &str, attrs: &[(&str, &str)]) -> El {
        El {
            name: "style:style".to_owned(),
            attrs: vec![("style:name".to_owned(), "P9".to_owned())],
            children: vec![Node::El(El {
                name: holder.to_owned(),
                attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
                children: Vec::new(),
            })],
        }
    }

    fn attrs_of(style: &El) -> Vec<(String, String)> {
        style.children.iter().find_map(|c| if let Node::El(e) = c { Some(e.attrs.clone()) } else { None }).unwrap_or_default()
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    /// `text:enable-numbering` moves to its carrier in the place it stood, and the prefix is declared on the element.
    #[test]
    fn enable_numbering_moves_to_its_carrier_in_its_place_and_the_prefix_is_declared() {
        let mut s =
            style("style:paragraph-properties", &[("fo:text-align", "center"), ("text:enable-numbering", "false"), ("fo:margin-left", "1cm")]);
        let mut work = WriteWork::default();
        assert_eq!(in_1_4_form(&mut s, &standard(), &mut work).unwrap(), 0);
        assert_eq!(
            attrs_of(&s),
            [
                pair("xmlns:diacraft", DIA),
                pair("fo:text-align", "center"),
                pair("diacraft:older-enable-numbering", "false"),
                pair("fo:margin-left", "1cm")
            ]
        );
        assert!(work.forms_1_4);
        // Where the prefix is DiaCraft's already, no declaration is made; where it is another namespace's and an attribute of the element needs it so, no carrier is written and the attribute is counted.
        let mut s = style("style:paragraph-properties", &[("text:enable-numbering", "true")]);
        let mut bound = standard();
        bound.insert("diacraft".to_owned(), DIA.to_owned());
        assert_eq!(in_1_4_form(&mut s, &bound, &mut WriteWork::default()).unwrap(), 0);
        assert_eq!(attrs_of(&s), [pair("diacraft:older-enable-numbering", "true")]);
        let mut s = style("style:paragraph-properties", &[("text:enable-numbering", "true"), ("diacraft:other", "1")]);
        let mut other = standard();
        other.insert("diacraft".to_owned(), "urn:example:other".to_owned());
        let mut work = WriteWork::default();
        assert_eq!(in_1_4_form(&mut s, &other, &mut work).unwrap(), 1, "the carrier cannot be written: the prefix means another namespace");
        assert_eq!(attrs_of(&s), [pair("diacraft:other", "1")]);
        assert!(!work.forms_1_4, "nothing was carried");
    }

    /// An attribute no version of the schema has a place for and that has no carrier is left out and counted; one of `fo`, `style`, a foreign namespace or none is let be.
    #[test]
    fn an_attribute_with_no_place_and_no_carrier_is_left_out_and_the_others_are_let_be() {
        let mut s = style(
            "style:paragraph-properties",
            &[
                ("draw:fill-color", "#ff0000"),
                ("fo:text-align", "center"),
                ("style:writing-mode", "lr-tb"),
                ("ex:note", "x"),
                ("plain", "y"),
                ("text:number-lines", "true"),
            ],
        );
        let mut outer = standard();
        outer.insert("ex".to_owned(), "urn:example:ex".to_owned());
        let mut work = WriteWork::default();
        assert_eq!(in_1_4_form(&mut s, &outer, &mut work).unwrap(), 1);
        assert_eq!(
            attrs_of(&s),
            [
                pair("fo:text-align", "center"),
                pair("style:writing-mode", "lr-tb"),
                pair("ex:note", "x"),
                pair("plain", "y"),
                pair("text:number-lines", "true")
            ]
        );
        assert!(!work.forms_1_4, "nothing was written in another form");
    }

    /// A weight 1.4 lists no value for is the nearest, its value at the end of the element in a carrier; a value 1.4 takes, an element that holds none and other elements are let be.
    #[test]
    fn a_weight_is_written_as_the_nearest_value_and_carried_and_a_valid_one_is_let_be() {
        let mut s = style(
            "style:text-properties",
            &[("fo:font-weight", "250"), ("fo:font-style", "italic"), ("style:font-weight-complex", "heavy"), ("fo:color", "#fff")],
        );
        let mut work = WriteWork::default();
        assert_eq!(in_1_4_form(&mut s, &standard(), &mut work).unwrap(), 0);
        assert_eq!(
            attrs_of(&s),
            [
                pair("xmlns:diacraft", DIA),
                pair("fo:font-weight", "200"),
                pair("fo:font-style", "italic"),
                pair("style:font-weight-complex", "normal"),
                pair("fo:color", "#fff"),
                pair("diacraft:older-font-weight", "250"),
                pair("diacraft:older-font-weight-complex", "heavy"),
            ]
        );
        assert!(work.forms_1_4);
        // The same attributes on paragraph properties are none of the group's.
        let mut s = style("style:paragraph-properties", &[("fo:font-weight", "250")]);
        assert_eq!(in_1_4_form(&mut s, &standard(), &mut WriteWork::default()).unwrap(), 0);
        assert_eq!(attrs_of(&s), [pair("fo:font-weight", "250")]);
        // Nothing to change: the style is not rebuilt, and the save is not marked.
        let mut s = style("style:text-properties", &[("fo:font-weight", "bold"), ("fo:font-variant", "small-caps")]);
        let mut work = WriteWork::default();
        assert_eq!(in_1_4_form(&mut s, &standard(), &mut work).unwrap(), 0);
        assert_eq!(attrs_of(&s), [pair("fo:font-weight", "bold"), pair("fo:font-variant", "small-caps")]);
        assert!(!work.forms_1_4);
    }

    /// A model of a hostile file may hold the carrier's name already: it is never written twice, and what cannot be written is counted.
    #[test]
    fn a_carrier_is_never_written_twice() {
        let mut s = style("style:text-properties", &[("fo:font-weight", "250"), ("diacraft:older-font-weight", "x")]);
        let mut bound = standard();
        bound.insert("diacraft".to_owned(), DIA.to_owned());
        assert_eq!(in_1_4_form(&mut s, &bound, &mut WriteWork::default()).unwrap(), 1);
        assert_eq!(attrs_of(&s), [pair("fo:font-weight", "200"), pair("diacraft:older-font-weight", "x")]);
        let mut s = style("style:paragraph-properties", &[("text:enable-numbering", "false"), ("diacraft:older-enable-numbering", "true")]);
        assert_eq!(in_1_4_form(&mut s, &bound, &mut WriteWork::default()).unwrap(), 1);
        assert_eq!(attrs_of(&s), [pair("diacraft:older-enable-numbering", "true")]);
    }

    /// The walk is charged: one step for each child of the style and one for each attribute of a properties element.
    #[test]
    fn the_walk_is_charged_a_step_for_each_child_and_each_attribute() {
        let mut s = style("style:text-properties", &[("fo:color", "#fff"), ("fo:font-size", "9pt")]);
        let mut work = WriteWork::default();
        in_1_4_form(&mut s, &standard(), &mut work).unwrap();
        assert_eq!(work.steps_used(), 1 + 2);
        let mut tight = WriteWork { step_cap: 2, ..WriteWork::default() };
        assert!(matches!(in_1_4_form(&mut s, &standard(), &mut tight), Err(WriteError::Limit { .. })));
    }
}
