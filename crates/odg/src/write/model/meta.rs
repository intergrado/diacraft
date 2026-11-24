//! `meta.xml` written from the model (the native format design's §3.4 row for it).
//!
//! **What it writes.** The document's title (`dc:title`), the fields of [`DocProps`] that both formats say (`dc:description`, `dc:subject`, `meta:keyword`,
//! `meta:initial-creator` for the creator, `dc:creator` for the last modifier, `meta:creation-date`, `dc:date`, `meta:print-date`, `dc:language`), each user-defined property
//! (`meta:user-defined`), and the fields the reader kept whole ([`diacraft_doc::OdfDoc::meta`]), every one at its place: the typed fields in the order of the schema, the kept nodes
//! after the number of typed fields they came after (`read/keep.rs`). **Only fields with a value are written**: the reader types a field only while it holds text.
//!
//! **The generator** (design 4.4): with [`super::WriteOptions::content_unchanged`](crate::WriteOptions::content_unchanged) true the generator the model holds is written as it was
//! read; otherwise DiaCraft's own is (`DiaCraft/` and the version), in the place of every generator the model holds. A model that holds none is given DiaCraft's. **The dates are the model's**: no clock is
//! read (the writer has none), so the modification date is renewed only by whoever sets it in the properties, as the splice writer leaves it.
//!
//! **A VSDX property** (a user-defined property that came from a VSDX file) is written as an ODF one by its name, its value and the nearest ODF type ([`odf_custom`]); the attributes VSDX
//! keeps on it have no place in `meta.xml` and are not written. [`odf_props`] says what the properties are after that: the model a read of the file gives back.
//!
//! **The root** keeps the attributes the model holds for it (the namespace declarations among them), and is given the three declarations this writer's own names need
//! (`office`, `meta`, `dc`) when it has none; one that binds such a prefix to another namespace is refused in words. A model with no root gets the default.
//!
//! **Cost.** The kept nodes are bounded by [`super::bounded`] (the reader's small-part cap) before a tree is made of them, so a hostile model cannot make a copy larger than the part
//! the reader would read back as fields.

use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{CustomProp, DocProps, Document, Format, Placed, RawNode};

use super::{Declared, WORDS_NAMES, WORDS_SMALL_PART, bounded, ensure_declarations, root_of, set_version};
use crate::base::VERSION;
use crate::limits::Limits;
use crate::write::{GENERATOR, WriteError, WriteWork};

/// The namespace of the `meta` vocabulary and of Dublin Core's, as a root declares them.
const META_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const DC_URI: &str = "http://purl.org/dc/elements/1.1/";
const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

/// The name of the part.
pub(crate) const PART: &str = "meta.xml";

/// The ODF type nearest to a VSDX property's type: strings are strings, numbers floats and truth values booleans; any other type (a date as VSDX spells it, a blob) is a string, which holds
/// its text exactly.
fn odf_type(vsdx: &str) -> &'static str {
    match vsdx.to_ascii_lowercase().as_str() {
        "i1" | "i2" | "i4" | "i8" | "int" | "ui1" | "ui2" | "ui4" | "ui8" | "uint" | "r4" | "r8" | "decimal" | "float" | "double" | "number"
        | "cy" => "float",
        "bool" | "boolean" => "boolean",
        _ => "string",
    }
}

/// A user-defined property as `meta.xml` says it, and as the reader gives it back: an ODF property is as it is; a VSDX one is an ODF one with the nearest ODF type and none of VSDX's
/// own attributes.
pub(crate) fn odf_custom(prop: &CustomProp) -> CustomProp {
    match prop.format {
        Format::Odf => prop.clone(),
        Format::Vsdx | Format::Native => CustomProp {
            format: Format::Odf,
            name: Arc::clone(&prop.name),
            value_type: Arc::from(odf_type(&prop.value_type)),
            value: Arc::clone(&prop.value),
            raw: diacraft_doc::RawAttrs::new(),
        },
    }
}

/// The properties a read of the file this module writes gives back: every property of the document, the user-defined ones as [`odf_custom`] makes them. The fields ODF has no place
/// for are the caller's to have refused or left out first.
pub(crate) fn odf_props(props: &DocProps) -> DocProps {
    DocProps { custom: props.custom.iter().map(odf_custom).collect(), ..props.clone() }
}

/// A text element `<name>text</name>`.
fn text_el(name: &str, text: &str) -> El {
    El { name: name.to_owned(), attrs: Vec::new(), children: vec![Node::Text(text.to_owned())] }
}

/// The fields of the document, typed, in the order of the schema, each with a value.
fn typed_fields(doc: &Document) -> Vec<El> {
    let p = &doc.props;
    let mut out = Vec::new();
    let mut put = |name: &str, value: &str| {
        if !value.is_empty() {
            out.push(text_el(name, value));
        }
    };
    put("dc:title", &doc.title);
    put("dc:description", p.description.as_deref().unwrap_or_default());
    put("dc:subject", p.subject.as_deref().unwrap_or_default());
    put("meta:keyword", p.keywords.as_deref().unwrap_or_default());
    put("meta:initial-creator", p.creator.as_deref().unwrap_or_default());
    put("dc:creator", p.last_modified_by.as_deref().unwrap_or_default());
    put("meta:creation-date", p.created.as_deref().unwrap_or_default());
    put("dc:date", p.modified.as_deref().unwrap_or_default());
    put("meta:print-date", p.last_printed.as_deref().unwrap_or_default());
    put("dc:language", p.language.as_deref().unwrap_or_default());
    for custom in &p.custom {
        let prop = odf_custom(custom);
        let mut attrs = vec![("meta:name".to_owned(), prop.name.to_string())];
        if !prop.value_type.is_empty() {
            attrs.push(("meta:value-type".to_owned(), prop.value_type.to_string()));
        }
        attrs.extend(prop.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())));
        out.push(El { name: "meta:user-defined".to_owned(), attrs, children: vec![Node::Text(prop.value.to_string())] });
    }
    out
}

/// Whether the node is a `meta:generator` element, by the prefix the root binds to the meta namespace.
fn is_generator(node: &RawNode, prefixes: &[&str]) -> bool {
    let RawNode::Element(el) = node else { return false };
    el.name.split_once(':').is_some_and(|(prefix, local)| local == "generator" && prefixes.contains(&prefix))
}

/// The prefixes a root binds to the meta namespace.
fn meta_prefixes(declared: &Declared) -> Vec<&str> {
    declared.bound(META_URI)
}

/// `meta.xml` as the model says it. `keep_generator` says to write the generator the model holds as it is (the content is as it was read).
pub(super) fn write(doc: &Document, keep_generator: bool, limits: &Limits, work: &mut WriteWork) -> Result<Vec<u8>, WriteError> {
    let held = doc.ext.get().map(|d| &d.odf);
    let mut attrs = root_of(held.map(|o| o.roots.as_slice()), PART, VERSION);
    let declared = ensure_declarations(&mut attrs, &[("office", OFFICE_URI), ("meta", META_URI), ("dc", DC_URI)])?;
    set_version(&mut attrs, &declared, OFFICE_URI, VERSION);
    let kept: Vec<&Placed> = held.map(|o| o.meta.iter().collect()).unwrap_or_default();
    work.charge_steps(kept.len().saturating_add(doc.props.custom.len()))?;
    // The whole of what is written is bounded before a tree is made of it: the kept nodes, and the strings of the fields.
    let field_bytes = typed_fields_bytes(doc);
    if !bounded(kept.iter().map(|p| &p.node), usize::try_from(limits.max_small_part_bytes).unwrap_or(usize::MAX).saturating_sub(field_bytes)) {
        return Err(WriteError::Unsupported(WORDS_SMALL_PART));
    }
    let typed = typed_fields(doc);
    let prefixes = meta_prefixes(&declared);
    let holds_generator = kept.iter().any(|p| is_generator(&p.node, &prefixes));
    let own_generator = El { name: "meta:generator".to_owned(), attrs: Vec::new(), children: vec![Node::Text(GENERATOR.to_owned())] };
    let mut children: Vec<Node> = Vec::new();
    // DiaCraft's generator comes first, where the schema puts it, unless the content is as it was read and the model holds one.
    let drop_held = !keep_generator;
    if drop_held || !holds_generator {
        children.push(Node::El(own_generator));
    }
    let mut typed = typed.into_iter();
    let mut written = 0u32;
    for placed in kept.iter().filter(|p| !(drop_held && is_generator(&p.node, &prefixes))) {
        while written < placed.after {
            let Some(field) = typed.next() else { break };
            children.push(Node::El(field));
            written += 1;
        }
        children.push(diacraft_sheetxml::passthrough::node_to_tree(&placed.node));
    }
    children.extend(typed.map(Node::El));
    let holder = El { name: "office:meta".to_owned(), attrs: Vec::new(), children };
    let root = El { name: "office:document-meta".to_owned(), attrs, children: vec![Node::El(holder)] };
    let bytes = diacraft_container::xml::to_bytes(&root, b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", b"\n", true)
        .map_err(|_| WriteError::Unsupported(WORDS_NAMES))?;
    work.charge_copy(bytes.len(), limits)?;
    if bytes.len() as u64 > limits.max_small_part_bytes {
        return Err(WriteError::Unsupported(WORDS_SMALL_PART));
    }
    Ok(bytes)
}

/// The bytes the typed fields come to, for the bound on what is written: the length of every string a field is written from, and a fixed overhead for each field and each property.
fn typed_fields_bytes(doc: &Document) -> usize {
    /// What the name of an element and the punctuation of a field take, at most.
    const FIELD: usize = 64;
    let p = &doc.props;
    let fields = [
        Some(&doc.title),
        p.description.as_ref(),
        p.subject.as_ref(),
        p.keywords.as_ref(),
        p.creator.as_ref(),
        p.last_modified_by.as_ref(),
        p.created.as_ref(),
        p.modified.as_ref(),
        p.last_printed.as_ref(),
        p.language.as_ref(),
    ];
    let mut total = fields.iter().fold(0usize, |sum, s| sum.saturating_add(s.map_or(0, |s| s.len())).saturating_add(FIELD));
    for c in &p.custom {
        let raw: usize = c.raw.iter().fold(0usize, |sum, (k, v)| sum.saturating_add(k.len()).saturating_add(v.len()).saturating_add(4));
        total = total
            .saturating_add(c.name.len())
            .saturating_add(c.value_type.len())
            .saturating_add(c.value.len())
            .saturating_add(raw)
            .saturating_add(FIELD);
    }
    total
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A VSDX property type is the nearest ODF one: strings are strings, numbers floats, truth values booleans, any other a string (which holds its text exactly), in any case.
    #[test]
    fn a_vsdx_type_is_the_nearest_odf_type() {
        for (vsdx, odf) in [
            ("lpwstr", "string"),
            ("LPSTR", "string"),
            ("i4", "float"),
            ("R8", "float"),
            ("decimal", "float"),
            ("bool", "boolean"),
            ("Boolean", "boolean"),
            ("filetime", "string"),
            ("", "string"),
            ("anything else", "string"),
        ] {
            assert_eq!(odf_type(vsdx), odf, "{vsdx}");
        }
    }

    /// An ODF property is as it is; a VSDX one is an ODF one with its name and value, the nearest type and none of VSDX's attributes; and the properties after it are the same but for
    /// the user-defined ones.
    #[test]
    fn a_property_a_read_gives_back_is_what_was_written() {
        let vsdx = CustomProp {
            format: Format::Vsdx,
            name: Arc::from("Count"),
            value_type: Arc::from("i4"),
            value: Arc::from("7"),
            raw: diacraft_doc::RawAttrs::from_pairs([("fmtid", "{x}"), ("pid", "2")]),
        };
        let odf = CustomProp { format: Format::Odf, ..vsdx.clone() };
        assert_eq!(odf_custom(&odf), odf, "an ODF property is as it is");
        let back = odf_custom(&vsdx);
        assert_eq!((back.format, &*back.name, &*back.value_type, &*back.value, back.raw.is_empty()), (Format::Odf, "Count", "float", "7", true));
        let props = DocProps { subject: Some(Arc::from("s")), custom: vec![vsdx, odf.clone()], ..DocProps::default() };
        let after = odf_props(&props);
        assert_eq!(after.subject, props.subject);
        assert_eq!(after.custom, [back, odf]);
        assert_eq!(odf_props(&after), after, "what a read gives back is a fixed point");
    }

    /// The generator is found by the prefix the root binds to the meta namespace, and by it alone.
    #[test]
    fn a_generator_is_told_by_the_prefix_the_root_binds() {
        let element = |name: &str| {
            RawNode::Element(diacraft_doc::RawElement { name: Arc::from(name), attrs: diacraft_doc::RawAttrs::new(), children: Vec::new() })
        };
        let generator = element("m:generator");
        assert!(is_generator(&generator, &["m"]));
        assert!(!is_generator(&generator, &["meta"]), "another prefix");
        assert!(!is_generator(&RawNode::Text(Arc::from("m:generator")), &["m"]), "a text is no element");
        assert!(!is_generator(&element("m:title"), &["m"]));
        assert!(!is_generator(&element("generator"), &["m"]), "no prefix is no namespace");
    }
}
