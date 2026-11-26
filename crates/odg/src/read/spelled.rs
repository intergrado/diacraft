//! What a run's or a paragraph's automatic style **stated** of its text properties, held beside the cells that read it.
//!
//! The cells say more than a style stated, and in other forms: a `Character` row of `IX` 1 or more holds every cell its chain resolves (the size always), and the cell `Style` is one number whose
//! bits stand for the weight, the style, the small caps and the complex script's two. A writer that says the attributes from the cells writes the size a bold span never stated and all five of the
//! `Style` group where the file stated the weight alone. And a paragraph style's text properties read as the cells of the `Character` row its text outside the spans has, so
//! that the writer wrote them as a span round the text and carried the text in DiaCraft's data. Two kinds of value are held, each only in the slots the writers of 0.2 never held values of that kind in
//! (they pick a paragraph's values by the kind of their group, so an older build leaves them be):
//!
//! * **A run's statement of a group** ([`run_values`]): a value for each group of the run's row where the automatic text style's statement and the cells part ways. The groups [`RUN_GROUPS`]
//!   calls **whole** (`Size`, `Style`, `Color`, `FontScale`, which no value of 0.2 was held for) hold the attributes the style stated, as written, when they are not what the cells write: the value
//!   is the group's whole statement while its stamp ([`stamp_text_group`]) holds, none of the attributes the cells write for the group included. The others, whose values of 0.2 are additions
//!   to what the cells write (an underline's colour, a family), hold **no attribute** when the style stated none of the group and the cells write some (the row holds every cell its chain
//!   resolves, the face and the colour among them): a value with no attribute is the statement "the style stated none". Nothing is held when the cells write what the file stated.
//! * **A paragraph's own text properties** ([`paragraph_values`]): the mapped attributes of its automatic paragraph style's `style:text-properties`, one value for each group, as written, each
//!   stamped by **the whole `Character` row of the paragraph's own text** ([`stamp_text_row`]), `None` for a paragraph with no text of its own. The writer says the style's attributes as they were,
//!   and takes a stretch of the paragraph's text that is on that row for the paragraph's own, which needs no span. A face is named as the file named it: the part's declarations of faces are
//!   written back as they were read.
//!
//! **Cost.** A style is looked at once for each row it is used with ([`Spelling`] keeps the values, so that a style of a million uses is one look): each look is a style step, and its values are
//! charged then by their attributes and bytes; each use is charged by the number of values it is given, by the caller, before they are copied (an attribute list is shared, so a copy is a record).

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use diacraft_doc::{HeldStyleValue, Passthrough, RawAttrs, Row, TextGroup, stamp_text_group, stamp_text_row};

use super::Reader;
use super::names::Ns;
use super::styles::{Family, Kind, P, StyleSet, text_attribute};
use super::units;
use crate::error::OdgError;

/// The groups of a run's row a statement is held for, and whether the value says the **whole** of the group (the cells' attributes for it are then not written) or, with attributes, adds to
/// what the cells write, as the values of 0.2 do.
const RUN_GROUPS: [(TextGroup, bool); 13] = [
    (TextGroup::Size, true),
    (TextGroup::Style, true),
    (TextGroup::Color, true),
    (TextGroup::FontScale, true),
    (TextGroup::Underline, false),
    (TextGroup::Strike, false),
    (TextGroup::Overline, false),
    (TextGroup::Case, false),
    (TextGroup::Pos, false),
    (TextGroup::Letterspace, false),
    (TextGroup::Font, false),
    (TextGroup::AsianFont, false),
    (TextGroup::ComplexScriptFont, false),
];

/// The group of cells a mapped text property feeds.
fn group_of(p: P) -> Option<TextGroup> {
    Some(match p {
        P::FontSize => TextGroup::Size,
        P::FontWeight | P::FontStyle | P::FontVariant | P::FontWeightComplex | P::FontStyleComplex => TextGroup::Style,
        P::UnderlineStyle | P::UnderlineType => TextGroup::Underline,
        P::LineThroughStyle | P::LineThroughType => TextGroup::Strike,
        P::OverlineStyle => TextGroup::Overline,
        P::TextTransform => TextGroup::Case,
        P::TextPosition => TextGroup::Pos,
        P::Color => TextGroup::Color,
        P::LetterSpacing => TextGroup::Letterspace,
        P::TextScale => TextGroup::FontScale,
        P::FontName | P::FontFamily => TextGroup::Font,
        P::FontNameAsian | P::FontFamilyAsian => TextGroup::AsianFont,
        P::FontNameComplex | P::FontFamilyComplex => TextGroup::ComplexScriptFont,
        _ => return None,
    })
}

/// The mapped attributes the style at `at` stated in its `style:text-properties`, as the standard's own qualified names with the values as written, each with the group it feeds.
fn stated(set: &StyleSet, at: usize) -> Vec<(TextGroup, String, String)> {
    let Some(style) = set.get(at) else { return Vec::new() };
    style
        .props(Kind::Text)
        .iter()
        .filter_map(|(p, prop)| {
            let group = group_of(*p)?;
            let (ns, local) = text_attribute(*p)?;
            let prefix = match ns {
                Ns::Fo => "fo",
                Ns::Style => "style",
                _ => return None,
            };
            Some((group, format!("{prefix}:{local}"), prop.text.clone()))
        })
        .collect()
}

/// A value of `group`, made of the attributes of `stated` that feed it.
fn value(group: TextGroup, stamp: Option<diacraft_doc::Stamp>, stated: &[(TextGroup, String, String)]) -> HeldStyleValue {
    let pairs = stated.iter().filter(|(g, _, _)| *g == group).map(|(_, name, text)| (name.as_str(), text.as_str()));
    HeldStyleValue { group, stamp, attrs: RawAttrs::from_pairs(pairs), kept: Passthrough::new() }
}

/// What the held spellings made so far are, by the style and the row they are for: a style used with a row a million times is looked at once.
#[derive(Default)]
pub(crate) struct Spelling {
    runs: HashMap<(usize, Option<u32>), Vec<HeldStyleValue>>,
    paragraphs: HashMap<(Option<usize>, Option<u32>), Vec<HeldStyleValue>>,
}

impl Spelling {
    /// The values a run whose automatic text style is the one at `at` and whose own `Character` row is `ix` (`row`, as the sheet holds it) is given: see the module documentation. A style used with
    /// a row is looked at once ([`Reader::style_step`]) and its values are charged then, by their attributes and bytes; the caller charges each use by their number.
    pub(crate) fn run(&mut self, r: &mut Reader, set: &StyleSet, at: usize, ix: Option<u32>, row: &Row) -> Result<&[HeldStyleValue], OdgError> {
        let values = match self.runs.entry((at, ix)) {
            Entry::Occupied(found) => found.into_mut(),
            Entry::Vacant(slot) => {
                r.style_step();
                let made = run_values(set, at, row);
                charge_once(r, &made)?;
                slot.insert(made)
            }
        };
        Ok(values.as_slice())
    }

    /// The values a paragraph whose style is the one at `at` (when it names one the file holds) and whose own text is on the `Character` row `ix` (`row`) is given.
    pub(crate) fn paragraph(
        &mut self,
        r: &mut Reader,
        set: &StyleSet,
        at: Option<usize>,
        own: Option<(u32, &Row)>,
    ) -> Result<&[HeldStyleValue], OdgError> {
        let values = match self.paragraphs.entry((at, own.map(|(ix, _)| ix))) {
            Entry::Occupied(found) => found.into_mut(),
            Entry::Vacant(slot) => {
                r.style_step();
                let made = paragraph_values(set, at, own.map(|(_, row)| row));
                charge_once(r, &made)?;
                slot.insert(made)
            }
        };
        Ok(values.as_slice())
    }
}

/// Charges the values of a style used with a row, once, when they are first made: the entry that holds them, and each value's attributes (two items each) and bytes.
fn charge_once(r: &mut Reader, made: &[HeldStyleValue]) -> Result<(), OdgError> {
    let (mut items, mut bytes) = (1usize, 0usize);
    for value in made {
        items = items.saturating_add(value.attrs.len().saturating_mul(2));
        for (name, text) in value.attrs.iter() {
            bytes = bytes.saturating_add(name.len()).saturating_add(text.len());
        }
    }
    r.take_items(items)?;
    r.take_bytes(bytes)
}

/// Whether a font size as the file spelled it is one ODF 1.4 takes: a length or a percentage above 0.
fn size_is_positive(text: &str) -> bool {
    match text.strip_suffix('%') {
        Some(number) => units::decimal(number).is_some_and(|n| n > 0.0),
        None => units::length(text).is_some_and(|length| length.inches > 0.0),
    }
}

/// The run values: see [`Spelling::run`].
fn run_values(set: &StyleSet, at: usize, row: &Row) -> Vec<HeldStyleValue> {
    let Some(style) = set.get(at) else { return Vec::new() };
    if style.common || style.family != Family::Text {
        return Vec::new();
    }
    let stated = stated(set, at);
    // What the cells of the row write: a row the writer cannot say holds no statement (the writer refuses it, or leaves its cell out and says so).
    let Some(written) = crate::write::runs::written_groups(row) else { return Vec::new() };
    let mut out = Vec::new();
    for (group, whole) in RUN_GROUPS {
        let said: Vec<(&str, &str)> = stated.iter().filter(|(g, _, _)| *g == group).map(|(_, name, text)| (name.as_str(), text.as_str())).collect();
        let wrote: &[(String, String)] = written.iter().find(|(id, _)| *id == group.id()).map_or(&[], |(_, attrs)| attrs.as_slice());
        if whole {
            // A size that ODF 1.4 refuses (an earlier version took 0) is not said back: the cells write the least size and the writer carries the 0.
            if group == TextGroup::Size && said.iter().any(|(_, text)| !size_is_positive(text)) {
                continue;
            }
            let same = said.len() == wrote.len() && said.iter().all(|(name, text)| wrote.iter().any(|(n, t)| n == name && t == text));
            if !same {
                out.push(value(group, Some(stamp_text_group(row, group)), &stated));
            }
        } else if said.is_empty() && !style.text_templates.iter().any(|t| t.group == group) && wrote.iter().any(|(_, text)| text != "none") {
            // The row holds a group the style stated none of (it read it through its chain): the statement is that none was stated. A line that is off (`none`) is no attribute the file's style
            // would have, and the writer says it of no run that holds no underline cell.
            out.push(value(group, Some(stamp_text_group(row, group)), &[]));
        }
    }
    out
}

/// The paragraph values: see [`Spelling::paragraph`]. **A paragraph with text of its own on a row that holds cells is always given a value** (a paragraph of a style that states no text property reads
/// through the shape's chain, and its row holds the size all the same): when its style states none, the value has no attribute, and its stamp is what the writer knows the row by.
fn paragraph_values(set: &StyleSet, at: Option<usize>, own: Option<&Row>) -> Vec<HeldStyleValue> {
    let stated = match at.and_then(|at| set.get(at).map(|style| (at, style))) {
        Some((at, style)) if !style.common && style.family == Family::Paragraph => stated(set, at),
        _ => Vec::new(),
    };
    // A size ODF 1.4 refuses is not said back (see [`size_is_positive`]): the text is written from its cells.
    if stated.iter().any(|(group, _, text)| *group == TextGroup::Size && !size_is_positive(text)) {
        return Vec::new();
    }
    let stamp = own.map(stamp_text_row);
    if stated.is_empty() {
        return match own {
            Some(row) if !row.cells.is_empty() => vec![value(TextGroup::Size, stamp, &stated)],
            _ => Vec::new(),
        };
    }
    let mut groups: Vec<TextGroup> = Vec::new();
    for (group, _, _) in &stated {
        if !groups.contains(group) {
            groups.push(*group);
        }
    }
    groups.into_iter().map(|group| value(group, stamp, &stated)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_held_only_when_it_is_above_zero() {
        for text in ["18pt", "0.25in", "150%", "0.5cm"] {
            assert!(size_is_positive(text), "{text}");
        }
        for text in ["0pt", "0in", "0%", "-1pt", "big", "", "pt", "%"] {
            assert!(!size_is_positive(text), "{text}");
        }
    }

    #[test]
    fn every_mapped_text_property_feeds_a_group_of_the_style_it_is_in() {
        for p in [P::FontSize, P::FontWeight, P::FontStyle, P::FontVariant, P::FontWeightComplex, P::FontStyleComplex, P::Color, P::TextTransform] {
            assert!(group_of(p).is_some() && text_attribute(p).is_some(), "{p:?}");
        }
        assert_eq!(group_of(P::FontWeight), Some(TextGroup::Style));
        assert_eq!(group_of(P::FontName), Some(TextGroup::Font));
        assert_eq!(group_of(P::Fill), None, "a graphic property feeds no group of the text");
    }
}
