//! Typing on a class dropped from the UML Class stencil edits its name (`IsTextEditTarget`; `SelectMode`). The class is the stencil's
//! own group master (`uml-class.class`: operations, attributes and the name, as members), dropped by `shape.drop` as a person drops it from the Shapes window; the engine's tests say which member
//! the name is, and this file says that the window sends the typing there.

use diacraft_engine::doc::Owner;
use diacraft_engine::draw::text_view;
use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{at, click_at, edit_window, key, type_text};
use crate::support::{Rest, Window};

/// The page the window shows.
pub(crate) fn page_of(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// Runs a command as the window would, draws until the window settles, and gives the answer.
pub(crate) fn command(w: &mut Window, id: &str, params: Value) -> Value {
    let answer = w.state_mut().execute(id, &params).unwrap_or_else(|e| panic!("`{id}` failed: {e}"));
    w.rest();
    answer
}

/// A window at 100% with a class dropped at (4, 6): the class and its members in document order (operations, attributes, name).
fn class_window() -> (Window, u32, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_of(&w);
    let dropped = command(&mut w, "shape.drop", json!({ "master": "uml-class.class", "stencil": "uml-class", "page": page, "x": 4.0, "y": 6.0 }));
    let class = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(page), class).unwrap();
    let members: Vec<u32> = doc.shape(&at).unwrap().children.iter().map(|m| m.id).collect();
    assert_eq!(members.len(), 3, "operations, attributes and the name");
    (w, class, members)
}

fn chars(w: &Window, id: u32) -> String {
    text_view(w.state().session().document().unwrap(), page_of(w), id).unwrap().chars
}

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

pub(crate) fn editing(w: &Window) -> bool {
    w.state().window_state()["editingText"].as_bool().unwrap()
}

#[test]
fn typing_on_a_selected_class_edits_its_name() {
    let (mut w, class, members) = class_window();
    let name = *members.last().unwrap();
    // The class is 1.6 by 1.2 inches at (4, 6): its middle is on the attributes. The first click takes the group (SelectMode 1).
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().selection(), [class], "the first click takes the class");
    pause(&mut w);
    let before = w.state().session().revision();
    type_text(&mut w, "Account");
    assert!(editing(&w), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(name), "over the name, and not the class nor another compartment");
    assert_eq!(w.state().selection(), [class], "the class stays selected");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    // The box is drawn over the top compartment (the name is a rank-0 member and stands at the top of the class), 0.4 inch under its top edge.
    let shown = w.state().element("shape.text").expect("the box is drawn").rect.center();
    let (top, middle) = (at(&w, 4.0, 6.0 + 0.6 - 0.2), at(&w, 4.0, 6.0));
    assert!((shown - top).length() < (shown - middle).length(), "the box is at {shown:?}: the name is at {top:?}, the middle at {middle:?}");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, name), "Account");
    assert_eq!(chars(&w, members[1]), "", "the attributes keep their placeholder and hold no text of their own");
    assert_eq!(chars(&w, class), "", "and the class has none");
    assert_eq!(w.state().session().revision(), before + 1, "one command, one undo step");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_double_click_on_the_attributes_of_a_selected_class_edits_them() {
    let (mut w, class, members) = class_window();
    click_at(&mut w, 4.0, 6.0);
    pause(&mut w);
    click_at(&mut w, 4.0, 6.0);
    click_at(&mut w, 4.0, 6.0);
    assert!(editing(&w), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(members[1]), "the attributes are the member that was double clicked");
    type_text(&mut w, "+ id : Int");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, members[1]), "+ id : Int");
    assert_eq!(chars(&w, members[2]), "", "the name is not touched");
    assert_eq!(w.state().selection(), [members[1]]);
    assert_eq!(chars(&w, class), "");
}

/// What a shape shows: its own `Text`, and when it has none its master's.
pub(crate) fn shown(w: &Window, id: u32) -> String {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(page_of(w)), id).unwrap();
    let own = doc.shape(&at).unwrap().text.clone();
    let text = own.or_else(|| {
        diacraft_engine::doc::Resolver::new(doc).master_step(&at).ok().flatten().and_then(|step| doc.shape(&step.shape).and_then(|m| m.text.clone()))
    });
    text.iter()
        .flat_map(|t| t.parts.iter())
        .filter_map(|part| match part {
            diacraft_engine::doc::TextPart::Chars(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

/// Typing all of a compartment's text away in the box leaves it empty, and the placeholder of the master does not come back.
#[test]
fn typing_all_the_text_of_the_attributes_away_leaves_them_empty() {
    let (mut w, _class, members) = class_window();
    click_at(&mut w, 4.0, 6.0);
    pause(&mut w);
    click_at(&mut w, 4.0, 6.0);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().edited_text_shape(), Some(members[1]), "the attributes are the member that was double clicked");
    type_text(&mut w, "+ id : Int");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(shown(&w, members[1]), "+ id : Int");
    // Opened again with its text selected: one key takes it all away.
    pause(&mut w);
    click_at(&mut w, 4.0, 6.0);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().edited_text_shape(), Some(members[1]));
    key(&mut w, Key::Backspace, Modifiers::NONE);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, members[1]), "");
    assert_eq!(shown(&w, members[1]), "", "the master's `+ attribute : Type` is not drawn where the text was typed away");
    assert_eq!(shown(&w, members[0]), "+ operation()", "the other compartment is as it was");
    assert_eq!(w.state().ui_panics(), 0);
}

// ----: the box opens on the text a compartment shows -------------------------------------------------------------------------

const ATTRIBUTES: &str = "+ attribute : Type";

/// Whether a shape holds a `Text` of its own (and not only shows its master's).
pub(crate) fn has_own_text(w: &Window, id: u32) -> bool {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(page_of(w)), id).unwrap();
    doc.shape(&at).unwrap().text.is_some()
}

/// The text box opened over the attributes of a class: a double click on a selected class goes in to the member under the pointer.
fn open_on_attributes() -> (Window, u32, Vec<u32>) {
    let (mut w, class, members) = class_window();
    click_at(&mut w, 4.0, 6.0);
    pause(&mut w);
    click_at(&mut w, 4.0, 6.0);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().edited_text_shape(), Some(members[1]), "the attributes are the member that was double clicked");
    assert_eq!(w.state().edited_text(), Some(ATTRIBUTES), "the box holds the text the compartment shows, which the shape does not hold itself");
    (w, class, members)
}

#[test]
fn the_box_opens_on_the_text_the_attributes_show_with_all_of_it_selected() {
    let (w, _class, members) = open_on_attributes();
    assert_eq!(chars(&w, members[1]), "", "the shape holds no text of its own");
    assert_eq!(w.state().edited_text(), Some(ATTRIBUTES), "the box holds what the compartment shows");
    assert_eq!(
        w.state().edited_text_selection(),
        Some((0, ATTRIBUTES.chars().count())),
        "all of it is selected, as a double click selects the whole text"
    );
}

/// Select all, delete, commit: the compartment is empty and stays so (it was a box that opened empty, with nothing to select and delete).
#[test]
fn selecting_all_of_the_text_the_attributes_show_and_deleting_it_leaves_them_empty() {
    let (mut w, class, members) = open_on_attributes();
    let height = frame_height(&w, members[1]);
    let before = w.state().session().revision();
    key(&mut w, Key::A, Modifiers::COMMAND);
    key(&mut w, Key::Backspace, Modifiers::NONE);
    assert_eq!(w.state().edited_text(), Some(""));
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before + 1, "one command, one undo step");
    assert!(has_own_text(&w, members[1]), "an empty text of its own says the master's is not shown");
    assert_eq!(shown(&w, members[1]), "", "the compartment is empty");
    assert_eq!(shown(&w, members[0]), "+ operation()", "the other compartment is as it was");
    assert_eq!(shown(&w, class), "");
    assert!((frame_height(&w, members[1]) - height).abs() < 1e-9, "an empty compartment is as tall as its least height");
    assert_eq!(w.state().ui_panics(), 0);
    // Undo gives the placeholder back.
    key(&mut w, Key::Z, Modifiers::COMMAND);
    assert!(!has_own_text(&w, members[1]), "the first edit is one undo step: the shape inherits again");
    assert_eq!(shown(&w, members[1]), ATTRIBUTES);
}

#[test]
fn typing_over_the_text_the_attributes_show_replaces_it_and_the_shape_holds_the_new_text() {
    let (mut w, _class, members) = open_on_attributes();
    let before = w.state().session().revision();
    type_text(&mut w, "+ id : Int");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before + 1);
    assert_eq!(chars(&w, members[1]), "+ id : Int");
    assert_eq!(shown(&w, members[1]), "+ id : Int");
    assert_eq!(shown(&w, members[0]), "+ operation()");
    // The first edit is one undo step, and it brings the inherited text back.
    key(&mut w, Key::Z, Modifiers::COMMAND);
    assert!(!has_own_text(&w, members[1]));
    assert_eq!(shown(&w, members[1]), ATTRIBUTES);
}

/// A caret in the text the shape shows and a letter typed there: the shape then holds the text with the letter in it, as if it had been its own.
#[test]
fn typing_at_the_end_of_the_text_the_attributes_show_adds_to_it() {
    let (mut w, _class, members) = open_on_attributes();
    key(&mut w, Key::End, Modifiers::NONE);
    type_text(&mut w, "s");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, members[1]), "+ attribute : Types");
    assert_eq!(shown(&w, members[1]), "+ attribute : Types");
}

/// Escape, Ctrl+Enter or a click away from a box that changed nothing write nothing: the shape goes on inheriting, and there is no undo step.
#[test]
fn ending_the_box_with_nothing_changed_writes_nothing_and_the_shape_goes_on_inheriting() {
    for end in ["escape", "ctrl-enter", "click away"] {
        let (mut w, _class, members) = open_on_attributes();
        let (before, held) = (w.state().session().revision(), w.state().session().document().unwrap().clone());
        match end {
            "escape" => key(&mut w, Key::Escape, Modifiers::NONE),
            "ctrl-enter" => key(&mut w, Key::Enter, Modifiers::COMMAND),
            _ => click_at(&mut w, 9.0, 2.0),
        }
        assert!(!editing(&w), "{end}: the box is closed");
        assert_eq!(w.state().session().revision(), before, "{end}: nothing was sent");
        assert_eq!(w.state().session().document().unwrap(), &held, "{end}: the document is as it was");
        assert!(!has_own_text(&w, members[1]), "{end}: the shape still inherits its text");
        assert_eq!(shown(&w, members[1]), ATTRIBUTES);
    }
}

/// The height of a shape, in inches (its `Height` cell, by inheritance).
fn frame_height(w: &Window, id: u32) -> f64 {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(page_of(w)), id).unwrap();
    let found = diacraft_engine::doc::Resolver::new(doc)
        .resolve(&diacraft_engine::doc::SheetRef::Shape(at), &diacraft_engine::doc::CellAddr::direct("Height"))
        .unwrap();
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}
