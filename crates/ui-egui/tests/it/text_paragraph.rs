//! Bullets, Decrease and Increase Indent, Change Case and the text direction buttons from the window: each is one `text.format` of the
//! selection, or of the characters selected in the text box that is open, and each shows the state of what it acts on.

use diacraft_engine::formatting::{Case, Direction, ShapeLook, look_of};
use diacraft_ui_egui::format::{Mix, Pressed};
use diacraft_ui_egui::ribbon::spec::Press;
use egui::{Key, Modifiers};
use serde_json::json;

use crate::editing::{click_at, edit_window, key, three_shapes};
use crate::support::{Rest, TempDir, Window, elements_named, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true };

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn look(w: &Window, id: u32) -> ShapeLook {
    look_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap().remove(0)
}

fn pressed(w: &Window, press: Press) -> Pressed {
    w.state().press_state(press)
}

fn enabled(w: &Window, control: &str) -> bool {
    w.state().element(&format!("ribbon.Home.{control}")).unwrap_or_else(|| panic!("{control} is not drawn")).enabled
}

/// A window with the three shapes of the editing tests, the ellipse selected and holding two lines of text.
fn lines() -> (Window, u32) {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "one\ntwo"}));
    (w, ids[2])
}

#[test]
fn the_new_controls_are_quiet_with_nothing_selected_and_work_with_a_shape_selected() {
    let mut w = edit_window(WIDE);
    let controls = [
        "Bullets",
        "IndentDecreaseWord",
        "IndentIncreaseWord",
        "ChangeCaseDialogClassic",
        "TextDirectionLeftToRight",
        "TextDirectionRightToLeft",
        "TextDirectionVertical",
    ];
    for c in controls {
        assert!(!enabled(&w, c), "{c} has nothing to act on");
    }
    three_shapes(&mut w);
    for c in controls {
        assert!(enabled(&w, c), "{c} acts on the shape selected");
    }
}

#[test]
fn the_bullets_button_toggles_and_its_menu_chooses_a_mark() {
    let (mut w, id) = lines();
    assert_eq!(pressed(&w, Press::Bullets), Pressed::Off);
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!(look(&w, id).bullet, 1, "the first mark");
    assert_eq!(look(&w, id).ind_left, 0.25, "and the hanging form");
    assert_eq!(pressed(&w, Press::Bullets), Pressed::On);
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!(look(&w, id).bullet, 0, "a second press takes it off");
    assert_eq!(look(&w, id).ind_left, 0.0);
    assert_eq!(pressed(&w, Press::Bullets), Pressed::Off);
    // The arrow opens the marks: None and the seven of the table.
    press(&mut w, "ribbon.Home.Bullets.menu");
    w.rest();
    assert_eq!(elements_named(&w, "menu.Bullet").len(), 8);
    press(&mut w, "menu.BulletDiamond");
    w.rest();
    assert_eq!(look(&w, id).bullet, 5);
    assert_eq!(w.state().bullet_mix(), Mix::One(5));
    // A press on the button, with a bullet on, takes it off whichever it is.
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!(look(&w, id).bullet, 0);
    // One undo step for each.
    key(&mut w, Key::Z, Modifiers::COMMAND);
    assert_eq!(look(&w, id).bullet, 5);
}

#[test]
fn the_indent_buttons_step_a_quarter_inch_and_stop_at_nothing() {
    let (mut w, id) = lines();
    press(&mut w, "ribbon.Home.IndentIncreaseWord");
    press(&mut w, "ribbon.Home.IndentIncreaseWord");
    w.rest();
    assert_eq!(look(&w, id).ind_left, 0.5);
    press(&mut w, "ribbon.Home.IndentDecreaseWord");
    w.rest();
    assert_eq!(look(&w, id).ind_left, 0.25);
    press(&mut w, "ribbon.Home.IndentDecreaseWord");
    press(&mut w, "ribbon.Home.IndentDecreaseWord");
    w.rest();
    assert_eq!(look(&w, id).ind_left, 0.0, "no further");
}

#[test]
fn change_case_is_a_menu_of_four_and_two_keys() {
    let (mut w, id) = lines();
    press(&mut w, "ribbon.Home.ChangeCaseDialogClassic");
    w.rest();
    assert_eq!(elements_named(&w, "menu.Text").len(), 4);
    press(&mut w, "menu.TextAllCaps");
    w.rest();
    assert_eq!(look(&w, id).case, Case::AllCaps);
    assert_eq!(w.state().case_mix(), Mix::One(Case::AllCaps));
    press(&mut w, "ribbon.Home.ChangeCaseDialogClassic");
    w.rest();
    press(&mut w, "menu.TextInitialCaps");
    w.rest();
    assert_eq!(look(&w, id).case, Case::InitialCaps);
    press(&mut w, "ribbon.Home.ChangeCaseDialogClassic");
    w.rest();
    press(&mut w, "menu.TextNormalCase");
    w.rest();
    assert_eq!(look(&w, id).case, Case::Normal);
    // Ctrl+Shift+A is All Caps on or off, and Ctrl+Shift+K Small Caps on or off.
    key(&mut w, Key::A, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::AllCaps);
    assert_eq!(pressed(&w, Press::AllCaps), Pressed::On);
    key(&mut w, Key::A, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::Normal);
    key(&mut w, Key::K, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::SmallCaps);
    assert_eq!(pressed(&w, Press::SmallCaps), Pressed::On);
    key(&mut w, Key::A, CTRL_SHIFT);
    assert_eq!(look(&w, id).case, Case::AllCaps, "one case at a time");
}

#[test]
fn the_direction_buttons_set_the_reading_direction_and_toggle_vertical_text() {
    let (mut w, id) = lines();
    assert_eq!((pressed(&w, Press::Ltr), pressed(&w, Press::Rtl), pressed(&w, Press::Vertical)), (Pressed::On, Pressed::Off, Pressed::Off));
    press(&mut w, "ribbon.Home.TextDirectionRightToLeft");
    w.rest();
    assert_eq!(look(&w, id).direction, Direction::RightToLeft);
    assert_eq!((pressed(&w, Press::Ltr), pressed(&w, Press::Rtl)), (Pressed::Off, Pressed::On));
    press(&mut w, "ribbon.Home.TextDirectionLeftToRight");
    w.rest();
    assert_eq!(look(&w, id).direction, Direction::LeftToRight);
    press(&mut w, "ribbon.Home.TextDirectionVertical");
    w.rest();
    assert!(look(&w, id).vertical);
    assert_eq!(pressed(&w, Press::Vertical), Pressed::On);
    press(&mut w, "ribbon.Home.TextDirectionVertical");
    w.rest();
    assert!(!look(&w, id).vertical, "a second press runs it across again");
}

#[test]
fn a_mixed_selection_is_half_a_bullet_and_a_press_puts_the_bullet_on_every_shape() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[0], "text": "a"}));
    run_command(&mut w, "shape.set_text", json!({"id": ids[2], "text": "b"}));
    run_command(&mut w, "text.format", json!({"ids": [ids[0]], "bullet": 3}));
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], ids[2]]}));
    w.rest();
    assert_eq!(pressed(&w, Press::Bullets), Pressed::Part);
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!((look(&w, ids[0]).bullet, look(&w, ids[2]).bullet), (1, 1), "a half pressed button turns it on for both");
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!((look(&w, ids[0]).bullet, look(&w, ids[2]).bullet), (0, 0));
}

/// The characters selected in the text box that is open get the case, and the paragraph they are in gets the bullet: the box stays open and the text is
/// not rewritten.
#[test]
fn with_the_text_box_open_the_range_gets_the_case_and_its_paragraph_the_bullet() {
    let dir = TempDir::new("textfmt");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(w.state().edited_text_selection(), Some((0, 2)));
    press(&mut w, "ribbon.Home.ChangeCaseDialogClassic");
    w.rest();
    press(&mut w, "menu.TextAllCaps");
    w.rest();
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is still open");
    assert_eq!(w.state().edited_text(), Some("abcd"), "and the characters are the ones typed");
    assert_eq!(w.state().edited_text_selection(), Some((0, 2)));
    let page = page_id(&w);
    let said = |w: &mut Window, from: usize, to: usize| {
        let mut answered = w.state_mut().execute("shape.formatting", &json!({"ids": [1], "page": page, "start": from, "end": to})).unwrap();
        answered["shapes"][0].take()
    };
    assert_eq!(said(&mut w, 0, 2)["case"], "all_caps");
    assert_eq!(said(&mut w, 0, 4)["case"], "mixed", "the rest of the label is as it was");
    assert_eq!(pressed(&w, Press::AllCaps), Pressed::On, "the button shows the characters selected");
    press(&mut w, "ribbon.Home.Bullets.main");
    w.rest();
    assert_eq!(said(&mut w, 0, 4)["bullet"], 1);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    assert_eq!(pressed(&w, Press::Bullets), Pressed::On);
    key(&mut w, Key::Escape, Modifiers::NONE);
}

#[test]
fn the_window_lists_the_keys_the_new_controls_send() {
    // The controls of this task carry the parameters of `format.text` and `format.toggle`, which the window lists with the keys they take.
    for key in ["bullet", "indent", "case", "direction", "vertical"] {
        let listed = diacraft_ui_egui::commands::find_ui_command("format.text").unwrap();
        assert!(diacraft_engine::param_keys(listed.params).unwrap().contains(&key), "format.text lists {key}");
    }
}
