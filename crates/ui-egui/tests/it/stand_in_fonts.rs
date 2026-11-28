//! Issue 9: the findings mark says which font is missing and what the text is drawn in.
//!
//! A drawing made by DiaCraft names Liberation Sans for its text (its built-in themes do), which a Linux desktop usually has and the browser's bundled faces do not. The browser's status bar
//! then showed a mark for a file the desktop showed none for, and its list said "1 text block drawn in a stand-in font or cut short" with the font named nowhere. The mark is right (the
//! text is drawn in another face where the file's is not installed, on a Mac or a Windows machine as well); its words now name the font and the face that drew it. The window of these
//! tests draws with the bundled faces only, as the browser does.

use std::time::Duration;

use serde_json::json;

use crate::support::{Rest, call, controlled};

/// The lines of the list behind the status bar's mark, after the picture of full quality has arrived.
fn findings_after(commands: &[(&str, serde_json::Value)]) -> Vec<String> {
    let (mut w, tx) = controlled();
    for (command, params) in commands {
        call(&mut w, &tx, "engine.execute", json!({"command": command, "params": params})).unwrap();
    }
    w.rest();
    w.advance(Duration::from_millis(250));
    w.step();
    w.step();
    let inspected = call(&mut w, &tx, "ui.inspect", json!({})).unwrap();
    inspected["findings"]["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_owned()).collect()
}

#[test]
fn a_text_in_a_font_the_machine_lacks_names_the_font_and_the_face_that_drew_it() {
    let lines = findings_after(&[
        ("file.new", json!({})),
        ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})),
        ("shape.set_text", json!({"id": 1, "text": "Router"})),
        ("text.format", json!({"ids": [1], "font": "Liberation Sans"})),
    ]);
    let line = lines.iter().find(|l| l.contains("stand-in font")).unwrap_or_else(|| panic!("a line for the font: {lines:?}"));
    assert!(
        line.starts_with("1 text block drawn in a stand-in font, as the font the file names is not on this machine: Liberation Sans (drawn in "),
        "{line}"
    );
    assert!(line.ends_with(')'), "{line}");
    assert_eq!(lines.len(), 1, "one line for the one font: {lines:?}");
    assert!(!lines.iter().any(|l| l.contains("or cut short")), "the old catch-all words are gone: {lines:?}");
}

/// A drawing whose text is in a face the machine has says nothing: the bundled face is asked for by name.
#[test]
fn a_text_in_a_face_the_machine_has_says_nothing() {
    let lines = findings_after(&[
        ("file.new", json!({})),
        ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})),
        ("shape.set_text", json!({"id": 1, "text": "Router"})),
    ]);
    assert!(!lines.iter().any(|l| l.contains("stand-in")), "{lines:?}");
}
