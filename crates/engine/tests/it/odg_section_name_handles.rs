//! (the review of SECNAME; a `$n` is a value of the view box, which the frame maps, so a
//! resize of the frame keeps it).
//!
//! The reader wrote a handle's `X` and `Y` as constants and the modifier it drives as `Controls.Row_1.X/Width*21600`. With the handles' section named as the model names
//! it (SECNAME), calc followed that formula, and **a resize of the shape rewrote `draw:modifiers`**: an x handle, width 4 cm to 8 cm, `5400 7` to `2700 7`; a y handle,
//! height 4 cm to 8 cm, `5400 7` to `13500 7`; width times 1e-9, `5399999999999.999 7`. The handle is now a share of the frame (`Width*0.25`, `Height*0.25`), so that the
//! modifier works out to what the file says whatever the frame.
//!
//! Each case opens a drawing written by hand, resizes the shape through the command a window uses, saves, and reads what the file says of the modifiers.

use diacraft_calc::{Budget, Changes, EvalOptions, commit_changed};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_engine::Session;
use diacraft_odg::{Limits, OriginalPackage, WriteOptions, read, write_with};
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing};
use crate::odg_file::{assert_saves_again_as_no_edit, frame_at, save};
use crate::run;

/// What a handle and its modifiers are in a custom shape of 4 cm by 4 cm.
struct Shape {
    name: &'static str,
    view_box: &'static str,
    modifiers: &'static str,
    handles: &'static str,
}

const CASES: [Shape; 6] = [
    Shape {
        name: "an x handle",
        view_box: "0 0 21600 21600",
        modifiers: "5400 7",
        handles: r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/>"#,
    },
    Shape {
        name: "a y handle",
        view_box: "0 0 21600 21600",
        modifiers: "5400 7",
        handles: r#"<draw:handle draw:handle-position-x="right" draw:handle-position-y="$0"/>"#,
    },
    Shape {
        name: "a handle that drives both axes",
        view_box: "0 0 21600 21600",
        modifiers: "5400 16200",
        handles: r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="$1"/>"#,
    },
    Shape {
        name: "two handles of a view box with an origin",
        view_box: "100 200 1000 1000",
        modifiers: "300 400",
        handles: r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/><draw:handle draw:handle-position-x="left" draw:handle-position-y="$1"/>"#,
    },
    Shape {
        name: "a value that is not a share a decimal fraction holds",
        view_box: "0 0 21599 21601",
        modifiers: "7777.7777 1234.5678",
        handles: r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="$1"/>"#,
    },
    // A polar handle is kept and not editable: it has no row, and no formula reads the modifiers it moves.
    Shape {
        name: "a polar handle",
        view_box: "0 0 21600 21600",
        modifiers: "5400 7",
        handles: r#"<draw:handle draw:handle-position="$0 $1" draw:handle-polar="10800 10800" draw:handle-radius-range-minimum="0" draw:handle-radius-range-maximum="10800"/>"#,
    },
];

/// The drawing of one case: the shape's path names the modifiers, so that there is an outline to follow them.
fn drawing(case: &Shape) -> Drawing {
    let Shape { view_box, modifiers, handles, .. } = case;
    Drawing::new().page(&format!(
        r#"<draw:custom-shape draw:name="h" draw:layer="layout" svg:x="1cm" svg:y="2cm" svg:width="4cm" svg:height="4cm"><text:p/><draw:enhanced-geometry svg:viewBox="{view_box}" draw:modifiers="{modifiers}" draw:enhanced-path="M 0 0 L $0 $1 21600 21600 Z N">{handles}</draw:enhanced-geometry></draw:custom-shape>"#
    ))
}

/// What `draw:modifiers` says in a file's `content.xml`.
fn modifiers_of(content: &str) -> String {
    let at = content.find("draw:modifiers=\"").map(|i| i + "draw:modifiers=\"".len()).unwrap_or_else(|| panic!("no draw:modifiers: {content}"));
    content[at..].split('"').next().unwrap_or_default().to_owned()
}

/// The value of the modifier `n` of the one shape, as the model holds it.
fn modifier(doc: &Document, n: usize) -> f64 {
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").unwrap();
    let row = section.rows.iter().find(|r| r.name.as_deref() == Some(&format!("ODFMod{n}"))).unwrap();
    row.cells.get("Value").and_then(|c| c.value.as_ref()?.number()).unwrap_or_else(|| panic!("ODFMod{n} holds no number"))
}

/// The shape resized to `wf` times its width and `hf` times its height about the corner at the lower left, as the window's resize does.
fn resize(s: &mut Session, wf: f64, hf: f64) {
    let doc = s.document().unwrap();
    let id = id_of(doc, 0, "h");
    let [pin_x, pin_y, width, height, _] = frame_at(doc, 0, 0);
    let (x, y) = (pin_x - width / 2.0, pin_y - height / 2.0);
    run(s, "shape.resize", json!({ "id": id, "x": x, "y": y, "width": width * wf, "height": height * hf }));
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// **A resize keeps `draw:modifiers`**: for each handle and each resize, the file says what it said, the model holds what it held, and the second save is no
/// edit. The frame did change: the save is an edit and says the new width.
#[test]
fn a_resize_keeps_draw_modifiers_for_an_x_handle_a_y_handle_both_a_shifted_view_box_and_a_polar_handle() {
    let sizes = [(2.0, 1.0), (1.0, 2.0), (2.0, 2.0), (0.5, 0.5), (1e-9, 1.0), (1.0, 1e-9), (3.7, 0.13)];
    for case in &CASES {
        for (wf, hf) in sizes {
            let what = format!("{}, resized by {wf} and {hf}", case.name);
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &drawing(case));
            let before = content_of(&dir.read("a.odg"));
            let held: Vec<f64> = (0..2).map(|n| modifier(s.document().unwrap(), n)).collect();
            resize(&mut s, wf, hf);
            let saved = save(&mut s);
            assert_eq!(saved["edited"], true, "{what}: the frame changed");
            let after = content_of(&dir.read("a.odg"));
            assert_eq!(modifiers_of(&after), modifiers_of(&before), "{what}: the file says what it said of the modifiers");
            assert_ne!(after, before, "{what}: and the resize is in the file");
            assert!(
                !after.contains("diacraft:s-Control"),
                "{what}: the handles' section is the reader's own and is not carried because the frame changed"
            );
            for (n, was) in held.iter().enumerate() {
                let now = modifier(s.document().unwrap(), n);
                assert!(close(*was, now), "{what}: modifier {n} was {was} and is {now} in the model");
            }
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
            // The file opened again holds the same modifiers.
            let mut again = Session::new();
            run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
            for (n, was) in held.iter().enumerate() {
                let now = modifier(again.document().unwrap(), n);
                assert!(close(*was, now), "{what}: modifier {n} was {was} and the file read again says {now}");
            }
        }
    }
}

/// The page's first shape's cell, as the model holds it.
fn cell_of(doc: &Document, section: &str, row: &str, cell: &str) -> Option<diacraft_doc::Cell> {
    let sheet = doc.sheet_of(&SheetRef::Shape(ShapeRef::top(Owner::Page(doc.pages.get(0).unwrap().id), 0)))?;
    match sheet.lookup(&CellAddr::in_row(section, None, RowKey::name(row), cell)) {
        Lookup::Found(c) => Some(c.clone()),
        _ => None,
    }
}

/// **A drag still changes the modifiers** (the handle written as the share of the frame it was dragged to, which a handle's drag writes), and a resize after it keeps what
/// the drag made. The save writes the one attribute, and the file saved again with no edit is the same bytes.
#[test]
fn a_drag_still_changes_the_modifier_and_a_resize_after_it_keeps_the_new_one() {
    let bytes = drawing(&CASES[0]).bytes();
    let (mut doc, report) = read(&bytes, &Limits::default()).unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(page), 0));
    let handle = CellAddr::in_row("Control", None, RowKey::name("Row_1"), "X");
    let settle = |doc: &mut Document, cell: &CellAddr| {
        let changes = Changes { cells: vec![(sheet.clone(), cell.clone())], ..Changes::default() };
        let done = commit_changed(doc, &Budget::default(), EvalOptions::default(), page, &changes);
        assert!(!done.budget_exhausted && !done.memory_exhausted);
    };
    // The drag: the handle goes to an eighth of the width.
    let formula = std::sync::Arc::new(diacraft_formula::parse("Width*0.125"));
    doc.write_cell(&sheet, &handle, CellEdit::Formula { formula, value: None }, WriteOrigin::Drawing).unwrap();
    settle(&mut doc, &handle);
    assert!(close(modifier(&doc, 0), 2700.0), "the modifier follows the handle: {}", modifier(&doc, 0));
    // A resize of the frame: twice the width.
    let width = CellAddr::direct("Width");
    let wide = cell_of_shape(&doc, "Width") * 2.0;
    doc.write_cell(&sheet, &width, CellEdit::Value(Value::from_number(wide).unwrap()), WriteOrigin::Drawing).unwrap();
    settle(&mut doc, &width);
    assert!(close(modifier(&doc, 0), 2700.0), "the modifier is what the drag made it after a resize: {}", modifier(&doc, 0));
    assert!(close(cell_of(&doc, "User", "ODFMod0", "Value").and_then(|c| c.value?.number()).unwrap(), 2700.0));
    let out = write_with(&doc, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &WriteOptions::default()).unwrap();
    assert!(out.edited);
    let content = content_of(&out.bytes);
    assert_eq!(modifiers_of(&content), "2700 7", "the one attribute that holds the modifier");
    let (back, back_report) = read(&out.bytes, &Limits::default()).unwrap();
    assert!(close(modifier(&back, 0), 2700.0));
    let again = write_with(&back, Some(&OriginalPackage { bytes: &out.bytes, index: &back_report.index }), &WriteOptions::default()).unwrap();
    assert!(!again.edited, "a second save is no edit");
    assert_eq!(again.bytes, out.bytes);
}

fn cell_of_shape(doc: &Document, name: &str) -> f64 {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap()
}

/// **A modifier of 1.7e308 is not lost by a resize**: the model holds what the file says afterwards. The handle's share of the
/// frame is a very large number (`Width*7.87037037037037e303`), the modifier works out to the file's own, and halving the width does not turn it into an error.
#[test]
fn a_modifier_of_1_7e308_survives_a_resize_in_the_model_and_in_the_file() {
    let case = Shape {
        name: "a modifier at the top of the range",
        view_box: "0 0 21600 21600",
        modifiers: "1.7e308 7",
        handles: r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/>"#,
    };
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &drawing(&case));
    assert!(close(modifier(s.document().unwrap(), 0), 1.7e308));
    resize(&mut s, 0.5, 1.0);
    let in_model = modifier(s.document().unwrap(), 0);
    assert!(in_model.is_finite() && close(in_model, 1.7e308), "the model lost the value: it holds {in_model}");
    // The resize is saved, and the file says what it said: the same modifiers, as the model holds them.
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true);
    let said = modifiers_of(&content_of(&dir.read("a.odg")));
    assert_eq!(said, "1.7e308 7");
    assert!(
        close(said.split_whitespace().next().and_then(|t| t.parse().ok()).unwrap_or(f64::NAN), in_model),
        "the file says {said}, the model holds {in_model}"
    );
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}
