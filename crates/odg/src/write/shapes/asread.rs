//! A shape of the model path written back **as it was read** (the native-format design's §3.3, §3.4 and §4.4).
//!
//! A shape the `.odg` reader made holds, in its `odf` slot, the element it was read from (`draw:rect`, `draw:polygon`, `draw:line`, ...), the automatic style it named, the unit
//! each length was written in where no cell spells it, the values of its element that cells only approximate (the frame, the layer, a circle's centre form, a rectangle's corners, a
//! polygon's view box, a path's data, a connector's route and type), and whether its element held a bare `text:p`. **While the values' stamps hold, the shape is written as it was
//! read**: its own element, on the style name the file gave it, with its lengths in the units the file wrote them in, and a layer and a text style name only where the file had them.
//! **When a stamp no longer holds** (the shape was edited) the shape is written from its cells as a new shape is, but for the style: a shape keeps its style name while the style's own
//! cells are as they were, and takes a style of its own, under a name no shape holds, when they are not.
//!
//! * **Which element is written.** A plain shape (`draw:rect`, `draw:ellipse`, `draw:circle`, `draw:regular-polygon`, `draw:polygon`, `draw:polyline`, `draw:path`, and a text
//!   frame) is written as its element while the stamp of its outline (the `Geometry` sections) holds, and the frame is one the element can say (a flip of a shape that was not flipped
//!   is not). A circle or an ellipse given by its centre is written in that form while it is not turned. Any other shape is written as the element the writer writes for its kind
//!   (a line, a connector, a custom shape, a picture frame), with the layer, the style name and the units of the file.
//! * **The frame.** The frame the shape was read with is held as written when the element had a `draw:transform`; it is written as held while the size, the turn and the place of the
//!   frame are as they were, with its last translation moved by what the frame moved otherwise (the rest of the transform as written), and from the cells when its size or turn changed.
//!   A frame with no transform is written from the cells, each length in the unit it was read in: the unit the cell shows, else the unit the shape's `units` hold, else the page's.
//! * **The style.** A held name is written once: a second shape that names it and has the same style writes no second one. A name that two different styles would take is not given
//!   twice: the second gets a fresh name, and the fresh names skip every name a shape holds ([`held_graphic_names`]). A shape that named no style and has nothing for one gets none.

use std::collections::HashSet;

use diacraft_doc::{Document, Held, HeldGroup, RawAttrs, RawElement, RawNode, Shape, Sheet};

use super::{Editor, Element, Kind, NewShape, NewWrote, TierCtx, WriteError, native, rebuilt_connection, refuse, static_name, turn_and_flips};
use crate::read::names::Ns;
use crate::read::units::{LenUnit, length};
use crate::read::{HeldValue as Value, frame_read, transform_map};
use crate::write::compare::PageDiff;
use crate::write::emit::Pfx;
use crate::write::lengths::{length_text, same, unit_of_suffix, unit_of_word};
use crate::write::own::{self, Pose};
use crate::write::scan::Fam;
use crate::write::splice::has_bad_char;
use crate::write::stylemap::Op;
use crate::write::styles::{KeptAttr, KeptAttrs, KeptRest, Written};
use crate::write::transform::{translated, turned};

/// The words of the refusal of an automatic style that keeps more than a shape's kept markup may be.
const WORDS_KEPT_STYLE_BIG: &str = "an automatic style that keeps more than 1 MiB of the markup of the file it was read from beside what its cells say (its attributes and the elements kept whole)";

/// The elements of ODF's drawing namespace a shape is written back as when its outline is its element's own.
const PLAIN: [&str; 8] = ["rect", "ellipse", "circle", "regular-polygon", "polygon", "polyline", "path", "frame"];

/// The local name of a name as written.
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

fn pairs(attrs: &RawAttrs) -> Vec<(String, String)> {
    attrs.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// The namespace of an attribute name the writer writes under its own prefix, and its local name: only the prefixes of ODF's own namespaces, and only a qualified name of XML Namespaces
/// (a name with a quote, a space or a `>` in it is none: it would write markup of its own). A name with any other prefix is none.
fn attr_ns(name: &str) -> Option<(Ns, &str)> {
    if !crate::write::kept::is_qname(name) {
        return None;
    }
    let (prefix, local) = name.split_once(':')?;
    let ns = match prefix {
        "draw" => Ns::Draw,
        "svg" => Ns::Svg,
        "fo" => Ns::Fo,
        "style" => Ns::Style,
        "text" => Ns::Text,
        _ => return None,
    };
    Some((ns, local))
}

/// What a kept style's remainder is made of while it is made ([`Editor::remainder_of`]).
#[derive(Default)]
struct StyleKeep {
    attrs: Vec<(Ns, String, String)>,
    complete: bool,
    rest: KeptRest,
}

/// The element of a kept style an attribute is written on.
#[derive(Clone, Copy)]
enum KeepAt {
    /// The `style:style` element.
    Own,
    /// Its graphic properties.
    Graphic,
}

/// Why a shape is not written as it was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotRead {
    /// It holds no element: no `.odg` reader made it.
    NoElement,
    /// It holds this many names or values the writer does not write.
    Refused(usize),
}

/// What a shape the `.odg` reader made held, as the model writer reads it (see the module documentation).
#[derive(Clone, Debug, Default)]
pub(crate) struct AsRead {
    /// The local name of the element the shape was read from.
    pub element: String,
    /// The attributes beside its frame that give a plain shape's outline (`draw:kind`, the angles), as written.
    pub outline: Vec<(String, String)>,
    /// The element is a plain shape's own, whose outline the element gives; false for an element the model has no outline stamp for.
    pub plain: bool,
    /// The stamp of the `Geometry` sections is the one the reader set (a plain shape's outline is as it was read; always true for the rest).
    pub outline_holds: bool,
    /// The element held a bare `text:p`.
    pub bare_text: bool,
    /// The automatic style the shape named.
    pub style: Option<String>,
    /// The `draw:layer` the file wrote, when it wrote one.
    pub layer: Option<String>,
    /// The unit each length attribute was written in where no cell spells it.
    pub units: Vec<(String, String)>,
    /// The frame of a 2-D shape as written, when its element had a transform.
    pub frame2d: Option<Vec<(String, String)>>,
    /// The ends of a segment as written (and its transform).
    pub frame1d: Option<Vec<(String, String)>>,
    /// A circle's or an ellipse's centre form.
    pub centre: Option<Vec<(String, String)>>,
    /// A rectangle's corner radii.
    pub corner: Vec<(String, String)>,
    /// A polygon's or polyline's `svg:viewBox`.
    pub view_box: Option<String>,
    /// **The outline attributes of a polygon or polyline the reader could not read** (a view box with no width or height, or none, or one that is no four numbers; points that
    /// are no points): it left them in the shape's kept attributes and gave the shape the rectangle of its frame for an outline. Those that the kept attributes hold, among `svg:viewBox` and
    /// `draw:points`, are the file's and are written back as they were (`kept_tag`), while the `Geometry` sections are the stand-in the reader made. Empty for an outline the reader read.
    pub unread_outline: Vec<&'static str>,
    /// A path's `svg:d` and `svg:viewBox`, while the rows are as they were read.
    pub path: Option<Vec<(String, String)>>,
    /// A connector's route (`svg:d`, `svg:viewBox`, `draw:line-skew`), while the rows are as they were read; and whether the file held one at all.
    pub route: Option<Vec<(String, String)>>,
    pub route_held: bool,
    /// A connector's `draw:type` as written (`Some(None)`: the file wrote none), when the shape is a connector.
    pub conn_type: Option<Option<String>>,
}

impl AsRead {
    /// What `shape` held when it was read, or why it is not written as it was: it is no shape an `.odg` reader made ([`NotRead::NoElement`]), or **it holds a name or a value the writer
    /// does not write** ([`NotRead::Refused`], with how many): an attribute name that is no qualified name of ODF's namespaces under the prefixes the writer has, or a value with a
    /// character XML 1.0 does not allow. Such a shape is written as a new shape, and the save says how many attributes it held that were not written.
    pub(crate) fn of(shape: &Shape) -> Result<AsRead, NotRead> {
        let odf = &shape.ext.get().ok_or(NotRead::NoElement)?.odf;
        let mut out = AsRead { style: odf.style_name.as_ref().map(|s| s.to_string()), units: pairs(&odf.units), ..AsRead::default() };
        let mut found = false;
        for h in odf.held.iter().filter(|h| h.group == HeldGroup::Geometry && h.attrs.is_empty()) {
            let Some(el) = h.kept.iter().find_map(|p| if let RawNode::Element(e) = &p.node { Some(e) } else { None }) else { continue };
            out.element = local(&el.name).to_owned();
            out.outline = pairs(&el.attrs);
            out.bare_text = el.children.iter().any(|c| matches!(c, RawNode::Element(e) if local(&e.name) == "p"));
            out.plain = h.stamp.is_some();
            out.outline_holds = !out.plain || h.stamp == Value::PlainOutline.stamp(&shape.sheet);
            found = true;
            break;
        }
        if !found || !crate::write::kept::is_ncname(&out.element) {
            return Err(NotRead::NoElement);
        }
        if matches!(out.element.as_str(), "polygon" | "polyline") {
            out.unread_outline = ["svg:viewBox", "draw:points"].into_iter().filter(|name| odf.kept.raw.get(name).is_some()).collect();
        }
        for h in &odf.held {
            out.take(h, shape);
        }
        out.validated()
    }

    /// What a **group** of the model held when it was read, as far as the model writer writes it back: the automatic graphic style it named, and nothing else (a `draw:g` has
    /// no frame, outline or layer of its own). `Ok(None)` for a group that names no style, or names the empty one, which is a new group with none. **The record is checked as a shape's is**
    /// ([`AsRead::validated`]): a style name with a character XML 1.0 does not allow is `Err`, so that the group's own style, which the markup writes, is refused where the shape's is, and the
    /// document file and the markup tell a styled group by this one answer ([`super::super::groups::names_a_style`]).
    pub(crate) fn of_group(shape: &Shape) -> Result<Option<AsRead>, NotRead> {
        let name = shape.ext.get().and_then(|data| data.odf.style_name.as_deref()).filter(|name| !name.is_empty());
        let Some(name) = name else { return Ok(None) };
        AsRead { style: Some(name.to_owned()), ..AsRead::default() }.validated().map(Some)
    }

    /// The record itself, when every name and value that is written from it is one the writer writes, else `Err` with how many are not: **the one check of what a shape's or a group's record
    /// holds**, so that the two ways of making a record cannot differ in it. A name is a qualified name under a prefix the writer has, a value holds no character XML 1.0 does not allow, and a unit
    /// is one of the six.
    fn validated(self) -> Result<AsRead, NotRead> {
        let out = self;
        let bad_pair = |(k, v): &(String, String)| attr_ns(k).is_none() || has_bad_char(v);
        let lists =
            [&out.outline, &out.corner].into_iter().chain([&out.frame2d, &out.frame1d, &out.centre, &out.path, &out.route].into_iter().flatten());
        let mut bad: usize = lists.map(|l| l.iter().filter(|p| bad_pair(p)).count()).sum();
        bad += out.units.iter().filter(|(k, v)| attr_ns(k).is_none() || unit_of_suffix(v).is_none()).count();
        let texts = [out.style.as_deref(), out.layer.as_deref(), out.view_box.as_deref(), out.conn_type.as_ref().and_then(|t| t.as_deref())];
        bad += texts.into_iter().flatten().filter(|t| has_bad_char(t)).count();
        if bad > 0 {
            return Err(NotRead::Refused(bad));
        }
        Ok(out)
    }

    /// Sorts one held value of the shape into its field.
    fn take(&mut self, h: &Held, shape: &Shape) {
        let get = |name: &str| h.attrs.get(name).map(str::to_owned);
        match h.group {
            HeldGroup::Frame => {
                if let Some(layer) = get("draw:layer") {
                    self.layer = Some(layer);
                } else if h.attrs.get("svg:x1").is_some() {
                    self.frame1d = Some(pairs(&h.attrs));
                } else if h.attrs.get("svg:cx").is_some() {
                    self.centre = Some(pairs(&h.attrs));
                } else if !h.attrs.is_empty() {
                    self.frame2d = Some(pairs(&h.attrs));
                }
            }
            HeldGroup::Geometry => {
                if h.attrs.is_empty() {
                    // The element, the enhanced geometry (a stamp alone, written by the custom shape's own code) and a connector's type that the file did not write.
                    if self.element == "connector" && h.stamp.is_none() && h.kept.is_empty() {
                        self.conn_type = Some(None);
                    }
                } else if h.attrs.get("svg:d").is_some() {
                    if self.element == "connector" {
                        self.route_held = true;
                        if h.stamp == Value::Route.stamp(&shape.sheet) {
                            self.route = Some(pairs(&h.attrs));
                        }
                    } else if h.stamp == Value::PathData.stamp(&shape.sheet) {
                        self.path = Some(pairs(&h.attrs));
                    }
                } else if h.attrs.get("draw:line-skew").is_some() && self.element == "connector" {
                    self.route_held = true;
                    if h.stamp == Value::Route.stamp(&shape.sheet) {
                        self.route = Some(pairs(&h.attrs));
                    }
                } else if h.attrs.get("svg:viewBox").is_some() && h.attrs.iter().count() == 1 {
                    self.view_box = get("svg:viewBox");
                } else if self.element == "connector" && h.attrs.get("draw:type").is_some() && h.attrs.iter().count() == 1 {
                    self.conn_type = Some(get("draw:type"));
                } else if ["draw:corner-radius", "svg:rx", "svg:ry"].iter().any(|k| h.attrs.get(k).is_some()) {
                    self.corner = pairs(&h.attrs);
                }
            }
            _ => {}
        }
    }

    /// The unit the held `units` give the attribute `attr`.
    fn held_unit(&self, attr: &str) -> Option<LenUnit> {
        self.units.iter().find(|(k, _)| k == attr).and_then(|(_, v)| unit_of_suffix(v))
    }
}

/// The names of the automatic graphic styles the shapes of the model hold, for the names a new style takes to avoid: a shape that must have a style of its own while another shape holds
/// the name it would be given is given another. Walks the pages' shapes and the members of groups with a stack of its own, a step for each shape.
pub(crate) fn held_graphic_names(doc: &Document, work: &mut crate::write::WriteWork) -> Result<Vec<(Fam, String)>, WriteError> {
    let mut names: HashSet<String> = HashSet::new();
    let mut out: Vec<(Fam, String)> = Vec::new();
    for page in doc.pages.iter() {
        let mut stack: Vec<&diacraft_doc::Shape> = page.shapes.iter().map(|s| &**s).collect();
        while let Some(shape) = stack.pop() {
            work.charge_steps(1)?;
            if let Some(odf) = shape.ext.get().map(|d| &d.odf) {
                if let Some(name) = odf.style_name.as_deref()
                    && names.insert(name.to_owned())
                {
                    out.push((Fam::Graphic, name.to_owned()));
                }
                if let Some(name) = odf.kept.raw.get("draw:text-style-name")
                    && names.insert(name.to_owned())
                {
                    out.push((Fam::Paragraph, name.to_owned()));
                }
            }
            stack.extend(shape.children.iter().map(|c| &**c));
        }
    }
    Ok(out)
}

/// The frame of a 2-D shape now, in the terms the held frame is judged by: the image of its centre on the page (inches, y up), its size, the turn of the frame as the file says it and
/// whether the map flips it.
struct Now {
    centre: (f64, f64),
    size: (f64, f64),
    angle: f64,
    flipped: bool,
}

/// How a frame the shape was read with compares with the one it has.
enum Was {
    Same,
    /// The size and the turn are as they were: the frame moved by this much on the page (inches, y down).
    Moved(f64, f64),
    Changed,
}

/// The inches of the attribute `name` among `held`, when it is there and reads as a length.
fn inches(held: &[(String, String)], name: &str) -> Option<f64> {
    held.iter().find(|(k, _)| k == name).and_then(|(_, v)| length(v)).map(|l| l.inches)
}

fn text_of<'a>(held: &'a [(String, String)], name: &str) -> Option<&'a str> {
    held.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

impl Now {
    /// Whether the held frame of a 2-D shape is the one the shape has: `Same`, a translation of it, or neither.
    fn against(&self, held: &[(String, String)], height: f64) -> Was {
        let Some(read) = held_frame(held) else { return Was::Changed };
        let size = same(read.size.0, self.size.0) && same(read.size.1, self.size.1);
        let turn = (read.angle - self.angle).abs() <= 1e-9 && read.flipped == self.flipped && !read.sheared;
        if !(size && turn) {
            return Was::Changed;
        }
        let (rx, ry) = (read.centre.0, height - read.centre.1);
        if same(rx, self.centre.0) && same(ry, self.centre.1) { Was::Same } else { Was::Moved(self.centre.0 - rx, -(self.centre.1 - ry)) }
    }
}

/// The frame a held `Frame2d` value gives, as the reader reads it.
fn held_frame(held: &[(String, String)]) -> Option<crate::read::FrameRead> {
    let get = |name: &str| inches(held, name).unwrap_or(0.0);
    frame_read(get("svg:x"), get("svg:y"), get("svg:width"), get("svg:height"), text_of(held, "draw:transform"))
}

impl Editor<'_, '_> {
    /// `inches` as a length in `unit`.
    fn length_in(&self, inches: f64, unit: LenUnit) -> Result<String, WriteError> {
        length_text(inches, unit).ok_or(WriteError::Unsupported("a length too large to write"))
    }

    /// Whether the ends of a segment are where the file had them: the ends it was read with (and its transform) put on the page are the ends it has.
    pub(super) fn ends_as_read(&self, read: &AsRead, ends: [(f64, f64); 2]) -> bool {
        let Some(held) = read.frame1d.as_deref() else { return false };
        let Some(map) = transform_map(text_of(held, "draw:transform")) else { return false };
        let at = |x: &str, y: &str| map.apply(inches(held, x).unwrap_or(0.0), inches(held, y).unwrap_or(0.0));
        let (b, e) = (at("svg:x1", "svg:y1"), at("svg:x2", "svg:y2"));
        let page = |p: (f64, f64)| (p.0, self.height - p.1);
        let same_point = |a: (f64, f64), b: (f64, f64)| same(a.0, b.0) && same(a.1, b.1);
        same_point(b, page(ends[0])) && same_point(e, page(ends[1]))
    }

    /// The route attributes the file held (`svg:d`, `svg:viewBox`, `draw:line-skew`), as written.
    pub(super) fn held_route(&self, pfx: &Pfx, held: &[(String, String)]) -> Result<String, WriteError> {
        self.held_text(pfx, held, &mut Vec::new())
    }

    /// The lengths of a style written in the unit the cell they come from shows (`svg:stroke-width` in the unit of `LineWeight`, the paddings in those of the margins): a
    /// style length is in inches in the ops and goes by the page's unit otherwise, which would respell `1pt` as `0.035cm`. A cell with no unit a length can spell keeps the page's.
    pub(super) fn in_cell_units(&self, ops: Vec<Op>, sheet: &Sheet) -> Vec<Op> {
        ops.into_iter()
            .map(|op| {
                let cell = match (op.ns, op.local) {
                    (Ns::Svg, "stroke-width") => "LineWeight",
                    (Ns::Fo, "padding-top") => "TopMargin",
                    (Ns::Fo, "padding-bottom") => "BottomMargin",
                    (Ns::Fo, "padding-left") => "LeftMargin",
                    (Ns::Fo, "padding-right") => "RightMargin",
                    _ => return op,
                };
                match (&op.val, sheet.cell(cell).and_then(|c| c.unit.as_deref()).and_then(unit_of_word)) {
                    (crate::write::stylemap::Val::Length(inches), Some(unit)) => match length_text(*inches, unit) {
                        Some(text) => Op { val: crate::write::stylemap::Val::Text(text), ..op },
                        None => op,
                    },
                    _ => op,
                }
            })
            .collect()
    }

    /// What the file's automatic graphic style `name` kept that no cell reads and that is written with the style: the attributes of its graphic
    /// properties in ODF's namespaces ([`KeptAttrs::attrs`]) and **the rest of what it kept, each where it stood** ([`KeptAttrs::rest`]): the style's own attributes beyond its identity, the
    /// attributes of its graphic properties in other namespaces (with the declaration of their prefix), the children of its graphic properties, and its other children (the paragraph and text
    /// properties it held, a `style:map`). `complete` says that is all of what the model's kept style of the name holds; what cannot be written (a name that is no qualified name, a value XML
    /// cannot hold, a prefix whose namespace nothing says, a text or a comment among its children) is left out and the note counts the style. Counted before anything is made; **made once for
    /// a name**, and shared by every shape that names it.
    fn remainder_of(&mut self, name: &str) -> Result<Option<std::sync::Arc<KeptAttrs>>, WriteError> {
        if let Some(made) = self.book.as_ref().and_then(|b| b.remainder_cache.get(name)) {
            return Ok(Some(std::sync::Arc::clone(made)));
        }
        let doc = self.doc;
        let Some(odf) = doc.ext.get().map(|d| &d.odf) else { return Ok(None) };
        if self.book.as_ref().is_some_and(|b| b.remainders.is_none()) {
            self.work.charge_steps(odf.styles.len().saturating_add(1))?;
            let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for (at, kept) in odf.styles.iter().enumerate() {
                if kept.family.as_ref() == "auto:graphic" {
                    index.entry(kept.name.to_string()).or_insert(at);
                }
            }
            // Where a prefix a style kept is bound when the style does not declare it: the root of `content.xml` and the container of its automatic styles, as the file had them.
            let mut bound = crate::write::kept::Bindings::new();
            for key in ["content.xml", "content.xml/office:automatic-styles"] {
                if let Some((_, attrs)) = odf.roots.iter().find(|(n, _)| &**n == key) {
                    self.work.charge_steps(attrs.len())?;
                    crate::write::kept::declare_into(&mut bound, attrs.iter());
                }
            }
            if let Some(book) = self.book.as_mut() {
                book.remainders = Some(index);
                book.remainder_bound = Some(bound);
            }
        }
        let at = self.book.as_ref().and_then(|b| b.remainders.as_ref()).and_then(|m| m.get(name)).copied();
        let Some(kept) = at.and_then(|at| odf.styles.get(at)) else { return Ok(None) };
        let el = &kept.element;
        self.work.charge_steps(el.attrs.len().saturating_add(el.children.len()).saturating_add(1))?;
        let scope =
            self.book.as_ref().and_then(|b| b.auto.as_ref()).map(|a| a.scope).ok_or(WriteError::Unsupported("a part with no automatic styles"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let bound = self.book.as_ref().and_then(|b| b.remainder_bound.clone()).unwrap_or_default();
        let mut made = StyleKeep { complete: true, ..StyleKeep::default() };
        // The style's own attributes: its identity is written by the style, and a declaration is no attribute (it binds what the others use).
        let mut own_bound = crate::write::kept::Bindings::new();
        crate::write::kept::declare_into(&mut own_bound, el.attrs.iter());
        for (k, v) in el.attrs.iter() {
            if crate::write::kept::declared_by(k).is_some() || matches!(local(k), "name" | "family" | "parent-style-name") {
                continue;
            }
            self.kept_attribute(&mut made, KeepAt::Own, (k, v), [&own_bound, &bound], &pfx)?;
            Self::bound_rest(&made)?;
        }
        let mut seen_graphic = false;
        for child in &el.children {
            let RawNode::Element(props) = child else {
                // A text, a comment or an instruction among the children has no place in the style written, unless it is blank.
                if !matches!(child, RawNode::Text(t) if t.trim().is_empty()) {
                    made.complete = false;
                }
                continue;
            };
            if local(&props.name) != "graphic-properties" || seen_graphic {
                // Another properties element, or any other child: written whole after the graphic properties.
                match self.style_element(props, [&own_bound, &bound], &pfx)? {
                    Some(text) => made.rest.after.push_str(&text),
                    None => made.complete = false,
                }
                Self::bound_rest(&made)?;
                continue;
            }
            seen_graphic = true;
            self.work.charge_steps(props.attrs.len().saturating_add(props.children.len()))?;
            let mut holder_bound = crate::write::kept::Bindings::new();
            crate::write::kept::declare_into(&mut holder_bound, props.attrs.iter());
            for (k, v) in props.attrs.iter() {
                if crate::write::kept::declared_by(k).is_some() {
                    continue;
                }
                match attr_ns(k) {
                    Some((ns, l)) if !has_bad_char(v) => {
                        // The bytes of the value are charged before they are copied, once for the style (the copy is shared by every shape that names it).
                        self.work.charge_copy(v.len().saturating_add(l.len()), self.limits)?;
                        made.attrs.push((ns, l.to_owned(), v.to_owned()));
                    }
                    Some(_) => made.complete = false,
                    None => self.kept_attribute(&mut made, KeepAt::Graphic, (k, v), [&holder_bound, &own_bound, &bound], &pfx)?,
                }
                Self::bound_rest(&made)?;
            }
            for inner in &props.children {
                match inner {
                    RawNode::Element(e) => match self.style_element(e, [&holder_bound, &own_bound, &bound], &pfx)? {
                        Some(text) => made.rest.graphic_children.push_str(&text),
                        None => made.complete = false,
                    },
                    RawNode::Text(t) if t.trim().is_empty() => {}
                    _ => made.complete = false,
                }
                Self::bound_rest(&made)?;
            }
        }
        let made = std::sync::Arc::new(KeptAttrs { attrs: made.attrs, complete: made.complete, rest: made.rest });
        if let Some(book) = self.book.as_mut() {
            book.remainder_cache.insert(name.to_owned(), std::sync::Arc::clone(&made));
        }
        Ok(Some(made))
    }

    /// A style keeps no more beyond its graphic properties' attributes of ODF's namespaces than a shape's kept markup may be: past it the save is refused in words (checked as each piece is made, so
    /// that one that is past it is never walked to its end).
    fn bound_rest(made: &StyleKeep) -> Result<(), WriteError> {
        if made.rest.bytes() > super::MOST_KEPT_PER_SHAPE {
            return Err(WriteError::Unsupported(WORDS_KEPT_STYLE_BIG));
        }
        Ok(())
    }

    /// Takes one attribute `(name, value)` of a kept style into `made`, at `at`: one of ODF's own namespaces by the prefix the file spelled it with, any other by the namespace its prefix
    /// stands for in `bound` (the declarations where it stood, the nearest first). **An attribute that cannot be written is left out and the style is not complete**: a name that is no
    /// qualified name, a value XML cannot hold, a prefix nothing binds, or one the writer has for another namespace on the element it goes on.
    fn kept_attribute<'a>(
        &mut self,
        made: &mut StyleKeep,
        at: KeepAt,
        (name, value): (&str, &str),
        bound: impl IntoIterator<Item = &'a crate::write::kept::Bindings>,
        pfx: &Pfx,
    ) -> Result<(), WriteError> {
        use crate::write::kept::is_qname;
        let into = match at {
            KeepAt::Own => &mut made.rest.own,
            KeepAt::Graphic => &mut made.rest.graphic_foreign,
        };
        if !is_qname(name) || has_bad_char(value) {
            made.complete = false;
            return Ok(());
        }
        // The bytes of the value are charged before they are copied, once for the style.
        self.work.charge_copy(name.len().saturating_add(value.len()), self.limits)?;
        if let Some((ns, l)) = attr_ns(name) {
            into.push(KeptAttr::Odf(ns, l.to_owned(), value.to_owned()));
            return Ok(());
        }
        let (prefix, local) = name.split_once(':').unwrap_or(("", name));
        // An attribute with no prefix is in no namespace, and needs no declaration.
        if prefix.is_empty() {
            into.push(KeptAttr::Foreign { prefix: String::new(), uri: String::new(), local: local.to_owned(), value: value.to_owned() });
            return Ok(());
        }
        let uri = bound.into_iter().find_map(|b| b.get(prefix)).map(String::as_str).filter(|u| !u.is_empty());
        let writer = [
            (pfx.draw.as_str(), crate::write::scan::DRAW_URI),
            (pfx.svg.as_str(), crate::write::scan::SVG_URI),
            (pfx.fo.as_str(), crate::write::scan::FO_URI),
            (pfx.style.as_str(), crate::write::scan::STYLE_URI),
            (pfx.text.as_str(), crate::write::scan::TEXT_URI),
            (crate::ext::names::PREFIX, crate::write::scan::DIACRAFT_URI),
        ];
        match uri {
            // A prefix the writer has for another namespace on this element cannot be declared again.
            Some(uri) if writer.iter().any(|(p, u)| *p == prefix && *u != uri) => made.complete = false,
            Some(uri) => {
                into.push(KeptAttr::Foreign { prefix: prefix.to_owned(), uri: uri.to_owned(), local: local.to_owned(), value: value.to_owned() })
            }
            None => made.complete = false,
        }
        Ok(())
    }

    /// The text of the kept element `el` as it stands in the style, with a declaration for each prefix it uses and does not declare itself that the writer does not bind to the same namespace
    /// where the style is written; `None` when the namespace of a prefix is nowhere (the style is then not complete). **Bounded** to what a shape's kept markup may be, charged by what it walks
    /// and copies before it is made, and refused in words past the bound or for markup XML cannot hold.
    fn style_element<'a>(
        &mut self,
        el: &RawElement,
        bound: impl IntoIterator<Item = &'a crate::write::kept::Bindings> + Clone,
        pfx: &Pfx,
    ) -> Result<Option<String>, WriteError> {
        if !super::super::model::bounded_element(el, super::MOST_KEPT_PER_SHAPE) {
            return Err(WriteError::Unsupported(WORDS_KEPT_STYLE_BIG));
        }
        let Some(free) = crate::write::kept::free_prefixes(el) else { return refuse(crate::write::splice::WORDS_XML_CHAR) };
        self.work.charge_steps(free.steps)?;
        let writer = [
            (pfx.draw.as_str(), crate::write::scan::DRAW_URI),
            (pfx.svg.as_str(), crate::write::scan::SVG_URI),
            (pfx.fo.as_str(), crate::write::scan::FO_URI),
            (pfx.style.as_str(), crate::write::scan::STYLE_URI),
            (pfx.text.as_str(), crate::write::scan::TEXT_URI),
        ];
        let mut declare: Vec<(String, String)> = Vec::new();
        for prefix in &free.prefixes {
            // The default namespace of an element with no prefix is the one the file bound where it stood, when it bound one.
            let uri = bound.clone().into_iter().find_map(|b| b.get(prefix.as_str())).map(String::as_str);
            match uri {
                None if prefix.is_empty() => {}
                None => return Ok(None),
                Some(uri) if writer.iter().any(|(p, u)| p == prefix && *u == uri) => {}
                Some(uri) => declare.push((prefix.clone(), uri.to_owned())),
            }
        }
        let estimate = crate::write::model::element_total(el, super::MOST_KEPT_PER_SHAPE).unwrap_or(super::MOST_KEPT_PER_SHAPE);
        let declared = declare.iter().fold(0usize, |sum, (p, u)| sum.saturating_add(p.len()).saturating_add(u.len()).saturating_add(10));
        self.work.charge_copy(estimate.saturating_add(declared), self.limits)?;
        match crate::write::kept::element_text(el, &declare) {
            Some(text) => Ok(Some(text)),
            None => refuse(crate::write::splice::WORDS_XML_CHAR),
        }
    }

    /// The unit the length attribute `attr` was written in: the unit the shape's `units` hold, else the one the cell `cell` shows, else the page's.
    fn unit_for(&self, read: Option<&AsRead>, attr: &str, sheet: &Sheet, cell: &str) -> LenUnit {
        read.and_then(|r| r.held_unit(attr)).or_else(|| sheet.cell(cell).and_then(|c| c.unit.as_deref()).and_then(unit_of_word)).unwrap_or(self.unit)
    }

    /// The attributes `held` gives as text under the writer's prefixes, each in the namespace its own prefix names, and what they say for the check of the bytes made.
    fn held_text(&self, pfx: &Pfx, held: &[(String, String)], wrote: &mut Vec<(Ns, &'static str)>) -> Result<String, WriteError> {
        let mut text = String::new();
        for (name, value) in held {
            let Some((ns, local)) = attr_ns(name) else { return Err(WriteError::Unsupported(crate::write::splice::WORDS_XML_CHAR)) };
            text.push_str(&pfx.attr_checked(ns, local, value)?);
            match (ns, local) {
                (Ns::Svg, "x" | "y" | "width" | "height" | "x1" | "y1" | "x2" | "y2") => wrote.push((Ns::Svg, static_name(local))),
                (Ns::Draw, "transform") => wrote.push((Ns::Draw, "transform")),
                _ => {}
            }
        }
        Ok(text)
    }

    /// The attributes of the frame of a 2-D shape (the corner it is placed by, its size and its transform) for the element of `read` or for a new shape (`read` is `None`): the frame the
    /// shape was read with while it is the frame it has, moved by what it moved, and otherwise written from the cells in the units the lengths were read in.
    pub(super) fn frame_attrs(
        &mut self,
        read: Option<&AsRead>,
        (sheet, pose, angle, flipped): (&Sheet, &Pose, f64, bool),
        pfx: &Pfx,
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<String, WriteError> {
        let (w, h) = pose.size;
        let centre = pose.centre;
        let now = Now { centre, size: pose.size, angle, flipped };
        if let Some(held) = read.and_then(|r| r.frame2d.as_deref()) {
            match now.against(held, self.height) {
                Was::Same => return self.held_text(pfx, held, wrote),
                Was::Moved(dx, dy) => {
                    let mut moved: Vec<(String, String)> = Vec::with_capacity(held.len());
                    for (name, value) in held {
                        if name == "draw:transform" {
                            let text = translated(value, dx, dy, self.unit).ok_or(WriteError::Unsupported("a length too large to write"))?;
                            moved.push((name.clone(), text));
                        } else {
                            moved.push((name.clone(), value.clone()));
                        }
                    }
                    return self.held_text(pfx, &moved, wrote);
                }
                Was::Changed => {}
            }
        }
        let mut text = String::new();
        for (attr, cell, value) in [("svg:width", "Width", w), ("svg:height", "Height", h)] {
            let unit = self.unit_for(read, attr, sheet, cell);
            let local = local(attr);
            text.push_str(&pfx.attr(Ns::Svg, local, &self.length_in(value, unit)?));
            wrote.push((Ns::Svg, static_name(local)));
        }
        let top = self.height - centre.1;
        if angle == 0.0 {
            for (attr, cell, value) in [("svg:x", "PinX", centre.0 - w / 2.0), ("svg:y", "PinY", top - h / 2.0)] {
                let unit = self.unit_for(read, attr, sheet, cell);
                let local = local(attr);
                text.push_str(&pfx.attr(Ns::Svg, local, &self.length_in(value, unit)?));
                wrote.push((Ns::Svg, static_name(local)));
            }
        } else {
            let (s, c) = angle.sin_cos();
            let (hx, hy) = (w / 2.0 * c + h / 2.0 * s, -w / 2.0 * s + h / 2.0 * c);
            let unit = self.unit_for(read, "draw:transform", sheet, "PinX");
            let transform = turned(angle, centre.0 - hx, top - hy, unit).ok_or(WriteError::Unsupported("a length too large to write"))?;
            text.push_str(&pfx.attr(Ns::Draw, "transform", &transform));
            wrote.push((Ns::Draw, "transform"));
        }
        Ok(text)
    }

    /// The ends of a segment (`svg:x1` to `svg:y2`): the ends as written while they are the ends the segment has, and otherwise the ends from the cells, each in the unit it was read in.
    pub(super) fn ends_attrs(
        &mut self,
        read: Option<&AsRead>,
        sheet: &Sheet,
        ends: [(f64, f64); 2],
        pfx: &Pfx,
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> Result<String, WriteError> {
        if let Some(read) = read
            && let Some(held) = read.frame1d.as_deref()
            && self.ends_as_read(read, ends)
        {
            return self.held_text(pfx, held, wrote);
        }
        let mut text = String::new();
        let values = [
            ("svg:x1", "BeginX", ends[0].0),
            ("svg:y1", "BeginY", self.height - ends[0].1),
            ("svg:x2", "EndX", ends[1].0),
            ("svg:y2", "EndY", self.height - ends[1].1),
        ];
        for (attr, cell, value) in values {
            let unit = self.unit_for(read, attr, sheet, cell);
            let local = local(attr);
            text.push_str(&pfx.attr(Ns::Svg, local, &self.length_in(value, unit)?));
            wrote.push((Ns::Svg, static_name(local)));
        }
        Ok(text)
    }

    /// The names of the automatic styles of a shape written as a new one, **for a shape read from a file**: the style it named while it is as it was, one of its own otherwise, and none
    /// for a shape that named none and has nothing for one. Returns the graphic style's name, and the paragraph style's (`None` here: the shape's own `draw:text-style-name` is the
    /// file's, written with the attributes it kept, and the style it names is the text writer's).
    pub(super) fn graphic_style(
        &mut self,
        read: Option<&AsRead>,
        ops: Vec<Op>,
        dia: &[(String, String)],
        parent: Option<&str>,
    ) -> Result<Option<String>, WriteError> {
        let held = read.and_then(|r| r.style.as_deref());
        if held.is_none() && ops.is_empty() && dia.is_empty() && parent.is_none() {
            return Ok(None);
        }
        // `draw:style-name=""` names no style: the attribute is written as the file had it, and no style is made for it.
        if held == Some("") && ops.is_empty() && dia.is_empty() && parent.is_none() {
            return Ok(Some(String::new()));
        }
        let held = held.filter(|name| !name.is_empty());
        self.note_markers(&ops);
        let mut attrs = Vec::new();
        let mut name = held.map(str::to_owned);
        // A shape that names a style already made, with the same inputs, names it: no byte is made or copied for it.
        if let Some(held) = &name
            && self
                .book
                .as_ref()
                .and_then(|b| b.held_inputs.get(held))
                .is_some_and(|(p, o, d)| p.as_deref() == parent && *o == ops && d.as_slice() == dia)
        {
            return Ok(Some(held.clone()));
        }
        // What the file's style kept that no cell reads (the attributes of its graphic properties): made once for the name, and every style made for the shape has it (a clone of it included).
        let kept = match &name {
            Some(held) => self.remainder_of(held)?,
            None => None,
        };
        if let (Some(held), true) = (&name, kept.as_ref().is_none_or(|k| k.complete)) {
            self.work.graphic_remainders_written.insert(held.clone());
        }
        // The attributes a cell writes are the cell's; the others are the kept ones (borrowed: nothing is copied here).
        let rest = kept.as_deref().map(|k| &k.rest);
        let extra: Vec<(Ns, &str, &str)> = kept
            .as_ref()
            .map(|k| {
                k.attrs
                    .iter()
                    .filter(|(ns, l, _)| !ops.iter().any(|o| o.ns == *ns && o.local == l.as_str()))
                    .map(|(ns, l, v)| (*ns, l.as_str(), v.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(held) = &name {
            // The file's name is written once, for the style the shape has; a shape whose style differs from the one written under it takes a name of its own.
            let text = self.new_style_with(held, parent, &ops, (&extra, rest), dia, &mut attrs)?;
            let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
            match book.held_styles.get(held) {
                Some(done) if *done == text => return Ok(Some(held.clone())),
                Some(_) => name = None,
                None => {
                    book.held_inputs.insert(held.clone(), (parent.map(str::to_owned), ops.clone(), dia.to_vec()));
                    book.held_styles.insert(held.clone(), text.clone());
                    self.put_style(text)?;
                    let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
                    book.written.push(Written { name: held.clone(), family: Fam::Graphic, attrs, prop: 0 });
                    return Ok(Some(held.clone()));
                }
            }
        }
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
        let fresh = book.names.next(Fam::Graphic);
        debug_assert!(name.is_none());
        let mut attrs = Vec::new();
        let text = self.new_style_with(&fresh, parent, &ops, (&extra, rest), dia, &mut attrs)?;
        self.put_style(text)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
        book.written.push(Written { name: fresh.clone(), family: Fam::Graphic, attrs, prop: 0 });
        Ok(Some(fresh))
    }

    /// The automatic paragraph style a shape of a file names for its text by `draw:text-style-name`, which is written among the attributes it kept (the model writer does not make a name of its
    /// own for it), is **noted** here and written once at the end of the part by [`Editor::put_held_paragraphs`], empty: the shapes that name it share it, and what the style says
    /// beyond its name is the text writer's, which has written it by then when a paragraph of the file named it.
    pub(super) fn text_style_element(&mut self) -> Result<(), WriteError> {
        let Some(name) = self.kept.attrs.iter().find(|(n, _, _)| n == "draw:text-style-name").map(|(_, v, _)| v.clone()) else { return Ok(()) };
        self.work.charge_steps(1)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
        book.held_paragraphs.insert(name);
        Ok(())
    }

    /// Writes the paragraph styles the shapes' `draw:text-style-name` names that no style of the text writer stands for (see [`Editor::text_style_element`]): each once, empty, in the order of
    /// their names. A name the text writer made a style under is left to it.
    pub(crate) fn put_held_paragraphs(&mut self) -> Result<(), WriteError> {
        let names: Vec<String> = match self.book.as_mut() {
            Some(book) => std::mem::take(&mut book.held_paragraphs).into_iter().collect(),
            None => return Ok(()),
        };
        for name in names {
            if self.work.text_names_written.contains(&(false, name.clone())) {
                continue;
            }
            self.work.charge_steps(1)?;
            let scope = self
                .book
                .as_ref()
                .and_then(|b| b.auto.as_ref())
                .map(|a| a.scope)
                .ok_or(WriteError::Unsupported("a part with no automatic styles"))?;
            let pfx = Pfx::of(&mut self.scopes, scope, self.work);
            self.put_style(format!(
                "<{0}:style {0}:name=\"{1}\" {0}:family=\"paragraph\"{2}/>",
                pfx.style,
                crate::write::splice::escape_value(&name, b'"'),
                pfx.declare
            ))?;
            let book = self.book.as_mut().ok_or(WriteError::Mismatch("a new shape without the styles of the part"))?;
            book.written.push(Written { name, family: Fam::Paragraph, attrs: Vec::new(), prop: 0 });
        }
        Ok(())
    }

    /// The start tag's first attributes of a shape: its style, its text style (a new shape's; the shape of a file keeps its own among the attributes it kept), its layer, its place in
    /// the page's order, its name and its ids, in the order the writer always wrote them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn head_attrs(
        &self,
        n: &NewShape,
        pfx: &Pfx,
        (graphic, paragraph, layer): (Option<&str>, Option<&str>, Option<&str>),
        z: Option<i64>,
        id: Option<&str>,
        wrote: &mut Vec<(Ns, &'static str)>,
    ) -> String {
        let mut tag = String::new();
        if let Some(graphic) = graphic {
            tag.push_str(&pfx.attr(Ns::Draw, "style-name", graphic));
        }
        if let Some(paragraph) = paragraph {
            tag.push_str(&pfx.attr(Ns::Draw, "text-style-name", paragraph));
        }
        if let Some(layer) = layer {
            tag.push_str(&pfx.attr(Ns::Draw, "layer", layer));
        }
        if let Some(z) = z {
            tag.push_str(&pfx.attr(Ns::Draw, "z-index", &z.to_string()));
            wrote.push((Ns::Draw, "z-index"));
        }
        if let Some(name) = n.shape.name_u.as_deref() {
            tag.push_str(&pfx.attr(Ns::Draw, "name", name));
        }
        if let Some(id) = id {
            if !self.legacy {
                tag.push_str(&format!(" xml:id=\"{id}\""));
            }
            tag.push_str(&pfx.attr(Ns::Draw, "id", id));
        }
        tag
    }

    /// What a new shape of the model path held when it was read, how many names and values of it the writer does not write (the note counts them), and the kind it is written as: a shape whose
    /// outline is its own element's, and as it was read, is written as that element ([`Kind::Plain`]).
    pub(super) fn as_read_kind(&mut self, n: &NewShape, kind: Kind) -> Result<(Option<AsRead>, usize, Kind), WriteError> {
        // Counted before anything is made, **by what is walked and cloned**: each held value, each of its attributes and kept nodes, the attributes and the children of the elements it keeps,
        // and each unit are a step (a shape with one held value of 100,000 attributes is 100,000 steps).
        if self.work.model_path
            && let Some(slot) = n.shape.ext.get()
        {
            self.work.charge_steps(walked(&slot.odf))?;
        }
        let (read, refused) = match (self.work.model_path && n.shape.children.is_empty()).then(|| AsRead::of(&n.shape)) {
            Some(Ok(read)) => (Some(read), 0),
            Some(Err(NotRead::Refused(count))) => (None, count),
            _ => (None, 0),
        };
        let kind = match (&read, kind) {
            (Some(r), Kind::Custom) if self.writes_plain(r, n, &n.shape.sheet) => Kind::Plain,
            (_, kind) => kind,
        };
        Ok((read, refused, kind))
    }

    /// Whether the shape is written as the plain element it was read from: the outline is as it was read, and the frame is one the element says.
    pub(super) fn writes_plain(&self, read: &AsRead, n: &NewShape, sheet: &Sheet) -> bool {
        if !read.plain || !read.outline_holds || !PLAIN.contains(&read.element.as_str()) {
            return false;
        }
        let Some(pose) = own::pose_2d(sheet, &n.outer) else { return false };
        let Ok((angle, flip)) = turn_and_flips(sheet, &pose) else { return false };
        // A flip is the frame's own only when the file's transform flipped it and still does (the held frame is judged with it); a shape flipped since is written as a custom shape.
        let flipped_as_read = read.frame2d.as_deref().and_then(held_frame).is_some_and(|f| f.flipped);
        if flip.0 || (flip.1 && !flipped_as_read) {
            return false;
        }
        let by_centre = matches!(read.element.as_str(), "circle" | "ellipse") && read.centre.is_some();
        if by_centre && (angle != 0.0 || flip.1) {
            return false;
        }
        match read.element.as_str() {
            "polygon" | "polyline" => read.view_box.is_some() || !read.unread_outline.is_empty(),
            "path" => read.path.is_some(),
            _ => true,
        }
    }

    /// A plain shape written as the element it was read from (see the module documentation): `draw:rect`, `draw:ellipse`, `draw:circle`, `draw:regular-polygon`, `draw:polygon`,
    /// `draw:polyline`, `draw:path`, or a `draw:frame` that shows a text box.
    pub(super) fn plain_text(
        &mut self,
        n: &NewShape,
        diff: &PageDiff,
        pfx: &Pfx,
        (id, z): (Option<&str>, Option<i64>),
        tc: &TierCtx<'_>,
        read: &AsRead,
    ) -> Result<Element, WriteError> {
        let sheet = &*n.shape.sheet;
        // The element is written into the markup: only the elements of a plain shape are.
        if !PLAIN.contains(&read.element.as_str()) {
            return refuse("a shape read from an element the writer does not write back");
        }
        if n.shape.name_u.as_deref().is_some_and(super::super::splice::has_bad_char)
            || n.shape.name.as_deref().is_some_and(super::super::splice::has_bad_char)
        {
            return refuse(super::super::splice::WORDS_XML_CHAR);
        }
        let parent = self.parent_of(sheet)?;
        let pose = own::pose_2d(sheet, &n.outer).ok_or(WriteError::Unsupported("a new shape whose frame is not numbers"))?;
        let (angle, flip) = turn_and_flips(sheet, &pose)?;
        let here = own::pose_2d(sheet, &crate::read::Affine::IDENTITY).ok_or(WriteError::Unsupported("a new shape whose frame is not numbers"))?;
        let kept_body = std::mem::take(&mut self.kept.body);
        let kept_before = std::mem::take(&mut self.kept.before);
        let kept_after = std::mem::take(&mut self.kept.after);
        let (paragraphs, holds_text, paras, plain, text_native) = self.paragraphs_text(n, pfx)?;
        let (_, mut ops, mut style_names) = super::native_style_as(sheet, &[], true, true);
        crate::write::held::graphic_ops_said(&n.shape, &mut ops, &mut style_names);
        let ops = self.in_cell_units(ops, sheet);
        let display = crate::write::fx::display_of(sheet, &ops);
        let layer_row = read.layer.as_deref().and_then(|name| diff.layers.row_named(name));
        let native = native::Native {
            frame: native::Frame::TwoD { cx: here.centre.0, cy: here.centre.1, w: pose.size.0, h: pose.size.1, angle, flip },
            style: &style_names,
            display: Some(&display),
            geometry: true,
            outline_sections: 1,
            picture: false,
            connection: rebuilt_connection(sheet, self.work.model_path),
            connector: false,
            route: None,
            glued: [false, false],
            text: &plain,
            text_native,
            name: n.shape.name_u.as_deref(),
            layer: layer_row,
            outline: None,
            format: self.format_written(diff, n, &plain, text_native),
            picture_bytes: None,
            expected: diacraft_doc::ShapeType::Shape,
        };
        let (tier2, dia) = self.extension_of(n, &native, tc)?;
        let graphic = self.graphic_style(Some(read), ops, &dia, parent.as_deref())?;
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let element = read.element.as_str();
        let mut tag = format!("<{}:{element}{}", pfx.draw, pfx.declare);
        tag.push_str(&self.head_attrs(n, pfx, (graphic.as_deref(), None, read.layer.as_deref()), z, id, &mut wrote));
        // The frame: a circle or an ellipse given by its centre in that form, any other as its corner, size and transform.
        let by_centre = matches!(element, "circle" | "ellipse") && read.centre.is_some();
        if let (true, Some(centre)) = (by_centre, read.centre.as_deref()) {
            tag.push_str(&self.centre_attrs(read, centre, sheet, &pose, pfx)?);
        } else {
            tag.push_str(&self.frame_attrs(Some(read), (sheet, &pose, angle, flip.1), pfx, &mut wrote)?);
        }
        tag.push_str(&self.held_text(pfx, &read.outline, &mut Vec::new())?);
        if element == "rect" {
            tag.push_str(&self.held_text(pfx, &read.corner, &mut Vec::new())?);
        }
        match element {
            // An outline the reader could not read is the file's attributes, which `kept_tag` writes as they were; a view box it did read is written from what it held.
            "polygon" | "polyline" if !read.unread_outline.is_empty() => {
                if let Some(view_box) = read.view_box.as_deref().filter(|_| !read.unread_outline.contains(&"svg:viewBox")) {
                    tag.push_str(&pfx.attr_checked(Ns::Svg, "viewBox", view_box)?);
                }
            }
            "polygon" | "polyline" => {
                let view_box = read.view_box.as_deref().unwrap_or_default();
                let vb = crate::read::geometry::view_box(view_box)
                    .and_then(Result::ok)
                    .ok_or(WriteError::Unsupported("a view box the reader does not read"))?;
                let points = crate::write::geometry::points_text(sheet, &vb, element == "polygon").map_err(WriteError::Unsupported)?;
                tag.push_str(&pfx.attr_checked(Ns::Svg, "viewBox", view_box)?);
                tag.push_str(&pfx.attr(Ns::Draw, "points", &points));
            }
            "path" => tag.push_str(&self.held_text(pfx, read.path.as_deref().unwrap_or_default(), &mut Vec::new())?),
            _ => {}
        }
        self.text_style_element()?;
        self.kept_tag(&mut tag, pfx)?;
        self.work.charge_attribute(self.limits)?;
        let tier2_written = (!dia.is_empty()).then(|| std::sync::Arc::new(tier2));
        let body = if element == "frame" {
            format!("<{0}:text-box>{paragraphs}</{0}:text-box>", pfx.draw)
        } else if !holds_text && !read.bare_text {
            String::new()
        } else {
            paragraphs
        };
        let paras = if holds_text || read.bare_text || element == "frame" { paras } else { Vec::new() };
        Ok((
            // The children the schema puts before the text first, and a frame's after its content.
            format!("{tag}>{kept_before}{body}{kept_body}{kept_after}</{}:{element}>", pfx.draw),
            NewWrote { attrs: wrote, paras, tier2: tier2_written, ..NewWrote::default() },
        ))
    }

    /// The centre form of a circle or an ellipse: `svg:cx` and `svg:cy` and the radius or radii, as written while the shape is as it was read, and from the cells in the unit of `svg:cx`
    /// otherwise.
    fn centre_attrs(&mut self, read: &AsRead, held: &[(String, String)], sheet: &Sheet, pose: &Pose, pfx: &Pfx) -> Result<String, WriteError> {
        let (w, h) = pose.size;
        let (cx, cy) = (pose.centre.0, self.height - pose.centre.1);
        let read_at = (inches(held, "svg:cx"), inches(held, "svg:cy"));
        let radii = match (inches(held, "svg:r"), inches(held, "svg:rx"), inches(held, "svg:ry")) {
            (Some(r), _, _) => Some((r, r)),
            (None, Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        if let ((Some(x), Some(y)), Some((rx, ry))) = (read_at, radii)
            && same(x, cx)
            && same(y, cy)
            && same(2.0 * rx, w)
            && same(2.0 * ry, h)
        {
            return self.held_text(pfx, held, &mut Vec::new());
        }
        let unit = unit_of_suffix(text_of(held, "svg:cx").map_or("", |t| t.get(t.len().saturating_sub(2)..).unwrap_or(""))).unwrap_or(self.unit);
        let _ = (read, sheet);
        let mut text = String::new();
        let mut put = |local: &str, value: f64| -> Result<(), WriteError> {
            text.push_str(&pfx.attr(Ns::Svg, local, &length_text(value, unit).ok_or(WriteError::Unsupported("a length too large to write"))?));
            Ok(())
        };
        put("cx", cx)?;
        put("cy", cy)?;
        if held.iter().any(|(k, _)| k == "svg:r") {
            if !same(w, h) {
                return Err(WriteError::Unsupported("a circle given by its centre resized to another height than width"));
            }
            put("r", w / 2.0)?;
        } else {
            put("rx", w / 2.0)?;
            put("ry", h / 2.0)?;
        }
        Ok(text)
    }
}

/// What reading a shape's held values walks and clones, in steps (see [`Editor::as_read_kind`]): the units, and for each held value one, its attributes, its kept nodes and the attributes and the
/// children of each element it keeps.
fn walked(odf: &diacraft_doc::OdfShape) -> usize {
    let mut steps = 1usize.saturating_add(odf.units.len());
    for h in &odf.held {
        steps = steps.saturating_add(1).saturating_add(h.attrs.len()).saturating_add(h.kept.len());
        for placed in h.kept.iter() {
            if let RawNode::Element(el) = &placed.node {
                steps = steps.saturating_add(el.attrs.len()).saturating_add(el.children.len());
            }
        }
    }
    steps
}

/// Whether a picture's entry name, as the file named it, is one a package of ours may be written with: a relative name of the entries the reader finds a picture by, with no `.` or `..`
/// segment and no character a name cannot hold, and none of the entries the writer makes itself, nor the name a flat drawing's inline picture has (it is no entry's).
pub(crate) fn entry_name_ok(name: &str) -> bool {
    // A picture read from a flat drawing is named by no entry (`office:binary-data`): it is written under a name of the writer's own.
    !name.is_empty()
        && name != crate::picturetype::INLINE_PART
        && name.len() <= 1024
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && name.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && !matches!(name, "mimetype" | "content.xml" | "styles.xml" | "meta.xml" | "settings.xml")
        && !name.starts_with("META-INF/")
        && !name.starts_with("DiaCraft/")
}

/// The attributes the `draw:image` of a picture of a file had (its link type, its MIME type), as written, for the element written back: those under the `xlink` prefix as they are (the
/// element declares it), and those of ODF's drawing namespace under the writer's own prefix; any other prefix has no place and is left out.
///
/// **A name that is no qualified name, or a value with a character XML does not allow, is left out and counted** (the second of the pair): the name is written into the element's own tag, and
/// a name with a quote, a space or a `>` in it would put markup of its own there.
pub(super) fn image_attrs(shape: &Shape, pfx: &Pfx) -> (String, usize) {
    let Some(foreign) = shape.foreign.as_deref() else { return (String::new(), 0) };
    let Some(held) = foreign.ext.get() else { return (String::new(), 0) };
    let mut text = String::new();
    let mut left = 0usize;
    for (name, value) in held.odf.raw.iter() {
        if !crate::write::kept::is_qname(name) || has_bad_char(value) {
            left += 1;
            continue;
        }
        if let Some(local) = name.strip_prefix("xlink:") {
            if local != "href" {
                text.push_str(&format!(" xlink:{local}=\"{}\"", crate::write::splice::escape_value(value, b'"')));
            }
        } else if let Some((Ns::Draw, local)) = attr_ns(name) {
            text.push_str(&pfx.attr(Ns::Draw, local, value));
        } else {
            left += 1;
        }
    }
    (text, left)
}

/// Whether the `draw:type` a connector was read with (`None`: none was written) says what the route cells now say, `ShapeRouteStyle` and `ConLineRouteExt`: the reader's own map of the
/// type to the two cells (`read::connectors::route_cells`).
pub(crate) fn type_agrees(held: Option<&str>, (style, ext): (Option<f64>, Option<f64>)) -> bool {
    let (want_style, want_ext) = route_of_type(held);
    (style == Some(want_style) && ext == Some(want_ext)) || (held.is_none() && style.is_none() && ext.is_none())
}

/// The cells `ShapeRouteStyle` and `ConLineRouteExt` the reader makes of the `draw:type` of a connector (`None`: it wrote none): `standard` and any type the standard does not define give
/// (1, 1), `line` and `lines` (2, 1) and `curve` (1, 2), as `read::connectors::route_cells` does.
pub(crate) fn route_of_type(draw_type: Option<&str>) -> (f64, f64) {
    match draw_type {
        None | Some("standard") => (1.0, 1.0),
        Some("line" | "lines") => (2.0, 1.0),
        Some("curve") => (1.0, 2.0),
        Some(_) => (1.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture is written under the entry it was read from only when that is a plain relative name of a package: no `.` or `..`, no backslash, no control character, none of the
    /// entries the writer makes itself.
    #[test]
    fn an_entry_name_is_one_a_package_of_ours_is_written_with_only_when_it_is_a_plain_relative_one() {
        for good in ["Pictures/a.png", "ObjectReplacements/Object 1", "a", "Pictures/a b.png", "Pictures/.hidden"] {
            assert!(entry_name_ok(good), "{good}");
        }
        for bad in [
            "",
            "/Pictures/a.png",
            "Pictures/../a.png",
            "./a.png",
            "Pictures//a.png",
            "Pictures/",
            "a\\b",
            "a\u{1}b",
            "mimetype",
            "content.xml",
            "META-INF/manifest.xml",
            "DiaCraft/document.xml",
            // The name a picture of a flat drawing has: no entry of any package, so the writer names the entry itself.
            "office:binary-data",
            &"a".repeat(1025),
        ] {
            assert!(!entry_name_ok(bad), "{bad:?}");
        }
        assert_eq!(crate::picturetype::INLINE_PART, "office:binary-data");
    }

    /// A connector's `draw:type` as written says what the route cells say when the reader's map of it gives them: the same map as `read::connectors::route_cells`.
    #[test]
    fn a_type_the_file_wrote_agrees_with_the_route_cells_the_reader_gave_it() {
        for (held, cells) in [
            (None, (Some(1.0), Some(1.0))),
            (Some("standard"), (Some(1.0), Some(1.0))),
            (Some("line"), (Some(2.0), Some(1.0))),
            (Some("lines"), (Some(2.0), Some(1.0))),
            (Some("curve"), (Some(1.0), Some(2.0))),
            (Some("a type the standard does not define"), (Some(1.0), Some(1.0))),
        ] {
            assert!(type_agrees(held, cells), "{held:?} {cells:?}");
        }
        assert!(!type_agrees(Some("line"), (Some(1.0), Some(1.0))), "the cells were edited");
        assert!(!type_agrees(None, (Some(2.0), Some(1.0))), "a type the file did not write, a route kind the cells now say");
        assert!(!type_agrees(Some("curve"), (None, None)));
        assert!(type_agrees(None, (None, None)), "a connector with no route cells says none");
    }

    /// The route kind a connector's `draw:type` gives the reader's cells (the writer carries the model's cells against it): the standard's three, and the default for any other
    /// type or none.
    #[test]
    fn the_route_cells_a_type_gives_are_the_readers() {
        assert_eq!(route_of_type(None), (1.0, 1.0));
        assert_eq!(route_of_type(Some("standard")), (1.0, 1.0));
        assert_eq!(route_of_type(Some("line")), (2.0, 1.0));
        assert_eq!(route_of_type(Some("lines")), (2.0, 1.0));
        assert_eq!(route_of_type(Some("curve")), (1.0, 2.0));
        assert_eq!(route_of_type(Some("a type the standard does not define")), (1.0, 1.0));
        assert_eq!(route_of_type(Some("")), (1.0, 1.0));
    }

    /// The names an attribute is written under: a qualified name of one of the five prefixes, and no other.
    #[test]
    fn an_attribute_name_is_written_only_when_it_is_a_qualified_name_under_a_prefix_the_writer_has() {
        assert_eq!(attr_ns("svg:x"), Some((Ns::Svg, "x")));
        assert_eq!(attr_ns("draw:kind"), Some((Ns::Draw, "kind")));
        for bad in ["x", "other:x", "svg:", ":x", "svg:x y", "svg:x\"", "svg:a:b", "svg:x>", "svg:x=\"1\" y", "xmlns:svg"] {
            assert_eq!(attr_ns(bad), None, "{bad:?}");
        }
    }
}
