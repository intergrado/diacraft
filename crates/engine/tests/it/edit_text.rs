//! Editing the text of a shape without losing its runs and fields (of the M3 review).
//!
//! `shape.set_text` takes a range of the shape's characters (`start` and `end`, counted in characters and leaving out the fields) and replaces
//! only those: the `cp`, `pp` and `tp` markers and the `fld` elements stay where they are, so typing over `cd` to give `ce` changes the character
//! data and nothing else. Without a range the command is the whole-text form it was.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, TextPart};
use diacraft_engine::draw::{MAX_TEXT_CHARS, set_text, set_text_range, text_view};
use diacraft_engine::{EngineError, Session};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const TEXT: &str = r#"<Text><cp IX="0"/>ab<cp IX="1"/>cd<fld IX="0">3</fld></Text>"#;

/// The model of `<cp IX="0"/>ab<cp IX="1"/>cd<fld IX="0">3</fld>`.
fn parts() -> Vec<TextPart> {
    vec![
        TextPart::Cp(0),
        TextPart::Chars(Arc::from("ab")),
        TextPart::Cp(1),
        TextPart::Chars(Arc::from("cd")),
        TextPart::Fld { ix: 0, text: Arc::from("3") },
    ]
}

fn doc_with(parts: Vec<TextPart>) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts))).build()
}

/// The parts of shape 1's text, each as a short word: `cp0`, `pp1`, `tp2`, `fld0=3`, and the characters in quotes.
fn words(doc: &Document) -> Vec<String> {
    let Some(text) = doc.page(1).unwrap().shapes.get(0).unwrap().text.clone() else { return vec!["(no text)".to_owned()] };
    text.parts
        .iter()
        .map(|part| match part {
            TextPart::Chars(s) => format!("{s:?}"),
            TextPart::Cp(ix) => format!("cp{ix}"),
            TextPart::Pp(ix) => format!("pp{ix}"),
            TextPart::Tp(ix) => format!("tp{ix}"),
            TextPart::Fld { ix, text } => format!("fld{ix}={text}"),
            TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => "raw".to_owned(),
        })
        .collect()
}

fn edit(parts: Vec<TextPart>, start: usize, end: usize, text: &str) -> Vec<String> {
    let mut doc = doc_with(parts);
    set_text_range(&mut doc, 1, 1, start, end, text).unwrap_or_else(|p| panic!("{start}..{end} {text:?}: {}", p.message));
    words(&doc)
}

#[test]
fn a_range_replaces_only_its_characters_and_every_marker_and_field_stays() {
    // Typing over `cd` to give `ce`: the second run keeps its marker and the field its place.
    assert_eq!(edit(parts(), 3, 4, "e"), ["cp0", "\"ab\"", "cp1", "\"ce\"", "fld0=3"]);
    // Characters in the middle of a run, and across the boundary of two.
    assert_eq!(edit(parts(), 1, 3, "Z"), ["cp0", "\"aZ\"", "cp1", "\"d\"", "fld0=3"]);
    // Taking characters away leaves the markers, though a run has none left: the next characters typed are that run's.
    assert_eq!(edit(parts(), 2, 4, ""), ["cp0", "\"ab\"", "cp1", "fld0=3"]);
    assert_eq!(edit(parts(), 0, 4, ""), ["cp0", "cp1", "fld0=3"]);
    // All the characters replaced: the new ones are the first run's.
    assert_eq!(edit(parts(), 0, 4, "xyz"), ["cp0", "\"xyz\"", "cp1", "fld0=3"]);
}

#[test]
fn characters_typed_at_a_point_join_the_run_before_it() {
    // At the start: the first run. In the middle of a run. At the boundary of two runs: the one before. At the end: the last run, before the field.
    assert_eq!(edit(parts(), 0, 0, "X"), ["cp0", "\"Xab\"", "cp1", "\"cd\"", "fld0=3"]);
    assert_eq!(edit(parts(), 1, 1, "X"), ["cp0", "\"aXb\"", "cp1", "\"cd\"", "fld0=3"]);
    assert_eq!(edit(parts(), 2, 2, "X"), ["cp0", "\"abX\"", "cp1", "\"cd\"", "fld0=3"]);
    assert_eq!(edit(parts(), 4, 4, "X"), ["cp0", "\"ab\"", "cp1", "\"cdX\"", "fld0=3"]);
    // Nothing typed and nothing taken away changes nothing.
    assert_eq!(edit(parts(), 2, 2, ""), ["cp0", "\"ab\"", "cp1", "\"cd\"", "fld0=3"]);
}

#[test]
fn a_range_across_a_field_leaves_the_field_and_the_other_markers_alone() {
    // The field sits after the characters; text with a field between two runs of characters is edited across it.
    let across = vec![
        TextPart::Cp(0),
        TextPart::Chars(Arc::from("ab")),
        TextPart::Fld { ix: 0, text: Arc::from("7") },
        TextPart::Pp(2),
        TextPart::Chars(Arc::from("cd")),
        TextPart::Tp(1),
    ];
    // `b` and `c` replaced: the new character goes where `b` was, and the field, the paragraph marker and the tab marker stay.
    assert_eq!(edit(across.clone(), 1, 3, "Z"), ["cp0", "\"aZ\"", "fld0=7", "pp2", "\"d\"", "tp1"]);
    assert_eq!(edit(across.clone(), 0, 4, ""), ["cp0", "fld0=7", "pp2", "tp1"]);
    // Characters typed at the point where the field is join the run before it.
    assert_eq!(edit(across, 2, 2, "!"), ["cp0", "\"ab!\"", "fld0=7", "pp2", "\"cd\"", "tp1"]);
}

#[test]
fn text_with_no_characters_takes_them_after_its_markers_and_before_a_field() {
    assert_eq!(edit(vec![TextPart::Cp(0), TextPart::Pp(0)], 0, 0, "x"), ["cp0", "pp0", "\"x\""]);
    assert_eq!(edit(vec![TextPart::Cp(3), TextPart::Fld { ix: 0, text: Arc::from("9") }], 0, 0, "x"), ["cp3", "\"x\"", "fld0=9"]);
    // A shape with no text at all gets one run, as the whole-text form does.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    set_text_range(&mut doc, 1, 1, 0, 0, "hi").unwrap();
    assert_eq!(words(&doc), ["\"hi\""]);
    // Taking away the last of the characters of a text with nothing else takes the text away.
    let mut plain = doc_with(vec![TextPart::Chars(Arc::from("hi"))]);
    set_text_range(&mut plain, 1, 1, 0, 2, "").unwrap();
    assert_eq!(words(&plain), ["(no text)"]);
}

#[test]
fn offsets_count_characters_and_not_bytes() {
    let accented = vec![TextPart::Chars(Arc::from("h\u{e9}llo \u{1f600}!"))];
    assert_eq!(edit(accented.clone(), 1, 2, "e"), ["\"hello \u{1f600}!\""]);
    assert_eq!(edit(accented.clone(), 6, 7, ":)"), ["\"h\u{e9}llo :)!\""]);
    assert_eq!(edit(accented, 8, 8, "?"), ["\"h\u{e9}llo \u{1f600}!?\""]);
}

#[test]
fn a_range_the_text_does_not_have_is_refused_and_leaves_the_document() {
    let mut doc = doc_with(parts());
    let before = doc.clone();
    for (start, end) in [(3, 2), (0, 5), (5, 5), (4, 9), (usize::MAX, usize::MAX), (0, usize::MAX)] {
        let problem = set_text_range(&mut doc, 1, 1, start, end, "x").unwrap_err();
        assert!(problem.bad_params, "{start}..{end}: {}", problem.message);
    }
    assert_eq!(doc, before);
    assert!(set_text_range(&mut doc, 1, 99, 0, 0, "x").is_err() && set_text_range(&mut doc, 9, 1, 0, 0, "x").is_err());
}

#[test]
fn text_that_xml_cannot_hold_and_text_too_long_are_refused_in_either_form() {
    for bad in ["\u{fffe}", "a\u{ffff}", "\u{1}", "\u{7f}", "x\r", "\u{0}"] {
        let mut doc = doc_with(parts());
        assert!(set_text(&mut doc, 1, 1, bad).unwrap_err().bad_params, "whole {bad:?}");
        assert!(set_text_range(&mut doc, 1, 1, 0, 1, bad).unwrap_err().bad_params, "range {bad:?}");
    }
    let mut doc = doc_with(parts());
    let before = doc.clone();
    let long = "x".repeat(MAX_TEXT_CHARS - 2);
    // 4 characters are there; the range keeps 3 of them and adds MAX - 2: one over.
    assert!(set_text_range(&mut doc, 1, 1, 0, 1, &long).unwrap_err().bad_params);
    assert!(set_text_range(&mut doc, 1, 1, 0, 0, &"x".repeat(MAX_TEXT_CHARS + 1)).is_err());
    assert_eq!(doc, before);
    // Line breaks and tabs are text.
    set_text_range(&mut doc, 1, 1, 4, 4, "\n\t").unwrap();
    assert_eq!(words(&doc)[3], "\"cd\\n\\t\"");
}

#[test]
fn the_view_of_a_text_gives_its_characters_and_where_its_fields_sit() {
    let doc = doc_with(parts());
    let view = text_view(&doc, 1, 1).unwrap();
    assert_eq!(view.chars, "abcd");
    assert_eq!(view.fields.iter().map(|f| (f.at, f.ix, f.text.as_str())).collect::<Vec<_>>(), [(4, 0, "3")]);
    let empty = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    let none = text_view(&empty, 1, 1).unwrap();
    assert_eq!((none.chars.as_str(), none.fields.len()), ("", 0));
    assert!(text_view(&empty, 1, 2).is_none() && text_view(&empty, 2, 1).is_none());
}

#[test]
fn the_whole_text_form_replaces_everything_unless_the_shape_shows_that_text_already() {
    let mut doc = doc_with(parts());
    // The text the shape shows, field result and all, is the text it has: nothing is touched.
    set_text(&mut doc, 1, 1, "abcd3").unwrap();
    assert_eq!(words(&doc), ["cp0", "\"ab\"", "cp1", "\"cd\"", "fld0=3"]);
    // Any other text is the one run it always was.
    set_text(&mut doc, 1, 1, "abce3").unwrap();
    assert_eq!(words(&doc), ["\"abce3\""]);
    set_text(&mut doc, 1, 1, "").unwrap();
    assert_eq!(words(&doc), ["(no text)"]);
}

// ---- through the commands -----------------------------------------------------------------------------------------------------------

#[test]
fn the_command_takes_a_range_and_both_ends_or_neither() {
    let mut s = blank_session();
    let id = crate::edit::drop_shape(&mut s, "rectangle", 2.0, 2.0);
    run(&mut s, "shape.set_text", json!({"id": id, "text": "hello"}));
    run(&mut s, "shape.set_text", json!({"id": id, "text": "J", "start": 0, "end": 1}));
    run(&mut s, "shape.set_text", json!({"id": id, "text": "ly", "start": 3, "end": 5}));
    let view = text_view(s.document().unwrap(), 1, id).unwrap();
    assert_eq!(view.chars, "Jelly");
    let before = snapshot(&s);
    for params in [
        json!({"id": id, "text": "x", "start": 1}),
        json!({"id": id, "text": "x", "end": 1}),
        json!({"id": id, "text": "x", "start": 3, "end": 2}),
        json!({"id": id, "text": "x", "start": 0, "end": 99}),
        json!({"id": id, "text": "x", "start": -1, "end": 1}),
        json!({"id": id, "text": "x", "start": 0.5, "end": 1}),
        json!({"id": id, "text": "x", "start": "0", "end": 1}),
        json!({"id": id, "start": 0, "end": 1}),
    ] {
        let error = s.execute("shape.set_text", &params).unwrap_err();
        assert!(matches!(error, EngineError::BadParams { .. }), "{params}: {error:?}");
        bad_params_msg(&error);
    }
    assert_eq!(snapshot(&s), before, "a refused range changes nothing, and is not an undo step");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- saved ---------------------------------------------------------------------------------------------------------------------------

struct Scratch(PathBuf);

impl Scratch {
    fn new(bytes: &[u8]) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("diacraft-engine-text-{}-{}.vsdx", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, bytes).unwrap();
        Scratch(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().unwrap()
    }

    fn page_xml(&self) -> String {
        let bytes = std::fs::read(&self.0).unwrap();
        let pkg = Package::read(&bytes, &Limits::default()).unwrap();
        let parts: BTreeMap<String, Vec<u8>> = pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect();
        String::from_utf8(parts.get("/visio/pages/page1.xml").unwrap().clone()).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn labelled_drawing() -> Vec<u8> {
    let label = shape(
        1,
        &format!(
            r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Section N="Character"><Row IX="0"><Cell N="Style" V="1"/></Row><Row IX="1"><Cell N="Style" V="2"/></Row></Section>
<Section N="Field"><Row IX="0"><Cell N="Value" V="3"/></Row></Section>{TEXT}"#
        ),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&label)).build()
}

#[test]
fn typing_over_cd_to_give_ce_keeps_the_markers_and_the_field_in_the_saved_file() {
    let file = Scratch::new(&labelled_drawing());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    assert_eq!(text_view(s.document().unwrap(), 1, 1).unwrap().chars, "abcd");
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "e", "start": 3, "end": 4}));
    run(&mut s, "file.save", json!({}));
    let xml = file.page_xml();
    assert!(xml.contains(r#"<Text><cp IX="0"/>ab<cp IX="1"/>ce<fld IX="0">3</fld></Text>"#), "{xml}");
    // The rows the markers name are as the file had them.
    assert!(xml.contains(r#"<Section N="Character">"#) && xml.contains(r#"<Section N="Field">"#), "{xml}");
    // And it opens as the same: characters `abce`, the field after them.
    let mut again = Session::new();
    run(&mut again, "file.open", json!({"path": file.text()}));
    let view = text_view(again.document().unwrap(), 1, 1).unwrap();
    assert_eq!((view.chars.as_str(), view.fields.len()), ("abce", 1));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_whole_text_form_is_still_there_for_programs_and_a_range_is_one_undo_step() {
    let file = Scratch::new(&labelled_drawing());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Z", "start": 0, "end": 1}));
    assert_eq!(text_view(s.document().unwrap(), 1, 1).unwrap().chars, "Zbcd");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text_view(s.document().unwrap(), 1, 1).unwrap().chars, "abcd");
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "plain"}));
    run(&mut s, "file.save", json!({}));
    assert!(file.page_xml().contains("<Text>plain</Text>"), "{}", file.page_xml());
}

// ---- what reads the text ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_cell_that_reads_the_text_is_worked_out_again_after_the_text_changes() {
    let echo = shape(
        1,
        r#"<Cell N="PinX" V="2"/><Section N="User"><Row N="Echo"><Cell N="Value" V="stale" F="SHAPETEXT(TheText)"/></Row></Section><Text>old</Text>"#,
    );
    let other = shape(2, r#"<Section N="User"><Row N="Same"><Cell N="Value" V="9" F="1+1"/></Row></Section>"#);
    let file = Scratch::new(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&format!("{echo}{other}"))).build());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": file.text()}));
    let user = |s: &Session, id: u32, name: &str| {
        let d = s.document().unwrap();
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        match d.shape(&at).unwrap().sheet.lookup(&CellAddr::in_row("User", None, RowKey::name(name), "Value")) {
            Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
            other => panic!("{other:?}"),
        }
    };
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "new words"}));
    assert_eq!(user(&s, 1, "Echo").as_deref(), Some("new words"));
    assert_eq!(user(&s, 2, "Same").as_deref(), Some("9"), "a cell that does not read the text keeps what the file saved");
    // A range edit is worked out the same way.
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "W", "start": 0, "end": 1}));
    assert_eq!(user(&s, 1, "Echo").as_deref(), Some("Wew words"));
}
