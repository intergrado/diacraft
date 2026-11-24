//! Editing the text of a shape of an `.odg` through the commands: typing across formatted runs, formatting part of a label, adding and removing a paragraph
//! (the writer refused each of these before: retext). Each is saved, reopened and
//! compared with the session, and saved again as no edit.

use serde_json::{Value, json};

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing, part_of, rect};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, four, refused, save, text};
use crate::run;

/// What `shape.formatting` says of the characters `start..end` of a shape of the session's drawing, as the fields a person sees.
fn formatting(s: &mut diacraft_engine::Session, name: &str, start: usize, end: usize) -> Value {
    let id = id_of(s.document().unwrap(), 0, name);
    let answer = run(s, "shape.formatting", json!({ "ids": [id], "start": start, "end": end }));
    let one = &answer["shapes"][0];
    json!({ "bold": one["bold"], "italic": one["italic"], "size": one["size"], "font": one["font"], "color": one["color"], "align": one["align"] })
}

/// A session opened on a file that holds the drawing, then reopened from the file after each save.
fn reopened(dir: &Dir, name: &str) -> diacraft_engine::Session {
    let mut again = diacraft_engine::Session::new();
    run(&mut again, "file.open", json!({ "path": dir.path(name) }));
    again
}

#[test]
fn part_of_a_label_made_bold_is_saved_as_a_span_and_reads_back_bold() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "text.format", json!({ "ids": [a], "start": 1, "end": 3, "bold": true }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let mut again = reopened(&dir, "a.odg");
    for (start, end) in [(0, 1), (1, 3), (3, 5)] {
        assert_eq!(formatting(&mut s, "A", start, end), formatting(&mut again, "A", start, end), "{start}..{end}");
    }
    assert_eq!(formatting(&mut again, "A", 1, 3)["bold"], json!(true));
    assert_eq!(formatting(&mut again, "A", 3, 5)["bold"], json!(false));
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // Typing across the edge of the bold run (the `l` that is bold and the `l` that is not) is written too.
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "XY", "start": 2, "end": 4 }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(text(&s, 0, "A"), "heXYo");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let mut again = reopened(&dir, "a.odg");
    for (start, end) in [(0, 1), (1, 2), (2, 4), (4, 5)] {
        assert_eq!(formatting(&mut s, "A", start, end), formatting(&mut again, "A", start, end), "{start}..{end}");
    }
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

#[test]
fn a_paragraph_added_and_a_paragraph_removed_are_saved_and_read_back() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "hello\nthere\nagain" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(text(&s, 0, "A"), "hello\nthere\nagain");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // The two paragraphs the file has after that, one taken away.
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "hello\nagain" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // A file that holds two paragraphs, set to one.
    let dir = Dir::new();
    let two = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "").replace("<text:p/>", "<text:p>one</text:p><text:p>two</text:p>"));
    let mut s = open_drawing(&dir, &two);
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "one" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A colour and a font a person chose (`RGB(r,g,b)` in a row of the model, and a new name in the document's table of faces) are saved: the colour as an
/// attribute of a text style, the font as a `style:font-face` declaration beside it (the review).
#[test]
fn a_text_colour_and_a_font_a_person_chose_are_saved_and_read_back() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "text.format", json!({ "ids": [a], "start": 1, "end": 3, "color": "#cc0000", "font": "Papyrus", "italic": true }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("fo:color=\"#cc0000\""), "{content}");
    assert!(content.contains("<style:font-face style:name=\"Papyrus\" svg:font-family=\"'Papyrus'\"/>"), "{content}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let mut again = reopened(&dir, "a.odg");
    for (start, end) in [(0, 1), (1, 3), (3, 5)] {
        assert_eq!(formatting(&mut s, "A", start, end), formatting(&mut again, "A", start, end), "{start}..{end}");
    }
    let (run_one, run_two) = (formatting(&mut again, "A", 1, 3), formatting(&mut again, "A", 0, 1));
    assert_eq!((run_one["font"].clone(), run_one["italic"].clone()), (json!("Papyrus"), json!(true)), "{run_one}");
    assert_ne!(run_one["color"], run_two["color"], "the colour is the run's");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    // Another colour for the same run, and a second font: the declaration of the first stays.
    run(&mut s, "text.format", json!({ "ids": [a], "start": 3, "end": 5, "color": "#0000cc", "font": "Impact" }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert_eq!(
        content.matches("<style:font-face ").count(),
        2,
        "{}",
        &content[content.find("<office:font-face-decls").unwrap()..content.find("</office:font-face-decls>").unwrap()]
    );
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A numbered list of two items, the engine's sequence of the review: a paragraph typed at the end, formatted, and saved. The paragraph is
/// written after the list and not inside it, where the reader and LibreOffice would not read it; the file read again has the text the session has.
#[test]
fn a_paragraph_typed_after_a_numbered_list_and_formatted_is_saved_outside_the_list() {
    let list = concat!(
        r#"<text:list text:style-name="N1"><text:list-item><text:p text:style-name="P2">one</text:p></text:list-item>"#,
        r#"<text:list-item><text:p text:style-name="P2">two</text:p></text:list-item></text:list>"#
    );
    let numbered = r#"<text:list-style style:name="N1"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style>"#;
    let dir = Dir::new();
    let mut d = Drawing::new();
    d.auto.push_str(numbered);
    d.auto.push_str(r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#);
    let mut s = open_drawing(&dir, &d.page(&rect("A", (2.0, 3.0, 4.0, 2.0), "").replace("<text:p/>", list)));
    let a = id_of(s.document().unwrap(), 0, "A");
    assert_eq!(text(&s, 0, "A"), "one\ntwo");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "\nnew", "start": 7, "end": 7 }));
    run(&mut s, "text.format", json!({ "ids": [a], "start": 8, "end": 11, "align": "right" }));
    assert_eq!(text(&s, 0, "A"), "one\ntwo\nnew");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("</text:list><text:p text:style-name="), "{content}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let again = reopened(&dir, "a.odg");
    assert_eq!(text(&again, 0, "A"), "one\ntwo\nnew", "the paragraph is read again");
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The same list, two lines typed at the end and only the first formatted (the re-review): the first has a row of its own and goes after
/// the list, the second has the row of the list's items, which would put it inside the list and ahead of the first. The two lines read alike, so the file read
/// back had the same text with the paragraphs swapped, and the save said nothing. They are written in the order the session has them.
#[test]
fn two_paragraphs_typed_after_a_numbered_list_and_the_first_formatted_are_saved_in_order() {
    let list = concat!(
        r#"<text:list text:style-name="N1"><text:list-item><text:p text:style-name="P2">one</text:p></text:list-item>"#,
        r#"<text:list-item><text:p text:style-name="P2">two</text:p></text:list-item></text:list>"#
    );
    let numbered = r#"<text:list-style style:name="N1"><text:list-level-style-number text:level="1" style:num-format="1"/></text:list-style>"#;
    let dir = Dir::new();
    let mut d = Drawing::new();
    d.auto.push_str(numbered);
    d.auto.push_str(r#"<style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#);
    let mut s = open_drawing(&dir, &d.page(&rect("A", (2.0, 3.0, 4.0, 2.0), "").replace("<text:p/>", list)));
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "\nx\nx", "start": 7, "end": 7 }));
    run(&mut s, "text.format", json!({ "ids": [a], "start": 8, "end": 9, "align": "right" }));
    assert_eq!(text(&s, 0, "A"), "one\ntwo\nx\nx");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    assert!(content.contains("</text:list><text:p text:style-name="), "both after the list: {content}");
    assert_eq!(content.matches("<text:list-item>").count(), 2, "no new item of the list: {content}");
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(text(&again, 0, "A"), "one\ntwo\nx\nx");
    // The first line of the two is the one that is aligned right, in the file as in the session.
    for (start, end) in [(8, 9), (10, 11)] {
        assert_eq!(formatting(&mut s, "A", start, end)["align"], formatting(&mut again, "A", start, end)["align"], "{start}..{end}");
    }
    assert_ne!(formatting(&mut again, "A", 8, 9)["align"], formatting(&mut again, "A", 10, 11)["align"]);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Every value of the colour attributes of a part, as the schema of the format says a colour is written (`#` and six hexadecimal digits).
fn colour_values(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["fo:color", "draw:fill-color", "svg:stroke-color", "fo:background-color", "draw:shadow-color"] {
        let pattern = format!(" {name}=\"");
        let mut rest = part;
        while let Some(at) = rest.find(&pattern) {
            let value = &rest[at + pattern.len()..];
            let end = value.find('"').unwrap_or(value.len());
            out.push(format!("{name}={}", &value[..end]));
            rest = &value[end..];
        }
    }
    out
}

/// A run formatted without a colour is written with the colour the engine copies from the palette (black, the first of it): the attribute is `#000000`, as the
/// schema's colour is, and not `000000` (the re-review, parked 8: `Rgba::hex` gives no hash). Every colour attribute of the parts the save
/// wrote is of that form, with a colour a person chose too.
#[test]
fn a_run_formatted_with_the_default_colour_is_saved_with_a_valid_colour() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "text.format", json!({ "ids": [a], "start": 1, "end": 3, "italic": true }));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let bytes = dir.read("a.odg");
    let content = content_of(&bytes);
    assert!(content.contains("fo:color=\"#000000\""), "the default colour of the run: {content}");
    run(&mut s, "text.format", json!({ "ids": [a], "start": 3, "end": 5, "color": "#CC0000", "bold": true }));
    assert_eq!(save(&mut s)["edited"], true);
    let bytes = dir.read("a.odg");
    for name in ["content.xml", "styles.xml"] {
        for value in colour_values(&part_of(&bytes, name)) {
            let (attribute, colour) = value.split_once('=').unwrap();
            let digits = colour.strip_prefix('#').unwrap_or_else(|| panic!("{name}: {attribute} is {colour:?}, with no hash"));
            assert!(digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()), "{name}: {attribute} is {colour:?}");
        }
    }
    assert!(colour_values(&content_of(&bytes)).iter().any(|v| v == "fo:color=#cc0000"), "{:?}", colour_values(&content_of(&bytes)));
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// Text formatting on a shape that holds no text has no run to be written on, and the file would not hold it (the reader makes no row of a paragraph with no
/// character): the save is refused in words, and not saved as no edit (the review).
#[test]
fn text_formatting_on_a_shape_with_no_text_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let b = id_of(s.document().unwrap(), 0, "B");
    run(&mut s, "text.format", json!({ "ids": [b], "bold": true, "size": 18 }));
    let words = refused(&mut s);
    assert!(words.contains("no character"), "{words}");
    assert!(words.contains("edit.undo"), "{words}");
}

/// The characters of a formatted run deleted leave its row in the sheet, with nothing to use it (the engine does not take a row out): the save is made, and
/// what it writes reads back as the session's text. A refusal of such a row would make a person's italics-then-delete unsaveable (the re-review fix
/// round 1, parked 7).
#[test]
fn a_formatted_run_that_was_deleted_leaves_a_row_the_save_does_not_refuse() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "text.format", json!({ "ids": [a], "start": 1, "end": 3, "italic": true }));
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "", "start": 1, "end": 3 }));
    assert_eq!(text(&s, 0, "A"), "hlo");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// The same with the run at the end of a paragraph, and the paragraph left empty: the format is on a paragraph with no character, which the file cannot hold,
/// and the save is refused in words with the way out.
#[test]
fn a_formatted_last_paragraph_whose_characters_were_deleted_is_refused_in_words() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let a = id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "hello\nthere" }));
    run(&mut s, "text.format", json!({ "ids": [a], "start": 6, "end": 11, "bold": true, "size": 18 }));
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "", "start": 6, "end": 11 }));
    assert_eq!(text(&s, 0, "A"), "hello\n");
    let words = refused(&mut s);
    assert!(words.contains("no character") && words.contains("edit.undo"), "{words}");
}
