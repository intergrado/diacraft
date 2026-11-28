//! Where the elements a save may change lie in the bytes of a part: the drawing's pages and each
//! page's shape elements, numbered as the reader numbers them, with where each starts and ends, the white space before it, which element
//! holds it and which namespace declarations are in force on it.
//!
//! **The numbering is the reader's** (`read::shapes::number`): the children of a page that are drawing shape elements,
//! each numbered in document order, a group's and a hyperlink wrapper's children numbered in their turn (pre-order), and nothing else entered:
//! an element that is no drawing shape is skipped with everything in it. Names are matched by namespace and never by prefix, with the reader's
//! own `ns_of_uri` (so the W3C XSL-FO and SVG URIs count as ODF's). The pages of `content.xml` are the `draw:page`
//! children of `office:drawing` in `office:body`, and the pages of `styles.xml` the `style:master-page` children of `office:master-styles`,
//! as `read::pages` finds them.
//!
//! **What a scan costs**: one pass of the part by the container's tag scan ([`diacraft_container::xml_scan`]), one look at the attributes of
//! each start tag, and one entry of 40 to 60 bytes for each shape element: the work is linear in the part and counted
//! ([`WriteWork::scan_bytes`], [`WriteWork::elements_walked`], [`WriteWork::attribute_bytes`]), and a part past a cap is refused before it is
//! scanned or as the scan passes it. Namespace declarations are not kept as a chain a lookup walks (a shape that declares a prefix of its own
//! would walk the whole chain behind it, and a part of many such shapes would be quadratic in them): the scan holds the bindings in force (a stack
//! for each prefix, and the least prefixes bound to ODF's two namespaces in a set) and gives each shape a [`Scopes`] snapshot of what its start
//! tag can ask of them, shared by every shape that sees the same, so that a lookup is a read.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use diacraft_container::Work;
use diacraft_container::xml_scan::{AttrSpan, Tag, TagKind, is_blank, scan_tags, tag_attributes};

use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::names::{Ns, ns_of_uri};

/// The URI of ODF's `drawing` namespace.
pub(crate) const DRAW_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
/// The URI of ODF's own `svg` namespace (`svg-compatible`), as against the W3C's the reader also reads as ODF's.
pub(crate) const SVG_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
/// The URI of ODF's own `fo` namespace (`xsl-fo-compatible`), as against the W3C's the reader also reads as ODF's.
pub(crate) const FO_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
/// The URI of ODF's `style` namespace.
pub(crate) const STYLE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
/// The URI of ODF's `text` namespace.
pub(crate) const TEXT_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
/// The URI of ODF's `office` namespace (a shape's `office:event-listeners`).
pub(crate) const OFFICE_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
/// The URI of DiaCraft's extension: the scan keeps the least prefix bound to it, as it does for ODF's own, so that an attribute of
/// the extension is written under the prefix the place already binds.
pub(crate) const DIACRAFT_URI: &str = diacraft_doc::DIACRAFT_URN;
/// How many namespaces the scan keeps the least prefix of: ODF's `draw`, `svg`, `fo`, `style` and `text`, and DiaCraft's.
pub(crate) const ODF_NAMESPACES: usize = 6;

/// The most namespace declarations a scan keeps: a real part declares a few dozen. Past it the part is refused, so that no chain a prefix is
/// looked up in is long.
const MOST_DECLARATIONS: usize = 100_000;

/// The most automatic styles a scan keeps.
const MOST_STYLES: usize = 1_000_000;

/// The most `href` attributes a scan keeps.
const MOST_REFERENCES: usize = 4_000_000;

/// The refusal of a scan that could not get the memory for what it keeps: an error, never the abort a `Vec` makes.
fn starved(what: &'static str, held: usize) -> WriteError {
    WriteError::Limit { what, max: held as u64 }
}

/// Appends `item` to `list`, an error and not an abort when the memory for it is not had.
fn keep<T>(list: &mut Vec<T>, item: T, what: &'static str) -> Result<(), WriteError> {
    list.try_reserve(1).map_err(|_| starved(what, list.len()))?;
    list.push(item);
    Ok(())
}

/// What kind of drawing shape element an element is: the reader's `shape_kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Rect,
    Ellipse,
    Circle,
    Line,
    Polyline,
    Polygon,
    Path,
    RegularPolygon,
    Frame,
    Custom,
    Connector,
    Group,
    Measure,
    Caption,
    Control,
    Scene,
    /// `draw:a`, a hyperlink wrapper: numbered, kept whole, and its child shape numbered with it.
    Wrapper,
}

impl Kind {
    /// The kind of the element `local` in `ns`, when it is a drawing shape.
    fn of(ns: Ns, local: &str) -> Option<Kind> {
        Some(match (ns, local) {
            (Ns::Draw, "rect") => Kind::Rect,
            (Ns::Draw, "ellipse") => Kind::Ellipse,
            (Ns::Draw, "circle") => Kind::Circle,
            (Ns::Draw, "line") => Kind::Line,
            (Ns::Draw, "polyline") => Kind::Polyline,
            (Ns::Draw, "polygon") => Kind::Polygon,
            (Ns::Draw, "path") => Kind::Path,
            (Ns::Draw, "regular-polygon") => Kind::RegularPolygon,
            (Ns::Draw, "frame") => Kind::Frame,
            (Ns::Draw, "custom-shape") => Kind::Custom,
            (Ns::Draw, "connector") => Kind::Connector,
            (Ns::Draw, "g") => Kind::Group,
            (Ns::Draw, "measure") => Kind::Measure,
            (Ns::Draw, "caption") => Kind::Caption,
            (Ns::Draw, "control") => Kind::Control,
            (Ns::Dr3d, "scene") => Kind::Scene,
            (Ns::Draw, "a") => Kind::Wrapper,
            _ => return None,
        })
    }

    /// Whether the shape elements inside are numbered as shapes of the page too (the reader's `holds_shapes`).
    fn holds_shapes(self) -> bool {
        matches!(self, Kind::Group | Kind::Wrapper)
    }

    /// Whether the shape is a 1-D shape: a segment between two ends (the reader's `one_d`).
    pub(crate) fn one_d(self) -> bool {
        matches!(self, Kind::Line | Kind::Connector | Kind::Measure)
    }
}

/// What the start tag of a shape element sees of the namespace declarations in force on it, as far as an edit of the tag can ask: the namespace
/// each prefix of its attribute names stands for, and the least prefix in force bound to ODF's own `drawing` and `svg` namespaces.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
struct Snapshot {
    /// The namespace each prefix stands for, and whether its URI is one of the two W3C ones the reader folds into ODF's:
    /// two attributes in the same folded namespace are one name only when they are in the same URI.
    prefixes: BTreeMap<String, (Ns, bool)>,
    /// The least prefix in force bound to each of ODF's own namespaces, by [`odf_kind`]: `draw`, `svg`, `fo`, `style`, `text`.
    odf: [Option<String>; ODF_NAMESPACES],
}

/// The namespace declarations of a part, as the shapes see them: see the module documentation. A shape's scope is the number of the snapshot
/// the scan took of it; the scan takes one for each shape whose declarations differ from the last shape's, and a part whose shapes see the same
/// ones holds a single snapshot.
#[derive(Default)]
pub(crate) struct Scopes {
    snapshots: Vec<Snapshot>,
    by_value: HashMap<Snapshot, u32>,
    /// Every prefix any element of the part declares, in scope or not.
    declared: HashSet<String>,
    /// The fresh prefix chosen for a base (`fresh_prefix`).
    fresh: HashMap<String, String>,
    /// The bindings in force on the element that holds the pages, the first `office:drawing` of `content.xml` or `office:master-styles` of `styles.xml`, by prefix (`""` the default
    /// namespace), when the part has one and the scan is the model path's: the namespaces a shape's kept markup was read in, as far as the model knows them ([`super::kept`]).
    holder: Option<HashMap<String, String>>,
    /// The least prefixes the element that holds the pages sees bound to ODF's office namespace, to Dublin Core's and to ODF's meta namespace: what an annotation is
    /// written under without a declaration of its own.
    names: Option<HolderNames>,
}

/// The prefixes the element that holds the pages binds to the three namespaces an annotation's elements are in that the scan does not track in every scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HolderNames {
    pub office: Option<String>,
    pub dc: Option<String>,
    pub meta: Option<String>,
}

impl Scopes {
    /// The prefixes `office`, `dc` and `meta` are bound under where the pages are, when the part has such an element.
    pub(crate) fn holder_names(&self) -> Option<&HolderNames> {
        self.names.as_ref()
    }

    /// Whether any element of the part declares `prefix`, in scope or not.
    pub(crate) fn is_declared(&self, prefix: &str) -> bool {
        self.declared.contains(prefix)
    }

    /// The namespace `prefix` names in `scope`: `""` is the default namespace; the prefix `xml` is the XML namespace; a prefix the tag of the
    /// scope does not use is no namespace. One look in a map of the tag's own prefixes, charged as a step.
    pub(crate) fn ns_of_prefix(&mut self, scope: u32, prefix: &str, work: &mut WriteWork) -> Ns {
        self.ns_of_prefix_exact(scope, prefix, work).0
    }

    /// [`Scopes::ns_of_prefix`], and whether the prefix is bound to one of the W3C URIs the reader reads as ODF's: the
    /// namespace and that flag together say the URI. A tag may hold `svg:x` and a prefix bound to the W3C's SVG namespace with an `x`
    /// of its own, which are two names though the reader folds them.
    pub(crate) fn ns_of_prefix_exact(&mut self, scope: u32, prefix: &str, work: &mut WriteWork) -> (Ns, bool) {
        if prefix == "xml" {
            return (Ns::Xml, false);
        }
        if !work.step() {
            return (Ns::None, false);
        }
        self.snapshots.get(scope as usize).and_then(|s| s.prefixes.get(prefix)).copied().unwrap_or((Ns::None, false))
    }

    /// The namespace `prefix` stands for on the element that holds the pages (`""` the default namespace, which is the empty name where nothing declares one; `xml` the XML
    /// namespace): `None` for a prefix bound there to nothing, and for any prefix when the part has no such element. **Only the bindings in force on that element count**, never a
    /// declaration another element of the part makes (a sibling's kept element that declares a prefix binds nothing where a shape is written, and the scan
    /// answered once for any declaration in the part).
    pub(crate) fn holder_binding(&self, prefix: &str) -> Option<&str> {
        if prefix == "xml" {
            return Some(super::kept::XML_URI);
        }
        let holder = self.holder.as_ref()?;
        match holder.get(prefix).map(String::as_str) {
            Some(uri) if !uri.is_empty() || prefix.is_empty() => Some(uri),
            Some(_) => None,
            None if prefix.is_empty() => Some(""),
            None => None,
        }
    }

    /// The prefix, bound in `scope` to the ODF namespace `uri` and not hidden by another binding, to write an attribute of that namespace
    /// under: the least of them, so that the same input gives the same bytes; `None` when none is. The prefix of a binding to any other URI
    /// is never taken, though the reader reads the W3C XSL-FO and SVG URIs as ODF's: what the file binds there it keeps, and
    /// an attribute the writer adds is written in ODF's own namespace under a prefix of its own ([`Scopes::fresh_prefix`]). The scan found
    /// the answer as it passed the tag, from the bindings it held then, so a lookup is a read and never a walk.
    pub(crate) fn prefix_for(&mut self, scope: u32, uri: &str, work: &mut WriteWork) -> Option<String> {
        if !work.step() {
            return None;
        }
        let snapshot = self.snapshots.get(scope as usize)?;
        snapshot.odf.get(odf_kind(uri)?).cloned().flatten()
    }

    /// A prefix starting with `base` and ending in a number that no element of the part declares: `svg1`, or the next number up when that is
    /// declared. It is free wherever it is written, since nothing binds it anywhere in the part, and it is the same for every tag of the part,
    /// so it is looked for once (each candidate a step) however many tags ask.
    pub(crate) fn fresh_prefix(&mut self, base: &str, work: &mut WriteWork) -> String {
        if let Some(chosen) = self.fresh.get(base) {
            return chosen.clone();
        }
        // The extension's own prefix is written as it is when the part declares no prefix of that name (a file DiaCraft wrote declares it at its root,
        // and an attribute then takes the prefix the place binds, [`Scopes::prefix_for`]).
        if base == crate::ext::names::PREFIX && !self.declared.contains(base) {
            self.fresh.insert(base.to_owned(), base.to_owned());
            return base.to_owned();
        }
        let mut n = 1usize;
        let chosen = loop {
            if !work.step() {
                return format!("{base}1");
            }
            let candidate = format!("{base}{n}");
            if !self.declared.contains(&candidate) {
                break candidate;
            }
            n += 1;
        };
        self.fresh.insert(base.to_owned(), chosen.clone());
        chosen
    }

    /// The scope of a tag whose attribute names use these prefixes (as places in `bytes`, each with the namespace it stands for) and that sees these
    /// least prefixes bound to ODF's namespaces: the last snapshot's when it answers the same, else one found among those taken, else a new one.
    fn scope_of(
        &mut self,
        last: Option<u32>,
        bytes: &[u8],
        used: &[((usize, usize), Ns, bool)],
        odf: &[Option<&str>; ODF_NAMESPACES],
    ) -> Result<u32, WriteError> {
        let text = |r: &(usize, usize)| bytes.get(r.0..r.1).and_then(|b| std::str::from_utf8(b).ok());
        let answers = |s: &Snapshot| {
            s.odf.iter().zip(odf.iter()).all(|(a, b)| a.as_deref() == *b)
                && used.iter().all(|(r, ns, w3c)| text(r).is_some_and(|p| s.prefixes.get(p) == Some(&(*ns, *w3c))))
        };
        if let Some(at) = last.filter(|at| self.snapshots.get(*at as usize).is_some_and(answers)) {
            return Ok(at);
        }
        let snapshot = Snapshot {
            prefixes: used.iter().filter_map(|(r, ns, w3c)| text(r).map(|p| (p.to_owned(), (*ns, *w3c)))).collect(),
            odf: odf.map(|p| p.map(str::to_owned)),
        };
        if let Some(at) = self.by_value.get(&snapshot) {
            return Ok(*at);
        }
        let at = u32::try_from(self.snapshots.len()).unwrap_or(u32::MAX);
        let what = "memory for the namespace scopes of a part";
        self.by_value.try_reserve(1).map_err(|_| starved(what, self.snapshots.len()))?;
        self.snapshots.try_reserve(1).map_err(|_| starved(what, self.snapshots.len()))?;
        self.by_value.insert(snapshot.clone(), at);
        self.snapshots.push(snapshot);
        Ok(at)
    }
}

/// A shape element of a page.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScanShape {
    /// The range of [`Scan::kids`] that holds this shape's kids, for a shape the scan was asked to look inside.
    pub kids: (u32, u32),
    pub kind: Kind,
    /// The start tag: from its `<` to after its `>`.
    pub open: (usize, usize),
    /// Where the end tag starts, and where the element ends. An element written empty ends where its tag does, and has no end tag.
    pub close_start: usize,
    pub end: usize,
    /// Where the element's unit starts: the white space directly before it when that is all that stands between it and what came before,
    /// else the element's own start. A unit moves and goes with its element, so that the layout around it stays as the file laid it out.
    pub lead: usize,
    /// The ordinal of the shape element that holds this one, or 0 for the page.
    pub parent: u32,
    /// What its start tag sees of the namespace declarations in force on it: the number of a snapshot of [`Scopes`].
    pub scope: u32,
}

/// A page of the part: a `draw:page` of `content.xml`, or a `style:master-page` of `styles.xml`.
#[derive(Debug, Default)]
pub(crate) struct ScanPage {
    /// The page's start tag, the white space before it, and the namespace scope it sees.
    pub open: (usize, usize),
    pub lead: usize,
    pub scope: u32,
    /// The page's `draw:name` or `style:name`, its master page (`draw:master-page-name`) or, for a master page, its layout
    /// (`style:page-layout-name`) and display name.
    pub name: Option<String>,
    pub master: Option<String>,
    pub layout: Option<String>,
    pub display: Option<String>,
    pub close_start: usize,
    pub end: usize,
    /// The page is written as an empty element (`<draw:page .../>`, which LibreOffice writes for a page that holds no shape): its end is its start tag's,
    /// and what a save puts in it needs the element opened first. `qname` is its qualified name as written, which its end tag is made under.
    pub empty: bool,
    pub qname: String,
    /// The page's shape elements in document order: the one numbered `n` is `shapes[n - 1]`.
    pub shapes: Vec<ScanShape>,
    /// Where the page's leading children end that the ODF 1.4 schema puts before its shapes (`svg:title`, `svg:desc`, `draw:layer-set`, `office:forms`), when it has any before its
    /// first shape: a shape put first on the page goes after them (found the forms of 96 visits written after the shapes, which the schema refuses).
    pub first_place: Option<usize>,
    /// The declarations the page's own start tag makes (prefix and URI, in order), when the page is a child of the element whose bindings [`Scopes::holder_binding`] gives: what is
    /// bound where a new shape is put in it is these over those. `None` for a page of any other holder.
    pub ns: Option<Vec<(String, String)>>,
    /// The `draw:layer` elements of the page's own layer set (its first `draw:layer-set`, the one the reader makes its `Layer` rows of) that the reader makes rows of, in order,
    /// when the scan was asked for them ([`Ask::page_layers`]): what a save that changes a flag of one writes on.
    pub layers: Vec<LayerTag>,
    /// Where the scan is in the page's layer sets: 0 before the first, 1 inside it, 2 after it.
    pub layer_sets: u8,
    /// The page's annotation children (`office:annotation`, or LibreOffice's `officeooo:annotation`), each as where its unit starts (the white space before it) and where it ends, in
    /// order, when the scan was asked for them ([`Ask::annotation_pages`]): what a save that rewrites the page's comments takes out.
    pub annotations: Vec<(usize, usize)>,
    /// The prefixes the page's own start tag declares: an annotation written in the page is not written under one of them for a namespace it does not name.
    pub declared: Vec<String>,
}

impl ScanPage {
    /// The shape element numbered `ordinal`.
    pub(crate) fn shape(&self, ordinal: u32) -> Option<&ScanShape> {
        self.shapes.get((ordinal as usize).checked_sub(1)?)
    }
}

/// The family of an automatic style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Fam {
    Graphic,
    Paragraph,
    Text,
    /// A `text:list-style` (named `L` and a number, as LibreOffice names them).
    List,
    /// A `drawing-page` style (a page's or a master page's background).
    DrawingPage,
    /// Any other family: a list style, a table style, a chart style.
    Other,
    /// A page layout (`style:page-layout`): its names are numbered apart from the styles' (`PM1`, `PM2`).
    Layout,
}

/// A `style:graphic-properties`, `style:paragraph-properties` or `style:text-properties` element of a style.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PropsTag {
    pub open: (usize, usize),
    /// Written empty (`<.../>`): the element has no end tag.
    pub empty: bool,
    pub end: usize,
    pub scope: u32,
}

/// An automatic style element of `office:automatic-styles`.
#[derive(Clone, Debug)]
pub(crate) struct StyleDef {
    pub family: Fam,
    /// The qualified name of the element as written.
    pub qname: String,
    pub name: String,
    /// Where the value of `style:name` is, between its quotes.
    pub name_value: (usize, usize),
    /// The unit of the style: the white space before it when that is all that stands between it and what came before, else its own start.
    pub lead: usize,
    pub open: (usize, usize),
    pub empty: bool,
    pub close_start: usize,
    pub end: usize,
    pub scope: u32,
    /// The graphic, paragraph and text properties it holds, by that order.
    pub props: [Option<PropsTag>; 3],
}

/// `office:styles` of `styles.xml`: where a common element (a marker) is put to be the last.
#[derive(Clone, Debug)]
pub(crate) struct CommonStyles {
    pub open: (usize, usize),
    pub empty: bool,
    pub close_start: usize,
    pub end: usize,
    pub scope: u32,
    /// The qualified name as written, which an element written empty is opened and closed under.
    pub qname: String,
}

/// `office:font-face-decls` of a part: where a `style:font-face` is put to be the last.
#[derive(Clone, Debug)]
pub(crate) struct FaceDecls {
    pub open: (usize, usize),
    pub empty: bool,
    pub close_start: usize,
    pub end: usize,
    pub scope: u32,
    /// The qualified name as written, which an element written empty is opened and closed under.
    pub qname: String,
}

/// A `draw:marker` of `office:styles`: its name and the form it draws, and where its unit is (the blank run before it, and its end).
#[derive(Clone, Debug)]
pub(crate) struct MarkerDef {
    pub name: String,
    pub view_box: Option<String>,
    pub d: Option<String>,
    pub lead: usize,
    pub end: usize,
}

/// A common `style:style` of `office:styles` of `styles.xml`: its name and family, and where its unit is (the blank run before it, and its end). What a save
/// that takes a style sheet the model no longer has out of the file removes.
#[derive(Clone, Debug)]
pub(crate) struct CommonStyleDef {
    pub name: String,
    pub family: String,
    pub lead: usize,
    pub end: usize,
}

/// A `style:font-face` of `office:font-face-decls`: the name a style names it by and the family it stands for, its quotes taken off.
#[derive(Clone, Debug)]
pub(crate) struct FaceDef {
    pub name: String,
    pub family: String,
}

/// The most font faces a scan keeps.
const MOST_FACES: usize = 100_000;

/// The most markers a scan keeps.
const MOST_MARKERS: usize = 100_000;

/// The most `draw:layer` elements of the document's layer set the scan notes: as many as a page's `Layer` section holds rows (the reader refuses a set of more).
const MOST_LAYERS: usize = diacraft_doc::MAX_ROWS_PER_SECTION;

/// A `draw:layer` of the document's layer set that the reader makes a `Layer` row of (the first `draw:layer-set` of `office:master-styles` of `styles.xml`): its
/// start tag, the scope it sees and its `draw:name` as the reader reads it. What a save that changes a layer's `Visible`, `Print` or `Lock` writes on.
#[derive(Clone, Debug)]
pub(crate) struct LayerTag {
    pub open: (usize, usize),
    pub scope: u32,
    pub name: String,
}
/// The most common styles a part's scan keeps: the most style sheets a drawing holds (the reader refuses a file of more).
const MOST_COMMON_STYLES: usize = diacraft_doc::MAX_STYLES;

/// A `style:page-layout` of `office:automatic-styles`.
#[derive(Clone, Debug)]
pub(crate) struct LayoutDef {
    pub name: String,
    /// Where the value of `style:name` is, between its quotes.
    pub name_value: (usize, usize),
    pub lead: usize,
    pub empty: bool,
    pub end: usize,
    pub props: Option<PropsTag>,
}

/// The `office:automatic-styles` element of a part: where a style is put to be the last.
#[derive(Clone, Debug)]
pub(crate) struct AutoStyles {
    pub open: (usize, usize),
    pub empty: bool,
    pub close_start: usize,
    pub end: usize,
    pub scope: u32,
    /// The qualified name as written, which a tag that is written empty is opened and closed under.
    pub qname: String,
    /// The part has no such element: this one is the place a save makes it in (`open`, `close_start` and `end` are all the start of `office:body`), and
    /// the element is written, with the styles put in it, only if a style is put in it (`write::styles`).
    pub made: bool,
}

/// Whether a part that declares `version` is one of ODF 1.0 or 1.1, whose schemas lack what 1.2 added (`xml:id`, and `svg:d` and `svg:viewBox` on a
/// connector), so that a save writes none of it there (the milestone-1 class F1). A part that declares none is read by the schemas of
/// 1.0 and 1.1 (ODF 1.2 says an absent version is 1.0 or 1.1), and one that declares a version that is not a number of the line is not written to
/// with attributes it may not have either.
pub(crate) fn is_legacy(version: Option<&str>) -> bool {
    let Some(version) = version else { return true };
    let mut numbers = version.trim().split('.');
    // "1" is 1.0; a version that is not numbers of the line is no version this save writes 1.2's attributes to.
    let major = numbers.next().and_then(|n| n.parse::<u32>().ok());
    let minor = numbers.next().map_or(Some(0), |n| n.parse::<u32>().ok());
    match (major, minor) {
        (Some(major), Some(minor)) => (major, minor) < (1, 2),
        _ => true,
    }
}

/// The `office:body` of `content.xml`: where a part that has no `office:automatic-styles` gets one (before the body, after `office:font-face-decls`, where the
/// schema puts it).
#[derive(Clone, Debug)]
pub(crate) struct BodyAt {
    /// Where the body's start tag starts.
    pub start: usize,
    /// The body's qualified name as written; the new element is in the same namespace, so under the same prefix.
    pub qname: String,
    /// What the body's start tag sees of the namespace declarations (the scan took it).
    pub scope: u32,
    /// The body's own start tag declares a prefix: one that may be the body's own, so the new element, which is not inside the body, cannot be written
    /// under it.
    pub declares: bool,
}

/// What a shape holds that a save may edit inside it, found for the shapes a save asks for.
#[derive(Clone, Debug)]
pub(crate) enum Kid {
    /// `draw:enhanced-geometry`.
    Geometry { open: (usize, usize), empty: bool, end: usize, scope: u32 },
    /// `text:p` or `text:h`, in document order: `lead` is where its unit starts (the white space before it), and `item` the list item it is the child of, as the
    /// place of a [`Kid::Item`] in the same list.
    Para { open: (usize, usize), empty: bool, close_start: usize, end: usize, scope: u32, lead: usize, item: Option<usize>, list: Option<usize> },
    /// `text:list-item` or `text:list-header` that holds a paragraph, with how many elements it holds.
    Item { lead: usize, end: usize, children: u32, header: bool },
    /// A `text:soft-page-break` between paragraphs (R10 item 1): a direct child of the shape or of its `draw:text-box` (one in a list item is inside the item's unit,
    /// which a paragraph put in goes beside). It is an anchor at the start of the paragraph that follows it: it stays with that paragraph while the plan keeps it (a paragraph put in before it goes before the break), and a block that writes it whole or takes it out carries it over by the mapping.
    Break { lead: usize, end: usize },
    /// The outermost `text:list` of a shape's text: a paragraph in a list of any depth is inside it, and the place outside it is where a paragraph that is not
    /// the list's goes.
    List { lead: usize, end: usize },
    /// `svg:title` or `svg:desc`, a direct child of the shape: `desc` says which, `lead` is where its unit starts (the white space before it) and `end` where the
    /// element ends, and `content` is what is between its tags. `plain` is whether the reader reads it as a row: character data and some of it, and no attribute but a declaration.
    Alt { desc: bool, plain: bool, lead: usize, end: usize, content: (usize, usize) },
    /// `draw:glue-point`, a direct child of the shape (CPDOUBLE.ODG): a glue point the file declares as an element. `open` is its start tag, `lead` where its unit starts (the white space
    /// before it) and `end` where the element ends.
    Glue { open: (usize, usize), lead: usize, end: usize, scope: u32 },
}

/// An `href` attribute: which page and shape element it is in (the nearest that is numbered; 0 for none), what element holds it, and its
/// text.
#[derive(Debug)]
pub(crate) struct HrefAt {
    pub page: Option<u32>,
    pub shape: u32,
    pub holder: Holder,
    pub href: String,
}

/// Which element an `href` is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holder {
    /// `draw:object`: a sub-document in a folder.
    Object,
    /// `draw:object-ole`: an object in one file.
    Ole,
    /// `draw:image`: a picture.
    Image,
    /// Another element of the standard that loads a picture by its `href`: `draw:fill-image`, `draw:stroke-image`, `style:background-image` and `text:list-level-style-image`
    /// (a fill, a stroke, a background and a bullet).
    StyleImage,
    /// Any other element.
    Other,
}

/// A child of a flat drawing's root, `office:document`: where its start tag starts (a place a save may put another child before it), the namespaces its start tag
/// sees, its qualified name as written, and whether its start tag declares a prefix of its own.
#[derive(Clone, Debug)]
pub(crate) struct RootChild {
    pub start: usize,
    pub scope: u32,
    pub qname: String,
    pub declares: bool,
}

/// What a scan of a part found.
#[derive(Default)]
pub(crate) struct Scan {
    /// The root is `office:document`: the part is a flat drawing, whose children [`Scan::children`] holds.
    pub flat: bool,
    /// For a flat drawing, when the scan was asked for the styles: the first of each of the eight children, by [`crate::flat::Child::rank`].
    pub children: [Option<RootChild>; 8],
    pub pages: Vec<ScanPage>,
    pub scopes: Scopes,
    pub hrefs: Vec<HrefAt>,
    /// The automatic styles, the page layouts and `office:automatic-styles` itself, when the scan was asked for them.
    pub defs: Vec<StyleDef>,
    pub layouts: Vec<LayoutDef>,
    pub auto: Option<AutoStyles>,
    /// `office:styles` and the markers it holds, in `styles.xml`.
    pub common: Option<CommonStyles>,
    /// The first `office:master-styles` of `styles.xml`, and the first `draw:layer-set` in it, the document's: where a layer the save adds is put.
    pub master_styles: Option<CommonStyles>,
    pub layer_set: Option<CommonStyles>,
    /// The `draw:layer` elements of that layer set, in order.
    pub doc_layers: Vec<LayerTag>,
    /// `office:font-face-decls`, when the part has it and the scan was asked for the styles.
    pub fonts: Option<FaceDecls>,
    pub markers: Vec<MarkerDef>,
    /// The common `style:style` elements of `office:styles`, in document order (`styles.xml`).
    pub common_styles: Vec<CommonStyleDef>,
    /// The markers of DiaCraft's own name that a `draw:marker-start` or `draw:marker-end` of the part names, when the scan was asked for the styles (what a
    /// save looks at to tell a marker that no style names from one that a style does). At most 45 times 100.
    pub marker_refs: BTreeSet<super::markers::Def>,
    /// The font faces the part declares, when the scan was asked for the styles.
    pub faces: Vec<FaceDef>,
    /// `office:body` of `content.xml`, when the scan was asked for the styles.
    pub body: Option<BodyAt>,
    /// What the start tag of the first `office:drawing` of `content.xml` sees of the namespace declarations (the scope a page put in it sees), when the scan was asked for the
    /// styles.
    pub drawing_scope: Option<u32>,
    /// The `office:version` the part's root declares, `None` when it declares none.
    pub version: Option<String>,
    pub kids: Vec<Kid>,
    /// The largest number in an id of the form `id` and digits that an `xml:id` or `draw:id` of the part holds, when the scan was asked for the
    /// styles: a new id is one more.
    pub max_id: u64,
}

/// What a scan is asked to find besides the shape elements.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ask {
    /// The automatic styles, the page layouts, the pages' and master pages' names and `office:automatic-styles`.
    pub styles: bool,
    /// The shapes to look inside, by the page's place in the part and the shape's ordinal: their paragraphs and their enhanced geometry.
    pub inside: HashSet<(usize, u32)>,
    /// The pages, by their place in the part, whose own layer set's layers are noted ([`ScanPage::layers`]).
    pub page_layers: HashSet<usize>,
    /// The pages, by their place in the part, whose annotation children are noted ([`ScanPage::annotations`]).
    pub annotation_pages: HashSet<usize>,
    /// The prefixes the element that holds the pages binds to ODF's office namespace, Dublin Core's and ODF's meta namespace are asked for, as a save that writes comments needs them
    /// ([`Scopes::holder_names`]). The model path's scan takes them whatever is asked (it takes the holder's bindings); a save into a file takes none unless this is set.
    pub holder_names: bool,
}

/// Which part is scanned: it says where the pages are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    Content,
    Styles,
}

/// What an element is to the scan, from where it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Root,
    Body,
    Drawing,
    MasterStyles,
    Page,
    Shape(Kind),
    /// `office:automatic-styles`.
    AutoStyles,
    /// An automatic `style:style`.
    Style,
    /// `style:page-layout`.
    Layout,
    /// `style:graphic-properties`, `style:paragraph-properties` or `style:text-properties` of a style (0, 1, 2), or `style:page-layout-properties`
    /// (3) of a layout.
    Props(usize),
    /// The document's `draw:layer-set`, in `office:master-styles` of `styles.xml`.
    LayerSet,
    /// A `draw:layer` of it.
    DocLayer,
    /// A `draw:layer-set` of a page (a `draw:page`, or a master page), and a `draw:layer` of it.
    PageLayerSet,
    PageLayer,
    /// An `office:annotation` (or LibreOffice's `officeooo:annotation`) child of a page of `content.xml`.
    Annotation,
    /// `office:styles` of `styles.xml`, a `draw:marker` in it and a common `style:style` in it.
    Common,
    Marker,
    CommonStyle,
    /// `office:font-face-decls`, and a `style:font-face` in it.
    FontFaces,
    Face,
    Other,
}

/// What a prefix is bound to: the vocabulary it names, and which of ODF's own two namespaces (0 the drawing's, 1 the SVG one) it is when it is one.
#[derive(Clone, Copy)]
struct Binding {
    ns: Ns,
    odf: Option<usize>,
    /// The URI is one of the two W3C ones the reader folds into ODF's own.
    w3c: bool,
    /// The URI, by its place in [`Scanner::uris`].
    uri: u32,
}

/// Whether `uri` is one of the two W3C URIs the reader reads as ODF's `fo` and `svg`.
fn is_w3c(uri: &str) -> bool {
    matches!(uri, "http://www.w3.org/1999/XSL/Format" | "http://www.w3.org/2000/svg")
}

/// Which of ODF's own namespaces a URI is, as an index of `Scanner::odf`.
fn odf_kind(uri: &str) -> Option<usize> {
    match uri {
        DRAW_URI => Some(0),
        SVG_URI => Some(1),
        FO_URI => Some(2),
        STYLE_URI => Some(3),
        TEXT_URI => Some(4),
        DIACRAFT_URI => Some(5),
        _ => None,
    }
}

/// An element being scanned.
struct Frame {
    role: Role,
    /// The prefixes the element declared, in order.
    declared: Vec<String>,
    /// The ordinal of the nearest numbered shape element at or above this one.
    shape: u32,
    /// Whether its children are numbered as shapes if they are.
    counting: bool,
    /// The element is a numbered shape, and the page it is on.
    numbered: Option<usize>,
    /// The element is a style, a layout or `office:automatic-styles`, by its place in the scan's lists (a style's, a layout's).
    def: Option<usize>,
    /// The element is a kid of a shape the scan looks inside, by its place in `Scan::kids`.
    kid: Option<usize>,
    /// The outermost `text:list` at or above this element, by its place in `Scan::kids`.
    list: Option<usize>,
    /// The element is the `draw:text-box` of a frame: a place where paragraphs, and a soft page break between them, stand.
    text_box: bool,
    /// The element is the one whose bindings [`Scopes::holder_binding`] gives (the first `office:drawing` or `office:master-styles`).
    holder: bool,
    /// The element is a child of a page that the schema puts before its shapes ([`ScanPage::first_place`]).
    leading: bool,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    part: PartKind,
    scan: Scan,
    stack: Vec<Frame>,
    /// Each prefix in force, to what it is bound to, the innermost last.
    bound: HashMap<String, Vec<Binding>>,
    /// The prefixes in force that are bound to ODF's drawing namespace and to its svg namespace, in order: the first of each is the least.
    odf: [BTreeSet<String>; ODF_NAMESPACES],
    /// Namespace declarations met so far.
    decls: usize,
    /// The URI of each declaration met so far, in order (a [`Binding`] names its URI by its place here).
    uris: Vec<String>,
    /// The scope taken of the last shape, and the prefixes of the attribute names of the tag now being scanned (as places in the part).
    last_scope: Option<u32>,
    used: Vec<((usize, usize), Ns, bool)>,
    spans: Vec<AttrSpan>,
    work: &'a mut WriteWork,
    ask: &'a Ask,
    /// The shape being looked inside, by page and ordinal, and where its kids start in `Scan::kids`.
    inside: Option<((usize, u32), u32)>,
}

/// Scans the part `bytes` of kind `part` within `limits`, adding what it cost to `work`. Each pass is charged before it starts: the part's
/// bytes against [`Limits::max_write_bytes`]'s share of scanning, and each tag as it is read against the container's XML item limit.
pub(crate) fn scan_part(bytes: &[u8], part: PartKind, limits: &Limits, work: &mut WriteWork) -> Result<Scan, WriteError> {
    scan_part_asking(bytes, part, &Ask::default(), limits, work)
}

/// [`scan_part`], and what `ask` asks for besides the shape elements.
pub(crate) fn scan_part_asking(bytes: &[u8], part: PartKind, ask: &Ask, limits: &Limits, work: &mut WriteWork) -> Result<Scan, WriteError> {
    work.charge_scan(bytes.len(), limits)?;
    let mut scanner = Scanner {
        bytes,
        part,
        scan: Scan::default(),
        stack: Vec::new(),
        bound: HashMap::new(),
        odf: Default::default(),
        decls: 0,
        uris: Vec::new(),
        last_scope: None,
        used: Vec::new(),
        spans: Vec::new(),
        work,
        ask,
        inside: None,
    };
    let mut container = Work::default();
    let result = scan_tags(bytes, &limits.container.xml, &mut container, |tag| scanner.visit(tag));
    scanner.work.package.scan_events = scanner.work.package.scan_events.saturating_add(container.xml_events);
    match result {
        Ok(()) => Ok(scanner.scan),
        Err(diacraft_container::xml_scan::ScanError::Visit(e)) => Err(e),
        Err(diacraft_container::xml_scan::ScanError::Xml(e)) => {
            Err(WriteError::Original(crate::OdgError::Xml { part: part_name(part).to_owned(), error: e }))
        }
    }
}

fn part_name(part: PartKind) -> &'static str {
    match part {
        PartKind::Content => "content.xml",
        PartKind::Styles => "styles.xml",
    }
}

impl Scanner<'_> {
    fn visit(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        match tag.kind {
            TagKind::Close => {
                self.close(tag);
                Ok(())
            }
            TagKind::Open | TagKind::Empty => {
                self.work.elements_walked = self.work.elements_walked.saturating_add(1);
                self.open(tag)?;
                if tag.kind == TagKind::Empty {
                    self.close(tag);
                }
                Ok(())
            }
        }
    }

    /// The namespace and the local name of an element or attribute name here.
    fn ns_of<'n>(&self, name: &'n str, attribute: bool) -> (Ns, &'n str) {
        match name.split_once(':') {
            Some((prefix, local)) => (self.ns_of_prefix(prefix), local),
            None if attribute => (Ns::None, name),
            None => (self.ns_of_prefix(""), name),
        }
    }

    fn ns_of_prefix(&self, prefix: &str) -> Ns {
        if prefix == "xml" {
            return Ns::Xml;
        }
        self.bound.get(prefix).and_then(|stack| stack.last()).map_or(Ns::None, |b| b.ns)
    }

    /// Binds `prefix` to `uri` for the element being scanned: the prefix names `uri`'s vocabulary from here, and if `uri` is one of ODF's own
    /// the prefix is one of those a tag may write ODF's attributes under, and hides what the prefix was bound to before.
    fn bind(&mut self, prefix: &str, uri: &str, at: u32) {
        let odf = odf_kind(uri);
        let stack = self.bound.entry(prefix.to_owned()).or_default();
        if let Some(hidden) = stack.last().and_then(|b| b.odf) {
            self.odf[hidden].remove(prefix);
        }
        stack.push(Binding { ns: ns_of_uri(uri), odf, w3c: is_w3c(uri), uri: at });
        // The default namespace has no prefix to write a name under.
        if let (Some(kind), false) = (odf, prefix.is_empty()) {
            self.odf[kind].insert(prefix.to_owned());
        }
    }

    /// Undoes the last binding of `prefix`: what it was bound to before is in force again.
    fn unbind(&mut self, prefix: &str) {
        let Some(stack) = self.bound.get_mut(prefix) else { return };
        if let Some(gone) = stack.pop().and_then(|b| b.odf) {
            self.odf[gone].remove(prefix);
        }
        if let (Some(kind), false) = (stack.last().and_then(|b| b.odf), prefix.is_empty()) {
            self.odf[kind].insert(prefix.to_owned());
        }
    }

    /// Reads the tag's declarations into the bindings in force, and gives the prefixes it declared. Each binding is charged two steps (it is made
    /// and undone) before it is made.
    fn declare(&mut self) -> Result<Vec<String>, WriteError> {
        let mut made = Vec::new();
        for i in 0..self.spans.len() {
            let Some(a) = self.spans.get(i).copied() else { break };
            let name = self.bytes.get(a.name.0..a.name.1).unwrap_or_default();
            let prefix = if name == b"xmlns" {
                String::new()
            } else if let Some(p) = name.strip_prefix(b"xmlns:") {
                String::from_utf8_lossy(p).into_owned()
            } else {
                continue;
            };
            if self.decls >= MOST_DECLARATIONS {
                return Err(WriteError::Limit { what: "namespace declarations in a part", max: MOST_DECLARATIONS as u64 });
            }
            self.decls += 1;
            self.work.step();
            self.work.step();
            self.work.check_steps()?;
            let uri = crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default());
            self.scan.scopes.declared.insert(prefix.clone());
            // The URI is kept only where the holder's bindings are taken (the model path's): a save into a file walks and keeps what it did.
            let at = if self.work.model_path { u32::try_from(self.uris.len()).unwrap_or(u32::MAX) } else { u32::MAX };
            self.bind(&prefix, &uri, at);
            if self.work.model_path {
                keep(&mut self.uris, uri, "memory for the namespace declarations of a part")?;
            }
            made.push(prefix);
        }
        Ok(made)
    }

    /// The scope of the shape whose start tag is being scanned: what its attribute names' prefixes stand for here, and the least prefixes in force
    /// bound to ODF's drawing and svg namespaces. Taken from the bindings in force now, so that no later lookup walks to find them.
    fn take_scope(&mut self) -> Result<u32, WriteError> {
        let bytes = self.bytes;
        self.used.clear();
        for a in &self.spans {
            let Some(name) = bytes.get(a.name.0..a.name.1) else { continue };
            let Some(colon) = name.iter().position(|b| *b == b':') else { continue };
            let Some(prefix) = name.get(..colon).and_then(|p| std::str::from_utf8(p).ok()) else { continue };
            if prefix == "xmlns" {
                continue;
            }
            let (ns, w3c) = if prefix == "xml" {
                (Ns::Xml, false)
            } else {
                self.bound.get(prefix).and_then(|stack| stack.last()).map_or((Ns::None, false), |b| (b.ns, b.w3c))
            };
            self.used.push(((a.name.0, a.name.0 + colon), ns, w3c));
        }
        let odf: [Option<&str>; ODF_NAMESPACES] = std::array::from_fn(|k| self.odf.get(k).and_then(|set| set.iter().next()).map(String::as_str));
        let scope = self.scan.scopes.scope_of(self.last_scope, bytes, &self.used, &odf)?;
        self.last_scope = Some(scope);
        Ok(scope)
    }

    fn open(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        if tag.depth != self.stack.len() {
            return Err(WriteError::Mismatch("the part's tags do not nest as they were counted"));
        }
        self.spans.clear();
        self.work.charge_attribute_bytes(tag.end - tag.start)?;
        if tag_attributes(self.bytes, tag.start, tag.end, &mut self.spans).is_none() {
            return Err(WriteError::Mismatch("a start tag the scan cannot read the attributes of"));
        }
        let declared = self.declare()?;
        if self.ask.styles {
            self.note_ids();
            self.note_marker_refs();
        }
        let (ns, local) = self.ns_of(tag.name, false);
        let parent = self.stack.last();
        let role = self.role(parent.map(|f| (f.role, f.counting)), ns, local, tag.depth);
        let parent_shape = parent.map_or(0, |f| f.shape);
        let parent_def = parent.map(|f| f.def).unwrap_or_default();
        let mut frame = Frame {
            role,
            declared,
            shape: parent_shape,
            counting: false,
            numbered: None,
            def: None,
            kid: None,
            list: parent.and_then(|f| f.list),
            text_box: matches!((ns, local), (Ns::Draw, "text-box")),
            holder: false,
            leading: parent.is_some_and(|f| f.role == Role::Page)
                && matches!((ns, local), (Ns::Svg, "title" | "desc") | (Ns::Draw, "layer-set") | (Ns::Office, "forms")),
        };
        // A flat drawing: the root is `office:document`, and each of its eight children is noted where it starts (the first of each).
        if role == Role::Root {
            self.scan.flat = (ns, local) == (Ns::Office, "document");
        } else if self.scan.flat
            && self.ask.styles
            && tag.depth == 1
            && ns == Ns::Office
            && let Some(child) = crate::flat::Child::of(local)
            && self.scan.children.get(child.rank()).is_some_and(Option::is_none)
        {
            let scope = self.take_scope()?;
            let held = RootChild { start: tag.start, scope, qname: tag.name.to_owned(), declares: !frame.declared.is_empty() };
            if let Some(slot) = self.scan.children.get_mut(child.rank()) {
                *slot = Some(held);
            }
        }
        match role {
            Role::Page => self.open_page(tag, &mut frame)?,
            Role::Root => self.scan.version = self.attr(Ns::Office, "version").map(|(_, v)| v),
            Role::Body => self.open_body(tag, &frame)?,
            Role::Drawing => {
                self.open_holder(&mut frame)?;
                self.open_drawing()?;
            }
            Role::MasterStyles => {
                self.open_holder(&mut frame)?;
                if self.scan.master_styles.is_none() {
                    self.scan.master_styles = Some(self.held_at(tag)?);
                }
            }
            Role::LayerSet if self.scan.layer_set.is_none() => self.scan.layer_set = Some(self.held_at(tag)?),
            Role::DocLayer => self.open_doc_layer(tag)?,
            Role::PageLayerSet => {
                if let Some(page) = self.scan.pages.last_mut()
                    && page.layer_sets == 0
                {
                    page.layer_sets = 1;
                }
            }
            Role::PageLayer => self.open_page_layer(tag)?,
            Role::Annotation => self.open_annotation(tag)?,
            Role::Shape(kind) => self.open_shape(tag, kind, &mut frame)?,
            Role::AutoStyles => self.open_auto(tag)?,
            Role::Common => self.open_common(tag)?,
            Role::FontFaces => self.open_fonts(tag)?,
            Role::Marker => self.open_marker(tag)?,
            Role::CommonStyle => self.open_common_style(tag)?,
            Role::Face => self.open_face()?,
            Role::Style => self.open_style(tag, &mut frame)?,
            Role::Layout => self.open_layout(tag, &mut frame)?,
            Role::Props(kind) => self.open_props(tag, kind, parent_def)?,
            _ => {}
        }
        if self.inside.is_some() && matches!(role, Role::Other) {
            self.open_kid(tag, ns, local, &mut frame)?;
        }
        self.note_hrefs(&frame, ns, local)?;
        self.stack.push(frame);
        Ok(())
    }

    /// What an element is, from its parent's role and what it is called.
    fn role(&self, parent: Option<(Role, bool)>, ns: Ns, local: &str, depth: usize) -> Role {
        let Some((parent, counting)) = parent else { return if depth == 0 { Role::Root } else { Role::Other } };
        match (self.part, parent, ns, local) {
            (PartKind::Content, Role::Root, Ns::Office, "body") => Role::Body,
            (PartKind::Content, Role::Body, Ns::Office, "drawing") => Role::Drawing,
            (PartKind::Content, Role::Drawing, Ns::Draw, "page") => Role::Page,
            (PartKind::Styles, Role::Root, Ns::Office, "master-styles") => Role::MasterStyles,
            (PartKind::Styles, Role::MasterStyles, Ns::Style, "master-page") => Role::Page,
            (PartKind::Styles, Role::MasterStyles, Ns::Draw, "layer-set") => Role::LayerSet,
            (PartKind::Styles, Role::LayerSet, Ns::Draw, "layer") => Role::DocLayer,
            (_, Role::Page, Ns::Draw, "layer-set") => Role::PageLayerSet,
            (PartKind::Content, Role::Page, Ns::Office | Ns::Officeooo, "annotation") => Role::Annotation,
            (_, Role::PageLayerSet, Ns::Draw, "layer") => Role::PageLayer,
            (PartKind::Styles, Role::Root, Ns::Office, "styles") => Role::Common,
            // A line end's marker, and the gradients and hatches of `office:styles`: each is held by its name, to be told from the ones DiaCraft writes and to be taken out.
            (PartKind::Styles, Role::Common, Ns::Draw, "marker" | "gradient" | "hatch" | "stroke-dash") => Role::Marker,
            (PartKind::Styles, Role::Common, Ns::Style, "style") => Role::CommonStyle,
            (_, Role::Root, Ns::Office, "font-face-decls") if self.ask.styles => Role::FontFaces,
            (_, Role::FontFaces, Ns::Style, "font-face") => Role::Face,
            (_, Role::Root, Ns::Office, "automatic-styles") if self.ask.styles => Role::AutoStyles,
            (_, Role::AutoStyles, Ns::Style, "style") => Role::Style,
            (_, Role::AutoStyles, Ns::Text, "list-style") => Role::Style,
            (_, Role::AutoStyles, Ns::Style, "page-layout") => Role::Layout,
            (_, Role::Style, Ns::Style, "graphic-properties") => Role::Props(0),
            (_, Role::Style, Ns::Style, "paragraph-properties") => Role::Props(1),
            (_, Role::Style, Ns::Style, "text-properties") => Role::Props(2),
            (_, Role::Layout, Ns::Style, "page-layout-properties") => Role::Props(3),
            _ if counting => Kind::of(ns, local).map_or(Role::Other, Role::Shape),
            _ => Role::Other,
        }
    }

    fn open_page(&mut self, tag: &Tag<'_>, frame: &mut Frame) -> Result<(), WriteError> {
        frame.counting = true;
        frame.shape = 0;
        let (scope, lead) = if self.ask.styles { (self.take_scope()?, self.lead_of(tag)) } else { (0, tag.start) };
        let text = |s: &Self, ns, local: &str| s.attr(ns, local).map(|(_, v)| v);
        let (name, master, layout, display) = if self.ask.styles {
            let name = if self.part == PartKind::Content { text(self, Ns::Draw, "name") } else { text(self, Ns::Style, "name") };
            (name, text(self, Ns::Draw, "master-page-name"), text(self, Ns::Style, "page-layout-name"), text(self, Ns::Style, "display-name"))
        } else {
            (None, None, None, None)
        };
        // The page's own declarations, for a page of the holder whose bindings the scan took: what is in force where a shape is put in it.
        let ns = if self.stack.last().is_some_and(|parent| parent.holder) {
            let mut own = Vec::new();
            own.try_reserve_exact(frame.declared.len()).map_err(|_| starved("memory for the namespace scopes of a part", own.len()))?;
            for prefix in &frame.declared {
                let uri =
                    self.bound.get(prefix).and_then(|stack| stack.last()).and_then(|b| self.uris.get(b.uri as usize)).cloned().unwrap_or_default();
                own.push((prefix.clone(), uri));
            }
            Some(own)
        } else {
            None
        };
        let page = ScanPage {
            open: (tag.start, tag.end),
            lead,
            scope,
            name,
            master,
            layout,
            display,
            close_start: tag.end,
            end: tag.end,
            empty: false,
            qname: tag.name.to_owned(),
            shapes: Vec::new(),
            first_place: None,
            ns,
            layers: Vec::new(),
            layer_sets: 0,
            annotations: Vec::new(),
            declared: frame.declared.clone(),
        };
        keep(&mut self.scan.pages, page, "memory for the pages of a part")
    }

    /// An annotation child of a page, noted when the scan was asked for the page's annotations: where its unit starts and, once it closes, where it ends.
    fn open_annotation(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let at = self.scan.pages.len().saturating_sub(1);
        if !self.ask.annotation_pages.contains(&at) {
            return Ok(());
        }
        let lead = self.lead_of(tag);
        let Some(page) = self.scan.pages.last_mut() else { return Ok(()) };
        keep(&mut page.annotations, (lead, tag.end), "memory for the comments of a page")
    }

    /// A `draw:layer` of a page's layer set: noted when the scan was asked for the page's layers, the set is the page's first (the one the reader reads) and still
    /// open, and the layer is one the reader makes a `Layer` row of (a `draw:name` none of LibreOffice's five), so that the layers noted are the page's rows in their order; no
    /// more of them than the reader reads a set of.
    fn open_page_layer(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let at = self.scan.pages.len().saturating_sub(1);
        if !self.ask.page_layers.contains(&at) || self.scan.pages.last().is_none_or(|p| p.layer_sets != 1) {
            return Ok(());
        }
        let Some(name) = self.attr(Ns::Draw, "name").map(|(_, v)| v) else { return Ok(()) };
        if crate::read::layers::PRODUCER_LAYERS.contains(&name.as_str()) {
            return Ok(());
        }
        let scope = self.take_scope()?;
        let Some(page) = self.scan.pages.last_mut() else { return Ok(()) };
        if page.layers.len() >= MOST_LAYERS {
            return Err(WriteError::Limit { what: "layers in a page's layer set", max: MOST_LAYERS as u64 });
        }
        page.layers.try_reserve(1).map_err(|_| starved("memory for the layers of a page", page.layers.len()))?;
        page.layers.push(LayerTag { open: (tag.start, tag.end), scope, name });
        Ok(())
    }

    /// Notes the largest number among the ids (`xml:id`, `draw:id`) of the form `id` and digits on the tag being scanned.
    fn note_ids(&mut self) {
        let mut best = self.scan.max_id;
        for a in &self.spans {
            let Some(name) = self.bytes.get(a.name.0..a.name.1).and_then(|n| std::str::from_utf8(n).ok()) else { continue };
            if !name.ends_with(":id") {
                continue;
            }
            let (ns, local) = self.ns_of(name, true);
            if local != "id" || !matches!(ns, Ns::Xml | Ns::Draw) {
                continue;
            }
            let value = self.bytes.get(a.value.0..a.value.1).and_then(|v| std::str::from_utf8(v).ok()).unwrap_or_default();
            if let Some(n) = value
                .strip_prefix("id")
                .filter(|d| !d.is_empty() && d.len() <= 18 && d.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|d| d.parse::<u64>().ok())
            {
                best = best.max(n);
            }
        }
        self.scan.max_id = best;
    }

    /// Where the unit of the element whose start tag is `tag` starts: the blank run before it, else the tag.
    fn lead_of(&self, tag: &Tag<'_>) -> usize {
        match tag.text_before {
            Some((from, to)) if is_blank(self.bytes.get(from..to).unwrap_or_default()) => from,
            _ => tag.start,
        }
    }

    /// The attribute `local` of `ns` on the tag being scanned: where its value is and the value, references resolved.
    fn attr(&self, ns: Ns, local: &str) -> Option<((usize, usize), String)> {
        self.spans.iter().find_map(|a| {
            let name = std::str::from_utf8(self.bytes.get(a.name.0..a.name.1)?).ok()?;
            if name == "xmlns" || name.starts_with("xmlns:") {
                return None;
            }
            let (n, l) = self.ns_of(name, true);
            (n == ns && l == local).then(|| (a.value, crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default())))
        })
    }

    fn open_auto(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        let qname = tag.name.to_owned();
        self.scan.auto = Some(AutoStyles { open: (tag.start, tag.end), empty: false, close_start: tag.end, end: tag.end, scope, qname, made: false });
        Ok(())
    }

    /// Where the element whose start tag is `tag` is, and the namespaces in force on it; its end is put in when it closes.
    fn held_at(&mut self, tag: &Tag<'_>) -> Result<CommonStyles, WriteError> {
        let scope = self.take_scope()?;
        Ok(CommonStyles { open: (tag.start, tag.end), empty: false, close_start: tag.end, end: tag.end, scope, qname: tag.name.to_owned() })
    }

    /// A `draw:layer` of a layer set of `office:master-styles`: noted when the set is the first, the document's, which is still open (a later set's layers are no rows of
    /// any page the document's set gives), and the layer is one the reader makes a `Layer` row of: it has a `draw:name` that is none of LibreOffice's five. So the layers
    /// noted are the rows in their order, and no more of them than the reader reads a set of.
    fn open_doc_layer(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let in_first = self.scan.layer_set.as_ref().is_some_and(|h| h.close_start == h.open.1 && !h.empty && h.end == h.open.1);
        if !in_first {
            return Ok(());
        }
        let Some(name) = self.attr(Ns::Draw, "name").map(|(_, v)| v) else { return Ok(()) };
        if crate::read::layers::PRODUCER_LAYERS.contains(&name.as_str()) {
            return Ok(());
        }
        if self.scan.doc_layers.len() >= MOST_LAYERS {
            return Err(WriteError::Limit { what: "layers in the document's layer set", max: MOST_LAYERS as u64 });
        }
        let scope = self.take_scope()?;
        self.scan.doc_layers.try_reserve(1).map_err(|_| starved("memory for the layers of a part", self.scan.doc_layers.len()))?;
        self.scan.doc_layers.push(LayerTag { open: (tag.start, tag.end), scope, name });
        Ok(())
    }

    /// `office:styles` of `styles.xml`.
    fn open_common(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        self.scan.common =
            Some(CommonStyles { open: (tag.start, tag.end), empty: false, close_start: tag.end, end: tag.end, scope, qname: tag.name.to_owned() });
        Ok(())
    }

    /// `office:font-face-decls`.
    fn open_fonts(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        self.scan.fonts =
            Some(FaceDecls { open: (tag.start, tag.end), empty: false, close_start: tag.end, end: tag.end, scope, qname: tag.name.to_owned() });
        Ok(())
    }

    /// A `draw:marker` of `office:styles`: its name and the form it draws, and where its unit is.
    fn open_marker(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        if self.scan.markers.len() >= MOST_MARKERS {
            return Err(WriteError::Limit { what: "markers in a part", max: MOST_MARKERS as u64 });
        }
        let text = |s: &Self, ns, local: &str| s.attr(ns, local).map(|(_, v)| v);
        let name = text(self, Ns::Draw, "name").unwrap_or_default();
        let (view_box, d) = (text(self, Ns::Svg, "viewBox"), text(self, Ns::Svg, "d"));
        let (lead, end) = (self.lead_of(tag), tag.end);
        keep(&mut self.scan.markers, MarkerDef { name, view_box, d, lead, end }, "memory for the markers of a part")
    }

    /// A common `style:style` of `office:styles`: its name and family, and where its unit is.
    fn open_common_style(&mut self, tag: &Tag<'_>) -> Result<(), WriteError> {
        if self.scan.common_styles.len() >= MOST_COMMON_STYLES {
            return Err(WriteError::Limit { what: "common styles in a part", max: MOST_COMMON_STYLES as u64 });
        }
        let text = |s: &Self, ns, local: &str| s.attr(ns, local).map(|(_, v)| v).unwrap_or_default();
        let (name, family) = (text(self, Ns::Style, "name"), text(self, Ns::Style, "family"));
        let (lead, end) = (self.lead_of(tag), tag.end);
        keep(&mut self.scan.common_styles, CommonStyleDef { name, family, lead, end }, "memory for the common styles of a part")
    }

    /// Notes the markers of DiaCraft's own name that the tag being scanned names, by `draw:marker-start` or `draw:marker-end`. The values are
    /// matched as the reader reads a name, references resolved; at most 45 times 100 are kept.
    fn note_marker_refs(&mut self) {
        // The line ends, the gradient and the hatch a fill names, and the dash a line names: each of DiaCraft's own name is a definition a style may be the last to name.
        for local in ["marker-start", "marker-end", "fill-gradient-name", "fill-hatch-name", "stroke-dash"] {
            let Some((_, value)) = self.attr(Ns::Draw, local) else { continue };
            if let Some(key) = super::markers::Def::of_name(&value) {
                self.scan.marker_refs.insert(key);
            }
        }
    }

    /// A `style:font-face` of `office:font-face-decls`: its name and its family, the quotes and the white space round the family taken off.
    fn open_face(&mut self) -> Result<(), WriteError> {
        if self.scan.faces.len() >= MOST_FACES {
            return Err(WriteError::Limit { what: "font faces in a part", max: MOST_FACES as u64 });
        }
        let text = |s: &Self, ns, local: &str| s.attr(ns, local).map(|(_, v)| v);
        let name = text(self, Ns::Style, "name").unwrap_or_default();
        let family = text(self, Ns::Svg, "font-family").unwrap_or_default();
        let family = family.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_owned();
        keep(&mut self.scan.faces, FaceDef { name, family }, "memory for the font faces of a part")
    }

    /// `office:body`: where a part with no `office:automatic-styles` gets one, when the scan is asked for the styles.
    fn open_body(&mut self, tag: &Tag<'_>, frame: &Frame) -> Result<(), WriteError> {
        if !self.ask.styles {
            return Ok(());
        }
        let scope = self.take_scope()?;
        self.scan.body = Some(BodyAt { start: tag.start, qname: tag.name.to_owned(), scope, declares: !frame.declared.is_empty() });
        Ok(())
    }

    /// The element that holds the pages (`office:drawing`, `office:master-styles`): the bindings in force on it, for the first one the part has. Each binding taken is
    /// a step, charged before the next. Taken on the model path only, whose new shapes write the markup they kept against them: a save into a file writes none of it, and its scan
    /// costs what it did.
    fn open_holder(&mut self, frame: &mut Frame) -> Result<(), WriteError> {
        if self.scan.scopes.holder.is_some() || !self.work.model_path {
            return Ok(());
        }
        let mut holder = HashMap::new();
        for (prefix, stack) in &self.bound {
            let Some(binding) = stack.last() else { continue };
            self.work.step();
            self.work.check_steps()?;
            let uri = self.uris.get(binding.uri as usize).cloned().unwrap_or_default();
            holder.try_reserve(1).map_err(|_| starved("memory for the namespace scopes of a part", holder.len()))?;
            holder.insert(prefix.clone(), uri);
        }
        self.scan.scopes.holder = Some(holder);
        frame.holder = true;
        Ok(())
    }

    /// `office:drawing`: what its start tag sees, for the pages a save puts in it (the first one the part has).
    fn open_drawing(&mut self) -> Result<(), WriteError> {
        if self.ask.styles && self.scan.drawing_scope.is_none() {
            self.scan.drawing_scope = Some(self.take_scope()?);
        }
        // The prefixes an annotation's elements are written under, the least bound to each namespace here (one look at each binding, a step each: the model path's holder has charged
        // them already, and a save into a file that writes no comment takes none).
        if self.scan.scopes.names.is_none() && (self.work.model_path || self.ask.holder_names) {
            let mut names = HolderNames::default();
            for (prefix, stack) in &self.bound {
                if !self.work.model_path {
                    self.work.step();
                    self.work.check_steps()?;
                }
                let Some(binding) = stack.last() else { continue };
                let slot = match binding.ns {
                    Ns::Office => &mut names.office,
                    Ns::Dc => &mut names.dc,
                    Ns::Meta => &mut names.meta,
                    _ => continue,
                };
                if !prefix.is_empty() && slot.as_deref().is_none_or(|have| prefix.as_str() < have) {
                    *slot = Some(prefix.clone());
                }
            }
            self.scan.scopes.names = Some(names);
        }
        Ok(())
    }

    fn open_style(&mut self, tag: &Tag<'_>, frame: &mut Frame) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        let lead = self.lead_of(tag);
        let is_list = tag.name.rsplit(':').next() == Some("list-style");
        let family = match self.attr(Ns::Style, "family").map(|(_, v)| v).as_deref() {
            _ if is_list => Fam::List,
            Some("graphic") => Fam::Graphic,
            Some("paragraph") => Fam::Paragraph,
            Some("text") => Fam::Text,
            Some("drawing-page") => Fam::DrawingPage,
            _ => Fam::Other,
        };
        let (name_value, name) = self.attr(Ns::Style, "name").unwrap_or(((0, 0), String::new()));
        if self.scan.defs.len() >= MOST_STYLES {
            return Err(WriteError::Limit { what: "automatic styles in a part", max: MOST_STYLES as u64 });
        }
        let def = StyleDef {
            family,
            qname: tag.name.to_owned(),
            name,
            name_value,
            lead,
            open: (tag.start, tag.end),
            empty: false,
            close_start: tag.end,
            end: tag.end,
            scope,
            props: [None, None, None],
        };
        frame.def = Some(self.scan.defs.len());
        keep(&mut self.scan.defs, def, "memory for the automatic styles of a part")
    }

    fn open_layout(&mut self, tag: &Tag<'_>, frame: &mut Frame) -> Result<(), WriteError> {
        let lead = self.lead_of(tag);
        let (name_value, name) = self.attr(Ns::Style, "name").unwrap_or(((0, 0), String::new()));
        let layout = LayoutDef { name, name_value, lead, empty: false, end: tag.end, props: None };
        frame.def = Some(self.scan.layouts.len());
        keep(&mut self.scan.layouts, layout, "memory for the page layouts of a part")
    }

    /// The properties element of a style (`kind` 0 to 2) or a layout (3): `holder` is the style's or layout's place.
    fn open_props(&mut self, tag: &Tag<'_>, kind: usize, holder: Option<usize>) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        let props = PropsTag { open: (tag.start, tag.end), empty: false, end: tag.end, scope };
        let Some(at) = holder else { return Ok(()) };
        if kind == 3 {
            if let Some(layout) = self.scan.layouts.get_mut(at) {
                layout.props.get_or_insert(props);
            }
        } else if let Some(slot) = self.scan.defs.get_mut(at).and_then(|d| d.props.get_mut(kind)) {
            slot.get_or_insert(props);
        }
        Ok(())
    }

    /// An element inside a shape the scan looks inside: its enhanced geometry and its paragraphs.
    fn open_kid(&mut self, tag: &Tag<'_>, ns: Ns, local: &str, frame: &mut Frame) -> Result<(), WriteError> {
        // An element inside a list item counts for it, whatever it is: an item with one child that is a paragraph is the paragraph's unit.
        if let Some(at) = self.stack.last().and_then(|f| f.kid)
            && let Some(Kid::Item { children, .. }) = self.scan.kids.get_mut(at)
        {
            *children = children.saturating_add(1);
        }
        // An element inside a title or a description: the reader does not read it as a row.
        if let Some(at) = self.stack.last().and_then(|f| f.kid)
            && let Some(Kid::Alt { plain, .. }) = self.scan.kids.get_mut(at)
        {
            *plain = false;
        }
        let kid = match (ns, local) {
            (Ns::Draw, "enhanced-geometry") => {
                let scope = self.take_scope()?;
                Kid::Geometry { open: (tag.start, tag.end), empty: false, end: tag.end, scope }
            }
            (Ns::Text, "p" | "h") => {
                let scope = self.take_scope()?;
                let lead = self.lead_of(tag);
                let item = self.stack.last().and_then(|f| f.kid).filter(|at| matches!(self.scan.kids.get(*at), Some(Kid::Item { .. })));
                Kid::Para { open: (tag.start, tag.end), empty: false, close_start: tag.end, end: tag.end, scope, lead, item, list: frame.list }
            }
            (Ns::Svg, "title" | "desc") => {
                if !self.stack.last().is_some_and(|f| matches!(f.role, Role::Shape(_))) {
                    return Ok(());
                }
                let declared_only = self.spans.iter().all(|a| self.bytes.get(a.name.0..a.name.1).is_some_and(|name| name.starts_with(b"xmlns")));
                Kid::Alt { desc: local == "desc", plain: declared_only, lead: self.lead_of(tag), end: tag.end, content: (tag.end, tag.end) }
            }
            (Ns::Draw, "glue-point") => {
                if !self.stack.last().is_some_and(|f| matches!(f.role, Role::Shape(_))) {
                    return Ok(());
                }
                let scope = self.take_scope()?;
                Kid::Glue { open: (tag.start, tag.end), lead: self.lead_of(tag), end: tag.end, scope }
            }
            (Ns::Text, "soft-page-break") => {
                if !self.stack.last().is_some_and(|f| matches!(f.role, Role::Shape(_)) || f.text_box) {
                    return Ok(());
                }
                Kid::Break { lead: self.lead_of(tag), end: tag.end }
            }
            (Ns::Text, "list-item" | "list-header") => {
                let lead = self.lead_of(tag);
                Kid::Item { lead, end: tag.end, children: 0, header: local == "list-header" }
            }
            (Ns::Text, "list") => {
                // The outermost list of the shape's text is a kid; a list in it is inside it.
                if frame.list.is_some() {
                    return Ok(());
                }
                let lead = self.lead_of(tag);
                frame.list = Some(self.scan.kids.len());
                Kid::List { lead, end: tag.end }
            }
            _ => return Ok(()),
        };
        frame.kid = Some(self.scan.kids.len());
        keep(&mut self.scan.kids, kid, "memory for what is inside the shapes of a part")
    }

    fn open_shape(&mut self, tag: &Tag<'_>, kind: Kind, frame: &mut Frame) -> Result<(), WriteError> {
        let scope = self.take_scope()?;
        let lead = match tag.text_before {
            Some((from, to)) if is_blank(self.bytes.get(from..to).unwrap_or_default()) => from,
            _ => tag.start,
        };
        let page_at = self.scan.pages.len().saturating_sub(1);
        let Some(page) = self.scan.pages.last_mut() else { return Ok(()) };
        let shape =
            ScanShape { kids: (0, 0), kind, open: (tag.start, tag.end), close_start: tag.end, end: tag.end, lead, parent: frame.shape, scope };
        keep(&mut page.shapes, shape, "memory for the shape elements of a part")?;
        frame.shape = u32::try_from(page.shapes.len()).unwrap_or(u32::MAX);
        frame.counting = kind.holds_shapes();
        frame.numbered = Some(page_at);
        if !kind.holds_shapes() && self.ask.inside.contains(&(page_at, frame.shape)) {
            self.inside = Some(((page_at, frame.shape), u32::try_from(self.scan.kids.len()).unwrap_or(u32::MAX)));
        }
        Ok(())
    }

    /// Notes each `xlink:href` of the tag: where it is and what holds it.
    fn note_hrefs(&mut self, frame: &Frame, ns: Ns, local: &str) -> Result<(), WriteError> {
        let holder = match (ns, local) {
            (Ns::Draw, "object") => Holder::Object,
            (Ns::Draw, "object-ole") => Holder::Ole,
            (Ns::Draw, "image") => Holder::Image,
            (Ns::Draw, "fill-image" | "stroke-image") | (Ns::Style, "background-image") | (Ns::Text, "list-level-style-image") => Holder::StyleImage,
            _ => Holder::Other,
        };
        for i in 0..self.spans.len() {
            let Some(a) = self.spans.get(i).copied() else { break };
            let name = String::from_utf8_lossy(self.bytes.get(a.name.0..a.name.1).unwrap_or_default());
            if name.starts_with("xmlns") || self.ns_of(&name, true) != (Ns::Xlink, "href") {
                continue;
            }
            if self.scan.hrefs.len() >= MOST_REFERENCES {
                return Err(WriteError::Limit { what: "href attributes in a part", max: MOST_REFERENCES as u64 });
            }
            let page = (frame.shape != 0).then(|| u32::try_from(self.scan.pages.len().saturating_sub(1)).unwrap_or(u32::MAX));
            let href = crate::package::unescape(self.bytes.get(a.value.0..a.value.1).unwrap_or_default());
            keep(&mut self.scan.hrefs, HrefAt { page, shape: frame.shape, holder, href }, "memory for the references of a part")?;
        }
        Ok(())
    }

    /// The end of the properties element just closed: it is the last one opened under the style or the layout that holds it.
    fn close_props(&mut self, kind: usize, end: usize, empty: bool) {
        let props = if kind == 3 {
            self.scan.layouts.last_mut().and_then(|l| l.props.as_mut())
        } else {
            self.scan.defs.last_mut().and_then(|d| d.props.get_mut(kind)).and_then(Option::as_mut)
        };
        if let Some(p) = props
            && p.end <= p.open.1
        {
            (p.end, p.empty) = (end, empty);
        }
    }

    fn close(&mut self, tag: &Tag<'_>) {
        let Some(frame) = self.stack.pop() else { return };
        // Undo the declarations the element made: the innermost last.
        for prefix in frame.declared.iter().rev() {
            self.unbind(prefix);
        }
        let (close_start, end) = if tag.kind == TagKind::Empty { (tag.end, tag.end) } else { (tag.start, tag.end) };
        let empty = tag.kind == TagKind::Empty;
        if frame.leading
            && let Some(page) = self.scan.pages.last_mut()
            && page.shapes.is_empty()
        {
            page.first_place = Some(end);
        }
        match frame.role {
            Role::Page => {
                if let Some(page) = self.scan.pages.last_mut() {
                    page.close_start = close_start;
                    page.end = end;
                    page.empty = empty;
                }
            }
            Role::Shape(_) => {
                if let Some(page) = frame.numbered.and_then(|at| self.scan.pages.get_mut(at))
                    && let Some(shape) = page.shapes.get_mut((frame.shape as usize).saturating_sub(1))
                {
                    shape.close_start = close_start;
                    shape.end = end;
                    if let Some((key, from)) = self.inside
                        && Some(key.0) == frame.numbered
                        && key.1 == frame.shape
                    {
                        shape.kids = (from, u32::try_from(self.scan.kids.len()).unwrap_or(u32::MAX));
                        self.inside = None;
                    }
                }
            }
            Role::Common => {
                if let Some(common) = self.scan.common.as_mut() {
                    (common.close_start, common.end, common.empty) = (close_start, end, empty);
                }
            }
            // The first of each, whose start tag the scan noted: a later one closing leaves it alone.
            Role::MasterStyles => {
                if let Some(held) = self.scan.master_styles.as_mut().filter(|h| h.close_start == h.open.1 && !h.empty && h.end == h.open.1) {
                    (held.close_start, held.end, held.empty) = (close_start, end, empty);
                }
            }
            Role::LayerSet => {
                if let Some(held) = self.scan.layer_set.as_mut().filter(|h| h.close_start == h.open.1 && !h.empty && h.end == h.open.1) {
                    (held.close_start, held.end, held.empty) = (close_start, end, empty);
                }
            }
            Role::Annotation => {
                let at = self.scan.pages.len().saturating_sub(1);
                if self.ask.annotation_pages.contains(&at)
                    && let Some(last) = self.scan.pages.last_mut().and_then(|p| p.annotations.last_mut())
                {
                    last.1 = end;
                }
            }
            // The page's first layer set is closed: a later one's layers are no rows of the page.
            Role::PageLayerSet => {
                if let Some(page) = self.scan.pages.last_mut()
                    && page.layer_sets == 1
                {
                    page.layer_sets = 2;
                }
            }
            Role::FontFaces => {
                if let Some(fonts) = self.scan.fonts.as_mut() {
                    (fonts.close_start, fonts.end, fonts.empty) = (close_start, end, empty);
                }
            }
            Role::AutoStyles => {
                if let Some(auto) = self.scan.auto.as_mut() {
                    auto.close_start = close_start;
                    auto.end = end;
                    auto.empty = empty;
                }
            }
            Role::Style => {
                if let Some(def) = frame.def.and_then(|at| self.scan.defs.get_mut(at)) {
                    (def.close_start, def.end, def.empty) = (close_start, end, empty);
                }
            }
            Role::Layout => {
                if let Some(layout) = frame.def.and_then(|at| self.scan.layouts.get_mut(at)) {
                    (layout.end, layout.empty) = (end, empty);
                }
            }
            Role::Props(kind) => self.close_props(kind, end, empty),
            Role::Marker => {
                if let Some(marker) = self.scan.markers.last_mut() {
                    marker.end = end;
                }
            }
            Role::CommonStyle => {
                if let Some(style) = self.scan.common_styles.last_mut() {
                    style.end = end;
                }
            }
            _ => {}
        }
        match frame.kid.and_then(|at| self.scan.kids.get_mut(at)) {
            Some(Kid::Geometry { empty: e, end: en, .. }) => (*e, *en) = (empty, end),
            Some(Kid::Para { empty: e, close_start: cs, end: en, .. }) => (*e, *cs, *en) = (empty, close_start, end),
            Some(Kid::Item { end: en, .. } | Kid::List { end: en, .. } | Kid::Break { end: en, .. } | Kid::Glue { end: en, .. }) => *en = end,
            Some(Kid::Alt { plain, end: en, content, .. }) => {
                *en = end;
                content.1 = close_start;
                // An element written empty has no text, and is no row.
                *plain &= !empty;
            }
            None => {}
        }
    }
}

#[cfg(test)]
impl Scopes {
    /// A scope for a tag that uses these prefixes (each with the namespace it stands for) and sees these least prefixes bound to ODF's drawing
    /// and svg namespaces; and its number.
    pub(crate) fn with_scope(prefixes: &[(&str, Ns)], draw: Option<&str>, svg: Option<&str>) -> (Scopes, u32) {
        let mut scopes = Scopes::default();
        let snapshot = Snapshot {
            prefixes: prefixes.iter().map(|(prefix, ns)| ((*prefix).to_owned(), (*ns, false))).collect(),
            odf: [draw.map(str::to_owned), svg.map(str::to_owned), None, None, None, None],
        };
        scopes.snapshots.push(snapshot);
        (scopes, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_that_does_not_scan_is_named_in_the_error() {
        for (kind, name) in [(PartKind::Content, "content.xml"), (PartKind::Styles, "styles.xml")] {
            let failed = scan_part(b"<a><b></a>", kind, &Limits::default(), &mut WriteWork::default());
            match failed {
                Err(WriteError::Original(crate::OdgError::Xml { part, .. })) => assert_eq!(part, name),
                other => panic!("{name}: {:?}", other.map(|_| ())),
            }
        }
    }

    #[test]
    fn the_bytes_of_each_start_tag_are_charged_as_looked_at() {
        let part = b"<a><b x=\"1\"/><c></c></a>";
        let mut work = WriteWork::default();
        assert!(scan_part(part, PartKind::Content, &Limits::default(), &mut work).is_ok());
        // `<a>` 3, `<b x="1"/>` 10 and `<c>` 3: the end tags are not looked at for attributes.
        assert_eq!(work.attribute_bytes, 16);
        assert_eq!(work.scan_bytes, part.len() as u64);
        assert_eq!(work.elements_walked, 3);
    }

    /// The extension's own prefix is written as it is where the part declares none of that name, and where it does the part gets another that is free; the
    /// answer is kept for the part.
    #[test]
    fn the_extensions_prefix_is_kept_where_the_part_declares_none_and_another_is_made_where_it_does() {
        let prefix = crate::ext::names::PREFIX;
        let mut work = WriteWork::default();
        let mut free = Scopes::default();
        assert_eq!(free.fresh_prefix(prefix, &mut work), prefix);
        assert_eq!(free.fresh_prefix(prefix, &mut work), prefix, "asked again, the same");
        let mut taken = Scopes::default();
        taken.declared.insert(prefix.to_owned());
        assert_eq!(taken.fresh_prefix(prefix, &mut work), format!("{prefix}1"));
        // A prefix of another base always gets its number.
        assert_eq!(Scopes::default().fresh_prefix("svg", &mut work), "svg1");
    }

    const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

    fn content(version: &str, page: &str) -> String {
        format!(
            "<office:document-content xmlns:office=\"{OFFICE}\" xmlns:draw=\"{DRAW_URI}\"{version}><office:body><office:drawing>{page}</office:drawing></office:body></office:document-content>"
        )
    }

    #[test]
    fn a_version_before_1_2_is_legacy_and_so_is_none_and_what_is_no_version() {
        for legacy in [None, Some("1.0"), Some("1.1"), Some(" 1.1 "), Some("1"), Some("0.9"), Some("one"), Some(""), Some("1.x")] {
            assert!(is_legacy(legacy), "{legacy:?}");
        }
        for current in ["1.2", "1.3", "1.4", "1.10", "2.0", "2"] {
            assert!(!is_legacy(Some(current)), "{current}");
        }
    }

    #[test]
    fn the_scan_gives_the_version_the_root_declares_and_where_a_page_that_is_an_empty_element_ends() {
        let scan = |text: &str| {
            scan_part_asking(
                text.as_bytes(),
                PartKind::Content,
                &Ask { styles: true, ..Ask::default() },
                &Limits::default(),
                &mut WriteWork::default(),
            )
            .unwrap()
        };
        let pages = "<draw:page draw:name=\"a\"/><draw:page draw:name=\"b\"></draw:page><draw:page draw:name=\"c\"> </draw:page>";
        let whole = content(" office:version=\"1.1\"", pages);
        let found = scan(&whole);
        assert_eq!(found.version.as_deref(), Some("1.1"));
        let pages: Vec<(bool, &str)> = found.pages.iter().map(|p| (p.empty, p.qname.as_str())).collect();
        assert_eq!(pages, [(true, "draw:page"), (false, "draw:page"), (false, "draw:page")]);
        // The empty page's end is its start tag's: nothing is inside it.
        let first = &found.pages[0];
        assert_eq!((first.open.1, first.close_start, first.end), (first.end, first.end, first.end));
        assert_eq!(scan(&content("", "<draw:page/>")).version, None, "no version declared");
        // The body is where an element that is missing goes: its start, its name, and whether its own tag declares a namespace.
        let body = found.body.as_ref().unwrap();
        assert_eq!(body.qname, "office:body");
        assert!(!body.declares);
        assert_eq!(body.start, whole.find("<office:body").unwrap());
        let text = content("", "");
        let declaring = text.replace("<office:body>", "<office:body xmlns:x=\"urn:x\">");
        assert!(scan(&declaring).body.unwrap().declares);
        // A part that holds the element has none to make.
        let with = text.replace("<office:body>", "<office:automatic-styles/><office:body>");
        let scanned = scan(&with);
        assert!(scanned.auto.is_some() && !scanned.auto.unwrap().made);
    }

    #[test]
    fn the_font_faces_are_scanned_only_when_the_styles_are_asked_for() {
        let part = format!(
            "<office:document-content xmlns:office=\"{OFFICE}\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"><office:font-face-decls><style:font-face style:name=\"Mono\" svg:font-family=\"&apos;DejaVu Sans Mono&apos;\"/></office:font-face-decls></office:document-content>"
        );
        let limits = Limits::default();
        let plain = scan_part(part.as_bytes(), PartKind::Content, &limits, &mut WriteWork::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(plain.faces.is_empty(), "no faces were asked for");
        let ask = Ask { styles: true, ..Ask::default() };
        let asked = scan_part_asking(part.as_bytes(), PartKind::Content, &ask, &limits, &mut WriteWork::default()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(asked.faces.len(), 1);
        assert_eq!(
            (asked.faces[0].name.as_str(), asked.faces[0].family.as_str()),
            ("Mono", "DejaVu Sans Mono"),
            "the quotes are taken off the family"
        );
    }

    /// The bindings a new shape's kept markup is written against are those in force on the element that holds the pages (the first `office:drawing`: the root's declarations under the
    /// containers'), with each page's own declarations beside them; a declaration that another element of the part makes (a style, a page's kept child) is in neither, though the part
    /// declares the prefix somewhere (the scan answered `true` for any prefix declared anywhere, and a shape's kept name was written unbound under it).
    #[test]
    fn the_holder_s_bindings_and_each_page_s_own_are_what_a_new_shape_is_written_against() {
        let part = format!(
            "<o:document-content xmlns:o=\"{OFFICE}\" xmlns:draw=\"{DRAW_URI}\" xmlns:my=\"urn:one\" xmlns:gone=\"urn:g\"><o:automatic-styles><x:s xmlns:x=\"urn:x\"/></o:automatic-styles><o:body xmlns:b=\"urn:b\"><o:drawing xmlns:gone=\"\"><draw:page draw:name=\"p1\" xmlns:my=\"urn:two\"><sib:e xmlns:sib=\"urn:sib\"/></draw:page><draw:page draw:name=\"p2\"/></o:drawing><o:drawing xmlns:late=\"urn:late\"><draw:page draw:name=\"p3\"/></o:drawing></o:body></o:document-content>"
        );
        let model = || WriteWork { model_path: true, ..WriteWork::default() };
        let mut work = model();
        let scan = scan_part(part.as_bytes(), PartKind::Content, &Limits::default(), &mut work).unwrap_or_else(|e| panic!("{e}"));
        let s = &scan.scopes;
        assert_eq!(s.holder_binding("my"), Some("urn:one"), "the root's, not the first page's");
        assert_eq!(s.holder_binding("b"), Some("urn:b"), "a container's");
        assert_eq!(s.holder_binding("x"), None, "a style's own declaration binds nothing where the pages are");
        assert_eq!(s.holder_binding("sib"), None, "nor does a page's kept child's");
        assert_eq!(s.holder_binding("gone"), None, "a binding undone on the way is none");
        assert_eq!(s.holder_binding("late"), None, "only the first drawing's");
        assert_eq!(s.holder_binding(""), Some(""), "no default namespace is the empty name");
        assert_eq!(s.holder_binding("xml"), Some(super::super::kept::XML_URI));
        assert!(s.declared.contains("sib") && s.declared.contains("x"), "the part declares both somewhere");
        assert_eq!(scan.pages[0].ns, Some(vec![("my".to_owned(), "urn:two".to_owned())]), "the first page's own");
        assert_eq!(scan.pages[1].ns, Some(Vec::new()));
        assert_eq!(scan.pages[2].ns, None, "a page of another drawing is written against nothing the scan took");
        // In `styles.xml` the master styles hold the pages.
        let styles = format!(
            "<o:document-styles xmlns:o=\"{OFFICE}\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:my=\"urn:one\"><o:master-styles xmlns:m=\"urn:m\"><style:master-page style:name=\"Default\"/></o:master-styles></o:document-styles>"
        );
        let scan = scan_part(styles.as_bytes(), PartKind::Styles, &Limits::default(), &mut model()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((scan.scopes.holder_binding("m"), scan.scopes.holder_binding("my")), (Some("urn:m"), Some("urn:one")));
        assert_eq!(scan.pages[0].ns, Some(Vec::new()));
        // A part with no element that holds pages has no bindings to write against.
        let bare = format!("<o:document-content xmlns:o=\"{OFFICE}\" xmlns:my=\"urn:one\"/>");
        let scan = scan_part(bare.as_bytes(), PartKind::Content, &Limits::default(), &mut model()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((scan.scopes.holder_binding("my"), scan.scopes.holder_binding("")), (None, None));
        // A save into a file takes none of it: its scan costs what it did.
        let mut plain = WriteWork::default();
        let scan = scan_part(part.as_bytes(), PartKind::Content, &Limits::default(), &mut plain).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((scan.scopes.holder_binding("my"), scan.pages[0].ns.as_ref()), (None, None));
        assert!(plain.scope_steps < work.scope_steps, "{} steps, and {} on the model path", plain.scope_steps, work.scope_steps);
    }

    /// Taking the holder's bindings is a step for each prefix in force, charged: a part of many declarations above its pages is refused at the step cap, not walked past it.
    #[test]
    fn the_holder_s_bindings_are_charged_a_step_each() {
        let many: String = (0..2_000).map(|k| format!(" xmlns:p{k}=\"urn:{k}\"")).collect();
        let part = format!("<o:document-content xmlns:o=\"{OFFICE}\"{many}><o:body><o:drawing/></o:body></o:document-content>");
        let mut work = WriteWork { model_path: true, ..WriteWork::default() };
        let _ = scan_part(part.as_bytes(), PartKind::Content, &Limits::default(), &mut work).unwrap_or_else(|e| panic!("{e}"));
        let counted = work.scope_steps;
        // Two for each of the 2,001 declarations (made and undone), and one for each binding in force on the drawing.
        assert!(counted >= 2 * 2_001 + 2_001, "a step for each binding in force on the drawing, beside the declarations' own: {counted}");
        // A cap the declarations pass and the holder's bindings do not.
        let mut tight = WriteWork { step_cap: 5_000, model_path: true, ..WriteWork::default() };
        assert!(matches!(scan_part(part.as_bytes(), PartKind::Content, &Limits::default(), &mut tight), Err(WriteError::Limit { .. })));
        let mut file = WriteWork { step_cap: 5_000, ..WriteWork::default() };
        assert!(scan_part(part.as_bytes(), PartKind::Content, &Limits::default(), &mut file).is_ok(), "a save into a file takes no holder");
    }
}
