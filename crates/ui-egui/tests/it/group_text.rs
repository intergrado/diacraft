//! Typing on a group reaches the member that holds its text (`IsTextEditTarget`; `SelectMode`).
//! A group whose `IsTextEditTarget` is FALSE sends typed text, F2 and a double click on the group to the member on top of the
//! stack, the last in document order; a group whose cell is TRUE, or that has none, takes the text itself as it always did. A double click on a
//! member that is selected on its own edits that member.
//!
//! The groups are made as a person makes them (`shape.group` on three rectangles, each with its own words) and their cells are set by a command.
//! The drop order is the document order: the first rectangle dropped is the first member and the last is the one on top of the stack, so the
//! rectangle that holds "Name", dropped last, is the member the group's text goes to.

use diacraft_engine::doc::Owner;
use diacraft_engine::draw::text_view;
use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{at, click_at, edit_window, key, type_text};
use crate::support::{Rest, Window};

/// A group of three boxes one over the other, and the `ID`s of its members in document order.
struct Class {
    group: u32,
    /// Operations (at 5 inches up), Attributes (6), Name (7): the last is the one on top of the stack.
    members: [u32; 3],
}

impl Class {
    fn operations(&self) -> u32 {
        self.members[0]
    }

    fn attributes(&self) -> u32 {
        self.members[1]
    }

    fn name(&self) -> u32 {
        self.members[2]
    }
}

/// The page the window shows.
fn page_of(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// Runs a command as the window would, draws until the window settles, and gives the answer.
fn command(w: &mut Window, id: &str, params: Value) -> Value {
    let answer = w.state_mut().execute(id, &params).unwrap_or_else(|e| panic!("`{id}` failed: {e}"));
    w.rest();
    answer
}

fn id_of(answer: &Value) -> u32 {
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// A box of 1.5 by 1 inch centred at `(x, y)`, with `text` in it.
fn boxed_with(w: &mut Window, text: &str, (x, y): (f64, f64)) -> u32 {
    let page = page_of(w);
    let id = id_of(&command(w, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y})));
    command(w, "shape.set_text", json!({"id": id, "text": text, "page": page}));
    id
}

fn set_cell(w: &mut Window, id: u32, cell: &str, formula: &str) {
    let page = page_of(w);
    command(w, "sheet.set_cell", json!({"id": id, "page": page, "cell": cell, "formula": formula}));
}

fn group_of(w: &mut Window, ids: &[u32]) -> u32 {
    let page = page_of(w);
    id_of(&command(w, "shape.group", json!({"ids": ids, "page": page})))
}

/// The `ID`s of the members of group `id`, in document order.
fn members_of(w: &Window, id: u32) -> Vec<u32> {
    let doc = w.state().session().document().unwrap();
    let at = doc.find_shape(Owner::Page(page_of(w)), id).unwrap();
    doc.shape(&at).unwrap().children.iter().map(|shape| shape.id).collect()
}

/// A window at 100% on a class: three boxes grouped, with `cells` written to the group.
fn class_window(cells: &[(&str, &str)]) -> (Window, Class) {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = [boxed_with(&mut w, "Operations", (4.0, 5.0)), boxed_with(&mut w, "Attributes", (4.0, 6.0)), boxed_with(&mut w, "Name", (4.0, 7.0))];
    let group = group_of(&mut w, &ids);
    for (cell, formula) in cells {
        set_cell(&mut w, group, cell, formula);
    }
    assert_eq!(members_of(&w, group), ids, "the members keep the order they were dropped in");
    (w, Class { group, members: ids })
}

/// The characters of shape `id`.
fn chars(w: &Window, id: u32) -> String {
    text_view(w.state().session().document().unwrap(), page_of(w), id).unwrap().chars
}

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

fn editing(w: &Window) -> bool {
    w.state().window_state()["editingText"].as_bool().unwrap()
}

/// The group is selected by a click on the middle box.
fn select_group(w: &mut Window, class: &Class) {
    click_at(w, 4.0, 6.0);
    assert_eq!(w.state().selection(), [class.group], "the first click takes the group");
    pause(w);
}

// ----: FALSE sends the text to the member on top of the stack ---------------------------------------------------------

/// Given 0 and members A then B, typed text reaches B, the last in document order.
#[test]
fn typing_on_a_selected_group_whose_is_text_edit_target_is_false_edits_its_last_member() {
    let (mut w, class) = class_window(&[("IsTextEditTarget", "0")]);
    select_group(&mut w, &class);
    let before = w.state().session().revision();
    type_text(&mut w, "Zed");
    assert!(editing(&w), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(class.name()), "over the member on top of the stack, and not the group");
    assert_eq!(w.state().edited_text(), Some("Zed"), "the member's words were selected, and what is typed replaces them");
    assert_eq!(w.state().selection(), [class.group], "the group stays selected");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, class.name()), "Zed");
    assert_eq!(chars(&w, class.attributes()), "Attributes", "no other member is touched");
    assert_eq!(chars(&w, class.operations()), "Operations");
    assert_eq!(chars(&w, class.group), "", "and the group has no text of its own");
    assert_eq!(w.state().session().revision(), before + 1, "one command, one undo step");
    assert_eq!(w.state().selection(), [class.group]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// F2 on such a group opens its last member with all its words selected.
#[test]
fn f2_on_such_a_group_edits_its_last_member() {
    let (mut w, class) = class_window(&[("IsTextEditTarget", "0")]);
    select_group(&mut w, &class);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(editing(&w));
    assert_eq!(w.state().edited_text_shape(), Some(class.name()));
    assert_eq!(w.state().edited_text(), Some("Name"));
    assert_eq!(w.state().edited_text_selection(), Some((0, 4)), "all of the member's words are selected");
    // The box is drawn over that member, whose middle is at (4, 7), and not over the group's middle at (4, 6).
    let shown = w.state().element("shape.text").expect("the box is drawn").rect.center();
    let (member, group) = (at(&w, 4.0, 7.0), at(&w, 4.0, 6.0));
    assert!((shown - member).length() < (shown - group).length(), "the box is at {shown:?}: the member is at {member:?}, the group at {group:?}");
    type_text(&mut w, "Person");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, class.name()), "Person");
    assert_eq!(chars(&w, class.group), "");
}

/// A double click on a member of a group that is selected edits that member, and not the group's last one: the attributes box is double clicked
/// and its own words are in the text box.
#[test]
fn a_double_click_on_a_member_of_a_selected_group_edits_that_member() {
    let (mut w, class) = class_window(&[("IsTextEditTarget", "0")]);
    select_group(&mut w, &class);
    click_at(&mut w, 4.0, 6.0);
    click_at(&mut w, 4.0, 6.0);
    assert!(editing(&w), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(class.attributes()));
    assert_eq!(w.state().edited_text(), Some("Attributes"));
    assert_eq!(w.state().selection(), [class.attributes()], "the member is the selection, as a second click makes it");
    type_text(&mut w, "+ id : Int");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, class.attributes()), "+ id : Int");
    assert_eq!(chars(&w, class.name()), "Name");
    assert_eq!(chars(&w, class.group), "");
}

/// A group whose cell is TRUE (or has none: the root style holds 1) takes the text itself, as it always did: typing, F2 and the
/// double click all edit the group's own words, whatever its members say.
#[test]
fn a_group_with_is_text_edit_target_true_edits_its_own_text_as_before() {
    for cells in [&[("IsTextEditTarget", "1")][..], &[("IsTextEditTarget", "TRUE")], &[]] {
        let (mut w, class) = class_window(cells);
        select_group(&mut w, &class);
        type_text(&mut w, "Whole");
        assert!(editing(&w), "{cells:?}");
        assert_eq!(w.state().edited_text_shape(), Some(class.group), "{cells:?}");
        assert_eq!(w.state().edited_text(), Some("Whole"));
        key(&mut w, Key::Escape, Modifiers::NONE);
        assert_eq!(chars(&w, class.group), "Whole", "{cells:?}");
        assert_eq!(chars(&w, class.name()), "Name", "{cells:?}");
        // F2 opens the group's own words.
        key(&mut w, Key::F2, Modifiers::NONE);
        assert_eq!(w.state().edited_text_shape(), Some(class.group), "{cells:?}");
        assert_eq!(w.state().edited_text(), Some("Whole"), "{cells:?}");
        key(&mut w, Key::Escape, Modifiers::NONE);
    }
}

/// A group with 1 takes the first click, and its member the second; the text cell changes neither.
#[test]
fn a_group_with_select_mode_1_takes_the_first_click_and_its_member_the_second() {
    let (mut w, class) = class_window(&[("SelectMode", "1"), ("IsTextEditTarget", "0")]);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().selection(), [class.group], "the first click picks the group");
    pause(&mut w);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().selection(), [class.attributes()], "the second picks the member under the pointer");
    assert!(!editing(&w), "and neither opens the text");
}

// ---- the rest of the ways in ----------------------------------------------------------------------------------------------------------------

/// A double click on a group that was not selected selects it with the first click, and its second click does not go in to the member under the
/// pointer: the double click edits the group's text target, the last member, wherever on the group it lands.
#[test]
fn a_double_click_on_a_group_that_is_not_selected_edits_its_last_member() {
    let (mut w, class) = class_window(&[("IsTextEditTarget", "0")]);
    assert!(w.state().selection().is_empty(), "nothing is selected: the first click of the pair takes the group");
    click_at(&mut w, 4.0, 5.0);
    click_at(&mut w, 4.0, 5.0);
    assert!(editing(&w), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(class.name()), "the operations box was double clicked, and the name holds the text");
    assert_eq!(w.state().selection(), [class.group]);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!editing(&w));
}

/// The same double click on a group that takes the text itself edits the group's text, as before: the second click of the pair does not pick the
/// member under it.
#[test]
fn a_double_click_on_a_group_with_is_text_edit_target_true_edits_its_own_text() {
    let (mut w, class) = class_window(&[]);
    click_at(&mut w, 4.0, 5.0);
    click_at(&mut w, 4.0, 5.0);
    assert!(editing(&w));
    assert_eq!(w.state().edited_text_shape(), Some(class.group));
    assert_eq!(w.state().selection(), [class.group]);
}

/// A member selected on its own is typed into whatever its group's cell says: F2 on the attributes box edits the attributes, in a group of either kind.
#[test]
fn a_member_that_is_selected_on_its_own_is_typed_into_whatever_its_group_says() {
    for cell in ["0", "1"] {
        let (mut w, class) = class_window(&[("IsTextEditTarget", cell)]);
        select_group(&mut w, &class);
        click_at(&mut w, 4.0, 6.0);
        assert_eq!(w.state().selection(), [class.attributes()], "{cell}");
        pause(&mut w);
        key(&mut w, Key::F2, Modifiers::NONE);
        assert_eq!(w.state().edited_text_shape(), Some(class.attributes()), "{cell}");
        assert_eq!(w.state().edited_text(), Some("Attributes"), "{cell}");
    }
}

/// The text tool on a group is typing on it: the group is selected and the text goes where its cell says.
#[test]
fn the_text_tool_on_a_group_whose_is_text_edit_target_is_false_types_into_its_last_member() {
    let (mut w, class) = class_window(&[("IsTextEditTarget", "0")]);
    command(&mut w, "tool.text", json!({}));
    click_at(&mut w, 4.0, 5.0);
    assert!(editing(&w));
    assert_eq!(w.state().edited_text_shape(), Some(class.name()));
    assert_eq!(w.state().selection(), [class.group]);
}

/// A group whose `SelectMode` is 0 offers no members to the pointer, and typing still reaches the member that holds the text: the box is drawn over that
/// member, from the place the engine finds for it, since the pointer's scene has none.
#[test]
fn a_group_set_to_group_only_still_sends_its_typing_to_its_last_member() {
    let (mut w, class) = class_window(&[("SelectMode", "0"), ("IsTextEditTarget", "0")]);
    click_at(&mut w, 4.0, 6.0);
    assert_eq!(w.state().selection(), [class.group]);
    pause(&mut w);
    type_text(&mut w, "Quiet");
    pause(&mut w);
    assert!(editing(&w), "the box stays open though the pointer cannot reach the member");
    assert_eq!(w.state().edited_text_shape(), Some(class.name()));
    let shown = w.state().element("shape.text").expect("and is drawn").rect.center();
    let (member, group) = (at(&w, 4.0, 7.0), at(&w, 4.0, 6.0));
    assert!((shown - member).length() < (shown - group).length(), "over the member at {member:?}, and not the group's middle {group:?}: {shown:?}");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(chars(&w, class.name()), "Quiet");
    assert_eq!(chars(&w, class.group), "");
}

/// A group that holds a group follows the cell of each: FALSE, then FALSE again, ends at the last member of the inner group; the inner group with
/// TRUE takes the text itself.
#[test]
fn a_group_in_a_group_is_followed_by_the_cell_of_each() {
    let mut w = edit_window([1280.0, 900.0]);
    let first = boxed_with(&mut w, "First", (4.0, 7.0));
    let (below, last) = (boxed_with(&mut w, "Below", (4.0, 6.0)), boxed_with(&mut w, "Last", (4.0, 5.0)));
    let inner = group_of(&mut w, &[below, last]);
    let outer = group_of(&mut w, &[first, inner]);
    assert_eq!(members_of(&w, outer), [first, inner], "the inner group is the last member of the outer one");
    assert_eq!(members_of(&w, inner), [below, last]);
    set_cell(&mut w, outer, "IsTextEditTarget", "0");
    set_cell(&mut w, inner, "IsTextEditTarget", "0");
    click_at(&mut w, 4.0, 7.0);
    assert_eq!(w.state().selection(), [outer]);
    pause(&mut w);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text_shape(), Some(last), "the outer group sends it to the inner, and the inner to its last member");
    key(&mut w, Key::Escape, Modifiers::NONE);
    set_cell(&mut w, inner, "IsTextEditTarget", "1");
    pause(&mut w);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text_shape(), Some(inner), "an inner group that takes the text itself ends the walk");
}
