//! The text box edits a right-to-left paragraph right to left, as the drawing shows it.
//!
//! A paragraph whose `Flags` is 1 stands against the right of the box when its alignment is 0 and against the left when it is 2, the start indent is on its
//! right and the stop indent on its left, the first word of a line is the rightmost, and the caret keeps to the screen: Left and Right move it visually,
//! Home and End go to the first and the last character of the row. Text with no right-to-left paragraph and no right-to-left letter is laid out as it
//! always was (the box's text centred, one face and one size).
//!
//! The box's faces are the window's: Hack and Ubuntu Light carry no Hebrew and no Arabic, so every window here is given a face that has the letters the
//! tests type (as the window is given one from the machine at run time, `fonts.rs`), and no glyph is ever missing.

use std::sync::Arc;

use diacraft_fonts::synth::SynthFont;
use diacraft_ui_egui::fonts::{font_definitions, strong_family};
use egui::{Event, FontData, FontFamily, FontId, Key, Modifiers, Pos2, Rect};
use serde_json::json;

use crate::editing::{click_at, edit_window, key};
use crate::support::{Rest, Window, click, move_to, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];

/// Hebrew: "shalom olam" (hello, world).
const HEBREW: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}";
/// Arabic: "marhaban bil-alam" (hello, world).
const ARABIC: &str = "\u{645}\u{631}\u{62D}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}";
/// The letters the tests type, for the face the window is given.
const LETTERS: &str = "\u{5D0}\u{5D1}\u{5D2}\u{5D3}\u{5D4}\u{5D5}\u{5D6}\u{5D7}\u{5D8}\u{5D9}\u{5DA}\u{5DB}\u{5DC}\u{5DD}\u{5DE}\u{5DF}\u{5E0}\u{5E1}\u{5E2}\u{5E3}\u{5E4}\u{5E5}\u{5E6}\u{5E7}\u{5E8}\u{5E9}\u{5EA}\u{627}\u{628}\u{62A}\u{62D}\u{631}\u{639}\u{644}\u{645}\u{647}\u{64A}";

/// The advance of every letter of the face the tests give the window, in thousandths of an em.
const ADVANCE: u16 = 600;

/// Gives the window the face of the tests behind its proportional faces, as `fonts.rs` gives it one from the machine.
fn with_script_faces(w: &mut Window) {
    let mut definitions = font_definitions();
    let face = SynthFont::new("Test Script Face").chars(LETTERS, ADVANCE).build();
    definitions.font_data.insert("Test-Script".to_owned(), Arc::new(FontData::from_owned(face)));
    for family in [FontFamily::Proportional, strong_family()] {
        definitions.families.entry(family).or_default().push("Test-Script".to_owned());
    }
    w.ctx.set_fonts(definitions);
    w.step();
}

/// The cells a paragraph of the shape is given.
#[derive(Clone, Copy, Default)]
struct Para {
    /// `Flags`: 1 reads right to left.
    flags: Option<u32>,
    /// `HorzAlign`.
    align: Option<&'static str>,
    ind_left: Option<f64>,
    ind_right: Option<f64>,
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// A window with a text box of `width` inches holding `text`, its paragraph formatted as `para` says, selected at 100% zoom. → the shape.
fn shape_with(text: &str, width: f64, para: Para) -> (Window, u32) {
    let mut w = edit_window(WIDE);
    with_script_faces(&mut w);
    let page = page_id(&w);
    let made = w.state_mut().execute("text.box", &json!({"page": page, "x": 3.0, "y": 6.0, "width": width, "height": 1.0, "text": text})).unwrap();
    let id = made["id"].as_u64().unwrap() as u32;
    w.rest();
    let mut format = json!({"ids": [id], "page": page});
    if let (Some(flags), Some(map)) = (para.flags, format.as_object_mut()) {
        map.insert("direction".to_owned(), json!(if flags == 1 { "rtl" } else { "ltr" }));
    }
    if let (Some(align), Some(map)) = (para.align, format.as_object_mut()) {
        map.insert("align".to_owned(), json!(align));
    }
    if para.flags.is_some() || para.align.is_some() {
        run_command(&mut w, "text.format", format);
    }
    for (cell, value) in [("IndLeft", para.ind_left), ("IndRight", para.ind_right)] {
        if let Some(value) = value {
            run_command(&mut w, "sheet.set_cell", json!({"id": id, "page": page, "cell": format!("Paragraph.{cell}"), "formula": value.to_string()}));
        }
    }
    (w, id)
}

/// A shape formatted as `para` with `text` in it, and its box open.
fn editing(text: &str, width: f64, para: Para) -> Window {
    let (mut w, _) = shape_with(text, width, para);
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    w.rest();
    w
}

/// The room the box's text edit took, and the left and right ends of what its rows show.
fn extent(w: &Window) -> (Rect, f32, f32) {
    let area = w.state().edited_text_area().expect("the box has been drawn");
    let rows = w.state().edited_text_rows();
    assert!(!rows.is_empty(), "the box has rows");
    let left = rows.iter().map(|(rect, _)| rect.left()).fold(f32::INFINITY, f32::min);
    let right = rows.iter().map(|(rect, _)| rect.right()).fold(f32::NEG_INFINITY, f32::max);
    (area, left, right)
}

/// Where a caret in front of character `index` is, left to right on the screen (the place the caret has there when it sticks to the character after it).
fn caret_x(w: &Window, index: usize) -> f32 {
    w.state().edited_text_caret_rect(index).expect("the box has been drawn").center().x
}

/// Where the box's own caret is, left to right on the screen.
fn caret_now(w: &Window) -> f32 {
    w.state().edited_text_caret().expect("the box has the keyboard and has been drawn").center().x
}

/// How far the room of the text is from the left and from the right edge of the box.
fn margins(w: &Window) -> (f32, f32) {
    let outer = w.state().element("shape.text").expect("the box is drawn").rect;
    let area = w.state().edited_text_area().expect("the box has been drawn");
    (area.left() - outer.left(), outer.right() - area.right())
}

/// The characters selected, which for a caret are one place twice.
fn selected(w: &Window) -> (usize, usize) {
    w.state().edited_text_selection().expect("the box has been drawn")
}

const RTL: Para = Para { flags: Some(1), align: None, ind_left: None, ind_right: None };

// ---- direction and alignment ---------------------------------------------------------------------------------------------------------

/// `Flags` 1 is the base direction of the paragraph, whatever its first letter: the `!` that ends "abc!" is neutral, takes the paragraph's direction
/// and stands at the left of the letters, where in a left-to-right paragraph it stands at their right. (The words of "one two" are one left-to-right run in either,
/// so the direction shows in what stands beside the text.)
#[test]
fn a_right_to_left_paragraph_puts_what_ends_it_on_the_left() {
    let w = editing("abc!", 3.0, Para { align: Some("left"), ..RTL });
    // A caret before `a` and one after the `!`: the end of the text is at the left end of the row.
    let (before_a, after_mark) = (caret_x(&w, 0), caret_x(&w, 4));
    assert!(after_mark < before_a, "the end of the text ({after_mark}) is left of the start of `abc` ({before_a})");
}

/// The same text with `Flags` 0 reads left to right (the pin of the case above).
#[test]
fn a_left_to_right_paragraph_puts_what_ends_it_on_the_right() {
    let w = editing("abc!", 3.0, Para { flags: Some(0), align: Some("left"), ..Para::default() });
    let (before_a, after_mark) = (caret_x(&w, 0), caret_x(&w, 4));
    assert!(after_mark > before_a, "the end of the text ({after_mark}) is right of the start of `abc` ({before_a})");
}

/// Alignment 0 puts the rows against the right of the box, 2 against the left, and 1 between, as the drawing has them.
#[test]
fn alignment_is_mirrored_in_a_right_to_left_paragraph() {
    for (align, name) in [("left", "0"), ("right", "2"), ("center", "1")] {
        let w = editing(HEBREW, 3.0, Para { align: Some(align), ..RTL });
        let (area, left, right) = extent(&w);
        match name {
            "0" => assert!(
                (area.right() - right).abs() < 1.5,
                "alignment 0 stands against the right: the row ends at {right}, the room at {}",
                area.right()
            ),
            "2" => assert!(
                (left - area.left()).abs() < 1.5,
                "alignment 2 stands against the left: the row starts at {left}, the room at {}",
                area.left()
            ),
            _ => assert!(
                ((left + right) / 2.0 - area.center().x).abs() < 1.5,
                "alignment 1 is centred: {left} to {right} in {} to {}",
                area.left(),
                area.right()
            ),
        }
    }
}

/// An empty paragraph that reads right to left puts the caret at the right (the start of the text), where an empty left-to-right one has it at the left.
#[test]
fn an_empty_right_to_left_paragraph_has_its_caret_at_the_right() {
    let mut w = edit_window(WIDE);
    with_script_faces(&mut w);
    let page = page_id(&w);
    let made = w.state_mut().execute("text.box", &json!({"page": page, "x": 3.0, "y": 6.0, "width": 3.0, "height": 1.0, "text": "x"})).unwrap();
    let id = made["id"].as_u64().unwrap() as u32;
    run_command(&mut w, "text.format", json!({"ids": [id], "page": page, "direction": "rtl", "align": "left"}));
    run_command(&mut w, "shape.set_text", json!({"id": id, "page": page, "text": ""}));
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    w.rest();
    let area = w.state().edited_text_area().expect("the box has been drawn");
    let x = caret_now(&w);
    assert!(x > area.center().x, "the caret ({x}) is in the right half of the room ({} to {})", area.left(), area.right());
}

// ---- indents -------------------------------------------------------------------------------------------------------------------------

/// The start indent (`IndLeft`) is on the right and the stop indent (`IndRight`) on the left: half an inch and a quarter, at 96 pixels to the inch. The room of the
/// text is that much further in from the box's sides than it is with no indent, and the rows stand against the right of it.
#[test]
fn the_start_indent_is_on_the_right_and_the_stop_indent_on_the_left() {
    let plain = editing(HEBREW, 3.0, Para { align: Some("left"), ..RTL });
    let (left0, right0) = margins(&plain);
    let w = editing(HEBREW, 3.0, Para { align: Some("left"), ind_left: Some(0.5), ind_right: Some(0.25), ..RTL });
    let (left, right) = margins(&w);
    assert!((right - right0 - 48.0).abs() < 2.5, "the start indent (half an inch) is on the right: {} more than with none", right - right0);
    assert!((left - left0 - 24.0).abs() < 2.5, "the stop indent (a quarter) is on the left: {} more than with none", left - left0);
    let (area, _, row_right) = extent(&w);
    assert!((area.right() - row_right).abs() < 1.5, "and the row stands against the right of the room that is left");
}

// ---- the caret ------------------------------------------------------------------------------------------------------------------------

/// Home goes to the start of the text, which in a right-to-left paragraph is its right end; End goes to the other end.
#[test]
fn home_and_end_go_to_the_logical_ends_of_a_right_to_left_line() {
    let mut w = editing(HEBREW, 3.0, Para { align: Some("left"), ..RTL });
    let n = HEBREW.chars().count();
    key(&mut w, Key::Home, Modifiers::NONE);
    assert_eq!(selected(&w), (0, 0), "Home is the start of the text");
    let (_, left, right) = extent(&w);
    let at_home = caret_now(&w);
    assert!((at_home - right).abs() < 3.0, "and that is the right end of the row ({at_home} against {right})");
    key(&mut w, Key::End, Modifiers::NONE);
    assert_eq!(selected(&w), (n, n), "End is the end of the text");
    assert!((caret_now(&w) - left).abs() < 3.0, "the left end of the row");
}

/// Home and End go to the logical start and end of the text (the first and the last place of the characters) in text of both directions, whichever side of the
/// row each is on: a paragraph that reads left to right and ends in Hebrew has its last character at the middle of the row, and one that reads right to left and
/// ends in Latin letters has it there too.
#[test]
fn home_and_end_are_the_logical_ends_in_text_of_both_directions() {
    for (text, flags) in
        [(format!("abc {HEBREW}"), 0), (format!("{HEBREW} abc"), 1), (format!("{HEBREW} abc {HEBREW}"), 0), (format!("abc {HEBREW} abc"), 1)]
    {
        let mut w = editing(&text, 4.0, Para { flags: Some(flags), align: Some("left"), ..Para::default() });
        let n = text.chars().count();
        key(&mut w, Key::End, Modifiers::NONE);
        assert_eq!(selected(&w), (n, n), "End in {text:?} with Flags {flags}");
        key(&mut w, Key::Home, Modifiers::NONE);
        assert_eq!(selected(&w), (0, 0), "Home in {text:?} with Flags {flags}");
        key(&mut w, Key::End, Modifiers::SHIFT);
        assert_eq!(selected(&w), (0, n), "Shift+End from the start in {text:?} with Flags {flags}");
        key(&mut w, Key::ArrowLeft, Modifiers::NONE);
        key(&mut w, Key::Home, Modifiers::SHIFT);
        let (from, to) = selected(&w);
        assert_eq!(from, 0, "Shift+Home selects back to the start in {text:?} with Flags {flags}: {from} to {to}");
    }
}

/// Left and Right move the caret on the screen in a right-to-left paragraph, one letter a press: from the end of the text, at the left, Right goes towards the
/// start of the text.
#[test]
fn right_moves_the_caret_to_the_right_on_the_screen() {
    let mut w = editing(HEBREW, 3.0, Para { align: Some("left"), ..RTL });
    let n = HEBREW.chars().count();
    key(&mut w, Key::End, Modifiers::NONE);
    let mut last = caret_now(&w);
    for step in 1..=3 {
        key(&mut w, Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected(&w), (n - step, n - step), "Right moves back through the text");
        let x = caret_now(&w);
        assert!(x > last, "step {step}: the caret moved to the right ({last} to {x})");
        last = x;
    }
    key(&mut w, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(selected(&w), (n - 2, n - 2), "and Left goes back the way it came");
}

/// In text of both directions each press of Right moves the caret to the right on the screen, however the characters are ordered in the text, until the row ends.
#[test]
fn right_always_moves_the_caret_to_the_right_in_mixed_text() {
    let text = format!("{HEBREW} abc 123 {ARABIC}");
    let mut w = editing(&text, 4.0, Para { align: Some("left"), ..RTL });
    key(&mut w, Key::Home, Modifiers::NONE);
    let n = text.chars().count();
    // Walk the caret to the leftmost place by Left, then back by Right: its x never goes the wrong way.
    let mut xs = Vec::new();
    for _ in 0..=n {
        xs.push(caret_now(&w));
        key(&mut w, Key::ArrowLeft, Modifiers::NONE);
    }
    for pair in xs.windows(2) {
        assert!(pair[1] <= pair[0] + 0.5, "Left moved the caret to the right: {xs:?}");
    }
    let mut ys = Vec::new();
    for _ in 0..=n {
        ys.push(caret_now(&w));
        key(&mut w, Key::ArrowRight, Modifiers::NONE);
    }
    for pair in ys.windows(2) {
        assert!(pair[1] >= pair[0] - 0.5, "Right moved the caret to the left: {ys:?}");
    }
}

/// Shift and the arrows select by the screen too: from the end of an Arabic line, at the left, Shift+Right selects the letters to the right of it, which are the
/// ones before it in the text.
#[test]
fn shift_and_the_arrows_select_by_the_screen() {
    let mut w = editing(ARABIC, 3.0, Para { align: Some("left"), ..RTL });
    let n = ARABIC.chars().count();
    key(&mut w, Key::End, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(selected(&w), (n - 2, n), "two letters, counted from the end of the text");
    key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(selected(&w), (n - 1, n), "Shift+Left gives one back");
}

/// Shift and the arrows in text of both directions: from the start of the text, each Shift+Left moves the end of the selection to the left on the screen and each
/// Shift+Right back to the right, however the characters between are ordered, and the other end stays where the selection began.
#[test]
fn shift_and_the_arrows_extend_the_selection_by_the_screen_in_mixed_text() {
    let text = format!("{HEBREW} abc 123 {ARABIC}");
    let mut w = editing(&text, 4.0, Para { align: Some("left"), ..RTL });
    key(&mut w, Key::Home, Modifiers::NONE);
    let n = text.chars().count();
    let mut xs = vec![caret_now(&w)];
    for _ in 0..n {
        key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
        xs.push(caret_now(&w));
        assert_eq!(selected(&w).0, 0, "the selection still begins at the start of the text");
    }
    for pair in xs.windows(2) {
        assert!(pair[1] <= pair[0] + 0.5, "Shift+Left moved the end of the selection to the right: {xs:?}");
    }
    let mut ys = vec![caret_now(&w)];
    for _ in 0..n {
        key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
        ys.push(caret_now(&w));
    }
    for pair in ys.windows(2) {
        assert!(pair[1] >= pair[0] - 0.5, "Shift+Right moved the end of the selection to the left: {ys:?}");
    }
    assert_eq!(selected(&w), (0, 0), "back at the start of the text, nothing is selected");
}

// ---- the pointer ----------------------------------------------------------------------------------------------------------------------

/// A click at a place in a right-to-left row puts the caret there: at the right end of the row it is the start of the text, at the left end the end of it.
#[test]
fn a_click_puts_the_caret_by_the_rows_direction() {
    let mut w = editing(HEBREW, 3.0, Para { align: Some("left"), ..RTL });
    let n = HEBREW.chars().count();
    let (_, left, right) = extent(&w);
    let y = w.state().edited_text_rows()[0].0.center().y;
    click(&mut w, Pos2::new(right - 1.0, y));
    w.rest();
    assert_eq!(selected(&w), (0, 0), "the right end of the row is the start of the text");
    click(&mut w, Pos2::new(left + 1.0, y));
    w.rest();
    assert_eq!(selected(&w), (n, n), "the left end of the row is the end of the text");
}

/// A drag from the right end of an Arabic row to its middle selects the first half of the text.
#[test]
fn a_drag_selects_from_where_it_started() {
    let mut w = editing(ARABIC, 3.0, Para { align: Some("left"), ..RTL });
    let n = ARABIC.chars().count();
    let (_, left, right) = extent(&w);
    let y = w.state().edited_text_rows()[0].0.center().y;
    let (from, to) = (Pos2::new(right - 1.0, y), Pos2::new((left + right) / 2.0, y));
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    crate::support::button(&mut w, from, true);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    crate::support::button(&mut w, to, false);
    w.rest();
    let (start, end) = selected(&w);
    assert_eq!(start, 0, "the selection starts where the drag did, the start of the text");
    assert!((n / 2).abs_diff(end) <= 2, "and ends near the middle of it: {end} of {n}");
}

// ---- left to right and mixed paragraphs ----------------------------------------------------------------------------------------------

/// A paragraph with no right-to-left letter and `Flags` 0 is laid out as the box always laid out text: the rows centred in the room, whatever the alignment cell
/// says (the box has one face and one alignment, and that is not changed for left-to-right text).
#[test]
fn a_left_to_right_paragraph_is_laid_out_as_it_always_was() {
    for align in ["left", "right", "center"] {
        let w = editing("one two three", 3.0, Para { flags: Some(0), align: Some(align), ..Para::default() });
        let (area, left, right) = extent(&w);
        assert!(
            ((left + right) / 2.0 - area.center().x).abs() < 1.5,
            "alignment {align}: the row ({left} to {right}) is centred in the room ({} to {})",
            area.left(),
            area.right()
        );
    }
}

/// A left-to-right paragraph with Arabic in it: the base direction is the cell's, so the Arabic that starts the paragraph is at its left and the Latin after it to
/// the right of it, where egui's own choice by the first strong letter would have turned the whole paragraph round.
#[test]
fn a_left_to_right_paragraph_with_arabic_reads_left_to_right() {
    let text = format!("{} abc", &ARABIC[..ARABIC.char_indices().nth(5).map_or(ARABIC.len(), |(i, _)| i)]);
    let w = editing(&text, 3.0, Para { flags: Some(0), align: Some("left"), ..Para::default() });
    // After the fifth letter of the Arabic, and after the `a` of `abc` (the Arabic has five letters, a space, `abc`).
    let (in_arabic, in_latin) = (caret_x(&w, 2), caret_x(&w, 7));
    assert!(in_arabic < in_latin, "the Arabic ({in_arabic}) is to the left of the Latin ({in_latin})");
}

/// A right-to-left paragraph with a number and Latin letters in it: the runs are ordered from the right, the first run of the text on the right, and each run of
/// Latin letters or digits reads left to right (the number is not turned round).
#[test]
fn a_right_to_left_paragraph_with_a_number_and_latin_orders_its_runs_from_the_right() {
    let text = format!("{HEBREW} 123 abc");
    let w = editing(&text, 4.0, Para { align: Some("left"), ..RTL });
    let hebrew_n = HEBREW.chars().count();
    // After the second Hebrew letter, after the `2` of 123, and after the `b` of abc.
    let (in_hebrew, in_number, in_latin) = (caret_x(&w, 2), caret_x(&w, hebrew_n + 3), caret_x(&w, hebrew_n + 7));
    assert!(in_hebrew > in_number && in_number > in_latin, "Hebrew {in_hebrew}, then the number {in_number}, then Latin {in_latin}, from the right");
    // `123` and `abc` read left to right: the second letter is to the right of the first.
    assert!(caret_x(&w, hebrew_n + 3) > caret_x(&w, hebrew_n + 2), "123 reads left to right");
    assert!(caret_x(&w, hebrew_n + 7) > caret_x(&w, hebrew_n + 6), "abc reads left to right");
}

// ---- input methods and shaping ---------------------------------------------------------------------------------------------------------

/// An input method composes text in a right-to-left paragraph: the uncommitted text is in the box while it is composed and the committed text takes its place,
/// as in any other box.
#[test]
fn an_input_method_composes_in_a_right_to_left_paragraph() {
    let mut w = editing("", 3.0, Para { align: Some("left"), ..RTL });
    w.input_mut().events.push(Event::Ime(egui::ImeEvent::Preedit { text: "\u{5D0}\u{5D1}".to_owned(), active_range_chars: None }));
    w.step();
    assert_eq!(w.state().edited_text(), Some("\u{5D0}\u{5D1}"), "the composition is shown in the box");
    w.input_mut().events.push(Event::Ime(egui::ImeEvent::Commit("\u{5D0}\u{5D1}\u{5D2}".to_owned())));
    w.step();
    w.rest();
    assert_eq!(w.state().edited_text(), Some("\u{5D0}\u{5D1}\u{5D2}"), "and the committed text replaces it");
    let (area, _, right) = extent(&w);
    assert!((area.right() - right).abs() < 40.0, "the row is still at the right: {right} in a room that ends at {}", area.right());
}

/// A letter pair that the face joins into one glyph is shaped in the box: two characters, one glyph, in a right-to-left paragraph as in a left-to-right one.
#[test]
fn shaping_is_kept() {
    let mut w = edit_window(WIDE);
    let mut definitions = font_definitions();
    let mut face = SynthFont::new("Test Joining Face").chars(LETTERS, ADVANCE);
    face.ligatures.push(('\u{644}', '\u{627}', 700));
    definitions.font_data.insert("Test-Join".to_owned(), Arc::new(FontData::from_owned(face.build())));
    for family in [FontFamily::Proportional, strong_family()] {
        definitions.families.entry(family).or_default().push("Test-Join".to_owned());
    }
    w.ctx.set_fonts(definitions);
    w.step();
    let page = page_id(&w);
    let made =
        w.state_mut().execute("text.box", &json!({"page": page, "x": 3.0, "y": 6.0, "width": 3.0, "height": 1.0, "text": "\u{644}\u{627}"})).unwrap();
    let id = made["id"].as_u64().unwrap() as u32;
    run_command(&mut w, "text.format", json!({"ids": [id], "page": page, "direction": "rtl", "align": "left"}));
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    w.rest();
    let rows = w.state().edited_text_rows();
    let width = rows.iter().map(|(rect, _)| rect.width()).fold(0.0, f32::max);
    // The font is 16 pixels at 100%: two letters side by side are 2 x 9.6 wide, the joined pair 700 thousandths of an em, 11.2.
    assert!((width - 11.2).abs() < 1.0, "the two letters are one joined glyph, {width} wide");
}

// ---- the fonts -----------------------------------------------------------------------------------------------------------------------

/// The window's own faces hold a face for Hebrew and one for Arabic when the machine has them: with a database that has a face with the letters, the interface
/// fonts the window is given have it behind the proportional faces, so the box shows the letters and not a box for each.
#[test]
fn the_interface_fonts_carry_faces_for_arabic_and_hebrew() {
    let mut db = diacraft_fonts::FontDb::empty();
    assert_eq!(db.add_font_data(SynthFont::new("Test Arabic and Hebrew").chars(LETTERS, ADVANCE).build()).added, 1);
    let mut w = edit_window(WIDE);
    w.ctx.set_fonts(diacraft_ui_egui::fonts::font_definitions_with(&db));
    w.step();
    for family in [FontFamily::Proportional, strong_family()] {
        for (letter, script) in [('\u{5D0}', "Hebrew"), ('\u{627}', "Arabic")] {
            let has = w.ctx.fonts_mut(|fonts| fonts.has_glyph(&FontId::new(14.0, family.clone()), letter));
            assert!(has, "{family:?} has no glyph for {script}, though a face of the machine has");
        }
    }
}

/// A face that has one script serves that script and not the other, and a face that is not a plain upright one (here a bold italic) is not taken, so a machine
/// with only such faces is left with the boxes it had.
#[test]
fn a_face_serves_the_script_it_has_and_only_a_plain_upright_one_is_taken() {
    let has = |db: &diacraft_fonts::FontDb, letter: char| {
        let mut w = edit_window(WIDE);
        w.ctx.set_fonts(diacraft_ui_egui::fonts::font_definitions_with(db));
        w.step();
        w.ctx.fonts_mut(|fonts| fonts.has_glyph(&FontId::proportional(14.0), letter))
    };
    let mut hebrew_only = diacraft_fonts::FontDb::empty();
    assert_eq!(
        hebrew_only
            .add_font_data(SynthFont::new("Test Hebrew").chars(&LETTERS[..LETTERS.char_indices().nth(27).map_or(0, |(at, _)| at)], ADVANCE).build())
            .added,
        1
    );
    assert!(has(&hebrew_only, '\u{5D0}'), "Hebrew is served");
    assert!(!has(&hebrew_only, '\u{627}'), "Arabic is not, by a face that lacks it");
    let mut slanted = diacraft_fonts::FontDb::empty();
    let face = SynthFont { bold: true, italic: true, ..SynthFont::new("Test Slanted").chars(LETTERS, ADVANCE) };
    assert_eq!(slanted.add_font_data(face.build()).added, 1);
    assert!(!has(&slanted, '\u{5D0}') && !has(&slanted, '\u{627}'), "a bold italic face is not taken for either script");
}

// ---- paragraphs of both directions in one shape -------------------------------------------------------------------------------------

/// A shape with a paragraph that reads right to left and one that reads left to right: one job has one direction, so each paragraph takes its own from its
/// first strong letter and its alignment 0 is its own start: the short Hebrew line stands against the right of the long Latin one, and the Latin one at the left.
#[test]
fn paragraphs_of_both_directions_each_start_on_their_own_side() {
    let latin = "abcdefghijklmnopqrstuvwxyz abc";
    let (mut w, id) = shape_with(&format!("\u{5D0}\u{5D1}\u{5D2}\n{latin}"), 3.0, Para { flags: Some(0), align: Some("left"), ..Para::default() });
    let page = page_id(&w);
    run_command(&mut w, "text.format", json!({"ids": [id], "page": page, "start": 0, "end": 3, "direction": "rtl"}));
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    w.rest();
    let rows = w.state().edited_text_rows();
    let area = w.state().edited_text_area().expect("the box has been drawn");
    assert_eq!(rows.len(), 2, "two rows: {rows:?}");
    assert!((rows[1].0.left() - area.left()).abs() < 1.5, "the Latin row stands at the left ({} in a room from {})", rows[1].0.left(), area.left());
    assert!(
        (rows[0].0.right() - rows[1].0.right()).abs() < 1.5,
        "the Hebrew row stands against the right of the widest row ({} and {})",
        rows[0].0.right(),
        rows[1].0.right()
    );
}
