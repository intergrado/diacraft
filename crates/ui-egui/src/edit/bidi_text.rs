//! How the text box lays out a paragraph that reads right to left.
//!
//! **What does the work.** The box is egui's text edit, and the egui the window uses is the suite's fork, whose galley already shapes right-to-left runs, orders each row
//! visually, puts the caret and the pointer's hit on the screen's side, moves Left and Right by the screen and takes an input method's composition as any other text
//! (the fork's egui patches). Its Home and End go to the edges of a row on the screen, which are not the first and the last character of a row that
//! holds both directions, so the box takes those two keys itself ([`logical_home_and_end`]). What the galley cannot know is the shape's own
//! paragraph format, so the box gives it a layout job of its own in place of the default one: [`layout_job`] says which way the paragraphs read (the cell `Flags`, never
//! guessed from the first letter), where the rows stand (the cell `HorzAlign`, mirrored for a paragraph that reads right to left, which makes alignment 0 the right
//! side), and how far in the paragraph starts (`IndLeft` on the right and `IndRight` on the left).
//!
//! **What is not in the box.** One job lays out the whole text, so one direction and one alignment: where the paragraphs of a shape agree on them the box is exact,
//! and where they differ the direction is chosen by each paragraph's first strong letter (egui's own rule) and the alignment is the first paragraph's. The box has
//! never shown the runs' faces, sizes or colours, the bullets, the tab stops or the first line's indent (the galley moves a row's glyphs to the side its alignment
//! says whatever space a section leaves before them, so there is no first-line indent to give it), and does not now. **Text with no right-to-left paragraph and no right-to-left letter
//! has no look at all** ([`editor_look`] gives `None`): the box lays it out with egui's default layout, as it always did.

use std::sync::Arc;

use diacraft_engine::formatting::{Align as HAlign, Direction, EditorParagraph};
use egui::epaint::text::TextDirection;
use egui::text::{ByteIndex, CCursor, CCursorRange, CharIndex, Galley, LayoutJob, LayoutSection, TextFormat, TextWrapping};
use egui::text_edit::TextEditState;
use egui::{Align, Color32, Context, Event, FontId, Id, Key, TextBuffer, Ui};

/// The most an indent is taken for, in inches: a paragraph indented past it is indented by it, so the text always has room in the box.
const MOST_INDENT: f64 = 40.0;

/// How the paragraphs of a text are laid out in the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EditorLook {
    /// The base direction of every paragraph: from the cells where they agree, each paragraph's own first strong letter where they do not.
    pub direction: TextDirection,
    /// Where the rows stand in the room the text has.
    pub halign: Align,
    /// With `halign` the left side: every row on the start side of its own paragraph, whichever way it reads.
    pub min_is_start: bool,
    /// Justified and distributed paragraphs fill the room.
    pub justify: bool,
    /// How far in the room is from the box's left and right, in inches: the start indent on the side the paragraph starts on.
    pub left: f64,
    pub right: f64,
}

/// The paragraph the box takes when the shape has none to read: a new box.
const NEW: EditorParagraph = EditorParagraph { direction: Direction::LeftToRight, align: HAlign::Center, ind_left: 0.0, ind_right: 0.0 };

/// Whether `c` belongs to a script written right to left or marks text as running that way: Hebrew, Arabic, Syriac, Thaana, N'Ko, Samaritan and Mandaic with their
/// extensions and presentation forms, the scripts of the planes beyond, and the right-to-left mark and embeddings.
pub(super) fn is_right_to_left(c: char) -> bool {
    matches!(u32::from(c), 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF | 0x200F | 0x202B | 0x202E | 0x2067)
}

/// A finite number of inches from 0 to [`MOST_INDENT`].
fn inches(n: f64) -> f64 {
    if n.is_finite() { n.clamp(0.0, MOST_INDENT) } else { 0.0 }
}

/// How the box lays `text` out when its paragraphs are `paragraphs` (as the engine reads them for the text being edited), or `None` for the layout the box has always
/// had: **only text with a paragraph that reads right to left, or a right-to-left letter, has a look**, so that left-to-right text is not changed by any of this.
pub(super) fn editor_look(paragraphs: &[EditorParagraph], text: &str) -> Option<EditorLook> {
    let flagged = paragraphs.iter().any(|p| p.direction == Direction::RightToLeft);
    if !flagged && !text.chars().any(is_right_to_left) {
        return None;
    }
    let first = paragraphs.first().copied().unwrap_or(NEW);
    // One job has one base direction: the cells' where every paragraph has the same, else each paragraph's first strong letter.
    let agree = paragraphs.iter().all(|p| p.direction == first.direction);
    let direction = match (agree, first.direction) {
        (false, _) => TextDirection::Auto,
        (true, Direction::LeftToRight) => TextDirection::LeftToRight,
        (true, Direction::RightToLeft) => TextDirection::RightToLeft,
    };
    let reads_right_to_left = agree && first.direction == Direction::RightToLeft;
    // Alignment 0 is the start side and 2 the end side of a paragraph; with paragraphs that read both ways the start side is each row's own.
    let (halign, min_is_start, justify) = match first.align {
        HAlign::Center => (Align::Center, false, false),
        HAlign::Left if !agree => (Align::LEFT, true, false),
        HAlign::Left => (if reads_right_to_left { Align::RIGHT } else { Align::LEFT }, false, false),
        HAlign::Right => (if reads_right_to_left { Align::LEFT } else { Align::RIGHT }, false, false),
        HAlign::Justify | HAlign::Distributed if !agree => (Align::LEFT, true, true),
        HAlign::Justify | HAlign::Distributed => (if reads_right_to_left { Align::RIGHT } else { Align::LEFT }, false, true),
    };
    // The start indent is `IndLeft`, on the right for a paragraph that reads right to left, and the stop indent `IndRight` on the other side.
    let (start, stop) = (inches(first.ind_left), inches(first.ind_right));
    let (left, right) = if first.direction == Direction::RightToLeft { (stop, start) } else { (start, stop) };
    Some(EditorLook { direction, halign, min_is_start, justify, left, right })
}

/// The job that lays `text` out in the box: one section for all of it, wrapped at `wrap_width`.
pub(super) fn layout_job(look: &EditorLook, text: &str, format: TextFormat, wrap_width: f32, language: Option<String>) -> LayoutJob {
    let sections = vec![LayoutSection { leading_space: 0.0, byte_range: ByteIndex(0)..ByteIndex(text.len()), format }];
    LayoutJob {
        text: text.to_owned(),
        sections,
        wrap: TextWrapping { max_width: wrap_width, ..TextWrapping::default() },
        direction: look.direction,
        halign: look.halign,
        halign_min_is_start: look.min_is_start,
        justify: look.justify,
        // The spaces a person types are seen, as in the box's default layout.
        keep_trailing_whitespace: true,
        language,
        ..LayoutJob::default()
    }
}

/// The galley of `text` in the box: [`layout_job`] with the box's font and colour and the line height egui's own layout gives, laid out by the window's fonts.
pub(super) fn lay_out(look: &EditorLook, ui: &Ui, text: &dyn TextBuffer, wrap_width: f32, font: &FontId, color: Color32) -> Arc<Galley> {
    let line_height = ui.fonts_mut(|fonts| fonts.row_height(font)) + ui.spacing().extra_text_line_spacing;
    let format = TextFormat { line_height: Some(line_height), ..TextFormat::simple(font.clone(), color) };
    let job = layout_job(look, text.as_str(), format, wrap_width, ui.language());
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// The place of the first and of the last character of the row of `cursor` in `galley`, in the text's own order: where Home and End go. The row's characters are
/// a stretch of the text however the row is ordered on the screen, so **the first is where the row's text starts and the last where it ends, whichever side of the
/// row each is on** (the end of a left-to-right row that finishes in Hebrew is in the middle of it). The galley's own Home and End go to the row's edges on the
/// screen, which are the same places only where the row reads one way.
pub(super) fn row_ends(galley: &Galley, cursor: &CCursor) -> (CCursor, CCursor) {
    let row = galley.layout_from_cursor(*cursor).row;
    let before: usize = galley.rows.iter().take(row).map(|placed| placed.char_count_including_newline().0).fold(0, usize::saturating_add);
    let length = galley.rows.get(row).map_or(0, |placed| placed.char_count_excluding_newline().0);
    // The start sticks to the character after it and the end to the one before it, so that at the break of a wrapped row each is drawn on its own row.
    (CCursor { index: CharIndex(before), prefer_next_row: true }, CCursor { index: CharIndex(before.saturating_add(length)), prefer_next_row: false })
}

/// Home and End, as the box has them: with or without Shift, to [`row_ends`] of the caret, taken from the keys before the text edit sees them (Ctrl with either is the
/// start and the end of the whole text, which are the galley's own, and so is any other chord). `galley` is the last frame's, which is the text's as long as the keys
/// come in a frame of their own.
pub(super) fn logical_home_and_end(ctx: &Context, field: Id, galley: &Galley) {
    // Taken by hand: `consume_key` with no modifiers would take Shift with the key too (it matches logically).
    let mut taken: Vec<(Key, bool)> = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| match event {
            Event::Key { key: key @ (Key::Home | Key::End), pressed: true, modifiers, .. }
                if !(modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd) =>
            {
                taken.push((*key, modifiers.shift));
                false
            }
            _ => true,
        });
    });
    for (key, shift) in taken {
        let mut state = TextEditState::load(ctx, field).unwrap_or_default();
        let Some(range) = state.cursor.range(galley) else { continue };
        let (start, end) = row_ends(galley, &range.primary);
        let to = if key == Key::Home { start } else { end };
        let moved = if shift { CCursorRange { primary: to, secondary: range.secondary, h_pos: None } } else { CCursorRange::one(to) };
        state.cursor.set_char_range(Some(moved));
        state.store(ctx, field);
    }
}

/// How far in the room is from the box's left and right edges, in points, whole points (a margin is whole points) and never so much that the text has no room:
/// `width` is the box's, and at least a third of it stays for the text.
pub(super) fn indents_in_points(look: &EditorLook, per_inch: f32, width: f32) -> (i8, i8) {
    let room = (width / 3.0).max(0.0);
    let points = |inches: f64| ((inches * f64::from(per_inch)) as f32).clamp(0.0, 127.0).round();
    let (left, right) = (points(look.left), points(look.right));
    let over = (left + right - (width - room).max(0.0)).max(0.0);
    let scale = if left + right > 0.0 { 1.0 - over / (left + right) } else { 1.0 };
    ((left * scale).floor() as i8, (right * scale).floor() as i8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(direction: Direction, align: HAlign) -> EditorParagraph {
        EditorParagraph { direction, align, ind_left: 0.0, ind_right: 0.0 }
    }

    #[test]
    fn left_to_right_text_has_no_look() {
        let paragraphs = [para(Direction::LeftToRight, HAlign::Left)];
        assert_eq!(editor_look(&paragraphs, "plain text, 123 and \u{e9}\u{4e2d}"), None);
        assert_eq!(editor_look(&[], ""), None);
    }

    #[test]
    fn a_paragraph_that_reads_right_to_left_or_a_letter_that_does_has_a_look() {
        let flagged = editor_look(&[para(Direction::RightToLeft, HAlign::Left)], "abc").unwrap();
        assert_eq!(flagged.direction, TextDirection::RightToLeft);
        let lettered = editor_look(&[para(Direction::LeftToRight, HAlign::Left)], "abc \u{5d0}").unwrap();
        assert_eq!(lettered.direction, TextDirection::LeftToRight, "the cell decides, and not the first strong letter");
        let new_box = editor_look(&[], "\u{627}").unwrap();
        assert_eq!((new_box.direction, new_box.halign), (TextDirection::LeftToRight, Align::Center));
    }

    #[test]
    fn alignment_is_mirrored_for_a_paragraph_that_reads_right_to_left() {
        let side = |direction, align| {
            let look = editor_look(&[para(direction, align)], "\u{5d0}").unwrap();
            (look.halign, look.justify)
        };
        assert_eq!(side(Direction::RightToLeft, HAlign::Left), (Align::RIGHT, false));
        assert_eq!(side(Direction::RightToLeft, HAlign::Right), (Align::LEFT, false));
        assert_eq!(side(Direction::RightToLeft, HAlign::Center), (Align::Center, false));
        assert_eq!(side(Direction::RightToLeft, HAlign::Justify), (Align::RIGHT, true));
        assert_eq!(side(Direction::LeftToRight, HAlign::Left), (Align::LEFT, false));
        assert_eq!(side(Direction::LeftToRight, HAlign::Right), (Align::RIGHT, false));
        assert_eq!(side(Direction::LeftToRight, HAlign::Distributed), (Align::LEFT, true));
    }

    #[test]
    fn paragraphs_that_disagree_take_each_their_own_start() {
        let paragraphs = [para(Direction::RightToLeft, HAlign::Left), para(Direction::LeftToRight, HAlign::Left)];
        let look = editor_look(&paragraphs, "\u{5d0}\nabc").unwrap();
        assert_eq!((look.direction, look.halign, look.min_is_start), (TextDirection::Auto, Align::LEFT, true));
    }

    #[test]
    fn the_start_indent_is_on_the_right_for_a_paragraph_that_reads_right_to_left() {
        let mut p = para(Direction::RightToLeft, HAlign::Left);
        (p.ind_left, p.ind_right) = (0.5, 0.25);
        let look = editor_look(&[p], "\u{5d0}").unwrap();
        assert_eq!((look.left, look.right), (0.25, 0.5));
        p.direction = Direction::LeftToRight;
        let look = editor_look(&[p], "\u{5d0}").unwrap();
        assert_eq!((look.left, look.right), (0.5, 0.25));
    }

    #[test]
    fn hostile_numbers_are_held_to_a_room() {
        let mut p = para(Direction::RightToLeft, HAlign::Left);
        (p.ind_left, p.ind_right) = (1.0e15, f64::NAN);
        let look = editor_look(&[p], "\u{5d0}").unwrap();
        assert_eq!((look.left, look.right), (0.0, MOST_INDENT), "a number that is no number is no indent, and a huge one is the most");
        let (left, right) = indents_in_points(&look, 96.0, 288.0);
        assert!(i32::from(left) + i32::from(right) <= 288 - 96, "a third of the box stays for the text: {left} and {right}");
        assert_eq!(indents_in_points(&look, f32::NAN, f32::INFINITY), (0, 0));
    }

    #[test]
    fn the_one_section_covers_the_text() {
        let look = editor_look(&[para(Direction::RightToLeft, HAlign::Left)], "x").unwrap();
        for text in ["", "a", "a\n", "a\nb", "\n\n", "\u{5d0}\u{5d1}\nabc\n"] {
            let job = layout_job(&look, text, TextFormat::default(), 100.0, None);
            assert_eq!(job.sections.len(), 1, "{text:?}");
            assert_eq!(job.sections.first().map(|s| (s.byte_range.start.0, s.byte_range.end.0)), Some((0, text.len())));
            assert!(job.keep_trailing_whitespace && job.wrap.max_width == 100.0);
            job.debug_sanity_check();
        }
    }
}
