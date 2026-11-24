//! What the theme functions read from a theme part besides its colours and scheme numbers:
//! `vt:bkgnd`, the colour and style variants, the lists of `a:fmtScheme` and `vt:fmtConnectorScheme`, and the reference app's line, fill and font
//! extensions. The bytes stay verbatim in the model; this is a view of them (`diacraft_doc::ThemeEntries`).
//!
//! **Found by name, as R7 asks:** each extension is an `a:ext` in an `a:extLst`, and its child is found by its element name in the
//! namespace of `vt:`, never by the `uri`. **A slot that cannot be read is left unread and counted** ([`Tally::unread`]); a list position
//! that cannot be read keeps its place (`None`), so that an entry number still names the entry it names. **Every list is capped** at
//! `MAX_THEME_ENTRIES` (stops at `MAX_THEME_STOPS`, modifiers at `MAX_THEME_MODIFIERS`) and the cut is counted ([`Tally::capped`]), so a part
//! of 4 MiB cannot make a view larger than itself. Nothing recurses: the tree is walked a fixed number of levels.

use std::cell::Cell;

use diacraft_color::{ColorTransform, Rgba, SchemeSlot};
use diacraft_doc::{
    ColourVariant, EffectEntry, FillEntry, FillProps, FontProps, GradientEntry, GradientKind, LineEntry, LineEx, MAX_THEME_ENTRIES,
    MAX_THEME_MODIFIERS, MAX_THEME_STOPS, ShadowEntry, StyleLists, StyleVariant, ThemeBase, ThemeColour, ThemeEntries, ThemeStop, VarStyle,
};
use diacraft_opc::xml::El;

use crate::ns::Scope;
use crate::theme::{NS_VT, NS_VT_PLACEHOLDER, colour, hex6};

/// How many `a:ext` children of one holder are looked through: a real holder has up to eight, and a hostile one a million.
const MAX_EXT_CHILDREN: usize = 4096;

/// What reading left undone: counted, and warned of by the caller. Counters are cells so that a reader can hold the tally in two places at once
/// (a list reader and the closure that reads each of its items).
#[derive(Debug, Default)]
pub(crate) struct Tally {
    unread: Cell<usize>,
    capped: Cell<usize>,
}

impl Tally {
    /// Slots (a list position, a variant, `vt:bkgnd`) that were there and could not be read.
    pub(crate) fn unread(&self) -> usize {
        self.unread.get()
    }

    /// Lists, stop lists or modifier lists that were cut at their cap.
    pub(crate) fn capped(&self) -> usize {
        self.capped.get()
    }

    fn unread_if(&self, is_none: bool) {
        self.unread.set(self.unread.get().saturating_add(usize::from(is_none)));
    }

    fn cut(&self) {
        self.capped.set(self.capped.get().saturating_add(1));
    }
}

/// The children of the `a:ext`s of `holder`'s `a:extLst`s that are in the vocabulary of `vt:`, in order, up to [`MAX_EXT_CHILDREN`].
fn vt_children<'a>(holder: &'a El, holder_scope: &Scope) -> Vec<&'a El> {
    let mut out = Vec::new();
    for list in holder.children_named("extLst") {
        let lsc = holder_scope.enter(list);
        for ext in list.children_named("ext") {
            let xsc = lsc.enter(ext);
            for child in ext.elements() {
                if out.len() >= MAX_EXT_CHILDREN {
                    return out;
                }
                let csc = xsc.enter(child);
                if matches!(csc.uri_of_element(child).as_deref(), Some(NS_VT | NS_VT_PLACEHOLDER)) {
                    out.push(child);
                }
            }
        }
    }
    out
}

fn named<'a>(children: &[&'a El], local: &str) -> Option<&'a El> {
    children.iter().copied().find(|c| c.is(local))
}

/// The entries of `themeElements` (`elements_scope` is the scope inside it); the colour extensions are in its `a:clrScheme`.
pub(crate) fn read_entries(elements: &El, elements_scope: &Scope, tally: &Tally) -> ThemeEntries {
    let mut entries = ThemeEntries::default();
    if let Some(scheme) = elements.child("clrScheme") {
        let ssc = elements_scope.enter(scheme);
        let ext = vt_children(scheme, &ssc);
        if let Some(bkgnd) = named(&ext, "bkgnd") {
            entries.background = colour(bkgnd);
            tally.unread_if(entries.background.is_none());
        }
        if let Some(list) = named(&ext, "variationClrSchemeLst") {
            entries.colour_variants = capped(list.children_named("variationClrScheme"), tally, |v| colour_variant(v, tally));
        }
    }
    if let Some(fmt) = elements.child("fmtScheme") {
        entries.effect = style_lists(fmt, tally);
    }
    let ext = vt_children(elements, elements_scope);
    if let Some(connector) = named(&ext, "fmtConnectorScheme") {
        entries.connector = style_lists(connector, tally);
    }
    if let Some(styles) = named(&ext, "lineStyles") {
        entries.connector_line_ex = line_ex_list(styles, "fmtConnectorSchemeLineStyles", tally);
        entries.line_ex = line_ex_list(styles, "fmtSchemeLineStyles", tally);
    }
    if let Some(fills) = named(&ext, "fillStyles") {
        entries.fill_props = capped(fills.children_named("fillProps"), tally, |p| Some(FillProps { pattern: opt_u32(p, "pattern")? }));
    }
    if let Some(group) = named(&ext, "fontStylesGroup") {
        entries.connector_fonts = font_list(group, "connectorFontStyles", tally);
        entries.fonts = font_list(group, "fontStyles", tally);
    }
    if let Some(list) = named(&ext, "variationStyleSchemeLst") {
        entries.style_variants = capped(list.children_named("variationStyleScheme"), tally, |v| style_variant(v, tally));
    }
    entries
}

/// Reads up to [`MAX_THEME_ENTRIES`] items with `read`, keeping the place of one that cannot be read, and counting the unread and the cut.
fn capped<'a, T>(items: impl Iterator<Item = &'a El>, tally: &Tally, mut read: impl FnMut(&'a El) -> Option<T>) -> Vec<Option<T>> {
    let mut out = Vec::new();
    for item in items {
        if out.len() >= MAX_THEME_ENTRIES {
            tally.cut();
            break;
        }
        let read = read(item);
        tally.unread_if(read.is_none());
        out.push(read);
    }
    out
}

// ---- numbers ----------------------------------------------------------------------------------------------------------------

/// An attribute that may be absent (`Some(None)`) but, when there, must be a plain whole number: `None` for a bad one.
fn opt_u32(el: &El, name: &str) -> Option<Option<u32>> {
    match el.attr(name) {
        None => Some(None),
        Some(t) => plain(t).and_then(|n| u32::try_from(n).ok()).map(Some),
    }
}

fn opt_i64(el: &El, name: &str) -> Option<Option<i64>> {
    match el.attr(name) {
        None => Some(None),
        Some(t) => signed(t).map(Some),
    }
}

/// Digits only, no sign and no blank, up to 18 of them.
fn plain(text: &str) -> Option<i64> {
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn signed(text: &str) -> Option<i64> {
    match text.strip_prefix('-') {
        Some(rest) => plain(rest).map(|n| -n),
        None => plain(text),
    }
}

/// A Boolean as XML Schema writes it.
fn boolean(text: &str) -> Option<bool> {
    match text {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

// ---- colours ----------------------------------------------------------------------------------------------------------------

/// The seven modifiers that applies, by the name a part spells them.
fn modifier(name: &str, value: i32) -> Option<ColorTransform> {
    Some(match name {
        "tint" => ColorTransform::Tint(value),
        "shade" => ColorTransform::Shade(value),
        "lumMod" => ColorTransform::LumMod(value),
        "lumOff" => ColorTransform::LumOff(value),
        "satMod" => ColorTransform::SatMod(value),
        "satOff" => ColorTransform::SatOff(value),
        "alpha" => ColorTransform::Alpha(value),
        _ => return None,
    })
}

/// A colour element (`a:srgbClr`, `a:sysClr`, `a:schemeClr`) with its modifiers. `None` for any other element, and for one whose colour or
/// whose modifier's value cannot be read: what the colour would be is not guessed.
fn colour_element(el: &El, tally: &Tally) -> Option<ThemeColour> {
    let base = if el.is("srgbClr") {
        ThemeBase::Rgb(hex6(el.attr("val")?)?)
    } else if el.is("sysClr") {
        ThemeBase::Rgb(sys_colour(el)?)
    } else if el.is("schemeClr") {
        ThemeBase::Slot(SchemeSlot::from_xml(el.attr("val")?)?)
    } else {
        return None;
    };
    let mut modifiers = Vec::new();
    for m in el.elements() {
        let Some(kind) = ["tint", "shade", "lumMod", "lumOff", "satMod", "satOff", "alpha"].into_iter().find(|n| m.is(n)) else { continue };
        let value = i32::try_from(signed(m.attr("val")?)?).ok()?;
        if modifiers.len() >= MAX_THEME_MODIFIERS {
            tally.cut();
            break;
        }
        modifiers.extend(modifier(kind, value));
    }
    Some(ThemeColour { base, modifiers })
}

/// `a:sysClr`'s colour: its `lastClr`, else the two system colours a theme names.
fn sys_colour(el: &El) -> Option<Rgba> {
    if let Some(rgb) = el.attr("lastClr").and_then(hex6) {
        return Some(rgb);
    }
    match el.attr("val")? {
        "windowText" => Some(Rgba::BLACK),
        "window" => Some(Rgba::WHITE),
        _ => None,
    }
}

/// The colour a container (`a:solidFill`, `a:gs`, `vt:color`) holds: its first element child, which must be a colour.
fn colour_in(container: &El, tally: &Tally) -> Option<ThemeColour> {
    colour_element(container.elements().next()?, tally)
}

// ---- the lists of a scheme --------------------------------------------------------------------------------------------------

fn style_lists(scheme: &El, tally: &Tally) -> StyleLists {
    let mut lists = StyleLists::default();
    if let Some(list) = scheme.child("fillStyleLst") {
        lists.fills = capped(list.elements(), tally, |e| fill_entry(e, tally));
    }
    if let Some(list) = scheme.child("lnStyleLst") {
        lists.lines = capped(list.children_named("ln"), tally, |e| line_entry(e, tally));
    }
    if let Some(list) = scheme.child("effectStyleLst") {
        lists.effects = capped(list.children_named("effectStyle"), tally, |e| effect_entry(e, tally));
    }
    lists
}

/// A fill: `a:solidFill`, `a:gradFill`, `a:noFill`, and the rest as `Other` (MS-VSDX says to ignore them).
fn fill_entry(el: &El, tally: &Tally) -> Option<FillEntry> {
    if el.is("solidFill") {
        return colour_in(el, tally).map(FillEntry::Solid);
    }
    if el.is("noFill") {
        return Some(FillEntry::NoFill);
    }
    if !el.is("gradFill") {
        return Some(FillEntry::Other);
    }
    let mut stops = Vec::new();
    for gs in el.child("gsLst")?.children_named("gs") {
        if stops.len() >= MAX_THEME_STOPS {
            tally.cut();
            break;
        }
        let pos = plain(gs.attr("pos")?)? as f64 / 100_000.0;
        stops.push(ThemeStop { pos, colour: colour_in(gs, tally)? });
    }
    let kind = if let Some(lin) = el.child("lin") {
        GradientKind::Linear { degrees: opt_i64(lin, "ang")?.unwrap_or(0) as f64 / 60_000.0 }
    } else if let Some(path) = el.child("path") {
        match path.attr("path") {
            Some("circle") => GradientKind::Circle,
            Some("rect") => GradientKind::Rect,
            Some("shape") => GradientKind::Shape,
            _ => GradientKind::Unstated,
        }
    } else {
        GradientKind::Unstated
    };
    Some(FillEntry::Gradient(GradientEntry { stops, kind, rotate_with_shape: el.attr("rotWithShape").and_then(boolean) }))
}

/// An `a:ln` of the line list.
fn line_entry(el: &El, tally: &Tally) -> Option<LineEntry> {
    let fill = match el.elements().find(|c| ["solidFill", "gradFill", "noFill", "pattFill"].iter().any(|n| c.is(n))) {
        Some(f) => Some(fill_entry(f, tally)?),
        None => None,
    };
    Some(LineEntry {
        width_emu: opt_i64(el, "w")?,
        cap: el.attr("cap").map(str::to_owned),
        compound: el.attr("cmpd").map(str::to_owned),
        dash: el.child("prstDash").and_then(|d| d.attr("val")).map(str::to_owned),
        fill,
    })
}

/// An `a:effectStyle`: its outer shadow, if it has one.
fn effect_entry(el: &El, tally: &Tally) -> Option<EffectEntry> {
    let Some(list) = el.child("effectLst") else { return Some(EffectEntry::default()) };
    let outer_shadow = match list.child("outerShdw") {
        Some(s) => {
            let scale = |name: &str| opt_i64(s, name).map(|v| v.map_or(1.0, |n| n as f64 / 100_000.0));
            Some(ShadowEntry {
                blur_emu: opt_i64(s, "blurRad")?.unwrap_or(0),
                dist_emu: opt_i64(s, "dist")?.unwrap_or(0),
                dir: opt_i64(s, "dir")?.unwrap_or(0),
                scale_x: scale("sx")?,
                scale_y: scale("sy")?,
                colour: colour_in(s, tally)?,
            })
        }
        None => None,
    };
    Some(EffectEntry { outer_shadow, has_inner_shadow: list.child("innerShdw").is_some() })
}

// ---- the VSDX extensions -----------------------------------------------------------------------------------------------------

/// The `vt:lineEx`s of `vt:lineStyles`' child `which` (`vt:fmtSchemeLineStyles` or `vt:fmtConnectorSchemeLineStyles`).
fn line_ex_list(styles: &El, which: &str, tally: &Tally) -> Vec<Option<LineEx>> {
    let Some(list) = styles.child(which) else { return Vec::new() };
    capped(list.children_named("lineStyle"), tally, |style| {
        let ex = style.child("lineEx")?;
        Some(LineEx {
            rounding_emu: opt_i64(ex, "rndg")?,
            start: opt_u32(ex, "start")?,
            start_size: opt_u32(ex, "startSize")?,
            end: opt_u32(ex, "end")?,
            end_size: opt_u32(ex, "endSize")?,
            pattern: opt_u32(ex, "pattern")?,
        })
    })
}

/// The `vt:fontProps` of `vt:fontStylesGroup`'s child `which`.
fn font_list(group: &El, which: &str, tally: &Tally) -> Vec<Option<FontProps>> {
    let Some(list) = group.child(which) else { return Vec::new() };
    capped(list.children_named("fontProps"), tally, |props| {
        let colour = match props.child("color") {
            Some(c) => Some(colour_in(c, tally)?),
            None => None,
        };
        Some(FontProps { style: opt_u32(props, "style")?, colour })
    })
}

/// A `vt:variationClrScheme`: `monotone` (false where absent) and `vt:varColor1` to `vt:varColor7`, each an `a:srgbClr` or `a:sysClr` (a
/// colour that is not one of those is left unread, and the variant counts as read).
fn colour_variant(el: &El, tally: &Tally) -> Option<ColourVariant> {
    let monotone = match el.attr("monotone") {
        None => false,
        Some(t) => boolean(t)?,
    };
    let mut colours: [Option<Rgba>; 7] = [None; 7];
    for (i, slot) in colours.iter_mut().enumerate() {
        *slot = el.child(&format!("varColor{}", i + 1)).and_then(colour);
        tally.unread_if(slot.is_none());
    }
    Some(ColourVariant { monotone, colours })
}

/// A `vt:variationStyleScheme` and its `vt:varStyle`s.
fn style_variant(el: &El, tally: &Tally) -> Option<StyleVariant> {
    let embellishment = opt_u32(el, "embellishment")?;
    let styles = capped(el.children_named("varStyle"), tally, |s| {
        Some(VarStyle {
            fill_idx: opt_u32(s, "fillIdx")?,
            line_idx: opt_u32(s, "lineIdx")?,
            effect_idx: opt_u32(s, "effectIdx")?,
            font_idx: opt_u32(s, "fontIdx")?,
        })
    });
    // A `varStyle` that cannot be read keeps its place as a default one: an index into the variant still lands on the entry it names.
    Some(StyleVariant { embellishment, styles: styles.into_iter().map(Option::unwrap_or_default).collect() })
}
