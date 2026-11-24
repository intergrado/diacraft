//! The Home tab: the Format Painter, Fill Options and Line Options, and the line ends the arrow menus offer.
//!
//! Each control is driven as a person would drive it, by the pointer and the keys, and what is checked is the document the commands changed, or the window state the
//! commands report (`window_state`). Nothing here is read from another application.

use diacraft_engine::formatting::{ShapeLook, look_of};
use diacraft_ui_egui::ribbon::spec::{self, Ctl};
use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::editing::{click_at, click_with, edit_window, key, three_shapes};
use crate::support::{Rest, Window, click, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true };

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn look(w: &Window, id: u32) -> ShapeLook {
    look_of(w.state().session().document().unwrap(), page_id(w), &[id]).unwrap().remove(0)
}

/// Lets the pointer rest for a while, so that the next click is not a double click of the last one (0.3 s apart at most, in frames of 1/60 s).
fn pause(w: &mut Window) {
    for _ in 0..40 {
        w.step();
    }
}

/// A click on the control called `name` in the last frame's list, then the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn painter(w: &Window) -> Value {
    w.state().window_state()["formatPainter"].clone()
}

/// Three shapes (a rectangle at (3, 6), a diamond at (7, 6) and an ellipse at (5, 3)), and the ellipse, which is the one selected, made unlike the others in
/// its fill, its line (colour, weight, pattern, both ends and rounding) and its text (size, style, colour and alignment).
fn styled() -> (Window, Vec<u32>) {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    let (a, b) = (
        json!({"ids": [ids[2]], "fill": "#ff0000", "line": "#0000ff", "weight": 3.0, "pattern": 4, "begin_arrow": 10, "end_arrow": 13, "rounding": 0.1}),
        json!({"ids": [ids[2]], "size": 20.0, "bold": true, "italic": true, "color": "#00ff00", "align": "right", "valign": "bottom"}),
    );
    w.state_mut().execute("shape.format", &a).unwrap();
    w.state_mut().execute("text.format", &b).unwrap();
    w.rest();
    (w, ids)
}

// ---- Format Painter --------------------------------------------------------------------------------------------------------------------------

/// A press on Format Painter copies the shape selected, and the next click on a shape paints it with the fill, the line and the text of the first; the painter is
/// then off, the paint is one undo step, and a second click paints nothing.
#[test]
fn the_format_painter_copies_the_selected_shape_and_paints_the_next_one_clicked() {
    let (mut w, ids) = styled();
    assert!(painter(&w).is_null(), "it starts off");
    let revision = w.state().session().revision();
    hit(&mut w, "ribbon.Home.FormatPainter");
    assert_eq!(painter(&w)["keep"], false, "on, for one use");
    assert_eq!(painter(&w)["from"], ids[2]);
    assert_eq!(w.state().session().revision(), revision, "copying changes nothing in the drawing");
    let before = look(&w, ids[0]);
    click_at(&mut w, 3.0, 6.0);
    let (source, painted) = (look(&w, ids[2]), look(&w, ids[0]));
    assert_ne!(before.fill, painted.fill, "the rectangle took a fill");
    assert_eq!((painted.fill, painted.line), (source.fill, source.line));
    assert_eq!(
        (painted.weight, painted.pattern, painted.begin_arrow, painted.end_arrow),
        (source.weight, source.pattern, source.begin_arrow, source.end_arrow)
    );
    assert!((painted.rounding - source.rounding).abs() < 1e-9, "{} {}", painted.rounding, source.rounding);
    assert_eq!((painted.size, painted.bold, painted.italic, painted.color), (source.size, source.bold, source.italic, source.color));
    assert_eq!((painted.align, painted.valign), (source.align, source.valign));
    assert!(w.state().session().revision() > revision, "the paint changed the drawing");
    assert!(painter(&w).is_null(), "the painter is off after one use");
    assert_eq!(w.state().selection(), &ids[2..], "the selection is where it was");
    // The diamond is not painted by a second click: the painter is off, and a click selects.
    let diamond = look(&w, ids[1]);
    click_at(&mut w, 7.0, 6.0);
    assert_eq!(look(&w, ids[1]), diamond);
    assert_eq!(w.state().selection(), &ids[1..2]);
    // One undo takes the whole paint back, fill and text together: it is one undo step.
    run_command(&mut w, "edit.undo", json!({}));
    let back = look(&w, ids[0]);
    assert_eq!((back.fill, back.line, back.bold, back.size), (before.fill, before.line, before.bold, before.size));
}

/// A double click on the button keeps the painter on, and it paints every shape clicked until Escape, which leaves the drawing as it was.
#[test]
fn a_double_click_keeps_the_format_painter_on_until_escape() {
    let (mut w, ids) = styled();
    let at = w.state().element("ribbon.Home.FormatPainter").unwrap().rect.center();
    click(&mut w, at);
    click(&mut w, at);
    w.rest();
    assert_eq!(painter(&w)["keep"], true, "{}", painter(&w));
    click_at(&mut w, 3.0, 6.0);
    click_at(&mut w, 7.0, 6.0);
    let fill = look(&w, ids[2]).fill;
    assert_eq!((look(&w, ids[0]).fill, look(&w, ids[1]).fill), (fill, fill), "both were painted");
    assert_eq!(painter(&w)["keep"], true, "and the painter is still on");
    // A press on the bare page paints nothing and starts no rubber band; it leaves the painter on.
    let revision = w.state().session().revision();
    click_at(&mut w, 9.5, 9.5);
    assert_eq!(w.state().session().revision(), revision);
    assert!(!painter(&w).is_null());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(painter(&w).is_null(), "Escape puts it down");
    assert_eq!(w.state().selection(), &ids[2..], "and the selection stays, as Escape put only the painter down");
    assert_eq!(w.state().session().revision(), revision);
}

/// Escape before the click cancels the painter: the click that follows selects and paints nothing.
#[test]
fn escape_cancels_the_format_painter_before_the_click() {
    let (mut w, ids) = styled();
    hit(&mut w, "ribbon.Home.FormatPainter");
    assert!(!painter(&w).is_null(), "the painter is on");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(painter(&w).is_null());
    let (revision, before) = (w.state().session().revision(), look(&w, ids[0]));
    click_at(&mut w, 3.0, 6.0);
    assert_eq!(w.state().session().revision(), revision);
    assert_eq!(look(&w, ids[0]), before);
    assert_eq!(w.state().selection(), &ids[..1]);
}

/// The button is a toggle (a second press puts it down), choosing a tool puts it down, and Ctrl+Shift+P is the key.
#[test]
fn the_format_painter_is_toggled_by_its_button_by_ctrl_shift_p_and_by_choosing_a_tool() {
    let (mut w, _) = styled();
    hit(&mut w, "ribbon.Home.FormatPainter");
    assert!(!painter(&w).is_null());
    pause(&mut w);
    hit(&mut w, "ribbon.Home.FormatPainter");
    assert!(painter(&w).is_null(), "a second press puts it down");
    key(&mut w, Key::P, CTRL_SHIFT);
    assert!(!painter(&w).is_null(), "Ctrl+Shift+P picks it up");
    key(&mut w, Key::P, CTRL_SHIFT);
    assert!(painter(&w).is_null(), "and puts it down");
    hit(&mut w, "ribbon.Home.FormatPainter");
    hit(&mut w, "ribbon.Home.ConnectorTool");
    assert!(painter(&w).is_null(), "a tool is a mode, so choosing one ends the painter");
    assert_eq!(w.state().tool().name(), "connector");
    hit(&mut w, "ribbon.Home.PointerTool");
    hit(&mut w, "ribbon.Home.FormatPainter");
    hit(&mut w, "ribbon.Home.PointerTool");
    assert!(painter(&w).is_null(), "the Pointer Tool is the way back");
}

/// With nothing selected the button is quiet and says why; a command run anyway is refused in the same words; the painter that is on can always be put down.
#[test]
fn the_format_painter_is_quiet_with_nothing_selected_and_says_why() {
    let mut w = edit_window(WIDE);
    three_shapes(&mut w);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    assert!(!w.state().element("ribbon.Home.FormatPainter").unwrap().enabled);
    assert_eq!(w.state().command_enabled("format.painter"), Err("nothing is selected".to_owned()));
    let refused = w.state_mut().execute("format.painter", &json!({})).unwrap_err();
    assert!(refused.to_string().contains("nothing is selected"), "{refused}");
    // Painting names a painter that is on.
    let refused = w.state_mut().execute("format.painter", &json!({"to": [1]}));
    assert!(refused.is_err());
    // Select a shape: it works, and with the painter on, the selection may go and the button still puts it down.
    click_at(&mut w, 3.0, 6.0);
    assert!(w.state().element("ribbon.Home.FormatPainter").unwrap().enabled);
    hit(&mut w, "ribbon.Home.FormatPainter");
    w.state_mut().execute("edit.select", &json!({"ids": []})).unwrap();
    w.rest();
    assert!(w.state().element("ribbon.Home.FormatPainter").unwrap().enabled, "it can be put down");
    hit(&mut w, "ribbon.Home.FormatPainter");
    assert!(painter(&w).is_null());
}

/// Two shapes selected: the first is the one copied and the others are painted at once, as the page describes (select the source, Ctrl-click the shapes to change, then
/// Format Painter); no mode is turned on.
#[test]
fn with_two_shapes_selected_the_first_is_copied_and_the_others_are_painted_at_once() {
    let (mut w, ids) = styled();
    click_at(&mut w, 5.0, 3.0);
    click_with(&mut w, (3.0, 6.0), Modifiers::SHIFT);
    click_with(&mut w, (7.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), &[ids[2], ids[0], ids[1]], "the ellipse was chosen first");
    let before = (look(&w, ids[0]), look(&w, ids[1]));
    hit(&mut w, "ribbon.Home.FormatPainter");
    let fill = look(&w, ids[2]).fill;
    assert_eq!((look(&w, ids[0]).fill, look(&w, ids[1]).fill), (fill, fill));
    assert!(painter(&w).is_null(), "and no painter is left on");
    // One undo takes both back.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!((look(&w, ids[0]), look(&w, ids[1])), before);
}

/// A shape in a group cannot be copied from, and a click on a group paints the whole group.
#[test]
fn a_member_of_a_group_is_not_copied_from_and_a_click_on_a_group_paints_all_of_it() {
    let (mut w, ids) = styled();
    click_at(&mut w, 3.0, 6.0);
    click_with(&mut w, (7.0, 6.0), Modifiers::SHIFT);
    run_command(&mut w, "arrange.group", json!({}));
    // A member selected by the command is a shape that is in a group.
    run_command(&mut w, "edit.select", json!({"ids": [ids[0]]}));
    let refused = w.state_mut().execute("format.painter", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("in a group"), "{refused}");
    assert!(painter(&w).is_null());
    // The ellipse is copied, and a click on either member paints the group: both members take the look.
    click_at(&mut w, 5.0, 3.0);
    hit(&mut w, "ribbon.Home.FormatPainter");
    click_at(&mut w, 7.0, 6.0);
    let fill = look(&w, ids[2]).fill;
    assert_eq!((look(&w, ids[0]).fill, look(&w, ids[1]).fill), (fill, fill), "every shape of the group");
}

/// The control channel's way in: `format.painter` with `to` paints those shapes as a click would, and the window state reports the painter.
#[test]
fn format_painter_is_a_command_the_control_channel_reaches() {
    let (mut w, ids) = styled();
    run_command(&mut w, "format.painter", json!({"on": true}));
    assert_eq!(painter(&w)["from"], ids[2]);
    run_command(&mut w, "format.painter", json!({"to": [ids[1]]}));
    assert_eq!(look(&w, ids[1]).fill, look(&w, ids[2]).fill);
    assert!(painter(&w).is_null(), "used up");
    // `keep` keeps it, `on: false` puts it down, and a word that is not a parameter is refused.
    run_command(&mut w, "format.painter", json!({"on": true, "keep": true}));
    run_command(&mut w, "format.painter", json!({"to": [ids[0]]}));
    assert_eq!(painter(&w)["keep"], true);
    run_command(&mut w, "format.painter", json!({"on": false}));
    assert!(painter(&w).is_null());
    let refused = w.state_mut().execute("format.painter", &json!({"bogus": 1})).unwrap_err().to_string();
    assert!(refused.contains("unknown parameter `bogus`"), "{refused}");
    let refused = w.state_mut().execute("format.painter", &json!({"to": [ids[0]]})).unwrap_err().to_string();
    assert!(refused.contains("not on"), "{refused}");
}

// ---- Fill Options, Line Options, More Arrows ---------------------------------------------------------------------------------------------------

fn pane_open(w: &Window, pane: &str) -> bool {
    w.state().window_state()["panes"].to_string().contains(pane)
}

/// Fill Options... and Line Options... open the Format Shape pane on Fill & Line, also when it was on its Effects tab.
#[test]
fn fill_options_and_line_options_open_the_format_shape_pane() {
    let mut w = edit_window(WIDE);
    three_shapes(&mut w);
    assert!(!pane_open(&w, "format_shape"));
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "menu.FillDialog");
    assert!(pane_open(&w, "format_shape"), "{}", w.state().window_state()["panes"]);
    assert!(w.state().element("pane.format.tab.fill_line").is_some());
    // Close it, and the Line menu does the same.
    run_command(&mut w, "window.pane", json!({"pane": "format_shape", "shown": false}));
    assert!(!pane_open(&w, "format_shape"));
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineDialog");
    assert!(pane_open(&w, "format_shape"));
    // On the Effects tab, Fill Options brings Fill & Line back.
    hit(&mut w, "pane.format.tab.effects");
    assert!(w.state().element("pane.format.effects.0").is_some(), "the Effects tab is on view");
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "menu.FillDialog");
    assert!(w.state().element("pane.format.fill.none").is_some(), "Fill & Line is on view again");
    assert!(w.state().element("pane.format.effects.0").is_none());
}

/// The rows act on the selection: with nothing selected the Fill and Line buttons stay quiet (their option rows do not wake them), and the command says why.
#[test]
fn the_option_rows_wait_for_a_selection() {
    let mut w = edit_window(WIDE);
    assert!(!w.state().element("ribbon.Home.FillColorGallery").unwrap().enabled);
    assert!(!w.state().element("ribbon.Home.LineColorGallery").unwrap().enabled);
    assert_eq!(w.state().command_enabled("format.options"), Err("nothing is selected".to_owned()));
    let refused = w.state_mut().execute("format.options", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("nothing is selected"), "{refused}");
    three_shapes(&mut w);
    assert!(w.state().command_enabled("format.options").is_ok());
    let refused = w.state_mut().execute("format.options", &json!({"tab": "Nowhere"})).unwrap_err().to_string();
    assert!(refused.contains("no tab `Nowhere`") && refused.contains("Fill & Line"), "{refused}");
    run_command(&mut w, "format.options", json!({"tab": "effects"}));
    assert!(pane_open(&w, "format_shape") && w.state().element("pane.format.effects.0").is_some());
}

/// The Arrows gallery ends in More Arrows..., which opens the Format Shape pane where every end can be set.
#[test]
fn more_arrows_opens_the_format_shape_pane() {
    let mut w = edit_window(WIDE);
    three_shapes(&mut w);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineArrowsGallery");
    hit(&mut w, "menu.ArrowsMore");
    assert!(pane_open(&w, "format_shape"));
    assert!(w.state().element("pane.format.line.end_arrow").is_some(), "the pane has the arrow boxes");
}

// ---- line ends -------------------------------------------------------------------------------------------------------------------------------

/// The items of the Home tab's menu control `id` (BeginArrowMenu, EndArrowMenu), as the ribbon's table holds them.
fn menu_items(id: &str) -> Vec<Ctl> {
    let mut found: Option<Ctl> = None;
    for group in spec::tab_by_label("Home").unwrap().groups {
        group.walk(&mut |c| {
            if c.id == id {
                found = Some(*c);
            }
        });
    }
    found.unwrap_or_else(|| panic!("no control {id}")).items.to_vec()
}

/// The begin arrow menu and the end arrow menu offer every end `format.shape` takes, 0 to 45, in order, each with its own caption, and each item sends its number.
#[test]
fn the_arrow_menus_offer_every_line_end_from_0_to_45() {
    for (items, key, side) in [(menu_items("BeginArrowMenu"), "begin_arrow", "Begin"), (menu_items("EndArrowMenu"), "end_arrow", "End")] {
        assert_eq!(items.len(), 46);
        let mut captions = std::collections::HashSet::new();
        for (n, item) in items.iter().enumerate() {
            assert_eq!(item.id, format!("{side}Arrow{n}"));
            assert_eq!(item.command, Some("format.shape"));
            assert_eq!(serde_json::from_str::<Value>(item.args).unwrap(), json!({ key: n }), "{}", item.id);
            assert!(captions.insert(item.label), "{} shares its caption {:?}", item.id, item.label);
            assert!(matches!(item.sample, spec::Sample::Arrow { number, begin } if usize::from(number) == n && begin == (side == "Begin")));
        }
    }
}

/// Every item of both menus changes the line end of a shape selected, to its own number: 92 presses of the commands the rows hold.
#[test]
fn every_line_end_of_the_menus_sets_its_number_on_the_selected_shape() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    for (items, key) in [(menu_items("BeginArrowMenu"), "begin_arrow"), (menu_items("EndArrowMenu"), "end_arrow")] {
        for (n, item) in items.iter().enumerate() {
            let params: Value = serde_json::from_str(item.args).unwrap();
            w.state_mut().execute(item.command.unwrap(), &params).unwrap_or_else(|e| panic!("{}: {e}", item.id));
            let seen = look(&w, ids[2]);
            let got = if key == "begin_arrow" { seen.begin_arrow } else { seen.end_arrow };
            assert_eq!(usize::try_from(got).unwrap(), n, "{}", item.id);
        }
    }
}

/// The menu itself, drawn: the End Arrow submenu lists the 46 rows (the rows past the first screenful scroll), a click on one sets the end, and the row of the end
/// the shape has is the one marked.
#[test]
fn the_end_arrow_menu_lists_all_the_ends_and_a_click_sets_one() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineArrowsGallery");
    hit(&mut w, "menu.EndArrowMenu");
    for n in 0..=45 {
        let row = w.state().element(&format!("menu.EndArrow{n}")).unwrap_or_else(|| panic!("EndArrow{n} is not listed"));
        assert!(row.enabled, "EndArrow{n}");
    }
    hit(&mut w, "menu.EndArrow3");
    assert_eq!(look(&w, ids[2]).end_arrow, 3);
    assert_eq!(look(&w, ids[2]).begin_arrow, 0, "the begin end is untouched");
    assert_eq!(w.state().notice(), None, "{:?}", w.state().notice());
}

/// The menu is longer than the screen it opens in: the wheel scrolls it, and a row far down the list (the last, 45) is reached and sets its end.
#[test]
fn a_line_end_far_down_the_list_is_reached_by_scrolling_the_menu() {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineArrowsGallery");
    hit(&mut w, "menu.EndArrowMenu");
    let top = w.state().element("menu.EndArrow0").unwrap().rect;
    let last_before = w.state().element("menu.EndArrow45").unwrap().rect;
    // The list is as tall as the window lets it be, and the 46 ends are more than a window of 900 points holds.
    let screen = w.ctx.content_rect();
    assert!(last_before.max.y > screen.max.y, "the last row lies below the window: {last_before:?} in {screen:?}");
    // The wheel over the list.
    let over = top.center();
    crate::support::move_to(&mut w, over);
    for _ in 0..30 {
        w.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        });
        w.step();
    }
    let last = w.state().element("menu.EndArrow45").unwrap().rect;
    assert!(screen.contains_rect(last) && last.min.y >= top.min.y, "the last row is in view: {last:?} under {top:?} in {screen:?}");
    // (Not `hit`: the pointer is over a row whose tooltip is still on its way, which asks for frames until it shows.)
    press(&mut w, "menu.EndArrow45");
    assert_eq!(look(&w, ids[2]).end_arrow, 45);
}
