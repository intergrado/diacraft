//! The Comments pane, New Comment, the markers on the page, Show Markup, Previous and Next, and the Comments button, given to the window as a person's pointer and keys
//! would be. What is checked is the document they changed through commands.

use egui::{Key, Modifiers, Rect};
use serde_json::{Value, json};

use crate::editing::{at, drag_tile_to, edit_window, key, shape_ids, type_text};
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, run_command};

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn drawn(w: &Window, name: &str) -> bool {
    w.state().element(name).is_some()
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
}

fn window_with_a_shape() -> (Window, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "comment.author", json!({"name": "Ada Lovelace", "initials": "AL"}));
    (w, id)
}

fn comments(w: &Window) -> Vec<(u32, Option<u32>, Option<u32>, String)> {
    w.state().session().document().unwrap().comments.iter().map(|c| (c.id, c.parent, c.shape, c.text.to_string())).collect()
}

fn panes(w: &Window) -> Value {
    w.state().window_state()["panes"].clone()
}

/// Types into the box that has the keyboard.
fn write(w: &mut Window, text: &str) {
    type_text(w, text);
    w.rest();
}

#[test]
fn new_comment_opens_the_pane_with_a_box_and_post_adds_a_comment_on_the_shape_selected() {
    let (mut w, shape) = window_with_a_shape();
    run_command(&mut w, "window.newComment", json!({}));
    assert!(panes(&w).to_string().contains("comments"), "the pane is open: {}", panes(&w));
    assert!(drawn(&w, "comments.compose"), "the box a comment is typed in");
    write(&mut w, "Check this size");
    click_on(&mut w, "comments.post");
    let all = comments(&w);
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].2, all[0].3.as_str()), (Some(shape), "Check this size"));
    // The card is in the pane, with its reply button.
    assert!(drawn(&w, "comments.card.1") && drawn(&w, "comments.reply.1"));
}

#[test]
fn with_nothing_selected_a_comment_is_the_pages_and_post_stays_quiet_until_there_is_text() {
    let (mut w, _) = window_with_a_shape();
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    run_command(&mut w, "window.newComment", json!({}));
    assert!(!w.state().element("comments.post").unwrap().enabled, "no text, no Post");
    write(&mut w, "About the page");
    click_on(&mut w, "comments.post");
    assert_eq!(comments(&w).iter().map(|c| c.2).collect::<Vec<_>>(), vec![None]);
}

#[test]
fn ctrl_enter_posts_and_cancel_takes_the_box_away() {
    let (mut w, _) = window_with_a_shape();
    run_command(&mut w, "window.newComment", json!({}));
    write(&mut w, "keyboard");
    key(&mut w, Key::Enter, ctrl());
    assert_eq!(comments(&w).len(), 1);
    run_command(&mut w, "window.newComment", json!({}));
    write(&mut w, "never posted");
    click_on(&mut w, "comments.cancel");
    assert_eq!(comments(&w).len(), 1);
    assert!(!drawn(&w, "comments.compose"), "the box is gone");
}

/// Ctrl+Enter posts and a press with Alt held too (AltGr is Ctrl and Alt) does not.
#[test]
fn ctrl_alt_enter_does_not_post_a_comment() {
    let (mut w, _) = window_with_a_shape();
    run_command(&mut w, "window.newComment", json!({}));
    write(&mut w, "not yet");
    key(&mut w, Key::Enter, Modifiers { alt: true, ..ctrl() });
    assert!(comments(&w).is_empty(), "Ctrl+Alt+Enter posts nothing");
    key(&mut w, Key::Enter, ctrl());
    assert_eq!(comments(&w).len(), 1, "Ctrl+Enter posts");
}

#[test]
fn a_reply_an_edit_and_a_delete_are_commands_of_the_pane_and_one_undo_step_each() {
    let (mut w, _) = window_with_a_shape();
    run_command(&mut w, "comment.add", json!({"text": "question"}));
    run_command(&mut w, "window.pane", json!({"pane": "comments", "shown": true}));
    click_on(&mut w, "comments.reply.1");
    write(&mut w, "answer");
    click_on(&mut w, "comments.reply_post");
    assert_eq!(comments(&w).len(), 2);
    assert_eq!(comments(&w)[1].1, Some(1), "the reply answers the first comment");
    click_on(&mut w, "comments.edit.1");
    key(&mut w, Key::A, ctrl());
    write(&mut w, "question, edited");
    click_on(&mut w, "comments.save");
    assert_eq!(comments(&w)[0].3, "question, edited");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(comments(&w)[0].3, "question", "the edit was one undo step");
    click_on(&mut w, "comments.delete.1");
    assert!(comments(&w).is_empty(), "the comment and its reply are gone");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(comments(&w).len(), 2);
}

#[test]
fn the_ribbon_has_new_comment_and_the_comments_menu_with_show_markup_previous_and_next() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "Review"}));
    for name in ["ribbon.Review.ReviewNewComment", "ribbon.Review.ShowCommentsMenu"] {
        assert!(w.state().element(name).is_some_and(|e| e.enabled), "{name} works");
    }
}

#[test]
fn a_marker_is_painted_for_a_comment_on_the_page_and_a_click_on_it_opens_the_comment() {
    let (mut w, shape) = window_with_a_shape();
    run_command(&mut w, "comment.add", json!({"id": shape, "text": "on the shape"}));
    run_command(&mut w, "window.pane", json!({"pane": "comments", "shown": false}));
    // The marker is where the engine says: the shape's top right corner.
    let list = w.state_mut().execute("comment.list", &json!({"details": true})).unwrap();
    let anchor = &list["comments"][0]["anchor"];
    let (x, y) = (anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap());
    // The marker's centre is a little right of and above the corner; a click there opens the comment and the pane.
    let corner = at(&w, x, y);
    click(&mut w, corner + egui::vec2(10.0, -10.0));
    w.rest();
    assert!(panes(&w).to_string().contains("comments"), "the click opened the pane: {}", panes(&w));
    assert!(drawn(&w, "comments.card.1"));
    assert_eq!(w.state().selection(), &[shape], "the shape is selected");
}

#[test]
fn show_markup_off_hides_the_markers_and_adding_a_comment_turns_it_on() {
    let (mut w, shape) = window_with_a_shape();
    run_command(&mut w, "comment.add", json!({"id": shape, "text": "t"}));
    run_command(&mut w, "comment.markers", json!({"show": false}));
    assert_eq!(w.state().session().document().unwrap().comments.show_tags, Some(false));
    run_command(&mut w, "comment.add", json!({"text": "again"}));
    assert_eq!(w.state().session().document().unwrap().comments.show_tags, Some(true));
}

#[test]
fn previous_and_next_go_to_the_comment_and_select_its_shape() {
    let (mut w, a) = window_with_a_shape();
    drag_tile_to(&mut w, "Rectangle", 7.0, 5.0);
    let b = shape_ids(&w)[1];
    run_command(&mut w, "comment.add", json!({"id": a, "text": "on a"}));
    run_command(&mut w, "comment.add", json!({"id": b, "text": "on b"}));
    run_command(&mut w, "window.stepComment", json!({"forward": true}));
    assert_eq!(w.state().selection(), &[a]);
    run_command(&mut w, "window.stepComment", json!({"forward": true}));
    assert_eq!(w.state().selection(), &[b]);
    run_command(&mut w, "window.stepComment", json!({"forward": false}));
    assert_eq!(w.state().selection(), &[a]);
}

#[test]
fn the_comments_button_of_the_title_bar_opens_and_closes_the_pane() {
    let (mut w, _) = window_with_a_shape();
    assert!(drawn(&w, "titlebar.comments"));
    click_on(&mut w, "titlebar.comments");
    assert!(panes(&w).to_string().contains("comments"));
    click_on(&mut w, "titlebar.comments");
    assert!(!panes(&w).to_string().contains("comments"));
}

#[test]
fn escape_cancels_a_comment_being_typed_and_leaves_the_selection_alone() {
    let (mut w, _shape) = window_with_a_shape();
    assert_eq!(w.state().selection().len(), 1, "the shape is selected");
    run_command(&mut w, "window.newComment", json!({}));
    write(&mut w, "half a thought");
    key(&mut w, Key::Escape, Modifiers::NONE);
    w.rest();
    assert!(!drawn(&w, "comments.compose"), "the box is gone");
    assert!(comments(&w).is_empty(), "nothing was posted");
    assert_eq!(w.state().selection().len(), 1, "the Escape was the box's: the selection is as it was");
}

#[test]
fn escape_cancels_a_reply_and_an_edit_being_typed() {
    let (mut w, _) = window_with_a_shape();
    run_command(&mut w, "comment.add", json!({"text": "first"}));
    run_command(&mut w, "window.pane", json!({"pane": "comments", "shown": true}));
    click_on(&mut w, "comments.reply.1");
    assert!(drawn(&w, "comments.replybox.1"));
    write(&mut w, "an answer that is not sent");
    key(&mut w, Key::Escape, Modifiers::NONE);
    w.rest();
    assert!(!drawn(&w, "comments.replybox.1"), "the reply box is gone");
    assert_eq!(comments(&w).len(), 1, "no reply was posted");
    assert_eq!(w.state().selection().len(), 1);
    // The edit box.
    click_on(&mut w, "comments.edit.1");
    assert!(drawn(&w, "comments.editbox.1"));
    write(&mut w, " and more");
    key(&mut w, Key::Escape, Modifiers::NONE);
    w.rest();
    assert!(!drawn(&w, "comments.editbox.1"), "the edit box is gone");
    assert_eq!(comments(&w)[0].3, "first", "the text is as it was");
}
