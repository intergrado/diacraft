//! Issue 1: a menu taller than the window scrolls inside it, and the arrow keys reach every row of it.
//!
//! The Line menu with its colours is 536 points tall, and the least window the app takes is 320 points tall. egui moves a popup into the window only as far as the
//! window's edge: a menu taller than the window had its top at the top of the window and its last rows (Arrows, Line Options...) below its bottom edge, out of reach of
//! the pointer, and the arrow keys moved the focus to rows no one could see. A list inside a menu already scrolled but did not follow the focus.

use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, Order, Pos2, Rect, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use diacraft_ui_egui::ribbon::spec::{Body, Ctl, Kind, TabContext, visible_tabs};

use crate::editing::key;
use crate::submenu_sides::{click_row, rect_of, window_with_a_shape};
use crate::support::{Rest, Window, elements_named, move_to, run_command};

/// Windows too short for the Line menu, from the least the app takes: the size is the height that matters, in points.
const SHORT: [[f32; 2]; 4] = [[480.0, 320.0], [900.0, 420.0], [1280.0, 500.0], [640.0, 480.0]];

/// The ribbon's control with the id `id`: the label of its tab, the id of its group, and the control.
fn control(id: &str) -> (&'static str, &'static str, &'static Ctl) {
    for tab in visible_tabs(TabContext::default()) {
        for group in tab.groups {
            for top in group.top_level() {
                if top.id == id {
                    return (tab.label, group.id, top);
                }
            }
        }
    }
    panic!("the ribbon has no control {id}");
}

/// A window of `size` on a drawing with a shape selected, with the menu of the ribbon's control `id` open. In a narrow window the group is folded into one button, and the
/// control is in the card that button opens.
fn open_menu(id: &str, size: [f32; 2]) -> Window {
    let mut w = window_with_a_shape(size);
    let (tab, group, _) = control(id);
    run_command(&mut w, "window.tab", json!({"tab": tab}));
    let name = format!("ribbon.{tab}.{id}");
    if rect_of(&w, &name).is_none() {
        click_row(&mut w, &format!("ribbon.{tab}.fold.{group}"));
    }
    click_row(&mut w, &name);
    w
}

/// The rectangle of each menu or list that is open: the areas of the foreground layer, which is where a popup is.
fn popups(w: &Window) -> Vec<Rect> {
    w.ctx.memory(|m| m.areas().visible_layer_ids().into_iter().filter(|l| l.order == Order::Foreground).filter_map(|l| m.area_rect(l.id)).collect())
}

/// The window's own rectangle.
fn window_rect(w: &Window) -> Rect {
    w.ctx.content_rect()
}

/// An arrow-down key press and release, then the window comes to rest.
fn down(w: &mut Window) {
    key(w, Key::ArrowDown, Modifiers::NONE);
}

/// The label and the place of the widget that has the keyboard focus, if any: the accessibility tree's own answer.
fn focus(w: &Window) -> Option<(String, Rect)> {
    use egui_kittest::kittest::{By, NodeT};
    w.query_all(By::new().predicate(|node| node.is_focused()))
        .next()
        .map(|node| (node.accesskit_node().label().unwrap_or_default().to_owned(), node.rect()))
}

/// The row of `rows` that `label` names.
fn row_named<'a>(rows: &'a [Ctl], label: &str) -> Option<&'a Ctl> {
    rows.iter().find(|row| row.label == label)
}

/// The wheel turned over `at`, down the menu by `by` points (up, for a number below 0).
fn wheel(w: &mut Window, at: Pos2, by: f32) {
    move_to(w, at);
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -by),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    for _ in 0..30 {
        w.step();
    }
}

/// The room a menu's row has in the open menu: inside the menu's own rectangle, less the frame, and so in the window as well.
fn on_the_screen(w: &Window, row: Rect) -> bool {
    popups(w).iter().any(|menu| menu.shrink(3.0).contains_rect(row)) && window_rect(w).contains_rect(row)
}

/// Brings the row the last frame drew under `name` whole onto the screen by turning the wheel over its menu: a menu taller than the window scrolls. → the row's place, or `None` when the
/// row is not drawn or the wheel could not bring it.
pub fn bring_into_view(w: &mut Window, name: &str) -> Option<Rect> {
    for _ in 0..40 {
        let row = rect_of(w, name)?;
        if on_the_screen(w, row) {
            return Some(row);
        }
        // The menu the row is in is the one whose width holds the row's middle: the wheel over it moves the rows under the pointer, down for a row below the menu, up for one above.
        let menu = popups(w).into_iter().find(|menu| menu.x_range().contains(row.center().x))?;
        wheel(w, menu.center(), if row.bottom() > menu.bottom() { 120.0 } else { -120.0 });
    }
    None
}

/// **The Line menu is inside the window whatever the window's height**: its rectangle is in the window, and a menu that fits is not cut (the room it was given is
/// the window's, less a margin). Before a change the menu was as tall as its rows in a window of any height and its bottom was past the window's.
#[test]
fn a_menu_taller_than_the_window_is_held_inside_the_window() {
    for size in SHORT {
        for id in ["LineColorGallery", "FillColorGallery"] {
            let w = open_menu(id, size);
            let (menus, screen) = (popups(&w), window_rect(&w));
            assert!(!menus.is_empty(), "{id} in a window {size:?}: the menu opened");
            for menu in menus {
                assert!(
                    screen.contains_rect(menu.shrink(0.5)),
                    "{id} in a window {size:?}: the menu {menu:?} is not inside the window {screen:?}: its last rows cannot be reached"
                );
            }
            assert_eq!(w.state().ui_panics(), 0);
        }
    }
}

/// **Every menu of the ribbon is held inside a short window**, whatever it holds: colours, a gallery of tiles, rows, a list of page sizes. Each control of the ribbon that opens a menu is opened
/// in the least window the app takes and in one of 420 points, and every menu and card that is open is inside the window. (A menu that is cut by the window's edge has rows nobody reaches.)
#[test]
fn every_menu_of_the_ribbon_is_held_inside_a_short_window() {
    let mut opened = 0;
    for size in [[480.0, 320.0], [1100.0, 420.0]] {
        for tab in visible_tabs(TabContext::default()) {
            for group in tab.groups {
                for ctl in group.top_level() {
                    let holds_a_menu = ctl.palette.is_some() || !ctl.items.is_empty() || ctl.body != Body::None;
                    if !holds_a_menu || !matches!(ctl.kind, Kind::Menu | Kind::Split | Kind::Gallery) {
                        continue;
                    }
                    let mut w = window_with_a_shape(size);
                    run_command(&mut w, "window.tab", json!({"tab": tab.label}));
                    let name = format!("ribbon.{}.{}", tab.label, ctl.id);
                    if rect_of(&w, &name).is_none()
                        && let Some(fold) = rect_of(&w, &format!("ribbon.{}.fold.{}", tab.label, group.id))
                    {
                        crate::support::click(&mut w, fold.center());
                        w.rest();
                    }
                    let Some(at) = rect_of(&w, &format!("{name}.menu")).or_else(|| rect_of(&w, &name)).map(|r| r.center()) else { continue };
                    crate::support::click(&mut w, at);
                    w.rest();
                    let (menus, screen) = (popups(&w), window_rect(&w));
                    if elements_named(&w, "menu.").is_empty() && menus.is_empty() {
                        continue;
                    }
                    opened += 1;
                    for menu in menus {
                        assert!(
                            screen.contains_rect(menu.shrink(0.5)),
                            "{} on the {} tab in a window {size:?}: the menu {menu:?} is not inside the window {screen:?}",
                            ctl.label,
                            tab.label
                        );
                    }
                    assert_eq!(w.state().ui_panics(), 0);
                }
            }
        }
    }
    assert!(opened >= 30, "only {opened} menus of the ribbon were opened");
}

/// **The arrow keys reach every row of a tall menu**: pressing the down arrow moves the focus down the Line menu (its colours first, one at a time, then its five rows), and the widget
/// that has the focus is on the screen each time: the menu scrolls to it. The menu is open in windows from 500 points tall down to the least the app takes, where Arrows and Line Options...
/// were below the window's edge.
#[test]
fn the_down_arrow_brings_every_row_of_a_tall_menu_on_the_screen() {
    for size in SHORT {
        let (_, _, line) = control("LineColorGallery");
        let mut w = open_menu("LineColorGallery", size);
        let mut reached: Vec<&str> = Vec::new();
        for press in 1..=200 {
            down(&mut w);
            let Some((label, rect)) = focus(&w) else { continue };
            assert!(
                on_the_screen(&w, rect),
                "{size:?}: after {press} presses {label} has the focus and is at {rect:?}, off the screen (menus {:?})",
                popups(&w)
            );
            if let Some(row) = row_named(line.items, &label)
                && !reached.contains(&row.id)
            {
                reached.push(row.id);
            }
            if reached.len() == line.items.len() {
                break;
            }
        }
        let every: Vec<&str> = line.items.iter().map(|row| row.id).collect();
        assert_eq!(reached, every, "{size:?}: the keys reached every row, in order");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// **A list inside a menu follows the focus as well**: with Dashes open in a window 500 points tall, the down arrow takes the focus down its 23 rows and each is on the
/// screen when it has it.
#[test]
fn the_down_arrow_brings_every_row_of_a_list_in_a_menu_on_the_screen() {
    for size in [[1280.0, 500.0], [480.0, 320.0]] {
        let (_, _, line) = control("LineColorGallery");
        let dashes = line.items.iter().find(|row| row.id == "LinePatternGallery").expect("the Dashes row");
        let mut w = open_menu("LineColorGallery", size);
        // The focus on the Dashes row, and its list open by Enter (the arrow that opens a list is the one on the side it opens on: here the left).
        w.get_by_label(dashes.label).focus();
        w.step();
        key(&mut w, Key::Enter, Modifiers::NONE);
        assert!(rect_of(&w, "menu.LinePattern1").is_some(), "{size:?}: the list opened by the key");
        let mut reached: Vec<&str> = Vec::new();
        // The focus is on the first row as the list opens, and each press takes it one row down.
        for press in 0..=60 {
            if press > 0 {
                down(&mut w);
            }
            let Some((label, rect)) = focus(&w) else { continue };
            if let Some(row) = row_named(dashes.items, &label) {
                assert!(
                    on_the_screen(&w, rect),
                    "{size:?}: after {press} presses {label} has the focus and is at {rect:?}, off the screen (menus {:?})",
                    popups(&w)
                );
                if !reached.contains(&row.id) {
                    reached.push(row.id);
                }
            }
        }
        let every: Vec<&str> = dashes.items.iter().map(|row| row.id).collect();
        assert_eq!(reached, every, "{size:?}: the keys reached the last of the {} dashes, in order", dashes.items.len());
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// The wheel brings the last row of a tall menu on the screen too (a person with a mouse, and no keys).
#[test]
fn the_wheel_brings_the_last_row_of_a_tall_menu_on_the_screen() {
    let mut w = open_menu("LineColorGallery", [1280.0, 500.0]);
    let (_, _, line) = control("LineColorGallery");
    let first = rect_of(&w, &format!("menu.{}", line.items[0].id)).expect("the first row is drawn");
    wheel(&mut w, first.center(), 2000.0);
    let last = line.items.last().expect("rows");
    let rect = rect_of(&w, &format!("menu.{}", last.id)).expect("the last row is drawn");
    assert!(on_the_screen(&w, rect), "the last row {} came to {rect:?} (menus {:?})", last.label, popups(&w));
}

/// **The list of all the pages scrolls too**: a drawing of 40 pages has a list taller than the least window, which is held inside the window, and the down arrow takes the focus down every
/// row of it, the last page's included, each on the screen when it has the focus.
#[test]
fn the_list_of_all_pages_is_held_inside_a_short_window_and_the_keys_follow_it_to_the_last_page() {
    let mut w = window_with_a_shape([480.0, 320.0]);
    for n in 2..=40 {
        run_command(&mut w, "page.add", json!({"name": format!("Sheet {n}")}));
    }
    run_command(&mut w, "page.select", json!({"id": 1}));
    click_row(&mut w, "page-tabs.all");
    let (menus, screen) = (popups(&w), window_rect(&w));
    assert!(!menus.is_empty() && menus.iter().all(|m| screen.contains_rect(m.shrink(0.5))), "the list {menus:?} is inside the window {screen:?}");
    let last = w.state().session().document().unwrap().pages.iter().next_back().map(|p| p.name.to_string()).expect("pages");
    let mut reached = false;
    for press in 1..=45 {
        down(&mut w);
        let Some((label, rect)) = focus(&w) else { continue };
        if label.starts_with("Sheet ") || label == "Page-1" {
            assert!(
                on_the_screen(&w, rect),
                "after {press} presses {label} has the focus and is at {rect:?}, off the screen (menus {:?})",
                popups(&w)
            );
            reached |= label == last;
        }
    }
    assert!(reached, "the keys reached {last}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Hands the queued input to the window a frame at a time, as the desktop app does, until none is left.
fn pump(window: &mut Window) {
    for _ in 0..40 {
        let events = window.state_mut().next_input_frame();
        let idle = events.is_empty();
        window.input_mut().events.extend(events);
        window.step();
        if idle && window.state().pending_input() == 0 {
            break;
        }
    }
    window.rest();
}

/// **The control channel's keys do what a person's do** (the way the app is driven when it is looked at): `ui.click` opens the Line menu in a window 500 points tall, and each `ui.key`
/// `ArrowDown` takes the focus one widget down it, through the colours and on to the five rows, and the menu scrolls so that the last row, Line Options..., is on the screen when it has the
/// focus. A press more takes the focus back to the top of the menu, and the menu scrolls back.
#[test]
fn the_control_channels_arrow_keys_take_the_focus_down_a_tall_menu_and_the_menu_follows() {
    use crate::support::{call, controlled_with};
    let (mut w, tx) = controlled_with(|mut app| {
        app.execute("file.new", &json!({})).unwrap();
        app
    });
    w.set_size(vec2(1280.0, 500.0));
    w.rest();
    let made = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})).unwrap();
    w.state_mut().execute("edit.select", &json!({"ids": [made["id"]]})).unwrap();
    w.rest();
    call(&mut w, &tx, "ui.click", json!({"element": "ribbon.Home.LineColorGallery"})).unwrap();
    pump(&mut w);
    assert!(rect_of(&w, "menu.LineDialog").is_some(), "the menu is open");
    let mut labels = Vec::new();
    for _ in 0..90 {
        call(&mut w, &tx, "ui.key", json!({"key": "ArrowDown"})).unwrap();
        pump(&mut w);
        let Some((label, rect)) = focus(&w) else { continue };
        assert!(on_the_screen(&w, rect), "{label} has the focus at {rect:?}, off the screen (menus {:?})", popups(&w));
        let last = label == "Line Options...";
        labels.push(label);
        if last {
            break;
        }
    }
    assert_eq!(labels.first().map(String::as_str), Some("Use Default"), "the first key takes the focus to the top of the menu");
    assert_eq!(labels.last().map(String::as_str), Some("Line Options..."), "the keys reach the last row: {labels:?}");
    let rows: Vec<&String> = labels.iter().rev().take(5).collect();
    assert_eq!(rows, ["Line Options...", "Arrows", "Dashes", "Transparency", "Weight"], "the five rows come last, in order");
    // One key more: the focus goes round to the top, and the menu scrolls back to it.
    call(&mut w, &tx, "ui.key", json!({"key": "ArrowDown"})).unwrap();
    pump(&mut w);
    let (label, rect) = focus(&w).expect("a widget has the focus");
    assert!(label == "Use Default" && on_the_screen(&w, rect), "{label} at {rect:?}");
    assert_eq!(w.state().ui_panics(), 0);
}
