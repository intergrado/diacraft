//! The property parts: `docProps/core.xml` and `docProps/app.xml`.
//!
//! What is taken from the parts is what the document
//! functions read: the `title` that is the document's, the other six core elements, the three dates
//! (`created`, `modified`, `lastPrinted`) and the `language` that the date functions read, `Company`, `Manager`,
//! `HyperlinkBase` and `AppVersion` of the extended properties, each as the element's text, unchanged. An element that is missing is
//! `None`; one that is present and empty is `Some("")`, as the spec's tell them apart. The writer's own name,
//! `Application`, goes to the report for the census's definition of a VSDX file and to no field of the model: it is held, with every other element and attribute no
//! field reads, in `VsdxHeld::props_rest` (`held.rs`), and the custom properties are `DocProps::custom`.
//!
//! Elements are found by their local names (`dc:title` and `title` are one), anywhere directly under the root, the first of each name.

use std::sync::Arc;

use diacraft_doc::DocProps;
use diacraft_opc::xml::El;

/// The content types of the three property parts.
pub(crate) const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
pub(crate) const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
pub(crate) const CT_CUSTOM: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";

/// The namespaces of the property parts, and the prefixes a writer declares on their roots (`write/props.rs`): the declarations the reader does not hold,
/// since every writer regenerates them.
pub(crate) const NS_APP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
pub(crate) const NS_CUSTOM: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
pub(crate) const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
pub(crate) const NS_CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_TERMS: &str = "http://purl.org/dc/terms/";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// The local names of the elements of `docProps/core.xml` that fields of the model read, the first of each; every other element is held (`VsdxHeld::props_rest`).
pub(crate) const CORE_READ: &[&str] =
    &["title", "subject", "creator", "lastModifiedBy", "keywords", "description", "category", "created", "modified", "lastPrinted", "language"];

/// The order a writer makes the field elements of `docProps/core.xml` in: the model writer's (`write/props.rs`). A part whose fields came in another order has its own held (`hold_props_rest`).
pub(crate) const CORE_ORDER: [&str; 11] =
    ["title", "subject", "creator", "keywords", "description", "lastModifiedBy", "lastPrinted", "created", "modified", "category", "language"];

/// The same for `docProps/app.xml`.
pub(crate) const APP_ORDER: [&str; 4] = ["Manager", "Company", "HyperlinkBase", "AppVersion"];

/// The same for `docProps/app.xml`. `Application` is not among them: the writer's name goes to the report, and is held with the rest.
pub(crate) const APP_READ: &[&str] = &["Company", "Manager", "HyperlinkBase", "AppVersion"];

/// Whether a declaration of the root of `docProps/core.xml` is one the writer makes itself.
pub(crate) fn is_standard_core(name: &str, value: &str) -> bool {
    matches!((name, value), ("xmlns:cp", NS_CORE) | ("xmlns:dc", NS_DC) | ("xmlns:dcterms", NS_TERMS) | ("xmlns:xsi", NS_XSI))
}

/// Whether a declaration of the root of `docProps/app.xml` is one the writer makes itself.
pub(crate) fn is_standard_app(name: &str, value: &str) -> bool {
    matches!((name, value), ("xmlns", NS_APP) | ("xmlns:vt", NS_VT))
}

/// The property part of one kind, found through the package relationships **by its content type as well as by the relationship's type**: a file may
/// hang its extended properties under the core properties' relationship (one in the sample files do), and then the part still says what it is. `reached`
/// is the package relationships to parts that are there, in order, as `(relationship type, part name)`; `rel` is the relationship type the kind has,
/// `content_type` the type its part has, and `type_of` gives a part's content type. In order of preference:
///
/// 1. a relationship of the kind's type to a part of the kind's content type (every ordinary file);
/// 2. a relationship of any type to a part of the kind's content type (the part says what it is, whoever points at it);
/// 3. a relationship of the kind's type to a part whose content type is not that of another kind of property part (a part with no `Override`, say). A
///    part that says it is another kind is not this one however it is reached.
///
/// The first such relationship wins. A part that no relationship reaches is not found: it is an unknown part.
pub(crate) fn property_part<'a, 't>(
    reached: &[(&str, &'a str)],
    rel: &str,
    content_type: &str,
    type_of: impl Fn(&str) -> Option<&'t str>,
) -> Option<&'a str> {
    let is = |part: &str, ct: &str| type_of(part).is_some_and(|c| c.eq_ignore_ascii_case(ct));
    let kind_of_other = |part: &str| [CT_APP, CT_CORE, CT_CUSTOM].iter().any(|ct| !ct.eq_ignore_ascii_case(content_type) && is(part, ct));
    reached
        .iter()
        .find(|(ty, part)| *ty == rel && is(part, content_type))
        .or_else(|| reached.iter().find(|(_, part)| is(part, content_type)))
        .or_else(|| reached.iter().find(|(ty, part)| *ty == rel && !kind_of_other(part)))
        .map(|(_, part)| *part)
}

/// What the two parts gave.
#[derive(Default)]
pub(crate) struct Props {
    pub title: Option<Arc<str>>,
    pub props: DocProps,
    pub application: Option<String>,
}

/// The text of the first child of `root` whose local name is `local`.
fn element_text(root: &El, local: &str) -> Option<Arc<str>> {
    root.child(local).map(|e| Arc::from(e.text()))
}

/// Reads the tree of `docProps/core.xml` into `into`. `Err` carries the class of what is wrong with it, and then nothing was read.
pub(crate) fn read_core(root: &El, into: &mut Props) -> Result<(), &'static str> {
    if !root.is("coreProperties") {
        return Err("properties_unexpected_root");
    }
    into.title = element_text(root, "title");
    into.props.subject = element_text(root, "subject");
    into.props.creator = element_text(root, "creator");
    into.props.last_modified_by = element_text(root, "lastModifiedBy");
    into.props.keywords = element_text(root, "keywords");
    into.props.description = element_text(root, "description");
    into.props.category = element_text(root, "category");
    // `created`, `modified` and `lastPrinted` are what `DOCCREATION`, `DOCLASTSAVE`, `DOCLASTEDIT` and `DOCLASTPRINT` read; `language` is the
    // locale a string date is read in. Text as the file has it, never parsed here.
    into.props.created = element_text(root, "created");
    into.props.modified = element_text(root, "modified");
    into.props.last_printed = element_text(root, "lastPrinted");
    into.props.language = element_text(root, "language");
    Ok(())
}

/// Reads the tree of `docProps/app.xml` into `into`.
pub(crate) fn read_app(root: &El, into: &mut Props) -> Result<(), &'static str> {
    if !root.is("Properties") {
        return Err("properties_unexpected_root");
    }
    into.props.company = element_text(root, "Company");
    into.props.manager = element_text(root, "Manager");
    into.props.hyperlink_base = element_text(root, "HyperlinkBase");
    into.props.app_version = element_text(root, "AppVersion");
    into.application = root.child("Application").map(|e| e.text().chars().take(64).collect());
    Ok(())
}
