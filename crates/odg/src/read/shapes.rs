//! The shapes of a page.
//!
//! **Which elements are shapes, and which are modelled.** A page's drawing shapes are numbered in document order, pre-order (see
//! `index.rs`). This reader models the plain shapes (`draw:rect`, `draw:ellipse`, `draw:circle`, `draw:line`, `draw:polyline`,
//! `draw:polygon`, `draw:path`, `draw:regular-polygon`), frames holding a `draw:text-box` or a `draw:image`, custom shapes (`custom.rs`),
//! connectors (`connectors.rs`), groups with their members inside them, and the objects it cannot draw (frames holding an
//! object, `draw:measure`, `draw:caption`, `draw:control`, `dr3d:scene`, and custom shapes with extrusion or fontwork), each a `Foreign` shape
//! on its frame, drawn from its replacement image when it has one and counted by kind. Only a hyperlink wrapper (`draw:a`) is still numbered
//! and kept whole in the page's passthrough as the file has it, and counted by kind; its child shape is numbered and kept with it.
//!
//! **A shape's cells**: its frame, its geometry (`geometry.rs`), the four standard connection
//! rows, its layer, its names, its style links and local cells (of
//! the defaults), and its text (`text.rs`). Every attribute the reader does not map is kept in the shape's `raw`, and every child it does
//! not read in its `passthrough`, as the file has them.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{
    Cell, CellName, ForeignData, Kept, Links, Media, Passthrough, RawAttrs, RawElement, RawNode, Row, RowKey, Section, Shape, ShapeType, Sheet,
};

use super::Reader;
use super::cells::{self, Forms};
use super::connectors;
use super::custom;
use super::geometry::{self, ArcAxes, Budget};
use super::held;
use super::layers;
use super::names::{Names, Ns};
use super::props::{self, Out, Serves, View};
use super::sheets::{self, Sheets};
use super::styles::{Family, File, Kind, P, Resolved, Resolver, StyleSet};
use super::text;
use super::texts::StyleTexts;
use super::units::{self, LenUnit, Length};
use crate::error::{LimitKind, OdgError};
use crate::ext::docfile::Binding;
use crate::index::{PageShapes, PageSource};
use crate::package::{Package, percent_decode};

/// What reading the shapes of one part needs.
pub(crate) struct Cx<'a> {
    pub set: &'a StyleSet,
    pub res: &'a mut Resolver,
    pub sheets: &'a Sheets,
    /// The package the drawing was read from, whose parts the pictures' hrefs name; `None` for a flat drawing, which has no parts: its pictures are
    /// inline or linked.
    pub package: Option<&'a Package>,
    pub file: File,
    /// Each shape style worked out, by its graphic and paragraph style.
    pub styles: HashMap<(Option<usize>, Option<usize>, bool), Rc<ShapeStyle>>,
    /// Sections that are the same for many shapes, built once and shared (the model's sections are behind `Arc`): a rectangle's geometry,
    /// and the connection rows of each size (at most [`MOST_SHARED`] sizes).
    pub shared: &'a mut Shared,
    /// The values of the style set's texts shown in cells, for as long as `set` is borrowed (`texts.rs`).
    pub texts: StyleTexts<'a>,
}

/// The size of a shape's frame and the units of its `Width` and `Height`: the key its connection rows are shared by.
type SizeKey = (u64, u64, Option<&'static str>, Option<&'static str>);

/// What shapes share, in both parts: sections that are the same for many shapes, and each picture part's bytes.
#[derive(Default)]
pub(crate) struct Shared {
    rectangle: Option<Arc<Section>>,
    connections: HashMap<SizeKey, Arc<Section>>,
    /// Each picture part a frame has named, by its name: its media, made once and shared by every frame that names it, or `None` when it
    /// did not fit the media budget (the budget only shrinks, so a later frame would not fit it either).
    pictures: HashMap<String, Option<Arc<Media>>>,
    /// Short lists of attributes that a held value repeats (a layer's name on every shape), by their text: one list behind a pointer for every shape that holds it.
    attr_lists: HashMap<String, RawAttrs>,
    /// The names of the automatic styles shapes named, shared.
    style_names: HashMap<String, Arc<str>>,
    /// Short kept elements of a plain shape's outline, by their text, shared.
    kept_lists: HashMap<String, Passthrough>,
}

impl Shared {
    pub(super) fn attr_lists(&self) -> &HashMap<String, RawAttrs> {
        &self.attr_lists
    }

    /// Keeps an interned list, while there is room for it (the most [`MOST_SHARED`]).
    pub(super) fn keep_attr_list(&mut self, key: String, list: &RawAttrs) {
        if self.attr_lists.len() < MOST_SHARED {
            self.attr_lists.insert(key, list.clone());
        }
    }

    pub(super) fn kept_list(&self, key: &str) -> Option<Passthrough> {
        self.kept_lists.get(key).cloned()
    }

    pub(super) fn keep_kept_list(&mut self, key: String, list: &Passthrough) {
        if self.kept_lists.len() < MOST_SHARED {
            self.kept_lists.insert(key, list.clone());
        }
    }

    pub(super) fn style_name(&self, name: &str) -> Option<Arc<str>> {
        self.style_names.get(name).cloned()
    }

    pub(super) fn keep_style_name(&mut self, name: &Arc<str>) {
        if self.style_names.len() < MOST_SHARED && name.len() <= MOST_INTERNED_BYTES {
            self.style_names.insert(name.to_string(), Arc::clone(name));
        }
    }
}

impl Shared {
    /// The names of the package entries a frame's picture was made of: the entries the model holds as pictures, and so not as parts of the package.
    pub(super) fn picture_entries(&self) -> impl Iterator<Item = &str> {
        self.pictures.iter().filter(|(_, made)| made.is_some()).map(|(name, _)| name.as_str())
    }
}

/// The most sizes whose connection rows are kept for sharing.
pub(super) const MOST_SHARED: usize = 4096;

/// The most bytes a list of attributes or a name has to be shared by the pointer behind it.
pub(super) const MOST_INTERNED_BYTES: usize = 256;

/// What reading one page needs.
pub(crate) struct PageCx {
    /// The page's height, inches, which every shape's y is flipped against.
    pub height: f64,
    /// The page's layer rows by name: each name's row.
    pub layers: HashMap<String, u32>,
    /// What the page may still spend on path commands.
    pub budget: Budget,
    /// What DiaCraft's document file says of the page's groups, by the group's `draw:name` and its number among the groups of that name on the page.
    pub groups: crate::ext::docfile::PageGroups,
    /// The stamps (the names of the members, [`crate::ext::items::group_fp`]) that more than one group with no name of the page has: an item of VSDX data an
    /// unnamed group's key finds, whose stamp is one of these, cannot be told to be that group's and not another's that another program put in its place, so it is left out with a note.
    pub ambiguous: HashSet<crate::ext::fingerprint::Fp>,
    /// The page's annotation children, and the ones the model took as comments.
    pub annotations: super::comments::PageAnnotations,
}

/// What reading a page's shapes gives.
pub(crate) struct PageShapesRead {
    /// The page's `Connect` rows: each attached end of each connector.
    pub connects: Vec<diacraft_doc::Connect>,
    /// The modelled top-level shapes, in drawing order.
    pub shapes: Vec<Arc<Shape>>,
    /// The page's children kept unmodelled, each after as many modelled top-level shapes as came before it in document order.
    pub passthrough: Passthrough,
    pub index: PageShapes,
    /// The page's annotations the model took, in place of the markup.
    pub annotations: super::comments::PageAnnotations,
}

/// A shape's links and the cells its styles give it locally, worked out once for each pair of graphic and paragraph style.
pub(crate) struct ShapeStyle {
    links: Links,
    local: Out,
    /// The chain its text reads through, for its paragraphs and runs.
    text_layers: Vec<(Rc<Resolved>, Serves)>,
    text_size: f64,
    /// Whether the shape draws a fill and a line (`FillPattern` and `LinePattern` not 0): what `hasfill` and `hasstroke` of a custom shape's
    /// equations read, worked out once with the style (a shape reads it, and walks no chain of its own).
    patterns: (bool, bool),
}

/// The kinds of drawing shape element, and what this reader does with each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kindof {
    Rect,
    Ellipse,
    Circle,
    Line,
    Polyline,
    Polygon,
    Path,
    RegularPolygon,
    Frame,
    /// `draw:custom-shape`.
    Custom,
    /// `draw:connector`.
    Connector,
    /// `draw:g`.
    Group,
    /// An object DiaCraft cannot draw, read as a `Foreign` shape on its frame: `draw:measure`, `draw:caption`, `draw:control`,
    /// `dr3d:scene`; the word it is counted under in the report.
    Object(&'static str),
    /// Numbered and kept unmodelled: the word it is counted under.
    Kept(&'static str),
}

/// The kind of drawing shape `el` is, or `None` for an element that is not one.
fn shape_kind<'t>(names: &Names<'t>, el: &'t El) -> Option<Kindof> {
    Some(match names.element(el) {
        (Ns::Draw, "rect") => Kindof::Rect,
        (Ns::Draw, "ellipse") => Kindof::Ellipse,
        (Ns::Draw, "circle") => Kindof::Circle,
        (Ns::Draw, "line") => Kindof::Line,
        (Ns::Draw, "polyline") => Kindof::Polyline,
        (Ns::Draw, "polygon") => Kindof::Polygon,
        (Ns::Draw, "path") => Kindof::Path,
        (Ns::Draw, "regular-polygon") => Kindof::RegularPolygon,
        (Ns::Draw, "frame") => Kindof::Frame,
        (Ns::Draw, "custom-shape") => Kindof::Custom,
        (Ns::Draw, "connector") => Kindof::Connector,
        (Ns::Draw, "g") => Kindof::Group,
        (Ns::Draw, "measure") => Kindof::Object("measure"),
        (Ns::Draw, "caption") => Kindof::Object("caption"),
        (Ns::Draw, "control") => Kindof::Object("control"),
        (Ns::Dr3d, "scene") => Kindof::Object("scene_3d"),
        (Ns::Draw, "a") => Kindof::Kept("hyperlink_wrapper"),
        _ => return None,
    })
}

/// Whether the shape elements inside `el` are numbered as shapes of the page too: a group's members, and a hyperlink wrapper's child.
fn holds_shapes(kind: Kindof) -> bool {
    matches!(kind, Kindof::Group | Kindof::Kept("hyperlink_wrapper"))
}

/// The children of a frame that make it an object this reader does not draw.
fn is_object<'t>(names: &Names<'t>, el: &'t El) -> bool {
    matches!(names.element(el), (Ns::Draw, "object" | "object-ole" | "applet" | "plugin" | "floating-frame") | (Ns::Table, "table"))
}

/// Whether `el`, a `draw:frame` the caller has entered, holds an object: the frame of an object that a text frame holds in its text box. The element is no shape of
/// the page and the model keeps none of it, so the shape that holds it is marked ([`TEXT_OBJECT_ROW`]).
pub(super) fn frame_holds_object<'t>(names: &mut Names<'t>, el: &'t El) -> bool {
    if !names.is(el, Ns::Draw, "frame") {
        return false;
    }
    el.elements().any(|child| {
        let mark = names.enter(child);
        let found = is_object(names, child);
        names.leave(mark);
        found
    })
}

/// The name of the `User` row of a shape whose text holds the frame of an object (a copy of the shape could not carry the object, which the model keeps no part of): its
/// `Value` says what is in the text. The row is the model's, like `ODFObject` is: the file has no place for it and needs none.
pub const TEXT_OBJECT_ROW: &str = "ODFTextObject";

/// What an object with no picture is called on the box that stands for it (the words), by the word it is counted under. An
/// extruded custom shape is called what is nearest in the list (a 3-D scene); `frame_object` is a frame's, which [`frame_object_label`] tells.
fn object_label(word: &str) -> &'static str {
    match word {
        "measure" => "Dimension line",
        "caption" => "Callout",
        "control" => "Form control",
        "scene_3d" | "extrusion" => "3-D scene",
        "fontwork" => "Fontwork",
        _ => "Embedded object",
    }
}

/// What the object a frame holds is called: a table when the first of its objects is one (`table:table`), an embedded object otherwise. Called before any child's
/// namespaces are in scope, as [`frame_shows`] is.
fn frame_object_label<'t>(names: &mut Names<'t>, el: &'t El) -> &'static str {
    for child in el.elements() {
        let mark = names.enter(child);
        let table = names.is(child, Ns::Table, "table");
        let object = is_object(names, child);
        names.leave(mark);
        if table {
            return "Table";
        }
        if object {
            break;
        }
    }
    "Embedded object"
}

/// Counts an object DiaCraft does not draw (`object`, the word it is counted under) among what is kept unmodelled, and, for one of the kinds kept only as a placeholder (`kind`), among
/// what the drawing does not hold: a copy written from the model writes its placeholder as a plain shape, so the open names it. A frame's object is a frame
/// still, and comes back as one.
fn count_object(r: &mut Reader, kind: Kindof, object: Option<&'static str>) {
    if let Some(word) = object {
        r.keep(word);
    }
    if matches!(kind, Kindof::Object(_)) {
        r.not_kept("element_kind", 1);
    }
}

/// An object with no picture to show says what it is, for the placeholder the renderer draws from it: a replacement image the
/// package does not hold is no picture either. `object` is the word the object is counted under, `frame_label` what a frame's object is called.
fn mark_object(
    r: &mut Reader,
    sheet: &mut Sheet,
    shape: &Shape,
    object: Option<&'static str>,
    frame_label: Option<&'static str>,
) -> Result<(), OdgError> {
    let Some(word) = object else { return Ok(()) };
    if shape.foreign.as_ref().is_some_and(|f| f.picture.is_some()) {
        return Ok(());
    }
    r.take_items(super::cost::SECTION + super::cost::ROW)?;
    r.take_cells(1)?;
    r.report.object_placeholders = r.report.object_placeholders.saturating_add(1);
    put_object_marker(sheet, frame_label.unwrap_or_else(|| object_label(word)));
    Ok(())
}

/// A text that holds the frame of an object (`holds`): the model keeps nothing of the frame, so the shape says it does, in the row [`TEXT_OBJECT_ROW`]. The
/// row is charged as a section and a row and a cell are, before it is made.
fn mark_text_object(r: &mut Reader, sheet: &mut Sheet, holds: bool) -> Result<(), OdgError> {
    if holds {
        r.take_items(super::cost::SECTION + super::cost::ROW)?;
        r.take_cells(1)?;
        put_marker_row(sheet, TEXT_OBJECT_ROW, "Object in text");
    }
    Ok(())
}

/// The `User` row `ODFObject` of an object with no picture: its `Value` holds the words that name it, which the renderer draws in the box that stands for it
/// The row goes into the shape's `User` section when it has one.
fn put_object_marker(sheet: &mut Sheet, label: &str) {
    put_marker_row(sheet, "ODFObject", label);
}

/// The `User` row `name` with `label` as its `Value`, in the shape's `User` section when it has one.
fn put_marker_row(sheet: &mut Sheet, name: &str, label: &str) {
    let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
    row.cells.insert(CellName::new("Value"), cells::text(label));
    let row = Arc::new(row);
    match sheet.sections.iter_mut().find(|s| s.name.as_str() == "User" && s.ix.is_none()) {
        Some(section) => Arc::make_mut(section).rows.push(row),
        None => {
            let mut section = Section::new("User", None);
            section.rows.push(row);
            sheet.sections.push(Arc::new(section));
        }
    }
}

/// What a frame shows: the first child this reader supports, or an object it keeps.
#[derive(Clone, Copy)]
enum FrameShows<'t> {
    Text(&'t El),
    Image(&'t El),
    Object,
}

impl<'t> FrameShows<'t> {
    /// The child shown, when there is one.
    fn child(self) -> Option<&'t El> {
        match self {
            FrameShows::Text(el) | FrameShows::Image(el) => Some(el),
            FrameShows::Object => None,
        }
    }
}

/// The first child DiaCraft supports, a `draw:text-box` or a `draw:image` of a format it draws by its signature; failing
/// those, the first `draw:image` of another format, which draws as a placeholder. A frame holding an object is the object.
fn frame_shows<'t>(names: &mut Names<'t>, package: Option<&Package>, el: &'t El) -> FrameShows<'t> {
    let mut first = None;
    let mut first_image = None;
    let mut object = false;
    for child in el.elements() {
        let mark = names.enter(child);
        if is_object(names, child) {
            object = true;
        } else if first.is_none() {
            if names.is(child, Ns::Draw, "text-box") {
                first = Some(FrameShows::Text(child));
            } else if names.is(child, Ns::Draw, "image") {
                if image_supported(names, package, child) {
                    first = Some(FrameShows::Image(child));
                } else if first_image.is_none() {
                    first_image = Some(child);
                }
            }
        }
        names.leave(mark);
    }
    match (object, first.or(first_image.map(FrameShows::Image))) {
        (false, Some(shows)) => shows,
        _ => FrameShows::Object,
    }
}

/// Whether a `draw:image` holds a picture of a format DiaCraft draws, by the signature of its first bytes: the part's, or the first
/// characters of its inline data decoded. A part that is not there is not supported.
fn image_supported<'t>(names: &Names<'t>, package: Option<&Package>, image: &'t El) -> bool {
    /// The bytes a signature needs: an EMF's is at 40 to 44.
    const HEAD: usize = 44;
    let data = image.elements().find(|c| names.is(c, Ns::Office, "binary-data"));
    // A flat drawing's picture is its inline data when it has some, and an `xlink:href` beside the data is ignored; one with none
    // is a linked picture, which is not read, so no picture is shown. A package's `draw:image` follows its href when it has one.
    let href = names.attrs(image).peek(Ns::Xlink, "href").filter(|_| package.is_some() || data.is_none());
    match href {
        Some(href) => package
            .and_then(|package| resolve_href(package, href).and_then(|n| package.part(&n)))
            .is_some_and(|b| signature(b.get(..HEAD).unwrap_or(b)).1.is_some()),
        None => {
            let Some(data) = data else { return false };
            // Four characters give three bytes: sixty characters are enough.
            let head: String = data
                .children
                .iter()
                .filter_map(|n| if let Node::Text(t) | Node::CData(t) = n { Some(t.as_str()) } else { None })
                .flat_map(str::chars)
                .filter(|c| !c.is_ascii_whitespace())
                .take(HEAD.div_ceil(3) * 4)
                .collect();
            base64(&head).is_some_and(|b| signature(&b).1.is_some())
        }
    }
}

/// The diacraft id the automatic style of a shape carries, and whether it says the shape's rows start at 0 with its first listed glue point
/// (`diacraft:glue="own"`).
fn style_dia(r: &mut Reader, cx: &Cx<'_>, attrs: &super::names::Attrs<'_>) -> (Option<u32>, bool) {
    let Some(name) = attrs.peek(Ns::Draw, "style-name") else { return (None, false) };
    let Some(at) = cx.set.find(r, Family::Graphic, name, cx.file) else { return (None, false) };
    cx.set.get(at).filter(|s| !s.common).map_or((None, false), |s| (s.dia.id, s.dia.glue_own))
}

/// What pass 1 gives an element: its ordinal in the page's document order, its `ID`, whether it is a copy (its DiaCraft id is held by an
/// earlier shape, so it is read without DiaCraft's data), and whether the page models it (it is not a kept kind, and no ancestor is).
#[derive(Clone, Copy, Debug)]
struct Numbered {
    ordinal: u32,
    id: u32,
    copy: bool,
    kind: Kindof,
    modelled: bool,
    /// For a group: how it finds what DiaCraft's document file says of it: by its number among the groups of its `draw:name` on the page, by its name when
    /// no other group of the page has it, or not at all.
    bound: Binding,
}

/// A shape element met in pass 1's walk: its address, the `ID` it asks for, its kind, whether the page models it and, for a group, its `draw:name` and its number among
/// the groups of that name; and how it finds its entry in DiaCraft's document file, which is told when the walk is over.
struct Met<'t> {
    key: usize,
    id: Option<u32>,
    kind: Kindof,
    modelled: bool,
    group: Option<(&'t str, u32)>,
    bound: Binding,
    /// The group (its place in the walk's order) the shape is a member of, and the shape's `draw:name` (the stamps of the groups with no name).
    parent: Option<usize>,
    name: Option<&'t str>,
}

/// What pass 1 finds on a page: the numbering of each shape element by its address, and the shapes' ids for the connectors that name them.
struct Pass1<'t> {
    numbers: HashMap<usize, Numbered>,
    /// The element (by address) that has each `xml:id`, or a `draw:id` where it has no `xml:id`: the first of a name.
    names: HashMap<&'t str, usize>,
    /// The elements (by address) a connector may name whose standard glue points 1 and 3 are the model's rows 1 and 3 ([`glue_flipped`]).
    flipped: HashSet<usize>,
    /// The custom shapes (by address) a connector may name whose style says their rows start at 0 with the first listed glue point (`diacraft:glue="own"`).
    own: HashSet<usize>,
    /// The stamps more than one group with no name has ([`PageCx::ambiguous`]).
    ambiguous: HashSet<crate::ext::fingerprint::Fp>,
}

/// Whether the four standard glue points of a shape element are numbered across the model's frame the other way round: **its model has exactly one flip**
/// (`FlipX` and `FlipY` differ) and it is a kind LibreOffice 26.8.2.2 was seen to place them on (a custom shape, a rectangle, an ellipse or circle, a frame).
/// LibreOffice puts them at the middles of the sides of the frame **as it draws it, not mirrored**: its point 1 is the right side's middle of the frame on the page
/// and 3 the left's, whatever the mirrors, while the model's rows mirror with the shape. So beside one flip the model's row 1 (its own left side's middle, which the
/// flip takes to the right) is LibreOffice's point 1, and row 3 its point 3; beside none or two the fixed map of holds (connectors to
/// points 0 to 3 of custom shapes in each mirror state at 0, 30 and 135 degrees, and of rectangles, ellipses, text frames and pictures whose transform flips them, all
/// within 0.01 inch of where LibreOffice ends them; a polygon's points are the middles of the sides of the box of its own points, and keep the fixed map).
///
/// The flips are the reader's own: a negative determinant of `draw:transform` is a `FlipY`, and a custom shape's mirrors (`draw:mirror-horizontal`,
/// `draw:mirror-vertical`, unless the geometry is an object the reader does not draw) are `FlipX` and one more `FlipY` ([`custom_geometry`]). The walk has entered `el`.
fn glue_flipped<'t>(names: &mut Names<'t>, el: &'t El, kind: Kindof, attrs: &super::names::Attrs<'t>, terms: &mut usize) -> bool {
    if !matches!(kind, Kindof::Custom | Kindof::Rect | Kindof::Ellipse | Kindof::Circle | Kindof::Frame) {
        return false;
    }
    let turned =
        attrs.peek(Ns::Draw, "transform").and_then(|text| read_transform(text, terms)).is_some_and(|t| t.map.a * t.map.d - t.map.b * t.map.c < 0.0);
    if kind != Kindof::Custom {
        return turned;
    }
    let mut mirrors = (false, false);
    for child in el.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "enhanced-geometry") {
            mirrors = custom::mirrors(&names.attrs(child));
            names.leave(mark);
            break;
        }
        names.leave(mark);
    }
    turned != (mirrors.0 != mirrors.1)
}

/// The stamps that more than one group with no name of the page has: each group's members are the modelled shapes the walk met inside it, in document order,
/// and its stamp the names they have ([`crate::ext::items::group_fp`], as a group is stamped when its item is written). Each member is charged.
fn ambiguous_stamps(r: &mut Reader, order: &[Met<'_>]) -> Result<HashSet<crate::ext::fingerprint::Fp>, OdgError> {
    let unnamed = |m: &Met<'_>| m.kind == Kindof::Group && m.modelled && m.group.is_some_and(|(name, _)| name.is_empty());
    let mut members: HashMap<usize, Vec<Option<&str>>> = HashMap::new();
    for met in order.iter().filter(|m| m.modelled) {
        if let Some(group) = met.parent.filter(|g| order.get(*g).is_some_and(unnamed)) {
            members.entry(group).or_default().push(met.name);
        }
    }
    r.take_items(members.values().map(Vec::len).sum())?;
    let mut seen: HashMap<crate::ext::fingerprint::Fp, u32> = HashMap::new();
    for (at, met) in order.iter().enumerate() {
        if !unnamed(met) {
            continue;
        }
        let names = members.get(&at).map_or(&[][..], Vec::as_slice);
        let count = seen.entry(crate::ext::items::group_fp(names.iter().copied())).or_insert(0);
        *count = count.saturating_add(1);
    }
    Ok(seen.into_iter().filter(|(_, n)| *n > 1).map(|(fp, _)| fp).collect())
}

/// Pass 1: every drawing shape element of the page, in pre-order, with the `ID` it takes: its DiaCraft id when no earlier shape on the
/// page carries it, else the next number not taken. Keyed by the element's address, which the second pass looks up.
fn number<'t>(
    r: &mut Reader,
    cx: &Cx<'_>,
    names: &mut Names<'t>,
    page: &'t El,
    groups: &mut crate::ext::docfile::PageGroups,
) -> Result<Pass1<'t>, OdgError> {
    // The walk keeps its own stack of (element, declarations to undo): no input makes it recurse.
    let mut order: Vec<Met<'t>> = Vec::new();
    // The groups by `draw:name`, counted in the pre-order of the page (members of groups included): a group's number among those of its name, and in the end how many
    // groups the page has of each name.
    let mut group_names: HashMap<&'t str, u32> = HashMap::new();
    let mut by_name: HashMap<&'t str, usize> = HashMap::new();
    let mut flipped: HashSet<usize> = HashSet::new();
    let mut own: HashSet<usize> = HashSet::new();
    let mut terms = 0usize;
    enum Step<'e> {
        /// An element, whether its parent is modelled, and the group it is a member of (its place in `order`).
        Enter(&'e El, bool, Option<usize>),
        Leave(usize),
    }
    let children = |el: &'t El, modelled: bool, parent: Option<usize>| -> Vec<Step<'t>> {
        let mut kids: Vec<Step<'t>> = el.elements().map(|k| Step::Enter(k, modelled, parent)).collect();
        kids.reverse();
        kids
    };
    let mut stack: Vec<Step<'t>> = children(page, true, None);
    let most = r.limits.max_shapes_per_page.min(diacraft_doc::MAX_SHAPES_PER_PAGE);
    while let Some(step) = stack.pop() {
        match step {
            Step::Leave(n) => names.leave(n),
            Step::Enter(el, parent_modelled, parent) => {
                r.saw_element();
                let mark = names.enter(el);
                let Some(kind) = shape_kind(names, el) else {
                    names.leave(mark);
                    continue;
                };
                if order.len() >= most {
                    return Err(OdgError::Limit { kind: LimitKind::ShapesInAPage, max: most as u64 });
                }
                let attrs = names.attrs(el);
                let key = el as *const El as usize;
                let modelled = parent_modelled && !matches!(kind, Kindof::Kept(_));
                if let Some(id) = attrs.peek(Ns::Xml, "id").or_else(|| attrs.peek(Ns::Draw, "id")) {
                    by_name.entry(id).or_insert(key);
                    // Only a shape a connector can name is asked how its glue points are numbered.
                    if modelled && glue_flipped(names, el, kind, &attrs, &mut terms) {
                        flipped.insert(key);
                    }
                }
                // A group's own style carries none of DiaCraft's data: its `ID` is what the document file says of it, when it says anything, and that is told once
                // the walk has counted the groups of each name.
                let (id, group) = if kind == Kindof::Group {
                    let name = attrs.peek(Ns::Draw, "name").unwrap_or_default();
                    let count = group_names.entry(name).or_insert(0);
                    *count = count.saturating_add(1);
                    (None, Some((name, *count)))
                } else {
                    let (id, glue_own) = style_dia(r, cx, &attrs);
                    if glue_own && kind == Kindof::Custom && modelled {
                        own.insert(key);
                    }
                    (id, None)
                };
                let name = attrs.peek(Ns::Draw, "name");
                let at = order.len();
                order.push(Met { key, id, kind, modelled, group, bound: Binding::No, parent, name });
                stack.push(Step::Leave(mark));
                if holds_shapes(kind) {
                    // A group's children are its members; what a wrapper holds is no member of anything the model holds.
                    stack.extend(children(el, modelled, (kind == Kindof::Group).then_some(at)));
                }
            }
        }
    }
    // A group finds its entry now that every group is counted: by its number among those of its name, or by its name alone when no other group of the page has it (an
    // entry with no `nth`: the review's reader side, where an entry bound to the first group of a name that others had come to share).
    for met in &mut order {
        let Some((name, nth)) = met.group else { continue };
        let unique = group_names.get(name).is_some_and(|n| *n == 1);
        met.bound = Binding::of(groups, name, nth, unique);
        met.id = met.bound.get(groups, name).and_then(|e| e.id);
    }
    let mut taken: HashSet<u32> = HashSet::new();
    let mut owner: HashMap<u32, usize> = HashMap::new();
    for (at, met) in order.iter().enumerate() {
        if let Some(id) = met.id
            && taken.insert(id)
        {
            owner.insert(id, at);
        }
    }
    // An entry no group takes keeps its `ID` all the same: no other shape is numbered it, so a formula that names it reads no shape and never another one.
    taken.extend(groups.values().filter_map(|e| e.id));
    // An entry with no `nth` that a name several groups have is read for none of them: it is dropped here, and the page says so.
    let held = groups.len();
    groups.retain(|(name, nth), _| nth.is_some() || group_names.get(name.as_str()).is_none_or(|n| *n < 2));
    if groups.len() < held {
        r.warn_once("diacraft_group_name_shared", || {
            "DiaCraft's document file holds data for a group whose name several groups of a page have now (it was copied or renamed in another program): that data is not read".to_owned()
        });
    }
    let mut out = HashMap::with_capacity(order.len());
    let mut next = 1u32;
    for (at, met) in order.iter().enumerate() {
        let ordinal = u32::try_from(at + 1).unwrap_or(u32::MAX);
        let own = met.id.filter(|id| owner.get(id) == Some(&at));
        // A shape whose DiaCraft id an earlier shape holds is a copy: it is read from its native markup alone.
        let copy = met.id.is_some() && own.is_none();
        let id = match own {
            Some(id) => id,
            None => {
                if met.id.is_some() {
                    r.warn("diacraft_id_repeated", || "a shape's DiaCraft id is held by an earlier shape too; it is read as a copy".to_owned());
                }
                while taken.contains(&next) {
                    next = next.saturating_add(1);
                }
                let id = next;
                next = next.saturating_add(1);
                id
            }
        };
        out.insert(met.key, Numbered { ordinal, id, copy, kind: met.kind, modelled: met.modelled, bound: met.bound });
    }
    r.path_units(terms);
    Ok(Pass1 { numbers: out, names: by_name, flipped, own, ambiguous: ambiguous_stamps(r, &order)? })
}

/// A box on the page (inches, y up): what a shape takes up, for the box of the group that holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

impl Bounds {
    /// The box of a frame of `w` by `h` turned `angle` about its centre `pin`.
    fn of_frame(pin: (f64, f64), w: f64, h: f64, angle: f64) -> Bounds {
        let (s, c) = angle.sin_cos();
        let (ex, ey) = ((w * c).abs() / 2.0 + (h * s).abs() / 2.0, (w * s).abs() / 2.0 + (h * c).abs() / 2.0);
        Bounds { x0: pin.0 - ex, x1: pin.0 + ex, y0: pin.1 - ey, y1: pin.1 + ey }
    }

    /// The box of two points.
    fn of_ends(a: (f64, f64), b: (f64, f64)) -> Bounds {
        Bounds { x0: a.0.min(b.0), x1: a.0.max(b.0), y0: a.1.min(b.1), y1: a.1.max(b.1) }
    }

    fn union(self, o: Bounds) -> Bounds {
        Bounds { x0: self.x0.min(o.x0), x1: self.x1.max(o.x1), y0: self.y0.min(o.y0), y1: self.y1.max(o.y1) }
    }
}

/// A shape read: the shape, its `draw:z-index`, and the box it takes up on the page.
pub(crate) struct Built {
    pub shape: Shape,
    pub z: Option<i64>,
    pub bounds: Bounds,
    /// The shape's own style carried tier 2, which was put over what the markup gave.
    pub tier2: bool,
    /// The keys of the tier-2 items that were stale (their native outputs changed) and were left out (tier 3).
    pub stale: Vec<crate::ext::fingerprint::Key>,
    /// The `Master` and `MasterShape` of the link that was left out as stale.
    pub stale_link: Option<crate::index::MasterLink>,
    /// The native outputs of the shape's markup that its tier-2 items rest on, as the file has them: what the writer stamps a missing fingerprint from.
    pub natives: Option<crate::ext::tier3::Natives>,
}

/// What reading one page's shapes keeps as it goes: the numbering, the ids connectors name, the index, and the `Connect` rows.
pub(crate) struct Scope<'a, 't> {
    numbers: &'a HashMap<usize, Numbered>,
    ids: connectors::Ids<'t>,
    index: PageShapes,
    connects: Vec<diacraft_doc::Connect>,
    /// How deep in groups the shapes being read are.
    depth: usize,
}

/// What a `Connect` row costs the item cap: its place in the page's list and its two small names, by the law of the memory tests ([`super::cost::CONNECT`]; it was four items, about 190 bytes).
const CONNECT_ITEMS: usize = super::cost::CONNECT;

/// The most groups one inside another that are read: the tree's own depth cap (200) leaves room for about this many under a page.
const MOST_GROUP_DEPTH: usize = 190;

/// What one level of a page holds: the modelled shapes in document order, and the children kept unmodelled.
struct Level {
    shapes: Vec<Built>,
    passthrough: Passthrough,
}

/// Reads the shapes of a page (or of a master page): pass 1 numbers them, pass 2 builds the modelled ones in document order and keeps the
/// rest, a group's members inside the group, and the top-level ones are put in drawing order by `draw:z-index`.
pub(crate) fn read_page<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    page: &'t El,
    pcx: &mut PageCx,
    source: PageSource,
) -> Result<PageShapesRead, OdgError> {
    let pass = number(r, cx, names, page, &mut pcx.groups)?;
    pcx.ambiguous.clone_from(&pass.ambiguous);
    // A connector names a shape the page models.
    let mut ids = connectors::Ids::default();
    for (name, key) in &pass.names {
        if let Some(n) = pass.numbers.get(key).filter(|n| n.modelled) {
            ids.by_name.insert(*name, n.id);
            if pass.flipped.contains(key) {
                ids.flipped.insert(n.id);
            }
            // A copy is read without DiaCraft's data, so its rows are the reader's: the standard four and the listed points from 4.
            if pass.own.contains(key) && !n.copy {
                ids.own.insert(n.id);
            }
        }
    }
    let mut sc = Scope { numbers: &pass.numbers, ids, index: PageShapes::new(source), connects: Vec::new(), depth: 0 };
    let Level { mut shapes, passthrough } = read_level(r, names, cx, page, pcx, &mut sc, true)?;
    // By `draw:z-index` where present, a shape without one at its document position, document order breaking ties.
    let mut order: Vec<(i64, usize)> = shapes.iter().enumerate().map(|(at, b)| (b.z.unwrap_or(at as i64), at)).collect();
    order.sort_unstable();
    let mut taken: Vec<Option<Built>> = shapes.drain(..).map(Some).collect();
    let sorted: Vec<Arc<Shape>> = order.iter().filter_map(|(_, at)| taken.get_mut(*at).and_then(Option::take)).map(|b| Arc::new(b.shape)).collect();
    r.report.stats.max_shapes_in_a_page = r.report.stats.max_shapes_in_a_page.max(pass.numbers.len());
    r.path_units(pcx.budget.spent);
    pcx.budget.spent = 0;
    let annotations = std::mem::take(&mut pcx.annotations);
    let mut index = sc.index;
    index.set_comments(annotations.taken.iter().map(|(place, _)| *place).collect(), annotations.seen);
    Ok(PageShapesRead { shapes: sorted, passthrough, index, connects: sc.connects, annotations })
}

/// Whether an attribute in `ns` is another producer's: in no namespace the reader reads (LibreOffice's pre-standard `officeooo` included).
fn is_foreign(ns: Ns) -> bool {
    matches!(ns, Ns::Other | Ns::Officeooo | Ns::None)
}

/// The children of a page or of a group: each shape element read (or kept unmodelled), the other children kept in the passthrough.
fn read_level<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    parent: &'t El,
    pcx: &mut PageCx,
    sc: &mut Scope<'_, 't>,
    is_page: bool,
) -> Result<Level, OdgError> {
    let mut shapes: Vec<Built> = Vec::new();
    let mut passthrough = Passthrough::new();
    let mut layer_sets = 0usize;
    // The places of the shape elements kept unmodelled (a hyperlink wrapper), each as the number of modelled shapes before it: one with a modelled shape after it changes its place in a copy.
    let mut kept_shapes: Vec<usize> = Vec::new();
    for node in &parent.children {
        let Node::El(child) = node else { continue };
        let mark = names.enter(child);
        let modelled_before = shapes.len();
        match shape_kind(names, child) {
            Some(kind) => {
                let Numbered { ordinal, id, copy, bound, .. } = sc.numbers.get(&(child as *const El as usize)).copied().unwrap_or(Numbered {
                    ordinal: 0,
                    id: 0,
                    copy: false,
                    kind,
                    modelled: false,
                    bound: Binding::No,
                });
                match read_shape(r, names, cx, child, kind, pcx, sc, (id, copy, bound))? {
                    Some(mut built) => {
                        sc.index.note(ordinal, id, true);
                        if copy {
                            sc.index.note_copy(id);
                        }
                        if built.tier2 {
                            sc.index.note_tier2(id);
                        }
                        if !built.stale.is_empty() {
                            sc.index.note_stale(id, std::mem::take(&mut built.stale), built.stale_link.take());
                        }
                        if let Some(natives) = built.natives.take() {
                            sc.index.note_natives(id, natives);
                        }
                        r.report.stats.shapes = r.report.stats.shapes.saturating_add(1);
                        shapes.push(built);
                    }
                    None => {
                        note_kept(r, child, sc.numbers, &mut sc.index);
                        keep_element(r, names, &mut passthrough, modelled_before, child)?;
                        kept_shapes.push(modelled_before);
                    }
                }
            }
            None if is_page => match names.element(child) {
                // A page's own layer set is read for its `Layer` rows, and held whole beside them, as the page's other markup is.
                (Ns::Draw, "layer-set") => {
                    // Only the first is read, and it is held beside its rows unless they say all of it.
                    layer_sets += 1;
                    if layer_sets > 1 || !layers::rows_say_all(names, child) {
                        keep_element(r, names, &mut passthrough, modelled_before, child)?;
                    }
                }
                // A page's comment is the document's, and no markup is kept beside it, unless the model cannot hold it exactly.
                (ns, "annotation") if super::comments::is_annotation(ns, "annotation") && matches!(sc.index.source, PageSource::Page(_)) => {
                    if !super::comments::take(r, names, child, &mut pcx.annotations)? {
                        r.keep("annotation");
                        keep_element(r, names, &mut passthrough, modelled_before, child)?;
                    }
                }
                _ => {
                    r.keep("page_child");
                    keep_element(r, names, &mut passthrough, modelled_before, child)?;
                }
            },
            None => {
                r.keep(kept_word(names, child, false));
                keep_element(r, names, &mut passthrough, modelled_before, child)?;
            }
        }
        names.leave(mark);
    }
    // A copy written from the model puts the shapes it keeps unmodelled after the ones it models: one that had a modelled shape over it is then drawn over that shape, and
    // the open names it.
    let moved = kept_shapes.iter().filter(|before| **before < shapes.len()).count();
    if moved > 0 {
        r.not_kept("shape_order", moved as u64);
    }
    Ok(Level { shapes, passthrough })
}

/// Notes in the index an element kept unmodelled, and every numbered shape inside it, in document order; each is a kept shape, and each
/// one inside it is counted by its kind as well (the element itself was counted when it was kept): a group's members, a hyperlink's
/// shape.
fn note_kept(r: &mut Reader, el: &El, numbers: &HashMap<usize, Numbered>, index: &mut PageShapes) {
    let mut stack = vec![el];
    while let Some(here) = stack.pop() {
        if let Some(n) = numbers.get(&(here as *const El as usize)) {
            index.note(n.ordinal, n.id, false);
            r.report.stats.kept_shapes = r.report.stats.kept_shapes.saturating_add(1);
            if !std::ptr::eq(here, el) {
                r.keep(match n.kind {
                    Kindof::Kept(word) | Kindof::Object(word) => word,
                    Kindof::Group => "group",
                    Kindof::Custom => "custom_shape",
                    Kindof::Connector => "connector",
                    _ => "nested_shape",
                });
            }
        }
        let mut kids: Vec<&El> = here.elements().collect();
        kids.reverse();
        stack.extend(kids);
    }
}

/// Keeps `el` whole in `passthrough`, after `after` modelled shapes, its items taken from the total first: the markup's, and its place in the list
/// ([`super::cost::KEPT_SLOT`]).
fn keep_element(r: &mut Reader, names: &Names<'_>, passthrough: &mut Passthrough, after: usize, el: &El) -> Result<(), OdgError> {
    r.take_items(element_cost(el).saturating_add(super::cost::KEPT_SLOT))?;
    passthrough.push(u32::try_from(after).unwrap_or(u32::MAX), RawNode::Element(raw_element_in(r, names, el)?));
    Ok(())
}

/// The items an element costs a document that keeps it: itself, each attribute and each node in it, walked with a stack of its own.
pub(super) fn element_cost(el: &El) -> usize {
    // Measured (`limits.rs`): a kept element is about three items with its node and name, an attribute two with its pair of strings, and a
    // text node two; each string's bytes besides.
    const ELEMENT: usize = 3;
    const PAIR: usize = 2;
    const NODE: usize = 2;
    let mut items = 0usize;
    let mut bytes = 0usize;
    let mut stack = vec![el];
    while let Some(here) = stack.pop() {
        items = items.saturating_add(ELEMENT).saturating_add(here.attrs.len().saturating_mul(PAIR));
        bytes = bytes.saturating_add(here.name.len());
        for (k, v) in &here.attrs {
            bytes = bytes.saturating_add(k.len()).saturating_add(v.len());
        }
        for node in &here.children {
            match node {
                Node::El(child) => stack.push(child),
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => {
                    items = items.saturating_add(NODE);
                    bytes = bytes.saturating_add(t.len());
                }
                Node::Pi { target, data } => {
                    items = items.saturating_add(NODE);
                    bytes = bytes.saturating_add(target.len()).saturating_add(data.len());
                }
            }
        }
    }
    items.saturating_add(super::cost::text(bytes).div_ceil(super::ITEM_BYTES))
}

/// An element and everything in it, as the model keeps an unknown element. The tree is at most the container's depth cap deep, so the
/// recursion is bounded by it.
pub(super) fn raw_element(el: &El) -> RawElement {
    RawElement {
        name: Arc::from(el.name.as_str()),
        attrs: RawAttrs::from_pairs(el.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
        children: el.children.iter().map(raw_node).collect(),
    }
}

/// [`raw_element`] for markup read inside the pages, **with the declarations its prefixes need** that the model does not hold: a prefix an element of the page, a shape or
/// a paragraph above it declares on its own start tag is bound where the markup was read and in no place the model writes it ([`Names::local_binding`]), so the element declares the
/// prefix itself, as XML Namespaces 1.0 lets it, and is written whole wherever it goes. Charged before the declarations are made.
pub(super) fn raw_element_in(r: &mut Reader, names: &Names<'_>, el: &El) -> Result<RawElement, OdgError> {
    let mut made = raw_element(el);
    bind_locals(r, names, &mut made)?;
    Ok(made)
}

/// The declarations `made` (an element and everything in it, as kept) needs, one for each prefix it uses and does not declare that a declaration the model does not hold binds
/// ([`Names::local_binding`]), as the pairs `xmlns:p` and the URI (`xmlns` for the default namespace), in the order of the prefixes. A declaration XML Namespaces does not allow is
/// not one: such markup is refused in words where it is written. `None` for markup whose names are no qualified names (the same).
fn local_declarations(names: &Names<'_>, made: &RawElement) -> Vec<(String, String)> {
    if !names.has_locals() {
        return Vec::new();
    }
    let Some(free) = crate::write::kept::free_prefixes(made) else { return Vec::new() };
    free.prefixes
        .iter()
        .filter_map(|prefix| {
            let uri = names.local_binding(prefix)?;
            crate::write::kept::declaration_ok(prefix, uri)
                .then(|| (if prefix.is_empty() { "xmlns".to_owned() } else { format!("xmlns:{prefix}") }, uri.to_owned()))
        })
        .collect()
}

/// Puts the declarations `made` needs on its start tag, before its own attributes ([`local_declarations`]), charged first: each is two items and its bytes.
pub(super) fn bind_locals(r: &mut Reader, names: &Names<'_>, made: &mut RawElement) -> Result<(), OdgError> {
    let declared = local_declarations(names, made);
    if declared.is_empty() {
        return Ok(());
    }
    take_declarations(r, &declared)?;
    let own: Vec<(&str, &str)> = made.attrs.iter().collect();
    made.attrs = RawAttrs::from_pairs(declared.iter().map(|(k, v)| (k.as_str(), v.as_str())).chain(own));
    Ok(())
}

/// Takes the items the declarations `declared` cost the document, before they are stored.
fn take_declarations(r: &mut Reader, declared: &[(String, String)]) -> Result<(), OdgError> {
    let bytes = declared.iter().fold(0usize, |sum, (k, v)| sum.saturating_add(k.len()).saturating_add(v.len()));
    r.take_items(declared.len().saturating_mul(2))?;
    r.take_bytes(bytes)
}

/// The declarations the attributes `names_of` (the names as written, of attributes kept on a shape) need that the model does not hold, one for each prefix in the order the prefixes are
/// first used, as the pairs `xmlns:p` and the URI: an attribute cannot declare its own prefix, so the shape's kept attributes carry the declaration beside them, and the
/// writer puts it on the start tag it writes them on. Charged first.
fn attribute_declarations<'a>(r: &mut Reader, names: &Names<'_>, names_of: impl Iterator<Item = &'a str>) -> Result<Vec<(String, String)>, OdgError> {
    if !names.has_locals() {
        return Ok(Vec::new());
    }
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for name in names_of {
        let Some((prefix, _)) = name.split_once(':') else { continue };
        if prefix == "xml" || prefix == "xmlns" || seen.contains(&prefix) {
            continue;
        }
        seen.push(prefix);
        if let Some(uri) = names.local_binding(prefix)
            && crate::write::kept::declaration_ok(prefix, uri)
        {
            declared.push((format!("xmlns:{prefix}"), uri.to_owned()));
        }
    }
    take_declarations(r, &declared)?;
    Ok(declared)
}

fn raw_node(node: &Node) -> RawNode {
    match node {
        Node::El(el) => RawNode::Element(raw_element(el)),
        Node::Text(t) | Node::CData(t) => RawNode::Text(Arc::from(t.as_str())),
        Node::Comment(c) => RawNode::Comment(Arc::from(c.as_str())),
        Node::Pi { target, data } => RawNode::Pi { target: Arc::from(target.as_str()), data: Arc::from(data.as_str()) },
    }
}

/// The turn a shape has in the model (`Angle`, counterclockwise as seen) and the turn its file holds (`rotate` of `draw:transform`, the same direction) beside the mirrors of its
/// geometry: the same number when the shape has both mirrors or none, **and minus the other** when it has exactly one. The model flips the shape in its own
/// frame and then turns it; LibreOffice, whose files are the reference for these, draws a single mirror (`draw:mirror-horizontal` or `draw:mirror-vertical`) on the picture of the
/// frame already turned, across the vertical or the horizontal line through the frame's centre, which is the model's turn the other way round. The map is its own inverse, so the
/// reader and the writer both use it.
pub(crate) fn turn_beside_mirrors(angle: f64, mirror_x: bool, mirror_y: bool) -> f64 {
    if mirror_x != mirror_y { -angle } else { angle }
}

/// The text's own turn a shape has in the model (`TxtAngle`, degrees, counterclockwise) beside its flips (`FlipX`, `FlipY`), of the turn its file gives the text
/// (`draw:text-rotate-angle` of a custom shape's geometry, degrees, counterclockwise; 0 for a shape that has none).
///
/// LibreOffice 26.8.2.2 (a black box) turns a shape's text by the turn of the frame it draws, **half a turn more when the model has `FlipY`** (a
/// `draw:mirror-vertical`, or a transform whose determinant is negative, not undone by the other), and the file's own turn of the text, counterclockwise whatever the
/// mirrors: right triangles with the text "Ab", in each mirror state at 0, 30 and 135 degrees with no text turn, 90 and 180 degrees, and beside `scale (1 -1)` and
/// `scale (-1 1)`, and rectangles and ellipses beside those transforms, each run where its SVG export put it. The model turns a block by `Angle` + `TxtAngle` with no
/// flip or two, and by `Angle` − `TxtAngle` with one, so `TxtAngle` is the file's turn, minus it beside one flip, and half a turn
/// more with `FlipY`, brought into (−180, 180] when it has the half turn. With no `FlipY` it is the file's number itself, or minus it, as the reader read it before.
pub(crate) fn text_turn_beside_flips(file: f64, flip_x: bool, flip_y: bool) -> f64 {
    let turn = if flip_x != flip_y { -file } else { file };
    if !flip_y {
        return turn;
    }
    let r = (turn + 180.0).rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

/// The box LibreOffice measures a shape's glue points in ([`connectors::Place`]), by its kind and `draw:kind`: a whole ellipse's own, a custom shape's, a rectangle's or
/// a frame's (a text box, a picture: measured LibreOffice 26.8.2.2 placing a frame's `draw:glue-point` as a custom shape's) frame's; a polygon's, a
/// polyline's or a path's is the box of its outline, which its geometry gives (`connectors::outline_extent`, in [`read_shape`]); any other shape's points are read in its own
/// frame, as before (an arc's are on the box of the arc it draws, measured by and not built).
pub(crate) fn glue_box(kind: Kindof, ellipse_kind: Option<&str>) -> connectors::Boxed {
    match kind {
        Kindof::Custom | Kindof::Rect | Kindof::Frame => connectors::Boxed::Frame,
        Kindof::Ellipse | Kindof::Circle if ellipse_kind.is_none_or(|k| k == "full") => connectors::Boxed::Ellipse,
        _ => connectors::Boxed::Own,
    }
}

/// The text's own turn beside the shape's flips ([`text_turn_beside_flips`]), on the kinds LibreOffice was seen to turn it on: a custom shape (`file`, its
/// `draw:text-rotate-angle`, `None` for an object the reader does not draw), a rectangle, an ellipse or a circle. Gives the place of the shape's glue points.
fn turn_text(r: &mut Reader, sheet: &mut Sheet, kind: Kindof, file: Option<f64>, boxed: connectors::Boxed) -> Result<connectors::Place, OdgError> {
    let file = if matches!(kind, Kindof::Rect | Kindof::Ellipse | Kindof::Circle) { Some(0.0) } else { file };
    let flips = (number_of(sheet, "FlipX") == Some(1.0), number_of(sheet, "FlipY") == Some(1.0));
    if let Some(file) = file {
        let turn = text_turn_beside_flips(file, flips.0, flips.1);
        if turn != 0.0 {
            put(r, sheet, "TxtAngle", cells::angle(turn.to_radians()))?;
        }
    }
    Ok(connectors::Place { angle: number_of(sheet, "Angle").unwrap_or(0.0), flip: flips, boxed })
}

/// The number a cell of the sheet holds.
fn number_of(sheet: &Sheet, name: &str) -> Option<f64> {
    sheet.cells.get(name).and_then(|c| c.value.as_ref()?.number())
}

/// An affine map of the page (y down, inches): `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine {
    pub(crate) a: f64,
    pub(crate) b: f64,
    pub(crate) c: f64,
    pub(crate) d: f64,
    pub(crate) e: f64,
    pub(crate) f: f64,
}

impl Affine {
    pub(crate) const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub(crate) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// `then` after `self`.
    pub(crate) fn then(&self, then: &Affine) -> Affine {
        Affine {
            a: then.a * self.a + then.c * self.b,
            b: then.b * self.a + then.d * self.b,
            c: then.a * self.c + then.c * self.d,
            d: then.b * self.c + then.d * self.d,
            e: then.a * self.e + then.c * self.f + then.e,
            f: then.b * self.e + then.d * self.f + then.f,
        }
    }
}

/// A `draw:transform` read: the map, the unit of its `translate` (for `PinX`'s unit), and whether a shear was left out.
struct Transform {
    map: Affine,
    translate_unit: Option<LenUnit>,
    sheared: bool,
}

/// The most terms one `draw:transform` holds that are read; real ones hold two.
const MOST_TERMS: usize = 64;

/// The transforms of `draw:transform` composed in the order written, every angle in radians, `rotate(a)` taking
/// (u, v) to (u cos a + v sin a, -u sin a + v cos a). `skewX` and `skewY` are left out (a shear has no cell). `None` for a list that cannot
/// be read: the shape then reads as if it had none, with a warning, and the attribute is kept.
fn parse_transform(r: &mut Reader, text: &str) -> Option<Transform> {
    let mut terms = 0usize;
    let read = read_transform(text, &mut terms);
    r.path_units(terms);
    read
}

/// Whether the reader reads `text` as a `draw:transform`: the question the save asks of what it wrote (the check of the bytes made).
pub(crate) fn transform_reads(text: &str) -> bool {
    read_transform(text, &mut 0).is_some()
}

/// [`parse_transform`] without the reader's counts: it adds the terms it looked at, the one past the most included, to `terms`.
fn read_transform(text: &str, terms: &mut usize) -> Option<Transform> {
    let mut map = Affine::IDENTITY;
    let mut translate_unit = None;
    let mut sheared = false;
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        *terms += 1;
        if *terms > MOST_TERMS {
            return None;
        }
        let name_end = rest.find(|c: char| !c.is_ascii_alphabetic())?;
        let (name, after) = rest.split_at(name_end);
        let after = after.trim_start().strip_prefix('(')?;
        let close = after.find(')')?;
        let (inside, tail) = after.split_at(close);
        rest = tail.get(1..)?;
        let args: Vec<&str> = inside.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        let num = |at: usize| args.get(at).and_then(|a| units::number_any(a));
        // A coordinate of a translation: a length, or a plain number in hundredths of a millimetre, as LibreOffice reads one (`units::translation`).
        let len = |at: usize| args.get(at).and_then(|a| units::translation(a));
        let term = match (name, args.len()) {
            ("rotate", 1) => {
                let a = num(0)?;
                let (s, c) = a.sin_cos();
                Affine { a: c, b: -s, c: s, d: c, e: 0.0, f: 0.0 }
            }
            ("translate", 1 | 2) => {
                let (tx, unit) = len(0)?;
                let ty = if args.len() == 2 { len(1)?.0 } else { 0.0 };
                translate_unit = unit.or(translate_unit);
                Affine { e: tx, f: ty, ..Affine::IDENTITY }
            }
            ("scale", 1 | 2) => {
                let sx = num(0)?;
                let sy = if args.len() == 2 { num(1)? } else { sx };
                Affine { a: sx, d: sy, ..Affine::IDENTITY }
            }
            ("skewX" | "skewY", 1) => {
                num(0)?;
                sheared = true;
                Affine::IDENTITY
            }
            ("matrix", 6) => {
                let ((e, unit), (f, _)) = (len(4)?, len(5)?);
                translate_unit = unit.or(translate_unit);
                Affine { a: num(0)?, b: num(1)?, c: num(2)?, d: num(3)?, e, f }
            }
            _ => return None,
        };
        map = map.then(&term);
    }
    let finite = [map.a, map.b, map.c, map.d, map.e, map.f].iter().all(|n| n.is_finite());
    finite.then_some(Transform { map, translate_unit, sheared })
}

/// A shape's frame on the page as R6 reads it.
struct Frame {
    width: f64,
    height: f64,
    /// The image of the frame's centre, page coordinates (y down).
    pin: (f64, f64),
    angle: f64,
    flip_y: bool,
}

/// The frame (`x`, `y`, `w`, `h`, inches) under `transform`.
fn frame_of(x: f64, y: f64, w: f64, h: f64, transform: Option<&Transform>) -> (Frame, bool) {
    let map = transform.map_or(Affine::IDENTITY, |t| t.map);
    let (a, b, c, d) = (map.a, map.b, map.c, map.d);
    let col1 = a.hypot(b);
    let col2 = c.hypot(d);
    let det = a * d - b * c;
    let sheared = col1 > 0.0 && col2 > 0.0 && (a * c + b * d).abs() > 1e-9 * col1 * col2;
    let angle = if col1 > 0.0 { (-b).atan2(a) } else { 0.0 };
    let height_scale = if sheared && col1 > 0.0 { det.abs() / col1 } else { col2 };
    let pin = map.apply(x + w / 2.0, y + h / 2.0);
    let frame = Frame { width: col1 * w, height: height_scale * h, pin, angle: if angle == 0.0 { 0.0 } else { angle }, flip_y: det < 0.0 };
    (frame, sheared || transform.is_some_and(|t| t.sheared))
}

/// The frame of a 2-D shape as its attributes give it (what the model writer compares the pose of a shape with, to tell the frame the shape was read with from
/// one an edit changed): the size, the image of the frame's centre on the page (inches, y down), the turn, and whether the map flips the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameRead {
    pub size: (f64, f64),
    pub centre: (f64, f64),
    pub angle: f64,
    pub flipped: bool,
    pub sheared: bool,
}

/// The frame of `x`, `y`, `w` and `h` (inches) under the `draw:transform` text `transform`, as the reader reads it; `None` for a list it cannot read.
pub(crate) fn frame_read(x: f64, y: f64, w: f64, h: f64, transform: Option<&str>) -> Option<FrameRead> {
    let transform = match transform {
        Some(text) => Some(read_transform(text, &mut 0)?),
        None => None,
    };
    let (frame, sheared) = frame_of(x, y, w.max(0.0), h.max(0.0), transform.as_ref());
    Some(FrameRead { size: (frame.width, frame.height), centre: frame.pin, angle: frame.angle, flipped: frame.flip_y, sheared })
}

/// The map of a `draw:transform` text as the reader reads it (the identity for none); `None` for a list it cannot read.
pub(crate) fn transform_map(transform: Option<&str>) -> Option<Affine> {
    match transform {
        Some(text) => Some(read_transform(text, &mut 0)?.map),
        None => Some(Affine::IDENTITY),
    }
}

/// Puts a cell directly under the sheet, taken from the document's total first.
fn put(r: &mut Reader, sheet: &mut Sheet, name: &'static str, cell: Cell) -> Result<(), OdgError> {
    r.take_cells(1)?;
    sheet.cells.insert(CellName::new(name), cell);
    Ok(())
}

/// A row of a section keyed by `IX`.
pub(super) fn row(ix: u32, row_type: Option<&str>, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: row_type.map(Arc::from), ..Row::default() };
    for (name, cell) in cells {
        row.cells.insert(CellName::new(name), cell);
    }
    Arc::new(row)
}

/// The units of a shape's `Width` and `Height`, which the cells written after them take.
#[derive(Clone, Copy)]
pub(super) struct Units {
    pub width: Option<&'static str>,
    pub height: Option<&'static str>,
}

/// The four standard connection rows, in the order and form of DiaCraft's basic shapes: top, left, bottom,
/// right, for ODF glue ids 0, 3, 2 and 1.
fn connection(forms: &mut Forms, w: f64, h: f64, u: Units) -> Section {
    let mut section = Section::new("Connection", None);
    let points = [(0.5, 1.0, 0.0, 1.0), (0.0, 0.5, -1.0, 0.0), (0.5, 0.0, 0.0, -1.0), (1.0, 0.5, 1.0, 0.0)];
    for (ix, (fx, fy, dx, dy)) in points.into_iter().enumerate() {
        section.rows.push(row(
            u32::try_from(ix).unwrap_or(u32::MAX),
            None,
            vec![
                ("X", cells::share(forms, "Width", fx, w, u.width)),
                ("Y", cells::share(forms, "Height", fy, h, u.height)),
                ("DirX", cells::plain(dx)),
                ("DirY", cells::plain(dy)),
                ("Type", cells::plain(0.0)),
            ],
        ));
    }
    section
}

/// The `Geometry` sections of `specs`, `IX` from 0, rows from 1.
fn geometry_sections(r: &mut Reader, specs: Vec<geometry::Section>, w: f64, h: f64, sheet: &mut Sheet) -> Result<(), OdgError> {
    if specs.len() > diacraft_doc::MAX_SECTIONS_PER_SHEET.saturating_sub(4) {
        return Err(OdgError::TooComplex { what: "subpaths in one shape", max: diacraft_doc::MAX_SECTIONS_PER_SHEET as u64 });
    }
    for (n, spec) in specs.into_iter().enumerate() {
        if spec.rows.len() > diacraft_doc::MAX_ROWS_PER_SECTION {
            return Err(OdgError::TooComplex { what: "rows in one subpath", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
        }
        let mut section = Section::new("Geometry", Some(u32::try_from(n).unwrap_or(u32::MAX)));
        if spec.no_fill {
            section.cells.insert(CellName::new("NoFill"), cells::plain(1.0));
        }
        let rows = spec.rows.len().saturating_mul(super::cost::ROW);
        r.take_items(super::cost::SECTION + if spec.no_fill { super::cost::SECTION_CELLS } else { 0 } + rows)?;
        for (i, spec_row) in spec.rows.iter().enumerate() {
            let ix = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let p = cells::plain;
            let made = match *spec_row {
                geometry::Row::RelMoveTo(x, y) => row(ix, Some("RelMoveTo"), vec![("X", p(x)), ("Y", p(y))]),
                geometry::Row::RelLineTo(x, y) => row(ix, Some("RelLineTo"), vec![("X", p(x)), ("Y", p(y))]),
                geometry::Row::RelCubBezTo { x, y, a, b, c, d } => {
                    row(ix, Some("RelCubBezTo"), vec![("X", p(x)), ("Y", p(y)), ("A", p(a)), ("B", p(b)), ("C", p(c)), ("D", p(d))])
                }
                geometry::Row::RelQuadBezTo { x, y, a, b } => row(ix, Some("RelQuadBezTo"), vec![("X", p(x)), ("Y", p(y)), ("A", p(a)), ("B", p(b))]),
                geometry::Row::RelEllipticalArcTo { x, y, a, b, axes } => {
                    let (c, d) = arc_axes(&mut r.forms, axes, w, h);
                    row(ix, Some("RelEllipticalArcTo"), vec![("X", p(x)), ("Y", p(y)), ("A", p(a)), ("B", p(b)), ("C", c), ("D", d)])
                }
            };
            r.take_cells(made.cells.len())?;
            section.rows.push(made);
        }
        sheet.sections.push(Arc::new(section));
    }
    Ok(())
}

/// The rectangle geometry, one section shared by every rectangle (its rows are values, the same whatever the frame).
fn rectangle(r: &mut Reader, cx: &mut Cx<'_>, sheet: &mut Sheet) -> Result<(), OdgError> {
    if let Some(section) = &cx.shared.rectangle {
        r.take_items(1)?;
        sheet.sections.push(Arc::clone(section));
        return Ok(());
    }
    let mut built = Sheet::default();
    geometry_sections(r, geometry::rectangle(), 1.0, 1.0, &mut built)?;
    if let Some(section) = built.sections.into_iter().next() {
        cx.shared.rectangle = Some(Arc::clone(&section));
        sheet.sections.push(section);
    }
    Ok(())
}

/// The `C` and `D` of an arc row, at the frame's size `w` by `h`.
fn arc_axes(forms: &mut Forms, axes: ArcAxes, w: f64, h: f64) -> (Cell, Cell) {
    match axes {
        ArcAxes::Values { d } => (cells::plain(0.0), cells::plain(d)),
        ArcAxes::Upright { k } => {
            let ratio = if h != 0.0 { k * w / h } else { 0.0 };
            (cells::plain(0.0), forms.cell(&format!("{}*Width/Height", cells::number(k).text()), ratio, None))
        }
        ArcAxes::Turned { c0, d0 } => {
            let n = |v: f64| cells::number(v).text().to_owned();
            let args = format!("{},{},{},{},Width,Height", n(c0), n(d0), n(w), n(h));
            (forms.cell(&format!("_ELLIPSE_THETA({args})"), c0, None), forms.cell(&format!("_ELLIPSE_ECC({args},C)"), d0, None))
        }
    }
}

/// The `Ellipse` row of a full ellipse or circle.
fn ellipse_section(forms: &mut Forms, circle: bool, w: f64, h: f64, u: Units) -> Section {
    let mut section = Section::new("Geometry", Some(0));
    let m = w.min(h);
    let (a, d) = if circle && w != h {
        (
            forms.cell("Width*0.5+MIN(Width,Height)*0.5", w * 0.5 + m * 0.5, u.width),
            forms.cell("Height*0.5+MIN(Width,Height)*0.5", h * 0.5 + m * 0.5, u.height),
        )
    } else {
        (forms.cell("Width", w, u.width), forms.cell("Height", h, u.height))
    };
    section.rows.push(row(
        1,
        Some("Ellipse"),
        vec![
            ("X", forms.cell("Width*0.5", w * 0.5, u.width)),
            ("Y", forms.cell("Height*0.5", h * 0.5, u.height)),
            ("A", a),
            ("B", forms.cell("Height*0.5", h * 0.5, u.height)),
            ("C", forms.cell("Width*0.5", w * 0.5, u.width)),
            ("D", d),
        ],
    ));
    section
}

/// The length of `attrs`' attribute `local` in `ns`, taken, with the `px` warning.
fn take_len(r: &mut Reader, attrs: &mut super::names::Attrs<'_>, ns: Ns, local: &str) -> Option<Length> {
    let l = attrs.take(ns, local).and_then(units::length)?;
    r.saw_px(l);
    Some(l)
}

/// The picture of the first `draw:image` a frame holds, whatever its format (the replacement image of an object).
fn replacement_image<'t>(names: &mut Names<'t>, el: &'t El) -> Option<&'t El> {
    for child in el.elements() {
        let mark = names.enter(child);
        let is = names.is(child, Ns::Draw, "image");
        names.leave(mark);
        if is {
            return Some(child);
        }
    }
    None
}

/// Reads one shape element. `None` for one this reader keeps unmodelled; the shape, its `draw:z-index` and its box otherwise.
#[allow(clippy::too_many_arguments)]
fn read_shape<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    el: &'t El,
    kind: Kindof,
    pcx: &mut PageCx,
    sc: &mut Scope<'_, 't>,
    numbered: (u32, bool, Binding),
) -> Result<Option<Built>, OdgError> {
    let (id, copy, bound) = numbered;
    let shows = match kind {
        Kindof::Kept(word) => {
            r.keep(word);
            return Ok(None);
        }
        Kindof::Frame => Some(frame_shows(names, cx.package, el)),
        _ => None,
    };
    // An object DiaCraft does not draw is a `Foreign` shape on its frame, counted by kind.
    let mut object: Option<&'static str> = match (kind, shows) {
        (Kindof::Object(word), _) => Some(word),
        (Kindof::Frame, Some(FrameShows::Object)) => Some("frame_object"),
        _ => None,
    };
    // What a frame's object is called, for the placeholder it gets when it has no picture: found here, before any child's namespaces are in scope.
    let frame_label = (object == Some("frame_object")).then(|| frame_object_label(names, el));
    count_object(r, kind, object);
    r.take_shape()?;
    let mut attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let mut shape = Shape::new(id);
    shape.kind = ShapeType::Shape;
    // The element's own markup that no field reads is the `.odg`'s, and goes in the shape's `odf` slot at the end, never in the VSDX `raw` and `passthrough`.
    let mut odf_kept = Kept::default();
    // The automatic style the shape named, as written (the shape's own slot holds the name).
    let style_name = held::named(r, cx.shared, &attrs, Ns::Draw, "style-name")?;
    if let Some(name) = attrs.take(Ns::Draw, "name") {
        r.take_bytes(name.len().saturating_mul(2))?;
        shape.name = Some(Arc::from(name));
        shape.name_u = Some(Arc::from(name));
    }
    let z = attrs.take(Ns::Draw, "z-index").and_then(units::integer);
    // Styles.
    let graphic = attrs.take(Ns::Draw, "style-name").and_then(|n| cx.set.find(r, Family::Graphic, n, cx.file));
    let paragraph = attrs.peek(Ns::Draw, "text-style-name").and_then(|n| cx.set.find(r, Family::Paragraph, n, cx.file));
    let style = shape_style(r, cx, graphic, paragraph, !copy)?;
    let mut sheet = Sheet { links: style.links, ..Sheet::default() };
    // Layer.
    if let Some(layer) = attrs.peek(Ns::Draw, "layer")
        && let Some(at) = {
            r.report.work.name_lookups = r.report.work.name_lookups.saturating_add(1);
            pcx.layers.get(layer).copied()
        }
    {
        attrs.take(Ns::Draw, "layer");
        put(r, &mut sheet, "LayerMember", cells::text(&at.to_string()))?;
    }
    let transform = match attrs.peek(Ns::Draw, "transform") {
        None => None,
        Some(_) if kind == Kindof::Group => None,
        Some(text) => match parse_transform(r, text) {
            Some(t) => {
                attrs.take(Ns::Draw, "transform");
                Some(t)
            }
            None => {
                r.warn("transform_unreadable", || "a shape's draw:transform cannot be read; the shape reads as if it had none".to_owned());
                None
            }
        },
    };
    // The frame, and what is glued at each end of a connector.
    let ends = if kind == Kindof::Connector {
        [connectors::end_of(r, &mut attrs, connectors::Which::Begin, &sc.ids), connectors::end_of(r, &mut attrs, connectors::Which::End, &sc.ids)]
    } else {
        [None, None]
    };
    let one_d = matches!(kind, Kindof::Line | Kindof::Connector | Kindof::Object("measure"));
    let mut line = None;
    let (mut w, mut h, mut bounds) = if kind == Kindof::Group {
        (0.0, 0.0, Bounds { x0: 0.0, x1: 0.0, y0: 0.0, y1: 0.0 })
    } else if one_d {
        let (w, h, bounds, l) = read_line(r, &mut attrs, &mut sheet, transform.as_ref(), pcx.height, ends)?;
        line = Some(l);
        (w, h, bounds)
    } else {
        read_2d(r, &mut attrs, &mut sheet, kind, transform.as_ref(), pcx)?
    };
    // A group's frame is the box of its members: they are read first, and then measured in it.
    let mut members: Vec<Built> = Vec::new();
    if kind == Kindof::Group {
        if sc.depth >= MOST_GROUP_DEPTH {
            return Err(OdgError::TooComplex { what: "groups nested", max: MOST_GROUP_DEPTH as u64 });
        }
        sc.depth += 1;
        let level = read_level(r, names, cx, el, pcx, sc, false);
        sc.depth -= 1;
        let Level { shapes, passthrough } = level?;
        odf_kept.kept = passthrough;
        members = shapes;
        let framed = group_frame(r, &mut sheet, &mut members)?;
        (w, h, bounds) = framed;
        shape.kind = ShapeType::Group;
    }
    // Geometry by kind.
    let u = units_of(&sheet);
    let mut extra_glue: Vec<Arc<Row>> = Vec::new();
    let mut standard_glue = true;
    let mut boxed = glue_box(kind, attrs.peek(Ns::Draw, "kind"));
    let mut text_turn = None;
    match kind {
        Kindof::Rect => {
            rectangle(r, cx, &mut sheet)?;
            rounding(r, &mut attrs, &mut sheet)?;
        }
        Kindof::Ellipse | Kindof::Circle => ellipse_geometry(r, &mut attrs, &mut sheet, kind, (w, h), u)?,
        Kindof::Line => line_geometry(r, &mut sheet, w, u)?,
        Kindof::Polyline | Kindof::Polygon | Kindof::Path => {
            let sections = poly_geometry(r, &mut attrs, kind, pcx, (w, h))?;
            boxed = outline_boxed(&sheet, &sections, (w, h)).unwrap_or(boxed);
            geometry_sections(r, sections, w, h, &mut sheet)?;
        }
        Kindof::RegularPolygon => regular_polygon_geometry(r, &mut attrs, &mut sheet, pcx, (w, h))?,
        // A text frame has the rectangle, so that its style's fill and line draw.
        Kindof::Frame if matches!(shows, Some(FrameShows::Text(_))) => rectangle(r, cx, &mut sheet)?,
        Kindof::Custom => {
            let made = custom_geometry(r, names, cx, el, &mut sheet, (w, h, u), pcx, &style)?;
            object = object.or(made.object);
            extra_glue = made.glue;
            standard_glue = made.standard_glue;
            // An object the reader does not draw (an extrusion, a fontwork) keeps its text as the file has it.
            text_turn = made.object.is_none().then_some(made.text_turn);
        }
        Kindof::Connector => {
            if let Some(line) = line {
                connector_geometry(r, &mut attrs, &mut sheet, &line, pcx, w, u)?;
            }
            for (which, glue) in [(connectors::Which::Begin, ends[0]), (connectors::Which::End, ends[1])] {
                if let Some(glue) = glue {
                    // A `Connect` row: its fields and the two names it holds.
                    r.take_items(CONNECT_ITEMS)?;
                    sc.connects.push(connectors::connect(id, which, glue));
                }
            }
        }
        Kindof::Frame | Kindof::Group | Kindof::Object(_) | Kindof::Kept(_) => {}
    }
    let place = turn_text(r, &mut sheet, kind, text_turn, boxed)?;
    // The four standard connection rows, shared by shapes of one size, and the glue points a shape lists.
    let element_glue = connectors::glue_points(r, names, el, (w, h), u, extra_glue.len(), place)?;
    extra_glue.extend(element_glue);
    let (standard_glue, extra_glue) = own_glue(cx, (kind, copy, graphic), standard_glue, extra_glue);
    connection_rows(r, cx, &mut sheet, (w, h, u), (standard_glue, place), extra_glue)?;
    // The style's local cells.
    sheets::apply(r, &mut sheet, clone_out(&style.local))?;
    // Text, pictures, and the children kept.
    // A frame's text box or picture is read within its own namespace declarations.
    let mut shown_image = None;
    // What each paragraph of the shape's text named; the text's `.odg` data is made at the end, from the rows as they are then.
    let mut text_paragraphs: Vec<text::ParaOdf> = Vec::new();
    let mut bare_text = false;
    let (container, inner) = match shows {
        Some(FrameShows::Text(text_box)) => {
            let mark = names.enter(text_box);
            keep_untaken(r, names, text_box, &mut odf_kept.kept)?;
            (Some(text_box), mark)
        }
        Some(FrameShows::Image(image)) => {
            let mark = names.enter(image);
            shape.kind = ShapeType::Foreign;
            let mut foreign = picture(r, names, cx, image)?;
            keep_image_text(r, names, image, &mut foreign)?;
            shape.foreign = Some(Arc::new(foreign));
            (None, mark)
        }
        // An object frame shows its replacement image, the first `draw:image` it holds, when it has one.
        Some(FrameShows::Object) => {
            shape.kind = ShapeType::Foreign;
            let mut mark = 0;
            if let Some(image) = replacement_image(names, el) {
                mark = names.enter(image);
                shape.foreign = Some(Arc::new(picture(r, names, cx, image)?));
                shown_image = Some(image);
            }
            (None, mark)
        }
        None => (if kind == Kindof::Group || object.is_some() { None } else { Some(el) }, 0),
    };
    if object.is_some() && shape.kind != ShapeType::Foreign {
        shape.kind = ShapeType::Foreign;
    }
    mark_object(r, &mut sheet, &shape, object, frame_label)?;
    if let Some(container) = container {
        let t = text::read(r, names, cx, container, &style.text_layers, style.text_size)?;
        bare_text = keep_empty_text(r, names, container, t.text.is_none(), &mut odf_kept.kept)?;
        rows_of_text(r, &mut sheet, &t)?;
        shape.text = t.text.map(Arc::new);
        text_paragraphs = t.odf;
        mark_text_object(r, &mut sheet, t.holds_object)?;
    }
    names.leave(inner);
    // Children of the element that are not text: kept, in place.
    let keep = match (kind, shows) {
        (Kindof::Frame, Some(FrameShows::Object)) => Keep::AllBut(shown_image),
        (Kindof::Frame, _) => Keep::AllBut(shows.and_then(FrameShows::child)),
        _ if object.is_some() => Keep::AllBut(None),
        (Kindof::Group, _) => Keep::Members,
        _ => Keep::NotText,
    };
    keep_children(r, names, el, &mut odf_kept.kept, keep)?;
    // Attributes not mapped: kept as the file has them.
    // Charged, by count and by their bytes, before they are kept.
    let (kept, bytes) = attrs.untaken().fold((0usize, 0usize), |(n, b), a| (n + 1, b.saturating_add(a.name.len() + a.value.len())));
    r.take_items(kept.saturating_mul(2))?;
    r.take_bytes(bytes)?;
    // An attribute whose prefix an element above it declares on its own start tag (a page's, a shape's) is kept with that declaration, which only the file's own start tag held.
    let declared = attribute_declarations(r, names, attrs.untaken().map(|a| a.name))?;
    let mut raw = RawAttrs::from_pairs(declared.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    for a in attrs.untaken() {
        raw.push(a.name, a.value);
        if is_foreign(a.ns) {
            r.keep("foreign_attribute");
        }
    }
    r.keep_n("attribute", kept as u64);
    odf_kept.raw = raw;
    // Tier 2: what the shape's own automatic style carries over what its native markup gave. A copy is read without it
    // A group's own style carries none.
    let mut carries_tier2 = false;
    put_group_entry(r, pcx, (kind, bound), &mut shape, &mut sheet, &members)?;
    let (mut stale, mut stale_link, mut natives) = (Vec::new(), None, None);
    // The outline DiaCraft's data carried: the cells' outline is then not the element's, and no value of the element's outline is held beside them.
    let mut geometry_carried = false;
    if !copy && kind != Kindof::Group {
        let set = cx.set;
        if let Some(dia) = graphic.and_then(|g| set.get(g)).filter(|s| !s.common && !s.dia.tier2.is_empty()) {
            let mut tier2 = crate::ext::payload::decode(r, &dia.dia.tier2)?;
            // Tier 3: an item whose native outputs changed since DiaCraft wrote it (LibreOffice edited the path, the text, the name or a line end) is left out, and what
            // the markup says stands.
            (stale, stale_link, natives) = judge_tier2(r, names, el, kind, (&shape, &sheet, &style.local.other_ends), &mut tier2)?;
            geometry_carried = carries_geometry(&tier2);
            // **A text DiaCraft's data carries is the model's, not the markup's**: what the paragraphs of the markup named (their styles, lists, elements) was stamped
            // by rows the data now replaces, so it holds nothing the model's text rests on, and a writer that finds the model's paragraphs named by it would match the wrong rows.
            forget_markup_text(&tier2, &mut text_paragraphs);
            // The glue points the markup gave, whose rows a carried `Connection` section replaces: a row of it that is still what its element gives has the element held again.
            let glue = if tier2.sections.iter().any(|s| s.name.as_str() == "Connection") { connectors::markup_glue_rows(&sheet) } else { Vec::new() };
            crate::ext::payload::apply(tier2, &mut shape, &mut sheet)?;
            connectors::restore_held_glue(r, &mut sheet, &glue)?;
            carries_tier2 = true;
        }
    }
    let natives = with_name(kind, &shape, natives);
    // The `.odg` data of the shape: what no field reads, the style it named, the values its styles and its element hold beside their stand-in cells, with the stamps of the cells as
    // read (after tier 2), the units no cell spells and the paragraphs of its text.
    let facts = held::Facts {
        kind,
        el,
        attrs: &attrs,
        graphic,
        transform: transform.is_some(),
        one_d,
        style_name,
        kept: odf_kept,
        paragraphs: text_paragraphs,
        frame_text: shows_text(shows.as_ref()),
        bare_text,
        geometry_carried,
    };
    held::shape_data(r, names, cx.set, cx.shared, &mut shape, &sheet, facts)?;
    shape.sheet = Arc::new(sheet);
    shape.children = diacraft_doc::SharedVec::from_vec(members.into_iter().map(|b| Arc::new(b.shape)).collect());
    Ok(Some(Built { shape, z, bounds, tier2: carries_tier2, stale, stale_link, natives }))
}

/// Whether a frame shows a text box.
fn shows_text(shows: Option<&FrameShows<'_>>) -> bool {
    matches!(shows, Some(FrameShows::Text(_)))
}

/// Whether tier 2 carries a `Geometry` section: the cells' outline is then not the element's.
fn carries_geometry(tier2: &crate::ext::payload::Tier2) -> bool {
    tier2.sections.iter().any(|s| s.name.as_str() == "Geometry")
}

/// What the paragraphs of the markup named is left out when DiaCraft's data carries the text: see the call.
fn forget_markup_text(tier2: &crate::ext::payload::Tier2, paragraphs: &mut Vec<text::ParaOdf>) {
    if tier2.text.is_some() {
        paragraphs.clear();
    }
}

/// The natives of a shape with the fingerprint of its own `draw:name` among them, as the file has it: a save that gives a shape a name (a tier-2 item the file did not
/// carry) stamps the fingerprint of the output it leaves as it is from this, so that a rename in another program is told (the review's probe E). A group's names are in
/// the document file. Hashing the name is a pass over bytes the read has charged twice already (`Name` and `NameU`), and the fingerprint is eight bytes.
fn with_name(kind: Kindof, shape: &Shape, natives: Option<crate::ext::tier3::Natives>) -> Option<crate::ext::tier3::Natives> {
    if kind == Kindof::Group {
        return natives;
    }
    let mut natives = natives.unwrap_or_default();
    natives.name.get_or_insert_with(|| crate::ext::fingerprint::name_fp(shape.name_u.as_deref()));
    Some(natives)
}

/// The outline an element of `kind` has, for the fingerprint of its native outputs (`ext::fingerprint`): a custom shape's geometry element, a path's `svg:d`, the
/// points of a polygon or a polyline. The other kinds have none: their outline is their frame, an input.
fn outline_kind(kind: Kindof) -> Option<crate::ext::fingerprint::OutlineKind> {
    use crate::ext::fingerprint::OutlineKind;
    match kind {
        Kindof::Custom => Some(OutlineKind::Custom),
        Kindof::Path => Some(OutlineKind::Path),
        Kindof::Polyline | Kindof::Polygon => Some(OutlineKind::Points),
        _ => None,
    }
}

/// What tier 3 found of a shape: the keys of the stale items, the link to a master that went with them, and the natives it made (kept for the writer).
type Judged = (Vec<crate::ext::fingerprint::Key>, Option<crate::index::MasterLink>, Option<crate::ext::tier3::Natives>);

/// Tier 3 for one shape (`ext::tier3`): the fingerprints of the outputs its markup has now, the judgement of the fingerprints the tier 2 carries, and the items that
/// were stale taken out. Gives the keys that were stale and the link to a master that went with them. The work is charged: an item for each fingerprint, and the
/// bytes of the outline and the text that are hashed.
fn judge_tier2<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    el: &'t El,
    kind: Kindof,
    (shape, sheet, other_ends): (&Shape, &Sheet, &[&'static str]),
    tier2: &mut crate::ext::payload::Tier2,
) -> Result<Judged, OdgError> {
    use crate::ext::tier3;
    // A carried frame is judged by the markup's frame, and a carried fingerprint by the markup's outputs: a tier 2 that holds neither, and no item that rests on an
    // output (which the writer will stamp a fingerprint for), has nothing to judge.
    let needs = tier3::needs(tier2);
    if tier2.fps.is_empty() && tier2.cells.is_empty() && needs.count() == 0 {
        return Ok((Vec::new(), None, None));
    }
    r.take_items(tier2.fps.len().saturating_add(tier2.cells.len().min(16)).saturating_add(needs.count()))?;
    let natives = super::outputs::natives(r, names, el, outline_kind(kind), (shape, sheet, other_ends), &needs)?;
    let verdict = tier3::judge(tier2, &natives, sheet);
    if verdict.unreadable > 0 {
        r.warn_counted("diacraft_fingerprint_unreadable", verdict.unreadable as u64, || {
            "a fingerprint of DiaCraft's data on a shape is not 16 lowercase hexadecimal digits; the data it was for is read as it is".to_owned()
        });
    }
    if !verdict.stale.is_empty() {
        r.warn_counted("diacraft_stale", verdict.stale.len() as u64, || {
            "DiaCraft's data on a shape is left out where what it was written for changed in another program (its path, text, name or a line end); the file's own markup is read".to_owned()
        });
    }
    Ok((verdict.stale, verdict.master_link, Some(natives)))
}

/// What DiaCraft's document file says of a group (the shape is one when `kind` says so) is put over the sheet the markup gave it: the entry
/// the numbering pass found for it by the page and the group's name (the shape's `draw:name`) and its number among the groups of that name, or by its name alone
/// when no other group of the page has it, its cells, sections and removals, charged by the sections the overlay walks, and the links to a master it holds go on the
/// shape. (The group's `ID` was given by the numbering pass.)
fn put_group_entry(
    r: &mut Reader,
    pcx: &mut PageCx,
    (kind, bound): (Kindof, Binding),
    shape: &mut Shape,
    sheet: &mut Sheet,
    members: &[Built],
) -> Result<(), OdgError> {
    if kind != Kindof::Group {
        return Ok(());
    }
    if let Some(entry) = bound.take(&mut pcx.groups, shape.name_u.as_deref().unwrap_or_default()) {
        crate::ext::docfile::charge_overlay(r, sheet, &entry.sections, &entry.gone)?;
        crate::ext::payload::overlay(sheet, entry.cells, entry.sections, &entry.gone)?;
        (shape.master, shape.master_shape) = (entry.master, entry.master_shape);
        // The VSDX data of the group, judged by the names of its members: a member another program put in, took out or renamed is a change of what the group is, and the
        // item is stale (the file wins).
        if let Some(item) = entry.item {
            r.take_items(members.len())?;
            let now = crate::ext::items::group_fp(members.iter().map(|m| m.shape.name_u.as_deref()));
            let unnamed = shape.name_u.as_deref().is_none_or(str::is_empty);
            match item.fp {
                Some(written) if written != now => {
                    r.report.carry.stale = r.report.carry.stale.saturating_add(1);
                    r.warn_counted("diacraft_stale", 1, || {
                        "DiaCraft's data on a shape is left out where what it was written for changed in another program (its path, text, name or a line end); the file's own markup is read".to_owned()
                    });
                }
                // The stamp of a group with no name that another such group of the page has too tells neither apart (another program may have put one in the other's
                // place), so the data is no group's.
                Some(written) if unnamed && pcx.ambiguous.contains(&written) => {
                    r.report.carry.left_out = r.report.carry.left_out.saturating_add(1);
                    r.warn_counted("diacraft_group_item_ambiguous", 1, || {
                        "DiaCraft's data of a group with no name is left out: another group of the page with no name has members of the same names, so which group it was written for cannot be told".to_owned()
                    });
                }
                _ => {
                    crate::ext::shell::apply(shape, sheet, &item.shell);
                    r.report.carry.kept = r.report.carry.kept.saturating_add(1);
                }
            }
        }
    }
    Ok(())
}

/// A group's frame: the smallest upright box holding every member's box. The members, read in page coordinates, are put in the
/// group's frame by taking the box's bottom-left corner off their pins and ends; the group gets the frame cells, no angle, no flip.
fn group_frame(r: &mut Reader, sheet: &mut Sheet, members: &mut [Built]) -> Result<(f64, f64, Bounds), OdgError> {
    let bounds = members.iter().map(|m| m.bounds).reduce(Bounds::union).unwrap_or(Bounds { x0: 0.0, x1: 0.0, y0: 0.0, y1: 0.0 });
    let (w, h) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
    let unit = members.first().map(|m| units_of(&m.shape.sheet));
    let (wu, hu) = unit.map_or((None, None), |u| (u.width, u.height));
    for m in members.iter_mut() {
        translate(&mut m.shape, -bounds.x0, -bounds.y0);
    }
    put(r, sheet, "Width", cells::length(w, wu))?;
    put(r, sheet, "Height", cells::length(h, hu))?;
    let locx = r.forms.cell("Width*0.5", w * 0.5, wu);
    let locy = r.forms.cell("Height*0.5", h * 0.5, hu);
    put(r, sheet, "LocPinX", locx)?;
    put(r, sheet, "LocPinY", locy)?;
    put(r, sheet, "PinX", cells::length((bounds.x0 + bounds.x1) / 2.0, wu))?;
    put(r, sheet, "PinY", cells::length((bounds.y0 + bounds.y1) / 2.0, hu))?;
    Ok((w, h, bounds))
}

/// Moves a member by (`dx`, `dy`) inches: the cached values of its pin and ends, as they are measured from the frame of the group that
/// holds it. A member's own members are in its own frame and do not move.
fn translate(shape: &mut Shape, dx: f64, dy: f64) {
    let sheet = Arc::make_mut(&mut shape.sheet);
    for (names, d) in [(["PinX", "BeginX", "EndX"], dx), (["PinY", "BeginY", "EndY"], dy)] {
        for name in names {
            if let Some(cell) = sheet.cells.get_mut(name)
                && let Some(v) = cell.value.as_ref().and_then(diacraft_doc::Value::number)
            {
                cell.value = Some(cells::number(v + d));
            }
        }
    }
}

/// The box LibreOffice measures the glue points of a polygon, a polyline or a path in: the box of the outline its `sections` draw, on the page, at the turn and
/// the flips the frame already has (a polygon has no mirror of its own); `None` when its extent is not worked out (`connectors::outline_extent`).
fn outline_boxed(sheet: &Sheet, sections: &[geometry::Section], size: (f64, f64)) -> Option<connectors::Boxed> {
    let flips = (number_of(sheet, "FlipX") == Some(1.0), number_of(sheet, "FlipY") == Some(1.0));
    connectors::outline_extent(sections, number_of(sheet, "Angle").unwrap_or(0.0), flips, size).map(connectors::Boxed::Outline)
}

/// Whether a shape has the standard rows, and its rows past them: a custom shape whose style says its rows start at 0 with its first listed glue point
/// (`diacraft:glue="own"`) has none, and glue id 4 + r is its row r. A copy is read without DiaCraft's data.
fn own_glue(cx: &Cx<'_>, (kind, copy, graphic): (Kindof, bool, Option<usize>), standard: bool, rows: Vec<Arc<Row>>) -> (bool, Vec<Arc<Row>>) {
    let own = kind == Kindof::Custom && !copy && graphic.and_then(|g| cx.set.get(g)).is_some_and(|s| !s.common && s.dia.glue_own);
    if own { (false, rows.into_iter().map(own_row).collect()) } else { (standard, rows) }
}

/// A row of a shape whose rows start at 0 with its first listed glue point: glue id 4 + r is row r ([`connectors::own_glue_row`]).
fn own_row(row: Arc<Row>) -> Arc<Row> {
    let mut row = Arc::unwrap_or_clone(row);
    row.ix = row.ix.and_then(connectors::own_glue_row);
    Arc::new(row)
}

/// The connection rows of a shape: the four standard rows, shared by shapes of one size, unless the
/// shape has none (a custom shape of glue-point type `none` that lists no points, or one whose rows are its own), and the rows past them. A shape whose glue points
/// LibreOffice puts on the box of its outline has the standard rows on that box (`connectors::outline_standard_rows`) when it is not the square frame's.
fn connection_rows(
    r: &mut Reader,
    cx: &mut Cx<'_>,
    sheet: &mut Sheet,
    size: (f64, f64, Units),
    (standard, place): (bool, connectors::Place),
    extra: Vec<Arc<Row>>,
) -> Result<(), OdgError> {
    let (w, h, u) = size;
    if standard && let Some(mut section) = connectors::outline_standard_rows(&mut r.forms, place, (w, h), u) {
        r.take_cells(20)?;
        r.take_items(super::cost::SECTION + 4 * super::cost::ROW)?;
        for made in extra {
            section.rows.push(made);
        }
        sheet.sections.push(Arc::new(section));
        return Ok(());
    }
    if extra.is_empty() {
        if !standard {
            return Ok(());
        }
        let key = (w.to_bits(), h.to_bits(), u.width, u.height);
        let section = match cx.shared.connections.get(&key) {
            Some(section) => {
                r.take_items(1)?;
                Arc::clone(section)
            }
            None => {
                r.take_cells(20)?;
                r.take_items(super::cost::SECTION + 4 * super::cost::ROW)?;
                let section = Arc::new(connection(&mut r.forms, w, h, u));
                if cx.shared.connections.len() < MOST_SHARED {
                    cx.shared.connections.insert(key, Arc::clone(&section));
                }
                section
            }
        };
        sheet.sections.push(section);
        return Ok(());
    }
    let mut section = if standard {
        r.take_cells(20)?;
        r.take_items(super::cost::SECTION + 4 * super::cost::ROW)?;
        connection(&mut r.forms, w, h, u)
    } else {
        r.take_items(super::cost::SECTION)?;
        Section::new("Connection", None)
    };
    for made in extra {
        section.rows.push(made);
    }
    sheet.sections.push(Arc::new(section));
    Ok(())
}

/// The ellipse, circle and their kinds.
fn ellipse_geometry(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    sheet: &mut Sheet,
    kind: Kindof,
    size: (f64, f64),
    u: Units,
) -> Result<(), OdgError> {
    let (w, h) = size;
    let circle = kind == Kindof::Circle;
    let kind_word = attrs.take(Ns::Draw, "kind").unwrap_or("full");
    let start = attrs.take(Ns::Draw, "start-angle").and_then(units::angle_degrees).unwrap_or(0.0);
    let end = attrs.take(Ns::Draw, "end-angle").and_then(units::angle_degrees).unwrap_or(360.0);
    let square = if circle && w > 0.0 && h > 0.0 {
        let s = w.min(h);
        ((w - s) / 2.0 / w, (h - s) / 2.0 / h, s / w, s / h)
    } else {
        (0.0, 0.0, 1.0, 1.0)
    };
    let ratio = if circle || h == 0.0 { 1.0 } else { w / h };
    let sections = if kind_word == "full" { None } else { geometry::kind(kind_word, start, end, square, ratio) };
    match sections {
        Some(sections) => geometry_sections(r, sections, w, h, sheet)?,
        None => {
            if !matches!(kind_word, "full" | "section" | "cut" | "arc") {
                r.keep("property_value");
            }
            let section = ellipse_section(&mut r.forms, circle, w, h, u);
            r.take_cells(6)?;
            r.take_items(2)?;
            sheet.sections.push(Arc::new(section));
        }
    }
    Ok(())
}

/// A `draw:line`'s geometry: `MoveTo` (0, 0) and `LineTo` (`Width*1`, 0).
fn line_geometry(r: &mut Reader, sheet: &mut Sheet, w: f64, u: Units) -> Result<(), OdgError> {
    let mut section = Section::new("Geometry", Some(0));
    section.rows.push(row(1, Some("MoveTo"), vec![("X", cells::length(0.0, u.width)), ("Y", cells::length(0.0, u.width))]));
    section.rows.push(row(2, Some("LineTo"), vec![("X", r.forms.cell("Width*1", w, u.width)), ("Y", cells::length(0.0, u.width))]));
    r.take_cells(4)?;
    r.take_items(super::cost::SECTION + 2 * super::cost::ROW)?;
    sheet.sections.push(Arc::new(section));
    Ok(())
}

/// `draw:regular-polygon`'s geometry.
fn regular_polygon_geometry(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    sheet: &mut Sheet,
    pcx: &mut PageCx,
    size: (f64, f64),
) -> Result<(), OdgError> {
    let (w, h) = size;
    let corners = attrs.take(Ns::Draw, "corners").and_then(units::integer).and_then(|n| u32::try_from(n).ok());
    let concave = attrs.take(Ns::Draw, "concave").and_then(units::boolean).unwrap_or(false);
    let sharpness = attrs.take(Ns::Draw, "sharpness").and_then(units::percent).map_or(0.0, |p| p / 100.0);
    // A concave polygon has twice as many points as corners. Its rows, the points and the closing row, are taken from the page's
    // budget before they are made.
    let points = corners.map(|c| if concave { u64::from(c).saturating_mul(2) } else { u64::from(c) });
    let fits = corners.is_some_and(|c| c >= 3) && points.is_some_and(|n| n <= r.limits.max_points as u64);
    let sections = match (corners, points) {
        (Some(c), Some(n)) if fits => {
            pcx.budget.take(usize::try_from(n).unwrap_or(usize::MAX).saturating_add(1))?;
            geometry::regular_polygon(c, concave, sharpness)
        }
        _ => None,
    };
    match sections {
        Some(sections) => geometry_sections(r, sections, w, h, sheet),
        None => {
            r.warn("geometry_unreadable", || "a regular polygon's corners cannot be read; it is drawn as its frame".to_owned());
            geometry_sections(r, geometry::rectangle(), w, h, sheet)
        }
    }
}

/// A connector's geometry and route cells and the marker of a route DiaCraft may route again.
fn connector_geometry(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    sheet: &mut Sheet,
    line: &connectors::Line,
    pcx: &mut PageCx,
    w: f64,
    u: Units,
) -> Result<(), OdgError> {
    for (name, cell) in connectors::route_cells(attrs) {
        put(r, sheet, name, cell)?;
    }
    match connectors::route(r, attrs, line, pcx.height, &mut pcx.budget)? {
        Some(section) => sheet.sections.push(Arc::new(section)),
        // No path: the route DiaCraft's router makes is the engine's to make (it routes a connector marked below again); a straight segment
        // stands until then.
        None => line_geometry(r, sheet, w, u)?,
    }
    r.take_items(super::cost::ROUTE_MARKER)?;
    r.take_cells(1)?;
    sheet.sections.push(Arc::new(connectors::route_marker()));
    Ok(())
}

/// What a custom shape's geometry gave besides its sections.
struct CustomMade {
    glue: Vec<Arc<Row>>,
    standard_glue: bool,
    object: Option<&'static str>,
    /// `draw:text-rotate-angle` in degrees, as the file says it (0 when it says none): the shape's `TxtAngle` is made of it once the flips are known ([`text_turn`]).
    text_turn: f64,
}

/// The geometry of a custom shape: its `draw:enhanced-geometry`, read into the sheet's sections and cells.
#[allow(clippy::too_many_arguments)]
fn custom_geometry<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    cx: &mut Cx<'_>,
    el: &'t El,
    sheet: &mut Sheet,
    size: (f64, f64, Units),
    pcx: &mut PageCx,
    style: &ShapeStyle,
) -> Result<CustomMade, OdgError> {
    let (w, h, u) = size;
    let mut made = CustomMade { glue: Vec::new(), standard_glue: true, object: None, text_turn: 0.0 };
    let mut eg = None;
    for child in el.elements() {
        let mark = names.enter(child);
        let is = names.is(child, Ns::Draw, "enhanced-geometry");
        names.leave(mark);
        if is {
            eg = Some(child);
            break;
        }
    }
    let Some(eg) = eg else {
        // A custom shape with no geometry at all is drawn as its frame's box.
        r.keep("shape_type");
        r.report.boxed_shapes = r.report.boxed_shapes.saturating_add(1);
        rectangle(r, cx, sheet)?;
        return Ok(made);
    };
    let has = style.patterns;
    let mark = names.enter(eg);
    let result = custom::read(r, names, eg, (w, h, u), &mut pcx.budget, has, true);
    names.leave(mark);
    let custom = result?;
    if let Some(kind) = custom.object {
        r.keep(kind);
        made.object = Some(kind);
        return Ok(made);
    }
    made.glue = custom.glue;
    made.standard_glue = custom.standard_glue;
    if custom.as_box {
        rectangle(r, cx, sheet)?;
    } else {
        sheet.sections.extend(custom.sections);
    }
    // One mirror and a turn: the file's turn is minus the model's ([`turn_beside_mirrors`]), **whatever the frame's own transform does**. LibreOffice 26.8.2.2 draws the
    // mirror across the page's line through the frame's centre after the whole of the transform, so beside a transform that flips the frame itself (a negative
    // determinant, from `scale` or `matrix`: the `FlipY` read above, which a mirror below may undo) the turn is the other way round too (`scale (1 -1)`,
    // `scale (-1 1)` and a reflecting `matrix` by the four mirror states at 0, 30 and 135 degrees, every one drawn as LibreOffice draws it).
    if custom.mirror_x != custom.mirror_y {
        let turn = sheet.cells.get("Angle").and_then(|c| c.value.as_ref()?.number());
        if let Some(turn) = turn {
            sheet.cells.insert(CellName::new("Angle"), cells::angle(turn_beside_mirrors(turn, custom.mirror_x, custom.mirror_y)));
        }
    }
    if custom.mirror_x {
        put(r, sheet, "FlipX", cells::plain(1.0))?;
    }
    if custom.mirror_y {
        // A turn of the frame that already flips it (a negative determinant) and a mirror together are no flip.
        if sheet.cells.remove("FlipY").is_none() {
            put(r, sheet, "FlipY", cells::plain(1.0))?;
        }
    }
    for (name, cell) in custom.text_block {
        sheet.cells.insert(CellName::new(name), cell);
    }
    made.text_turn = custom.text_turn;
    if let Some(user) = custom.user {
        sheet.sections.push(Arc::new(user));
    }
    if let Some(controls) = custom.controls {
        r.take_items(super::cost::SECTION)?;
        sheet.sections.push(Arc::new(controls));
    }
    Ok(made)
}

/// Whether the pattern cell `name` (`FillPattern` or `LinePattern`) of a shape with these links and local cells is not 0: its own cell, else its
/// style sheet's, else the root's. The walk up the style sheets is at most [`Sheets::MOST_LINKS`] steps.
fn pattern_on(cx: &Cx<'_>, links: Links, local: &Out, name: &str) -> bool {
    if let Some((_, cell)) = local.direct.iter().find(|(n, _)| *n == name) {
        return cell.value.as_ref().and_then(diacraft_doc::Value::number).is_none_or(|v| v != 0.0);
    }
    let link = if name == "FillPattern" { links.fill } else { links.line };
    cx.sheets.number(link.unwrap_or(0), name).is_none_or(|v| v != 0.0)
}

/// The units the frame cells were given, for the cells written after them.
fn units_of(sheet: &Sheet) -> Units {
    let unit = |name: &str| -> Option<&'static str> {
        let u = sheet.cells.get(name)?.unit.as_deref()?;
        ["CM", "MM", "IN", "PT", "P"].into_iter().find(|w| *w == u)
    };
    Units { width: unit("Width").or(unit("BeginX")), height: unit("Height").or(unit("BeginX")) }
}

/// A copy of a conversion's cells.
fn clone_out(out: &Out) -> Out {
    Out {
        direct: out.direct.clone(),
        paragraph: out.paragraph.clone(),
        character: out.character.clone(),
        stops: out.stops.clone(),
        stops_through: out.stops_through,
        tabs: out.tabs.clone(),
        other_ends: out.other_ends.clone(),
    }
}

/// The frame of a 2-D shape: its cells, and its size.
fn read_2d(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    sheet: &mut Sheet,
    kind: Kindof,
    transform: Option<&Transform>,
    pcx: &PageCx,
) -> Result<(f64, f64, Bounds), OdgError> {
    let mut x = take_len(r, attrs, Ns::Svg, "x");
    let mut y = take_len(r, attrs, Ns::Svg, "y");
    let mut w = take_len(r, attrs, Ns::Svg, "width");
    let mut h = take_len(r, attrs, Ns::Svg, "height");
    let mut centre_unit = None;
    // A circle or ellipse given by its centre: these win over the frame attributes.
    if matches!(kind, Kindof::Circle | Kindof::Ellipse) {
        let cx_ = attrs.peek(Ns::Svg, "cx").and_then(units::length);
        let cy_ = attrs.peek(Ns::Svg, "cy").and_then(units::length);
        let radii = if kind == Kindof::Circle {
            attrs.peek(Ns::Svg, "r").and_then(units::length).map(|r| (r, r))
        } else {
            match (attrs.peek(Ns::Svg, "rx").and_then(units::length), attrs.peek(Ns::Svg, "ry").and_then(units::length)) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            }
        };
        if let (Some(ccx), Some(ccy), Some((rx, ry))) = (cx_, cy_, radii) {
            for local in ["cx", "cy", "r", "rx", "ry"] {
                attrs.take(Ns::Svg, local);
            }
            centre_unit = Some(ccx.unit);
            x = Some(Length { inches: ccx.inches - rx.inches, unit: ccx.unit });
            y = Some(Length { inches: ccy.inches - ry.inches, unit: ccy.unit });
            w = Some(Length { inches: 2.0 * rx.inches, unit: rx.unit });
            h = Some(Length { inches: 2.0 * ry.inches, unit: ry.unit });
        }
    }
    let (xi, yi) = (x.map_or(0.0, |l| l.inches), y.map_or(0.0, |l| l.inches));
    let (wi, hi) = (w.map_or(0.0, |l| l.inches.max(0.0)), h.map_or(0.0, |l| l.inches.max(0.0)));
    let (frame, sheared) = frame_of(xi, yi, wi, hi, transform);
    if sheared {
        r.warn("shear_left_out", || "a shape's transform shears it, which no cell holds; it is read without the shear".to_owned());
    }
    let width_unit = w.and_then(|l| l.unit.word());
    let height_unit = h.and_then(|l| l.unit.word());
    let pin_unit = match centre_unit.or(x.map(|l| l.unit)).or(transform.and_then(|t| t.translate_unit)).or(w.map(|l| l.unit)) {
        Some(u) => u.word(),
        None => None,
    };
    put(r, sheet, "Width", cells::length(frame.width, width_unit))?;
    put(r, sheet, "Height", cells::length(frame.height, height_unit))?;
    let locx = r.forms.cell("Width*0.5", frame.width * 0.5, width_unit);
    let locy = r.forms.cell("Height*0.5", frame.height * 0.5, height_unit);
    put(r, sheet, "LocPinX", locx)?;
    put(r, sheet, "LocPinY", locy)?;
    put(r, sheet, "PinX", cells::length(frame.pin.0, pin_unit))?;
    put(r, sheet, "PinY", cells::length(pcx.height - frame.pin.1, pin_unit))?;
    if frame.angle != 0.0 {
        put(r, sheet, "Angle", cells::angle(frame.angle))?;
    }
    if frame.flip_y {
        put(r, sheet, "FlipY", cells::plain(1.0))?;
    }
    let bounds = Bounds::of_frame((frame.pin.0, pcx.height - frame.pin.1), frame.width, frame.height, frame.angle);
    Ok((frame.width, frame.height, bounds))
}

/// The cells of a `draw:line`: its ends after `draw:transform`, y flipped, and the 1-D formulas
/// `connector.add` writes.
fn read_line(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    sheet: &mut Sheet,
    transform: Option<&Transform>,
    page_height: f64,
    glue: [Option<connectors::Glue>; 2],
) -> Result<(f64, f64, Bounds, connectors::Line), OdgError> {
    let x1 = take_len(r, attrs, Ns::Svg, "x1");
    let y1 = take_len(r, attrs, Ns::Svg, "y1");
    let x2 = take_len(r, attrs, Ns::Svg, "x2");
    let y2 = take_len(r, attrs, Ns::Svg, "y2");
    let map = transform.map_or(Affine::IDENTITY, |t| t.map);
    let begin = map.apply(x1.map_or(0.0, |l| l.inches), y1.map_or(0.0, |l| l.inches));
    let end = map.apply(x2.map_or(0.0, |l| l.inches), y2.map_or(0.0, |l| l.inches));
    let (bx, by, ex, ey) = (begin.0, page_height - begin.1, end.0, page_height - end.1);
    let bu = x1.and_then(|l| l.unit.word());
    let eu = x2.and_then(|l| l.unit.word());
    let width = (ex - bx).hypot(ey - by);
    let angle = (ey - by).atan2(ex - bx);
    // An end glued to a connection point holds the formula `connector.add` writes, with the end's place as its value; any other end holds
    // its place.
    let end_cell = |r: &mut Reader, glue: Option<connectors::Glue>, v: f64, unit: Option<&str>| -> Cell {
        match glue {
            Some(connectors::Glue { target, row: Some(row) }) => r.forms.cell(&connectors::glue_formula(target, row), v, unit),
            _ => cells::length(v, unit),
        }
    };
    let cells_end = [
        ("BeginX", end_cell(r, glue[0], bx, bu)),
        ("BeginY", end_cell(r, glue[0], by, bu)),
        ("EndX", end_cell(r, glue[1], ex, eu)),
        ("EndY", end_cell(r, glue[1], ey, eu)),
    ];
    for (name, cell) in cells_end {
        put(r, sheet, name, cell)?;
    }
    let f = &mut r.forms;
    let cells_1d = [
        ("Width", f.cell("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)", width, bu)),
        ("Height", cells::length(0.0, bu)),
        ("Angle", f.cell("ATAN2(EndY-BeginY,EndX-BeginX)", if angle.is_finite() { angle } else { 0.0 }, Some("DEG"))),
        ("PinX", f.cell("(BeginX+EndX)/2", (bx + ex) / 2.0, bu)),
        ("PinY", f.cell("(BeginY+EndY)/2", (by + ey) / 2.0, bu)),
        ("LocPinX", f.cell("Width*0.5", width * 0.5, bu)),
        ("LocPinY", f.cell("Height*0.5", 0.0, bu)),
    ];
    for (name, cell) in cells_1d {
        put(r, sheet, name, cell)?;
    }
    let line = connectors::Line { begin: (bx, by), end: (ex, ey), unit: bu };
    Ok((width, 0.0, Bounds::of_ends((bx, by), (ex, ey)), line))
}

/// `Rounding` from `draw:corner-radius`, or `svg:rx` and `svg:ry`: one of the two standing for both, both
/// overriding the corner radius, the smaller when they differ (both kept).
fn rounding(r: &mut Reader, attrs: &mut super::names::Attrs<'_>, sheet: &mut Sheet) -> Result<(), OdgError> {
    let rx = attrs.peek(Ns::Svg, "rx").and_then(units::length);
    let ry = attrs.peek(Ns::Svg, "ry").and_then(units::length);
    let corner = attrs.peek(Ns::Draw, "corner-radius").and_then(units::length);
    let radius = match (rx, ry) {
        (Some(a), Some(b)) => {
            if a.inches != b.inches {
                r.keep("corner_radii");
            } else {
                attrs.take(Ns::Svg, "rx");
                attrs.take(Ns::Svg, "ry");
            }
            Some(if a.inches <= b.inches { a } else { b })
        }
        (Some(one), None) => {
            attrs.take(Ns::Svg, "rx");
            Some(one)
        }
        (None, Some(one)) => {
            attrs.take(Ns::Svg, "ry");
            Some(one)
        }
        (None, None) => corner.inspect(|_| {
            attrs.take(Ns::Draw, "corner-radius");
        }),
    };
    if let Some(l) = radius {
        put(r, sheet, "Rounding", cells::length(l.inches.max(0.0), l.unit.word()))?;
    }
    Ok(())
}

/// The geometry of a polyline, polygon or path.
fn poly_geometry(
    r: &mut Reader,
    attrs: &mut super::names::Attrs<'_>,
    kind: Kindof,
    pcx: &mut PageCx,
    size: (f64, f64),
) -> Result<Vec<geometry::Section>, OdgError> {
    let vb = match attrs.peek(Ns::Svg, "viewBox").map(geometry::view_box) {
        Some(Some(Ok(vb))) => vb,
        Some(Some(Err(_))) => {
            r.warn("view_box_empty", || "a shape's view box has a width or height of 0; it is drawn as its frame".to_owned());
            return Ok(geometry::rectangle());
        }
        _ => {
            r.warn("view_box_unreadable", || "a shape's view box is missing or cannot be read; it is drawn as its frame".to_owned());
            return Ok(geometry::rectangle());
        }
    };
    attrs.take(Ns::Svg, "viewBox");
    if kind == Kindof::Path {
        let Some(d) = attrs.peek(Ns::Svg, "d") else {
            r.warn("geometry_unreadable", || "a path has no path data; it is drawn as its frame".to_owned());
            return Ok(geometry::rectangle());
        };
        return match geometry::path(d, &vb, size, &mut pcx.budget)? {
            Ok(sections) => {
                attrs.take(Ns::Svg, "d");
                Ok(sections)
            }
            Err(_) => {
                r.warn("geometry_unreadable", || "a path's data breaks the syntax; it is drawn as its frame".to_owned());
                Ok(geometry::rectangle())
            }
        };
    }
    let Some(text) = attrs.peek(Ns::Draw, "points") else {
        r.warn("geometry_unreadable", || "a polyline or polygon has no points; it is drawn as its frame".to_owned());
        return Ok(geometry::rectangle());
    };
    let max = r.limits.max_points;
    match geometry::points(text, max, &mut pcx.budget)? {
        Some(points) => {
            attrs.take(Ns::Draw, "points");
            Ok(geometry::poly(&points, &vb, kind == Kindof::Polygon))
        }
        None => {
            r.warn("geometry_unreadable", || "a polyline's or polygon's points cannot be read; it is drawn as its frame".to_owned());
            Ok(geometry::rectangle())
        }
    }
}

/// Which children of a shape [`keep_children`] leaves out: a shape's own text, which was read; or, for a frame, the one child it shows.
#[derive(Clone, Copy)]
enum Keep<'t> {
    NotText,
    AllBut(Option<&'t El>),
    /// A group's: its members and the children between them were kept when the members were read.
    Members,
}

/// Keeps the children of `el` it did not read in `passthrough`, each counted by kind: for a shape, all but its text; for a frame, all but
/// the child it shows (a second picture or text box, or text outside a text box, is kept too).
///
/// **A frame's kept child says which side of the child it shows it stood on**: its `after` is 0 for a child before the shown one and 1 for a child after it (the one child the
/// frame shows is the one typed child it has), so that a writer that puts the frame's picture back writes the images in the order the file had them. A frame of two PNGs shows the first and
/// keeps the second, and a writer that wrote the kept one first would give the frame the other picture on its next read.
fn keep_children<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, passthrough: &mut Passthrough, keep: Keep<'t>) -> Result<(), OdgError> {
    if matches!(keep, Keep::Members) {
        return Ok(());
    }
    let mut after = 0u32;
    for node in &el.children {
        match node {
            Node::El(child) => {
                let mark = names.enter(child);
                let skip = match keep {
                    Keep::NotText => text::is_text(names, child),
                    Keep::AllBut(shown) => shown.is_some_and(|s| std::ptr::eq(s, child)),
                    Keep::Members => true,
                };
                if !skip {
                    r.keep(kept_word(names, child, matches!(keep, Keep::AllBut(_))));
                    r.take_items(element_cost(child))?;
                    passthrough.push(after, RawNode::Element(raw_element_in(r, names, child)?));
                } else if matches!(keep, Keep::AllBut(Some(_))) {
                    after = 1;
                }
                names.leave(mark);
            }
            Node::Text(t) | Node::CData(t) if !t.bytes().all(|b| b.is_ascii_whitespace()) => {
                r.keep("shape_text_node");
                r.take_items(1)?;
                r.take_bytes(t.len())?;
                passthrough.push(after, RawNode::Text(Arc::from(t.as_str())));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The word a child kept in a shape's passthrough is counted under.
fn kept_word<'t>(names: &Names<'t>, child: &'t El, frame: bool) -> &'static str {
    match names.element(child) {
        (Ns::Svg, "title" | "desc") => "title",
        (Ns::Draw, "glue-point") => "glue_point",
        (Ns::Office, "event-listeners") => "event_listeners",
        (Ns::Draw, "image-map") => "image_map",
        (Ns::Draw, "enhanced-geometry") => "enhanced_geometry",
        (Ns::Draw, "image") if frame => "frame_image",
        (Ns::Draw, "text-box") if frame => "frame_text_box",
        _ => "shape_child",
    }
}

/// Keeps the text children of `container` whole when its text is **empty** (no characters: R27, no `Text`), unless each is a `text:p` with no attribute and nothing in it
/// A heading, a list, a paragraph with a style or an attribute, a mark (a bookmark) are what an empty text still says, and the model holds a text only when it
/// has characters. Kept in place among the shape's other markup, and counted as the text's own (`empty_text_markup`). Says whether a bare `text:p` was among them
/// (the shape's element holds it, so that a save writes the paragraph the file had and no other).
fn keep_empty_text<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    container: &'t El,
    empty: bool,
    passthrough: &mut Passthrough,
) -> Result<bool, OdgError> {
    if !empty {
        return Ok(false);
    }
    let mut had_bare = false;
    for child in container.elements() {
        let mark = names.enter(child);
        let bare = names.is(child, Ns::Text, "p") && child.attrs.is_empty() && child.elements().next().is_none();
        had_bare |= bare;
        if text::is_text(names, child) && !bare {
            r.keep("empty_text_markup");
            r.take_items(element_cost(child))?;
            passthrough.push(0, RawNode::Element(raw_element_in(r, names, child)?));
        }
        names.leave(mark);
    }
    Ok(had_bare)
}

/// **The paragraphs of a picture frame's `draw:image`**: the words LibreOffice draws on a picture, which it keeps there, and which no field of the
/// model reads (a picture's text is not the shape's). They are kept whole on the picture's foreign data, in order, for a model writer to write back inside the `draw:image`
/// they were in, **when one of them holds anything**: LibreOffice writes an empty `text:p` in every image, and an image of empty paragraphs keeps nothing, as before. Each is charged by its
/// nodes before it is kept.
fn keep_image_text<'t>(r: &mut Reader, names: &mut Names<'t>, image: &'t El, foreign: &mut ForeignData) -> Result<(), OdgError> {
    let mut texts: Vec<&'t El> = Vec::new();
    for child in image.elements() {
        let mark = names.enter(child);
        if text::is_text(names, child) {
            texts.push(child);
        }
        names.leave(mark);
    }
    if texts.iter().all(|p| p.attrs.is_empty() && p.children.is_empty()) {
        return Ok(());
    }
    for child in texts {
        r.keep("picture_text");
        r.take_items(element_cost(child))?;
        let made = raw_element_in(r, names, child)?;
        foreign.ext.make_mut().odf.kept.push(0, RawNode::Element(made));
    }
    Ok(())
}

/// Keeps what a frame's text box holds besides its text.
fn keep_untaken<'t>(r: &mut Reader, names: &mut Names<'t>, text_box: &'t El, passthrough: &mut Passthrough) -> Result<(), OdgError> {
    for child in text_box.elements() {
        let mark = names.enter(child);
        if !text::is_text(names, child) {
            r.keep("text_box_child");
            r.take_items(element_cost(child))?;
            passthrough.push(0, RawNode::Element(raw_element_in(r, names, child)?));
        }
        names.leave(mark);
    }
    Ok(())
}

/// The rows the text needs: `Paragraph`, `Character` and `Field`, by `IX` from 0. A row the shape's style already holds at `IX` 0 (its
/// local text cells) is merged, the text's own cells winning.
fn rows_of_text(r: &mut Reader, sheet: &mut Sheet, t: &text::TextOut) -> Result<(), OdgError> {
    let model = |e: diacraft_doc::DocError| OdgError::Model(e);
    for (section, rows) in [("Paragraph", &t.paragraphs), ("Character", &t.characters)] {
        if rows.len() > diacraft_doc::MAX_ROWS_PER_SECTION {
            return Err(OdgError::TooComplex { what: "text rows in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
        }
        for (ix, cells) in rows.iter().enumerate() {
            // A row with no cells of its own is no row: its cells resolve as if it were absent.
            if cells.is_empty() {
                continue;
            }
            let key = RowKey::Ix(u32::try_from(ix).unwrap_or(u32::MAX));
            let row = sheet.row_mut(&diacraft_doc::SectionKey::new(section, None), &key).map_err(model)?;
            for (name, cell) in cells {
                row.cells.insert(CellName::new(name), cell.clone());
            }
        }
    }
    if t.fields.len() > diacraft_doc::MAX_ROWS_PER_SECTION {
        return Err(OdgError::TooComplex { what: "fields in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
    }
    for (ix, cell) in t.fields.iter().enumerate() {
        let key = RowKey::Ix(u32::try_from(ix).unwrap_or(u32::MAX));
        let row = sheet.row_mut(&diacraft_doc::SectionKey::new("Field", None), &key).map_err(model)?;
        row.cells.insert(CellName::new("Value"), cell.clone());
    }
    let _ = r;
    Ok(())
}

/// The keys a resolution holds whose value comes from a style that is not common: what a shape's automatic styles set.
fn automatic_keys(set: &StyleSet, resolved: &Resolved, out: &mut HashSet<(Kind, P)>) {
    for (key, found) in resolved.iter() {
        if set.get(found.style).is_some_and(|s| !s.common && !s.default) {
            out.insert(*key);
        }
    }
}

/// The view of a chain from `style` (when there is one) over the default styles.
fn view_from<'a>(r: &mut Reader, cx: &mut Cx<'a>, styles: &[Option<usize>]) -> Result<View<'a>, OdgError> {
    let mut layers = Vec::new();
    for at in styles.iter().flatten() {
        layers.push((cx.res.resolved(r, cx.set, *at)?, Serves::All));
    }
    layers.extend(cx.sheets.defaults.iter().cloned());
    Ok(View { set: cx.set, layers })
}

/// The links and local cells of a shape whose `draw:style-name` is `graphic` and `draw:text-style-name` is `paragraph`, with its DiaCraft
/// data when `tier2` (a copy has none), worked out once for each such pair.
fn shape_style(r: &mut Reader, cx: &mut Cx<'_>, graphic: Option<usize>, paragraph: Option<usize>, tier2: bool) -> Result<Rc<ShapeStyle>, OdgError> {
    if let Some(done) = cx.styles.get(&(graphic, paragraph, tier2)) {
        return Ok(Rc::clone(done));
    }
    let set = cx.set;
    let base = cx.sheets.base();
    let sheet_of = |at: Option<usize>| at.and_then(|i| set.get(i)).and_then(|s| s.sheet).unwrap_or(0);
    // The links: the first common style of the graphic chain, or the root.
    let link_g = graphic.and_then(|g| cx.res.link(r, set, g));
    let link_p = paragraph.and_then(|p| cx.res.link(r, set, p));
    let (mut line, mut fill) = (link_g, link_g);
    let mut text_link = link_p.or(link_g);
    // The three links DiaCraft wrote, when each names a common style.
    // The names were looked up once, when the style was read (`StyleSet::resolve`); none is hashed again for each pair.
    if let Some(linked) = graphic.and_then(|g| set.get(g)).filter(|s| tier2 && !s.common).and_then(|s| s.dia.linked) {
        match linked {
            Some([l, f, t]) => {
                (line, fill, text_link) = (Some(l), Some(f), Some(t));
            }
            None => {
                r.warn("diacraft_links_unknown", || "a shape's DiaCraft links name a style the drawing does not have; its parent is used".to_owned())
            }
        }
    }
    let links = Links { line: Some(sheet_of(line)), fill: Some(sheet_of(fill)), text: Some(sheet_of(text_link)) };
    let mut local = Out::default();
    // Line and fill cells: what the automatic styles set and the links do not give.
    if let Some(g) = graphic {
        let resolved = cx.res.resolved(r, set, g)?;
        let mut own_keys = HashSet::new();
        automatic_keys(set, &resolved, &mut own_keys);
        let own = |k: Kind, p: P| k == Kind::Graphic && !props::is_text_block(p) && own_keys.contains(&(k, p));
        let full_view = view_from(r, cx, &[Some(g)])?;
        let mut full = Out::default();
        props::line_fill(r, &full_view, &own, &base, &mut full);
        let line_view = view_from(r, cx, &[line])?;
        let mut against_line = Out::default();
        props::line_fill(r, &line_view, &own, &base, &mut against_line);
        let against_fill = if fill == line {
            None
        } else {
            let fill_view = view_from(r, cx, &[fill])?;
            let mut out = Out::default();
            props::line_fill(r, &fill_view, &own, &base, &mut out);
            Some(out)
        };
        let fill_against = against_fill.as_ref().unwrap_or(&against_line);
        let (fill_cells, line_cells): (Vec<_>, Vec<_>) =
            full.direct.into_iter().partition(|(name, _)| diacraft_doc::cell_info(None, name).class.family() == Some(diacraft_doc::Family::Fill));
        let mut kept = sheets::differing(r, Out { direct: line_cells, ..Out::default() }, &against_line);
        let fill_kept =
            sheets::differing(r, Out { direct: fill_cells, stops: full.stops, stops_through: full.stops_through, ..Out::default() }, fill_against);
        kept.direct.extend(fill_kept.direct);
        kept.stops = fill_kept.stops;
        local.direct.extend(kept.direct);
        local.stops = kept.stops;
        local.stops_through = fill_kept.stops_through;
        local.other_ends = full.other_ends;
    }
    // Text cells: what the text chain gives and the text link cannot reach.
    let text_view = view_from(r, cx, &[paragraph, graphic])?;
    let link_view = view_from(r, cx, &[text_link])?;
    let mut keys: Vec<(Kind, P)> = Vec::new();
    for (resolved, serves) in &text_view.layers {
        if *serves != Serves::All {
            continue;
        }
        for (key, _) in resolved.iter() {
            let text_side = key.0 != Kind::Graphic || props::is_text_block(key.1);
            if text_side && !keys.contains(key) {
                keys.push(*key);
            }
        }
    }
    let mut own_keys = HashSet::new();
    for (kind, p) in keys {
        let mine = text_view.hit(r, kind, p).map(|h| h.found.style);
        let theirs = link_view.hit(r, kind, p).map(|h| h.found.style);
        if mine.is_some() && mine != theirs {
            own_keys.insert((kind, p));
        }
    }
    let own = |k: Kind, p: P| own_keys.contains(&(k, p));
    let full_size = sheets::view_size(r, set, cx.res, &text_view, base.size);
    let link_size = sheets::view_size(r, set, cx.res, &link_view, base.size);
    let mut full_text = sheets::convert(r, &mut cx.texts, &text_view, &own, full_size, &base, true)?;
    full_text.tabs = text_view.tabs().cloned();
    let mut against = sheets::convert(r, &mut cx.texts, &link_view, &own, link_size, &base, true)?;
    against.tabs = link_view.tabs().cloned();
    // Tab stops equal to those the text link gives are not local, as any other cell.
    let text_out = sheets::differing(r, full_text, &against);
    local.direct.extend(text_out.direct);
    local.paragraph = text_out.paragraph;
    local.character = text_out.character;
    local.tabs = text_out.tabs;
    let text_size = full_size.unwrap_or(base.size);
    let patterns = (pattern_on(cx, links, &local, "FillPattern"), pattern_on(cx, links, &local, "LinePattern"));
    let made = Rc::new(ShapeStyle { links, local, text_layers: text_view.layers, text_size, patterns });
    r.take_items(made.local.cells())?;
    cx.styles.insert((graphic, paragraph, tier2), Rc::clone(&made));
    Ok(made)
}

/// What a frame's `draw:image` names, as found.
enum Picture {
    Shown(Arc<Media>),
    /// A flat drawing's `draw:image` with an `xlink:href` and no inline data: a file outside the document, which is never opened.
    Linked,
    Missing,
    OverBudget,
}

/// What a frame's `draw:image` shows: the picture's bytes from the package, or inline as
/// `office:binary-data`, as the shape's foreign data, `ForeignType` `Bitmap` for PNG, JPEG, GIF and BMP and `EnhMetaFile` for EMF, by the
/// bytes' own signature. A picture that cannot be found, or past the pictures' budget, leaves the shape with none (a placeholder draws).
///
/// **Cost.** A part is measured before anything is copied, and the model shares the package's own bytes: a part named by many frames is
/// one copy and is charged to the media budget once ([`Shared`]). Inline data is measured from its characters before it is decoded, and
/// decoded once, into the bytes the model keeps.
fn picture<'t>(r: &mut Reader, names: &mut Names<'t>, cx: &mut Cx<'_>, image: &'t El) -> Result<ForeignData, OdgError> {
    let mut foreign = picture_shown(r, names, cx, image)?;
    // The `draw:image` element's own attributes (its link type, its MIME type), as written and held on the picture; its `xlink:href` too when the picture is not held (a linked
    // picture's href is all there is of it), since a picture held is named by its part. Charged before they are made.
    let attrs = names.attrs(image);
    let no_picture = foreign.picture.is_none();
    let wanted = |a: &&super::names::Attr<'_>| no_picture || !(a.ns == Ns::Xlink && a.local == "href");
    if attrs.iter().any(|a| wanted(&a)) {
        r.take_items(super::cost::EXT_FOREIGN)?;
        let pairs: Vec<(&str, &str)> = attrs.iter().filter(wanted).map(|a| (a.name, a.value)).collect();
        // The list is shared by every picture that holds the same one (the link type and the MIME type of all of them), charged when it is first made.
        foreign.ext.make_mut().odf.raw = held::intern(r, cx.shared, &pairs)?;
    }
    Ok(foreign)
}

fn picture_shown<'t>(r: &mut Reader, names: &mut Names<'t>, cx: &mut Cx<'_>, image: &'t El) -> Result<ForeignData, OdgError> {
    let attrs = names.attrs(image);
    r.saw_attrs(attrs.len());
    let mut foreign = ForeignData::default();
    let data = image.elements().find(|c| names.is(c, Ns::Office, "binary-data"));
    // In a flat drawing inline data wins over an href beside it, and a picture with only an href is linked, outside the document: it is not read.
    let href = attrs.peek(Ns::Xlink, "href").filter(|_| cx.package.is_some() || data.is_none());
    let found = match (href, cx.package) {
        (Some(href), Some(package)) => match resolve_href(package, href) {
            Some(name) => part_picture(r, cx, name)?,
            None => Picture::Missing,
        },
        (Some(_), None) => Picture::Linked,
        (None, _) => match data {
            Some(data) => inline_picture(r, data)?,
            None => Picture::Missing,
        },
    };
    let media = match found {
        Picture::Shown(media) => media,
        Picture::Linked => {
            // One warning for the document, that counts them: it is given when the read is done.
            r.report.linked_pictures = r.report.linked_pictures.saturating_add(1);
            r.keep("picture_linked");
            return Ok(foreign);
        }
        Picture::Missing => {
            r.warn("picture_missing", || "a frame's picture is not in the package; a placeholder draws".to_owned());
            r.keep("picture_missing");
            return Ok(foreign);
        }
        Picture::OverBudget => {
            r.keep("picture_over_budget");
            return Ok(foreign);
        }
    };
    match signature(&media.bytes).1 {
        Some(t) => foreign.attrs.push("ForeignType", t),
        None => r.keep("picture_format"),
    }
    foreign.picture = Some(media);
    r.take_items(2)?;
    Ok(foreign)
}

/// The content type and `ForeignType` of a picture by its bytes' signature; `None` for a format this reader does not draw.
///
/// The table is the writer's too (`crate::picturetype`): a picture the writer writes under a type reads back as it.
fn signature(bytes: &[u8]) -> (&'static str, Option<&'static str>) {
    match crate::picturetype::picture_kind(bytes) {
        Some(kind) => (kind.media, Some(kind.foreign)),
        None => ("application/octet-stream", None),
    }
}

/// The picture of the part `name`: the one made for an earlier frame, or made now, measured against the media budget before anything is
/// copied and sharing the package's bytes.
fn part_picture(r: &mut Reader, cx: &mut Cx<'_>, name: String) -> Result<Picture, OdgError> {
    if let Some(made) = cx.shared.pictures.get(&name) {
        return Ok(made.clone().map_or(Picture::OverBudget, Picture::Shown));
    }
    let Some(package) = cx.package else { return Ok(Picture::Missing) };
    let Some(bytes) = package.part_shared(&name) else { return Ok(Picture::Missing) };
    let len = bytes.len() as u64;
    let made = if len > r.media_left {
        None
    } else {
        r.media_left -= len;
        r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(len);
        let by_signature = signature(&bytes).0;
        let manifest = package.manifest().entry(&name).and_then(|e| e.media_type.as_deref()).filter(|t| !t.is_empty());
        let content_type = manifest.unwrap_or(by_signature);
        Some(Arc::new(Media { part: Arc::from(name.as_str()), content_type: Arc::from(content_type), bytes }))
    };
    r.take_bytes(name.len())?;
    cx.shared.pictures.insert(name, made.clone());
    Ok(made.map_or(Picture::OverBudget, Picture::Shown))
}

/// An inline picture, `office:binary-data`: refused past the media budget by its characters' count before it is decoded (four characters
/// give three bytes, padding aside), then decoded once into the bytes the model keeps.
fn inline_picture(r: &mut Reader, data: &El) -> Result<Picture, OdgError> {
    let pieces: Vec<&str> =
        data.children.iter().filter_map(|n| if let Node::Text(t) | Node::CData(t) = n { Some(t.as_str()) } else { None }).collect();
    let characters: u64 = pieces.iter().map(|p| p.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').count() as u64).sum();
    if characters.saturating_mul(3) / 4 > r.media_left {
        return Ok(Picture::OverBudget);
    }
    let Some(len) = base64_into(&pieces, None) else { return Ok(Picture::Missing) };
    let mut bytes: Arc<[u8]> = std::iter::repeat_n(0u8, len).collect();
    if let Some(out) = Arc::get_mut(&mut bytes) {
        base64_into(&pieces, Some(out));
    }
    r.media_left = r.media_left.saturating_sub(len as u64);
    r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(len as u64);
    let content_type = signature(&bytes).0;
    Ok(Picture::Shown(Arc::new(Media { part: Arc::from(crate::picturetype::INLINE_PART), content_type: Arc::from(content_type), bytes })))
}

/// The bytes of inline picture data (`office:binary-data`) that a fill image's definition holds: refused, by the characters' count and before it is decoded, when it does
/// not fit the media budget; `None` for text that is not base64. Charged to the media budget.
pub(super) fn inline_bytes(r: &mut Reader, pieces: &[&str]) -> Option<Arc<[u8]>> {
    let characters: u64 = pieces.iter().map(|p| p.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').count() as u64).sum();
    if characters.saturating_mul(3) / 4 > r.media_left {
        return None;
    }
    let len = base64_into(pieces, None)?;
    let mut bytes: Arc<[u8]> = std::iter::repeat_n(0u8, len).collect();
    if let Some(out) = Arc::get_mut(&mut bytes) {
        base64_into(pieces, Some(out));
    }
    r.media_left = r.media_left.saturating_sub(len as u64);
    r.report.work.media_bytes = r.report.work.media_bytes.saturating_add(len as u64);
    Some(bytes)
}

/// An `xlink:href` as an entry of the package: percent-decoded with a leading `./` stripped, matched exactly; else as
/// written. `None` when neither names an entry.
pub(super) fn resolve_href(package: &Package, href: &str) -> Option<String> {
    let decoded = percent_decode(href);
    let stripped = decoded.strip_prefix("./").unwrap_or(&decoded).to_owned();
    if package.part(&stripped).is_some() {
        return Some(stripped);
    }
    let raw = href.strip_prefix("./").unwrap_or(href);
    package.part(raw).is_some().then(|| raw.to_owned())
}

/// Base64 (RFC 4648, the standard alphabet), white space ignored, over the text `pieces` in turn: the decoded length, and the bytes written
/// into `out` when it is given (of that length). `None` for text that is not base64.
fn base64_into(pieces: &[&str], mut out: Option<&mut [u8]>) -> Option<usize> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut padding = false;
    let mut len = 0usize;
    for c in pieces.iter().flat_map(|p| p.bytes()) {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            padding = true;
            continue;
        }
        if padding {
            return None;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if let Some(slot) = out.as_deref_mut().and_then(|o| o.get_mut(len)) {
                *slot = ((acc >> bits) & 0xFF) as u8;
            }
            len += 1;
        }
    }
    Some(len)
}

/// [`base64_into`] of one text, into a new vector.
fn base64(text: &str) -> Option<Vec<u8>> {
    let len = base64_into(&[text], None)?;
    let mut out = vec![0u8; len];
    base64_into(&[text], Some(&mut out));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A custom shape has the outline of its geometry element, a path that of its `svg:d`, a polygon and a polyline that of their points; the other kinds have none (their
    /// outline is their frame, an input).
    #[test]
    fn the_kinds_that_have_an_outline_are_told_from_those_that_have_none() {
        use crate::ext::fingerprint::OutlineKind;
        assert_eq!(outline_kind(Kindof::Custom), Some(OutlineKind::Custom));
        assert_eq!(outline_kind(Kindof::Path), Some(OutlineKind::Path));
        assert_eq!(outline_kind(Kindof::Polyline), Some(OutlineKind::Points));
        assert_eq!(outline_kind(Kindof::Polygon), Some(OutlineKind::Points));
        for none in [
            Kindof::Rect,
            Kindof::Ellipse,
            Kindof::Circle,
            Kindof::Line,
            Kindof::RegularPolygon,
            Kindof::Frame,
            Kindof::Connector,
            Kindof::Group,
            Kindof::Object("measure"),
            Kindof::Kept("x"),
        ] {
            assert_eq!(outline_kind(none), None);
        }
    }

    /// A flat drawing of one page holding `shapes`.
    fn flat_drawing(shapes: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.graphics"><office:body><office:drawing><draw:page draw:name="p">{shapes}</draw:page></office:drawing></office:body></office:document>"#
        )
        .into_bytes()
    }

    /// The row that says a text holds the frame of an object is charged as a section, a row and a cell are: the items a read takes for a text frame that holds
    /// the frame of an object are those of the same text frame that holds the frame of a picture, and one section, one row and one cell more.
    #[test]
    fn the_marker_of_a_text_that_holds_an_object_is_charged_a_section_a_row_and_a_cell() {
        let text_frame = |nested: &str| {
            flat_drawing(&format!(
                r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="6cm" svg:height="4cm"><draw:text-box><text:p>hello<draw:frame svg:width="2cm" svg:height="1cm">{nested}</draw:frame></text:p></draw:text-box></draw:frame>"#
            ))
        };
        let items = |bytes: Vec<u8>| crate::read(&bytes, &crate::Limits::default()).expect("reads").1.work.items;
        let (with_object, with_picture) =
            (items(text_frame(r#"<draw:object xlink:href="./Object 1"/>"#)), items(text_frame(r#"<draw:image xlink:href="p.png"/>"#)));
        assert_eq!(
            with_object - with_picture,
            (super::super::cost::SECTION + super::super::cost::ROW + super::super::ITEMS_PER_CELL) as u64,
            "{with_object} against {with_picture}"
        );
    }

    #[test]
    fn base64_reads_the_standard_alphabet() {
        assert_eq!(base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64("aGVs\nbG8h"), Some(b"hello!".to_vec()));
        assert_eq!(base64("+/8="), Some(vec![0xFB, 0xFF]), "the alphabet's last two");
        assert_eq!(base64("a*b"), None);
        assert_eq!(base64("aGVsbG8=x"), None);
    }

    /// The reader's judgement of a picture is the table the writer types a new picture by (`crate::picturetype`): each of the five kinds, and a format that is none.
    #[test]
    fn the_signature_of_a_picture_is_the_shared_tables_and_a_format_that_is_none_is_a_placeholder() {
        let mut emf = vec![0u8; 40];
        emf.extend_from_slice(b" EMF");
        let heads: [&[u8]; 5] = [&[0x89, b'P', b'N', b'G', 1], &[0xFF, 0xD8, 0], b"GIF89a", b"BM\0\0", &emf];
        for head in heads {
            let kind = crate::picturetype::picture_kind(head).expect("a kind");
            assert_eq!(signature(head), (kind.media, Some(kind.foreign)));
        }
        assert_eq!(signature(b"<svg xmlns="), ("application/octet-stream", None));
        assert_eq!(signature(b""), ("application/octet-stream", None));
    }

    #[test]
    fn percent_decoding_keeps_what_is_not_an_escape() {
        assert_eq!(percent_decode("./Object%201"), "./Object 1");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%e2%9c%93"), "\u{2713}");
    }

    #[test]
    fn transforms_compose_in_the_order_written() {
        let limits = crate::limits::Limits::default();
        let mut r = Reader::new(&limits);
        let t = parse_transform(&mut r, "rotate (0.5235987755982988) translate (5cm 10cm)").unwrap();
        let (frame, sheared) = frame_of(0.0, 0.0, 4.0 / 2.54, 2.0 / 2.54, Some(&t));
        assert!(!sheared);
        assert!((frame.angle.to_degrees() - 30.0).abs() < 1e-9);
        assert!((frame.pin.0 * 2.54 - 7.232050807568877).abs() < 1e-9);
        assert!((frame.pin.1 * 2.54 - 9.866025403784438).abs() < 1e-9);
        assert!(parse_transform(&mut r, "rotate(1").is_none());
        // A translation with no unit is hundredths of a millimetre, as LibreOffice 26.8.2.2 reads it; a list it cannot read is still none.
        let plain = parse_transform(&mut r, "translate(1 2)").map(|t| (t.map.e, t.map.f, t.translate_unit));
        assert_eq!(plain, Some((1.0 / 2540.0, 2.0 / 2540.0, None)));
        assert!(parse_transform(&mut r, "translate(1e3 2)").is_none(), "a number as a length writes it: no exponent");
        assert!(parse_transform(&mut r, "skewX (0.2) translate (1cm 1cm)").is_some_and(|t| t.sheared));
    }
}
