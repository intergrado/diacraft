//! the review, through the command the review found it by: `shape.pin` puts a shape's pin at another place of its box and gives the pin cells
//! formulas, with the shape drawing where it did. The `.odg` save used to be no edit at all (the frame has nothing to write), and the formulas were gone when the file was
//! read again. They are carried now, so that the file reads back as the session holds it.

use serde_json::json;

use crate::odg_build::{Dir, content_of, id_of, open_drawing, read_back};
use crate::odg_file::{four, save};
use crate::run;

fn formula(doc: &diacraft_doc::Document, name: &str, cell: &str) -> Option<String> {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap();
    shape.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned))
}

fn value(doc: &diacraft_doc::Document, name: &str, cell: &str) -> Option<f64> {
    let shape = doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap();
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()?.number())
}

#[test]
fn a_pin_put_at_another_place_is_saved_with_its_formulas_and_the_file_reads_back_as_the_session_holds_it() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &four());
    let id = id_of(s.document().unwrap(), 0, "A");
    let before_x = formula(s.document().unwrap(), "A", "LocPinX");
    let content_before = content_of(&dir.read("a.odg"));
    run(&mut s, "shape.pin", json!({ "ids": [id], "at": { "x": 0.0, "y": 0.0 } }));
    let (held_x, held_y) = (formula(s.document().unwrap(), "A", "LocPinX"), formula(s.document().unwrap(), "A", "LocPinY"));
    assert_ne!(held_x, before_x, "the command gives the pin a formula of its own");
    save(&mut s);
    let bytes = dir.read("a.odg");
    let (doc, _) = read_back(&bytes);
    assert_eq!(formula(&doc, "A", "LocPinX"), held_x, "the pin's formulas are in the file");
    assert_eq!(formula(&doc, "A", "LocPinY"), held_y);
    // The pin's position reads back as the session holds it too, and not as the frame's centre (the pin is the markup's edge plus the place that is carried; before it,
    // the file said the formulas and read the pin at the centre, half a shape from where the session had it).
    for cell in ["PinX", "PinY"] {
        let (held, back) = (value(s.document().unwrap(), "A", cell).unwrap(), value(&doc, "A", cell).unwrap());
        assert!((held - back).abs() < 1e-9, "{cell}: the session holds {held}, the file reads {back}");
    }
    // The shape is where it was: the frame in the file is as it was, and the formulas are the extension's.
    let content = content_of(&bytes);
    let frame = |c: &str| -> Vec<String> {
        ["svg:x", "svg:y", "svg:width", "svg:height"]
            .iter()
            .flat_map(|attr| c.split(&format!("{attr}=\"")).skip(1).map(|r| r.split('"').next().unwrap_or_default().to_owned()).collect::<Vec<_>>())
            .collect()
    };
    assert_eq!(frame(&content_before), frame(&content), "the shapes draw where they did");
    assert!(content.contains("diacraft:cells="), "the formulas are carried by the extension: {content}");
}
