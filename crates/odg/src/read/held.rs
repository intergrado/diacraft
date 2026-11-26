//! The held values of a shape (the native-format design's §3.3 and §3.4): what an `.odg` says of a shape that cells can only approximate,
//! kept exactly beside the stand-in cells, each with the stamp of the cells it stands beside as the reader set them. A writer of ODF uses a value while its stamp still matches.
//!
//! # Which value is which
//!
//! [`Value`] names each value R22 lists, with its group ([`diacraft_doc::HeldGroup`], which only groups them), its source and **its stamp, by the list for that value and not by
//! the group's**: [`Value::stamp`] is the one place the lists are, for the reader now and for the writer that checks a value later. A group may hold several values on a
//! shape (a fill has its style and its opacity name, a text its three).
//!
//! | Value | Held (as written) | Stamped by |
//! |---|---|---|
//! | [`Value::FillStyle`], [`Value::HatchFill`] | the shape's own automatic style's `draw:fill` (when it is `gradient`, `hatch` or `bitmap`) with its `draw:fill-gradient-name`, `-hatch-name`, `-hatch-solid`, `-image-name` and the other `draw:fill-image-*` | `FillPattern`, `FillGradientEnabled`, `FillGradientDir` and the rows of `FillGradient`; `FillGradientAngle` for a fill (the stand-in's angle); and `FillForegnd` for a hatch |
//! | [`Value::FillOpacity`] | `draw:opacity-name` | `FillForegndTrans` |
//! | [`Value::Dash`] | `draw:stroke-dash`, a dash DiaCraft did not write | none |
//! | [`Value::LineJoin`] | `draw:stroke-linejoin` | `LineCap` |
//! | [`Value::ForeignMarker`] | `draw:marker-start` or `-end` naming a marker DiaCraft did not write, with its `-width` and `-center` | that end's arrow and size cells |
//! | [`Value::OwnMarkerWidth`] | the `-width` (and `-center`) of a marker DiaCraft wrote, or of an end that names no marker (an empty name) (R22 holds the widths no size class matches; **this reader holds every such width**, which is exactly good while the stamp is) | that end's arrow and size cells and `LineWeight` |
//! | [`Value::Shadow`] | every `shadow` attribute of the style (`draw:shadow`, `-color`, `-opacity`, `-offset-x`, `-offset-y`, `-blur`, `loext:shadow-*`) | `ShdwPattern`, `ShapeShdwType` |
//! | [`Value::TextJustify`], [`Value::TextPadding`], [`Value::TextWriting`] | `draw:textarea-vertical-align="justify"`; `fo:padding` as one attribute; the graphic style's `style:writing-mode` | `VerticalAlign`; the four margins; `TextDirection` |
//! | [`Value::TabStops`] | the graphic style's `style:tab-stop-distance` and its `style:tab-stops` element | none |
//! | [`Value::Frame2d`], [`Value::CircleCentre`], [`Value::Frame1d`] | a shape's `draw:transform` with its `svg:x`, `svg:y`, `svg:width` and `svg:height` (a 1-D shape's `svg:x1` to `svg:y2`); the form a circle or ellipse given by its centre was written in | none: judged by the pose on the page |
//! | [`Value::Layer`] | `draw:layer` | none |
//! | [`Value::EnhancedGeometry`] | **nothing but its stamp**: the element itself stays in the shape's kept markup (`odf.kept`), where the splice writer reads it | the `Geometry` sections and the `User` rows `ODFEq<i>` |
//! | [`Value::Route`], [`Value::ConnectorType`] | a connector's `svg:d`, `svg:viewBox` and `draw:line-skew`; its `draw:type` (or, when it has none, an empty value: its absence) | none |
//! | [`Value::CornerRadii`] | a rectangle's `draw:corner-radius`, `svg:rx` and `svg:ry` | none |
//! | [`Value::PlainOutline`] | an element holding the plain shape's own element name and the attributes that give its outline (`draw:kind`, the angles; `draw:corners`, `draw:concave`, `draw:sharpness`) | the `Geometry` sections |
//! | [`Value::ViewBox`], [`Value::PathData`] | a polyline's or polygon's `svg:viewBox`; a path's `svg:d` and `svg:viewBox` | none; the `Geometry` sections |
//!
//! The glue points are held on the rows ([`crate::read::connectors`]), stamped by [`diacraft_doc::stamp_row`].
//!
//! **Copied, not lifted** (R22 says the reader lifts `draw:line-skew`, the corner radii and the glue elements out of the shape's kept markup): they are copied into the value, and the kept
//! markup is as it was, since the splice writer reads them there and an unedited save is the file's bytes. **The values of a common style** are not held here: a style sheet holds what
//! no cell reads in its own slot (`Style.ext.odf`, `odfdoc.rs`).
//!
//! # Cost
//!
//! A style's values are made **once, when the style is read** ([`style_templates`], charged there), and every shape that names the style shares the same attribute lists behind their
//! pointers, so a 1 MiB attribute the style holds is one copy for any number of shapes and groups. A shape charges [`super::cost::HELD`] items for each value it holds, and the
//! values it makes from its own element are charged by their attributes' count and bytes, before they are made; a list of attributes that repeats (a `draw:layer` of every shape)
//! is interned and shared ([`intern`], at most [`super::shapes::MOST_SHARED`] lists).

use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{
    Held, HeldGroup, HeldStyleValue, OdfParagraph, OdfRun, OdfShape, OdfText, OdfTextStyle, Passthrough, RawAttrs, RawElement, RawNode, Row, RowKey,
    SectionKey, Sheet, Stamp, TextGroup, stamp_all, stamp_cells, stamp_rows_named, stamp_sections, stamp_text_group, stamp_text_row,
};

use super::Reader;
use super::names::{Attr, Attrs, Names, Ns};
use super::shapes::{Kindof, Shared, element_cost, raw_element};
use super::styles::{Kind, OdfStyle, P, StyleSet};
use super::text::ParaOdf;
use super::typed::{Marker, Name, Typed};
use super::units;
use crate::error::OdgError;

/// Which end of a line a value is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Begin,
    End,
}

/// One held value of a shape: see the module documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    FillStyle,
    HatchFill,
    FillOpacity,
    Dash,
    /// `draw:stroke="dash"`: the line is drawn with the dash, which the cells say as a `LinePattern` of 2 or more and the model writer cannot write natively; stamped by it.
    DashStroke,
    LineJoin,
    ForeignMarker(End),
    OwnMarkerWidth(End),
    Shadow,
    TextJustify,
    TextPadding,
    TextWriting,
    TabStops,
    Frame2d,
    CircleCentre,
    Frame1d,
    Layer,
    EnhancedGeometry,
    Route,
    ConnectorType,
    CornerRadii,
    PlainOutline,
    ViewBox,
    PathData,
    /// The element the shape was read from, for the shapes whose outline the element does not give (a line, a connector, a custom shape, a picture or object frame): **no stamp**, the
    /// element is what the file said. The shapes whose outline is the element's own hold [`Value::PlainOutline`] instead.
    Element,
}

impl Value {
    /// The group of the value is in.
    pub(crate) fn group(self) -> HeldGroup {
        match self {
            Value::FillStyle | Value::HatchFill | Value::FillOpacity => HeldGroup::Fill,
            Value::Dash | Value::DashStroke | Value::LineJoin => HeldGroup::Line,
            Value::ForeignMarker(_) | Value::OwnMarkerWidth(_) => HeldGroup::LineEnds,
            Value::Shadow => HeldGroup::Shadow,
            Value::TextJustify | Value::TextPadding | Value::TextWriting | Value::TabStops => HeldGroup::Text,
            Value::Frame2d | Value::CircleCentre | Value::Frame1d | Value::Layer => HeldGroup::Frame,
            Value::EnhancedGeometry
            | Value::Route
            | Value::ConnectorType
            | Value::CornerRadii
            | Value::PlainOutline
            | Value::ViewBox
            | Value::PathData
            | Value::Element => HeldGroup::Geometry,
        }
    }

    /// The cells directly under the shape's sheet that R22 says the value's keeping rests on.
    pub(crate) fn cells(self) -> &'static [&'static str] {
        match self {
            // `FillGradientAngle` too: the stand-in of a linear or axial gradient has one, and an edit of it alone ends the keeping as an edit of the layout does.
            Value::FillStyle => &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillGradientAngle"],
            Value::HatchFill => &["FillPattern", "FillGradientEnabled", "FillGradientDir", "FillForegnd"],
            Value::FillOpacity => &["FillForegndTrans"],
            Value::LineJoin => &["LineCap"],
            Value::DashStroke => &["LinePattern"],
            Value::ForeignMarker(End::Begin) => &["BeginArrow", "BeginArrowSize"],
            Value::ForeignMarker(End::End) => &["EndArrow", "EndArrowSize"],
            Value::OwnMarkerWidth(End::Begin) => &["BeginArrow", "BeginArrowSize", "LineWeight"],
            Value::OwnMarkerWidth(End::End) => &["EndArrow", "EndArrowSize", "LineWeight"],
            // Every cell the reader sets from the shadow attributes: the model writer says them with the held attributes while all of them are as read.
            Value::Shadow => &["ShdwPattern", "ShapeShdwType", "ShdwForegnd", "ShdwForegndTrans", "ShapeShdwOffsetX", "ShapeShdwOffsetY"],
            Value::TextJustify => &["VerticalAlign"],
            Value::TextPadding => &["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"],
            Value::TextWriting => &["TextDirection"],
            Value::Dash
            | Value::TabStops
            | Value::Frame2d
            | Value::CircleCentre
            | Value::Frame1d
            | Value::Layer
            | Value::EnhancedGeometry
            | Value::Route
            | Value::ConnectorType
            | Value::CornerRadii
            | Value::PlainOutline
            | Value::ViewBox
            | Value::PathData
            | Value::Element => &[],
        }
    }

    /// The sections whose rows the value rests on, beside its cells.
    pub(crate) fn sections(self) -> &'static [&'static str] {
        match self {
            Value::FillStyle | Value::HatchFill => &["FillGradient"],
            // A connector's route is judged by its rows too: the model writer writes the file's path only while the rows are as they were read.
            Value::EnhancedGeometry | Value::PlainOutline | Value::PathData | Value::Route => &["Geometry"],
            _ => &[],
        }
    }

    /// The stamp of the cells the value stands beside in `sheet` (the list for this value), or `None` for a value that is not stamped: one judged by the pose of the shape, or
    /// written as held on every save.
    pub(crate) fn stamp(self, sheet: &Sheet) -> Option<Stamp> {
        let cells = self.cells();
        let sections = self.sections();
        if cells.is_empty() && sections.is_empty() && self != Value::EnhancedGeometry {
            return None;
        }
        let mut parts: Vec<Stamp> = Vec::new();
        if !cells.is_empty() {
            parts.push(stamp_cells(sheet, cells));
        }
        if !sections.is_empty() {
            parts.push(stamp_sections(sheet, sections));
        }
        if self == Value::EnhancedGeometry {
            parts.push(stamp_rows_named(sheet, "User", "ODFEq"));
        }
        match parts.as_slice() {
            [one] => Some(*one),
            _ => Some(stamp_all(&parts)),
        }
    }
}

/// The attributes a value is made of, as written: the name and the value of each.
type Pairs<'t> = Vec<(&'t str, &'t str)>;

/// A value found among the properties of a style, before it is charged: what it is, its attributes, and the tab stops' element when it has one.
type Found<'t> = (Value, Pairs<'t>, Option<&'t El>);

/// A value of a paragraph's or a run's style found before it is charged.
pub(crate) type FoundText<'t> = (TextGroup, Pairs<'t>);

/// A held value a style gives every shape that names it: made once, when the style is read, and shared (the lists behind pointers) by the shapes.
#[derive(Clone)]
pub(crate) struct Template {
    pub value: Value,
    pub attrs: RawAttrs,
    pub kept: Passthrough,
}

/// The names of the `draw:fill-image-*` family and the other attributes of a fill held with `draw:fill`.
fn is_fill_attr(a: &Attr<'_>) -> bool {
    a.ns == Ns::Draw
        && (matches!(a.local, "fill" | "fill-gradient-name" | "fill-hatch-name" | "fill-hatch-solid" | "fill-image-name")
            || a.local.starts_with("fill-image-"))
}

fn is_shadow_attr(a: &Attr<'_>) -> bool {
    matches!(a.ns, Ns::Draw | Ns::Other | Ns::Style) && a.local.starts_with("shadow")
}

/// The values the properties of a shape's own automatic graphic style give (`graphic` is its `style:graphic-properties`, `paragraph` its `style:paragraph-properties`): the
/// templates the shapes that name it share. Charged before they are made.
pub(crate) fn style_templates<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    graphic: Option<&'t El>,
    paragraph: Option<&'t El>,
) -> Result<Vec<Template>, OdgError> {
    let mut found: Vec<Found<'t>> = Vec::new();
    let graphic_mark = graphic.map(|el| names.enter(el));
    if let Some(el) = graphic {
        let attrs = names.attrs(el);
        let pick = |wanted: &dyn Fn(&Attr<'_>) -> bool| -> Vec<(&'t str, &'t str)> {
            attrs.iter().filter(|a| wanted(a)).map(|a| (a.name, a.value)).collect()
        };
        let fill = attrs.peek(Ns::Draw, "fill");
        // **A fill names its gradient, hatch or picture in its style, whatever `draw:fill` it says there**: a style that names one and says no `draw:fill` of its own takes the
        // kind from its parent (LibreOffice's `Filled Blue` names its gradient over `Filled`'s `draw:fill="gradient"`), and one that says `solid` keeps the name for when the kind turns back.
        // The cells say neither the name nor the kind left unsaid, so the value is held with the attributes as written (272 trips lost a gradient reference so).
        let names_one = ["fill-gradient-name", "fill-hatch-name", "fill-image-name"].iter().any(|local| attrs.peek(Ns::Draw, local).is_some());
        if matches!(fill, Some("gradient" | "hatch" | "bitmap")) || names_one {
            let value = if fill == Some("hatch") { Value::HatchFill } else { Value::FillStyle };
            found.push((value, pick(&is_fill_attr), None));
        }
        let simple = |local: &'static str, value: Value, ns: Ns| {
            (value, attrs.iter().filter(move |a| a.ns == ns && a.local == local).map(|a| (a.name, a.value)).collect::<Vec<_>>())
        };
        for (local, value, ns) in [
            ("opacity-name", Value::FillOpacity, Ns::Draw),
            ("stroke-dash", Value::Dash, Ns::Draw),
            ("stroke-linejoin", Value::LineJoin, Ns::Draw),
            ("padding", Value::TextPadding, Ns::Fo),
            ("writing-mode", Value::TextWriting, Ns::Style),
        ] {
            let (value, pairs) = simple(local, value, ns);
            if !pairs.is_empty() {
                found.push((value, pairs, None));
            }
        }
        // `svg:stroke-linejoin` is the standard's name; LibreOffice writes `draw:stroke-linejoin`: both are held under the one value.
        let svg_join: Vec<(&str, &str)> =
            attrs.iter().filter(|a| a.ns == Ns::Svg && a.local == "stroke-linejoin").map(|a| (a.name, a.value)).collect();
        if !svg_join.is_empty() {
            match found.iter_mut().find(|(v, _, _)| *v == Value::LineJoin) {
                Some((_, pairs, _)) => pairs.extend(svg_join),
                None => found.push((Value::LineJoin, svg_join, None)),
            }
        }
        if attrs.peek(Ns::Draw, "stroke") == Some("dash") {
            found.push((Value::DashStroke, vec![("draw:stroke", "dash")], None));
        }
        if attrs.peek(Ns::Draw, "textarea-vertical-align") == Some("justify") {
            let pairs = attrs.iter().filter(|a| a.ns == Ns::Draw && a.local == "textarea-vertical-align").map(|a| (a.name, a.value)).collect();
            found.push((Value::TextJustify, pairs, None));
        }
        let shadow = pick(&is_shadow_attr);
        if !shadow.is_empty() {
            found.push((Value::Shadow, shadow, None));
        }
        for (end, word) in [(End::Begin, "start"), (End::End, "end")] {
            let (marker, width, centre) = (format!("marker-{word}"), format!("marker-{word}-width"), format!("marker-{word}-center"));
            let ends = pick(&|a: &Attr<'_>| a.ns == Ns::Draw && (a.local == marker || a.local == width || a.local == centre));
            if !ends.is_empty() {
                // Which of the two values it is (a foreign marker's, or the width of one of DiaCraft's own) is known when the style's marker names are looked up: both are kept.
                found.push((Value::ForeignMarker(end), ends, None));
            }
        }
    }
    if let Some(mark) = graphic_mark {
        names.leave(mark);
    }
    if let Some(el) = paragraph {
        let mark = names.enter(el);
        let attrs = names.attrs(el);
        let distance: Vec<(&str, &str)> =
            attrs.iter().filter(|a| a.ns == Ns::Style && a.local == "tab-stop-distance").map(|a| (a.name, a.value)).collect();
        let stops = el.elements().find(|c| names.is(c, Ns::Style, "tab-stops"));
        if !distance.is_empty() || stops.is_some() {
            found.push((Value::TabStops, distance, stops));
        }
        names.leave(mark);
    }
    // The charge: a template and its attribute lists, and the tab stops' element, before any list is made.
    let mut items = 0usize;
    let mut bytes = 0usize;
    for (_, pairs, element) in &found {
        items = items.saturating_add(1).saturating_add(pairs.len().saturating_mul(2));
        for (name, value) in pairs {
            bytes = bytes.saturating_add(name.len() + value.len());
        }
        if let Some(element) = element {
            items = items.saturating_add(element_cost(element));
        }
    }
    r.take_items(items)?;
    r.take_bytes(bytes)?;
    let mut out = Vec::with_capacity(found.len());
    for (value, pairs, element) in found {
        let mut kept = Passthrough::new();
        if let Some(element) = element {
            kept.push(0, RawNode::Element(raw_element(element)));
        }
        out.push(Template { value, attrs: RawAttrs::from_pairs(pairs), kept });
    }
    Ok(out)
}

/// The values a shape gets from the templates of its own automatic graphic style `graphic`: the foreign dash, the foreign markers and the widths of DiaCraft's own, and the
/// others as they are. Each is charged ([`super::cost::HELD`]) before it is made; its stamp is that of `sheet`, the shape's sheet as read.
pub(crate) fn from_style(r: &mut Reader, set: &StyleSet, graphic: Option<usize>, sheet: &Sheet, out: &mut Vec<Held>) -> Result<(), OdgError> {
    let Some(style) = graphic.and_then(|g| set.get(g)) else { return Ok(()) };
    for template in &style.templates {
        let value = match template.value {
            Value::Dash => {
                // A dash DiaCraft wrote is regenerated from the cells; any other (one the drawing does not hold included) is held.
                let own = match style.own_prop(Kind::Graphic, P::StrokeDash).map(|p| &p.typed) {
                    Some(Typed::Dash(Name::Found(index))) => set.dash(*index).is_some_and(|d| d.own.is_some()),
                    _ => false,
                };
                if own { continue } else { Value::Dash }
            }
            Value::ForeignMarker(end) => match marker_of(style, end) {
                // No line end (an empty name), or no marker name of its own (a width alone): the width is held under the width's value, and the name is the cells' to write. (an
                // empty name's width and centre flag were held by nothing, so a style written again said neither, and the end read its parent's width.)
                Some(Marker::Empty | Marker::Own(_)) | None => Value::OwnMarkerWidth(end),
                Some(Marker::Other | Marker::Missing | Marker::Unresolved) => Value::ForeignMarker(end),
            },
            other => other,
        };
        let attrs = if let Value::OwnMarkerWidth(_) = value {
            // Only the width and the centre flag, never the name (DiaCraft's own markers are written from the cells).
            let kept: Vec<(&str, &str)> = template.attrs.iter().filter(|(name, _)| name.ends_with("-width") || name.ends_with("-center")).collect();
            if kept.is_empty() {
                continue;
            }
            r.take_items(2usize.saturating_mul(kept.len()))?;
            r.take_bytes(kept.iter().map(|(n, v)| n.len() + v.len()).fold(0usize, usize::saturating_add))?;
            RawAttrs::from_pairs(kept)
        } else {
            template.attrs.clone()
        };
        r.take_items(super::cost::HELD)?;
        out.push(Held { group: value.group(), stamp: value.stamp(sheet), attrs, kept: template.kept.clone() });
    }
    Ok(())
}

/// What a style's own marker property at `end` resolved to, if it sets one.
fn marker_of(style: &OdfStyle, end: End) -> Option<Marker> {
    let p = if end == End::Begin { P::MarkerStart } else { P::MarkerEnd };
    match style.own_prop(Kind::Graphic, p).map(|prop| &prop.typed) {
        Some(Typed::Marker(m)) => Some(*m),
        _ => None,
    }
}

/// The attributes of `attrs` that `wanted` names (by namespace and local name), as written, in the order written.
fn pairs_of<'t>(attrs: &Attrs<'t>, wanted: &[(Ns, &str)]) -> Vec<(&'t str, &'t str)> {
    attrs.iter().filter(|a| wanted.iter().any(|(ns, local)| a.ns == *ns && a.local == *local)).map(|a| (a.name, a.value)).collect()
}

/// A list of attributes shared by every shape that holds the same one: a list that is short is interned, up to [`super::shapes::MOST_SHARED`] of them, and a repeat costs nothing.
/// A new list is charged (two items for each attribute and its bytes) before it is made.
pub(crate) fn intern(r: &mut Reader, shared: &mut Shared, pairs: &[(&str, &str)]) -> Result<RawAttrs, OdgError> {
    if pairs.is_empty() {
        return Ok(RawAttrs::new());
    }
    let bytes = pairs.iter().map(|(n, v)| n.len() + v.len()).fold(0usize, usize::saturating_add);
    let key = (bytes <= super::shapes::MOST_INTERNED_BYTES).then(|| {
        let mut key = String::with_capacity(bytes + 2 * pairs.len());
        for (n, v) in pairs {
            key.push_str(n);
            key.push('\u{0}');
            key.push_str(v);
            key.push('\u{0}');
        }
        key
    });
    if let Some(found) = key.as_ref().and_then(|k| shared.attr_lists().get(k)) {
        return Ok(found.clone());
    }
    r.take_items(pairs.len().saturating_mul(2).saturating_add(1))?;
    r.take_bytes(bytes)?;
    let made = RawAttrs::from_pairs(pairs.iter().copied());
    if let Some(key) = key {
        shared.keep_attr_list(key, &made);
    }
    Ok(made)
}

/// What a shape's element says that the held values are made of, and what the reader knew while it read the shape.
pub(crate) struct Source<'a, 't> {
    pub kind: Kindof,
    pub el: &'t El,
    pub attrs: &'a Attrs<'t>,
    /// Whether `draw:transform` was read (the 2-D or 1-D frame took it).
    pub transform: bool,
    pub one_d: bool,
    /// Whether the shape's custom geometry element was found (`draw:enhanced-geometry`).
    pub enhanced_geometry: bool,
    /// The element is a frame that shows a text box (its outline is the rectangle's).
    pub frame_text: bool,
    /// The shape's text is empty and its element held a bare `text:p` (no attribute, no content), which the model keeps no text of.
    pub bare_text: bool,
    /// DiaCraft's data carried the shape's `Geometry` sections (tier 2), which are then not what the element's outline gives: **no value that stands beside them is held**.
    pub geometry_carried: bool,
}

/// The values a shape's own element gives (the Frame and Geometry rows), pushed to `out`, charged before they are made; their stamps are those of `sheet`.
pub(crate) fn from_element(r: &mut Reader, shared: &mut Shared, src: &Source<'_, '_>, sheet: &Sheet, out: &mut Vec<Held>) -> Result<(), OdgError> {
    let a = src.attrs;
    let put = |r: &mut Reader,
               shared: &mut Shared,
               out: &mut Vec<Held>,
               value: Value,
               pairs: Vec<(&str, &str)>,
               kept: Passthrough|
     -> Result<(), OdgError> {
        r.take_items(super::cost::HELD)?;
        let attrs = intern(r, shared, &pairs)?;
        out.push(Held { group: value.group(), stamp: value.stamp(sheet), attrs, kept });
        Ok(())
    };
    if let Some(layer) = pairs_non_empty(pairs_of(a, &[(Ns::Draw, "layer")])) {
        put(r, shared, out, Value::Layer, layer, Passthrough::new())?;
    }
    // A 1-D shape's ends as written, with the transform when it has one (every segment, since the model writer judges a connector's route by them).
    if src.one_d {
        let pairs = pairs_of(a, &[(Ns::Svg, "x1"), (Ns::Svg, "y1"), (Ns::Svg, "x2"), (Ns::Svg, "y2"), (Ns::Draw, "transform")]);
        put(r, shared, out, Value::Frame1d, pairs, Passthrough::new())?;
    }
    if src.transform && !src.one_d && src.kind != Kindof::Group {
        let pairs = pairs_of(a, &[(Ns::Svg, "x"), (Ns::Svg, "y"), (Ns::Svg, "width"), (Ns::Svg, "height"), (Ns::Draw, "transform")]);
        put(r, shared, out, Value::Frame2d, pairs, Passthrough::new())?;
    }
    let has = |ns: Ns, local: &str| a.peek(ns, local).is_some();
    let centred = match src.kind {
        Kindof::Circle => has(Ns::Svg, "cx") && has(Ns::Svg, "cy") && has(Ns::Svg, "r"),
        Kindof::Ellipse => has(Ns::Svg, "cx") && has(Ns::Svg, "cy") && has(Ns::Svg, "rx") && has(Ns::Svg, "ry"),
        _ => false,
    };
    if centred {
        let pairs = pairs_of(a, &[(Ns::Svg, "cx"), (Ns::Svg, "cy"), (Ns::Svg, "r"), (Ns::Svg, "rx"), (Ns::Svg, "ry")]);
        put(r, shared, out, Value::CircleCentre, pairs, Passthrough::new())?;
    }
    match src.kind {
        Kindof::Custom if src.enhanced_geometry && !src.geometry_carried => {
            put(r, shared, out, Value::EnhancedGeometry, Vec::new(), Passthrough::new())?
        }
        Kindof::Connector => {
            if let Some(route) =
                pairs_non_empty(pairs_of(a, &[(Ns::Svg, "d"), (Ns::Svg, "viewBox"), (Ns::Draw, "line-skew")])).filter(|_| !src.geometry_carried)
            {
                put(r, shared, out, Value::Route, route, Passthrough::new())?;
            }
            // The type, or its absence: an empty value says the element wrote none.
            put(r, shared, out, Value::ConnectorType, pairs_of(a, &[(Ns::Draw, "type")]), Passthrough::new())?;
        }
        Kindof::Rect | Kindof::Circle | Kindof::Ellipse | Kindof::RegularPolygon => {
            if src.kind == Kindof::Rect
                && let Some(radii) = pairs_non_empty(pairs_of(a, &[(Ns::Draw, "corner-radius"), (Ns::Svg, "rx"), (Ns::Svg, "ry")]))
            {
                put(r, shared, out, Value::CornerRadii, radii, Passthrough::new())?;
            }
        }
        Kindof::Polyline | Kindof::Polygon => {
            if let Some(view_box) = pairs_non_empty(pairs_of(a, &[(Ns::Svg, "viewBox")])) {
                put(r, shared, out, Value::ViewBox, view_box, Passthrough::new())?;
            }
        }
        Kindof::Path => {
            if let Some(path) = pairs_non_empty(pairs_of(a, &[(Ns::Svg, "d"), (Ns::Svg, "viewBox")])).filter(|_| !src.geometry_carried) {
                put(r, shared, out, Value::PathData, path, Passthrough::new())?;
            }
        }
        _ => {}
    }
    // The element the shape was read from: a plain shape's own element and the attributes beside its frame that give its outline (stamped by the `Geometry` sections:
    // a polygon, polyline or path, a rectangle, a circle or ellipse, a regular polygon and a text frame have the outline the element implies), and for any other shape the element alone.
    let plain = if src.geometry_carried {
        None
    } else {
        match src.kind {
            Kindof::Circle | Kindof::Ellipse => Some(pairs_of(a, &[(Ns::Draw, "kind"), (Ns::Draw, "start-angle"), (Ns::Draw, "end-angle")])),
            Kindof::RegularPolygon => Some(pairs_of(a, &[(Ns::Draw, "corners"), (Ns::Draw, "concave"), (Ns::Draw, "sharpness")])),
            Kindof::Rect | Kindof::Polyline | Kindof::Polygon | Kindof::Path => Some(Vec::new()),
            Kindof::Frame if src.frame_text => Some(Vec::new()),
            _ => None,
        }
    };
    match plain {
        Some(outline) => {
            let kept = intern_outline(r, shared, src.el.name.as_str(), &outline, src.bare_text)?;
            put(r, shared, out, Value::PlainOutline, Vec::new(), kept)?;
        }
        None if !matches!(src.kind, Kindof::Group | Kindof::Object(_) | Kindof::Kept(_)) => {
            let kept = intern_outline(r, shared, src.el.name.as_str(), &[], src.bare_text)?;
            put(r, shared, out, Value::Element, Vec::new(), kept)?;
        }
        None => {}
    }
    Ok(())
}

/// The kept element of a plain shape's outline value: an element of the shape's own name holding the attributes that give its outline, shared by every shape that holds the same one
/// (a rectangle's is the same for all of them): charged when it is first made.
fn intern_outline(r: &mut Reader, shared: &mut Shared, name: &str, outline: &[(&str, &str)], bare: bool) -> Result<Passthrough, OdgError> {
    let bytes = outline.iter().map(|(n, v)| n.len() + v.len()).fold(name.len(), usize::saturating_add);
    let mut key = String::from(name);
    if bare {
        key.push('\u{1}');
    }
    if bytes <= super::shapes::MOST_INTERNED_BYTES {
        for (n, v) in outline {
            key.push('\u{0}');
            key.push_str(n);
            key.push('\u{0}');
            key.push_str(v);
        }
        if let Some(found) = shared.kept_list(&key) {
            return Ok(found);
        }
    }
    // A bare `text:p` the element held, which the model keeps no text of, is one child: a writer reads it by its local name.
    r.take_items(3usize.saturating_add(outline.len().saturating_mul(2)).saturating_add(1).saturating_add(if bare { 3 } else { 0 }))?;
    r.take_bytes(bytes)?;
    let children = if bare {
        vec![RawNode::Element(RawElement { name: Arc::from("text:p"), attrs: RawAttrs::new(), children: Vec::new() })]
    } else {
        Vec::new()
    };
    let mut kept = Passthrough::new();
    kept.push(0, RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(outline.iter().copied()), children }));
    if bytes <= super::shapes::MOST_INTERNED_BYTES {
        shared.keep_kept_list(key, &kept);
    }
    Ok(kept)
}

fn pairs_non_empty<T>(pairs: Vec<T>) -> Option<Vec<T>> {
    if pairs.is_empty() { None } else { Some(pairs) }
}

/// Records, for each length attribute of the shape that `cells` name the cell of, the unit it was written in when **no cell spells it** (a `px` length) or the cell that takes the
/// value has another unit (`odfdoc.rs`, "Units"). `pairs` is attribute and cell. Charged before the units are made.
pub(crate) fn units_of(r: &mut Reader, attrs: &Attrs<'_>, sheet: &Sheet, pairs: &[((Ns, &str), &str)]) -> Result<RawAttrs, OdgError> {
    let mut found: Vec<(&str, &str)> = Vec::new();
    for ((ns, local), cell) in pairs {
        let Some(attr) = attrs.iter().find(|a| a.ns == *ns && a.local == *local) else { continue };
        let Some(length) = units::length(attr.value) else { continue };
        let cell_unit = sheet.cells.get(*cell).and_then(|c| c.unit.as_deref());
        if length.unit == units::LenUnit::Px || length.unit.word() != cell_unit {
            // The unit as written is the two letters after the number.
            if let Some(unit) = attr.value.get(attr.value.len().saturating_sub(2)..) {
                found.push((attr.name, unit));
            }
        }
    }
    if found.is_empty() {
        return Ok(RawAttrs::new());
    }
    r.take_items(found.len().saturating_mul(2))?;
    r.take_bytes(found.iter().map(|(n, v)| n.len() + v.len()).fold(0usize, usize::saturating_add))?;
    Ok(RawAttrs::from_pairs(found))
}

/// The slot of a shape's `.odg` data: its kept markup, the automatic style it named, its held values and its units. A shape that holds none of it has no slot; the slot is charged
/// before it is made (what it holds was charged where it was made). Each field is set on its own: what another step put in the slot is left.
pub(crate) fn keep(r: &mut Reader, shape: &mut diacraft_doc::Shape, data: OdfShape) -> Result<(), OdgError> {
    if data == OdfShape::default() {
        return Ok(());
    }
    r.take_items(super::cost::EXT_SHAPE)?;
    let slot = &mut shape.ext.make_mut().odf;
    slot.kept = data.kept;
    slot.style_name = data.style_name;
    slot.held = data.held;
    slot.units = data.units;
    Ok(())
}

/// The name an attribute of `attrs` gives an automatic style, if it names one: shared by [`name_of`].
pub(crate) fn named(r: &mut Reader, shared: &mut Shared, attrs: &Attrs<'_>, ns: Ns, local: &str) -> Result<Option<Arc<str>>, OdgError> {
    attrs.peek(ns, local).map(|name| name_of(r, shared, name)).transpose()
}

/// The name of an automatic style a shape named, shared by every shape that names it (a name of at most [`super::shapes::MOST_INTERNED_BYTES`] bytes is interned, up to
/// [`super::shapes::MOST_SHARED`] of them): charged by its bytes when it is first made.
pub(crate) fn name_of(r: &mut Reader, shared: &mut Shared, name: &str) -> Result<Arc<str>, OdgError> {
    if let Some(found) = shared.style_name(name) {
        return Ok(found);
    }
    r.take_bytes(name.len())?;
    let made: Arc<str> = Arc::from(name);
    shared.keep_style_name(&made);
    Ok(made)
}

// ---------------------------------------------------------------------------------------------------------------------------------------- text

/// A held value of a paragraph's or a run's automatic style, made once when the style is read: the attributes as written, and the group of cells of the row that its keeping rests on.
#[derive(Clone)]
pub(crate) struct TextTemplate {
    pub group: TextGroup,
    pub attrs: RawAttrs,
}

/// The values the `style:paragraph-properties` of a style give (the alignment spelled `start` or `end`, the line spacing) and its `style:text-properties` (an underline's colour and
/// width and a style other than `solid`, the same for the line-through and the overline, a lowercase transform, a percentage position), for the element it is given: an automatic
/// **paragraph** style gives its paragraph properties' (held for each paragraph that names it), an automatic **text** style its text properties' (held for each run), and a
/// **common** style of a style sheet both (held in the style's own slot, [`for_sheet`]). Charged before they are made.
pub(crate) fn text_templates<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    paragraph: Option<&'t El>,
    text: Option<&'t El>,
) -> Result<Vec<TextTemplate>, OdgError> {
    let found = found_text(names, paragraph, text);
    let mut items = 0usize;
    let mut bytes = 0usize;
    for (_, pairs) in &found {
        items = items.saturating_add(1).saturating_add(pairs.len().saturating_mul(2));
        bytes = bytes.saturating_add(pairs.iter().map(|(n, v)| n.len() + v.len()).fold(0usize, usize::saturating_add));
    }
    r.take_items(items)?;
    r.take_bytes(bytes)?;
    Ok(found.into_iter().map(|(group, pairs)| TextTemplate { group, attrs: RawAttrs::from_pairs(pairs) }).collect())
}

/// The attributes of `paragraph` and `text` (the properties elements of a style) that give a value of a group, by group, as written and borrowed from the tree: what
/// [`text_templates`] makes templates of, and what an automatic graphic or paragraph style holds in its remainder ([`super::styles`]).
pub(crate) fn found_text<'t>(names: &mut Names<'t>, paragraph: Option<&'t El>, text: Option<&'t El>) -> Vec<FoundText<'t>> {
    let mut found: Vec<FoundText<'t>> = Vec::new();
    let mut walk = |names: &mut Names<'t>, el: Option<&'t El>, groups: &dyn Fn(&Attrs<'t>, &mut Vec<FoundText<'t>>)| {
        if let Some(el) = el {
            let mark = names.enter(el);
            let attrs = names.attrs(el);
            groups(&attrs, &mut found);
            names.leave(mark);
        }
    };
    walk(names, paragraph, &|attrs, found| {
        let align: Vec<(&str, &str)> = attrs
            .iter()
            .filter(|a| a.ns == Ns::Fo && matches!(a.local, "text-align" | "text-align-last") && matches!(a.value, "start" | "end"))
            .map(|a| (a.name, a.value))
            .collect();
        let spacing: Vec<(&str, &str)> = attrs
            .iter()
            .filter(|a| a.ns == Ns::Style && matches!(a.local, "line-spacing" | "line-height-at-least"))
            .map(|a| (a.name, a.value))
            .collect();
        for (group, pairs) in [(TextGroup::HorzAlign, align), (TextGroup::SpLine, spacing)] {
            if !pairs.is_empty() {
                found.push((group, pairs));
            }
        }
    });
    walk(names, text, &|attrs, found| {
        let line = |word: &str| -> Vec<(&str, &str)> {
            // The type the cells write is `single` or `double` for a line that is on and none for one that is off: a type beside a line that is off, or one of another kind, is the file's
            // (`style:text-line-through-type="none"` beside a style of `none` was lost on a copy).
            let on = attrs.peek(Ns::Style, &format!("text-{word}-style")).is_some_and(|v| v != "none");
            attrs
                .iter()
                .filter(|a| {
                    a.ns == Ns::Style
                        && (a.local == format!("text-{word}-color")
                            || a.local == format!("text-{word}-width")
                            || (a.local == format!("text-{word}-style") && a.value != "solid")
                            || (a.local == format!("text-{word}-type") && (!on || !matches!(a.value, "single" | "double"))))
                })
                .map(|a| (a.name, a.value))
                .collect()
        };
        // The family a face's cell holds is written as the face's name (`style:font-name`), and a family no declared face has is not written at all: the family as the file spelled it
        // is the file's, held beside the cell (`fo:font-family` beside `style:font-name` was lost on every copy, and a family alone with it).
        let family = |local: &str| -> Vec<(&str, &str)> {
            attrs
                .iter()
                .filter(|a| a.ns == if local == "font-family" { Ns::Fo } else { Ns::Style } && a.local == local)
                .map(|a| (a.name, a.value))
                .collect()
        };
        let case: Vec<(&str, &str)> =
            attrs.iter().filter(|a| a.ns == Ns::Fo && a.local == "text-transform" && a.value == "lowercase").map(|a| (a.name, a.value)).collect();
        let position: Vec<(&str, &str)> =
            attrs.iter().filter(|a| a.ns == Ns::Style && a.local == "text-position" && a.value.contains('%')).map(|a| (a.name, a.value)).collect();
        for (group, pairs) in [
            (TextGroup::Underline, line("underline")),
            (TextGroup::Strike, line("line-through")),
            (TextGroup::Overline, line("overline")),
            (TextGroup::Case, case),
            (TextGroup::Pos, position),
            (TextGroup::Font, family("font-family")),
            (TextGroup::AsianFont, family("font-family-asian")),
            (TextGroup::ComplexScriptFont, family("font-family-complex")),
        ] {
            if !pairs.is_empty() {
                found.push((group, pairs));
            }
        }
    });
    found
}

/// The values a **common** style of a style sheet gives that its cells only approximate, held in the style's own slot as a shape holds the values its own
/// automatic style gives it: the style's [`Template`]s as [`Held`] values stamped by the cells of the style sheet (`sheet`, as the reader set them), and its text templates as
/// [`HeldStyleValue`]s stamped by row 0 of the sheet's `Paragraph` or `Character` section. Each is charged ([`super::cost::HELD`]) before it is made.
pub(crate) fn for_sheet(r: &mut Reader, set: &StyleSet, at: usize, sheet: &Sheet, odf: &mut diacraft_doc::OdfStyle) -> Result<(), OdgError> {
    let Some(style) = set.get(at) else { return Ok(()) };
    from_style(r, set, Some(at), sheet, &mut odf.held)?;
    let row_of = |section: &str| -> Row {
        sheet.section(&SectionKey::new(section, None)).and_then(|s| s.row(&RowKey::Ix(0))).map_or_else(Row::default, |row| (**row).clone())
    };
    let (paragraph, run) =
        if style.text_templates.is_empty() { (Row::default(), Row::default()) } else { (row_of("Paragraph"), row_of("Character")) };
    r.take_items(style.text_templates.len().saturating_mul(super::cost::HELD))?;
    for template in &style.text_templates {
        let row = if template.group.is_paragraph() { &paragraph } else { &run };
        odf.text_held.push(HeldStyleValue {
            group: template.group,
            stamp: Some(stamp_text_group(row, template.group)),
            attrs: template.attrs.clone(),
            kept: Passthrough::new(),
        });
    }
    odf.held.shrink_to_fit();
    odf.text_held.shrink_to_fit();
    Ok(())
}

/// The held style of a paragraph or a run: the name it named (`None` is held too), the stamp of its row, and the values its automatic style gives, each stamped by its group of the
/// row, and `spelled`, the values of what the style stated that the cells say otherwise ([`super::spelled`]). A `row` that the sheet does not hold (a row with no cell of
/// its own is no row) stamps as an empty one. Charged by the caller.
fn text_style(set: &StyleSet, style: Option<usize>, name: Option<std::sync::Arc<str>>, row: &Row, spelled: Vec<HeldStyleValue>) -> OdfTextStyle {
    let mut held = style
        .and_then(|at| set.get(at))
        .map(|s| {
            s.text_templates
                .iter()
                .map(|t| HeldStyleValue {
                    group: t.group,
                    stamp: Some(stamp_text_group(row, t.group)),
                    attrs: t.attrs.clone(),
                    kept: Passthrough::new(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    held.extend(spelled);
    OdfTextStyle { name, stamp: Some(stamp_text_row(row)), held }
}

/// The `.odg` data of a shape's text (`Text.ext.odf`): one paragraph for each `text:p` of the file, in its order, with the style it named and its held values, and one run for
/// each `text:span`. Charged before it is made; the rows are those of `sheet` as the reader set them. A text with no paragraph has none.
pub(crate) fn text_odf(r: &mut Reader, set: &StyleSet, sheet: &Sheet, paras: &[ParaOdf]) -> Result<Option<OdfText>, OdgError> {
    if paras.is_empty() {
        return Ok(None);
    }
    // The slot, and one item for each held value (a record is 40 bytes). The paragraphs and the runs were charged when they were read, which is when they were first held.
    let mut items = super::cost::EXT_TEXT;
    for para in paras {
        items = items.saturating_add(held_count(set, para.style));
        for span in &para.spans {
            items = items.saturating_add(held_count(set, span.style));
        }
    }
    r.take_items(items)?;
    let empty = Row::default();
    let row_of = |section: &str, ix: u32| -> &Row {
        sheet.section(&SectionKey::new(section, None)).and_then(|s| s.row(&RowKey::Ix(ix))).map_or(&empty, |row| &**row)
    };
    let mut spelling = super::spelled::Spelling::default();
    let mut paragraphs = Vec::with_capacity(paras.len());
    for para in paras {
        let mut runs = Vec::with_capacity(para.spans.len());
        for span in &para.spans {
            // A run that is not a `text:span` names no style.
            let style = if span.span {
                let row = span.row.map_or(&empty, |ix| row_of("Character", ix));
                let spelled = match span.style {
                    Some(at) => {
                        let values = spelling.run(r, set, at, span.row, row)?;
                        let (count, values) = (values.len(), values.to_vec());
                        r.take_items(count)?;
                        values
                    }
                    None => Vec::new(),
                };
                text_style(set, span.style, span.name.clone(), row, spelled)
            } else {
                OdfTextStyle::default()
            };
            runs.push(OdfRun {
                from: u32::try_from(span.from).unwrap_or(u32::MAX),
                to: u32::try_from(span.to).unwrap_or(u32::MAX),
                style,
                element: span.element.clone(),
            });
        }
        let own = para.own_row.map(|ix| (ix, row_of("Character", ix)));
        let spelled = {
            let values = spelling.paragraph(r, set, para.style, own)?;
            let (count, values) = (values.len(), values.to_vec());
            r.take_items(count)?;
            values
        };
        paragraphs.push(OdfParagraph {
            style: text_style(set, para.style, para.name.clone(), row_of("Paragraph", para.row), spelled),
            characters: Some(para.characters),
            runs,
            element: para.element.clone(),
            levels: para.levels.clone(),
        });
    }
    Ok(Some(OdfText { kept: diacraft_doc::Kept::default(), paragraphs }))
}

/// How many held values the style at `style` gives a paragraph or a run by its templates (the values of what it stated that the cells say otherwise are charged where they are made,
/// [`super::spelled::Spelling`]).
fn held_count(set: &StyleSet, style: Option<usize>) -> usize {
    style.and_then(|at| set.get(at)).map_or(0, |s| s.text_templates.len())
}

/// What the reader knew of a shape while it read it, for its `.odg` data.
pub(crate) struct Facts<'a, 't> {
    pub kind: Kindof,
    pub el: &'t El,
    pub attrs: &'a Attrs<'t>,
    /// The shape's own automatic graphic style.
    pub graphic: Option<usize>,
    /// Whether `draw:transform` was read.
    pub transform: bool,
    pub one_d: bool,
    /// The automatic style the shape named, as written.
    pub style_name: Option<Arc<str>>,
    /// The attributes and children of the element that no field reads.
    pub kept: diacraft_doc::Kept,
    /// What each paragraph of the shape's text named.
    pub paragraphs: Vec<ParaOdf>,
    /// The element is a frame that shows a text box.
    pub frame_text: bool,
    /// The text is empty and its element held a bare `text:p`.
    pub bare_text: bool,
    /// DiaCraft's data carried the shape's `Geometry` sections.
    pub geometry_carried: bool,
}

/// The `.odg` data of a shape, made from what the reader knew of it and put in its slots (`Shape.ext`, and the slot of its text): the values its style and its element hold beside their
/// stand-in cells with the stamps of `sheet` (the cells as they are after tier 2), the units no cell spells, the style name, and the paragraphs of its text. Charged before it is made.
pub(crate) fn shape_data<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    set: &StyleSet,
    shared: &mut Shared,
    shape: &mut diacraft_doc::Shape,
    sheet: &Sheet,
    facts: Facts<'_, 't>,
) -> Result<(), OdgError> {
    let Facts { kind, el, attrs, graphic, transform, one_d, style_name, kept, paragraphs, frame_text, bare_text, geometry_carried } = facts;
    let mut held_values = Vec::new();
    from_style(r, set, graphic, sheet, &mut held_values)?;
    let enhanced_geometry = kind == Kindof::Custom
        && el.elements().any(|c| {
            let mark = names.enter(c);
            let is = names.is(c, Ns::Draw, "enhanced-geometry");
            names.leave(mark);
            is
        });
    let source = Source { kind, el, attrs, transform, one_d, enhanced_geometry, frame_text, bare_text, geometry_carried };
    from_element(r, shared, &source, sheet, &mut held_values)?;
    held_values.shrink_to_fit();
    let frame: &[((Ns, &str), &str)] = if one_d {
        &[((Ns::Svg, "x1"), "BeginX"), ((Ns::Svg, "y1"), "BeginY"), ((Ns::Svg, "x2"), "EndX"), ((Ns::Svg, "y2"), "EndY")]
    } else if kind == Kindof::Group {
        &[]
    } else {
        &[((Ns::Svg, "x"), "PinX"), ((Ns::Svg, "y"), "PinY"), ((Ns::Svg, "width"), "Width"), ((Ns::Svg, "height"), "Height")]
    };
    let units = units_of(r, attrs, sheet, frame)?;
    if let Some(odf_text) = text_odf(r, set, sheet, &paragraphs)?
        && let Some(text) = shape.text.as_mut()
    {
        Arc::make_mut(text).ext.make_mut().odf = odf_text;
    }
    keep(r, shape, OdfShape { kept, style_name, held: held_values, units })
}
