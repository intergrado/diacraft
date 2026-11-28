//! Issue 2: a menu inside a menu opens beside its parent, never over it.
//!
//! A submenu opens to the right of its row. Where it does not fit there (the Line menu sits near the right edge of a window), egui's popup tries the left, and when
//! the list is taller than the room (the 23 dashes, the 46 line ends) nothing fits and it keeps its first choice and is pushed back into the window: over its own parent
//! menu. The list then covered the rows below the row the pointer was on, and the pointer could not go on to them (Home, Line, Dashes, then Arrows).
//! The side is chosen by the room each side has now, for every menu inside a menu: the ribbon's galleries and the menus of a right click.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use serde_json::json;

use diacraft_ui_egui::ribbon::spec::{CANVAS_MENU, CONNECTOR_MENU, Ctl, LANE_MENU, TabContext, visible_tabs};

use crate::editing::edit_window;
use crate::support::{Rest, Window, click, elements_named, move_to, run_command};
use crate::tall_menus::bring_into_view;

/// Windows from the least the app takes to a large one, in points.
const SIZES: [[f32; 2]; 7] = [[480.0, 320.0], [640.0, 480.0], [900.0, 600.0], [1280.0, 720.0], [1280.0, 1000.0], [1600.0, 900.0], [1920.0, 1080.0]];

/// How much of a menu's frame, a list's scroll bar and the air between them a list needs beside the width of its rows, in points.
const FRAME: f32 = 40.0;

/// The least strip of its rows a parent menu keeps clear of the list when neither side has room for the whole list, in points.
const REACHABLE: f32 = 24.0;

/// A window on a drawing whose selection is what a right click's menu is for: a shape, a connector or a lane. → the window and the rows of the menu it opens.
fn window_with(kind: &str, size: [f32; 2]) -> (Window, &'static [Ctl]) {
    let mut w = edit_window(size);
    let drop = |w: &mut Window, x: f64, y: f64| {
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": x, "y": y})).unwrap()["id"].as_u64().unwrap()
    };
    let (ids, rows) = match kind {
        "shape" => (vec![drop(&mut w, 3.0, 3.0)], CANVAS_MENU),
        "connector" => {
            let (a, b) = (drop(&mut w, 2.0, 3.0), drop(&mut w, 6.0, 3.0));
            let made = w.state_mut().execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}})).unwrap();
            (vec![made["id"].as_u64().unwrap()], CONNECTOR_MENU)
        }
        _ => {
            w.state_mut().execute("swimlane.create", &json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 2, "phases": 2})).unwrap();
            let info = w.state_mut().execute("swimlane.info", &json!({})).unwrap();
            (vec![info["structures"][0]["lanes"][0]["id"].as_u64().unwrap()], LANE_MENU)
        }
    };
    run_command(&mut w, "edit.select", json!({"ids": ids}));
    (w, rows)
}

/// A window on a new drawing with one rectangle on it, selected.
pub fn window_with_a_shape(size: [f32; 2]) -> Window {
    window_with("shape", size).0
}

pub fn rect_of(w: &Window, name: &str) -> Option<Rect> {
    w.state().element(name).map(|e| e.rect)
}

/// A click on the control the last frame drew under `name`, then the window comes to rest.
pub fn click_row(w: &mut Window, name: &str) {
    let at = rect_of(w, name).unwrap_or_else(|| panic!("{name} is not drawn")).center();
    click(w, at);
    w.rest();
}

/// A pointer over `at` for a few frames, then the window comes to rest.
fn hover_at(w: &mut Window, at: Pos2) {
    for _ in 0..3 {
        move_to(w, at);
    }
    w.rest();
}

/// The x range the rows of `items` span in the open menu, if any is drawn.
fn x_range(w: &Window, items: &[Ctl]) -> Option<(f32, f32)> {
    let rects: Vec<Rect> = items.iter().filter_map(|c| rect_of(w, &format!("menu.{}", c.id))).collect();
    let first = rects.first()?;
    Some(rects.iter().fold((first.left(), first.right()), |(a, b), r| (a.min(r.left()), b.max(r.right()))))
}

/// The rectangle the rows of `items` span in the open menu, if any is drawn.
fn union_rect(w: &Window, items: &[Ctl]) -> Option<Rect> {
    items.iter().filter_map(|c| rect_of(w, &format!("menu.{}", c.id))).reduce(Rect::union)
}

/// What a list beside its parent must be: whole on a side that has room for it, and where neither side has, never over all of the parent's rows.
fn assert_beside(what: &str, size: [f32; 2], parent: (f32, f32), list: (f32, f32)) {
    let need = (list.1 - list.0) + FRAME;
    let (room_right, room_left) = (size[0] - parent.1, parent.0);
    let disjoint = list.0 >= parent.1 || list.1 <= parent.0;
    if room_right >= need || room_left >= need {
        assert!(
            disjoint,
            "{what} in a window {size:?}: the list {list:?} lies over the rows of its parent {parent:?}, with {room_right} points on the right and {room_left} on the left"
        );
    } else {
        let clear = list.0 >= parent.0 + REACHABLE || list.1 + REACHABLE <= parent.1;
        assert!(
            disjoint || clear,
            "{what} in a window {size:?}: neither side has room ({room_right} right, {room_left} left) and the list {list:?} covers the rows of its parent {parent:?}"
        );
    }
}

fn menu_is_open(w: &Window) -> bool {
    !elements_named(w, "menu.").is_empty()
}

/// The ribbon's controls that hold a menu that has menus in it: the tab's label, the id of the group the control is in, and the control.
fn cascades() -> Vec<(&'static str, &'static str, &'static Ctl)> {
    let mut found = Vec::new();
    for tab in visible_tabs(TabContext::default()) {
        for group in tab.groups {
            for top in group.top_level() {
                if top.items.iter().any(|item| !item.items.is_empty()) {
                    found.push((tab.label, group.id, top));
                }
            }
        }
    }
    found
}

/// The button that opens the menu of `ctl` on the tab `tab`: the arrow of a split button, else the control.
fn opener(w: &Window, tab: &str, ctl: &Ctl) -> Option<Pos2> {
    let name = format!("ribbon.{tab}.{}", ctl.id);
    rect_of(w, &format!("{name}.menu")).or_else(|| rect_of(w, &name)).map(|r| r.center())
}

/// Opens the cascades of one menu in `w` and checks each against its parent. → how many lists were checked, and how many of them were in a row that was past the edge of the window
/// until the menu was scrolled (those rows were left out of the test).
fn check_menu(w: &mut Window, size: [f32; 2], what: &str, items: &[Ctl]) -> (usize, usize) {
    let (mut checked, mut scrolled) = (0, 0);
    let Some(parent) = x_range(w, items) else { return (0, 0) };
    for row in items.iter().filter(|item| !item.items.is_empty()) {
        let name = format!("menu.{}", row.id);
        if rect_of(w, &name).is_none() {
            continue;
        }
        // A menu taller than the window scrolls: every row of it can be brought onto the screen, and the list beside it opens from there.
        let was_off = !Rect::from_min_size(Pos2::ZERO, egui::vec2(size[0], size[1])).contains_rect(rect_of(w, &name).unwrap_or(Rect::NOTHING));
        let row_rect = bring_into_view(w, &name)
            .unwrap_or_else(|| panic!("{what}, {}: the row cannot be brought onto the screen of a window {size:?}", row.label));
        hover_at(w, row_rect.center());
        if let Some(list) = x_range(w, row.items) {
            let (parent_rect, list_rect) = (union_rect(w, items), union_rect(w, row.items));
            assert_beside(
                &format!("{what}, {} (parent rows {parent_rect:?}, list rows {list_rect:?}, row {row_rect:?})", row.label),
                size,
                parent,
                list,
            );
            checked += 1;
            scrolled += usize::from(was_off);
        }
        // Back over the parent's own rows, so the next row's list opens from a menu with none open.
        hover_at(w, Pos2::new(parent.0 + 4.0, row_rect.center().y));
    }
    (checked, scrolled)
}

/// Every menu of the ribbon that has a menu in it: its list is beside its parent, in every size of window.
#[test]
fn every_submenu_of_the_ribbon_opens_beside_its_parent_in_every_size_of_window() {
    let all = cascades();
    assert!(all.len() >= 4, "the ribbon has menus with menus in them: {}", all.len());
    let (mut checked, mut reached, mut scrolled) = (0, 0, 0);
    for size in SIZES {
        for (tab, group, ctl) in &all {
            // A window of its own for each: a menu closed by Escape leaves the next click on the same row of the ribbon to toggle it shut.
            let mut w = window_with_a_shape(size);
            run_command(&mut w, "window.tab", json!({"tab": tab}));
            // In a narrow window the group is folded into one button, and its controls are in the card the button opens.
            if opener(&w, tab, ctl).is_none()
                && let Some(fold) = rect_of(&w, &format!("ribbon.{tab}.fold.{group}"))
            {
                click(&mut w, fold.center());
                w.rest();
            }
            let Some(at) = opener(&w, tab, ctl) else { continue };
            click(&mut w, at);
            w.rest();
            if !menu_is_open(&w) {
                continue;
            }
            reached += 1;
            let (n, off) = check_menu(&mut w, size, &format!("{tab}, {}", ctl.label), ctl.items);
            checked += n;
            scrolled += off;
            assert_eq!(w.state().ui_panics(), 0);
        }
    }
    assert!(reached >= 10, "only {reached} menus of the ribbon were opened");
    assert!(checked >= 15, "only {checked} lists were looked at");
    assert!(
        scrolled >= 2,
        "only {scrolled} of the lists were in rows that were past the window's edge before the menu was scrolled: the short windows leave none out"
    );
}

/// The menus of a right click: on a shape, on a connector and on a lane, at the left edge, the right edge, the bottom and the middle of windows of each size.
#[test]
fn every_submenu_of_a_right_click_menu_opens_beside_its_parent() {
    let (mut checked, mut scrolled) = (0, 0);
    for size in SIZES {
        for kind in ["shape", "connector", "lane"] {
            for (cx, cy) in [(0.12, 0.35), (0.5, 0.5), (0.9, 0.5), (0.9, 0.9), (0.5, 0.92)] {
                let (mut w, rows) = window_with(kind, size);
                let area = rect_of(&w, "canvas").expect("the drawing area is drawn");
                let at = Pos2::new(area.left() + area.width() * cx, area.top() + area.height() * cy);
                for pressed in [true, false] {
                    for _ in 0..3 {
                        move_to(&mut w, at);
                    }
                    w.input_mut().events.push(Event::PointerButton {
                        pos: at,
                        button: PointerButton::Secondary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    });
                    w.step();
                }
                w.rest();
                assert!(menu_is_open(&w), "{kind}: the right click at {at:?} in a window {size:?} opened no menu");
                let (n, off) = check_menu(&mut w, size, &format!("right click on a {kind} at {at:?}"), rows);
                (checked, scrolled) = (checked + n, scrolled + off);
                assert_eq!(w.state().ui_panics(), 0);
            }
        }
    }
    assert!(checked >= 40, "only {checked} lists were looked at");
    assert!(
        scrolled >= 2,
        "only {scrolled} of the lists were in rows that were past the window's edge before the menu was scrolled: the short windows leave none out"
    );
}

/// Home, Line, Dashes, with the Line menu at the right edge: the list is on the left of the menu, whole, and the Arrows row below the Dashes row is not covered.
#[test]
fn the_dashes_list_leaves_the_arrows_row_uncovered_at_the_right_edge() {
    let mut w = window_with_a_shape([1280.0, 720.0]);
    let at = rect_of(&w, "ribbon.Home.LineColorGallery").expect("the Line button is drawn").center();
    click(&mut w, at);
    w.rest();
    let dashes = rect_of(&w, "menu.LinePatternGallery").expect("the Dashes row is drawn");
    let arrows = rect_of(&w, "menu.LineArrowsGallery").expect("the Arrows row is drawn");
    hover_at(&mut w, dashes.center());
    let first = rect_of(&w, "menu.LinePattern1").expect("the list is open");
    let last = rect_of(&w, "menu.LinePattern23").expect("the whole list is on the screen");
    assert!(first.right() <= dashes.left() || first.left() >= dashes.right(), "the list {first:?} is beside the Dashes row {dashes:?}");
    assert!(!first.union(last).intersects(arrows), "the list {:?} does not cover the Arrows row {arrows:?}", first.union(last));
}

/// A person moves the pointer down the Line menu from Dashes to Arrows, then to End Arrow, at the speed of a hand: the lists open beside the menu and the pointer is never held by one.
#[test]
fn a_pointer_that_goes_down_the_line_menu_at_a_hands_speed_reaches_the_arrows_and_the_end_arrow_list() {
    for size in [[1280.0, 720.0], [1920.0, 1080.0], [900.0, 600.0]] {
        let mut w = window_with_a_shape(size);
        let at = rect_of(&w, "ribbon.Home.LineColorGallery").expect("the Line button is drawn").center();
        click(&mut w, at);
        w.rest();
        let dashes = rect_of(&w, "menu.LinePatternGallery").expect("the Dashes row is drawn").center();
        let arrows = rect_of(&w, "menu.LineArrowsGallery").expect("the Arrows row is drawn").center();
        hover_at(&mut w, dashes);
        assert!(rect_of(&w, "menu.LinePattern1").is_some(), "{size:?}: the Dashes list is open");
        // From the Dashes row to the Arrows row in 24 frames of 1/60 s: 0.4 s.
        for step in 1..=24 {
            let t = step as f32 / 24.0;
            move_to(&mut w, dashes + (arrows - dashes) * t);
        }
        hover_at(&mut w, arrows);
        assert!(
            rect_of(&w, "menu.LinePattern1").is_none(),
            "{size:?}: the Dashes list closed when the pointer left its row; the pointer is at {arrows:?}, the rows drawn: {:?}",
            elements_named(&w, "menu.").iter().take(10).map(|(n, r, _)| format!("{n} {r:?}")).collect::<Vec<_>>()
        );
        let end = rect_of(&w, "menu.EndArrowMenu").unwrap_or_else(|| panic!("{size:?}: the Arrows menu is open, with its End Arrow row"));
        let begin = rect_of(&w, "menu.BeginArrowMenu").expect("and its Begin Arrow row");
        // On to End Arrow, 20 frames, down the Arrows list's rows.
        let from = begin.center();
        for step in 1..=20 {
            let t = step as f32 / 20.0;
            move_to(&mut w, from + (end.center() - from) * t);
        }
        hover_at(&mut w, end.center());
        assert!(rect_of(&w, "menu.EndArrow1").is_some(), "{size:?}: the End Arrow list opened");
        assert!(
            rect_of(&w, "menu.EndArrow45").is_some() || rect_of(&w, "menu.EndArrow20").is_some(),
            "{size:?}: the End Arrow list is on the screen"
        );
        assert_eq!(w.state().ui_panics(), 0);
    }
}
