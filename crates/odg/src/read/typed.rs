//! What a style's property values read as, parsed once, when the style is read.
//!
//! **Why.** A conversion reads a style's properties again for every run, paragraph, list item and shape-style pair that shows it. If a
//! conversion parsed the value string each time, the work would be the string's length times the number of uses: a 5 KB file with 20,000
//! paragraphs on one style whose `fo:margin-left` is a million characters took 20.7 s. So each
//! value is read into a [`Typed`] when its style is read, by the parser its property needs, and every use reads the typed value, which is
//! a few bytes whatever the string's length. The values parsed are counted ([`crate::ReadWork::value_parses`], one for each value, whatever
//! forms it is tried as); that count is the number of values the styles hold, not the number of times they are shown.
//!
//! **Names.** A dash, a gradient, a marker and the styles a DiaCraft style links to are named by strings the style holds, and looking one
//! up hashes the name. They are looked up once for each style, when the part that holds the style is read ([`super::styles::StyleSet::resolve`]),
//! and the style keeps the index it found.
//!
//! The text of a value stays beside its typed reading: the keywords (`solid`, `center`, ...) are compared as they are written, which
//! costs nothing when their length differs from the keyword's.

use super::Reader;
use super::styles::P;
use super::units::{self, Length};

/// `fo:line-height`: a percentage, a length, `normal`, or something else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineHeight {
    Percent(f64),
    Length(Length),
    Normal,
    Other,
}

/// `fo:letter-spacing`: `normal`, a length, or something else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Spacing {
    Normal,
    Length(Length),
    Other,
}

/// `fo:font-size`: a length, a percentage of the parent's size, or something else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Size {
    Length(Length),
    Percent(f64),
    Other,
}

/// What a dash's or a gradient's name found, once the part holding the style is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Name {
    /// Not looked up yet.
    Unresolved,
    /// No dash or gradient of that name.
    Missing,
    /// The index of the one found, in the style set's list of them.
    Found(usize),
}

/// What a line end's marker name found, once the part holding the style is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Marker {
    /// Not looked up yet.
    Unresolved,
    /// The name is empty: no line end.
    Empty,
    /// No marker of that name.
    Missing,
    /// A marker that is not one of DiaCraft's own.
    Other,
    /// DiaCraft's own marker `DiaCraft_20_end_20_<n>`.
    Own(u32),
}

/// A property value as the parser its property needs read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Typed {
    /// A keyword or a name held elsewhere: compared as it is written.
    Word,
    Length(Option<Length>),
    /// A percentage, as the number before the `%`.
    Percent(Option<f64>),
    Colour(Option<[u8; 3]>),
    /// `svg:stroke-opacity`: a share from 0 to 1, written as a percentage or as a decimal.
    Share(Option<f64>),
    LineHeight(LineHeight),
    Size(Size),
    /// `style:text-position`: the `Pos` it gives (0, 1 or 2), and whether the text holds a `%`.
    Position {
        pos: Option<u8>,
        percent_sign: bool,
    },
    Spacing(Spacing),
    /// A font family: the byte range of the text left after white space and quotes are cut from its ends.
    Family {
        from: usize,
        to: usize,
    },
    Dash(Name),
    Gradient(Name),
    Marker(Marker),
}

impl Typed {
    /// The reading of `text`, the value of `p`. Each value parsed is counted.
    pub(crate) fn of(r: &mut Reader, p: P, text: &str) -> Typed {
        match p {
            P::StrokeWidth
            | P::MarkerStartWidth
            | P::MarkerEndWidth
            | P::ShadowOffsetX
            | P::ShadowOffsetY
            | P::Padding
            | P::PaddingTop
            | P::PaddingBottom
            | P::PaddingLeft
            | P::PaddingRight
            | P::MarginLeft
            | P::MarginRight
            | P::TextIndent
            | P::MarginTop
            | P::MarginBottom
            | P::TabStopDistance => {
                r.value_parses(1);
                Typed::Length(units::length(text))
            }
            P::Opacity | P::ShadowOpacity | P::TextScale => {
                r.value_parses(1);
                Typed::Percent(units::percent(text))
            }
            P::FillColor | P::StrokeColor | P::ShadowColor | P::Color => {
                r.value_parses(1);
                Typed::Colour(units::rgb(text))
            }
            P::StrokeOpacity => {
                r.value_parses(1);
                Typed::Share(units::percent(text).map(|p| p / 100.0).or_else(|| units::decimal(text)))
            }
            P::LineHeight => {
                r.value_parses(1);
                Typed::LineHeight(if let Some(p) = units::percent(text) {
                    LineHeight::Percent(p)
                } else if let Some(l) = units::length(text) {
                    LineHeight::Length(l)
                } else if text == "normal" {
                    LineHeight::Normal
                } else {
                    LineHeight::Other
                })
            }
            P::FontSize => {
                r.value_parses(1);
                Typed::Size(if let Some(l) = units::length(text) { Size::Length(l) } else { units::percent(text).map_or(Size::Other, Size::Percent) })
            }
            P::TextPosition => {
                r.value_parses(1);
                let first = text.split_ascii_whitespace().next().unwrap_or_default();
                let pos = match first {
                    "super" => Some(1),
                    "sub" => Some(2),
                    other => units::percent(other).map(|p| {
                        if p > 0.0 {
                            1
                        } else if p < 0.0 {
                            2
                        } else {
                            0
                        }
                    }),
                };
                Typed::Position { pos, percent_sign: text.contains('%') }
            }
            P::LetterSpacing => {
                if text == "normal" {
                    Typed::Spacing(Spacing::Normal)
                } else {
                    r.value_parses(1);
                    Typed::Spacing(units::length(text).map_or(Spacing::Other, Spacing::Length))
                }
            }
            P::FontFamily | P::FontFamilyAsian | P::FontFamilyComplex => {
                r.value_parses(1);
                let quote = |c: char| c == '\'' || c == '"';
                let trimmed = text.trim();
                let start = text.len() - text.trim_start().len();
                let unquoted_front = trimmed.trim_start_matches(quote);
                let from = start + (trimmed.len() - unquoted_front.len());
                Typed::Family { from, to: from + unquoted_front.trim_end_matches(quote).len() }
            }
            P::StrokeDash => Typed::Dash(Name::Unresolved),
            P::FillGradientName => Typed::Gradient(Name::Unresolved),
            P::MarkerStart | P::MarkerEnd => Typed::Marker(Marker::Unresolved),
            P::Fill
            | P::FillHatchSolid
            | P::Stroke
            | P::StrokeLinecap
            | P::Shadow
            | P::TextareaVerticalAlign
            | P::WritingMode
            | P::TextAlign
            | P::TextAlignLast
            | P::FontWeight
            | P::FontWeightComplex
            | P::FontStyle
            | P::FontStyleComplex
            | P::UnderlineStyle
            | P::UnderlineType
            | P::LineThroughStyle
            | P::LineThroughType
            | P::OverlineStyle
            | P::FontVariant
            | P::TextTransform
            | P::FontName
            | P::FontNameAsian
            | P::FontNameComplex
            | P::Language
            | P::Script
            | P::Country
            | P::RfcTag
            | P::LanguageAsian
            | P::ScriptAsian
            | P::CountryAsian
            | P::RfcTagAsian
            | P::LanguageComplex
            | P::ScriptComplex
            | P::CountryComplex
            | P::RfcTagComplex => Typed::Word,
        }
    }
}

/// A style property: its text as written and what it reads as.
#[derive(Debug)]
pub(crate) struct Prop {
    pub text: String,
    pub typed: Typed,
}

impl Prop {
    /// The property `p` with the value `text`, read once.
    pub(crate) fn read(r: &mut Reader, p: P, text: &str) -> Prop {
        Prop { typed: Typed::of(r, p, text), text: text.to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    #[test]
    fn a_family_is_cut_where_trim_and_trim_matches_cut_it() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        for text in
            ["", " ", "  'Free Serif'  ", "\"Free Serif\"", "' ' ", "'' a ''", "'", "\"'\"", " a b ", "'a' 'b'", "\t'x'\n", "\u{a0}'x'", "''''"]
        {
            let Typed::Family { from, to } = Typed::of(&mut r, P::FontFamily, text) else { panic!("{text:?} is not a family") };
            let expected = text.trim().trim_matches(|c| c == '\'' || c == '"');
            assert_eq!(text.get(from..to), Some(expected), "{text:?}");
        }
    }

    #[test]
    fn each_value_is_counted_once_and_a_keyword_not_at_all() {
        let limits = Limits::default();
        let mut r = Reader::new(&limits);
        let counted = |r: &mut Reader, p: P, text: &str| {
            let before = r.report.work.value_parses;
            let typed = Typed::of(r, p, text);
            (typed, r.report.work.value_parses - before)
        };
        // A value tried as two forms is one parse.
        assert_eq!(counted(&mut r, P::FontSize, "12%").1, 1);
        assert_eq!(counted(&mut r, P::LineHeight, "normal").1, 1);
        assert_eq!(counted(&mut r, P::StrokeOpacity, "0.5").1, 1);
        assert_eq!(counted(&mut r, P::MarginLeft, "1cm").1, 1);
        assert_eq!(counted(&mut r, P::LetterSpacing, "normal"), (Typed::Spacing(Spacing::Normal), 0));
        for (p, word) in [(P::Fill, "solid"), (P::TextAlign, "center"), (P::FontWeight, "bold"), (P::FontName, "F1"), (P::StrokeDash, "D")] {
            assert_eq!(counted(&mut r, p, word).1, 0, "{word}");
        }
        assert_eq!(counted(&mut r, P::TextPosition, "super 58%"), (Typed::Position { pos: Some(1), percent_sign: true }, 1));
        assert_eq!(counted(&mut r, P::TextPosition, "-33% 58%").0, Typed::Position { pos: Some(2), percent_sign: true });
        assert_eq!(counted(&mut r, P::TextPosition, "0%").0, Typed::Position { pos: Some(0), percent_sign: true });
        assert_eq!(counted(&mut r, P::TextPosition, "sub").0, Typed::Position { pos: Some(2), percent_sign: false });
        assert_eq!(counted(&mut r, P::StrokeOpacity, "50%").0, Typed::Share(Some(0.5)));
        assert_eq!(counted(&mut r, P::LineHeight, "150%").0, Typed::LineHeight(LineHeight::Percent(150.0)));
    }
}
