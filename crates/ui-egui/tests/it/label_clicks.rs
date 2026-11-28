//! Clicking the label of a group, with the mouse: the name of a BPMN, UML or C4 shape is typed in a member of the group that has no line and no fill, and a click on it must select the group,
//! and a second click the member. The pointer's scene leaves out the geometry a group hides (`NoShow`), and must not leave out this.

use diacraft_ui_egui::edit::plain_text;
use diacraft_ui_egui::edit::scene::{Kind, Scene};
use serde_json::json;

use egui::Modifiers;

use crate::editing::{at, click_at, drag_with, edit_window, frame_of};
use crate::support::{Rest, Window, move_to};

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

/// A group master of the notation stencils, dropped, with its name typed in the member that holds it, and where that label is: the group, the member and the middle of the member's text block.
fn group_with_label(w: &mut Window, master: &str, at: (f64, f64)) -> (u32, u32, (f64, f64)) {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap().id;
    let made = w.state_mut().execute("shape.drop", &json!({"master": master, "page": page, "x": at.0, "y": at.1})).unwrap();
    let group = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
    let doc = w.state().session().document().unwrap();
    let scene = Scene::build(doc, page, w.state().session().revision()).unwrap();
    // The name's member is the last of the group's members that is a plain shape that shows something to type in: the one typing goes to. A master that is one shape holds its own label.
    let member = scene.children_of(group).filter(|m| m.kind == Kind::Shape && !m.hidden).map(|m| m.id).next_back().unwrap_or(group);
    if plain_text(doc, page, member).is_empty() {
        w.state_mut().execute("shape.set_text", &json!({"id": member, "page": page, "text": "Name"})).unwrap();
    }
    let block = w.state_mut().execute("text.block", &json!({"id": member, "page": page})).unwrap();
    let corner = |i: usize| (block["corners"][i][0].as_f64().unwrap(), block["corners"][i][1].as_f64().unwrap());
    let middle = ((corner(0).0 + corner(2).0) / 2.0, (corner(0).1 + corner(2).1) / 2.0);
    w.rest();
    (group, member, middle)
}

#[test]
fn the_label_of_a_notation_group_is_clicked_to_select_the_group_and_a_second_click_selects_the_member_that_holds_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let masters = [
        "bpmn.task",
        "bpmn.sub-process",
        "bpmn.call-activity",
        "uml-class.class",
        "uml-class.interface",
        "uml-class.enumeration",
        "c4.person",
        "c4.key",
    ];
    let mut groups = 0;
    for (n, master) in masters.iter().enumerate() {
        let at = (1.6 + 2.4 * (n % 4) as f64, 6.5 - 2.4 * (n / 4) as f64);
        let (group, member, label) = group_with_label(&mut w, master, at);
        w.state_mut().select(&[]);
        w.rest();
        assert!(w.state().selection().is_empty());
        click_at(&mut w, label.0, label.1);
        assert_eq!(w.state().selection(), [group], "{master}: a click on its label selects the group");
        pause(&mut w);
        click_at(&mut w, label.0, label.1);
        assert_eq!(w.state().selection(), [member], "{master}: a second click selects the member that holds the label");
        groups += usize::from(member != group);
    }
    assert!(groups >= 6, "{groups} of the masters are groups whose label is in a member");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Whether the point is outside the shape's frame (lower left, then size).
fn outside_frame((x, y, w, h): (f64, f64, f64, f64), p: (f64, f64)) -> bool {
    p.0 < x || p.0 > x + w || p.1 < y || p.1 > y + h
}

/// The name of a shape that sits under it (a BPMN event, gateway or data object, a network device, a computer) is outside the shape's frame: a click on it selects the shape, and a second click the
/// member that holds it, as a name inside the shape does.
#[test]
fn the_name_under_a_shape_is_clicked_to_select_the_shape_and_a_second_click_selects_the_member_that_holds_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let masters = [
        "bpmn.start-event",
        "bpmn.end-event",
        "bpmn.intermediate-event",
        "bpmn.gateway",
        "bpmn.data-object",
        "network.router",
        "computers.laptop",
        "uml-sequence.destruction",
    ];
    let mut outside = 0;
    for (n, master) in masters.iter().enumerate() {
        let at = (1.6 + 2.4 * (n % 4) as f64, 6.5 - 2.4 * (n / 4) as f64);
        let (group, member, label) = group_with_label(&mut w, master, at);
        outside += usize::from(outside_frame(frame_of(&w, group), label));
        w.state_mut().select(&[]);
        w.rest();
        click_at(&mut w, label.0, label.1);
        assert_eq!(w.state().selection(), [group], "{master}: a click on its name selects it");
        pause(&mut w);
        click_at(&mut w, label.0, label.1);
        assert_eq!(w.state().selection(), [member], "{master}: a second click selects the member that holds the name");
    }
    assert!(outside >= 6, "{outside} of the names sit outside their shape's frame");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The window with an event and its name under it, and where the name is: the group, the member and the middle of the member's text block. The event is at (5, 4).
fn event_window() -> (Window, u32, u32, (f64, f64)) {
    let mut w = edit_window([1280.0, 900.0]);
    let (group, member, label) = group_with_label(&mut w, "bpmn.start-event", (5.0, 4.0));
    w.state_mut().select(&[]);
    w.rest();
    assert!(outside_frame(frame_of(&w, group), label), "the name is under the event: {label:?}");
    (w, group, member, label)
}

/// The pointer over the name of a shape is the move cursor, as it is over the shape, and over the page beside it is not.
#[test]
fn the_pointer_over_the_name_under_a_shape_is_the_move_cursor() {
    let (mut w, _, _, label) = event_window();
    let over = |w: &mut Window, p: (f64, f64)| {
        let pos = at(w, p.0, p.1);
        for _ in 0..3 {
            move_to(w, pos);
        }
        w.output().platform_output.cursor_icon
    };
    assert_eq!(over(&mut w, (5.0, 4.0)), egui::CursorIcon::Move, "over the event itself");
    assert_eq!(over(&mut w, label), egui::CursorIcon::Move, "over its name");
    assert_ne!(over(&mut w, (9.0, 2.0)), egui::CursorIcon::Move, "and not over the page beside it");
}

/// A double click on the name opens the text of the shape for editing, as one on the shape does.
#[test]
fn a_double_click_on_the_name_under_a_shape_opens_its_text() {
    let (mut w, _, member, label) = event_window();
    click_at(&mut w, label.0, label.1);
    click_at(&mut w, label.0, label.1);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    assert_eq!(w.state().edited_text_shape(), Some(member), "over the member that holds the name");
}

/// A drag that begins on the name moves the shape, and a net drawn from the page round the shape and its name takes it; a net round the shape alone takes it too.
#[test]
fn a_drag_from_the_name_moves_the_shape_and_a_net_takes_the_shape_with_its_name() {
    let (mut w, group, _, label) = event_window();
    let before = frame_of(&w, group);
    drag_with(&mut w, label, (label.0 + 2.0, label.1), Modifiers::NONE);
    assert_eq!(w.state().selection(), [group]);
    let after = frame_of(&w, group);
    assert!(after.0 > before.0 + 1.0, "the event moved right by about two inches: {before:?} to {after:?}");
    // Back to where it was, the nets: round the event and its name, then round the event alone.
    drag_with(&mut w, (label.0 + 2.0, label.1), label, Modifiers::NONE);
    w.state_mut().select(&[]);
    w.rest();
    drag_with(&mut w, (3.5, 2.5), (6.5, 5.5), Modifiers::NONE);
    assert_eq!(w.state().selection(), [group], "a net round the event and its name");
    w.state_mut().select(&[]);
    w.rest();
    drag_with(&mut w, (4.5, 3.7), (5.5, 4.5), Modifiers::NONE);
    assert_eq!(w.state().selection(), [group], "a net round the event alone");
}
