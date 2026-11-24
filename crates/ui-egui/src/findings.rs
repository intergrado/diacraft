//! What the window could not draw or read as the file says, in plain words.
//!
//! The renderer counts what it left out or drew by a stand-in, by kind ([`diacraft_render::Skipped`]), and `file.open` reports what
//! was odd in the file. The window says so in one place: a small mark in the status bar that opens a list (`status_bar.rs`). Nothing
//! here is a dialog and nothing says a class name: each kind has a sentence, with the count in it.
//!
//! Two lists make the whole: what is **not shown** (a picture the renderer cannot yet draw, a shape cut for time) and what is
//! **drawn approximately** (an arrowhead at a size DiaCraft had to estimate, a gradient the rasteriser steps). A guide is an editing aid that a
//! drawing does not show, so it is no gap and is not listed.

use diacraft_engine::shown;
use diacraft_render::{SkipKind, Skipped};

/// The most characters of a note a line holds.
const LINE_CHARS: usize = 200;

/// The most characters of an example a line holds.
const DETAIL_CHARS: usize = 160;

/// Where a finding belongs in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Left out, or shown as a box in the place of the real thing.
    NotShown,
    /// Drawn, by a rule that is not quite the picture's own.
    Approximate,
    /// Said by the reader of the file.
    Reading,
}

impl Group {
    /// The heading of the group in the list.
    pub fn heading(self) -> &'static str {
        match self {
            Group::NotShown => "Not shown",
            Group::Approximate => "Drawn approximately",
            Group::Reading => "When the file was read",
        }
    }
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub group: Group,
    /// The sentence, with its count.
    pub text: String,
    /// An example, for a tooltip: the first one the renderer met.
    pub detail: Option<String>,
}

/// `n` and the noun in its number: `1 picture`, `12 pictures`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The sentence for what the renderer left out or approximated, and where it goes. `None` for what is no gap.
fn describe(kind: SkipKind, n: usize) -> Option<(Group, String)> {
    use Group::{Approximate, NotShown};
    Some(match kind {
        SkipKind::Guide => return None,
        SkipKind::MetafilePlaceholder => (NotShown, format!("{} not shown yet (EMF or WMF)", count(n, "picture", "pictures"))),
        SkipKind::MetafileGdiFallback => (Approximate, format!("{} drawn from its older half", count(n, "EMF+ picture", "EMF+ pictures"))),
        SkipKind::MetafileEmfPlusMisplaced => (Approximate, format!("{} played as a plain EMF", count(n, "picture", "pictures"))),
        SkipKind::MetafileRecordSkipped => {
            (Approximate, format!("{} drawn without some of its records", count(n, "EMF or WMF picture", "EMF or WMF pictures")))
        }
        SkipKind::MetafileGlyphIndexText => {
            (Approximate, format!("{} whose text is glyph numbers, not drawn", count(n, "EMF or WMF picture", "EMF or WMF pictures")))
        }
        SkipKind::MetafileFontMissing => (
            Approximate,
            format!("{} whose text is in a symbol font DiaCraft cannot find, not drawn", count(n, "EMF or WMF picture", "EMF or WMF pictures")),
        ),
        SkipKind::MetafileBitmapUndecodable => {
            (Approximate, format!("{} with a bitmap that could not be read", count(n, "EMF or WMF picture", "EMF or WMF pictures")))
        }
        SkipKind::MetafileCutShort => {
            (Approximate, format!("{} cut short by a record of the wrong size", count(n, "EMF or WMF picture", "EMF or WMF pictures")))
        }
        SkipKind::MetafileCapReached => {
            (Approximate, format!("{} cut short because it is bigger than DiaCraft draws", count(n, "EMF or WMF picture", "EMF or WMF pictures")))
        }
        SkipKind::PictureUndecoded => (NotShown, format!("{} that could not be read, shown as a box", count(n, "picture", "pictures"))),
        SkipKind::InkNoPicture => (NotShown, format!("{} not shown", count(n, "handwritten drawing", "handwritten drawings"))),
        SkipKind::NoPicture => (NotShown, format!("{} with no picture in them", count(n, "picture shape", "picture shapes"))),
        // An object the file holds that DiaCraft cannot draw (a table, an embedded object, a 3-D scene, ...), with no picture of its own: its frame is a box with its name.
        SkipKind::ObjectPlaceholder => {
            let (noun, boxes) = if n == 1 { ("object", "a labelled box") } else { ("objects", "labelled boxes") };
            (NotShown, format!("{n} {noun} DiaCraft cannot draw, shown as {boxes}"))
        }
        SkipKind::FrameBudget => (NotShown, format!("{} left out because the page took too long to draw", count(n, "shape", "shapes"))),
        // A picture that was stopped is never shown, so this line is never seen; it has words because every kind does.
        SkipKind::Cancelled => (NotShown, format!("{} left out because the view moved on", count(n, "shape", "shapes"))),
        SkipKind::LineEndNoDirection => {
            let (noun, pronoun) = if n == 1 { ("line", "its line end is") } else { ("lines", "their line ends are") };
            (NotShown, format!("{n} {noun} with no length to point along, so {pronoun} left off"))
        }
        SkipKind::ShapeLimit => (NotShown, format!("{} left out: the page has more than DiaCraft draws at once", count(n, "shape", "shapes"))),
        SkipKind::RowLimit => (NotShown, format!("{} left out: their outlines are more than DiaCraft builds at once", count(n, "shape", "shapes"))),
        // A window's frame has no work budget (a saved file's preview has), so this line is not seen there; it has words because every kind does.
        SkipKind::WorkLimit => (NotShown, format!("{} left out: the page holds more than DiaCraft draws at once", count(n, "shape", "shapes"))),
        SkipKind::OutOfRange => (NotShown, format!("{} too far from the page to be drawn", count(n, "shape", "shapes"))),
        SkipKind::GeometryFailed => (NotShown, format!("{} whose outline could not be built", count(n, "shape", "shapes"))),
        SkipKind::SectionsCut => (NotShown, format!("{} with more outlines than DiaCraft draws", count(n, "shape", "shapes"))),
        SkipKind::InfiniteLine => (NotShown, format!("{} that go on without end, not drawn", count(n, "line", "lines"))),
        SkipKind::TextBudget | SkipKind::TextCharBudget => {
            (NotShown, format!("{} left out: the page has more text than DiaCraft lays out at once", count(n, "text block", "text blocks")))
        }
        SkipKind::GlyphsCut => (NotShown, format!("{} cut short", count(n, "long text block", "long text blocks"))),
        SkipKind::MissingGlyphs => (NotShown, format!("{} with letters no installed font can draw", count(n, "text block", "text blocks"))),
        SkipKind::CustomLineEnd => (NotShown, format!("{} not drawn", count(n, "custom line end", "custom line ends"))),
        SkipKind::TooDeep => (NotShown, format!("{} nested too deeply to draw", count(n, "group", "groups"))),
        SkipKind::ViewRefused => (NotShown, "The picture was too large to draw at this zoom".to_owned()),
        SkipKind::NoSuchPage => (NotShown, "The page is not in the drawing".to_owned()),
        SkipKind::Recovered => (NotShown, "The page met an internal problem while drawing and may be incomplete".to_owned()),
        SkipKind::ShapeFailed => (NotShown, format!("{} left out because DiaCraft met an internal problem drawing it", count(n, "shape", "shapes"))),
        SkipKind::GeometryRow => {
            (Approximate, format!("{} skipped or drawn by a fallback rule", count(n, "piece of an outline", "pieces of outlines")))
        }
        SkipKind::TextNote => (Approximate, format!("{} drawn in a stand-in font or cut short", count(n, "text block", "text blocks"))),
        SkipKind::PatternApproximate => (Approximate, format!("{} drawn approximately", count(n, "patterned fill", "patterned fills"))),
        SkipKind::CustomPattern => {
            (Approximate, format!("{} drawn solid where the file uses a custom pattern", count(n, "fill or line", "fills and lines")))
        }
        SkipKind::DashesTooMany => (Approximate, format!("{} drawn solid because the dashes were too fine", count(n, "dashed line", "dashed lines"))),
        SkipKind::StrokeClamped => (Approximate, format!("{} wider than DiaCraft draws, drawn at the widest it can", count(n, "line", "lines"))),
        // The kind of end is always drawn as its own art; what is estimated is its size (a box no source measures, or a size class that is none).
        SkipKind::LineEndApproximate => (Approximate, format!("{} drawn at a size DiaCraft had to estimate", count(n, "line end", "line ends"))),
        SkipKind::CompoundApproximate => {
            (Approximate, format!("{} drawn as bands of one line", count(n, "double or triple line", "double or triple lines")))
        }
        SkipKind::GradientApproximate => (Approximate, format!("{} drawn approximately", count(n, "gradient", "gradients"))),
        SkipKind::ShadowApproximate => (Approximate, format!("{} drawn approximately", count(n, "shadow", "shadows"))),
        SkipKind::ThemeUnanswered => (Approximate, format!("{} drawn with plain colours where the file uses a theme", count(n, "shape", "shapes"))),
        SkipKind::DanglingLink => (Approximate, format!("{} drawn without the master or style the file does not have", count(n, "shape", "shapes"))),
        // Only an export meets this (the window draws letters as outlines always): the sentence is for a list an export's report is shown in.
        SkipKind::TextAsOutlines => {
            (Approximate, format!("{} written as outlines, so it cannot be selected or searched", count(n, "piece of text", "pieces of text")))
        }
    })
}

/// The lines for what a picture of a page left out, not shown first.
pub fn from_render(skipped: &[Skipped]) -> Vec<Finding> {
    let mut lines: Vec<Finding> = skipped
        .iter()
        .filter_map(|s| {
            let (group, text) = describe(s.kind, s.count)?;
            let detail = (!s.first.is_empty()).then(|| format!("For example: {}", shown(&s.first, DETAIL_CHARS)));
            Some(Finding { group, text, detail })
        })
        .collect();
    lines.sort_by_key(|f| f.group == Group::Approximate);
    lines
}

/// The lines for what the reader of a file said: its first notes in its words, then how many more there were.
pub fn from_reading(warnings: &[String], total: u64) -> Vec<Finding> {
    let mut lines: Vec<Finding> = warnings.iter().map(|w| Finding { group: Group::Reading, text: shown(w, LINE_CHARS), detail: None }).collect();
    let more = total.saturating_sub(warnings.len() as u64);
    if more > 0 {
        let text = format!("and {more} more {}", if more == 1 { "note" } else { "notes" });
        lines.push(Finding { group: Group::Reading, text, detail: None });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip(kind: SkipKind, count: usize) -> Skipped {
        Skipped { kind, count, first: "shape 4: x".to_owned() }
    }

    #[test]
    fn a_count_and_a_noun_agree_and_the_sentence_says_what_is_missing() {
        let lines = from_render(&[skip(SkipKind::MetafilePlaceholder, 12)]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "12 pictures not shown yet (EMF or WMF)");
        assert_eq!(lines[0].group, Group::NotShown);
        assert_eq!(from_render(&[skip(SkipKind::MetafilePlaceholder, 1)])[0].text, "1 picture not shown yet (EMF or WMF)");
    }

    #[test]
    fn a_line_with_no_direction_has_its_own_sentence_and_is_not_a_stand_in() {
        let one = from_render(&[skip(SkipKind::LineEndNoDirection, 1)]);
        assert_eq!(one[0].text, "1 line with no length to point along, so its line end is left off");
        assert_eq!(one[0].group, Group::NotShown, "the line end is not drawn, which a stand-in would be");
        assert_eq!(
            from_render(&[skip(SkipKind::LineEndNoDirection, 3)])[0].text,
            "3 lines with no length to point along, so their line ends are left off"
        );
        let stand_in = from_render(&[skip(SkipKind::LineEndApproximate, 1)]);
        assert_eq!(stand_in[0].group, Group::Approximate);
        assert!(!stand_in[0].text.contains("no length"), "{stand_in:?}");
    }

    /// Every line end from 0 to 45 is drawn as its own art now, so a line end the renderer notes (a size no source measures, or a size
    /// class outside 0 to 6 drawn as class 2) is a size DiaCraft estimated, and the sentence must not say another kind of end was drawn in its place.
    #[test]
    fn a_line_end_drawn_at_an_estimated_size_says_that_and_not_that_another_kind_was_drawn() {
        let one = from_render(&[skip(SkipKind::LineEndApproximate, 1)]);
        assert_eq!(one[0].text, "1 line end drawn at a size DiaCraft had to estimate");
        assert_eq!(from_render(&[skip(SkipKind::LineEndApproximate, 4)])[0].text, "4 line ends drawn at a size DiaCraft had to estimate");
        assert_eq!(one[0].group, Group::Approximate);
        assert!(!one[0].text.contains("nearest kind"), "{}", one[0].text);
    }

    /// An object DiaCraft cannot draw and has no picture of is a labelled box, and the list says so, in the not-shown group, with the count and
    /// in the number the nouns agree with.
    #[test]
    fn an_object_shown_as_a_labelled_box_says_so_in_the_not_shown_group() {
        let one = from_render(&[skip(SkipKind::ObjectPlaceholder, 1)]);
        assert_eq!(one[0].text, "1 object DiaCraft cannot draw, shown as a labelled box");
        assert_eq!(one[0].group, Group::NotShown);
        assert_eq!(from_render(&[skip(SkipKind::ObjectPlaceholder, 6)])[0].text, "6 objects DiaCraft cannot draw, shown as labelled boxes");
        assert!(one[0].detail.as_deref().is_some_and(|d| d.starts_with("For example: ")));
    }

    #[test]
    fn what_is_not_shown_comes_before_what_is_approximate_and_a_guide_is_no_gap() {
        let lines = from_render(&[skip(SkipKind::GradientApproximate, 2), skip(SkipKind::Guide, 5), skip(SkipKind::FrameBudget, 3)]);
        assert_eq!(lines.len(), 2, "a guide is not listed: {lines:?}");
        assert_eq!(lines[0].group, Group::NotShown);
        assert_eq!(lines[1].group, Group::Approximate);
        assert!(lines[1].detail.as_deref().is_some_and(|d| d.starts_with("For example: ")));
    }

    #[test]
    fn no_sentence_names_a_class_or_a_rust_word() {
        let kinds = [
            SkipKind::ViewRefused,
            SkipKind::NoSuchPage,
            SkipKind::FrameBudget,
            SkipKind::Cancelled,
            SkipKind::ShapeLimit,
            SkipKind::RowLimit,
            SkipKind::WorkLimit,
            SkipKind::OutOfRange,
            SkipKind::GeometryFailed,
            SkipKind::GeometryRow,
            SkipKind::SectionsCut,
            SkipKind::InfiniteLine,
            SkipKind::TextBudget,
            SkipKind::TextCharBudget,
            SkipKind::TextNote,
            SkipKind::MissingGlyphs,
            SkipKind::GlyphsCut,
            SkipKind::TextAsOutlines,
            SkipKind::InkNoPicture,
            SkipKind::MetafilePlaceholder,
            SkipKind::PictureUndecoded,
            SkipKind::NoPicture,
            SkipKind::ObjectPlaceholder,
            SkipKind::PatternApproximate,
            SkipKind::CustomPattern,
            SkipKind::DashesTooMany,
            SkipKind::StrokeClamped,
            SkipKind::LineEndApproximate,
            SkipKind::LineEndNoDirection,
            SkipKind::CustomLineEnd,
            SkipKind::CompoundApproximate,
            SkipKind::GradientApproximate,
            SkipKind::ShadowApproximate,
            SkipKind::ThemeUnanswered,
            SkipKind::TooDeep,
            SkipKind::DanglingLink,
            SkipKind::Recovered,
            SkipKind::ShapeFailed,
        ];
        for kind in kinds {
            let (_, text) = describe(kind, 3).unwrap_or_else(|| panic!("{kind:?} has no sentence"));
            let class = format!("{kind:?}");
            assert!(!text.contains(&class), "{text:?} names its class {class}");
            assert!(!text.contains("::") && !text.contains('_'), "{text:?}");
            assert!(text.chars().next().is_some_and(char::is_alphanumeric), "{text:?}");
        }
    }

    #[test]
    fn the_readers_notes_are_listed_and_the_rest_counted() {
        let lines = from_reading(&["a page has no part".to_owned()], 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "and 3 more notes");
        assert_eq!(from_reading(&[], 0), Vec::new());
        assert_eq!(from_reading(&["x".to_owned()], 2)[1].text, "and 1 more note");
        // A control character in a note does not reach the screen as one.
        assert!(!from_reading(&["a\u{1b}[31mb".to_owned()], 1)[0].text.contains('\u{1b}'));
    }
}
