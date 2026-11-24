//! The property parts: `docProps/app.xml`, `docProps/core.xml` and `docProps/custom.xml`.
//!
//! **New parts** are written whole, as small as the specs ask (V10 of the document spec: `BuildNumberEdited` has low 16 bits above 2714).
//! **Parts of a file that was read** are edited at the tree level: `Application` is set to `DiaCraft` and every other element
//! stays; the page names are replaced in the page group only, found by matching the old names and never by the localised label;
//! `title`, `created` (for a copy of a template) and `modified` are set; and `RecalcDocument` is added to the custom properties when asked, once, with a `pid` above the
//! largest there.

use std::sync::Arc;

use diacraft_doc::{CustomProp, DocProps, Format, Kept, RawAttrs, RawElement, RawNode};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::build::weave;

use super::WriteError;
use super::build::{charge_kept, element, take_kept_for, with_kept_attrs};
use super::parts::Tree;

pub(super) const NS_APP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
pub(super) const NS_CUSTOM: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
pub(super) const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
pub(super) const NS_CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_TERMS: &str = "http://purl.org/dc/terms/";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// The name DiaCraft states as the application that wrote a file: never `the reference app`.
pub(super) const APPLICATION: &str = "DiaCraft";

/// The format id of a user-defined property (ECMA-376 Part 1 section 22.3), and of `RecalcDocument`.
const USER_PROPERTY_FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";

/// `BuildNumberEdited` of a new drawing: the low 16 bits must exceed 2714 and every sample's are 4229 or
/// more; the high 16 bits are our reading of "major version times 1024" and no document says so. Assumed about the reference app: 16 times 1024 in the
/// high half, and 4229 in the low.
const BUILD_NUMBER_EDITED: i32 = (16 * 1024) << 16 | 4229;

/// A time as W3CDTF text (`2031-03-07T20:05:09Z`), from seconds since 1970. `None` for a time jiff cannot hold: its civil dates run from -9999 to 9999, and no W3CDTF text has a year
/// past 9999.
pub(super) fn format_time(seconds: i64) -> Option<String> {
    // A civil date and time, so the whole of the year 9999 is held (a jiff `Timestamp` stops at 22:00 on 30 December of it). A duration of more days than the Unix epoch's range is
    // refused whatever it is added to, and every time of -9999 to 9999 is within that range of 1970.
    let at = jiff::civil::DateTime::constant(1970, 1, 1, 0, 0, 0, 0).checked_add(jiff::SignedDuration::from_secs(seconds)).ok()?;
    let rest = jiff::fmt::strtime::format("%m-%dT%H:%M:%SZ", at).ok()?;
    // The year is four digits after a minus sign when it is negative, as W3CDTF and XML Schema write it (jiff's `%Y` writes `-999`).
    Some(format!("{}{:04}-{rest}", if at.year() < 0 { "-" } else { "" }, at.year().unsigned_abs()))
}

fn leaf(name: &str, attrs: Vec<(&str, &str)>, text: &str) -> Node {
    let attrs = attrs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    Node::El(element(name, attrs, if text.is_empty() { vec![] } else { vec![Node::Text(text.to_owned())] }))
}

fn child_el(node: Node) -> El {
    match node {
        Node::El(e) => e,
        _ => El::default(),
    }
}

/// `HeadingPairs` and `TitlesOfParts` for the groups given: `(label, names)`.
fn lists(groups: &[(&str, Vec<String>)]) -> Vec<Node> {
    let groups: Vec<&(&str, Vec<String>)> = groups.iter().filter(|(_, names)| !names.is_empty()).collect();
    if groups.is_empty() {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    let mut titles = Vec::new();
    for (label, names) in &groups {
        pairs.push(Node::El(element("vt:variant", vec![], vec![leaf("vt:lpstr", vec![], label)])));
        pairs.push(Node::El(element("vt:variant", vec![], vec![leaf("vt:i4", vec![], &names.len().to_string())])));
        titles.extend(names.iter().map(|n| leaf("vt:lpstr", vec![], n)));
    }
    let pair_vector = element("vt:vector", vec![("size".to_owned(), pairs.len().to_string()), ("baseType".to_owned(), "variant".to_owned())], pairs);
    let title_vector = element("vt:vector", vec![("size".to_owned(), titles.len().to_string()), ("baseType".to_owned(), "lpstr".to_owned())], titles);
    vec![
        Node::El(element("HeadingPairs", vec![], vec![Node::El(pair_vector)])),
        Node::El(element("TitlesOfParts", vec![], vec![Node::El(title_vector)])),
    ]
}

/// A new `docProps/app.xml`: the fields of `props` it says (`Manager`, `Company`, `HyperlinkBase`), the writer's name and the lists of page and master names.
pub(super) fn new_app(props: &DocProps, pages: Vec<String>, masters: Vec<String>) -> Tree {
    let mut children: Vec<Node> = [("Manager", &props.manager), ("Company", &props.company), ("HyperlinkBase", &props.hyperlink_base)]
        .into_iter()
        .filter_map(|(name, text)| text.as_deref().map(|text| leaf(name, vec![], text)))
        .collect();
    children.push(leaf("Application", vec![], APPLICATION));
    children.extend(lists(&[("Pages", pages), ("Masters", masters)]));
    let root = element("Properties", vec![("xmlns".to_owned(), NS_APP.to_owned()), ("xmlns:vt".to_owned(), NS_VT.to_owned())], children);
    Tree::new(root)
}

/// A new `docProps/core.xml`: the title, **the fields of `props` it says** and the two times, in the order the model writer makes them (`CORE_ORDER`). The fields are the drawing's own: a drawing
/// that was never a VSDX file (one read from an `.odg`, or made here) keeps its subject, creator, keywords, description, category and language in the first VSDX file it is saved as (
/// the save across from an `.odg` named "document properties" in what it carried, and the file had only the title, so they did not come back; the times are the save's).
pub(super) fn new_core(title: &str, props: &DocProps, created: Option<&str>, modified: Option<&str>) -> Tree {
    let mut children = vec![leaf("dc:title", vec![], title)];
    let mut field = |name: &str, text: Option<&str>| {
        if let Some(text) = text {
            children.push(leaf(name, vec![], text));
        }
    };
    field("dc:subject", props.subject.as_deref());
    field("dc:creator", props.creator.as_deref());
    field("cp:keywords", props.keywords.as_deref());
    field("dc:description", props.description.as_deref());
    field("cp:lastModifiedBy", props.last_modified_by.as_deref());
    field("cp:lastPrinted", props.last_printed.as_deref());
    for (name, time) in [("dcterms:created", created), ("dcterms:modified", modified)] {
        if let Some(t) = time {
            children.push(leaf(name, vec![("xsi:type", "dcterms:W3CDTF")], t));
        }
    }
    for (name, text) in [("cp:category", props.category.as_deref()), ("dc:language", props.language.as_deref())] {
        if let Some(text) = text {
            children.push(leaf(name, vec![], text));
        }
    }
    let attrs = [("cp", NS_CORE), ("dc", NS_DC), ("dcterms", NS_TERMS), ("xsi", NS_XSI)].map(|(p, u)| (format!("xmlns:{p}"), u.to_owned())).to_vec();
    Tree::new(element("cp:coreProperties", attrs, children))
}

fn property(pid: u32, name: &str, vt: Node) -> Node {
    Node::El(element(
        "property",
        vec![("fmtid".to_owned(), USER_PROPERTY_FMTID.to_owned()), ("pid".to_owned(), pid.to_string()), ("name".to_owned(), name.to_owned())],
        vec![vt],
    ))
}

/// A new `docProps/custom.xml`: the build number and the unit system, and `RecalcDocument` when the caller asks.
pub(super) fn new_custom(recalc: bool) -> Tree {
    let mut children = vec![
        property(2, "BuildNumberEdited", leaf("vt:i4", vec![], &BUILD_NUMBER_EDITED.to_string())),
        property(3, "IsMetric", leaf("vt:bool", vec![], "false")),
    ];
    if recalc {
        children.push(property(4, "RecalcDocument", leaf("vt:bool", vec![], "true")));
    }
    Tree::new(element("Properties", vec![("xmlns".to_owned(), NS_CUSTOM.to_owned()), ("xmlns:vt".to_owned(), NS_VT.to_owned())], children))
}

// ------------------------------------------------------------------------------------------------------------------ the model's own

use crate::props::{APP_ORDER, CORE_ORDER};

/// The fields of a property part in the order the part had them (`{root}/-order`, held when it is not the order the model writer makes them in: `CORE_ORDER`, `APP_ORDER`), and the ones it did
/// not have after them, in the writer's.
fn fields_in_order(canonical: &[&'static str], fields: &[(Arc<str>, Kept)], root: &str) -> Vec<&'static str> {
    let recorded = field_entry(fields, root, "-order").and_then(|kept| kept.raw.get("order")).unwrap_or("");
    let mut out: Vec<&'static str> = recorded.split(',').filter_map(|name| canonical.iter().copied().find(|c| *c == name)).collect();
    for name in canonical {
        if !out.contains(name) {
            out.push(name);
        }
    }
    out
}

/// What the model holds of the element `local` of the property part `root` beside its text: its attributes and the nodes among its text, as the reader held them (`held.rs`, `hold_field`).
fn field_entry<'a>(fields: &'a [(Arc<str>, Kept)], root: &str, local: &str) -> Option<&'a Kept> {
    fields.iter().find(|(key, _)| key.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) == Some(local)).map(|(_, kept)| kept)
}

/// A property field as an element: with its text, and **as the file had it** when the model holds that (its attributes, which are not the writer's own `xsi:type` when the file had none, and what
/// it held among its text); else as the writer makes it.
fn field_node(name: &str, default_attrs: Vec<(&str, &str)>, text: &str, held: Option<&Kept>) -> Result<Node, WriteError> {
    let Some(kept) = held else { return Ok(leaf(name, default_attrs, text)) };
    charge_kept(kept)?;
    let attrs: Vec<(String, String)> = kept.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    let typed = if text.is_empty() { Vec::new() } else { vec![Node::Text(text.to_owned())] };
    Ok(Node::El(element(name, attrs, weave(typed, &[&kept.kept]))))
}

/// The element a core field is written as: the name with its prefix, and the attributes that say its type.
fn core_element(field: &str, text: &str, held: Option<&Kept>) -> Result<Node, WriteError> {
    let (name, typed) = match field {
        "title" => ("dc:title", false),
        "subject" => ("dc:subject", false),
        "creator" => ("dc:creator", false),
        "description" => ("dc:description", false),
        "language" => ("dc:language", false),
        "keywords" => ("cp:keywords", false),
        "lastModifiedBy" => ("cp:lastModifiedBy", false),
        "lastPrinted" => ("cp:lastPrinted", false),
        "category" => ("cp:category", false),
        "created" => ("dcterms:created", true),
        _ => ("dcterms:modified", true),
    };
    field_node(name, if typed { vec![("xsi:type", "dcterms:W3CDTF")] } else { vec![] }, text, held)
}

/// What `docProps/core.xml` is made from: the title and the fields of the properties, the two times (which are the writer's to choose, `created` and `modified` of the model or the save's),
/// and what the part held beside them.
pub(super) struct CoreWrite<'a> {
    pub title: &'a str,
    pub props: &'a DocProps,
    pub rest: Option<&'a Kept>,
    /// What each field's element held beside its text (`VsdxHeld::props_rest`, the entries named `coreProperties/{field}`).
    pub fields: &'a [(Arc<str>, Kept)],
    pub created: Option<&'a str>,
    pub modified: Option<&'a str>,
}

/// A `docProps/core.xml` made from the model: each field that is there as the element it is, in a fixed order, and the rest of the part (its attributes, comments and elements) put back after as
/// many of the field elements as came before it. **The title is written whether or not it is empty**: the model holds it as text and cannot tell an empty element from none.
pub(super) fn model_core(w: &CoreWrite<'_>) -> Result<Tree, WriteError> {
    let p = w.props;
    let mut typed = Vec::new();
    for field in fields_in_order(&CORE_ORDER, w.fields, "coreProperties") {
        let text = match field {
            // A part that had no title is not given an empty one: the model holds the title as text and cannot tell an empty element from none, and the reader says which it was.
            "title" if w.title.is_empty() && field_entry(w.fields, "coreProperties", "-title").is_some() => None,
            "title" => Some(w.title),
            "subject" => p.subject.as_deref(),
            "creator" => p.creator.as_deref(),
            "keywords" => p.keywords.as_deref(),
            "description" => p.description.as_deref(),
            "lastModifiedBy" => p.last_modified_by.as_deref(),
            "lastPrinted" => p.last_printed.as_deref(),
            "created" => w.created,
            "modified" => w.modified,
            "category" => p.category.as_deref(),
            _ => p.language.as_deref(),
        };
        if let Some(text) = text {
            typed.push(core_element(field, text, field_entry(w.fields, "coreProperties", field))?);
        }
    }
    let decls = [("cp", NS_CORE), ("dc", NS_DC), ("dcterms", NS_TERMS), ("xsi", NS_XSI)].map(|(p, u)| (format!("xmlns:{p}"), u.to_owned())).to_vec();
    take_kept_for(w.rest)?;
    let attrs = with_kept_attrs(decls, w.rest);
    let children = match w.rest {
        Some(kept) => weave(typed, &[&kept.kept]),
        None => typed,
    };
    Ok(Tree::new(element("cp:coreProperties", attrs, children)))
}

/// What `docProps/app.xml` is made from.
pub(super) struct AppWrite<'a> {
    pub props: &'a DocProps,
    pub rest: Option<&'a Kept>,
    /// What each field's element held beside its text (`VsdxHeld::props_rest`, the entries named `Properties/{field}`).
    pub fields: &'a [(Arc<str>, Kept)],
    /// The save is an edited one, or the drawing is new: the writer names itself (`Application`).
    pub names_writer: bool,
    /// The lists of page and master names a part made anew holds; none when the part is the model's (its lists are held, and are the file's).
    pub lists: Option<(Vec<String>, Vec<String>)>,
}

/// A `docProps/app.xml` made from the model: the fields (`Manager`, `Company`, `HyperlinkBase`, `AppVersion`), the rest of the part put back after as many of them as came before it, and
/// the writer's name: `Application` is the one element of the rest that the save sets, when it is an edited one, and is made when there is none.
pub(super) fn model_app(w: &AppWrite<'_>) -> Result<Tree, WriteError> {
    let p = w.props;
    let mut typed = Vec::new();
    for name in fields_in_order(&APP_ORDER, w.fields, "Properties") {
        let text = match name {
            "Manager" => &p.manager,
            "Company" => &p.company,
            "HyperlinkBase" => &p.hyperlink_base,
            _ => &p.app_version,
        };
        if let Some(text) = text {
            typed.push(field_node(name, vec![], text, field_entry(w.fields, "Properties", name))?);
        }
    }
    take_kept_for(w.rest)?;
    let mut children = match w.rest {
        Some(kept) => weave(typed, &[&kept.kept]),
        None => typed,
    };
    if w.names_writer {
        let at = children.iter().position(|n| matches!(n, Node::El(e) if e.is("Application")));
        match at {
            Some(at) => {
                if let Some(Node::El(e)) = children.get_mut(at) {
                    set_text(e, APPLICATION);
                }
            }
            None => children.insert(0, leaf("Application", vec![], APPLICATION)),
        }
    }
    if let Some((pages, masters)) = &w.lists {
        children.extend(lists(&[("Pages", pages.clone()), ("Masters", masters.clone())]));
    }
    let decls = vec![("xmlns".to_owned(), NS_APP.to_owned()), ("xmlns:vt".to_owned(), NS_VT.to_owned())];
    let attrs = with_kept_attrs(decls, w.rest);
    Ok(Tree::new(element("Properties", attrs, children)))
}

/// One property of the custom properties part, as the list holds it: its attributes (the name last), and its value in an element named as the type was written.
fn custom_property(prop: &CustomProp) -> Node {
    let mut attrs: Vec<(String, String)> = prop.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    attrs.push(("name".to_owned(), prop.name.to_string()));
    Node::El(element("property", attrs, vec![leaf(&prop.value_type, vec![], &prop.value)]))
}

/// A `docProps/custom.xml` of the properties the VSDX side of the model lists (an ODF property is the `.odg` carrier's), and, for a drawing that has none and was made new, the build number and the
/// unit system a new drawing has. `recalc` asks for `RecalcDocument`.
pub(super) fn model_custom(list: &[&CustomProp], new_drawing: bool, recalc: bool) -> Result<Tree, WriteError> {
    // What the list makes is taken from the room of the part before any of it is made: a model that lists more than a part may hold is refused in words.
    super::build::take_properties(list)?;
    let mut tree = if list.is_empty() && new_drawing {
        new_custom(false)
    } else {
        let children = list.iter().filter(|c| c.format == Format::Vsdx).map(|c| custom_property(c)).collect();
        Tree::new(element("Properties", vec![("xmlns".to_owned(), NS_CUSTOM.to_owned()), ("xmlns:vt".to_owned(), NS_VT.to_owned())], children))
    };
    if recalc {
        ensure_recalc(&mut tree);
    }
    Ok(tree)
}

// ------------------------------------------------------------------------------------------------------------------ the page names

/// What an edited save does with the lists of names `docProps/app.xml` holds (`HeadingPairs` and `TitlesOfParts`).
pub(super) enum Titles {
    /// The page group lists the names the model's pages have: nothing changes.
    Same,
    /// The page group is given the model's names: the rest, with the lists changed.
    Changed(Kept),
    /// The lists could not be told apart (a count that does not match the titles, a group that is neither the masters' nor the first): they are left as the file had them.
    Left,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn element_of<'a>(node: &'a RawNode, name: &str) -> Option<&'a RawElement> {
    match node {
        RawNode::Element(e) if local(&e.name) == name => Some(e),
        _ => None,
    }
}

fn text_of(e: &RawElement) -> String {
    e.children
        .iter()
        .filter_map(|n| match n {
            RawNode::Text(t) => Some(&**t),
            _ => None,
        })
        .collect()
}

fn text_element(name: &str, text: &str) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(text))] })
}

/// The count of a group in its variant: its first child that is one (`vt:i4`, or `vt:int`).
fn count_of(variant: &RawElement) -> Option<&RawElement> {
    variant.children.iter().find_map(|n| match n {
        RawNode::Element(e) if matches!(local(&e.name), "i4" | "int") => Some(e),
        _ => None,
    })
}

/// The groups of the first vector of `HeadingPairs`: the count of each, in order, **or `None` when the count of any group is not a number** (a group that is skipped would number the
/// groups after it as if it were not there, and `replace_group` numbers every one).
fn group_counts(heading: &RawElement) -> Option<Vec<usize>> {
    let vector = heading.children.iter().find_map(|n| element_of(n, "vector"))?;
    let mut counts = Vec::new();
    for variant in vector.children.iter().filter_map(|n| element_of(n, "variant")) {
        if let Some(count) = count_of(variant) {
            counts.push(text_of(count).trim().parse().ok()?);
        }
    }
    Some(counts)
}

/// Whether the rest of `docProps/app.xml` holds either of its two lists (`HeadingPairs`, `TitlesOfParts`).
pub(super) fn has_lists(rest: &Kept) -> bool {
    rest.kept.iter().any(|p| element_of(&p.node, "HeadingPairs").is_some() || element_of(&p.node, "TitlesOfParts").is_some())
}

/// The rest of `docProps/app.xml` without its two lists: for **a new document**, whose lists name its own pages and masters and are made from the model (the template's name the template's
/// pages, which are no longer the drawing's), where [`update_page_names`] could not tell which of the template's names are the pages'.
pub(super) fn without_lists(rest: &Kept) -> Kept {
    let mut out = rest.clone();
    out.kept.nodes_mut().retain(|p| element_of(&p.node, "HeadingPairs").is_none() && element_of(&p.node, "TitlesOfParts").is_none());
    out
}

/// The names of the page group of the lists in the rest of `docProps/app.xml`, given the names of the model's pages and masters: **the page group is told by the masters' (the labels are the user's
/// language) and only when that is certain**: with two groups, exactly one is the model's masters (and not its pages as well), and the other is the pages'; with one group,
/// it is the pages' only when the model has no masters. The page group is given `pages` when it lists others, **so the lists change when a page is added, renamed or deleted and not otherwise**
/// (the model holds no earlier names to tell a page that was renamed from a list that never named the pages); a list that cannot be told is left as it is.
pub(super) fn update_page_names(rest: &Kept, pages: &[String], masters: &[String]) -> Titles {
    let nodes: Vec<&RawNode> = rest.kept.iter().map(|p| &p.node).collect();
    let (Some(heading), Some(titles)) =
        (nodes.iter().find_map(|n| element_of(n, "HeadingPairs")), nodes.iter().find_map(|n| element_of(n, "TitlesOfParts")))
    else {
        return Titles::Left;
    };
    let Some(counts) = group_counts(heading) else { return Titles::Left };
    let Some(vector) = titles.children.iter().find_map(|n| element_of(n, "vector")) else { return Titles::Left };
    let names: Vec<String> = vector.children.iter().filter_map(|n| element_of(n, "lpstr")).map(text_of).collect();
    // (A count a hostile part states may be any number: the sum is checked.)
    if counts.iter().try_fold(0usize, |sum, count| sum.checked_add(*count)) != Some(names.len()) {
        return Titles::Left;
    }
    // The groups as ranges of the titles.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for count in &counts {
        let end = start.saturating_add(*count);
        ranges.push((start, end));
        start = end;
    }
    let equals = |range: &(usize, usize), list: &[String]| {
        names.get(range.0..range.1).is_some_and(|got| got.iter().map(String::as_str).eq(list.iter().map(String::as_str)))
    };
    // **Which group lists the pages is told by the masters' group, and only when that is certain** (the labels are the user's language). With two groups, exactly one must be the model's masters
    // and not the model's pages as well (a master named as a page is two lists that cannot be told apart): the other is the pages'. With one group it is the pages' only when the model has no masters
    // (a group that lists both is no group of pages). Anything else is left as the file had it.
    // A group that is the model's pages and no other list of the model's needs no change, whatever the masters are.
    let pages_only: Vec<&(usize, usize)> = ranges.iter().filter(|r| equals(r, pages) && !equals(r, masters)).collect();
    if let [_] = pages_only.as_slice() {
        return Titles::Same;
    }
    let chosen = match ranges.as_slice() {
        [_] => masters.is_empty().then_some(0),
        [first, second] => match (equals(first, masters) && !equals(first, pages), equals(second, masters) && !equals(second, pages)) {
            (true, false) => Some(1),
            (false, true) => Some(0),
            _ => None,
        },
        _ => None,
    };
    let Some(group) = chosen else { return Titles::Left };
    let Some(&(start, end)) = ranges.get(group) else { return Titles::Left };
    if equals(&(start, end), pages) {
        return Titles::Same;
    }
    match replace_group(rest, (group, start, end), pages) {
        Some(changed) => Titles::Changed(changed),
        None => Titles::Left,
    }
}

/// The rest with the titles of the group `(group, start, end)` replaced by `pages`, and the group's count and the vector's size to match: **the first vector of the first `TitlesOfParts`
/// and the first of the first `HeadingPairs`, which are the ones the groups were found in**, and no other. `None` when the lists are not as they were counted (fewer titles than the group's
/// range ends at, a range that is empty, no group of that number): they are then left as they are.
fn replace_group(rest: &Kept, (group, start, end): (usize, usize, usize), pages: &[String]) -> Option<Kept> {
    let mut out = rest.clone();
    let nodes = out.kept.nodes_mut();
    let titles = nodes.iter_mut().find_map(|p| match &mut p.node {
        RawNode::Element(e) if local(&e.name) == "TitlesOfParts" => Some(e),
        _ => None,
    })?;
    rewrite_titles(titles, (start, end), pages)?;
    let heading = nodes.iter_mut().find_map(|p| match &mut p.node {
        RawNode::Element(e) if local(&e.name) == "HeadingPairs" => Some(e),
        _ => None,
    })?;
    rewrite_count(heading, group, pages.len())?;
    Some(out)
}

/// The first vector of `TitlesOfParts` with the titles `start..end` replaced by `pages` and its `size` made the number of titles. `None`, and the vector as it was, when it does not hold
/// the titles the range ends at.
fn rewrite_titles(titles: &mut RawElement, (start, end): (usize, usize), pages: &[String]) -> Option<()> {
    let vector = titles.children.iter_mut().find_map(|n| match n {
        RawNode::Element(v) if local(&v.name) == "vector" => Some(v),
        _ => None,
    })?;
    let seen = vector.children.iter().filter(|n| element_of(n, "lpstr").is_some()).count();
    let removed = end.checked_sub(start).filter(|removed| *removed > 0)?;
    if end > seen {
        return None;
    }
    let total = seen.checked_sub(removed)?.checked_add(pages.len())?;
    let prefix = vector
        .children
        .iter()
        .find_map(|n| element_of(n, "lpstr"))
        .and_then(|e| e.name.split_once(':').map(|(p, _)| p.to_owned()))
        .unwrap_or_else(|| "vt".to_owned());
    let (mut at, mut inserted) = (0usize, false);
    let mut children: Vec<RawNode> = Vec::with_capacity(vector.children.len().saturating_add(pages.len()));
    for node in std::mem::take(&mut vector.children) {
        if element_of(&node, "lpstr").is_none() {
            children.push(node);
            continue;
        }
        if at >= start && at < end {
            if !inserted {
                inserted = true;
                children.extend(pages.iter().map(|p| text_element(&format!("{prefix}:lpstr"), p)));
            }
        } else {
            children.push(node);
        }
        at += 1;
    }
    vector.children = children;
    let mut attrs: Vec<(String, String)> = vector.attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    match attrs.iter_mut().find(|(k, _)| k == "size") {
        Some(size) => size.1 = total.to_string(),
        None => attrs.push(("size".to_owned(), total.to_string())),
    }
    vector.attrs = RawAttrs::from_pairs(attrs);
    Some(())
}

/// The count of the group `group` in the first vector of `HeadingPairs`, made `count`. `None` when there is no such group.
fn rewrite_count(heading: &mut RawElement, group: usize, count: usize) -> Option<()> {
    let vector = heading.children.iter_mut().find_map(|n| match n {
        RawNode::Element(v) if local(&v.name) == "vector" => Some(v),
        _ => None,
    })?;
    let mut number = 0usize;
    for variant in vector.children.iter_mut().filter_map(|n| match n {
        RawNode::Element(v) if local(&v.name) == "variant" => Some(v),
        _ => None,
    }) {
        let Some(slot) = variant.children.iter_mut().find_map(|n| match n {
            RawNode::Element(c) if matches!(local(&c.name), "i4" | "int") => Some(c),
            _ => None,
        }) else {
            continue;
        };
        if number == group {
            slot.children = vec![RawNode::Text(Arc::from(count.to_string().as_str()))];
            return Some(());
        }
        number += 1;
    }
    None
}

// ------------------------------------------------------------------------------------------------------------------ editing

fn set_text(el: &mut El, text: &str) -> bool {
    if el.text() == text && el.children.iter().all(|n| matches!(n, Node::Text(_) | Node::CData(_))) {
        return false;
    }
    el.children = if text.is_empty() { vec![] } else { vec![Node::Text(text.to_owned())] };
    true
}

/// The first child of `el` whose local name is `local`.
fn child_mut<'a>(el: &'a mut El, local: &str) -> Option<&'a mut El> {
    el.children.iter_mut().find_map(|n| match n {
        Node::El(e) if e.is(local) => Some(e),
        _ => None,
    })
}

/// Sets, adds or takes out the element `local` of `root`, which is written `qualified` when it is made: the change of a field of the properties from `old` to `new`. Nothing changes when
/// the field is as it was (an element of the file that the field does not say stays as it is).
fn patch_field(root: &mut El, local: &str, qualified: &str, old: Option<&str>, new: Option<&str>) -> bool {
    if old == new {
        return false;
    }
    match (root.children.iter().position(|n| matches!(n, Node::El(e) if e.is(local))), new) {
        (Some(at), Some(text)) => match root.children.get_mut(at) {
            Some(Node::El(e)) => set_text(e, text),
            _ => false,
        },
        (Some(at), None) => {
            root.children.remove(at);
            true
        }
        (None, Some(text)) => {
            let decls: Vec<(&str, &str)> = match qualified.split_once(':').map(|(prefix, _)| prefix) {
                Some("dc") => vec![("xmlns:dc", NS_DC)],
                Some("cp") => vec![("xmlns:cp", NS_CORE)],
                _ => vec![],
            };
            root.children.push(leaf(qualified, decls, text));
            true
        }
        (None, None) => false,
    }
}

/// The fields of the properties that the file's core part says, changed from `old` to `new`.
pub(super) fn patch_core_fields(tree: &mut Tree, old: &DocProps, new: &DocProps) -> bool {
    let root = &mut tree.root;
    if !root.is("coreProperties") {
        return false;
    }
    let mut changed = false;
    for (local, qualified, before, after) in [
        ("subject", "dc:subject", &old.subject, &new.subject),
        ("creator", "dc:creator", &old.creator, &new.creator),
        ("keywords", "cp:keywords", &old.keywords, &new.keywords),
        ("description", "dc:description", &old.description, &new.description),
        ("lastModifiedBy", "cp:lastModifiedBy", &old.last_modified_by, &new.last_modified_by),
        ("lastPrinted", "cp:lastPrinted", &old.last_printed, &new.last_printed),
        ("category", "cp:category", &old.category, &new.category),
        ("language", "dc:language", &old.language, &new.language),
    ] {
        changed |= patch_field(root, local, qualified, before.as_deref(), after.as_deref());
    }
    changed
}

/// The fields of the properties that the file's extended part says, changed from `old` to `new`.
pub(super) fn patch_app_fields(tree: &mut Tree, old: &DocProps, new: &DocProps) -> bool {
    let root = &mut tree.root;
    if !root.is("Properties") {
        return false;
    }
    let mut changed = false;
    for (local, before, after) in [
        ("Manager", &old.manager, &new.manager),
        ("Company", &old.company, &new.company),
        ("HyperlinkBase", &old.hyperlink_base, &new.hyperlink_base),
        ("AppVersion", &old.app_version, &new.app_version),
    ] {
        changed |= patch_field(root, local, local, before.as_deref(), after.as_deref());
    }
    changed
}

/// Edits `docProps/app.xml`: `Application` is `DiaCraft`, and when the pages changed their names replace the old ones in the group that holds
/// them. `None` when nothing changes. A warning for a list that could not be found.
pub(super) fn patch_app(tree: &mut Tree, old_pages: &[String], new_pages: &[String], warnings: &mut Vec<String>) -> bool {
    let mut changed = false;
    let root = &mut tree.root;
    if !root.is("Properties") {
        return false;
    }
    match child_mut(root, "Application") {
        Some(app) => changed |= set_text(app, APPLICATION),
        None => {
            let mut made = child_el(leaf("Application", vec![], APPLICATION));
            super::build::with_prefix(&mut made, root.prefix().to_owned().as_str());
            root.children.insert(0, Node::El(made));
            changed = true;
        }
    }
    if old_pages != new_pages {
        match replace_page_group(root, old_pages, new_pages) {
            Some(()) => changed = true,
            None => warnings.push("the list of page names in docProps/app.xml was not found, and is left as it was".to_owned()),
        }
    }
    changed
}

/// Replaces the names of the pages in the group of `TitlesOfParts` that holds exactly `old`, and its count in `HeadingPairs`. The group is found
/// by its names and its count, never by the label, which is in the user's language.
fn replace_page_group(root: &mut El, old: &[String], new: &[String]) -> Option<()> {
    // The counts of the groups, in order, and which variant of HeadingPairs holds each.
    let counts: Vec<usize> = {
        let heading = root.child("HeadingPairs")?.child("vector")?;
        let numbers: Vec<usize> = heading
            .children_named("variant")
            .filter_map(|v| v.child("i4").or_else(|| v.child("int")).and_then(|n| n.text().trim().parse().ok()))
            .collect();
        numbers
    };
    let titles: Vec<String> = root.child("TitlesOfParts")?.child("vector")?.children_named("lpstr").map(El::text).collect();
    let mut start = 0usize;
    let mut group = None;
    for (g, count) in counts.iter().enumerate() {
        let end = start.checked_add(*count)?;
        if *count == old.len() && titles.get(start..end)? == old {
            group = Some((g, start, end));
            break;
        }
        start = end;
    }
    let (g, start, end) = group?;
    // TitlesOfParts: the names of the group are replaced.
    let vector = child_mut(child_mut(root, "TitlesOfParts")?, "vector")?;
    let mut seen = 0usize;
    let mut out: Vec<Node> = Vec::with_capacity(vector.children.len() + new.len());
    let mut inserted = false;
    for node in std::mem::take(&mut vector.children) {
        let is_title = matches!(&node, Node::El(e) if e.is("lpstr"));
        if is_title {
            if seen >= start && seen < end {
                if !inserted {
                    inserted = true;
                    out.extend(new.iter().map(|n| leaf("vt:lpstr", vec![], n)));
                }
            } else {
                out.push(node);
            }
            seen += 1;
        } else {
            out.push(node);
        }
    }
    if !inserted {
        // An empty group (count 0) cannot be told from another: nothing was matched above, so this does not happen; keep the guard.
        return None;
    }
    let total = titles.len() - (end - start) + new.len();
    vector.children = out;
    super::patch::set_attr(vector, "size", &total.to_string());
    // HeadingPairs: the count of the group.
    let heading = child_mut(child_mut(root, "HeadingPairs")?, "vector")?;
    let mut number = 0usize;
    for node in &mut heading.children {
        if let Node::El(v) = node
            && v.is("variant")
            && (v.child("i4").is_some() || v.child("int").is_some())
        {
            if number == g
                && let Some(slot) = child_mut(v, "i4")
            {
                set_text(slot, &new.len().to_string());
            }
            number += 1;
        }
    }
    Some(())
}

/// Sets `title`, `created` and `modified` of `docProps/core.xml`. Elements that are not there are made, with the namespaces they need declared on
/// them.
pub(super) fn patch_core(tree: &mut Tree, title: Option<&str>, created: Option<&str>, modified: Option<&str>) -> bool {
    let root = &mut tree.root;
    if !root.is("coreProperties") {
        return false;
    }
    let mut changed = false;
    if let Some(title) = title {
        match child_mut(root, "title") {
            Some(e) => changed |= set_text(e, title),
            None => {
                root.children.push(leaf("dc:title", vec![("xmlns:dc", NS_DC)], title));
                changed = true;
            }
        }
    }
    for (local, qualified, time) in [("created", "dcterms:created", created), ("modified", "dcterms:modified", modified)] {
        let Some(time) = time else { continue };
        match child_mut(root, local) {
            Some(e) => changed |= set_text(e, time),
            None => {
                let made = leaf(qualified, vec![("xmlns:dcterms", NS_TERMS), ("xmlns:xsi", NS_XSI), ("xsi:type", "dcterms:W3CDTF")], time);
                root.children.push(made);
                changed = true;
            }
        }
    }
    changed
}

/// Makes sure `docProps/custom.xml` holds `RecalcDocument` as true, once: found by name first; a new one takes a `pid` above the
/// largest there, and at least 2.
pub(super) fn ensure_recalc(tree: &mut Tree) -> bool {
    let root = &mut tree.root;
    if !root.is("Properties") {
        return false;
    }
    let vt = root.attrs.iter().find(|(k, v)| k.starts_with("xmlns:") && v == NS_VT).map(|(k, _)| k.trim_start_matches("xmlns:").to_owned());
    let bool_name = format!("{}:bool", vt.as_deref().unwrap_or("vt"));
    if let Some(existing) = root.children.iter_mut().find_map(|n| match n {
        Node::El(e) if e.is("property") && e.attr("name") == Some("RecalcDocument") => Some(e),
        _ => None,
    }) {
        return match child_mut(existing, "bool") {
            Some(b) => set_text(b, "true"),
            None => {
                let mut made = child_el(leaf(&bool_name, vec![], "true"));
                if vt.is_none() {
                    made.attrs.push(("xmlns:vt".to_owned(), NS_VT.to_owned()));
                }
                existing.children = vec![Node::El(made)];
                true
            }
        };
    }
    let pid = root.children_named("property").filter_map(|p| p.attr("pid")?.parse::<u32>().ok()).max().map_or(2, |m| m.saturating_add(1).max(2));
    let mut made = child_el(property(pid, "RecalcDocument", leaf(&bool_name, vec![], "true")));
    if vt.is_none() {
        made.attrs.push(("xmlns:vt".to_owned(), NS_VT.to_owned()));
    }
    root.children.push(Node::El(made));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
        RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
    }

    fn node(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
        RawNode::Element(el(name, attrs, children))
    }

    fn titles(prefix: &str, names: &[&str]) -> RawElement {
        let items = names.iter().map(|n| node(&format!("{prefix}:lpstr"), &[], vec![RawNode::Text(Arc::from(*n))])).collect();
        el("TitlesOfParts", &[], vec![node(&format!("{prefix}:vector"), &[("size", &names.len().to_string()), ("baseType", "lpstr")], items)])
    }

    fn texts(titles: &RawElement) -> Vec<String> {
        let Some(RawNode::Element(vector)) = titles.children.first() else { return Vec::new() };
        vector.children.iter().filter_map(|n| element_of(n, "lpstr")).map(text_of).collect()
    }

    fn size(titles: &RawElement) -> Option<String> {
        let Some(RawNode::Element(vector)) = titles.children.first() else { return None };
        vector.attrs.get("size").map(str::to_owned)
    }

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn the_titles_of_a_range_are_replaced_and_the_size_follows() {
        let mut t = titles("vt", &["A", "B", "Box"]);
        assert!(rewrite_titles(&mut t, (0, 2), &strings(&["X", "Y", "Z"])).is_some());
        assert_eq!(texts(&t), ["X", "Y", "Z", "Box"]);
        assert_eq!(size(&t).as_deref(), Some("4"));
        // A range that ends where the titles do is a range that is there (the last title).
        let mut t = titles("vt", &["A", "Box"]);
        assert!(rewrite_titles(&mut t, (1, 2), &strings(&["M"])).is_some());
        assert_eq!(texts(&t), ["A", "M"]);
        // The new titles are written with the prefix the old ones had.
        let mut t = titles("q", &["A"]);
        assert!(rewrite_titles(&mut t, (0, 1), &strings(&["B"])).is_some());
        let Some(RawNode::Element(vector)) = t.children.first() else { panic!() };
        assert!(matches!(vector.children.first(), Some(RawNode::Element(e)) if &*e.name == "q:lpstr"));
    }

    #[test]
    fn a_range_that_is_empty_or_past_the_titles_leaves_them_as_they_were() {
        for (range, why) in [((1, 1), "empty"), ((2, 1), "backwards"), ((0, 3), "past the end"), ((3, 4), "all past the end")] {
            let mut t = titles("vt", &["A", "B"]);
            assert!(rewrite_titles(&mut t, range, &strings(&["X"])).is_none(), "{why}");
            assert_eq!(texts(&t), ["A", "B"], "{why}");
            assert_eq!(size(&t).as_deref(), Some("2"), "{why}");
        }
        // No vector at all.
        let mut none = el("TitlesOfParts", &[], vec![]);
        assert!(rewrite_titles(&mut none, (0, 1), &strings(&["X"])).is_none());
        // The size is made when the vector had none.
        let mut t = titles("vt", &["A"]);
        if let Some(RawNode::Element(vector)) = t.children.first_mut() {
            vector.attrs = RawAttrs::new();
        }
        assert!(rewrite_titles(&mut t, (0, 1), &strings(&["X", "Y"])).is_some());
        assert_eq!(size(&t).as_deref(), Some("2"));
    }

    fn heading(counts: &[(&str, &str)]) -> RawElement {
        let mut items = Vec::new();
        for (label, count) in counts {
            items.push(node("vt:variant", &[], vec![node("vt:lpstr", &[], vec![RawNode::Text(Arc::from(*label))])]));
            let kind = if count.starts_with('~') { "vt:int" } else { "vt:i4" };
            items.push(node("vt:variant", &[], vec![node(kind, &[], vec![RawNode::Text(Arc::from(count.trim_start_matches('~')))])]));
        }
        el("HeadingPairs", &[], vec![node("vt:vector", &[("size", &items.len().to_string()), ("baseType", "variant")], items)])
    }

    fn counted(heading: &RawElement) -> Vec<String> {
        let Some(RawNode::Element(vector)) = heading.children.first() else { return Vec::new() };
        vector
            .children
            .iter()
            .filter_map(|n| element_of(n, "variant"))
            .filter_map(|v| v.children.iter().find_map(|c| element_of(c, "i4").or_else(|| element_of(c, "int"))))
            .map(text_of)
            .collect()
    }

    #[test]
    fn the_count_of_the_numbered_group_is_made_the_count_and_a_group_that_is_not_there_changes_nothing() {
        let mut h = heading(&[("Pages", "2"), ("Masters", "~3")]);
        assert!(rewrite_count(&mut h, 0, 5).is_some());
        assert_eq!(counted(&h), ["5", "3"]);
        // The second group is numbered one, and its count is an `int`.
        assert!(rewrite_count(&mut h, 1, 7).is_some());
        assert_eq!(counted(&h), ["5", "7"]);
        // A group that is not there.
        assert!(rewrite_count(&mut h, 2, 9).is_none());
        assert_eq!(counted(&h), ["5", "7"]);
        // No vector.
        assert!(rewrite_count(&mut el("HeadingPairs", &[], vec![]), 0, 1).is_none());
    }

    #[test]
    fn a_variant_that_holds_no_count_is_not_a_group() {
        // The labels are variants of a string and the counts variants of a number: only the numbered ones are the groups.
        let mut h = heading(&[("Pages", "2"), ("Masters", "3")]);
        assert!(rewrite_count(&mut h, 1, 4).is_some(), "the second group, whatever labels come between");
        assert_eq!(counted(&h), ["2", "4"]);
    }

    fn rest(heading: RawElement, titles: RawElement) -> Kept {
        let mut kept = Kept::default();
        kept.kept.push(0, RawNode::Element(heading));
        kept.kept.push(0, RawNode::Element(titles));
        kept
    }

    fn changed_titles(titles: Titles) -> Option<Vec<String>> {
        match titles {
            Titles::Changed(kept) => kept.kept.iter().find_map(|p| element_of(&p.node, "TitlesOfParts").map(texts)),
            _ => None,
        }
    }

    #[test]
    fn lists_that_do_not_agree_with_themselves_are_left_and_lists_that_agree_are_read() {
        let pages = strings(&["A"]);
        let masters = strings(&["Box"]);
        // The counts say three titles and there are two.
        let kept = rest(heading(&[("Pages", "2"), ("Masters", "1")]), titles("vt", &["A", "Box"]));
        assert!(matches!(update_page_names(&kept, &strings(&["B"]), &masters), Titles::Left));
        // No `TitlesOfParts`, no `HeadingPairs`.
        let mut only_titles = Kept::default();
        only_titles.kept.push(0, RawNode::Element(titles("vt", &["A"])));
        assert!(matches!(update_page_names(&only_titles, &pages, &masters), Titles::Left));
        let mut only_heading = Kept::default();
        only_heading.kept.push(0, RawNode::Element(heading(&[("Pages", "1")])));
        assert!(matches!(update_page_names(&only_heading, &pages, &masters), Titles::Left));
        // A heading with no vector; titles with no vector.
        let kept = rest(el("HeadingPairs", &[], vec![]), titles("vt", &["A"]));
        assert!(matches!(update_page_names(&kept, &pages, &[]), Titles::Left));
        let kept = rest(heading(&[("Pages", "1")]), el("TitlesOfParts", &[], vec![]));
        assert!(matches!(update_page_names(&kept, &pages, &[]), Titles::Left));
        // The lists agree, and the pages are the model's: nothing changes; they are not: the pages' group follows.
        let kept = rest(heading(&[("Pages", "1"), ("Masters", "1")]), titles("vt", &["A", "Box"]));
        assert!(matches!(update_page_names(&kept, &pages, &masters), Titles::Same));
        assert_eq!(changed_titles(update_page_names(&kept, &strings(&["B", "C"]), &masters)), Some(strings(&["B", "C", "Box"])));
        // Three groups cannot be told: they are left.
        let kept = rest(heading(&[("A", "1"), ("B", "1"), ("C", "1")]), titles("vt", &["A", "Box", "Z"]));
        assert!(matches!(update_page_names(&kept, &strings(&["B"]), &masters), Titles::Left));
        // One group of the pages, in a model with no masters.
        let kept = rest(heading(&[("Pages", "1")]), titles("vt", &["A"]));
        assert_eq!(changed_titles(update_page_names(&kept, &strings(&["B"]), &[])), Some(strings(&["B"])));
        assert!(
            matches!(update_page_names(&kept, &strings(&["B"]), &masters), Titles::Left),
            "a model with masters does not say the one group is the pages'"
        );
    }

    /// Chrono to jiff: the text of a time at the ends of the calendar, pinned before the change (what chrono wrote) and unchanged by it. A year before 1 is a minus
    /// sign and four digits (`-0001`), the way W3CDTF and XML Schema write one.
    #[test]
    fn a_time_is_w3cdtf_text_at_the_ends_of_the_calendar_as_it_was() {
        for (seconds, want) in [
            (0, "1970-01-01T00:00:00Z"),
            (1_700_000_000, "2023-11-14T22:13:20Z"),
            (-1, "1969-12-31T23:59:59Z"),
            (253_402_207_200, "9999-12-30T22:00:00Z"),
            (253_402_207_201, "9999-12-30T22:00:01Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
            (-62_167_219_200, "0000-01-01T00:00:00Z"),
            (-62_167_219_201, "-0001-12-31T23:59:59Z"),
            (-62_135_596_800, "0001-01-01T00:00:00Z"),
            (-377_705_116_800, "-9999-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
        ] {
            assert_eq!(format_time(seconds).as_deref(), Some(want), "{seconds}");
        }
    }

    /// A time past the years a W3CDTF text can hold has no text and no panic. chrono wrote `+10000-01-01T00:00:00Z` for the first of them, which is no W3CDTF; jiff holds -9999 to
    /// 9999, and the writer leaves the property out.
    #[test]
    fn a_time_past_the_years_the_text_can_hold_has_none() {
        for seconds in [253_402_300_800, -377_705_116_801, 265_990_566_232, -8_334_601_228_800, i64::MAX, i64::MIN] {
            assert_eq!(format_time(seconds), None, "{seconds}");
        }
    }

    #[test]
    fn a_count_that_does_not_fit_a_number_is_no_count() {
        // Counts that add up past what a count holds are not a sum.
        let huge = usize::MAX.to_string();
        let kept = rest(heading(&[("Pages", &huge), ("Masters", "2")]), titles("vt", &["A"]));
        assert!(matches!(update_page_names(&kept, &strings(&["A"]), &[]), Titles::Left));
    }
}
