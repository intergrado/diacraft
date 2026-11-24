//! The styles of a drawing as ODF holds them: every `style:style` and `style:default-style` of the
//! families the model maps (`graphic`, `paragraph`, `text`), by family and name, with the properties of each that the model has a cell for,
//! and the common elements those properties name (dashes, markers, gradients, list styles, font faces), the page layouts, and the layer
//! set. What the model has no cell for is counted (`kept`), and its bytes stay in the file.
//!
//! **Lookup**: a reference from `content.xml` is looked for in its automatic styles and then in `office:styles`; one from `styles.xml`
//! in its own automatic styles and then in `office:styles`. A parent (`style:parent-style-name`) is looked for in `office:styles`, and then,
//! breaking the standard, in the automatic styles of the style's own file, with a warning. A name that finds no style resolves as if
//! absent, with one warning per name.
//!
//! **Chains** ([`StyleSet::chain`]): a style, its parent, the parent's parent, ..., at most [`MAX_CHAIN`] steps and never round a cycle,
//! which ends the chain where it would repeat, with a warning. Each step is counted.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use diacraft_container::xml::El;
use diacraft_doc::{Kept, RawAttrs, RawElement, RawNode};

use super::Reader;
use super::names::{Names, Ns};
use super::typed::{Marker, Name, Prop, Size, Typed};
use super::units::{self, Length};
use crate::error::OdgError;

/// Most styles a chain holds: the start and 64 parents (as for VSDX styles).
pub(crate) const MAX_CHAIN: usize = 65;

/// The style families the reader maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Family {
    Graphic,
    Paragraph,
    Text,
}

impl Family {
    fn of(word: &str) -> Option<Family> {
        match word {
            "graphic" => Some(Family::Graphic),
            "paragraph" => Some(Family::Paragraph),
            "text" => Some(Family::Text),
            _ => None,
        }
    }
}

/// Which part a style is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum File {
    Styles,
    Content,
}

/// Which properties element of a style a property is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Graphic,
    Paragraph,
    Text,
}

impl Kind {
    fn at(self) -> usize {
        match self {
            Kind::Graphic => 0,
            Kind::Paragraph => 1,
            Kind::Text => 2,
        }
    }
}

/// A property the model has a cell for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum P {
    // `style:graphic-properties`: fill, line, shadow, text area.
    Fill,
    FillColor,
    Opacity,
    FillGradientName,
    FillHatchSolid,
    Stroke,
    StrokeDash,
    StrokeWidth,
    StrokeColor,
    StrokeOpacity,
    StrokeLinecap,
    MarkerStart,
    MarkerEnd,
    MarkerStartWidth,
    MarkerEndWidth,
    Shadow,
    ShadowColor,
    ShadowOpacity,
    ShadowOffsetX,
    ShadowOffsetY,
    TextareaVerticalAlign,
    Padding,
    PaddingTop,
    PaddingBottom,
    PaddingLeft,
    PaddingRight,
    WritingMode,
    // `style:paragraph-properties`.
    TextAlign,
    TextAlignLast,
    MarginLeft,
    MarginRight,
    TextIndent,
    MarginTop,
    MarginBottom,
    LineHeight,
    TabStopDistance,
    // `style:text-properties`.
    FontSize,
    FontWeight,
    FontWeightComplex,
    FontStyle,
    FontStyleComplex,
    UnderlineStyle,
    UnderlineType,
    LineThroughStyle,
    LineThroughType,
    OverlineStyle,
    FontVariant,
    TextTransform,
    TextPosition,
    Color,
    LetterSpacing,
    TextScale,
    FontName,
    FontFamily,
    FontNameAsian,
    FontFamilyAsian,
    FontNameComplex,
    FontFamilyComplex,
}

/// The properties of `style:graphic-properties` the model maps, by namespace and name.
const GRAPHIC: &[(Ns, &str, P)] = &[
    (Ns::Draw, "fill", P::Fill),
    (Ns::Draw, "fill-color", P::FillColor),
    (Ns::Draw, "opacity", P::Opacity),
    (Ns::Draw, "fill-gradient-name", P::FillGradientName),
    (Ns::Draw, "fill-hatch-solid", P::FillHatchSolid),
    (Ns::Draw, "stroke", P::Stroke),
    (Ns::Draw, "stroke-dash", P::StrokeDash),
    (Ns::Svg, "stroke-width", P::StrokeWidth),
    (Ns::Svg, "stroke-color", P::StrokeColor),
    (Ns::Svg, "stroke-opacity", P::StrokeOpacity),
    (Ns::Svg, "stroke-linecap", P::StrokeLinecap),
    (Ns::Draw, "marker-start", P::MarkerStart),
    (Ns::Draw, "marker-end", P::MarkerEnd),
    (Ns::Draw, "marker-start-width", P::MarkerStartWidth),
    (Ns::Draw, "marker-end-width", P::MarkerEndWidth),
    (Ns::Draw, "shadow", P::Shadow),
    (Ns::Draw, "shadow-color", P::ShadowColor),
    (Ns::Draw, "shadow-opacity", P::ShadowOpacity),
    (Ns::Draw, "shadow-offset-x", P::ShadowOffsetX),
    (Ns::Draw, "shadow-offset-y", P::ShadowOffsetY),
    (Ns::Draw, "textarea-vertical-align", P::TextareaVerticalAlign),
    (Ns::Fo, "padding", P::Padding),
    (Ns::Fo, "padding-top", P::PaddingTop),
    (Ns::Fo, "padding-bottom", P::PaddingBottom),
    (Ns::Fo, "padding-left", P::PaddingLeft),
    (Ns::Fo, "padding-right", P::PaddingRight),
    (Ns::Style, "writing-mode", P::WritingMode),
];

/// The properties of `style:paragraph-properties` the model maps.
const PARAGRAPH: &[(Ns, &str, P)] = &[
    (Ns::Fo, "text-align", P::TextAlign),
    (Ns::Fo, "text-align-last", P::TextAlignLast),
    (Ns::Style, "writing-mode", P::WritingMode),
    (Ns::Fo, "margin-left", P::MarginLeft),
    (Ns::Fo, "margin-right", P::MarginRight),
    (Ns::Fo, "text-indent", P::TextIndent),
    (Ns::Fo, "margin-top", P::MarginTop),
    (Ns::Fo, "margin-bottom", P::MarginBottom),
    (Ns::Fo, "line-height", P::LineHeight),
    (Ns::Style, "tab-stop-distance", P::TabStopDistance),
];

/// The properties of `style:text-properties` the model maps.
const TEXT: &[(Ns, &str, P)] = &[
    (Ns::Fo, "font-size", P::FontSize),
    (Ns::Fo, "font-weight", P::FontWeight),
    (Ns::Style, "font-weight-complex", P::FontWeightComplex),
    (Ns::Fo, "font-style", P::FontStyle),
    (Ns::Style, "font-style-complex", P::FontStyleComplex),
    (Ns::Style, "text-underline-style", P::UnderlineStyle),
    (Ns::Style, "text-underline-type", P::UnderlineType),
    (Ns::Style, "text-line-through-style", P::LineThroughStyle),
    (Ns::Style, "text-line-through-type", P::LineThroughType),
    (Ns::Style, "text-overline-style", P::OverlineStyle),
    (Ns::Fo, "font-variant", P::FontVariant),
    (Ns::Fo, "text-transform", P::TextTransform),
    (Ns::Style, "text-position", P::TextPosition),
    (Ns::Fo, "color", P::Color),
    (Ns::Fo, "letter-spacing", P::LetterSpacing),
    (Ns::Style, "text-scale", P::TextScale),
    (Ns::Style, "font-name", P::FontName),
    (Ns::Fo, "font-family", P::FontFamily),
    (Ns::Style, "font-name-asian", P::FontNameAsian),
    (Ns::Style, "font-family-asian", P::FontFamilyAsian),
    (Ns::Style, "font-name-complex", P::FontNameComplex),
    (Ns::Style, "font-family-complex", P::FontFamilyComplex),
];

/// A tab stop of `style:tab-stops`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tab {
    pub position: Length,
    /// `Alignment`: 0 left, 1 centre, 2 right, 3 a character.
    pub alignment: u8,
}

/// What DiaCraft wrote on a style's `style:graphic-properties`, matched by namespace.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Dia {
    pub version: Option<String>,
    /// `diacraft:id`, when it reads as a number.
    pub id: Option<u32>,
    /// `diacraft:links`: the three `style:name` values, line, fill, text, until [`StyleSet::resolve`] looks them up and lets them go.
    pub links: Option<[String; 3]>,
    /// What the links found, once looked up: `None` when the style states none, and for one that does, the three common styles they name
    /// (`None` when any of them names no common style).
    pub linked: Option<Option<[usize; 3]>>,
    /// The attributes of the extension the writer owns (`ext::names::owned`) but `id` and `links`, as their local names and values: a shape's cells,
    /// sections, text and names, which `ext::payload::decode` reads when a shape that is not a copy names the style.
    pub tier2: Vec<(String, String)>,
    /// `diacraft:glue="own"`: the rows of the `Connection` section of a custom shape that names the style start at 0 with its
    /// first listed glue point.
    pub glue_own: bool,
}

/// A style of a family the reader maps.
pub(crate) struct OdfStyle {
    pub name: String,
    pub display: Option<String>,
    pub family: Family,
    pub parent: Option<String>,
    /// A common style (`office:styles`), not an automatic one.
    pub common: bool,
    /// A `style:default-style`.
    pub default: bool,
    pub file: File,
    /// The mapped properties of its three properties elements, in the order written.
    props: [Vec<(P, Prop)>; 3],
    pub tabs: Option<Rc<[Tab]>>,
    pub dia: Dia,
    /// The `ID` of the style sheet made of it (common styles only).
    pub sheet: Option<u32>,
    /// What the `.odg` holds of a style sheet's style that no cell reads, for the sheet's own slot (`Style.ext.odf`): its names, the attributes of its properties that no cell
    /// reads and the units of the lengths no cell spells. `None` for a style that is no style sheet.
    pub odf: Option<Box<diacraft_doc::OdfStyle>>,
    /// The held values a shape's own automatic graphic style gives, made once for every shape that names the style (`held.rs`); empty for any other style.
    pub templates: Vec<super::held::Template>,
    /// The held values an automatic paragraph or text style gives each paragraph or run that names it (`held.rs`); empty for any other style.
    pub text_templates: Vec<super::held::TextTemplate>,
}

impl OdfStyle {
    /// The value this style itself gives `p` on its `kind` properties.
    pub(crate) fn own(&self, kind: Kind, p: P) -> Option<&str> {
        self.own_prop(kind, p).map(|v| v.text.as_str())
    }

    /// The property this style itself sets as `p` on its `kind` properties: its text and what it reads as.
    pub(crate) fn own_prop(&self, kind: Kind, p: P) -> Option<&Prop> {
        self.props.get(kind.at()).and_then(|list| list.iter().find(|(q, _)| *q == p)).map(|(_, v)| v)
    }

    /// The properties this style sets on `kind`, in the order written.
    pub(crate) fn props(&self, kind: Kind) -> &[(P, Prop)] {
        self.props.get(kind.at()).map_or(&[], Vec::as_slice)
    }
}

/// A length or a share of the stroke width: a run of a dash.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Run {
    Percent(f64),
    Inches(f64),
}

/// A `draw:stroke-dash`. Each field is `None` when the attribute is absent or unreadable.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Dash {
    /// The pattern and cap form its name says, when its name is one DiaCraft writes (`props::own_dash`), worked out once when it is read.
    pub own: Option<(u8, bool)>,
    pub dots1: Option<u32>,
    pub dots1_length: Option<Run>,
    pub dots2: Option<u32>,
    pub dots2_length: Option<Run>,
    pub distance: Option<Run>,
}

/// A `draw:gradient`, as the cells that stand in for it: made once, when the definition is read (`gradient.rs`).
pub(crate) type Gradient = super::gradient::StandIn;

/// A level of a `text:list-style`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListLevel {
    pub level: u32,
    /// The bullet character, for a bullet level; `None` for a numbered or image level.
    pub bullet: Option<String>,
    pub numbered: bool,
    pub font_name: Option<String>,
    /// The family, cut of white space and quotes at its ends when the level is read, so that no run or list item cuts it again.
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    /// What `font_size` reads as, when it is a length: read once, with the level.
    pub font_size_length: Option<Length>,
    pub margin_left: Option<Length>,
    pub text_indent: Option<Length>,
}

/// The level numbered `number` in a list style's `levels`, which are sorted by number (`StyleSet::add_list_style`): the first written of
/// that number. A binary search, so that a list style of any number of levels costs a list the logarithm of them and not all of them; the
/// most levels it can visit (`levels.len()` has this many bits) are counted before it searches.
pub(crate) fn list_level<'s>(r: &mut Reader, levels: &'s [ListLevel], number: u32) -> Option<&'s ListLevel> {
    r.list_levels(levels.len().checked_ilog2().map_or(0, |bits| u64::from(bits) + 1));
    let at = levels.partition_point(|l| l.level < number);
    levels.get(at).filter(|l| l.level == number)
}

/// A page layout's properties.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PageLayout {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub margins: [Option<Length>; 4],
    pub orientation: Option<String>,
    /// The attributes of `style:page-layout-properties` that no page cell reads, as written: the page holds them as `layout_props`.
    pub unmapped: RawAttrs,
    /// The lengths of the layout that no cell spells in the unit they were written in (`px`), by attribute name, as written.
    pub units: RawAttrs,
}

/// Every style and common element of a drawing.
#[derive(Default)]
pub(crate) struct StyleSet {
    pub list: Vec<OdfStyle>,
    common: HashMap<(Family, String), usize>,
    auto_styles: HashMap<(Family, String), usize>,
    auto_content: HashMap<(Family, String), usize>,
    defaults: HashMap<Family, usize>,
    pub page_layouts: HashMap<String, PageLayout>,
    /// The dashes and gradients in the order read, and the index of each by name (the first of a name wins): a style finds one by name
    /// once, when its part is read ([`StyleSet::resolve`]), and keeps the index.
    dash_list: Vec<Dash>,
    dashes: HashMap<String, usize>,
    markers: HashSet<String>,
    gradient_list: Vec<Gradient>,
    gradients: HashMap<String, usize>,
    /// The most rows of `FillGradient` a gradient of the drawing stands in with: a sheet that stands in with fewer deletes the rows past its own up to this many, so that
    /// a style under it with more lends it none.
    most_stop_rows: usize,
    lists_styles: HashMap<String, Vec<ListLevel>>,
    lists_content: HashMap<String, Vec<ListLevel>>,
    fonts_styles: HashMap<String, String>,
    fonts_content: HashMap<String, String>,
    /// `draw:layer-set` of `office:master-styles`.
    pub layers: Option<Vec<super::layers::Layer>>,
    /// The document's own ODF data as it is gathered: the definitions, the layer set and the style elements kept whole (`odfdoc.rs`).
    pub doc: super::odfdoc::DocData,
}

/// What the walk of the properties elements of a **style sheet's** style collects for the style's own slot: what no cell reads, and the lengths that no
/// cell spells in the unit they were written in. Borrowed from the tree, and charged before any of it is copied ([`odf_style`]).
#[derive(Default)]
struct Collect<'t> {
    /// The properties element's name, the attribute's name and its value, of each attribute no cell reads (DiaCraft's own, which its writer writes again, are not among them).
    unmapped: Vec<(&'t str, &'t str, &'t str)>,
    /// The names of the mapped length attributes written in `px`, which no cell has a unit for.
    px: Vec<&'t str>,
    /// The children of a properties element that nothing reads: the properties element and the child.
    children: Vec<(&'t El, &'t El)>,
    /// The declarations the attributes of `unmapped` need that the model does not hold, for the properties element of the name: its name, the prefix and the namespace. An
    /// attribute whose prefix the properties element or the style declared on its own start tag is kept with the declaration, which only that start tag held.
    declared: Vec<(&'t str, &'t str, &'t str)>,
}

impl<'t> Collect<'t> {
    /// Collects the attribute `attribute` of the properties element named `holder` that no cell reads, with the declaration of its prefix when the model does not hold it.
    fn unmapped(&mut self, names: &Names<'t>, holder: &'t str, attribute: &'t str, value: &'t str) {
        self.unmapped.push((holder, attribute, value));
        let Some((prefix, _)) = attribute.split_once(':') else { return };
        if prefix == "xml" || prefix == "xmlns" || self.declared.iter().any(|(h, p, _)| *h == holder && *p == prefix) {
            return;
        }
        if let Some(uri) = names.local_binding(prefix)
            && crate::write::kept::declaration_ok(prefix, uri)
        {
            self.declared.push((holder, prefix, uri));
        }
    }
}

/// Whether a value read as a length was written in `px`.
fn is_px(typed: &Typed) -> bool {
    use super::typed::{LineHeight, Size, Spacing};
    let length = match typed {
        Typed::Length(l) => *l,
        Typed::Size(Size::Length(l)) | Typed::LineHeight(LineHeight::Length(l)) | Typed::Spacing(Spacing::Length(l)) => Some(*l),
        _ => None,
    };
    length.is_some_and(|l| l.unit == units::LenUnit::Px)
}

/// Whether the value a mapped property was written with is one the conversion to cells reads nothing from (the standard does not define it): a length, a
/// percentage, a colour or a share that does not read, a keyword the property does not take. **Such an attribute is held with the ones no cell reads**, since
/// the cell of a property it does not set has nothing to say of it. The keywords are those `props.rs` reads (`property_value` there counts the same ones).
fn unread_value(p: P, prop: &Prop) -> bool {
    use super::typed::{LineHeight, Spacing};
    match &prop.typed {
        Typed::Length(None)
        | Typed::Percent(None)
        | Typed::Colour(None)
        | Typed::Share(None)
        | Typed::LineHeight(LineHeight::Other)
        | Typed::Size(Size::Other)
        | Typed::Spacing(Spacing::Other)
        | Typed::Position { pos: None, .. } => true,
        Typed::Word => {
            let text = prop.text.as_str();
            match p {
                P::Fill => !matches!(text, "none" | "solid" | "gradient" | "hatch" | "bitmap"),
                P::Stroke => !matches!(text, "none" | "solid" | "dash"),
                P::StrokeLinecap => !matches!(text, "round" | "butt" | "square"),
                P::Shadow => !matches!(text, "visible" | "hidden"),
                P::TextareaVerticalAlign => !matches!(text, "top" | "middle" | "bottom" | "justify"),
                P::TextAlign => !matches!(text, "left" | "start" | "center" | "right" | "end" | "justify"),
                P::TextTransform => !matches!(text, "uppercase" | "capitalize" | "none" | "lowercase"),
                P::FillHatchSolid => units::boolean(text).is_none(),
                P::FontWeight | P::FontWeightComplex => super::props::bold(text).is_none(),
                _ => false,
            }
        }
        _ => false,
    }
}

/// One step of the walk of a properties element: the attributes the reader maps are kept, the rest counted (and, for a style sheet's style, collected).
fn props_of<'t>(
    r: &mut Reader,
    names: &Names<'t>,
    el: &'t El,
    table: &[(Ns, &str, P)],
    dia: Option<&mut Dia>,
    mut collect: Option<&mut Collect<'t>>,
) -> Vec<(P, Prop)> {
    let attrs = names.attrs(el);
    r.saw_attrs(attrs.len());
    let mut out = Vec::new();
    let mut dia = dia;
    for a in attrs.iter() {
        if let Some((_, _, p)) = table.iter().find(|(ns, local, _)| *ns == a.ns && *local == a.local) {
            if !out.iter().any(|(q, _)| q == p) {
                let prop = Prop::read(r, *p, a.value);
                if let Some(c) = collect.as_deref_mut() {
                    if is_px(&prop.typed) {
                        c.px.push(a.name);
                    }
                    // A value no cell can show is held as written, with the attributes no cell reads.
                    if unread_value(*p, &prop) {
                        c.unmapped(names, el.name.as_str(), a.name, a.value);
                    }
                }
                out.push((*p, prop));
            } else if let Some(c) = collect.as_deref_mut() {
                // A second attribute of a mapped name: the first is the one read, so the repeat is held with the attributes no cell reads.
                c.unmapped(names, el.name.as_str(), a.name, a.value);
            }
        } else if a.ns == Ns::Diacraft
            && let Some((original, uri)) = crate::ext::odf14::carried_on_properties(el.name.rsplit(':').next().unwrap_or_default(), a.local)
            && names.uri_of(original) == Some(uri)
            && let Some(c) = collect.as_deref_mut()
        {
            // The form of an earlier version that this file's writer carried in an attribute of its own (`text:enable-numbering`, which ODF 1.2 and 1.4 have no place for): read back as the
            // attribute it stood for, so that the style is the one that was read from the file written first.
            r.keep("property");
            c.unmapped(names, el.name.as_str(), original, a.value);
        } else if a.ns == Ns::Diacraft {
            match (a.local, dia.as_deref_mut()) {
                ("version", Some(d)) => d.version = Some(a.value.to_owned()),
                ("id", Some(d)) => d.id = units::integer(a.value).and_then(|n| u32::try_from(n).ok()),
                ("links", Some(d)) => {
                    let parts: Vec<&str> = a.value.split_ascii_whitespace().collect();
                    if let [l, f, t] = parts.as_slice() {
                        d.links = Some([(*l).to_owned(), (*f).to_owned(), (*t).to_owned()]);
                    }
                }
                ("glue", Some(d)) if a.value == "own" => d.glue_own = true,
                (local, Some(d)) if crate::ext::names::owned(local) => {
                    if d.tier2.len() < crate::ext::payload::MOST_ATTRIBUTES {
                        d.tier2.push((local.to_owned(), a.value.to_owned()));
                    } else {
                        r.keep("diacraft_attribute");
                    }
                }
                _ => r.keep("diacraft_attribute"),
            }
        } else {
            r.keep("property");
            if let Some(c) = collect.as_deref_mut() {
                c.unmapped(names, el.name.as_str(), a.name, a.value);
            }
        }
    }
    out
}

/// What an **automatic** style of the graphic, paragraph or text family holds that no cell reads, as an element: the style's own attributes, then, for each
/// properties element that holds an attribute no cell reads or a child nothing reads, an element of its name holding only those, then the style's children that are no properties
/// element. The attributes a cell reads are in the cells, and the ones the held values keep (a hatch's name, a paragraph's alignment spelled `start`) are in the shapes'
/// and the paragraphs' own values: the style is **not** kept whole, which would charge every shape that has a style of its own the style's every attribute twice. `None` when the
/// style holds nothing of the kind and its element holds no attribute but its names. Charged before it is made.
fn remainder_style<'t>(
    r: &mut Reader,
    names: &Names<'t>,
    el: &'t El,
    attrs: &super::names::Attrs<'t>,
    _kind: &str,
    collect: Collect<'t>,
) -> Result<Option<RawElement>, OdgError> {
    let identity = |a: &super::names::Attr<'_>| a.ns == Ns::Style && matches!(a.local, "name" | "display-name" | "parent-style-name" | "family");
    let other: Vec<&'t El> = el
        .elements()
        .filter(|child| !matches!(names.element(child), (Ns::Style, "graphic-properties" | "paragraph-properties" | "text-properties")))
        .collect();
    let extra = attrs.iter().filter(|a| !identity(a)).count();
    if collect.unmapped.is_empty() && collect.children.is_empty() && other.is_empty() && extra == 0 {
        return Ok(None);
    }
    // The properties elements that hold something, in the order they were met, each with the attributes and children it holds.
    type Holder<'t> = (&'t str, Vec<(&'t str, &'t str)>, Vec<&'t El>);
    let mut holders: Vec<Holder<'t>> = Vec::new();
    for (holder, attribute, value) in &collect.unmapped {
        match holders.iter_mut().find(|h| h.0 == *holder) {
            Some(h) => h.1.push((attribute, value)),
            None => holders.push((holder, vec![(*attribute, *value)], Vec::new())),
        }
    }
    for (holder, child) in &collect.children {
        match holders.iter_mut().find(|h| h.0 == holder.name.as_str()) {
            Some(h) => h.2.push(child),
            None => holders.push((holder.name.as_str(), Vec::new(), vec![*child])),
        }
    }
    let (mut items, mut bytes) = (2usize.saturating_add(3).saturating_add(attrs.len().saturating_mul(2)), el.name.len());
    for a in attrs.iter() {
        bytes = bytes.saturating_add(a.name.len() + a.value.len());
    }
    for (holder, pairs, children) in &holders {
        items = items.saturating_add(3).saturating_add(pairs.len().saturating_mul(2));
        bytes = bytes.saturating_add(holder.len());
        for (n, v) in pairs {
            bytes = bytes.saturating_add(n.len() + v.len());
        }
        for child in children {
            items = items.saturating_add(super::shapes::element_cost(child));
        }
    }
    // The declarations the attributes need: two items and the bytes of each.
    for (_, prefix, uri) in &collect.declared {
        items = items.saturating_add(2);
        bytes = bytes.saturating_add("xmlns:".len() + prefix.len() + uri.len());
    }
    for child in &other {
        items = items.saturating_add(super::shapes::element_cost(child));
    }
    r.take_items(items)?;
    r.take_bytes(bytes)?;
    let mut children: Vec<RawNode> = Vec::new();
    for (holder, pairs, kids) in holders {
        let declarations: Vec<(String, &str)> =
            collect.declared.iter().filter(|(h, _, _)| *h == holder).map(|(_, prefix, uri)| (format!("xmlns:{prefix}"), *uri)).collect();
        let own = RawAttrs::from_pairs(declarations.iter().map(|(k, v)| (k.as_str(), *v)).chain(pairs));
        let inner: Vec<RawNode> = kids.into_iter().map(|c| RawNode::Element(super::shapes::raw_element(c))).collect();
        children.push(RawNode::Element(RawElement { name: std::sync::Arc::from(holder), attrs: own, children: inner }));
    }
    for child in other {
        children.push(RawNode::Element(super::shapes::raw_element(child)));
    }
    Ok(Some(RawElement {
        name: std::sync::Arc::from(el.name.as_str()),
        attrs: RawAttrs::from_pairs(attrs.iter().map(|a| (a.name, a.value))),
        children,
    }))
}

/// The `.odg` data of a style sheet's style (`Style.ext.odf`): its names and family, the attributes of its properties elements that no cell reads (keyed
/// `<properties element>/<attribute>`, as written), what its element holds besides (`kept`: the element's other attributes, its children that are no properties element, and
/// for each properties element with children no cell reads, an element of the same name holding only those children), and the units of the lengths written in `px`.
/// Charged before any of it is made.
fn odf_style<'t>(
    r: &mut Reader,
    names: &Names<'t>,
    el: &'t El,
    attrs: &super::names::Attrs<'t>,
    family: &'static str,
    collect: Collect<'t>,
) -> Result<Box<diacraft_doc::OdfStyle>, OdgError> {
    let name = attrs.peek(Ns::Style, "name");
    let mut kept = Kept::default();
    let mut raw_items = 0usize;
    let mut bytes = 0usize;
    for a in attrs.iter() {
        if a.ns == Ns::Style && matches!(a.local, "name" | "display-name" | "parent-style-name" | "family") {
            continue;
        }
        raw_items = raw_items.saturating_add(2);
        bytes = bytes.saturating_add(a.name.len() + a.value.len());
    }
    for (properties, attribute, value) in &collect.unmapped {
        raw_items = raw_items.saturating_add(2);
        bytes = bytes.saturating_add(properties.len() + 1 + attribute.len() + value.len());
    }
    for attribute in &collect.px {
        raw_items = raw_items.saturating_add(2);
        bytes = bytes.saturating_add(attribute.len() + 2);
    }
    let mut other: Vec<&'t El> = Vec::new();
    for child in el.elements() {
        let mark = names.element(child);
        if !matches!(mark, (Ns::Style, "graphic-properties" | "paragraph-properties" | "text-properties")) {
            other.push(child);
        }
    }
    let mut cost = raw_items.saturating_add(super::cost::EXT_STYLE);
    for child in &other {
        cost = cost.saturating_add(super::shapes::element_cost(child));
    }
    let mut remainders: Vec<(&'t El, Vec<&'t El>)> = Vec::new();
    for (holder, child) in &collect.children {
        match remainders.last_mut() {
            Some((last, list)) if std::ptr::eq(*last, *holder) => list.push(child),
            _ => remainders.push((holder, vec![child])),
        }
        cost = cost.saturating_add(super::shapes::element_cost(child));
    }
    cost = cost.saturating_add(remainders.len().saturating_mul(3));
    r.take_items(cost)?;
    r.take_bytes(bytes)?;
    for a in attrs.iter() {
        if a.ns == Ns::Style && matches!(a.local, "name" | "display-name" | "parent-style-name" | "family") {
            continue;
        }
        kept.raw.push(a.name, a.value);
    }
    for child in other {
        kept.kept.push(0, RawNode::Element(super::shapes::raw_element(child)));
    }
    let mut unmapped = RawAttrs::new();
    for (properties, attribute, value) in &collect.unmapped {
        unmapped.push(&format!("{properties}/{attribute}"), value);
    }
    let mut units = RawAttrs::new();
    for attribute in &collect.px {
        units.push(attribute, "px");
    }
    for (holder, list) in remainders {
        let children: Vec<RawNode> = list.into_iter().map(|c| RawNode::Element(super::shapes::raw_element(c))).collect();
        kept.kept.push(0, RawNode::Element(RawElement { name: std::sync::Arc::from(holder.name.as_str()), attrs: RawAttrs::new(), children }));
    }
    Ok(Box::new(diacraft_doc::OdfStyle {
        kept,
        name: name.map(std::sync::Arc::from),
        display_name: attrs.peek(Ns::Style, "display-name").map(std::sync::Arc::from),
        parent: attrs.peek(Ns::Style, "parent-style-name").filter(|p| !p.is_empty()).map(std::sync::Arc::from),
        family: Some(std::sync::Arc::from(family)),
        unmapped,
        units,
        // The values the cells only approximate are held when the style sheet's cells are made (`sheets::build`), whose stamps they carry.
        held: Vec::new(),
        text_held: Vec::new(),
    }))
}

/// The `style:graphic-properties`, `style:paragraph-properties` and `style:text-properties` elements of a style, in that order.
type Properties<'t> = (Option<&'t El>, Option<&'t El>, Option<&'t El>);

/// Makes the values the style gives that its cells only approximate (`held.rs`), once, charged: for an automatic graphic style the values of a shape that names it, for an automatic
/// paragraph or text style the values of a paragraph or a run that names it, and for a common style of a style sheet (`sheet`) both, held in the style's own slot once the sheet's
/// cells are made (`sheets::build`). `(is_sheet, automatic)` says which the style is.
fn make_templates<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    style: &mut OdfStyle,
    (graphic_el, paragraph_el, text_el): Properties<'t>,
    (is_sheet, automatic): (bool, bool),
) -> Result<(), OdgError> {
    if automatic && style.family == Family::Graphic {
        style.templates = super::held::style_templates(r, names, graphic_el, paragraph_el)?;
    } else if automatic {
        // A paragraph style gives each paragraph its paragraph properties' values, a text style each run its text properties'.
        let (paragraph, text) = if style.family == Family::Paragraph { (paragraph_el, None) } else { (None, text_el) };
        style.text_templates = super::held::text_templates(r, names, paragraph, text)?;
    } else if is_sheet {
        style.templates = super::held::style_templates(r, names, graphic_el, paragraph_el)?;
        style.text_templates = super::held::text_templates(r, names, paragraph_el, text_el)?;
    }
    Ok(())
}

/// The values of an automatic graphic or paragraph style's properties that the cells only approximate and that have no paragraph or run to be held with (a paragraph style's
/// paragraph properties are held for each paragraph, `held::text_templates`): they are collected with the attributes no cell reads.
fn keep_text_values<'t>(names: &mut Names<'t>, family: Family, (paragraph_el, text_el): (Option<&'t El>, Option<&'t El>), collect: &mut Collect<'t>) {
    if !matches!(family, Family::Graphic | Family::Paragraph) {
        return;
    }
    let paragraph = if family == Family::Graphic { paragraph_el } else { None };
    for (group, pairs) in super::held::found_text(names, paragraph, text_el) {
        if let Some(holder) = if group.is_paragraph() { paragraph_el } else { text_el } {
            for (n, v) in pairs {
                collect.unmapped.push((holder.name.as_str(), n, v));
            }
        }
    }
}

/// Whether the tab cells say everything a `style:tab-stops` holds: its stops are `style:tab-stop` elements that name a position that reads and a type of the four, and
/// nothing else is on them or in it.
fn tab_stops_exact<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> bool {
    if names.attrs(el).iter().next().is_some() {
        return false;
    }
    for stop in el.elements() {
        r.saw_element();
        let mark = names.enter(stop);
        let exact = names.is(stop, Ns::Style, "tab-stop") && {
            let attrs = names.attrs(stop);
            r.saw_attrs(attrs.len());
            r.value_parses(1);
            attrs.iter().all(|a| a.ns == Ns::Style && matches!(a.local, "position" | "type"))
                && attrs.peek(Ns::Style, "position").and_then(units::length).is_some()
                && matches!(attrs.peek(Ns::Style, "type").unwrap_or("left"), "left" | "center" | "right" | "char")
                && stop.elements().next().is_none()
        };
        names.leave(mark);
        if !exact {
            return false;
        }
    }
    true
}

/// The tab stops of a `style:tab-stops`.
/// The most tab stops one style holds: two cells each, in one row, which holds `MAX_CELLS_PER_CONTAINER` cells. More are refused here,
/// before any is stored, and not by the model's own check afterwards.
const MOST_TABS: usize = diacraft_doc::MAX_CELLS_PER_CONTAINER / 2;

fn tabs_of<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<Vec<Tab>, OdgError> {
    let mut out = Vec::new();
    for stop in el.elements() {
        r.saw_element();
        if !names.is(stop, Ns::Style, "tab-stop") {
            r.keep("style_element");
            continue;
        }
        let attrs = names.attrs(stop);
        r.saw_attrs(attrs.len());
        r.value_parses(1);
        let Some(position) = attrs.peek(Ns::Style, "position").and_then(units::length) else {
            r.keep("tab_stop");
            continue;
        };
        let alignment = match attrs.peek(Ns::Style, "type").unwrap_or("left") {
            "center" => 1,
            "right" => 2,
            "char" => 3,
            _ => 0,
        };
        if out.len() >= MOST_TABS {
            return Err(OdgError::TooComplex { what: "tab stops in one style", max: MOST_TABS as u64 });
        }
        out.push(Tab { position, alignment });
    }
    Ok(out)
}

impl StyleSet {
    /// Reads a `style:style` or `style:default-style`, adding it to the set.
    fn add_style<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File, common: bool, default: bool) -> Result<(), OdgError> {
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let container = super::odfdoc::Container::of(file, common);
        let Some(family) = attrs.peek(Ns::Style, "family").and_then(Family::of) else {
            r.keep("style_other_family");
            // A style of a family the model has no cells for (a drawing page's, a presentation's, a table's): held whole.
            return self.doc.keep_style(r, names, container, el);
        };
        let name = attrs.peek(Ns::Style, "name").unwrap_or_default().to_owned();
        if name.is_empty() && !default {
            r.keep("style_without_name");
            return self.doc.keep_style(r, names, container, el);
        }
        // A common style of the graphic and paragraph families is a style sheet of the model, which holds what no cell reads in its own slot; **an automatic style** of the
        // graphic, paragraph and text families is kept as what no cell reads (the cells and the held values say the rest); every other style is kept whole.
        let is_sheet = common && !default && matches!(family, Family::Graphic | Family::Paragraph);
        let automatic = !common && !default;
        let mut collect = Collect::default();
        let mut style = OdfStyle {
            name,
            display: attrs.peek(Ns::Style, "display-name").map(str::to_owned),
            family,
            parent: attrs.peek(Ns::Style, "parent-style-name").filter(|p| !p.is_empty()).map(str::to_owned),
            common: common && !default,
            default,
            file,
            props: [Vec::new(), Vec::new(), Vec::new()],
            tabs: None,
            dia: Dia::default(),
            sheet: None,
            odf: None,
            templates: Vec::new(),
            text_templates: Vec::new(),
        };
        let mut tabs: Option<Vec<Tab>> = None;
        let (mut graphic_el, mut paragraph_el, mut text_el): (Option<&'t El>, Option<&'t El>, Option<&'t El>) = (None, None, None);
        for child in el.elements() {
            r.saw_element();
            let mark = names.enter(child);
            match names.element(child) {
                (Ns::Style, "graphic-properties") => {
                    graphic_el.get_or_insert(child);
                    style.props[0] = props_of(r, names, child, GRAPHIC, Some(&mut style.dia), (is_sheet || automatic).then_some(&mut collect));
                    if is_sheet || automatic {
                        collect.children.extend(child.elements().map(|inner| (child, inner)));
                    }
                }
                (Ns::Style, "paragraph-properties") => {
                    paragraph_el.get_or_insert(child);
                    style.props[1] = props_of(r, names, child, PARAGRAPH, None, (is_sheet || automatic).then_some(&mut collect));
                    for inner in child.elements() {
                        r.saw_element();
                        let inner_mark = names.enter(inner);
                        if names.is(inner, Ns::Style, "tab-stops") {
                            // The last list written is the style's; it is shared below, once the style is read, so a list written over
                            // is let go and not kept in the reader's table.
                            tabs = Some(tabs_of(r, names, inner)?);
                            // What the tab cells (a position and an alignment) do not say of them (a leader, a character, a stop that does not read) stays with an automatic
                            // paragraph style, as the element; a style sheet's and a graphic style's are held whole as a held value (`held.rs`).
                            if automatic && family != Family::Graphic && !tab_stops_exact(r, names, inner) {
                                collect.children.push((child, inner));
                            }
                        } else {
                            r.keep("style_element");
                            if is_sheet || automatic {
                                collect.children.push((child, inner));
                            }
                        }
                        names.leave(inner_mark);
                    }
                }
                (Ns::Style, "text-properties") => {
                    text_el.get_or_insert(child);
                    style.props[2] = props_of(r, names, child, TEXT, None, (is_sheet || automatic).then_some(&mut collect));
                    if is_sheet || automatic {
                        collect.children.extend(child.elements().map(|inner| (child, inner)));
                    }
                }
                _ => r.keep("style_element"),
            }
            names.leave(mark);
        }
        // A default tab distance is read into `DefaultTabStop`, which no copy written from the model writes: the open names it. A default style is kept whole,
        // and comes back with it.
        if !default && style.props[1].iter().any(|(p, _)| *p == P::TabStopDistance) {
            r.not_kept("style_property", 1);
        }
        // The values an automatic graphic or paragraph style's own properties give the text that the cells only approximate have no paragraph or run to be held with (a paragraph
        // style's paragraph properties are held for each paragraph, `held::text_templates`): they stay with the style, as written.
        if automatic {
            keep_text_values(names, family, (paragraph_el, text_el), &mut collect);
        }
        // One item, one for each property and tab stop, and the bytes of its names and values.
        let cost = 1 + style.props.iter().map(Vec::len).sum::<usize>() + tabs.as_ref().map_or(0, Vec::len);
        let bytes = style.name.len()
            + style.display.as_ref().map_or(0, String::len)
            + style.parent.as_ref().map_or(0, String::len)
            + style.props.iter().flatten().map(|(_, v)| v.text.len()).sum::<usize>()
            + style.dia.tier2.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        r.take_items(cost)?;
        r.take_bytes(bytes)?;
        // The stops are shared with an equal list of an earlier style, once charged (`Reader::shared_tabs`).
        style.tabs = tabs.map(|stops| r.shared_tabs(stops));
        // The values a shape's own automatic style gives: made here once, for every shape that names the style.
        let elements = (graphic_el, paragraph_el, text_el);
        make_templates(r, names, &mut style, elements, (is_sheet, automatic))?;
        if is_sheet {
            let word = if family == Family::Graphic { "graphic" } else { "paragraph" };
            style.odf = Some(odf_style(r, names, el, &attrs, word, collect)?);
        } else if automatic {
            let word = match family {
                Family::Graphic => "graphic",
                Family::Paragraph => "paragraph",
                Family::Text => "text",
            };
            if let Some(element) = remainder_style(r, names, el, &attrs, word, collect)? {
                self.doc.push_style(container, word, &style.name, element);
            }
        } else {
            self.doc.keep_style(r, names, container, el)?;
        }
        let at = self.list.len();
        let key = (family, style.name.clone());
        let map = if default {
            self.defaults.entry(family).or_insert(at);
            None
        } else if common {
            Some(&mut self.common)
        } else {
            Some(match file {
                File::Styles => &mut self.auto_styles,
                File::Content => &mut self.auto_content,
            })
        };
        if let Some(map) = map {
            // No two styles of one family share a name in a container; a second is kept and the first is the one found.
            match map.entry(key) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    // One warning for each name that a container holds twice, naming it.
                    if r.first_repeated(&style.name) {
                        r.warn("style_name_repeated", || {
                            format!(
                                "two styles of one family have the name \"{}\"; the first is used",
                                crate::report::shown(&style.name).escape_debug()
                            )
                        });
                    }
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(at);
                }
            }
        }
        self.list.push(style);
        Ok(())
    }

    /// Holds `child`, a child of `office:styles` of a definition's kind, as a named definition; one with no `draw:name` is no definition and is held whole as a style element.
    fn hold_definition<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, child: &'t El) -> Result<(), OdgError> {
        if !self.doc.definition(r, names, child)? {
            self.doc.keep_style(r, names, super::odfdoc::Container::Common, child)?;
        }
        Ok(())
    }

    /// Reads a `draw:stroke-dash`.
    fn add_dash<'t>(&mut self, r: &mut Reader, names: &Names<'t>, el: &'t El) -> Result<(), OdgError> {
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let Some(name) = attrs.peek(Ns::Draw, "name") else { return Ok(()) };
        // Each style-level thing is charged where it is read: one item, and its strings' bytes.
        r.take_items(1)?;
        r.take_bytes(name.len())?;
        let run =
            |text: Option<&str>| text.and_then(|t| units::percent(t).map(Run::Percent).or_else(|| units::length(t).map(|l| Run::Inches(l.inches))));
        let count = |text: Option<&str>| text.and_then(units::integer).and_then(|n| u32::try_from(n).ok());
        let dash = Dash {
            own: super::props::own_dash(name),
            dots1: count(attrs.peek(Ns::Draw, "dots1")),
            dots1_length: run(attrs.peek(Ns::Draw, "dots1-length")),
            dots2: count(attrs.peek(Ns::Draw, "dots2")),
            dots2_length: run(attrs.peek(Ns::Draw, "dots2-length")),
            distance: run(attrs.peek(Ns::Draw, "distance")),
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = self.dashes.entry(name.to_owned()) {
            slot.insert(self.dash_list.len());
            self.dash_list.push(dash);
        }
        Ok(())
    }

    /// Reads a `draw:gradient`, and makes its stand-in (`gradient.rs`).
    fn add_gradient<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<(), OdgError> {
        use super::gradient::{Definition, Layout};
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let Some(name) = attrs.peek(Ns::Draw, "name") else { return Ok(()) };
        r.take_items(1)?;
        r.take_bytes(name.len())?;
        let layout = Layout::of(attrs.peek(Ns::Draw, "style")).unwrap_or_else(|| {
            // A layout the standard does not have is drawn as the one it is when absent.
            r.keep("property_value");
            Layout::Linear
        });
        let percent = |local: &str, absent: f64| attrs.peek(Ns::Draw, local).and_then(units::percent).unwrap_or(absent);
        let mut definition = Definition {
            layout,
            start: attrs.peek(Ns::Draw, "start-color").and_then(units::rgb),
            end: attrs.peek(Ns::Draw, "end-color").and_then(units::rgb),
            start_intensity: percent("start-intensity", 100.0),
            end_intensity: percent("end-intensity", 100.0),
            border: percent("border", 0.0),
            angle: attrs.peek(Ns::Draw, "angle").and_then(units::angle_degrees).unwrap_or(0.0),
            cx: percent("cx", 0.0),
            cy: percent("cy", 0.0),
            stops: Vec::new(),
        };
        definition.stops = super::gradient::read_stops(r, names, el)?;
        let gradient = super::gradient::stand_in(&definition);
        if let std::collections::hash_map::Entry::Vacant(slot) = self.gradients.entry(name.to_owned()) {
            // The rows it stands in with, each charged as the sheets that show it will be.
            r.take_items(gradient.stops.len())?;
            self.most_stop_rows = self.most_stop_rows.max(gradient.stops.len());
            slot.insert(self.gradient_list.len());
            self.gradient_list.push(gradient);
        }
        Ok(())
    }

    /// Reads a `text:list-style` into `file`'s list styles.
    fn add_list_style<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File) -> Result<(), OdgError> {
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let Some(name) = attrs.peek(Ns::Style, "name").map(str::to_owned) else { return Ok(()) };
        let mut levels: Vec<ListLevel> = Vec::new();
        for child in el.elements() {
            r.saw_element();
            let mark = names.enter(child);
            let (ns, local) = names.element(child);
            let numbered = (ns, local) == (Ns::Text, "list-level-style-number");
            let bullet = (ns, local) == (Ns::Text, "list-level-style-bullet");
            if numbered || bullet {
                let a = names.attrs(child);
                r.saw_attrs(a.len());
                let mut level = ListLevel {
                    level: a.peek(Ns::Text, "level").and_then(units::integer).and_then(|n| u32::try_from(n).ok()).unwrap_or(1),
                    bullet: if bullet { a.peek(Ns::Text, "bullet-char").map(str::to_owned) } else { None },
                    numbered,
                    font_name: None,
                    font_family: None,
                    font_size: None,
                    font_size_length: None,
                    margin_left: None,
                    text_indent: None,
                };
                read_level_children(r, names, child, &mut level);
                levels.push(level);
            } else {
                r.keep("list_level");
            }
            names.leave(mark);
        }
        r.take_items(1 + levels.len())?;
        let strings =
            |l: &ListLevel| [&l.bullet, &l.font_name, &l.font_family, &l.font_size].iter().map(|s| s.as_ref().map_or(0, String::len)).sum::<usize>();
        r.take_bytes(name.len() + levels.iter().map(strings).sum::<usize>())?;
        // By level number, equal numbers in the order written: a list finds its level by a binary search ([`list_level`]), the first
        // written of any number, as a scan of the levels in order would.
        levels.sort_by_cached_key(|l| l.level);
        let map = match file {
            File::Styles => &mut self.lists_styles,
            File::Content => &mut self.lists_content,
        };
        map.entry(name).or_insert(levels);
        Ok(())
    }

    /// Reads `office:font-face-decls`: each `style:font-face`'s name and family, quotes removed.
    fn add_fonts<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File) -> Result<(), OdgError> {
        for face in el.elements() {
            r.saw_element();
            let mark = names.enter(face);
            if names.is(face, Ns::Style, "font-face") {
                let a = names.attrs(face);
                r.saw_attrs(a.len());
                if let (Some(name), Some(family)) = (a.peek(Ns::Style, "name"), a.peek(Ns::Svg, "font-family")) {
                    let family = family.trim().trim_matches(|c| c == '\'' || c == '"');
                    r.take_items(1)?;
                    r.take_bytes(name.len() + family.len())?;
                    let family = family.to_owned();
                    let map = match file {
                        File::Styles => &mut self.fonts_styles,
                        File::Content => &mut self.fonts_content,
                    };
                    map.entry(name.to_owned()).or_insert(family);
                }
                // The declaration is held as a face of the font table, exactly as written.
                super::keep::face(r, names, face, file)?;
            }
            names.leave(mark);
        }
        Ok(())
    }

    /// Reads `office:styles` (in `styles.xml`).
    pub(crate) fn read_common<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<(), OdgError> {
        for child in el.elements() {
            r.saw_element();
            let mark = names.enter(child);
            match names.element(child) {
                (Ns::Style, "style") => self.add_style(r, names, child, File::Styles, true, false)?,
                (Ns::Style, "default-style") => self.add_style(r, names, child, File::Styles, true, true)?,
                (Ns::Draw, "stroke-dash") => {
                    self.add_dash(r, names, child)?;
                    self.hold_definition(r, names, child)?;
                }
                (Ns::Draw, "marker") => {
                    let a = names.attrs(child);
                    r.saw_attrs(a.len());
                    if let Some(name) = a.peek(Ns::Draw, "name") {
                        r.take_items(1)?;
                        r.take_bytes(name.len())?;
                        self.markers.insert(name.to_owned());
                    }
                    self.hold_definition(r, names, child)?;
                }
                (Ns::Draw, "gradient") => {
                    self.add_gradient(r, names, child)?;
                    self.hold_definition(r, names, child)?;
                }
                (Ns::Text, "list-style") => {
                    self.add_list_style(r, names, child, File::Styles)?;
                    self.doc.keep_style(r, names, super::odfdoc::Container::Common, child)?;
                }
                (Ns::Draw, "hatch" | "opacity" | "fill-image") => {
                    r.keep("styles_element");
                    self.hold_definition(r, names, child)?;
                }
                _ => {
                    r.keep("styles_element");
                    self.doc.keep_style(r, names, super::odfdoc::Container::Common, child)?;
                }
            }
            names.leave(mark);
        }
        Ok(())
    }

    /// Reads `office:automatic-styles` of `file`.
    pub(crate) fn read_automatic<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File) -> Result<(), OdgError> {
        // The declarations down to this container are the model's own (the roots); a style's own, and its properties', are local.
        let held = names.hold();
        let done = self.read_automatic_styles(r, names, el, file);
        if held {
            names.release();
        }
        done
    }

    fn read_automatic_styles<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File) -> Result<(), OdgError> {
        for child in el.elements() {
            r.saw_element();
            let mark = names.enter(child);
            match names.element(child) {
                (Ns::Style, "style") => self.add_style(r, names, child, file, false, false)?,
                (Ns::Style, "page-layout") => {
                    self.add_page_layout(r, names, child)?;
                    self.doc.keep_style(r, names, super::odfdoc::Container::of(file, false), child)?;
                }
                (Ns::Text, "list-style") => {
                    self.add_list_style(r, names, child, file)?;
                    self.doc.keep_style(r, names, super::odfdoc::Container::of(file, false), child)?;
                }
                _ => {
                    r.keep("styles_element");
                    self.doc.keep_style(r, names, super::odfdoc::Container::of(file, false), child)?;
                }
            }
            names.leave(mark);
        }
        Ok(())
    }

    /// Reads `office:font-face-decls` of `file`.
    pub(crate) fn read_fonts<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El, file: File) -> Result<(), OdgError> {
        self.add_fonts(r, names, el, file)
    }

    /// Reads a `style:page-layout`.
    fn add_page_layout<'t>(&mut self, r: &mut Reader, names: &mut Names<'t>, el: &'t El) -> Result<(), OdgError> {
        let attrs = names.attrs(el);
        r.saw_attrs(attrs.len());
        let Some(name) = attrs.peek(Ns::Style, "name").map(str::to_owned) else { return Ok(()) };
        r.take_items(1)?;
        r.take_bytes(name.len())?;
        let mut layout = PageLayout::default();
        for child in el.elements() {
            r.saw_element();
            let mark = names.enter(child);
            if names.is(child, Ns::Style, "page-layout-properties") {
                let a = names.attrs(child);
                r.saw_attrs(a.len());
                let len = |local: &str| a.peek(Ns::Fo, local).and_then(units::length);
                layout.width = len("page-width");
                layout.height = len("page-height");
                layout.margins = [len("margin-left"), len("margin-right"), len("margin-top"), len("margin-bottom")];
                let orientation = a.peek(Ns::Style, "print-orientation");
                r.take_bytes(orientation.map_or(0, str::len))?;
                layout.orientation = orientation.map(str::to_owned);
                // What no page cell reads is held as written, and a length in `px` (no cell spells it) keeps its unit; both are charged before they are made.
                let mapped = |a: &super::names::Attr<'_>| {
                    (a.ns == Ns::Fo
                        && matches!(a.local, "page-width" | "page-height" | "margin-left" | "margin-right" | "margin-top" | "margin-bottom"))
                        || (a.ns == Ns::Style && a.local == "print-orientation")
                };
                let (mut items, mut bytes) = (0usize, 0usize);
                for attr in a.iter().filter(|attr| !mapped(attr)) {
                    items = items.saturating_add(2);
                    bytes = bytes.saturating_add(attr.name.len() + attr.value.len());
                }
                for attr in a.iter().filter(|attr| mapped(attr) && units::length(attr.value).is_some_and(|l| l.unit == units::LenUnit::Px)) {
                    items = items.saturating_add(2);
                    bytes = bytes.saturating_add(attr.name.len() + 2);
                }
                r.take_items(items)?;
                r.take_bytes(bytes)?;
                for attr in a.iter() {
                    if !mapped(attr) {
                        layout.unmapped.push(attr.name, attr.value);
                    } else if units::length(attr.value).is_some_and(|l| l.unit == units::LenUnit::Px) {
                        layout.units.push(attr.name, "px");
                    }
                }
            } else {
                r.keep("styles_element");
            }
            names.leave(mark);
        }
        r.take_items(1)?;
        self.page_layouts.entry(name).or_insert(layout);
        Ok(())
    }

    /// The style a reference from `file` names, with one warning per name that finds none.
    pub(crate) fn find(&self, r: &mut Reader, family: Family, name: &str, file: File) -> Option<usize> {
        let key = (family, name.to_owned());
        let auto = match file {
            File::Styles => &self.auto_styles,
            File::Content => &self.auto_content,
        };
        let found = auto.get(&key).or_else(|| self.common.get(&key)).copied();
        if found.is_none() && r.first_missing(name) {
            r.warn("style_not_found", || "a reference names a style the drawing does not have; it reads as no style".to_owned());
        }
        found
    }

    /// The parent of the style at `at`.
    fn parent(&self, r: &mut Reader, at: usize) -> Option<usize> {
        let style = self.list.get(at)?;
        let parent = style.parent.as_deref()?;
        let key = (style.family, parent.to_owned());
        if let Some(&found) = self.common.get(&key) {
            return Some(found);
        }
        let auto = match style.file {
            File::Styles => &self.auto_styles,
            File::Content => &self.auto_content,
        };
        if let Some(&found) = auto.get(&key) {
            r.warn("style_parent_automatic", || "a style's parent is an automatic style, which the standard forbids; it is followed".to_owned());
            return Some(found);
        }
        if r.first_missing(parent) {
            r.warn("style_not_found", || "a style's parent names no style; the chain ends there".to_owned());
        }
        None
    }

    /// The default style of `family`.
    pub(crate) fn default_of(&self, family: Family) -> Option<usize> {
        self.defaults.get(&family).copied()
    }

    /// The style at `at`.
    pub(crate) fn get(&self, at: usize) -> Option<&OdfStyle> {
        self.list.get(at)
    }

    /// The family of the face `name` declares in `file`'s `office:font-face-decls`, then in the other file's.
    pub(crate) fn font_family(&self, name: &str, file: File) -> Option<&str> {
        let (first, second) = match file {
            File::Styles => (&self.fonts_styles, &self.fonts_content),
            File::Content => (&self.fonts_content, &self.fonts_styles),
        };
        first.get(name).or_else(|| second.get(name)).map(String::as_str)
    }

    /// The levels of the list style `name`, looked for in `file`'s automatic list styles and then in `styles.xml`'s, sorted by level
    /// number (`add_list_style`). Borrowed, not copied: a list style of many levels named by many lists is not cloned for each.
    pub(crate) fn list_style(&self, name: &str, file: File) -> Option<&[ListLevel]> {
        let first = match file {
            File::Styles => None,
            File::Content => self.lists_content.get(name),
        };
        first.or_else(|| self.lists_styles.get(name)).map(Vec::as_slice)
    }

    /// The common styles of the families style sheets are made of, in document order.
    pub(crate) fn sheet_styles(&self) -> Vec<usize> {
        self.list.iter().enumerate().filter(|(_, s)| s.common && matches!(s.family, Family::Graphic | Family::Paragraph)).map(|(i, _)| i).collect()
    }

    /// The dash at `index` in the order read, as a style's property found it.
    pub(crate) fn dash(&self, index: usize) -> Option<&Dash> {
        self.dash_list.get(index)
    }

    /// The gradient at `index` in the order read, as a style's property found it.
    pub(crate) fn gradient(&self, index: usize) -> Option<&Gradient> {
        self.gradient_list.get(index)
    }

    /// The most rows of `FillGradient` any gradient of the drawing stands in with.
    pub(crate) fn most_stop_rows(&self) -> usize {
        self.most_stop_rows
    }

    /// Looks up, once for each style from the one at `from` on, the names its properties and its DiaCraft links hold: a dash, a gradient, a
    /// marker, and the common styles the links name. Each style keeps what its name found (an index, or that there is none), so that a
    /// shape or a run that shows the style never hashes the name. The dashes, gradients and markers are all `styles.xml`'s common ones, so
    /// this runs when the part that holds the styles has been read, which is when every one a name may mean is known. The bytes of each
    /// name hashed are counted ([`crate::ReadWork::name_bytes`]).
    pub(crate) fn resolve(&mut self, r: &mut Reader, from: usize) {
        for at in from..self.list.len() {
            let Some(style) = self.list.get(at) else { break };
            let mut found: Vec<(usize, usize, Typed)> = Vec::new();
            for (kind, list) in style.props.iter().enumerate() {
                for (place, (_, prop)) in list.iter().enumerate() {
                    let name = prop.text.as_str();
                    let by_name = |map: &HashMap<String, usize>| map.get(name).map_or(Name::Missing, |&index| Name::Found(index));
                    let typed = match prop.typed {
                        Typed::Dash(Name::Unresolved) => Typed::Dash(by_name(&self.dashes)),
                        Typed::Gradient(Name::Unresolved) => Typed::Gradient(by_name(&self.gradients)),
                        Typed::Marker(Marker::Unresolved) => Typed::Marker(if name.is_empty() {
                            Marker::Empty
                        } else if !self.markers.contains(name) {
                            Marker::Missing
                        } else {
                            super::props::own_marker(name).map_or(Marker::Other, Marker::Own)
                        }),
                        _ => continue,
                    };
                    r.name_bytes(name.len());
                    found.push((kind, place, typed));
                }
            }
            let linked = style.dia.links.as_ref().map(|links| {
                let mut find = |name: &str| {
                    r.name_bytes(name.len());
                    self.common_named(Family::Graphic, name).or_else(|| {
                        r.name_bytes(name.len());
                        self.common_named(Family::Paragraph, name)
                    })
                };
                match (find(&links[0]), find(&links[1]), find(&links[2])) {
                    (Some(line), Some(fill), Some(text)) => Some([line, fill, text]),
                    _ => None,
                }
            });
            if let Some(style) = self.list.get_mut(at) {
                for (kind, place, typed) in found {
                    if let Some((_, prop)) = style.props.get_mut(kind).and_then(|list| list.get_mut(place)) {
                        prop.typed = typed;
                    }
                }
                if let Some(linked) = linked {
                    style.dia.linked = Some(linked);
                    style.dia.links = None;
                }
            }
        }
    }

    /// The common style of `family` named `name`, if any.
    pub(crate) fn common_named(&self, family: Family, name: &str) -> Option<usize> {
        self.common.get(&(family, name.to_owned())).copied()
    }

    /// Gives the style at `at` the style sheet `id`.
    pub(crate) fn set_sheet(&mut self, at: usize, id: u32) {
        if let Some(style) = self.list.get_mut(at) {
            style.sheet = Some(id);
        }
    }
}

/// The children of a list level: its text properties (face and size) and the label alignment's margin and indent.
fn read_level_children<'t>(r: &mut Reader, names: &mut Names<'t>, el: &'t El, level: &mut ListLevel) {
    for child in el.elements() {
        r.saw_element();
        let mark = names.enter(child);
        match names.element(child) {
            (Ns::Style, "text-properties") => {
                let a = names.attrs(child);
                r.saw_attrs(a.len());
                level.font_name = a.peek(Ns::Style, "font-name").map(str::to_owned);
                // The family is cut of white space and quotes here, once, and the size read here, once: a list item shows them cut and read.
                level.font_family = a.peek(Ns::Fo, "font-family").map(|f| f.trim().trim_matches(|c| c == '\'' || c == '"').to_owned());
                level.font_size = a.peek(Ns::Fo, "font-size").map(str::to_owned);
                level.font_size_length = level.font_size.as_deref().and_then(|size| {
                    r.value_parses(1);
                    units::length(size)
                });
            }
            (Ns::Style, "list-level-properties") => {
                for inner in child.elements() {
                    r.saw_element();
                    let inner_mark = names.enter(inner);
                    if names.is(inner, Ns::Style, "list-level-label-alignment") {
                        let a = names.attrs(inner);
                        r.saw_attrs(a.len());
                        level.margin_left = a.peek(Ns::Fo, "margin-left").and_then(units::length);
                        level.text_indent = a.peek(Ns::Fo, "text-indent").and_then(units::length);
                    }
                    names.leave(inner_mark);
                }
            }
            _ => r.keep("list_level_child"),
        }
        names.leave(mark);
    }
}

/// Where a property's value was found along a chain: which style, and how many parent steps from the style asked (0 for its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub style: usize,
    pub depth: usize,
}

/// A key of [`Resolved`]: a property on one of the three elements, or the tab stops (`tabs` true).
type Key = (Kind, P);

/// What a style's chain gives each property: the nearest style within [`MAX_CHAIN`] steps that sets it. Sorted by key.
#[derive(Debug, Default)]
pub(crate) struct Resolved {
    entries: Vec<(Key, Found)>,
    /// The nearest style that holds tab stops.
    tabs: Option<Found>,
}

impl Resolved {
    pub(crate) fn get(&self, kind: Kind, p: P) -> Option<Found> {
        self.entries.binary_search_by(|(k, _)| k.cmp(&(kind, p))).ok().and_then(|at| self.entries.get(at)).map(|(_, f)| *f)
    }

    pub(crate) fn tabs(&self) -> Option<Found> {
        self.tabs
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Every property the chain gives, and where.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &((Kind, P), Found)> {
        self.entries.iter()
    }

    /// `layers`, nearest first, as one: each property from the first layer that gives it, as a view walking them would find it. A value's
    /// depth becomes its layer's place times one more than the longest chain, plus its own, so that a nearer layer stays nearer.
    pub(crate) fn flattened(layers: &[&Resolved]) -> Resolved {
        let mut entries = Vec::with_capacity(layers.iter().map(|l| l.entries.len()).sum());
        let far = |at: usize, found: Found| Found { style: found.style, depth: at.saturating_mul(MAX_CHAIN + 1).saturating_add(found.depth) };
        for (at, layer) in layers.iter().enumerate() {
            entries.extend(layer.entries.iter().map(|(key, found)| (*key, far(at, *found))));
        }
        // A stable sort keeps the nearest first among equal keys, and the rest go.
        entries.sort_by_key(|e| e.0);
        entries.dedup_by(|a, b| a.0 == b.0);
        let tabs = layers.iter().enumerate().find_map(|(at, l)| l.tabs.map(|f| far(at, f)));
        Resolved { entries, tabs }
    }
}

impl PartialOrd for Kind {
    fn partial_cmp(&self, other: &Kind) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Kind {
    fn cmp(&self, other: &Kind) -> std::cmp::Ordering {
        self.at().cmp(&other.at())
    }
}

impl PartialOrd for P {
    fn partial_cmp(&self, other: &P) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for P {
    fn cmp(&self, other: &P) -> std::cmp::Ordering {
        (*self as usize).cmp(&(*other as usize))
    }
}

/// The parent of each style, its resolution and its link target, each worked out once (`Resolver::prepare`).
#[derive(Default)]
pub(crate) struct Resolver {
    parents: Vec<Option<usize>>,
    resolved: Vec<Option<std::rc::Rc<Resolved>>>,
    /// The first common style along each style's chain: the target of its links.
    links: Vec<Option<Option<usize>>>,
    /// Each style's font size, in inches, once worked out (`font_size`).
    sizes: HashMap<usize, f64>,
}

impl Resolver {
    /// Finds the parent of every style added since the last call, each once.
    pub(crate) fn prepare(&mut self, r: &mut Reader, set: &StyleSet) {
        while self.parents.len() < set.list.len() {
            let at = self.parents.len();
            let parent = set.parent(r, at);
            r.style_step();
            self.parents.push(parent);
            self.resolved.push(None);
            self.links.push(None);
        }
    }

    /// The parent of the style at `at`, as [`Resolver::prepare`] found it.
    pub(crate) fn parent_of(&self, at: usize) -> Option<usize> {
        self.parent(at)
    }

    fn parent(&self, at: usize) -> Option<usize> {
        self.parents.get(at).copied().flatten()
    }

    /// What the chain of the style at `at` gives each property. Each style's is built once, from its own properties and its parent's, so a
    /// long chain costs its length once and not once for each style in it; a value further than [`MAX_CHAIN`] - 1 steps away is dropped
    /// A cycle ends at the style where the walk would come round again, with a warning.
    pub(crate) fn resolved(&mut self, r: &mut Reader, set: &StyleSet, at: usize) -> Result<std::rc::Rc<Resolved>, OdgError> {
        if let Some(Some(done)) = self.resolved.get(at) {
            return Ok(std::rc::Rc::clone(done));
        }
        // The styles up the chain whose resolution is not yet known, nearest first.
        let mut todo = vec![at];
        let mut seen: HashSet<usize> = HashSet::from([at]);
        let mut base: Option<std::rc::Rc<Resolved>> = None;
        let mut here = at;
        // A walk visits each style once at most: the seen set ends it at a cycle, and this bound ends it whatever the set says.
        let mut steps = 0usize;
        while let Some(parent) = self.parent(here) {
            r.style_step();
            steps += 1;
            if steps > self.parents.len() {
                break;
            }
            if let Some(Some(done)) = self.resolved.get(parent) {
                base = Some(std::rc::Rc::clone(done));
                break;
            }
            if !seen.insert(parent) {
                r.warn("style_cycle", || "the parents of some styles go round in a circle; the chain ends where it would repeat".to_owned());
                break;
            }
            todo.push(parent);
            here = parent;
        }
        let mut made = base;
        for &style in todo.iter().rev() {
            let resolution = build(set, style, made.as_deref());
            r.take_items(resolution.len())?;
            let rc = std::rc::Rc::new(resolution);
            if let Some(slot) = self.resolved.get_mut(style) {
                *slot = Some(std::rc::Rc::clone(&rc));
            }
            made = Some(rc);
        }
        // `todo` holds `at`, so the loop made its resolution.
        Ok(made.unwrap_or_default())
    }

    /// The first common style along the chain of the style at `at` (itself when it is common), within [`MAX_CHAIN`] steps: the target of
    /// a shape's links. `None` when the chain holds none, and the links then name the root.
    pub(crate) fn link(&mut self, r: &mut Reader, set: &StyleSet, at: usize) -> Option<usize> {
        if let Some(Some(done)) = self.links.get(at) {
            return *done;
        }
        let mut here = at;
        let mut seen: HashSet<usize> = HashSet::new();
        let mut found = None;
        for _ in 0..MAX_CHAIN {
            r.style_step();
            if !seen.insert(here) {
                break;
            }
            if set.get(here).is_some_and(|s| s.common) {
                found = Some(here);
                break;
            }
            match self.parent(here) {
                Some(next) => here = next,
                None => break,
            }
        }
        if let Some(slot) = self.links.get_mut(at) {
            *slot = Some(found);
        }
        found
    }

    /// The font size the style at `at` gives its text, in inches: its own `fo:font-size` as a length, or a percentage of its
    /// parent's size, or its parent's when it sets none, within [`MAX_CHAIN`] steps; past the chain, `base`.
    pub(crate) fn font_size(&mut self, r: &mut Reader, set: &StyleSet, at: usize, base: f64) -> f64 {
        if let Some(size) = self.sizes.get(&at) {
            return *size;
        }
        // Walk up to the first style that sets a length, or to the end, then come back multiplying.
        let mut factors = Vec::new();
        let mut size = base;
        let mut here = Some(at);
        let mut seen: HashSet<usize> = HashSet::new();
        while let Some(style) = here {
            r.style_step();
            if factors.len() >= MAX_CHAIN || !seen.insert(style) {
                break;
            }
            if let Some(known) = self.sizes.get(&style) {
                size = *known;
                break;
            }
            match set.get(style).and_then(|s| s.own_prop(Kind::Text, P::FontSize)).map(|prop| prop.typed) {
                Some(Typed::Size(Size::Length(l))) => {
                    size = l.inches;
                    break;
                }
                Some(Typed::Size(Size::Percent(p))) => factors.push(p / 100.0),
                _ => factors.push(1.0),
            }
            here = self.parent(style);
        }
        for factor in factors.iter().rev() {
            size *= factor;
        }
        if size.is_finite() {
            self.sizes.insert(at, size);
            size
        } else {
            base
        }
    }
}

/// The resolution of `style` from its own properties and its parent's resolution `base`.
fn build(set: &StyleSet, style: usize, base: Option<&Resolved>) -> Resolved {
    let mut entries: Vec<(Key, Found)> = Vec::new();
    let mut tabs = None;
    if let Some(s) = set.get(style) {
        for kind in [Kind::Graphic, Kind::Paragraph, Kind::Text] {
            for (p, _) in s.props(kind) {
                entries.push(((kind, *p), Found { style, depth: 0 }));
            }
        }
        if s.tabs.is_some() {
            tabs = Some(Found { style, depth: 0 });
        }
    }
    entries.sort_by_key(|e| e.0);
    entries.dedup_by(|a, b| a.0 == b.0);
    if let Some(base) = base {
        let deeper = |f: Found| (f.depth + 1 < MAX_CHAIN).then_some(Found { style: f.style, depth: f.depth + 1 });
        let own = entries.len();
        for (key, found) in &base.entries {
            if let Some(found) = deeper(*found)
                && entries.get(..own).is_some_and(|mine| mine.binary_search_by(|(k, _)| k.cmp(key)).is_err())
            {
                entries.push((*key, found));
            }
        }
        entries.sort_by_key(|e| e.0);
        if tabs.is_none() {
            tabs = base.tabs.and_then(deeper);
        }
    }
    Resolved { entries, tabs }
}
