//! Writing a theme part: the inverse of `theme.rs` and `theme_entries.rs`.
//!
//! A [`ThemeDef`] is a theme as the reader sees one: the twelve colours, the two typefaces, the numbers the five components are selected by, and
//! the entries the theme functions read ([`diacraft_doc::ThemeEntries`]: the background, the colour and style variants, the lists of the effect
//! and connector schemes, and the reference app's line, fill and font extensions). [`theme_part_bytes`] makes the DrawingML part: `a:theme` with
//! `a:themeElements` (`a:clrScheme`, `a:fontScheme`, `a:fmtScheme`), `a:objectDefaults` and `a:extraClrSchemeLst`, and every extension of
//! R7 under the URI R7 gives it, each `vt:` element declaring its own namespace as the samples' do. **A part made here reads back as the
//! definition it was made from** (`write_then_read_gives_the_definition` in `tests/it/theme_write.rs`): the reader finds the extensions by the
//! child's name and not by the URI, the lists keep the lengths give, and `vt:lineStyles` holds one more entry
//! than the line list it goes with.
//!
//! **What it does not write.** A slot of a list that is `None` (a position the reader could not read) and a fill that is `Other` (a pattern, a
//! picture) have no text to write, so they are an error and not an invented entry. A [`ThemeDef`] holds no theme of Microsoft's: the ones DiaCraft
//! ships are its own (`docs/provenance.md`).
//!
//! **Bounded.** A definition is written by a loop over its lists and no recursion, and every list the reader keeps is capped
//! ([`diacraft_doc::MAX_THEME_ENTRIES`]): a definition longer than that is refused, since the reader would cut it.

use diacraft_color::{ColorTransform, Rgba};
use diacraft_doc::{
    ColourVariant, EffectEntry, FillEntry, FillProps, FontProps, GradientKind, LineEntry, LineEx, MAX_THEME_ENTRIES, MAX_THEME_MODIFIERS,
    MAX_THEME_STOPS, SchemeId, ShadowEntry, StyleLists, StyleVariant, ThemeBase, ThemeColour, ThemeEntries,
};
use diacraft_opc::xml::{El, Node, XmlWriteError, to_bytes};

use crate::theme::NS_VT;

/// The namespace of `a:` (DrawingML's main namespace, Summary).
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// The `uri` of each extension.
const URI_BKGND: &str = "{093E89EA-6996-430E-BFF9-83A9FAAAAB73}";
const URI_COLOUR_ID: &str = "{2703A3B3-D2E1-43D9-8057-6E9D74E0F44A}";
const URI_COLOUR_VARIANTS: &str = "{DDD2D869-C2EF-471E-B8FA-914AFA308C9F}";
const URI_FONT_ID: &str = "{B3C0788F-89CC-4CDB-A57D-0664CFDA821B}";
const URI_CONNECTOR: &str = "{1342405F-259F-4C95-8CDF-A9DCE2D418A2}";
const URI_EFFECT_ID: &str = "{27CD58D4-7086-4B73-B2AB-7AEBE2148A8C}";
const URI_CONNECTOR_ID: &str = "{C6430689-8E98-42EC-ADBF-087148533A3F}";
const URI_THEME_ID: &str = "{D75FF423-9257-4291-A4FE-1B2448832E17}";
const URI_LINE_STYLES: &str = "{6CAB99AB-0A78-4BAB-B597-526D62367CB4}";
const URI_FILL_STYLES: &str = "{56243398-1771-4C39-BF73-A5702A9C147F}";
const URI_FONT_STYLES: &str = "{EBE24D50-EC5C-4D6F-A1A3-C5F0A18B936A}";
const URI_STYLE_VARIANTS: &str = "{494CE47F-D151-47DC-95E8-85652EA8A67E}";

/// The twelve slots of `a:clrScheme`, in order.
const SLOTS: [&str; 12] = ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];

/// A theme, as the reader sees one (see the module documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDef {
    /// The theme's name: `a:theme`'s `name`, and the name of the colour, font and format schemes.
    pub name: String,
    /// The number the theme's five components are selected by: a `ThemeIndex`, `ColorSchemeIndex`, `FontSchemeIndex`,
    /// `EffectSchemeIndex` or `ConnectorSchemeIndex` of this number picks this part. Every component carries the same one, as the pages of
    /// real files hold one number in all five cells.
    pub scheme: SchemeId,
    /// `dk1`, `lt1`, `dk2`, `lt2`, `accent1` to `accent6`, `hlink`, `folHlink`.
    pub colours: [Rgba; 12],
    /// The Latin typeface of `a:majorFont` and of `a:minorFont`.
    pub major_font: String,
    pub minor_font: String,
    /// What the theme functions read: `background` (`vt:bkgnd`), the variants, the lists of both schemes, `vt:lineStyles`, `vt:fillStyles` and
    /// `vt:fontStylesGroup`.
    pub entries: ThemeEntries,
    /// `a:bgFillStyleLst` of the effect scheme and of the connector scheme: the reader does not read them, so they are
    /// not in `entries`. Three each.
    pub effect_background_fills: Vec<FillEntry>,
    pub connector_background_fills: Vec<FillEntry>,
}

/// Why a definition could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeWriteError {
    /// The definition holds something that has no text in a part: a slot the reader could not read, a fill that is not solid, gradient or empty,
    /// a list past the length the reader keeps, a colour or text XML cannot hold.
    Unwritable(&'static str),
    /// The tree could not be made into XML (a name or a character XML cannot hold).
    Xml(XmlWriteError),
}

impl std::fmt::Display for ThemeWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeWriteError::Unwritable(why) => write!(f, "the theme cannot be written: {why}"),
            ThemeWriteError::Xml(e) => write!(f, "the theme cannot be written: {e}"),
        }
    }
}

impl std::error::Error for ThemeWriteError {}

impl From<XmlWriteError> for ThemeWriteError {
    fn from(e: XmlWriteError) -> Self {
        ThemeWriteError::Xml(e)
    }
}

type R<T> = Result<T, ThemeWriteError>;

/// An element with these attributes and child elements.
fn el(name: &str, attrs: Vec<(&str, String)>, children: Vec<El>) -> El {
    El {
        name: name.to_owned(),
        attrs: attrs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        children: children.into_iter().map(Node::El).collect(),
    }
}

fn hex(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// `a:srgbClr`.
fn srgb(c: Rgba) -> El {
    el("a:srgbClr", vec![("val", hex(c))], Vec::new())
}

/// A whole number from a float in `lo..=hi`, 0 for NaN.
fn scaled(x: f64, factor: f64, lo: f64, hi: f64) -> i64 {
    let v = x * factor;
    if v.is_finite() { v.round().clamp(lo, hi) as i64 } else { 0 }
}

/// A list the reader keeps whole: at most [`MAX_THEME_ENTRIES`].
fn check_len(len: usize, what: &'static str) -> R<()> {
    if len > MAX_THEME_ENTRIES { Err(ThemeWriteError::Unwritable(what)) } else { Ok(()) }
}

/// The colour element with its modifiers (`a:srgbClr` or `a:schemeClr`, then `a:tint`, `a:shade` and the rest).
fn colour_el(c: &ThemeColour) -> R<El> {
    if c.modifiers.len() > MAX_THEME_MODIFIERS {
        return Err(ThemeWriteError::Unwritable("a colour with more modifiers than the reader keeps"));
    }
    let mut mods = Vec::with_capacity(c.modifiers.len());
    for m in &c.modifiers {
        let (ColorTransform::Tint(v)
        | ColorTransform::Shade(v)
        | ColorTransform::LumMod(v)
        | ColorTransform::LumOff(v)
        | ColorTransform::SatMod(v)
        | ColorTransform::SatOff(v)
        | ColorTransform::Alpha(v)) = *m
        else {
            return Err(ThemeWriteError::Unwritable("a colour modifier the theme functions do not apply"));
        };
        mods.push(el(&format!("a:{}", m.xml_name()), vec![("val", v.to_string())], Vec::new()));
    }
    Ok(match c.base {
        ThemeBase::Rgb(x) => el("a:srgbClr", vec![("val", hex(x))], mods),
        ThemeBase::Slot(s) => el("a:schemeClr", vec![("val", s.xml_name().to_owned())], mods),
    })
}

/// `a:solidFill`, `a:gradFill` or `a:noFill`.
fn fill_el(f: &FillEntry) -> R<El> {
    match f {
        FillEntry::Solid(c) => Ok(el("a:solidFill", Vec::new(), vec![colour_el(c)?])),
        FillEntry::NoFill => Ok(el("a:noFill", Vec::new(), Vec::new())),
        FillEntry::Other => Err(ThemeWriteError::Unwritable("a fill that is not solid, gradient or empty")),
        FillEntry::Gradient(g) => {
            if g.stops.len() > MAX_THEME_STOPS {
                return Err(ThemeWriteError::Unwritable("a gradient with more stops than the reader keeps"));
            }
            let mut stops = Vec::with_capacity(g.stops.len());
            for stop in &g.stops {
                stops.push(el("a:gs", vec![("pos", scaled(stop.pos, 100_000.0, 0.0, 100_000.0).to_string())], vec![colour_el(&stop.colour)?]));
            }
            let mut kids = vec![el("a:gsLst", Vec::new(), stops)];
            match g.kind {
                GradientKind::Linear { degrees } => {
                    kids.push(el(
                        "a:lin",
                        vec![("ang", scaled(degrees, 60_000.0, -21_600_000.0, 21_600_000.0).to_string()), ("scaled", "0".to_owned())],
                        Vec::new(),
                    ));
                }
                GradientKind::Circle | GradientKind::Rect | GradientKind::Shape => {
                    let path = match g.kind {
                        GradientKind::Circle => "circle",
                        GradientKind::Rect => "rect",
                        _ => "shape",
                    };
                    let centre = ["l", "t", "r", "b"].map(|side| (side, "50000".to_owned())).to_vec();
                    kids.push(el("a:path", vec![("path", path.to_owned())], vec![el("a:fillToRect", centre, Vec::new())]));
                }
                GradientKind::Unstated => {}
            }
            let mut attrs = Vec::new();
            if let Some(rotate) = g.rotate_with_shape {
                attrs.push(("rotWithShape", u8::from(rotate).to_string()));
            }
            Ok(el("a:gradFill", attrs, kids))
        }
    }
}

/// An `a:ln` of `a:lnStyleLst`.
fn line_el(l: &LineEntry) -> R<El> {
    let mut attrs = Vec::new();
    if let Some(w) = l.width_emu {
        attrs.push(("w", w.to_string()));
    }
    if let Some(cap) = &l.cap {
        attrs.push(("cap", cap.clone()));
    }
    if let Some(cmpd) = &l.compound {
        attrs.push(("cmpd", cmpd.clone()));
    }
    attrs.push(("algn", "ctr".to_owned()));
    let mut kids = Vec::new();
    if let Some(fill) = &l.fill {
        kids.push(fill_el(fill)?);
    }
    kids.push(el("a:prstDash", vec![("val", l.dash.clone().unwrap_or_else(|| "solid".to_owned()))], Vec::new()));
    kids.push(el("a:round", Vec::new(), Vec::new()));
    Ok(el("a:ln", attrs, kids))
}

/// An `a:effectStyle`: an empty `a:effectLst`, or one with an outer shadow.
fn effect_el(e: &EffectEntry) -> R<El> {
    let mut list = Vec::new();
    if let Some(s) = &e.outer_shadow {
        list.push(shadow_el(s)?);
    }
    Ok(el("a:effectStyle", Vec::new(), vec![el("a:effectLst", Vec::new(), list)]))
}

fn shadow_el(s: &ShadowEntry) -> R<El> {
    let mut attrs = vec![("blurRad", s.blur_emu.to_string()), ("dist", s.dist_emu.to_string()), ("dir", s.dir.to_string())];
    for (name, scale) in [("sx", s.scale_x), ("sy", s.scale_y)] {
        // The reader reads a scale it is not given as 1.
        if (scale - 1.0).abs() > f64::EPSILON {
            attrs.push((name, scaled(scale, 100_000.0, 0.0, 1.0e9).to_string()));
        }
    }
    attrs.push(("algn", "ctr".to_owned()));
    attrs.push(("rotWithShape", "0".to_owned()));
    Ok(el("a:outerShdw", attrs, vec![colour_el(&s.colour)?]))
}

/// The four lists of a scheme, under the names `a:fmtScheme` and `vt:fmtConnectorScheme` give them.
fn style_lists(lists: &StyleLists, background: &[FillEntry]) -> R<Vec<El>> {
    check_len(lists.fills.len(), "a fill list longer than the reader keeps")?;
    check_len(lists.lines.len(), "a line list longer than the reader keeps")?;
    check_len(lists.effects.len(), "an effect list longer than the reader keeps")?;
    check_len(background.len(), "a background fill list longer than the reader keeps")?;
    let mut fills = Vec::with_capacity(lists.fills.len());
    for fill in &lists.fills {
        fills.push(fill_el(fill.as_ref().ok_or(ThemeWriteError::Unwritable("a fill the reader could not read"))?)?);
    }
    let mut lines = Vec::with_capacity(lists.lines.len());
    for line in &lists.lines {
        lines.push(line_el(line.as_ref().ok_or(ThemeWriteError::Unwritable("a line the reader could not read"))?)?);
    }
    let mut effects = Vec::with_capacity(lists.effects.len());
    for effect in &lists.effects {
        effects.push(effect_el(effect.as_ref().ok_or(ThemeWriteError::Unwritable("an effect the reader could not read"))?)?);
    }
    let backgrounds = background.iter().map(fill_el).collect::<R<Vec<El>>>()?;
    Ok(vec![
        el("a:fillStyleLst", Vec::new(), fills),
        el("a:lnStyleLst", Vec::new(), lines),
        el("a:effectStyleLst", Vec::new(), effects),
        el("a:bgFillStyleLst", Vec::new(), backgrounds),
    ])
}

// ---- the VSDX extensions -----------------------------------------------------------------------------------------------------

/// `<a:ext uri="..."><child xmlns:vt="..."/></a:ext>`: the child declares the namespace itself, as the samples' do.
fn ext(uri: &str, mut child: El) -> El {
    child.attrs.insert(0, ("xmlns:vt".to_owned(), NS_VT.to_owned()));
    el("a:ext", vec![("uri", uri.to_owned())], vec![child])
}

fn ext_list(exts: Vec<El>) -> El {
    el("a:extLst", Vec::new(), exts)
}

fn scheme_id(id: &SchemeId) -> El {
    let mut attrs = vec![("schemeEnum", id.scheme_enum.to_string())];
    if let Some(guid) = &id.scheme_guid {
        attrs.push(("schemeGUID", guid.to_string()));
    }
    el("vt:schemeID", attrs, Vec::new())
}

fn colour_variant_el(v: &ColourVariant) -> R<El> {
    let attrs = if v.monotone { vec![("monotone", "1".to_owned())] } else { Vec::new() };
    let mut kids = Vec::with_capacity(7);
    for (i, colour) in v.colours.iter().enumerate() {
        let colour = colour.ok_or(ThemeWriteError::Unwritable("a variant colour the reader could not read"))?;
        kids.push(el(&format!("vt:varColor{}", i + 1), Vec::new(), vec![srgb(colour)]));
    }
    Ok(el("vt:variationClrScheme", attrs, kids))
}

fn style_variant_el(v: &StyleVariant) -> R<El> {
    check_len(v.styles.len(), "a style variant with more styles than the reader keeps")?;
    let mut attrs = Vec::new();
    if let Some(e) = v.embellishment {
        attrs.push(("embellishment", e.to_string()));
    }
    let number = |n: Option<u32>| n.map(|n| n.to_string()).ok_or(ThemeWriteError::Unwritable("a variant style with a missing index"));
    let mut kids = Vec::with_capacity(v.styles.len());
    for s in &v.styles {
        let ends = vec![
            ("fillIdx", number(s.fill_idx)?),
            ("lineIdx", number(s.line_idx)?),
            ("effectIdx", number(s.effect_idx)?),
            ("fontIdx", number(s.font_idx)?),
        ];
        kids.push(el("vt:varStyle", ends, Vec::new()));
    }
    Ok(el("vt:variationStyleScheme", attrs, kids))
}

fn line_ex_list(name: &str, list: &[Option<LineEx>]) -> R<El> {
    check_len(list.len(), "a line extension list longer than the reader keeps")?;
    let mut kids = Vec::with_capacity(list.len());
    for ex in list {
        let ex = ex.as_ref().ok_or(ThemeWriteError::Unwritable("a line extension the reader could not read"))?;
        let mut attrs = Vec::new();
        if let Some(r) = ex.rounding_emu {
            attrs.push(("rndg", r.to_string()));
        }
        for (name, value) in [("start", ex.start), ("startSize", ex.start_size), ("end", ex.end), ("endSize", ex.end_size), ("pattern", ex.pattern)] {
            if let Some(v) = value {
                attrs.push((name, v.to_string()));
            }
        }
        kids.push(el("vt:lineStyle", Vec::new(), vec![el("vt:lineEx", attrs, Vec::new())]));
    }
    Ok(el(name, Vec::new(), kids))
}

fn fill_props_list(list: &[Option<FillProps>]) -> R<El> {
    check_len(list.len(), "a fill property list longer than the reader keeps")?;
    let mut kids = Vec::with_capacity(list.len());
    for props in list {
        let props = props.as_ref().ok_or(ThemeWriteError::Unwritable("a fill property the reader could not read"))?;
        let attrs = props.pattern.map(|p| vec![("pattern", p.to_string())]).unwrap_or_default();
        kids.push(el("vt:fillProps", attrs, Vec::new()));
    }
    Ok(el("vt:fillStyles", Vec::new(), kids))
}

fn font_props_list(name: &str, list: &[Option<FontProps>]) -> R<El> {
    check_len(list.len(), "a font property list longer than the reader keeps")?;
    let mut kids = Vec::with_capacity(list.len());
    for props in list {
        let props = props.as_ref().ok_or(ThemeWriteError::Unwritable("a font property the reader could not read"))?;
        let attrs = vec![("style", props.style.unwrap_or(0).to_string())];
        let colour = match &props.colour {
            Some(c) => vec![el("vt:color", Vec::new(), vec![colour_el(c)?])],
            None => Vec::new(),
        };
        kids.push(el("vt:fontProps", attrs, colour));
    }
    Ok(el(name, Vec::new(), kids))
}

/// The extensions of `a:clrScheme`: `vt:bkgnd`, `vt:schemeID` and `vt:variationClrSchemeLst`.
fn colour_extensions(def: &ThemeDef) -> R<El> {
    let mut exts = Vec::new();
    if let Some(background) = def.entries.background {
        exts.push(ext(URI_BKGND, el("vt:bkgnd", Vec::new(), vec![srgb(background)])));
    }
    exts.push(ext(URI_COLOUR_ID, scheme_id(&def.scheme)));
    if !def.entries.colour_variants.is_empty() {
        check_len(def.entries.colour_variants.len(), "more colour variants than the reader keeps")?;
        let mut variants = Vec::with_capacity(def.entries.colour_variants.len());
        for v in &def.entries.colour_variants {
            variants.push(colour_variant_el(v.as_ref().ok_or(ThemeWriteError::Unwritable("a colour variant the reader could not read"))?)?);
        }
        exts.push(ext(URI_COLOUR_VARIANTS, el("vt:variationClrSchemeLst", Vec::new(), variants)));
    }
    Ok(ext_list(exts))
}

/// The extensions of `a:themeElements`.
fn element_extensions(def: &ThemeDef) -> R<El> {
    let entries = &def.entries;
    let connector_scheme =
        el("vt:fmtConnectorScheme", vec![("name", def.name.clone())], style_lists(&entries.connector, &def.connector_background_fills)?);
    let mut exts = vec![
        ext(URI_CONNECTOR, connector_scheme),
        ext(URI_EFFECT_ID, el("vt:fmtSchemeEx", Vec::new(), vec![scheme_id(&def.scheme)])),
        ext(URI_CONNECTOR_ID, el("vt:fmtConnectorSchemeEx", Vec::new(), vec![scheme_id(&def.scheme)])),
        ext(URI_THEME_ID, el("vt:themeScheme", Vec::new(), vec![scheme_id(&def.scheme)])),
    ];
    let line_styles =
        vec![line_ex_list("vt:fmtConnectorSchemeLineStyles", &entries.connector_line_ex)?, line_ex_list("vt:fmtSchemeLineStyles", &entries.line_ex)?];
    exts.push(ext(URI_LINE_STYLES, el("vt:lineStyles", Vec::new(), line_styles)));
    exts.push(ext(URI_FILL_STYLES, fill_props_list(&entries.fill_props)?));
    let fonts = vec![font_props_list("vt:connectorFontStyles", &entries.connector_fonts)?, font_props_list("vt:fontStyles", &entries.fonts)?];
    exts.push(ext(URI_FONT_STYLES, el("vt:fontStylesGroup", Vec::new(), fonts)));
    if !entries.style_variants.is_empty() {
        check_len(entries.style_variants.len(), "more style variants than the reader keeps")?;
        let mut variants = Vec::with_capacity(entries.style_variants.len());
        for v in &entries.style_variants {
            variants.push(style_variant_el(v.as_ref().ok_or(ThemeWriteError::Unwritable("a style variant the reader could not read"))?)?);
        }
        exts.push(ext(URI_STYLE_VARIANTS, el("vt:variationStyleSchemeLst", Vec::new(), variants)));
    }
    Ok(ext_list(exts))
}

/// A font scheme's major or minor font: `a:latin`, and the empty `a:ea` and `a:cs` the schema requires (every sample's are empty).
fn font_el(name: &str, latin: &str) -> El {
    let face = |tag: &str, typeface: &str| el(tag, vec![("typeface", typeface.to_owned())], Vec::new());
    el(name, Vec::new(), vec![face("a:latin", latin), face("a:ea", ""), face("a:cs", "")])
}

/// The theme part for `def`: a UTF-8 document with the declaration the samples' theme parts start with.
pub fn theme_part_bytes(def: &ThemeDef) -> Result<Vec<u8>, ThemeWriteError> {
    let mut colour_kids: Vec<El> = Vec::with_capacity(13);
    for (name, colour) in SLOTS.iter().zip(def.colours) {
        colour_kids.push(el(&format!("a:{name}"), Vec::new(), vec![srgb(colour)]));
    }
    colour_kids.push(colour_extensions(def)?);
    let colour_scheme = el("a:clrScheme", vec![("name", def.name.clone())], colour_kids);

    let mut font_scheme =
        el("a:fontScheme", vec![("name", def.name.clone())], vec![font_el("a:majorFont", &def.major_font), font_el("a:minorFont", &def.minor_font)]);
    font_scheme.children.push(Node::El(ext_list(vec![ext(URI_FONT_ID, scheme_id(&def.scheme))])));

    let format_scheme = el("a:fmtScheme", vec![("name", def.name.clone())], style_lists(&def.entries.effect, &def.effect_background_fills)?);

    let elements = el("a:themeElements", Vec::new(), vec![colour_scheme, font_scheme, format_scheme, element_extensions(def)?]);
    let root = el(
        "a:theme",
        vec![("xmlns:a", NS_A.to_owned()), ("name", def.name.clone())],
        vec![elements, el("a:objectDefaults", Vec::new(), Vec::new()), el("a:extraClrSchemeLst", Vec::new(), Vec::new())],
    );
    Ok(to_bytes(&root, &[], &[], true)?)
}
