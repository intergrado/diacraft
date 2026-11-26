//! **A style sheet's markup of an earlier ODF version, in the form ODF 1.4 takes** (R3 of the full gate on `9d0daec0`).
//!
//! The model writer labels every part it writes 1.4 (a package that states two versions is one LibreOffice refuses to load), and it writes the style sheets an
//! `.odg` reader made natively, with what their properties hold that no cell reads and the children they kept. A file labelled 1.0 or 1.1 may hold there what those versions' schemas
//! take (they accept any attribute and child of a properties element) and the 1.4 schema refuses. Each such form is written in its 1.4 form, and **the source's form rides in DiaCraft's
//! document file** (the style's entry, `older` children: `ext::docfile`), so that a DiaCraft read gives the style back as it was read:
//!
//! * `text:enable-numbering` on a style's paragraph properties (no attribute of 1.4) is left out;
//! * a `text:list-style` with no `style:name` (which 1.4 asks for) is given one, `DiaCraft_20_list_20_<n>`, the least `n` from 1 that no list style of the style holds;
//! * a negative `text:min-label-width` on a list level's properties (a non-negative length in 1.4) is written as 0 in the same unit.
//!
//! **Two forms ride in an attribute of DiaCraft's own on the same properties element**, the way `text:enable-numbering` of an automatic style a
//! paragraph names does (`write/text/model.rs`), because a style that holds them is not one the document file carries an entry for (a style no shape or paragraph names is kept whole, and
//! written whole):
//!
//! * **`text:enable-numbering` on the paragraph properties of a style kept whole** moves to `diacraft:older-enable-numbering` ([`carrier_on_properties`]), and any other attribute of the standard's
//!   namespaces that the schemas have no place for on the properties is left out and counted ([`refused_on_properties`]);
//! * **a weight, a slant or a variant that 1.4 lists no such value for** (`fo:font-weight="250"`: 1.4 takes `normal`, `bold` and 100 to 900 in steps of 100; the complex script's two, the slant
//!   and the small caps are the same kind) is written as the value 1.4 takes nearest ([`form_14`]) and the value it had rides in `diacraft:older-<attribute>` ([`listed`]). A read gives the
//!   value back **only while the attribute beside it is still the one the writer would have written for it**, so a weight another program changed is the program's.
//!
//! (A zero `fo:font-size` is a cell's, `Size`: the writer writes the least size it takes, `write::runs::LEAST_FONT_SIZE`, and the cells ride.)
//!
//! **What it costs** (the security scan of round 2, three findings). The writer works out each style's form once a save, before it writes ([`prepare`]): a walk of the style's kept children
//! that finds what is to be done, and a walk that makes them anew when something is, each charged to the save's steps before it is made, with a set of the list styles' names that each name
//! given is looked up in. A style of more than [`MOST_RECORDS`] records (list styles named, widths brought to 0, properties left out) is refused in words. The reader takes an item for each
//! record before it keeps it, keeps at most [`MOST_RECORDS`] for a style, keeps only a property that ODF 1.4 refuses and the first record of a key ([`Index`]), and puts the source's form back
//! by maps in one walk that it charges before it makes it, and makes none when no list style of the style is one a record names. The walks go one call a level deep, as the reader's trees of
//! kept markup are written (`passthrough::to_tree`): the reader bounds their depth.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, OdfStyle, Passthrough, Placed, RawAttrs, RawElement, RawNode};

use crate::write::{WriteError, WriteWork};

/// The unmapped properties ODF 1.4 has no place for, by the local name of the properties element and the attribute's qualified name as the reader keys them (`<element>/<attribute>`).
const NOT_IN_1_4: [(&str, &str); 1] = [("paragraph-properties", "text:enable-numbering")];

/// The attributes of the standard's own namespaces, other than `fo` and `style`, that the schemas of ODF 1.2 and 1.4 give the properties elements of a text or a paragraph style
/// (`style:text-properties` and `style:paragraph-properties`): by the element's local name, the namespace and the attribute's local name. Those two namespaces are written as they always
/// were; **every other attribute of a namespace of the standard is one the schemas give no place there**, and is not written (see [`refused_on_properties`]).
const TAKEN_ON_PROPERTIES: [(&str, &str, &str); 5] = [
    ("paragraph-properties", TEXT_URI, "line-number"),
    ("paragraph-properties", TEXT_URI, "number-lines"),
    ("paragraph-properties", TEXT_URI, "relative-tab-stop-position"),
    ("text-properties", TEXT_URI, "condition"),
    ("text-properties", TEXT_URI, "display"),
];

/// The namespace of the standard's text vocabulary.
const TEXT_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";

/// The attributes of an earlier version that no later version has a place for on a properties element of an **automatic** text or paragraph style, and the attribute of DiaCraft's own that
/// each rides in on the same element, so that a read of the file gives it back: the element's local name, the namespace and local name of the attribute, the DiaCraft attribute's local
/// name and the qualified name the read gives back (`text` is the prefix the standard's text namespace has in every file this writer makes). (A common style's are the document file's:
/// [`NOT_IN_1_4`].)
const CARRIED_ON_PROPERTIES: [(&str, &str, &str, &str, &str); 1] =
    [("paragraph-properties", TEXT_URI, "enable-numbering", "older-enable-numbering", "text:enable-numbering")];

/// The namespace of the standard's `fo` vocabulary, and the pre-standard W3C one the reader takes for it.
const FO_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const FO_URI_W3C: &str = "http://www.w3.org/1999/XSL/Format";

/// The namespace of the standard's `style` vocabulary.
const STYLE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";

/// Which of the two vocabularies a style's text properties take an attribute of the group `Style` from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Space {
    Fo,
    Style,
}

/// The vocabulary a namespace URI is, for [`listed`]: `fo` (the standard's and the pre-standard W3C one) and `style`.
pub(crate) fn space_of(uri: &str) -> Option<Space> {
    match uri {
        FO_URI | FO_URI_W3C => Some(Space::Fo),
        STYLE_URI => Some(Space::Style),
        _ => None,
    }
}

/// What a list of values ODF 1.4 gives an attribute of the group `Style` of a text's properties is a list of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listed {
    /// `normal`, `bold` and 100 to 900 in steps of 100 (the schema's `fontWeight`).
    Weight,
    /// `normal`, `italic` and `oblique` (`fontStyle`).
    Slant,
    /// `normal` and `small-caps` (`fontVariant`).
    Variant,
}

/// The five attributes of the group `Style` that a style's text properties may spell in a form 1.4 lists no value for (the schemas of 1.0 and 1.1 take any text there): the vocabulary, the
/// local name, what 1.4 lists, and the local name of the attribute of DiaCraft's own that the value it had rides in on the same element.
const LISTED: [(Space, &str, Listed, &str); 5] = [
    (Space::Fo, "font-weight", Listed::Weight, "older-font-weight"),
    (Space::Fo, "font-style", Listed::Slant, "older-font-style"),
    (Space::Fo, "font-variant", Listed::Variant, "older-font-variant"),
    (Space::Style, "font-weight-complex", Listed::Weight, "older-font-weight-complex"),
    (Space::Style, "font-style-complex", Listed::Slant, "older-font-style-complex"),
];

/// What the attribute `local` of `space` on a style's **text properties** lists, and the local name of the attribute of DiaCraft's own its exact value rides in: `None` for any other attribute.
pub(crate) fn listed(space: Space, local: &str) -> Option<(Listed, &'static str)> {
    LISTED.iter().find(|(s, l, _, _)| *s == space && *l == local).map(|(_, _, kind, carrier)| (*kind, *carrier))
}

/// Whether `local` is the local name of a carrier of [`listed`]: an attribute of DiaCraft's own that stands for the value of another attribute and is no attribute of the style.
pub(crate) fn is_value_carrier(local: &str) -> bool {
    LISTED.iter().any(|(_, _, _, carrier)| *carrier == local)
}

/// Whether `local` is the local name of a carrier of [`CARRIED_ON_PROPERTIES`]: an attribute of DiaCraft's own that stands in the place of an attribute the schemas have no place for.
pub(crate) fn is_move_carrier(local: &str) -> bool {
    CARRIED_ON_PROPERTIES.iter().any(|(_, _, _, carrier, _)| *carrier == local)
}

/// The weights ODF 1.4 lists, lightest first, and their values as numbers.
const WEIGHTS: [(&str, f64); 9] =
    [("100", 100.0), ("200", 200.0), ("300", 300.0), ("400", 400.0), ("500", 500.0), ("600", 600.0), ("700", 700.0), ("800", 800.0), ("900", 900.0)];

/// The first weight at which reads a weight as bold (`bold` or 600 to 900), and so the first of [`WEIGHTS`] that is on the bold side.
const BOLD_FROM: f64 = 600.0;

/// A text as a finite number, when it is one (a decimal, with a sign or an exponent if it has them).
fn finite(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The weight of [`WEIGHTS`] nearest to `n`, **on the side of 600 that `n` is on** (R22 reads 100 to 500 as not bold and 600 to 900 as bold, and a weight that is not bold stays so): a tie goes to
/// the lighter weight, so that 250 is 200.
fn nearest_weight(n: f64) -> &'static str {
    let (from, to) = if n < BOLD_FROM { (0, 5) } else { (5, WEIGHTS.len()) };
    let mut best: Option<(&'static str, f64)> = None;
    for (text, weight) in WEIGHTS.iter().skip(from).take(to.saturating_sub(from)) {
        let apart = (n - weight).abs();
        // Strictly nearer: the lighter weight keeps a tie.
        if best.is_none_or(|(_, so_far)| apart < so_far) {
            best = Some((text, apart));
        }
    }
    best.map_or("normal", |(text, _)| text)
}

/// The word of `words` that `text` is, when it differs from it by nothing but white space and letter case.
fn listed_word(text: &str, words: &[&'static str]) -> Option<&'static str> {
    let text = text.trim();
    words.iter().copied().find(|word| text.eq_ignore_ascii_case(word))
}

/// **The value ODF 1.4 lists nearest to `value`** of an attribute of `kind`, or `None` when 1.4 takes `value` as it is. A weight is the nearest of 100 to 900 on the side of 600 it is on (a tie to the
/// lighter: 250 is 200, 650 is 600), `bolder` is `bold` and `lighter` `normal`, and any other text is `normal`; a slant or a variant is the word it differs from by white space or letter case, and
/// any other text is `normal` (the value the cells of a style write when the group has no bit of it).
pub(crate) fn form_14(kind: Listed, value: &str) -> Option<&'static str> {
    const SLANTS: [&str; 3] = ["normal", "italic", "oblique"];
    const VARIANTS: [&str; 2] = ["normal", "small-caps"];
    let taken: &[&str] = match kind {
        Listed::Weight => &["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"],
        Listed::Slant => &SLANTS,
        Listed::Variant => &VARIANTS,
    };
    if taken.contains(&value) {
        return None;
    }
    Some(match kind {
        Listed::Weight => match value.trim().to_ascii_lowercase().as_str() {
            "bold" | "bolder" => "bold",
            "normal" | "lighter" => "normal",
            other => finite(other).map_or("normal", nearest_weight),
        },
        Listed::Slant => listed_word(value, &SLANTS).unwrap_or("normal"),
        Listed::Variant => listed_word(value, &VARIANTS).unwrap_or("normal"),
    })
}

/// The value a carrier gives back, **when it is still the one the attribute beside it stands for**: the value the writer wrote for `carried` is `written`, which is the value of the attribute
/// now. `None` for a carrier another program has outdated (it changed the attribute and left the carrier), and for a value 1.4 takes as it is (no carrier is written for one).
pub(crate) fn carried_still<'a>(kind: Listed, carried: &'a str, written: &str) -> Option<&'a str> {
    (form_14(kind, carried) == Some(written)).then_some(carried)
}

/// Whether `uri` is a namespace of the standard's own (ODF 1.4 Part 3's, and the W3C's and Dublin Core's that it takes names of): a schema of ODF says what an attribute of it may be where it
/// stands. Every other namespace is a producer's own, foreign markup that a schema of ODF takes anywhere.
fn is_standards_namespace(uri: &str) -> bool {
    uri.starts_with("urn:oasis:names:tc:opendocument:xmlns:")
        || matches!(
            uri,
            "http://www.w3.org/1999/XSL/Format"
                | "http://www.w3.org/2000/svg"
                | "http://www.w3.org/1999/xlink"
                | "http://purl.org/dc/elements/1.1/"
                | "http://www.w3.org/1998/Math/MathML"
        )
}

/// **Whether an attribute of a namespace of the standard's other than `fo` and `style` is one the schemas of ODF 1.2 and 1.4 give no place on the properties element `holder` (its local
/// name) of an automatic text or paragraph style**: the schemas of 1.0 and 1.1 take any attribute there, and a file of those versions holds what no later version has (`text:enable-numbering`,
/// or an attribute of a vocabulary that belongs on a graphic's properties). A save that writes a file of 1.2 or later writes none of it as it stands. An attribute of another namespace is
/// foreign markup, and no case of this.
pub(crate) fn refused_on_properties(holder: &str, uri: &str, local: &str) -> bool {
    is_standards_namespace(uri) && !TAKEN_ON_PROPERTIES.iter().any(|(h, u, l)| *h == holder && *u == uri && *l == local)
}

/// The attribute of DiaCraft's own (its local name) that carries the attribute `local` of the namespace `uri` that [`refused_on_properties`] refuses on `holder`, when there is one: the
/// source's form rides there (the style sheets' ride in the document file).
pub(crate) fn carrier_on_properties(holder: &str, uri: &str, local: &str) -> Option<&'static str> {
    CARRIED_ON_PROPERTIES.iter().find(|(h, u, l, _, _)| *h == holder && *u == uri && *l == local).map(|(_, _, _, carrier, _)| *carrier)
}

/// The attribute that the DiaCraft attribute `carrier` on the properties element `holder` carries, as its qualified name and the namespace that name is read in: the form the file
/// was in, which a read gives back.
pub(crate) fn carried_on_properties(holder: &str, carrier: &str) -> Option<(&'static str, &'static str)> {
    CARRIED_ON_PROPERTIES.iter().find(|(h, _, _, c, _)| *h == holder && *c == carrier).map(|(_, uri, _, _, qname)| (*qname, *uri))
}

/// The most records of one style sheet: list styles named, label widths brought to 0 and properties left out. The writer refuses a style of more; the reader keeps the first this many.
pub(crate) const MOST_RECORDS: usize = 4_096;

/// The words of the refusal of a style of more than [`MOST_RECORDS`] records.
const WORDS_TOO_MANY: &str = "a style sheet with more than 4,096 list styles with no name, label widths below 0 or properties of an earlier ODF version (ODF 1.4 takes none of them, and DiaCraft carries each)";

/// The local name of a name as written.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

/// Whether the unmapped property `key` (`<element>/<attribute>`) is one ODF 1.4 refuses.
pub(crate) fn refused_property(key: &str) -> bool {
    key.split_once('/').is_some_and(|(element, attribute)| NOT_IN_1_4.iter().any(|(e, a)| local(element) == *e && attribute == *a))
}

/// What a style's ODF data says in an earlier version's form, and what of it rides so that a read gives it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Older {
    /// The unmapped properties left out, with their values.
    pub dropped: Vec<(String, String)>,
    /// The names given to list styles that had none.
    pub named: Vec<String>,
    /// The label widths brought to 0: (the list style's name as written, the level, the width the source had).
    pub labels: Vec<(String, String, String)>,
}

impl Older {
    /// Whether nothing was in an earlier version's form.
    pub(crate) fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.named.is_empty() && self.labels.is_empty()
    }
}

/// One style's form for a save: the kept children in 1.4's form (`None`: as they are) and the record of what is in another form, beside the style's name (the check that the place the cache
/// holds it at holds the style it was worked out for).
#[derive(Clone, Debug, Default)]
pub(crate) struct Form14 {
    pub name: Option<Arc<str>>,
    pub kept: Option<Passthrough>,
    pub older: Older,
}

/// Whether a length is negative (a leading `-` before a digit or a point).
pub(crate) fn negative(length: &str) -> bool {
    length.trim().strip_prefix('-').is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit() || c == '.'))
}

/// A negative length as 0 in its own unit: `-0.4cm` is `0cm`.
fn zero_of(length: &str) -> String {
    let trimmed = length.trim();
    let unit_at = trimmed.rfind(|c: char| !(c.is_ascii_alphabetic() || c == '%')).map_or(0, |at| at.saturating_add(1));
    format!("0{}", trimmed.get(unit_at..).unwrap_or_default())
}

/// What the first walk of the kept children finds: the nodes, the names the list styles hold, and how many list styles have none and how many widths are below 0.
struct Survey {
    nodes: usize,
    names: HashSet<String>,
    unnamed: usize,
    negative: usize,
}

fn survey(kept: &Passthrough) -> Survey {
    let mut found = Survey { nodes: 0, names: HashSet::new(), unnamed: 0, negative: 0 };
    let mut stack: Vec<&RawNode> = kept.iter().map(|p| &p.node).collect();
    while let Some(node) = stack.pop() {
        found.nodes = found.nodes.saturating_add(1);
        let RawNode::Element(el) = node else { continue };
        match local(&el.name) {
            "list-style" => match el.attrs.get("style:name") {
                Some(name) => {
                    found.names.insert(name.to_owned());
                }
                None => found.unnamed = found.unnamed.saturating_add(1),
            },
            "list-level-properties" if el.attrs.get("text:min-label-width").is_some_and(negative) => {
                found.negative = found.negative.saturating_add(1)
            }
            _ => {}
        }
        stack.extend(el.children.iter());
    }
    found
}

/// The form of one style's ODF data for a save (see the [module documentation](self)): each walk of its kept children is charged to `work` (the first as soon as its size is known, before
/// anything is made of it; the reader bounded the tree), and a style of more than [`MOST_RECORDS`] records is refused in words before the second.
pub(crate) fn of(odf: &OdfStyle, work: &mut WriteWork) -> Result<Form14, WriteError> {
    let mut older = Older::default();
    work.charge_steps(odf.unmapped.len())?;
    older.dropped = odf.unmapped.iter().filter(|(k, _)| refused_property(k)).map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    let mut found = survey(&odf.kept.kept);
    work.charge_steps(found.nodes)?;
    if older.dropped.len().saturating_add(found.unnamed).saturating_add(found.negative) > MOST_RECORDS {
        return Err(WriteError::Unsupported(WORDS_TOO_MANY));
    }
    let mut kept = None;
    if found.unnamed > 0 || found.negative > 0 {
        // The second walk, which makes the children anew, and the names it may try: each name tried is either given or one the style holds, and none is tried twice.
        work.charge_steps(found.nodes.saturating_add(found.unnamed).saturating_add(found.names.len()))?;
        let mut namer = Namer { taken: &mut found.names, next: 1 };
        let nodes: Vec<Placed> = odf
            .kept
            .kept
            .iter()
            .map(|placed| Placed { after: placed.after, node: node_14(&placed.node, &mut namer, &mut older, None, None) })
            .collect();
        kept = Some(Passthrough::from_nodes(nodes));
    }
    Ok(Form14 { name: odf.name.clone(), kept, older })
}

/// The names a style's list styles hold, and the next number to try for one given.
struct Namer<'a> {
    taken: &'a mut HashSet<String>,
    next: u64,
}

impl Namer<'_> {
    fn give(&mut self) -> String {
        loop {
            let candidate = format!("DiaCraft_20_list_20_{}", self.next);
            self.next = self.next.saturating_add(1);
            if self.taken.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

/// One node in 1.4's form. `list` is the name of the list style it is in and `level` the level of the list level it is in (as written), for a label width brought to 0.
fn node_14(node: &RawNode, namer: &mut Namer<'_>, older: &mut Older, list: Option<&str>, level: Option<&str>) -> RawNode {
    let RawNode::Element(el) = node else { return node.clone() };
    let mut attrs = el.attrs.clone();
    let mut list_name: Option<String> = list.map(str::to_owned);
    match local(&el.name) {
        "list-style" => {
            let name = match el.attrs.get("style:name") {
                Some(name) => name.to_owned(),
                None => {
                    let made = namer.give();
                    attrs.push("style:name", &made);
                    older.named.push(made.clone());
                    made
                }
            };
            list_name = Some(name);
        }
        "list-level-properties" => {
            if let Some(width) = el.attrs.get("text:min-label-width").filter(|w| negative(w)) {
                older.labels.push((list.unwrap_or_default().to_owned(), level.unwrap_or_default().to_owned(), width.to_owned()));
                attrs = RawAttrs::from_pairs(
                    el.attrs.iter().map(|(k, v)| if k == "text:min-label-width" { (k.to_owned(), zero_of(v)) } else { (k.to_owned(), v.to_owned()) }),
                );
            }
        }
        _ => {}
    }
    let level = el.attrs.get("text:level").or(level);
    let children: Vec<RawNode> = el.children.iter().map(|c| node_14(c, namer, older, list_name.as_deref(), level)).collect();
    RawNode::Element(RawElement { name: el.name.clone(), attrs, children })
}

/// The forms of the style sheets of `doc` for a save, by their places (`None`: in 1.4's form already, or no style of a file), worked out once before the save writes anything and kept in
/// `work` (`WriteWork::odf14`); and whether the save writes some of the drawing in 1.4's form where it holds an earlier version's (`WriteWork::forms_1_4`: these, and a font size of 0
/// on a style or a shape, `write::runs::LEAST_FONT_SIZE`). One walk of the shapes, a step each, until a shape is found.
pub(crate) fn prepare(doc: &Document, work: &mut WriteWork) -> Result<(), WriteError> {
    let mut forms: Vec<Option<Arc<Form14>>> = Vec::with_capacity(doc.styles.len());
    let mut any = false;
    for (at, style) in doc.styles.iter().enumerate() {
        work.charge_steps(1)?;
        let form = match style.ext.get() {
            Some(data) if at > 0 => Some(of(&data.odf, work)?).filter(|f| !f.older.is_empty()).map(Arc::new),
            _ => None,
        };
        any |= form.is_some() || crate::write::runs::holds_zero_size(&style.sheet);
        forms.push(form);
    }
    let mut stack: Vec<&diacraft_doc::ShapeList> = doc.pages.iter().map(|page| &page.shapes).collect();
    while !any && let Some(list) = stack.pop() {
        for shape in list.iter() {
            work.charge_steps(1)?;
            if crate::write::runs::holds_zero_size(&shape.sheet) {
                any = true;
                break;
            }
            if !shape.children.is_empty() {
                stack.push(&shape.children);
            }
        }
    }
    work.odf14 = Arc::new(forms);
    work.forms_1_4 = any;
    Ok(())
}

/// The form [`prepare`] worked out for the style at `at` of a save, when the style there is the one it was worked out for (the model a save splices is the drawing with the data of the other
/// format taken out, its styles at the same places).
pub(crate) fn cached(work: &WriteWork, at: usize, odf: &OdfStyle) -> Option<Arc<Form14>> {
    work.odf14.get(at).cloned().flatten().filter(|form| form.name == odf.name)
}

/// The records a reader keeps of a style's entry, by what they are looked up by: the properties (the first of a key, and only one ODF 1.4 refuses), the names given, and the widths by the list
/// style and the level (the first of each).
#[derive(Debug, Default)]
pub(crate) struct Index {
    dropped: Vec<(String, String)>,
    keys: HashSet<String>,
    named: HashSet<String>,
    labels: HashMap<(String, String), String>,
    /// The list styles a record names (by the name the file holds them by).
    lists: HashSet<String>,
}

impl Index {
    /// Keeps a property left out, when it is one ODF 1.4 refuses and the first of its key.
    pub(crate) fn dropped(&mut self, key: String, value: String) {
        if refused_property(&key) && self.keys.insert(key.clone()) {
            self.dropped.push((key, value));
        }
    }

    /// Keeps a name given.
    pub(crate) fn named(&mut self, name: String) {
        self.lists.insert(name.clone());
        self.named.insert(name);
    }

    /// Keeps a width brought to 0 (the first of its list style and level).
    pub(crate) fn label(&mut self, list: String, level: String, width: String) {
        self.lists.insert(list.clone());
        self.labels.entry((list, level)).or_insert(width);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.dropped.is_empty() && self.named.is_empty() && self.labels.is_empty()
    }
}

/// The style's ODF data as a read of the file written gives it, put back in the source's form from what the document file carried (`index`): the unmapped properties left out, a list
/// style's name taken out again, a label width given back. What no longer fits (another program renamed the list style, or changed the width) is left as the file has it. `take` is the
/// reader's charge: the unmapped properties are counted before they are looked through and each one put back before it is kept, and the kept children's nodes before the walk that makes them
/// anew, which is made only when a list style of them is one a record names (the walk that finds it is of a tree whose nodes the reader charged when it kept them).
pub(crate) fn restore<E>(odf: &mut OdfStyle, index: &Index, mut take: impl FnMut(usize) -> Result<(), E>) -> Result<(), E> {
    if !index.dropped.is_empty() {
        take(odf.unmapped.len())?;
        let missing: Vec<(String, String)> = {
            let held: HashSet<&str> = odf.unmapped.iter().map(|(k, _)| k).collect();
            index.dropped.iter().filter(|(k, _)| !held.contains(k.as_str())).cloned().collect()
        };
        take(missing.len())?;
        for (key, value) in &missing {
            odf.unmapped.push(key, value);
        }
    }
    if index.named.is_empty() && index.labels.is_empty() {
        return Ok(());
    }
    let mut nodes = 0usize;
    let mut matched = false;
    let mut stack: Vec<&RawNode> = odf.kept.kept.iter().map(|p| &p.node).collect();
    while let Some(node) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if let RawNode::Element(el) = node {
            matched |= local(&el.name) == "list-style" && el.attrs.get("style:name").is_some_and(|n| index.lists.contains(n));
            stack.extend(el.children.iter());
        }
    }
    if !matched {
        return Ok(());
    }
    take(nodes)?;
    let rebuilt: Vec<Placed> = odf.kept.kept.iter().map(|p| Placed { after: p.after, node: restore_node(&p.node, index, None, None) }).collect();
    odf.kept.kept = Passthrough::from_nodes(rebuilt);
    Ok(())
}

fn restore_node(node: &RawNode, index: &Index, list: Option<&str>, level: Option<&str>) -> RawNode {
    let RawNode::Element(el) = node else { return node.clone() };
    let mut attrs = el.attrs.clone();
    let mut list_name = list.map(str::to_owned);
    match local(&el.name) {
        "list-style" => {
            let name = el.attrs.get("style:name").map(str::to_owned);
            if let Some(n) = name.as_deref().filter(|n| index.named.contains(*n)) {
                attrs = RawAttrs::from_pairs(el.attrs.iter().filter(|(k, v)| !(*k == "style:name" && *v == n)));
            }
            list_name = name;
        }
        "list-level-properties" => {
            let at_zero = el.attrs.get("text:min-label-width").is_some_and(|w| w.trim().starts_with('0'));
            let key = (list.unwrap_or_default().to_owned(), level.unwrap_or_default().to_owned());
            if at_zero && let Some(width) = index.labels.get(&key) {
                attrs = RawAttrs::from_pairs(
                    el.attrs
                        .iter()
                        .map(|(k, v)| if k == "text:min-label-width" { (k.to_owned(), width.clone()) } else { (k.to_owned(), v.to_owned()) }),
                );
            }
        }
        _ => {}
    }
    let level = el.attrs.get("text:level").or(level);
    let children: Vec<RawNode> = el.children.iter().map(|c| restore_node(c, index, list_name.as_deref(), level)).collect();
    RawNode::Element(RawElement { name: el.name.clone(), attrs, children })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
        RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children })
    }

    /// A list style of two levels: `name` for the list style, and the widths of the two levels.
    fn list(name: Option<&str>, widths: [&str; 2]) -> RawNode {
        let level = |n: &str, w: &str| {
            el(
                "text:list-level-style-bullet",
                &[("text:level", n)],
                vec![el("style:list-level-properties", &[("text:min-label-width", w)], Vec::new())],
            )
        };
        let attrs: Vec<(&str, &str)> = name.map(|n| vec![("style:name", n)]).unwrap_or_default();
        el("text:list-style", &attrs, vec![level("1", widths[0]), level("2", widths[1])])
    }

    /// A style's kept graphic properties holding `lists`.
    fn kept_of(lists: Vec<RawNode>) -> Passthrough {
        Passthrough::from_nodes(vec![Placed { after: 0, node: el("style:graphic-properties", &[], lists) }])
    }

    fn kept(name: Option<&str>, widths: [&str; 2]) -> Passthrough {
        kept_of(vec![list(name, widths)])
    }

    fn style(kept: Passthrough) -> OdfStyle {
        OdfStyle { kept: diacraft_doc::Kept { raw: RawAttrs::new(), kept }, ..OdfStyle::default() }
    }

    fn index_of(older: &Older) -> Index {
        let mut index = Index::default();
        older.dropped.iter().for_each(|(k, v)| index.dropped(k.clone(), v.clone()));
        older.named.iter().for_each(|n| index.named(n.clone()));
        older.labels.iter().for_each(|(l, lv, w)| index.label(l.clone(), lv.clone(), w.clone()));
        index
    }

    fn free(_: usize) -> Result<(), ()> {
        Ok(())
    }

    /// Each form of an earlier version is written in 1.4's, and the record puts the source's back; what is in 1.4's form already is let be.
    #[test]
    fn the_forms_of_an_earlier_version_are_written_in_1_4_s_and_come_back() {
        let mut work = WriteWork::default();
        let mut odf = style(kept(None, ["-0.4cm", "0.4cm"]));
        odf.unmapped.push("style:paragraph-properties/text:enable-numbering", "false");
        odf.unmapped.push("style:paragraph-properties/fo:hyphenate", "true");
        let form = of(&odf, &mut work).unwrap();
        let (written, older) = (form.kept.unwrap(), form.older);
        assert_eq!(written, kept(Some("DiaCraft_20_list_20_1"), ["0cm", "0.4cm"]), "named, and the negative width at 0 in its unit");
        assert_eq!(older.dropped, vec![("style:paragraph-properties/text:enable-numbering".to_owned(), "false".to_owned())], "only what 1.4 refuses");
        assert_eq!(older.named, vec!["DiaCraft_20_list_20_1".to_owned()]);
        assert_eq!(older.labels, vec![("DiaCraft_20_list_20_1".to_owned(), "1".to_owned(), "-0.4cm".to_owned())]);
        // A read of what was written, with the record put over it, is the source again.
        let mut read = style(written);
        read.unmapped.push("style:paragraph-properties/fo:hyphenate", "true");
        restore(&mut read, &index_of(&older), free).unwrap();
        assert_eq!(read.kept.kept, odf.kept.kept);
        assert_eq!(read.unmapped.get("style:paragraph-properties/text:enable-numbering"), Some("false"));
        // In 1.4's form already: nothing is changed.
        let form = of(&style(kept(Some("Tally"), ["0.4cm", "0.5cm"])), &mut work).unwrap();
        assert!(form.kept.is_none() && form.older.is_empty());
        // A name given is no name the style's list styles hold.
        let both = kept_of(vec![list(Some("DiaCraft_20_list_20_1"), ["0cm", "0cm"]), list(None, ["0cm", "0cm"])]);
        let form = of(&style(both), &mut work).unwrap();
        assert_eq!(form.older.named, vec!["DiaCraft_20_list_20_2".to_owned()]);
        assert!(negative("-0.4cm") && negative(" -.5in") && !negative("0.4cm") && !negative("-"));
        assert_eq!((zero_of("-0.4cm"), zero_of("-5")), ("0cm".to_owned(), "0".to_owned()));
    }

    /// **The security scan of round 2, finding 1: a style of many unnamed list styles.** The names are a set, so a style of 4,096 unnamed list styles is named in steps of the order of its
    /// nodes; each walk is charged to the save before it is made, and a style of more records than [`MOST_RECORDS`] is refused in words, after the first walk's charge.
    #[test]
    fn many_unnamed_list_styles_are_named_in_charged_linear_steps_and_too_many_are_refused() {
        let lists = |n: usize| kept_of((0..n).map(|_| list(None, ["0cm", "0cm"])).collect());
        let mut work = WriteWork::default();
        let form = of(&style(lists(MOST_RECORDS)), &mut work).unwrap();
        assert_eq!(form.older.named.len(), MOST_RECORDS);
        assert_eq!(form.older.named.iter().collect::<HashSet<_>>().len(), MOST_RECORDS, "each name is its own");
        // Two walks of 1 + 5n nodes, and a try for each name.
        let nodes = (1 + 5 * MOST_RECORDS) as u64;
        assert!(work.steps_used() >= 2 * nodes + MOST_RECORDS as u64, "charged: {}", work.steps_used());
        assert!(work.steps_used() <= 2 * nodes + 2 * MOST_RECORDS as u64 + 8, "linear: {}", work.steps_used());
        let mut work = WriteWork::default();
        assert_eq!(of(&style(lists(MOST_RECORDS + 1)), &mut work).err(), Some(WriteError::Unsupported(WORDS_TOO_MANY)));
        assert!(work.steps_used() >= (1 + 5 * (MOST_RECORDS + 1)) as u64, "the first walk is charged before the refusal: {}", work.steps_used());
        // Under a cap of steps, the walk is refused as a save past its steps.
        let mut tight = WriteWork { step_cap: 1_000, ..WriteWork::default() };
        assert!(matches!(of(&style(lists(MOST_RECORDS)), &mut tight), Err(WriteError::Limit { .. })));
    }

    /// **Finding 3, in the small: the restore is charged, finds by maps, and walks nothing when no list style is one a record names.**
    #[test]
    fn a_restore_is_charged_and_walks_nothing_for_a_record_of_no_list_style_of_the_tree() {
        let tree = kept_of((0..100).map(|k| list(Some(&format!("L{k}")), ["0cm", "0cm"])).collect());
        let mut index = Index::default();
        for k in 0..100 {
            index.label(format!("L{k}"), "2".to_owned(), "-1cm".to_owned());
        }
        let mut charged = 0usize;
        let mut odf = style(tree.clone());
        restore(&mut odf, &index, |n| {
            charged += n;
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!(charged, 1 + 5 * 100, "the tree's nodes, before the walk that makes it anew");
        assert_eq!(format!("{:?}", odf.kept.kept).matches("-1cm").count(), 100, "each width given back");
        let mut other = Index::default();
        other.named("M".to_owned());
        let mut charged = 0usize;
        let mut odf = style(tree.clone());
        restore(&mut odf, &other, |n| {
            charged += n;
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!((charged, &odf.kept.kept), (0, &tree), "no walk, and nothing changed");
        let mut odf = style(kept(Some("M"), ["0cm", "0cm"]));
        assert_eq!(restore(&mut odf, &other, |_| Err("no room")), Err("no room"));
    }

    /// **Finding 2, in the small: an index keeps only what 1.4 refuses, and the first record of each key.**
    #[test]
    fn an_index_keeps_the_first_of_each_record_and_only_properties_1_4_refuses() {
        let mut index = Index::default();
        for k in 0..1_000 {
            index.dropped(format!("style:paragraph-properties/fo:x{k}"), "1".to_owned());
            index.dropped("style:paragraph-properties/text:enable-numbering".to_owned(), k.to_string());
        }
        assert_eq!(index.dropped, vec![("style:paragraph-properties/text:enable-numbering".to_owned(), "0".to_owned())]);
        index.label("L".to_owned(), "1".to_owned(), "-1cm".to_owned());
        index.label("L".to_owned(), "1".to_owned(), "-2cm".to_owned());
        assert_eq!(index.labels.get(&("L".to_owned(), "1".to_owned())).map(String::as_str), Some("-1cm"));
    }

    #[test]
    fn an_attribute_of_the_standards_namespaces_is_refused_on_a_properties_element_unless_the_schemas_of_1_2_and_1_4_give_it_there() {
        const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
        // The five the schemas give: the two of a paragraph's properties, `relative-tab-stop-position` (1.4's), and the two of a text's.
        for (holder, local) in [
            ("paragraph-properties", "line-number"),
            ("paragraph-properties", "number-lines"),
            ("paragraph-properties", "relative-tab-stop-position"),
            ("text-properties", "condition"),
            ("text-properties", "display"),
        ] {
            assert!(!refused_on_properties(holder, TEXT_URI, local), "{holder} {local}");
        }
        // Another element's, the same name on the other element, a name of 1.0, and the standard's other namespaces (a graphic's own, the W3C's) are refused.
        assert!(refused_on_properties("paragraph-properties", TEXT_URI, "display"));
        assert!(refused_on_properties("text-properties", TEXT_URI, "number-lines"));
        assert!(refused_on_properties("paragraph-properties", TEXT_URI, "enable-numbering"));
        assert!(refused_on_properties("text-properties", DRAW, "fill-color"));
        assert!(refused_on_properties("text-properties", "http://www.w3.org/2000/svg", "stroke-width"));
        assert!(refused_on_properties("text-properties", "http://www.w3.org/1999/xlink", "href"));
        // A producer's namespace, no namespace, a URI that only looks like the standard's, and the empty one are foreign markup, and no case of this.
        for uri in [
            "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0",
            "http://openoffice.org/2009/office",
            "",
            "urn:example:text",
            "urn:oasis:names:tc:opendocument",
        ] {
            assert!(!refused_on_properties("paragraph-properties", uri, "enable-numbering"), "{uri:?}");
        }
    }

    #[test]
    fn the_carrier_of_an_attribute_and_the_attribute_a_carrier_stands_for_are_each_the_other_s() {
        assert_eq!(carrier_on_properties("paragraph-properties", TEXT_URI, "enable-numbering"), Some("older-enable-numbering"));
        assert_eq!(carried_on_properties("paragraph-properties", "older-enable-numbering"), Some(("text:enable-numbering", TEXT_URI)));
        // Only on the element it is carried for, and in the namespace it is the attribute of.
        assert_eq!(carrier_on_properties("text-properties", TEXT_URI, "enable-numbering"), None);
        assert_eq!(carrier_on_properties("paragraph-properties", "urn:example:text", "enable-numbering"), None);
        assert_eq!(carrier_on_properties("paragraph-properties", TEXT_URI, "other"), None);
        assert_eq!(carried_on_properties("text-properties", "older-enable-numbering"), None);
        assert_eq!(carried_on_properties("paragraph-properties", "older-other"), None);
        // Every carried attribute is one that is refused, and the name that is read back is the one the carrier was made for.
        for (holder, uri, local, carrier, qname) in CARRIED_ON_PROPERTIES {
            assert!(refused_on_properties(holder, uri, local), "{holder} {local}");
            assert_eq!(carrier_on_properties(holder, uri, local), Some(carrier));
            assert_eq!(carried_on_properties(holder, carrier), Some((qname, uri)));
            assert_eq!(qname.split_once(':').map(|(_, l)| l), Some(local));
        }
    }

    /// **A weight 1.4 has no value for is the nearest it lists, on the side of 600 it is on** (100 to 500 are not bold, 600 to 900 are), a tie to the lighter weight; a value it lists is none.
    #[test]
    fn a_weight_is_the_nearest_the_schema_lists_on_the_side_of_600_it_is_on() {
        let w = |v: &str| form_14(Listed::Weight, v);
        for taken in ["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"] {
            assert_eq!(w(taken), None, "{taken}");
        }
        for (value, nearest) in [
            ("250", "200"),
            ("251", "300"),
            ("249.9", "200"),
            ("350", "300"),
            ("550", "500"),
            ("650", "600"),
            ("0", "100"),
            ("-40", "100"),
            ("1000", "900"),
            ("1e9", "900"),
            // Not bold stays not bold, and bold stays bold, though the other side is nearer.
            ("599.9", "500"),
            ("600.1", "600"),
            // The words and the forms of a word.
            ("bolder", "bold"),
            ("Bold", "bold"),
            (" bold ", "bold"),
            ("NORMAL", "normal"),
            ("lighter", "normal"),
            // What is no weight.
            ("", "normal"),
            ("heavy", "normal"),
            ("NaN", "normal"),
            ("inf", "normal"),
            ("-inf", "normal"),
            ("1e999", "normal"),
        ] {
            assert_eq!(w(value), Some(nearest), "{value:?}");
        }
    }

    #[test]
    fn a_slant_and_a_variant_are_the_word_they_differ_from_by_case_or_else_normal() {
        let (s, v) = (|x: &str| form_14(Listed::Slant, x), |x: &str| form_14(Listed::Variant, x));
        assert_eq!((s("normal"), s("italic"), s("oblique")), (None, None, None));
        assert_eq!((v("normal"), v("small-caps")), (None, None));
        assert_eq!((s("Italic"), s(" OBLIQUE"), s("slanted"), s("")), (Some("italic"), Some("oblique"), Some("normal"), Some("normal")));
        assert_eq!((v("SMALL-CAPS"), v("small caps"), v("")), (Some("small-caps"), Some("normal"), Some("normal")));
    }

    /// Every value the function gives is one the schema lists (so a second pass changes nothing), and a carrier is still the value's only while the attribute beside it is what the writer wrote.
    #[test]
    fn what_form_14_gives_is_listed_and_a_carrier_is_used_only_while_it_stands_for_the_attribute() {
        for kind in [Listed::Weight, Listed::Slant, Listed::Variant] {
            for value in ["250", "x", "", "Bold", "1e3", "-1", "599.99", "italic ", "SMALL-CAPS"] {
                if let Some(form) = form_14(kind, value) {
                    assert_eq!(form_14(kind, form), None, "{kind:?} {value:?} gives {form:?}, which 1.4 takes");
                }
            }
        }
        assert_eq!(carried_still(Listed::Weight, "250", "200"), Some("250"));
        assert_eq!(carried_still(Listed::Weight, "250", "bold"), None, "another program changed the weight");
        assert_eq!(carried_still(Listed::Weight, "250", "250"), None, "the writer never writes the value it carries");
        assert_eq!(carried_still(Listed::Weight, "bold", "bold"), None, "a value 1.4 takes has no carrier");
        assert_eq!(carried_still(Listed::Slant, "slanted", "normal"), Some("slanted"));
    }

    #[test]
    fn the_five_attributes_of_the_group_are_listed_with_their_own_carriers() {
        for (space, local, kind, carrier) in LISTED {
            assert_eq!(listed(space, local), Some((kind, carrier)));
            assert!(is_value_carrier(carrier));
            assert_eq!(carrier, format!("older-{local}"));
            // The same name in the other vocabulary is another attribute.
            let other = if space == Space::Fo { Space::Style } else { Space::Fo };
            assert_eq!(listed(other, local), None, "{local}");
        }
        assert_eq!(listed(Space::Fo, "font-size"), None);
        assert!(!is_value_carrier("older-enable-numbering") && !is_value_carrier("font-weight"));
        assert_eq!(
            (space_of(FO_URI), space_of(FO_URI_W3C), space_of(STYLE_URI), space_of(TEXT_URI)),
            (Some(Space::Fo), Some(Space::Fo), Some(Space::Style), None)
        );
    }
}
