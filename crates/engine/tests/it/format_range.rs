//! `shape.formatting` for a range of characters: with `start` and `end` it answers what the characters selected in a shape's
//! text box have, each of font, size, bold, italic, underline, strike, colour and alignment as one value or `mixed`, so that a toggle on a range knows
//! the state of exactly those characters (gap U19). Through `Session::execute`, as the control channel's `engine.execute` and MCP's `command_run` call it.
//!
//! The values are those a file holds: sizes of 8, 11 and 12 points as inches on paper, a `Style` that is a bit set (1 bold, 2 italic, 4 underline) and
//! not a single flag, colours as `#rrggbb`.

use std::sync::Arc;

use diacraft_doc::{Document, RowKey, TextPart};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// 12, 11 and 8 points in inches on paper, as a file writes them.
const TWELVE: &str = "0.1666666666666667";
const ELEVEN: &str = "0.1527777777777778";
const EIGHT: &str = "0.1111111111111111";

fn chars(s: &str) -> TextPart {
    TextPart::Chars(Arc::from(s))
}

fn fld(ix: u32, text: &str) -> TextPart {
    TextPart::Fld { ix, text: Arc::from(text) }
}

/// The cells of one `Character` row.
#[derive(Clone, Copy)]
struct Look {
    font: &'static str,
    size: &'static str,
    style: &'static str,
    strike: &'static str,
    color: &'static str,
}

/// Calibri, 12 points, bold and underlined (`Style` 5), not struck, black.
const BASE: Look = Look { font: "Calibri", size: TWELVE, style: "5", strike: "0", color: "#000000" };

/// A shape, 1, with this text and the rows of `Character` and `Paragraph` given (row `n` of the list is `IX` `n`); the root style holds a row 0 of each
/// with the defaults a row that holds no cell reads.
fn shape_of(parts: Vec<TextPart>, looks: &[Look], aligns: &[&str]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(1).text_parts(parts);
    for (ix, look) in (0_u32..).zip(looks) {
        for (name, value) in [("Font", look.font), ("Size", look.size), ("Style", look.style), ("Strikethru", look.strike), ("Color", look.color)] {
            shape = shape.row_cell("Character", None, RowKey::Ix(ix), name, value);
        }
    }
    for (ix, align) in (0_u32..).zip(aligns) {
        shape = shape.row_cell("Paragraph", None, RowKey::Ix(ix), "HorzAlign", align);
    }
    shape
}

fn session_of(shape: ShapeBuilder) -> Session {
    let doc: Document = DocBuilder::new()
        .root(|r| {
            r.row_cell("Character", None, RowKey::Ix(0), "Size", TWELVE).row_cell("Character", None, RowKey::Ix(0), "Color", "#000000").row_cell(
                "Paragraph",
                None,
                RowKey::Ix(0),
                "HorzAlign",
                "1",
            )
        })
        .page(1, PageBuilder::new().shape(shape))
        .build();
    let mut s = blank_session();
    s.edit("fixture", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// The answer for shape 1 over `start..end`.
fn ask(s: &mut Session, start: usize, end: usize) -> Value {
    let r = run(s, "shape.formatting", json!({"ids": [1], "start": start, "end": end}));
    assert_eq!(r["page"], 1);
    assert_eq!(r["shapes"].as_array().map(Vec::len), Some(1), "{r}");
    r["shapes"][0].clone()
}

/// The properties of the answer that a range is about, in the order the reply lists them.
const PROPS: [&str; 8] = ["font", "size", "bold", "italic", "underline", "strike", "color", "align"];

/// The answer's character properties as one object, so that a test says them all at once.
fn state(answer: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for name in PROPS {
        out.insert(name.to_owned(), answer[name].clone());
    }
    Value::Object(out)
}

fn mixed(answer: &Value) -> Vec<String> {
    answer["mixed"].as_array().unwrap_or_else(|| panic!("no `mixed` list in {answer}")).iter().map(|v| v.as_str().unwrap().to_owned()).collect()
}

fn none_mixed(answer: &Value) {
    assert_eq!(mixed(answer), Vec::<String>::new(), "{answer}");
}

fn expected(font: &str, size: f64, style: u32, strike: bool, color: &str, align: &str) -> Value {
    json!({
        "font": font, "size": size, "bold": style & 1 != 0, "italic": style & 2 != 0, "underline": style & 4 != 0, "strike": strike,
        "color": color, "align": align
    })
}

// ---- a fixture of five runs and two paragraphs ----------------------------------------------------------------------------------------

/// `Aaa Bbb Ccc Ddd\nEee`: runs 0 and 1 agree in every property but are rows of their own; run 2 differs in all of them; run 3 is 11 points and plain;
/// run 4 is the second paragraph, right aligned.
///
/// | run | characters | font | size | style | strike | colour | paragraph |
/// |---|---|---|---|---|---|---|---|
/// | 0 | 0..4 `Aaa ` | Calibri | 12 | 5 bold underline | no | #000000 | 0, left |
/// | 1 | 4..8 `Bbb ` | Calibri | 12 | 5 | no | #000000 | 0 |
/// | 2 | 8..12 `Ccc ` | Arial | 8 | 6 italic underline | yes | #336699 | 0 |
/// | 3 | 12..16 `Ddd\n` | Calibri | 11 | 0 | no | #000000 | 0 |
/// | 4 | 16..19 `Eee` | Calibri | 12 | 4 underline | no | #000000 | 1, right |
fn five_runs() -> Session {
    let parts = vec![
        TextPart::Cp(0),
        TextPart::Pp(0),
        chars("Aaa "),
        TextPart::Cp(1),
        chars("Bbb "),
        TextPart::Cp(2),
        chars("Ccc "),
        TextPart::Cp(3),
        chars("Ddd\n"),
        TextPart::Cp(4),
        TextPart::Pp(1),
        chars("Eee"),
    ];
    let looks = [
        BASE,
        BASE,
        Look { font: "Arial", size: EIGHT, style: "6", strike: "1", color: "#336699" },
        Look { size: ELEVEN, style: "0", ..BASE },
        Look { style: "4", ..BASE },
    ];
    session_of(shape_of(parts, &looks, &["0", "2"]))
}

#[test]
fn a_range_inside_one_run_answers_that_runs_state() {
    let mut s = five_runs();
    let a = ask(&mut s, 1, 3);
    none_mixed(&a);
    assert_eq!(state(&a), expected("Calibri", 12.0, 5, false, "#000000", "left"));
    // The same for a range in a run that is not the first, and that is nothing like it.
    let c = ask(&mut s, 9, 11);
    none_mixed(&c);
    assert_eq!(state(&c), expected("Arial", 8.0, 6, true, "#336699", "left"));
    let d = ask(&mut s, 12, 16);
    none_mixed(&d);
    assert_eq!(state(&d), expected("Calibri", 11.0, 0, false, "#000000", "left"));
    // The whole of a run, to its last character.
    assert_eq!(state(&ask(&mut s, 8, 12)), state(&c));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn runs_that_agree_are_not_mixed_though_their_rows_are_two() {
    let mut s = five_runs();
    for (start, end) in [(0, 8), (2, 6), (3, 5), (0, 4), (4, 8)] {
        let a = ask(&mut s, start, end);
        none_mixed(&a);
        assert_eq!(state(&a), expected("Calibri", 12.0, 5, false, "#000000", "left"), "{start}..{end}");
    }
}

#[test]
fn runs_that_differ_make_the_properties_they_differ_in_mixed_and_the_rest_one_value() {
    let mut s = five_runs();
    // Runs 0, 1 and 2: Calibri and Arial, 12 and 8, bold in two and not in the third, italic in the third only, underline in all three (a bit that
    // does not differ stays one value though the number does), struck in the third, two colours. The paragraph is one.
    let a = ask(&mut s, 0, 12);
    assert_eq!(mixed(&a), ["font", "size", "bold", "italic", "strike", "color"]);
    assert_eq!(a["font"], "mixed");
    assert_eq!(a["size"], "mixed");
    assert_eq!(a["bold"], "mixed");
    assert_eq!(a["italic"], "mixed");
    assert_eq!(a["strike"], "mixed");
    assert_eq!(a["color"], "mixed");
    assert_eq!(a["underline"], true, "5 and 6 both hold bit 4");
    assert_eq!(a["align"], "left");
    // Runs 3 and 4 differ in the size and the bits (0 and 4) and in nothing else.
    let b = ask(&mut s, 14, 19);
    assert_eq!(mixed(&b), ["size", "underline", "align"]);
    assert_eq!(b["font"], "Calibri");
    assert_eq!(b["bold"], false);
    assert_eq!(b["italic"], false);
    assert_eq!(b["strike"], false);
    assert_eq!(b["color"], "#000000");
}

/// Two runs, `ab` and `cd`, in two rows that differ in one property; a range over both is mixed in that property alone, and a range in either run is the
/// state of that run.
#[test]
fn each_property_alone_can_be_the_one_that_is_mixed() {
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd")];
    // The base is bold and underlined (5), so a bit that differs is the only thing that does.
    let cases: [(&str, Look, Look); 7] = [
        ("font", BASE, Look { font: "Arial", ..BASE }),
        ("size", BASE, Look { size: ELEVEN, ..BASE }),
        ("bold", Look { style: "4", ..BASE }, BASE),
        ("italic", BASE, Look { style: "7", ..BASE }),
        ("underline", Look { style: "1", ..BASE }, BASE),
        ("strike", BASE, Look { strike: "1", ..BASE }),
        ("color", BASE, Look { color: "#336699", ..BASE }),
    ];
    for (name, first, second) in cases {
        let mut s = session_of(shape_of(parts.clone(), &[first, second], &["1"]));
        let both = ask(&mut s, 0, 4);
        assert_eq!(mixed(&both), [name], "{name}: {both}");
        assert_eq!(both[name], "mixed");
        for other in PROPS.iter().filter(|p| **p != name) {
            assert_ne!(both[*other], "mixed", "{name}: {other} in {both}");
        }
        // Over one run alone, nothing is mixed, and the value is that run's.
        let (in_first, in_second) = (ask(&mut s, 0, 2), ask(&mut s, 2, 4));
        none_mixed(&in_first);
        none_mixed(&in_second);
        assert_ne!(in_first[name], in_second[name], "{name}: the two runs differ in it");
        // Across the join: `b` and `c` are one of each.
        assert_eq!(mixed(&ask(&mut s, 1, 3)), [name]);
    }
}

#[test]
fn paragraphs_that_differ_in_alignment_make_the_alignment_mixed() {
    let parts = vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two\n"), TextPart::Pp(2), chars("three")];
    let mut s = session_of(shape_of(parts, &[BASE], &["0", "2", "3"]));
    let word = |s: &mut Session, start, end| ask(s, start, end)["align"].clone();
    assert_eq!(word(&mut s, 0, 3), "left");
    assert_eq!(word(&mut s, 4, 7), "right");
    assert_eq!(word(&mut s, 8, 13), "justify");
    // A range that reaches into the next paragraph by one character reads both.
    assert_eq!(word(&mut s, 0, 5), "mixed");
    assert_eq!(mixed(&ask(&mut s, 0, 5)), ["align"]);
    assert_eq!(word(&mut s, 3, 9), "mixed");
    // Runs agree throughout: only the alignment is mixed.
    assert_eq!(mixed(&ask(&mut s, 0, 13)), ["align"]);
    // Two paragraphs that hold the same alignment are not mixed; 3 is justify and 4 is distributed, so a justified paragraph and a distributed one are.
    let parts = vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")];
    let mut s = session_of(shape_of(parts, &[BASE], &["4", "4"]));
    assert_eq!(word(&mut s, 0, 7), "distributed");
    let mut s = session_of(shape_of(parts_again(), &[BASE], &["3", "4"]));
    assert_eq!(word(&mut s, 0, 7), "mixed");
}

fn parts_again() -> Vec<TextPart> {
    vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1), chars("two")]
}

#[test]
fn a_run_whose_row_holds_no_cell_reads_what_it_inherits_and_agrees_with_one_that_says_so() {
    // Run 1 names row 7, which nobody holds: a row of another `IX` inherits nothing by its key, so every cell it lacks reads as the renderer reads a
    // missing one (12 points, black, no face, plain). Run 0 says the same in cells, so the two agree. Run 2 is bold. No cell names a face, so the face is
    // the one the renderer draws an unnamed run in, the same for both runs.
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(7), chars("cd"), TextPart::Cp(2), chars("ef")];
    let looks = [Look { font: "0", style: "0", ..BASE }, BASE, Look { font: "0", style: "1", ..BASE }];
    let mut s = session_of(shape_of(parts, &looks, &["1"]));
    let a = ask(&mut s, 0, 4);
    none_mixed(&a);
    assert_eq!(
        state(&a),
        json!({"font": crate::face_query::shown_default(), "size": 12.0, "bold": false, "italic": false, "underline": false, "strike": false, "color": "#000000", "align": "center"})
    );
    assert_eq!(mixed(&ask(&mut s, 0, 6)), ["bold"]);
}

// ---- a point ---------------------------------------------------------------------------------------------------------------------

/// `ab` in a 12 point run and `cd` in an 8 point one, the second bold.
fn two_runs() -> Session {
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), chars("cd")];
    session_of(shape_of(parts, &[Look { style: "0", ..BASE }, Look { size: EIGHT, style: "1", ..BASE }], &["1"]))
}

#[test]
fn a_point_answers_the_run_before_it_which_is_the_run_typing_there_takes() {
    let mut s = two_runs();
    // A point `p` reads the character before it (the one after it at the start of the text), so a point at the end of a run is the
    // run's, and not the next run's: puts characters typed there in the run before.
    let twelve = expected("Calibri", 12.0, 0, false, "#000000", "center");
    let eight = expected("Calibri", 8.0, 1, false, "#000000", "center");
    for (point, want) in [(0, &twelve), (1, &twelve), (2, &twelve), (3, &eight), (4, &eight)] {
        let a = ask(&mut s, point, point);
        none_mixed(&a);
        assert_eq!(state(&a), *want, "the point {point}");
    }
}

#[test]
fn a_point_at_the_start_of_the_text_takes_the_run_of_the_first_character() {
    // `cp` 0 holds nothing: the first character is in run 1, and a point before it takes run 1 as typing there does.
    let parts = vec![TextPart::Cp(0), TextPart::Cp(1), chars("ab")];
    let mut s = session_of(shape_of(parts, &[BASE, Look { size: EIGHT, ..BASE }], &["1"]));
    assert_eq!(ask(&mut s, 0, 0)["size"], 8.0);
    assert_eq!(ask(&mut s, 1, 1)["size"], 8.0);
    // A run with no characters at the end is not the one a point at the end of the text takes.
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1)];
    let mut s = session_of(shape_of(parts, &[BASE, Look { size: EIGHT, ..BASE }], &["1"]));
    assert_eq!(ask(&mut s, 2, 2)["size"], 12.0);
}

#[test]
fn a_point_in_a_text_with_no_characters_takes_the_run_the_first_characters_would_join() {
    // A text with none takes them after its leading markers. The last `cp` of those names the run.
    let looks = [BASE, Look { size: EIGHT, ..BASE }, Look { size: ELEVEN, ..BASE }];
    let mut s = session_of(shape_of(vec![TextPart::Cp(0), TextPart::Cp(1)], &looks, &["1"]));
    assert_eq!(ask(&mut s, 0, 0)["size"], 8.0);
    // A field is not a marker: the characters go ahead of it, after the markers.
    let mut s = session_of(shape_of(vec![TextPart::Cp(2), fld(0, "3"), TextPart::Cp(1)], &looks, &["1"]));
    assert_eq!(ask(&mut s, 0, 0)["size"], 11.0);
    // Row 0 when the text names no run, and for a shape with no text at all: the answer a shape gives without a range.
    let mut s = session_of(shape_of(vec![fld(0, "3")], &looks, &["1"]));
    assert_eq!(ask(&mut s, 0, 0)["size"], 12.0);
    let mut s = session_of(ShapeBuilder::new(1));
    let plain = run(&mut s, "shape.formatting", json!({"ids": [1]}));
    let point = ask(&mut s, 0, 0);
    assert_eq!(state(&point), state(&plain["shapes"][0]));
    none_mixed(&point);
    // A shape with no text has no characters to range over.
    let error = s.execute("shape.formatting", &json!({"ids": [1], "start": 0, "end": 1})).unwrap_err();
    assert!(bad_params_msg(&error).contains("has 0 characters"), "{error}");
}

#[test]
fn a_point_reads_the_paragraph_it_is_in_as_text_format_takes_it() {
    // `one\n` is paragraph 0 (left), `two` paragraph 1 (right). A point before the break is in the first paragraph, one after it is in the second:
    // the paragraph `text.format` with `align` would take. The run is the one before the point, whatever the paragraph.
    let parts = vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Cp(1), TextPart::Pp(1), chars("two")];
    let mut s = session_of(shape_of(parts, &[BASE, Look { size: EIGHT, ..BASE }], &["0", "2"]));
    for (point, align, size) in [(0, "left", 12.0), (3, "left", 12.0), (4, "right", 12.0), (5, "right", 8.0), (7, "right", 8.0)] {
        let a = ask(&mut s, point, point);
        assert_eq!((a["align"].as_str(), a["size"].as_f64()), (Some(align), Some(size)), "the point {point}");
        none_mixed(&a);
    }
    // After a final line break there is no character in the paragraph: it is the one the last `pp` names.
    let parts = vec![TextPart::Cp(0), TextPart::Pp(0), chars("one\n"), TextPart::Pp(1)];
    let mut s = session_of(shape_of(parts, &[BASE], &["0", "2"]));
    assert_eq!(ask(&mut s, 4, 4)["align"], "right");
    assert_eq!(ask(&mut s, 2, 2)["align"], "left");
    let mut s = session_of(shape_of(vec![TextPart::Cp(0), chars("one\n")], &[BASE], &["0"]));
    assert_eq!(ask(&mut s, 4, 4)["align"], "left");
}

// ---- a field ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_field_inside_the_range_is_read_with_the_run_it_is_in() {
    // `ab` (12 points), a field in a run of its own (8 points), `cd` (12 points, and bold). The field shows a page number in the 8 point run.
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), fld(0, "3"), TextPart::Cp(2), chars("cd")];
    let looks = [Look { style: "0", ..BASE }, Look { size: EIGHT, style: "0", ..BASE }, Look { style: "1", ..BASE }];
    let mut s = session_of(shape_of(parts, &looks, &["1"]));
    // Over the characters on both sides the field is inside the range and its run counts: size is mixed, and so is bold (the third run).
    assert_eq!(mixed(&ask(&mut s, 0, 4)), ["size", "bold"]);
    assert_eq!(mixed(&ask(&mut s, 1, 3)), ["size", "bold"]);
    // At the edge the field is in the run it is in, whatever the range says: a range that ends or starts at it does not read its run.
    let before = ask(&mut s, 0, 2);
    none_mixed(&before);
    assert_eq!(before["size"], 12.0);
    let after = ask(&mut s, 2, 4);
    none_mixed(&after);
    assert_eq!(after["bold"], true);
    assert_eq!(after["size"], 12.0);
    // A point at the field's place is the character before it.
    assert_eq!(ask(&mut s, 2, 2)["size"], 12.0);
}

#[test]
fn a_field_in_the_middle_of_a_run_changes_nothing() {
    let parts = vec![TextPart::Cp(0), chars("ab"), fld(0, "3"), chars("cd")];
    let mut s = session_of(shape_of(parts, &[Look { size: ELEVEN, ..BASE }], &["1"]));
    for (start, end) in [(0, 4), (1, 3), (2, 4), (0, 2)] {
        let a = ask(&mut s, start, end);
        none_mixed(&a);
        assert_eq!(a["size"], 11.0, "{start}..{end}");
    }
}

#[test]
fn a_range_reads_what_text_format_would_change() {
    // The runs a range reads are those `text.format` writes to: after a format of any property over a range, the same range answers the new value and
    // is not mixed in it. Ranges that cut runs, cover them whole, cross a field and cross a paragraph.
    let ranges = [(1, 3), (0, 4), (5, 9), (2, 14), (13, 19), (0, 19), (16, 19), (4, 12)];
    let changes: [(Value, &str, Value); 8] = [
        (json!({"bold": true}), "bold", json!(true)),
        (json!({"bold": false}), "bold", json!(false)),
        (json!({"italic": true}), "italic", json!(true)),
        (json!({"underline": false}), "underline", json!(false)),
        (json!({"strike": true}), "strike", json!(true)),
        (json!({"size": 11}), "size", json!(11.0)),
        (json!({"font": "Arial"}), "font", json!("Arial")),
        (json!({"color": "#336699"}), "color", json!("#336699")),
    ];
    for (start, end) in ranges {
        for (change, name, want) in &changes {
            let mut s = five_runs();
            let mut params = change.clone();
            params["ids"] = json!([1]);
            params["start"] = json!(start);
            params["end"] = json!(end);
            run(&mut s, "text.format", params);
            let a = ask(&mut s, start, end);
            assert_eq!(a[*name], *want, "{start}..{end} {change}: {a}");
            assert!(!mixed(&a).contains(&(*name).to_owned()), "{start}..{end} {change}: {a}");
            assert_eq!(check_session(&s), Ok(()));
        }
    }
    // And the alignment, which takes the paragraphs the range touches.
    for (start, end) in [(1, 3), (14, 19), (16, 19), (0, 19)] {
        let mut s = five_runs();
        run(&mut s, "text.format", json!({"ids": [1], "start": start, "end": end, "align": "center"}));
        let a = ask(&mut s, start, end);
        assert_eq!(a["align"], "center", "{start}..{end}: {a}");
        assert!(!mixed(&a).contains(&"align".to_owned()), "{start}..{end}: {a}");
    }
}

#[test]
fn a_format_over_a_field_is_read_back_with_the_field_in_it() {
    // The field is in a run of its own, 8 points, between two of 12. A format of the characters on both sides reaches the field's run too (it is inside
    // the range), so the range reads one size; a format that stops at the field leaves its run, and the range over all three is mixed.
    let parts = vec![TextPart::Cp(0), chars("ab"), TextPart::Cp(1), fld(0, "3"), TextPart::Cp(2), chars("cd")];
    let looks = [BASE, Look { size: EIGHT, ..BASE }, BASE];
    let mut s = session_of(shape_of(parts.clone(), &looks, &["1"]));
    assert_eq!(mixed(&ask(&mut s, 0, 4)), ["size"]);
    run(&mut s, "text.format", json!({"ids": [1], "start": 0, "end": 4, "size": 11}));
    let a = ask(&mut s, 0, 4);
    none_mixed(&a);
    assert_eq!(a["size"], 11.0);
    let mut s = session_of(shape_of(parts, &looks, &["1"]));
    run(&mut s, "text.format", json!({"ids": [1], "start": 0, "end": 2, "size": 11}));
    assert_eq!(mixed(&ask(&mut s, 0, 4)), ["size"], "the field's run was at the edge of the format");
    assert_eq!(ask(&mut s, 0, 2)["size"], 11.0);
}

#[test]
fn a_toggle_on_a_range_that_is_not_the_first_run_flips_with_each_press() {
    // Gap U19. The label starts bold and a later word is not: the window asks the range, presses Bold with the opposite of what it finds, and asks again.
    let parts = vec![TextPart::Cp(0), chars("Bold "), TextPart::Cp(1), chars("plain word")];
    let mut s = session_of(shape_of(parts, &[Look { style: "1", ..BASE }, Look { style: "0", ..BASE }], &["1"]));
    let bold_of = |s: &mut Session| ask(s, 7, 11)["bold"].clone();
    assert_eq!(bold_of(&mut s), false);
    let mut seen = Vec::new();
    for _ in 0..4 {
        let now = bold_of(&mut s) == json!(true);
        seen.push(now);
        run(&mut s, "text.format", json!({"ids": [1], "start": 7, "end": 11, "bold": !now}));
    }
    assert_eq!(seen, [false, true, false, true], "each press flips what the characters have");
    assert_eq!(ask(&mut s, 0, 5)["bold"], true, "the first run was never touched");
    // A range that holds both a bold and a plain run is mixed, and the window sends bold on.
    run(&mut s, "text.format", json!({"ids": [1], "start": 7, "end": 11, "bold": false}));
    assert_eq!(ask(&mut s, 0, 11)["bold"], "mixed");
}

// ---- the rest of the answer, and what the call leaves alone ----------------------------------------------------------------------

#[test]
fn a_range_answer_holds_the_shape_cells_of_the_answer_without_one_and_a_list_of_what_is_mixed() {
    let mut s = five_runs();
    let plain = run(&mut s, "shape.formatting", json!({"ids": [1]}));
    assert!(plain["shapes"][0].get("mixed").is_none(), "an answer with no range is as it was: {plain}");
    let ranged = ask(&mut s, 0, 12);
    let mut want = plain["shapes"][0].clone();
    let mut got = ranged.clone();
    for name in PROPS.iter().chain(&["mixed"]) {
        want.as_object_mut().unwrap().remove(*name);
        got.as_object_mut().unwrap().remove(*name);
    }
    assert_eq!(got, want, "id, fill, line, weight, pattern, arrows, rounding and valign are the shape's");
    // Without a range the first run answers, as before.
    assert_eq!(state(&plain["shapes"][0]), expected("Calibri", 12.0, 5, false, "#000000", "left"));
}

#[test]
fn a_range_query_changes_nothing_and_is_no_step() {
    let mut s = five_runs();
    let before = snapshot(&s);
    for (start, end) in [(0, 19), (3, 3), (5, 9)] {
        ask(&mut s, start, end);
    }
    assert_eq!(snapshot(&s), before, "no change to the document, the history, the journal or the revision");
}

// ---- hostile offsets and shapes -------------------------------------------------------------------------------------------------

fn refused(s: &mut Session, params: Value) -> String {
    let before = snapshot(s);
    let error = s.execute("shape.formatting", &params).unwrap_err();
    assert_eq!(snapshot(s), before, "{params}");
    bad_params_msg(&error).to_owned()
}

#[test]
fn offsets_past_the_text_or_reversed_are_refused_in_the_words_text_format_uses() {
    let mut s = two_runs();
    for (start, end) in [(3, 2), (0, 5), (5, 5), (4, 5), (u32::MAX, u32::MAX), (0, u32::MAX), (u32::MAX, 0)] {
        let ours = refused(&mut s, json!({"ids": [1], "start": start, "end": end}));
        let theirs = {
            let error = s.execute("text.format", &json!({"ids": [1], "start": start, "end": end, "bold": true})).unwrap_err();
            bad_params_msg(&error).to_owned()
        };
        assert!(ours.contains(&format!("the range {start} to {end} is not within the text, which has 4 characters")), "{ours}");
        assert_eq!(ours, theirs, "{start}..{end}: the same words");
    }
}

#[test]
fn start_and_end_go_together_and_are_whole_numbers() {
    let mut s = two_runs();
    for (params, mention) in [
        (json!({"ids": [1], "start": 1}), "go together"),
        (json!({"ids": [1], "end": 1}), "go together"),
        (json!({"ids": [1], "start": -1, "end": 1}), "`start` must be a whole number"),
        (json!({"ids": [1], "start": 0, "end": 1.5}), "`end` must be a whole number"),
        (json!({"ids": [1], "start": "0", "end": 1}), "`start` must be a whole number"),
        (json!({"ids": [1], "start": 0, "end": 4_294_967_296_u64}), "`end` must be a whole number"),
        (json!({"ids": [1], "start": true, "end": 1}), "`start` must be a whole number"),
    ] {
        let message = refused(&mut s, params.clone());
        assert!(message.contains(mention), "{params}: {message}");
    }
    // `null` is as good as nothing, as everywhere else: the answer is the first run's.
    let r = run(&mut s, "shape.formatting", json!({"ids": [1], "start": null, "end": null}));
    assert!(r["shapes"][0].get("mixed").is_none(), "{r}");
}

#[test]
fn a_range_is_of_one_shape_that_is_not_a_group() {
    let mut s = blank_session();
    let (a, b) = (drop_shape(&mut s, "rectangle", 2.0, 2.0), drop_shape(&mut s, "rectangle", 5.0, 2.0));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "hello"}));
    run(&mut s, "shape.set_text", json!({"id": b, "text": "world"}));
    let ours = refused(&mut s, json!({"ids": [a, b], "start": 0, "end": 1}));
    assert!(ours.contains("a range of characters is of one shape: name one shape"), "{ours}");
    assert_eq!(refused(&mut s, json!({"ids": [a, a], "start": 0, "end": 1})), ours, "an id twice is two names");
    let group = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap();
    let ours = refused(&mut s, json!({"ids": [group], "start": 0, "end": 1}));
    assert!(ours.contains("a group holds no characters of its own"), "{ours}");
    // A member of the group is a shape with characters.
    let r = run(&mut s, "shape.formatting", json!({"ids": [a], "start": 1, "end": 3}));
    assert_eq!(r["shapes"][0]["id"], a);
    none_mixed(&r["shapes"][0]);
    // A shape or a page that is not there.
    assert!(s.execute("shape.formatting", &json!({"ids": [999], "start": 0, "end": 0})).is_err());
    assert!(s.execute("shape.formatting", &json!({"ids": [a], "page": 7, "start": 0, "end": 1})).is_err());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn offsets_count_characters_not_bytes() {
    // `h`, `é` (2 bytes), `llo `, a face with four bytes, `!`: 8 characters in 12 bytes.
    let parts = vec![TextPart::Cp(0), chars("h\u{e9}llo "), TextPart::Cp(1), chars("\u{1f600}!")];
    let mut s = session_of(shape_of(parts, &[BASE, Look { size: EIGHT, ..BASE }], &["1"]));
    // Character 1 is the é and 5 the space: both in run 0. The emoji is character 6.
    assert_eq!(ask(&mut s, 1, 2)["size"], 12.0);
    assert_eq!(ask(&mut s, 5, 6)["size"], 12.0);
    assert_eq!(ask(&mut s, 6, 7)["size"], 8.0);
    assert_eq!(mixed(&ask(&mut s, 5, 7)), ["size"]);
    ask(&mut s, 0, 8);
    // 9 is within the 12 bytes and not within the 8 characters.
    let message = refused(&mut s, json!({"ids": [1], "start": 0, "end": 9}));
    assert!(message.contains("which has 8 characters"), "{message}");
    // There is no offset that falls inside a character: every offset from 0 to 8 is answered.
    for point in 0..=8 {
        ask(&mut s, point, point);
    }
}
