//! Bullets, indents, Change Case and text direction through a save and a read: a new drawing written to `.diacraft`, `.vsdx` and `.odg`
//! by the model writers, a VSDX opened and saved over itself (the splice writer), and an `.odg` opened and saved over itself. What `shape.formatting`
//! says of each shape after the read is what it said before the save.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::odg_build::{Dir, Drawing, open_drawing, rect};
use crate::odg_file::{assert_reopens_as_saved, save};
use crate::{blank_session, run};

/// What a person sees of a shape's paragraphs and letters: the fields the Home tab shows.
fn seen(s: &mut Session, id: u32) -> Value {
    let answer = run(s, "shape.formatting", json!({ "ids": [id] }));
    let one = &answer["shapes"][0];
    json!({
        "bullet": one["bullet"], "ind_left": one["ind_left"], "case": one["case"], "direction": one["direction"], "vertical": one["vertical"],
        "bold": one["bold"], "align": one["align"],
    })
}

/// The ids of the shapes of the first page, in order.
fn ids(s: &Session) -> Vec<u32> {
    s.document().unwrap().pages.get(0).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

/// A drawing of two shapes: a list (a bullet, an indent step and capitals), and a label that runs down its block, right to left, in small capitals.
fn formatted() -> Session {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({ "id": a, "text": "one\ntwo" }));
    run(&mut s, "text.format", json!({ "ids": [a], "bullet": 2, "indent": "increase", "case": "all_caps", "align": "left" }));
    let b = drop_shape(&mut s, "rectangle", 7.0, 4.0);
    run(&mut s, "shape.set_text", json!({ "id": b, "text": "hello world" }));
    run(&mut s, "text.format", json!({ "ids": [b], "vertical": true, "direction": "rtl", "case": "small_caps" }));
    s
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

#[test]
fn what_the_commands_set_reads_back_from_all_three_formats() {
    let mut s = formatted();
    let before: Vec<Value> = ids(&s).into_iter().map(|id| seen(&mut s, id)).collect();
    // The commands set what the first lines say (a save keeps the drawing's format, so each format is saved from a drawing made again).
    assert_eq!(before[0]["bullet"], 2, "{before:?}");
    assert_eq!(before[0]["ind_left"], 0.5, "the bullet's hanging form, then a step: {before:?}");
    assert_eq!(before[0]["case"], "all_caps");
    assert_eq!(before[1]["vertical"], true);
    assert_eq!(before[1]["direction"], "rtl");
    assert_eq!(before[1]["case"], "small_caps");
    let dir = Dir::new();
    for name in ["out.diacraft", "out.vsdx", "out.odg"] {
        let path = dir.path(name);
        let mut s = formatted();
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut again = opened(&path);
        let after: Vec<Value> = ids(&again).into_iter().map(|id| seen(&mut again, id)).collect();
        assert_eq!(after.len(), 2, "{name}");
        for (i, (b, a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(b, a, "{name}: shape {i}");
        }
    }
}

#[test]
fn a_vsdx_opened_and_saved_over_itself_holds_the_cells_the_commands_wrote() {
    let dir = Dir::new();
    let mut s = formatted();
    let first = dir.path("first.vsdx");
    run(&mut s, "file.save_as", json!({ "path": first }));
    // Open the VSDX the writer made, format again (a bullet off one list and on a label, a step back, another case, across again), save it over itself
    // (the splice writer) and read it again.
    let mut s = opened(&first);
    let list = ids(&s);
    run(&mut s, "text.format", json!({ "ids": [list[0]], "indent": "decrease", "case": "normal" }));
    run(&mut s, "text.format", json!({ "ids": [list[1]], "vertical": false, "bullet": 4, "direction": "ltr", "case": "initial_caps" }));
    let want: Vec<Value> = list.iter().map(|id| seen(&mut s, *id)).collect();
    assert_eq!(want[0]["ind_left"], 0.25);
    assert_eq!(want[0]["case"], "normal");
    assert_eq!(want[1]["bullet"], 4);
    assert_eq!(want[1]["vertical"], false);
    let second = dir.path("second.vsdx");
    run(&mut s, "file.save_as", json!({ "path": second }));
    let mut again = opened(&second);
    let got: Vec<Value> = ids(&again).into_iter().map(|id| seen(&mut again, id)).collect();
    assert_eq!(want, got);
}

#[test]
fn an_odg_opened_edited_and_saved_over_itself_reads_back_the_same_paragraphs() {
    let dir = Dir::new();
    let drawing = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "").replace("<text:p/>", "<text:p>one</text:p><text:p>two</text:p>"));
    let mut s = open_drawing(&dir, &drawing);
    let a = ids(&s)[0];
    run(&mut s, "text.format", json!({ "ids": [a], "bullet": 3, "indent": "increase", "case": "all_caps", "direction": "rtl", "vertical": true }));
    let want = seen(&mut s, a);
    assert_eq!(want["bullet"], 3, "{want}");
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    let mut again = opened(&dir.path("a.odg"));
    let b = ids(&again)[0];
    assert_eq!(seen(&mut again, b), want);
}
