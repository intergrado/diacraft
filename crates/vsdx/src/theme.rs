//! A DrawingML theme part: the bytes, kept as they are, and what a viewer needs from them: the twelve
//! colours one by one, two typefaces, and the `schemeEnum` of each of the five components.
//!
//! The part's bytes are the model's (`Theme::bytes`) and a write at the bytes level gives them back unchanged. What is read from them is
//! the colour scheme (`a:clrScheme`: `dk1`, `lt1`, `dk2`, `lt2`, `accent1` to `accent6`, `hlink`, `folHlink`, each an `a:srgbClr` or an
//! `a:sysClr`), the Latin typeface of the major and minor fonts, and the `vt:schemeID` of the colour scheme, the font scheme, the effect scheme
//! (`vt:fmtSchemeEx`), the connector scheme (`vt:fmtConnectorSchemeEx`) and the theme (`vt:themeScheme`), **found by the child element's name
//! in the namespace of the `vt:`** and not by the extension's URI. A part that is not XML we accept, or has no such element, is still
//! a theme part and is kept, with nothing read from it. **A colour is read slot by slot** (`Theme::slots`): `Theme::colors` is `Some` only when
//! all twelve were read, as no colour is invented, but one slot that is not a colour we read no longer takes the other
//! eleven with it. The VSDX extensions of R7 that are not scheme ids are left in the bytes: selecting among them is `calc`'s and `render`'s, from
//! the bytes.

use std::sync::Arc;

use diacraft_color::{ColorScheme, Rgba};
use diacraft_doc::{SchemeId, Theme};
use diacraft_opc::xml::{El, XmlLimits};

use crate::ns::Scope;
use crate::theme_entries::{Tally, read_entries};

/// The twelve slots of a colour scheme, in the order `ColorScheme::colors` holds them.
const SLOTS: [&str; 12] = ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];

/// The content type of a theme part: what a part with no relationship is known as a theme by.
pub(crate) const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";

/// The conventional place of a theme part, relative to the document part (a folder `theme` beside it, in every sample): where a part is looked for when no
/// relationship and no content type says which part is a theme.
pub(crate) const CONVENTIONAL_THEME: &str = "theme/theme1.xml";

/// The namespace R7 calls `vt:`, and the placeholder MS-VSDX gives it (which no file uses).
pub(crate) const NS_VT: &str = "http://schemas.microsoft.com/office/visio/2012/theme"; // brand-ok: a namespace the format defines
pub(crate) const NS_VT_PLACEHOLDER: &str = "http://visThemeSchemaUri";

/// What reading a theme part gave: the part as the model holds it, why nothing could be read if so, and what was left undone.
pub struct ThemeRead {
    pub theme: Theme,
    /// Why nothing could be read from the bytes (they are kept either way).
    pub problem: Option<&'static str>,
    /// Entries, variants and list positions that were there and could not be read, left unread.
    pub unread: usize,
    /// Lists cut at their cap (`diacraft_doc::MAX_THEME_ENTRIES` and its kin).
    pub capped: usize,
}

/// The theme part `part` with these `bytes`: what the reader took from them, for a caller that has no package (a test, a tool).
pub fn read_theme_part(part: &str, bytes: &[u8]) -> Theme {
    read_theme(part, bytes, &XmlLimits::default()).theme
}

/// The theme part `part` with these `bytes`, **and what the read could not do with them** (`ThemeRead::problem`, `unread`, `capped`): for a caller that has no package and
/// says what it could not read, as a package read does (`theme_unreadable`, `theme_entry_unreadable`).
pub fn read_theme_part_reported(part: &str, bytes: &[u8]) -> ThemeRead {
    read_theme(part, bytes, &XmlLimits::default())
}

/// The theme part `part` with these `bytes`.
pub(crate) fn read_theme(part: &str, bytes: &[u8], limits: &XmlLimits) -> ThemeRead {
    let mut theme = Theme { part: Arc::from(part), bytes: Arc::from(bytes), ..Theme::default() };
    let done =
        |theme: Theme, problem: Option<&'static str>, tally: &Tally| ThemeRead { theme, problem, unread: tally.unread(), capped: tally.capped() };
    let tally = Tally::default();
    let root = match crate::work::parse(bytes, limits) {
        Ok(root) => root,
        Err(e) => return done(theme, Some(e.class()), &tally),
    };
    if !root.is("theme") {
        return done(theme, Some("theme_unexpected_root"), &tally);
    }
    let sc = Scope::empty().enter(&root);
    let Some(elements) = root.child("themeElements") else {
        return done(theme, None, &tally);
    };
    let esc = sc.enter(elements);
    if let Some(scheme) = elements.child("clrScheme") {
        theme.slots = colour_slots(scheme);
        theme.colors = colour_scheme(scheme, &theme.slots);
        theme.color_scheme_id = scheme_id(scheme, &esc.enter(scheme), None);
    }
    if let Some(fonts) = elements.child("fontScheme") {
        theme.major_font = typeface(fonts, "majorFont");
        theme.minor_font = typeface(fonts, "minorFont");
        theme.font_scheme_id = scheme_id(fonts, &esc.enter(fonts), None);
    }
    theme.effect_scheme_id = scheme_id(elements, &esc, Some("fmtSchemeEx"));
    theme.connector_scheme_id = scheme_id(elements, &esc, Some("fmtConnectorSchemeEx"));
    theme.theme_scheme_id = scheme_id(elements, &esc, Some("themeScheme"));
    theme.entries = read_entries(elements, &esc, &tally);
    done(theme, None, &tally)
}

/// The `vt:schemeID` in the extension list of `holder`: directly in an `a:ext`, or (for the three components of `a:themeElements`) in the child
/// of an `a:ext` called `via`. Found by element name and namespace, the first one whose `schemeEnum` is a plain number.
fn scheme_id(holder: &El, holder_scope: &Scope, via: Option<&str>) -> Option<SchemeId> {
    for list in holder.children_named("extLst") {
        let lsc = holder_scope.enter(list);
        for ext in list.children_named("ext") {
            let xsc = lsc.enter(ext);
            let (container, csc) = match via {
                None => (ext, xsc),
                Some(name) => {
                    let Some(found) = ext.child(name) else { continue };
                    let csc = xsc.enter(found);
                    (found, csc)
                }
            };
            for id in container.children_named("schemeID") {
                let isc = csc.enter(id);
                let uri = isc.uri_of_element(id);
                if !matches!(uri.as_deref(), Some(NS_VT | NS_VT_PLACEHOLDER)) {
                    continue;
                }
                let Some(scheme_enum) = id.attr("schemeEnum").and_then(plain_u32) else { continue };
                return Some(SchemeId { scheme_enum, scheme_guid: id.attr("schemeGUID").map(Arc::from) });
            }
        }
    }
    None
}

/// A number that is all digits (no sign, no blanks) and fits `u32`.
fn plain_u32(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Each of the twelve slots of an `a:clrScheme`: the colour it holds, or `None`.
fn colour_slots(scheme: &El) -> [Option<Rgba>; 12] {
    let mut slots = [None; 12];
    for (slot, name) in slots.iter_mut().zip(SLOTS) {
        *slot = scheme.child(name).and_then(colour);
    }
    slots
}

/// All twelve colours as a scheme, when every slot was read.
fn colour_scheme(scheme: &El, slots: &[Option<Rgba>; 12]) -> Option<ColorScheme> {
    let mut colors = [Rgba::BLACK; 12];
    for (out, slot) in colors.iter_mut().zip(slots) {
        *out = (*slot)?;
    }
    Some(ColorScheme { name: scheme.attr("name").unwrap_or("").to_owned(), colors })
}

/// The colour an `a:dk1`-style element holds: `a:srgbClr val`, or `a:sysClr lastClr` (else its `val`, for the two system colours a theme uses).
pub(crate) fn colour(slot: &El) -> Option<Rgba> {
    let inner = slot.elements().next()?;
    if inner.is("srgbClr") {
        return hex6(inner.attr("val")?);
    }
    if inner.is("sysClr") {
        if let Some(rgb) = inner.attr("lastClr").and_then(hex6) {
            return Some(rgb);
        }
        return match inner.attr("val")? {
            "windowText" => Some(Rgba::BLACK),
            "window" => Some(Rgba { r: 255, g: 255, b: 255, a: 255 }),
            _ => None,
        };
    }
    None
}

/// Six hexadecimal digits, and nothing else, as a colour.
pub(crate) fn hex6(text: &str) -> Option<Rgba> {
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Rgba::from_hex(text)
}

/// The `typeface` of the `a:latin` of a font scheme's `a:majorFont` or `a:minorFont`.
fn typeface(scheme: &El, which: &str) -> Option<Arc<str>> {
    let name = scheme.child(which)?.child("latin")?.attr("typeface")?;
    Some(Arc::from(name))
}
