//! The Line menu's Dashes list has 23 rows, and every one can be reached. Before a change the list was cut at 330 points and scrolled with a bar
//! that showed only under the pointer: in a window 900 points tall rows 13 to 23 were hidden, and the audit found rows 19 to 23 only by the wheel
//! (section 8). Now the list is as tall as the window lets it be (all 23 rows show at 800 points and more), and in a window too short for it the
//! list scrolls with a bar that is always drawn.

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, Rect, TouchPhase, vec2};

use diacraft_engine::formatting::look_of;

use crate::editing::{edit_window, shape_ids, three_shapes};
use crate::support::{Rest, Window, click, elements_named, move_to};

/// How many dash patterns the list offers.
const DASHES: usize = 23;

/// A click on the control called `name` in the last frame's list, then the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

/// The pointer over the control called `name` for a few frames. A submenu opens under it (egui opens one on hover), and **the Line menu is taller since the palette has its sections**:
/// the Dashes list, which is taller than the room under its row, is moved up and left to fit the window and lies over the parent's row, so a click on the row would land on the list.
fn hover(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    for _ in 0..3 {
        move_to(w, at);
    }
    w.rest();
}

/// The dash pattern of the shape selected (the last of `three_shapes`).
fn pattern(w: &Window) -> u32 {
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let id = *shape_ids(w).last().unwrap();
    look_of(doc, page, &[id]).unwrap().remove(0).pattern
}

/// Opens Line, then Dashes: the rows of the list, in order, with where each is.
fn open_dashes(w: &mut Window) -> Vec<(String, Rect)> {
    hit(w, "ribbon.Home.LineColorGallery");
    hover(w, "menu.LinePatternGallery");
    let rows: Vec<(String, Rect)> = (1..=DASHES)
        .map(|n| {
            let name = format!("menu.LinePattern{n}");
            let rect = w.state().element(&name).unwrap_or_else(|| panic!("{name} is not drawn")).rect;
            (name, rect)
        })
        .collect();
    assert_eq!(elements_named(w, "menu.LinePattern").len(), DASHES + 1, "the 23 rows and the Dashes row itself");
    rows
}

fn window_rect(w: &Window) -> Rect {
    w.ctx.content_rect()
}

/// The wheel turned over `at`, down the list.
fn wheel_down(w: &mut Window, at: Pos2) {
    move_to(w, at);
    w.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -2000.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: MouseWheelSource::Unknown,
    });
    // The wheel is smoothed over a few frames.
    for _ in 0..30 {
        w.step();
    }
}

/// In a window 900 points tall, in one of 800, and in a wide one of 1,000, the whole list is on the screen, each row below the one before, and a click
/// on any row sets its pattern: the rows the audit could not reach (19 to 23) and the ones a 330-point list hid (13 on) as well.
#[test]
fn every_dash_is_on_the_screen_and_a_click_away_in_a_window_of_800_points_or_more() {
    for size in [[1600.0, 900.0], [1280.0, 800.0], [1920.0, 1000.0]] {
        let mut w = edit_window(size);
        three_shapes(&mut w);
        let mut rows = open_dashes(&mut w);
        let screen = window_rect(&w);
        for pair in rows.windows(2) {
            assert!(pair[0].1.bottom() <= pair[1].1.top(), "{size:?}: {} is above {}", pair[0].0, pair[1].0);
        }
        for (name, rect) in &rows {
            assert!(screen.contains_rect(*rect), "{size:?}: {name} at {rect:?} is not inside the window {screen:?}");
        }
        for n in [DASHES, 19, 13, 2] {
            // The list is open the first time round; a row chosen closes it, so it is opened again for the next.
            if n != DASHES {
                rows = open_dashes(&mut w);
            }
            let (name, rect) = &rows[n - 1];
            click(&mut w, rect.center());
            w.rest();
            assert_eq!(pattern(&w), u32::try_from(n).unwrap(), "{size:?}: a click on {name} sets its pattern");
        }
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// In a window too short for the list (600 points) the list scrolls: the wheel over it brings the last row on the screen, and a click on it sets it.
#[test]
fn in_a_window_too_short_for_the_list_the_wheel_brings_the_last_dash_into_reach() {
    let mut w = edit_window([1600.0, 600.0]);
    three_shapes(&mut w);
    let rows = open_dashes(&mut w);
    let screen = window_rect(&w);
    assert!(screen.contains_rect(rows[0].1), "the first row is on the screen: {:?}", rows[0].1);
    wheel_down(&mut w, rows[0].1.center());
    let last = w.state().element(&format!("menu.LinePattern{DASHES}")).expect("the list is still open").rect;
    assert!(screen.contains_rect(last), "the last row came on the screen: {last:?} in {screen:?}");
    click(&mut w, last.center());
    w.rest();
    assert_eq!(pattern(&w), u32::try_from(DASHES).unwrap());
}
