//! Formatting and arranging from the window: the Home tab's Font, Paragraph, Shape Styles and Arrange controls show the state of the
//! selection and send one command; the keys; the rotation handle, the handles of a turned shape and a second click on a group; a text box that
//! stays open while its characters are formatted. The pointer and the keyboard are given to egui as a person's would be, and what is checked is the
//! document the commands changed.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_engine::doc::TextPart;
use diacraft_engine::draw::{shape_corners, shape_frame};
use diacraft_engine::formatting::{Align, Rgb, ShapeLook, VAlign, look_of};
use diacraft_engine::measure::scan_fonts_with;
use diacraft_engine::measure::set_measure_fonts;
use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::font_list::Scan;
use diacraft_ui_egui::format::{Mix, Pressed};
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::ribbon::spec::Press;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{click_at, click_with, drag_with, edit_window, edit_window_with, frame_of, key, shape_ids, three_shapes, type_text};
use crate::fixture;
use crate::support::{Rest, TempDir, Window, click, elements_named, fast_window, move_to, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];
const CTRL: Modifiers = Modifiers::COMMAND;
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

/// The `n`th swatch of the palette of `key` (in reading order), chosen: the colour it stands for.
fn pick_swatch(w: &mut Window, key: &str, n: usize) -> Rgb {
    let mut swatches = elements_named(w, &format!("palette.{key}.standard.#"));
    swatches.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    let (name, rect, enabled) =
        swatches.get(n).cloned().unwrap_or_else(|| panic!("the palette of {key} has no swatch {n}: {} drawn", swatches.len()));
    assert!(enabled, "{name}");
    let hex = name.rsplit('.').next().unwrap().to_owned();
    click(w, rect.center());
    w.rest();
    Rgb(u8::from_str_radix(&hex[1..3], 16).unwrap(), u8::from_str_radix(&hex[3..5], 16).unwrap(), u8::from_str_radix(&hex[5..7], 16).unwrap())
}

/// A window on a new drawing wide enough for every group of the Home tab, with the three shapes of [`three_shapes`] on it and the last, the
/// ellipse, selected.
fn three() -> (Window, Vec<u32>) {
    let mut w = edit_window(WIDE);
    let ids = three_shapes(&mut w);
    (w, ids)
}

// ---- the controls follow the selection --------------------------------------------------------------------------------------------------------

/// A control that acts on the selection is quiet with nothing selected and works with a shape selected; Align wants two shapes, as it moves the
/// others to the first. The words of a quiet one say why.
#[test]
fn the_formatting_controls_work_on_a_selection_and_are_quiet_without_one() {
    let mut w = edit_window(WIDE);
    let controls = ["Bold", "Italic", "Font", "FontSize", "FontSizeIncrease", "FontColorGallery", "AlignLeft", "TextAlignTop", "FillColorGallery"];
    let arrange = ["PositionMenu", "ObjectBringToFrontMenu", "ObjectSendToBackMenu"];
    for c in controls.iter().chain(arrange.iter()).chain(["AlignGallery"].iter()) {
        assert!(!enabled(&w, c), "{c} has nothing to act on");
    }
    let ids = three_shapes(&mut w);
    assert_eq!(w.state().selection(), &ids[2..], "the shape dropped last is selected");
    for c in controls.iter().chain(arrange.iter()) {
        assert!(enabled(&w, c), "{c} acts on the shape selected");
    }
    assert!(!enabled(&w, "AlignGallery"), "one shape has nothing to align to");
    click_with(&mut w, (3.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection().len(), 2);
    assert!(enabled(&w, "AlignGallery"), "two shapes can be aligned");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!enabled(&w, "Bold") && !enabled(&w, "AlignGallery"), "and Escape leaves them nothing again");
    // Groups the controls hold are still there, and the commands that wait for others stay quiet (Group, Paste).
    assert!(!enabled(&w, "ObjectsGroupMenu"), "Group waits for its command");
    assert!(!w.state().element("ribbon.Home.PasteMenu").unwrap().enabled, "and so does Paste");
}

/// The window's commands are listed with the engine's, with the reason a quiet one gives.
#[test]
fn the_window_lists_its_format_and_arrange_commands_and_says_why_one_cannot_run() {
    let mut w = edit_window(WIDE);
    let state = w.state();
    for id in ["format.shape", "format.text", "format.toggle", "format.size", "arrange.rotate", "arrange.flip", "arrange.order", "arrange.align"] {
        assert!(diacraft_ui_egui::commands::known(id), "{id}");
        let reason = if id == "arrange.align" { "select two shapes or more: the others move to the first" } else { "nothing is selected" };
        assert_eq!(state.command_enabled(id), Err(reason.to_owned()), "{id}");
    }
    three_shapes(&mut w);
    assert!(w.state().command_enabled("format.text").is_ok());
    assert!(w.state().command_enabled("arrange.align").is_err(), "one shape selected");
    // A command that cannot run is refused with its reason, and nothing changes.
    key(&mut w, Key::Escape, Modifiers::NONE);
    let before = w.state().session().revision();
    let refused = w.state_mut().execute("format.text", &json!({"bold": true}));
    assert!(refused.is_err());
    assert_eq!(w.state().session().revision(), before);
}

// ---- fill, line and colour --------------------------------------------------------------------------------------------------------------------

/// The Fill menu opens with a palette; a swatch fills the selected shape (one undo step), No Fill takes the fill away, and the colour bar under
/// the control shows the colour the shape has.
#[test]
fn a_swatch_of_the_fill_palette_fills_the_selected_shape_and_no_fill_takes_it_away() {
    let (mut w, ids) = three();
    hit(&mut w, "ribbon.Home.FillColorGallery");
    let before = w.state().session().revision();
    let rgb = pick_swatch(&mut w, "fill", 5);
    assert_eq!(look(&w, ids[2]).fill, Some(rgb));
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    assert_eq!(w.state().fill_mix(), Mix::One(Some(rgb)), "and the control shows it");
    assert!(w.state().element("palette.fill.none").is_none(), "the menu closed with the choice");
    // The other shapes are untouched.
    assert_ne!(look(&w, ids[0]).fill, Some(rgb));
    // No Fill.
    hit(&mut w, "ribbon.Home.FillColorGallery");
    hit(&mut w, "palette.fill.none");
    assert_eq!(look(&w, ids[2]).fill, None);
    assert_eq!(w.state().fill_mix(), Mix::One(None));
    // A line colour goes the same way and a colour typed as #RRGGBB in the Colors dialog goes too.
    hit(&mut w, "ribbon.Home.LineColorGallery");
    let line = pick_swatch(&mut w, "line", 3);
    assert_eq!(look(&w, ids[2]).line, Some(line));
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "palette.line.more");
    hit(&mut w, "colors.tab.custom");
    hit(&mut w, "colors.hex");
    type_text(&mut w, "#3C78D8");
    key(&mut w, Key::Enter, Modifiers::NONE);
    hit(&mut w, "colors.ok");
    assert_eq!(look(&w, ids[2]).line, Some(Rgb(0x3c, 0x78, 0xd8)));
    // And one that is not a colour is told in words and changes nothing.
    let revision = w.state().session().revision();
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "palette.line.more");
    hit(&mut w, "colors.tab.custom");
    hit(&mut w, "colors.hex");
    key(&mut w, Key::A, Modifiers::COMMAND);
    type_text(&mut w, "blue");
    key(&mut w, Key::Enter, Modifiers::NONE);
    let said = w.state().window_state()["colorsDialog"]["error"].as_str().unwrap_or_default().to_owned();
    assert!(said.contains("#RRGGBB"), "the dialog says why, in words that name the form: {said:?}");
    assert_eq!(w.state().session().revision(), revision);
}

/// The Line menu's Weight, Dashes and Arrows each open a menu of their own, and the current value is the one marked.
#[test]
fn weight_dashes_and_line_ends_come_from_the_line_menu() {
    let (mut w, ids) = three();
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineWeightGallery");
    hit(&mut w, "menu.LineWeight225");
    assert!((look(&w, ids[2]).weight - 2.25).abs() < 1e-9, "{}", look(&w, ids[2]).weight);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hover(&mut w, "menu.LinePatternGallery");
    hit(&mut w, "menu.LinePattern4");
    assert_eq!(look(&w, ids[2]).pattern, 4);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineArrowsGallery");
    hit(&mut w, "menu.EndArrowMenu");
    hit(&mut w, "menu.EndArrow4");
    assert_eq!(look(&w, ids[2]).end_arrow, 4);
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineArrowsGallery");
    hit(&mut w, "menu.BeginArrowMenu");
    hit(&mut w, "menu.BeginArrow10");
    assert_eq!(look(&w, ids[2]).begin_arrow, 10);
    // The menus remember the choice: weight 2.25 is the one marked when the menu is opened again.
    hit(&mut w, "ribbon.Home.LineColorGallery");
    hit(&mut w, "menu.LineWeightGallery");
    assert!(w.state().element("menu.LineWeight225").unwrap().enabled);
    assert_eq!(w.state().weight_mix(), Mix::One(2250));
}

// ---- font, size, style, alignment -------------------------------------------------------------------------------------------------------------

/// Bold, Italic, Underline and Strikethrough toggle on the shape's text, show whether the selection has them, and one undo takes each back.
#[test]
fn the_style_buttons_toggle_and_show_the_state() {
    let (mut w, ids) = three();
    assert_eq!(pressed(&w, Press::Bold), Pressed::Off);
    press(&mut w, "ribbon.Home.Bold");
    w.rest();
    assert!(look(&w, ids[2]).bold);
    assert_eq!(pressed(&w, Press::Bold), Pressed::On);
    press(&mut w, "ribbon.Home.Italic");
    press(&mut w, "ribbon.Home.Underline");
    press(&mut w, "ribbon.Home.Strikethrough");
    w.rest();
    let l = look(&w, ids[2]);
    assert!(l.bold && l.italic && l.underline && l.strike);
    press(&mut w, "ribbon.Home.Bold");
    w.rest();
    assert!(!look(&w, ids[2]).bold, "a second press turns it off");
    assert_eq!(pressed(&w, Press::Bold), Pressed::Off);
    // The keys.
    key(&mut w, Key::B, CTRL);
    assert!(look(&w, ids[2]).bold, "Ctrl+B");
    key(&mut w, Key::I, CTRL);
    assert!(!look(&w, ids[2]).italic, "Ctrl+I turns italic off");
    key(&mut w, Key::U, CTRL);
    assert!(!look(&w, ids[2]).underline, "Ctrl+U");
    key(&mut w, Key::Z, CTRL);
    assert!(look(&w, ids[2]).underline, "and one undo takes back one key");
}

/// A selection whose shapes differ: a toggle is half pressed, a press turns it on for all, and the Font and Size boxes show nothing.
#[test]
fn a_mixed_selection_shows_half_a_toggle_and_a_press_turns_it_on_for_every_shape() {
    let (mut w, ids) = three();
    key(&mut w, Key::B, CTRL);
    click_with(&mut w, (3.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection().len(), 2);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Part, "one of the two is bold");
    assert_eq!(pressed(&w, Press::Italic), Pressed::Off);
    key(&mut w, Key::B, CTRL);
    assert!(look(&w, ids[0]).bold && look(&w, ids[2]).bold, "a half pressed button turns it on for both");
    assert_eq!(pressed(&w, Press::Bold), Pressed::On);
    key(&mut w, Key::B, CTRL);
    assert!(!look(&w, ids[0]).bold && !look(&w, ids[2]).bold, "and a full one turns it off for both");
    // Sizes that differ: the box shows none.
    hit(&mut w, "ribbon.Home.FontSizeIncrease");
    assert_eq!(w.state().size_mix(), Mix::One(14000), "both were 12 and both went to 14");
    key(&mut w, Key::Escape, Modifiers::NONE);
    click_at(&mut w, 3.0, 6.0);
    hit(&mut w, "ribbon.Home.FontSizeIncrease");
    click_with(&mut w, (5.0, 3.0), Modifiers::SHIFT);
    assert_eq!(w.state().size_mix(), Mix::Mixed, "16 and 14");
}

/// The size: Increase and Decrease step through the list, Ctrl+Shift+> and Ctrl+Shift+< do the same, the list sets a size and so does a
/// number typed in its box.
#[test]
fn the_font_size_steps_comes_from_the_list_or_is_typed() {
    let (mut w, ids) = three();
    // (A size the file holds as inches times 72 is a little off the number of points.)
    let size = |w: &Window| look(w, ids[2]).size;
    assert!((size(&w) - 12.0).abs() < 1e-3, "{}", size(&w));
    hit(&mut w, "ribbon.Home.FontSizeIncrease");
    assert!((size(&w) - 14.0).abs() < 1e-3);
    hit(&mut w, "ribbon.Home.FontSizeDecrease");
    hit(&mut w, "ribbon.Home.FontSizeDecrease");
    assert!((size(&w) - 11.0).abs() < 1e-3, "{}", size(&w));
    // Ctrl+Shift+> is the full stop's key with Shift; < is the comma's.
    key(&mut w, Key::Period, CTRL_SHIFT);
    assert!((size(&w) - 12.0).abs() < 1e-3, "Ctrl+Shift+>: {}", size(&w));
    key(&mut w, Key::Comma, CTRL_SHIFT);
    key(&mut w, Key::Comma, CTRL_SHIFT);
    assert!((size(&w) - 10.0).abs() < 1e-3, "Ctrl+Shift+<: {}", size(&w));
    // The list.
    hit(&mut w, "ribbon.Home.FontSize");
    hit(&mut w, "size.item.24");
    assert!((size(&w) - 24.0).abs() < 1e-3);
    assert!(w.state().element("size.item.24").is_none(), "the list closed with the choice");
    // A number typed in its box, which has the keyboard when the list opens.
    hit(&mut w, "ribbon.Home.FontSize");
    type_text(&mut w, "10.5");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!((size(&w) - 10.5).abs() < 1e-3, "{}", size(&w));
    // Not a size: told in words, nothing changes.
    let revision = w.state().session().revision();
    hit(&mut w, "ribbon.Home.FontSize");
    type_text(&mut w, "big");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(w.state().notice().unwrap().contains("points"), "{:?}", w.state().notice());
    assert_eq!(w.state().session().revision(), revision);
}

/// The families of the machine are found on a thread of their own, never in a frame: the list says it is loading, the window goes on drawing,
/// and the families are there when the scan answers. A family chosen is the shape's font.
#[test]
fn the_font_list_is_found_off_the_windows_thread_and_a_family_chosen_is_the_font() {
    let (release, held) = channel::<()>();
    let held = Mutex::new(held);
    let scan: diacraft_ui_egui::font_list::Scan = Arc::new(move || {
        // Held until the test lets it go, or for twenty seconds.
        let _ = held.lock().map(|rx| rx.recv_timeout(Duration::from_secs(20)));
        vec!["Test Serif".to_owned(), "Test Sans".to_owned(), "Test Mono".to_owned()]
    });
    let mut w = edit_window_with(WIDE, |app: DiacraftApp| app.with_font_scan(scan));
    let ids = three_shapes(&mut w);
    assert!(!w.state().fonts_scanning(), "nothing is scanned until the list is wanted");
    let font_box = w.state().element("ribbon.Home.Font").unwrap().rect.center();
    click(&mut w, font_box);
    let frames = w.state().frames();
    for _ in 0..4 {
        w.step();
    }
    assert!(w.state().frames() >= frames + 4, "frames are drawn while the scan is held");
    assert!(w.state().fonts_scanning(), "the scan is going");
    assert!(w.state().element("font.loading").is_some(), "and the list says so");
    assert!(w.state().element("font.item.Test Sans").is_none());
    release.send(()).unwrap();
    w.rest();
    assert!(w.state().element("font.loading").is_none());
    let items = elements_named(&w, "font.item.");
    let names: Vec<&str> = items.iter().map(|(n, ..)| n.as_str()).collect();
    // The shapes' cells name no face, so `shape.formatting` names the face they are drawn in: with nothing shared with the engine, that is the bundled
    // default, `Ubuntu`. The machine's list lacks it: the list holds it first, as it holds any face the shapes have that the machine lacks, and then the
    // families found, sorted. (The name is pinned here and not read back from the engine or the window: a name that changed would not fail a test that
    // computed it.)
    assert_eq!(look(&w, ids[2]).font.as_deref(), Some("Ubuntu"), "the face the shapes are drawn in is named");
    assert_eq!(w.state().font_mix(), Mix::One(Some("Ubuntu".to_owned())), "the Font box names it");
    assert_eq!(names, ["font.item.Ubuntu", "font.item.Test Mono", "font.item.Test Sans", "font.item.Test Serif"], "sorted, as found");
    // Typing narrows the list.
    type_text(&mut w, "serif");
    assert_eq!(elements_named(&w, "font.item.").len(), 1);
    hit(&mut w, "font.item.Test Serif");
    assert_eq!(look(&w, ids[2]).font.as_deref(), Some("Test Serif"));
    assert_eq!(w.state().font_mix(), Mix::One(Some("Test Serif".to_owned())));
    // Opened again, the list is there at once, and a name typed that no font has is taken as it is.
    hit(&mut w, "ribbon.Home.Font");
    assert_eq!(elements_named(&w, "font.item.").len(), 3, "the scan is not made again");
    type_text(&mut w, "Nowhere Display");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(look(&w, ids[2]).font.as_deref(), Some("Nowhere Display"));
}

/// Faces of known names for a scan to find. The default of the set is `Liberation Sans` (`FontDb::default_family`: it comes before the other face), which is
/// not the bundled default, so a Font box that has read the set names something else than one that has not.
fn found_faces() -> FontDb {
    let mut db = FontDb::empty();
    for family in ["Ashlar", "Liberation Sans"] {
        assert_eq!(db.add_font_data(SynthFont::new(family).ascii(500).build()).added, 1, "{family}");
    }
    db
}

/// What the Text dialog's face box shows now.
fn dialog_face(w: &Window) -> String {
    w.get_by_label("Font face").value().unwrap_or_default()
}

/// Whether the last frame asked for another within 200 ms.
fn repaint_soon(w: &Window) -> bool {
    let delay = w.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
    delay.is_some_and(|d| d <= Duration::from_millis(200))
}

/// **The Font box names the face the shapes are drawn in, and reads it again when the scan of the machine's faces ends** (gap U21). The scan is held:
/// the box (and the Text dialog's face box) name the bundled default, and the window asks for a frame every tenth of a second. The scan is let go, and with no
/// edit, no click and no change of selection, only frames, both boxes name the default of the faces the scan found; the window stops asking.
#[test]
fn the_font_box_names_the_bundled_face_while_the_scan_goes_and_the_scanned_one_when_it_ends() {
    let (release, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let fonts = scan_fonts_with(move || {
        // Held until the test lets it go, or for twenty seconds.
        let _ = gate.lock().map(|rx| rx.recv_timeout(Duration::from_secs(20)));
        found_faces()
    });
    // The list of the Text dialog is found by a scan of its own, which answers at once: it is about the list and not about the face.
    let list: Scan = Arc::new(|| vec!["Test Sans".to_owned()]);
    let mut w = edit_window_with(WIDE, |app: DiacraftApp| app.with_font_watch(fonts.clone()).with_font_scan(list));
    let ids = three_shapes(&mut w);
    assert!(!fonts.is_ready(), "the scan is held");
    assert_eq!(w.state().font_mix(), Mix::One(Some("Ubuntu".to_owned())), "while the scan goes the box names the bundled default");
    run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
    assert_eq!(dialog_face(&w), "Ubuntu", "and the Text dialog's face box, which is not blank");
    // The picture of the page asks for a frame once the view has stood still (`REST_AFTER`, by the window's clock, which the test moves): let that pass first, so
    // that what is left to ask for is the scan's.
    w.advance(Duration::from_secs(2));
    w.rest();
    w.step();
    assert!(repaint_soon(&w), "the window asks for a frame while the scan goes");
    for _ in 0..4 {
        w.step();
    }
    assert!(!fonts.is_ready() && w.state().font_mix() == Mix::One(Some("Ubuntu".to_owned())), "frames alone do not change it while the scan is held");
    let revision = w.state().session().revision();
    release.send(()).unwrap();
    // Only frames from here on: the first to find the scan over takes its answer.
    let start = Instant::now();
    while !fonts.is_ready() {
        assert!(start.elapsed() < Duration::from_secs(30), "the scan was let go and no frame took its answer");
        w.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    w.rest();
    assert_eq!(w.state().session().revision(), revision, "nothing was edited");
    assert_eq!(w.state().font_mix(), Mix::One(Some("Liberation Sans".to_owned())), "the box names the default of the faces the scan found");
    assert_eq!(dialog_face(&w), "Liberation Sans", "and so does the Text dialog's face box");
    assert_eq!(look(&w, ids[2]).font.as_deref(), Some("Liberation Sans"), "the engine names the same face");
    w.advance(Duration::from_secs(2));
    w.rest();
    w.step();
    assert!(!repaint_soon(&w), "the window stops asking when the scan is over");
    set_measure_fonts(None);
}

/// The Text dialog's Font tab names the face the shapes are drawn in and does not narrow its list to it; OK with the name untouched sends nothing (the name
/// shown is not a choice); a face typed replaces the name (the box is selected when it takes the keyboard) and narrows the list; a face chosen is the shapes'.
#[test]
fn the_text_dialog_names_the_drawn_face_lists_every_face_and_what_is_typed_replaces_the_name() {
    let scan: Scan = Arc::new(|| vec!["Test Serif".to_owned(), "Test Sans".to_owned(), "Test Mono".to_owned()]);
    let mut w = edit_window_with(WIDE, |app: DiacraftApp| app.with_font_scan(scan));
    let ids = three_shapes(&mut w);
    let revision = w.state().session().revision();
    run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
    assert_eq!(dialog_face(&w), "Ubuntu", "the face box names the face the shapes are drawn in");
    assert_eq!(elements_named(&w, "textdialog.font.item.").len(), 3, "the list is not narrowed to that face (no family found has it in its name)");
    hit(&mut w, "textdialog.ok");
    assert!(w.state().window_state()["textDialog"].is_null(), "OK closes the dialog");
    assert_eq!(w.state().session().revision(), revision, "the name shown was not chosen, so OK sends nothing");
    run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
    hit(&mut w, "textdialog.font");
    type_text(&mut w, "serif");
    assert_eq!(dialog_face(&w), "serif", "what is typed replaces the name");
    assert_eq!(elements_named(&w, "textdialog.font.item.").len(), 1, "and narrows the list");
    hit(&mut w, "textdialog.font.item.Test Serif");
    assert_eq!(dialog_face(&w), "Test Serif");
    hit(&mut w, "textdialog.ok");
    assert_eq!(w.state().session().revision(), revision + 1);
    assert_eq!(look(&w, ids[2]).font.as_deref(), Some("Test Serif"));
}

/// Paragraph alignment and the place of the text in its block: the buttons and the keys, and the one that stands for the shape's state is pressed.
#[test]
fn alignment_comes_from_the_paragraph_buttons_and_the_keys() {
    let (mut w, ids) = three();
    assert_eq!(pressed(&w, Press::Center), Pressed::On, "text is centred when no cell says");
    assert_eq!(pressed(&w, Press::Middle), Pressed::On);
    press(&mut w, "ribbon.Home.AlignLeft");
    w.rest();
    assert_eq!(look(&w, ids[2]).align, Align::Left);
    assert_eq!((pressed(&w, Press::Left), pressed(&w, Press::Center)), (Pressed::On, Pressed::Off));
    press(&mut w, "ribbon.Home.TextAlignBottom");
    w.rest();
    assert_eq!(look(&w, ids[2]).valign, VAlign::Bottom);
    key(&mut w, Key::R, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Right, "Ctrl+Shift+R");
    key(&mut w, Key::J, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Justify, "Ctrl+Shift+J");
    key(&mut w, Key::C, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Center, "Ctrl+Shift+C");
    key(&mut w, Key::L, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Left, "Ctrl+Shift+L");
    key(&mut w, Key::T, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).valign, VAlign::Top, "Ctrl+Shift+T");
    key(&mut w, Key::M, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).valign, VAlign::Middle, "Ctrl+Shift+M");
    key(&mut w, Key::V, CTRL_SHIFT);
    assert_eq!(look(&w, ids[2]).valign, VAlign::Bottom, "Ctrl+Shift+V");
    // The colour of the text.
    hit(&mut w, "ribbon.Home.FontColorGallery");
    let colour = pick_swatch(&mut w, "color", 7);
    assert_eq!(look(&w, ids[2]).color, colour);
}

// ---- arrange ----------------------------------------------------------------------------------------------------------------------------------

fn angle_of(w: &Window, id: u32) -> f64 {
    shape_frame(w.state().session().document().unwrap(), page_id(w), id).unwrap().angle.to_degrees()
}

/// Rotate and flip: the Position menu and the keys, each one undo step, with Rotate Left a turn of 90 degrees counterclockwise.
#[test]
fn rotate_and_flip_come_from_the_position_menu_and_the_keys() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::L, CTRL);
    assert!((angle_of(&w, ids[0]) - 90.0).abs() < 1e-6, "Ctrl+L: {}", angle_of(&w, ids[0]));
    key(&mut w, Key::R, CTRL);
    key(&mut w, Key::R, CTRL);
    assert!((angle_of(&w, ids[0]) + 90.0).abs() < 1e-6, "Ctrl+R twice from 90: {}", angle_of(&w, ids[0]));
    key(&mut w, Key::H, CTRL);
    let f = shape_frame(w.state().session().document().unwrap(), page_id(&w), ids[0]).unwrap();
    assert!(f.flip_x && !f.flip_y, "Ctrl+H flips horizontally");
    key(&mut w, Key::J, CTRL);
    let f = shape_frame(w.state().session().document().unwrap(), page_id(&w), ids[0]).unwrap();
    assert!(f.flip_x && f.flip_y, "Ctrl+J flips vertically");
    // The menu: Position, Rotate Shapes, Rotate Right 90.
    let before = angle_of(&w, ids[0]);
    hit(&mut w, "ribbon.Home.PositionMenu");
    hit(&mut w, "menu.ShapesRotateGallery");
    hit(&mut w, "menu.ShapesRotateLeft");
    assert!((angle_of(&w, ids[0]) - before).abs() > 1.0, "Rotate Left turned it");
    assert_eq!(w.state().ui_panics(), 0);
    // Several shapes turn as one: the status bar has nothing to say of the angle of two.
    click_with(&mut w, (7.0, 6.0), Modifiers::SHIFT);
    assert!(w.state().selection_summary().is_none());
    let one = look_of(w.state().session().document().unwrap(), page_id(&w), &[ids[1]]).unwrap();
    assert_eq!(one.len(), 1);
}

/// On a Mac, Command+H hides the app (winit's app menu takes it before the window sees it), Command+J scrolls to the selection and F11 shows the desktop,
/// so the flips are Shift+Command+H and Option+Command+J and the Text dialog is Command+T there; the keys that were theirs do nothing in the window. Elsewhere they stay.
#[test]
fn on_a_mac_the_flips_and_the_text_dialog_have_keys_the_mac_does_not_reserve() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    let flips = |w: &Window| {
        let f = shape_frame(w.state().session().document().unwrap(), page_id(w), ids[0]).unwrap();
        (f.flip_x, f.flip_y)
    };
    // egui-winit reports the Command key of a Mac as `mac_cmd` and `command`.
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    w.ctx.set_os(egui::os::OperatingSystem::Mac);
    key(&mut w, Key::H, command);
    key(&mut w, Key::J, command);
    assert_eq!(flips(&w), (false, false), "Command+H and Command+J are not the flips on a Mac");
    key(&mut w, Key::H, command | Modifiers::SHIFT);
    assert_eq!(flips(&w), (true, false), "Shift+Command+H flips horizontally");
    key(&mut w, Key::J, command | Modifiers::ALT);
    assert_eq!(flips(&w), (true, true), "Option+Command+J flips vertically");
    key(&mut w, Key::F11, Modifiers::NONE);
    assert!(w.state().window_state()["textDialog"].is_null(), "F11 is the Mac's Show Desktop");
    key(&mut w, Key::T, command);
    assert!(!w.state().window_state()["textDialog"].is_null(), "Command+T opens the Text dialog");
    hit(&mut w, "textdialog.cancel");
    assert!(w.state().window_state()["textDialog"].is_null());
    // Elsewhere the keys are the other systems' as before.
    w.ctx.set_os(egui::os::OperatingSystem::Windows);
    key(&mut w, Key::H, CTRL);
    assert_eq!(flips(&w), (false, true), "Ctrl+H flips back on Windows");
    key(&mut w, Key::T, CTRL);
    assert!(w.state().window_state()["textDialog"].is_null(), "Ctrl+T is nothing on Windows");
    key(&mut w, Key::F11, Modifiers::NONE);
    assert!(!w.state().window_state()["textDialog"].is_null(), "F11 opens the Text dialog on Windows");
}

/// On a Mac, Center is Shift+Command+E and the font sizes Option+Command+. and Option+Command+,, keys every common Mac layout can press (the
/// Mac's own Command+| arrives in the window as `\` or a digit, and the French and Italian comma has no Shift character). The keys that were theirs do
/// nothing there; on Windows and Linux the keys stay Ctrl+Shift+C, Ctrl+Shift+> and Ctrl+Shift+<.
#[test]
fn on_a_mac_center_and_the_font_sizes_are_on_keys_every_common_layout_has() {
    let (mut w, ids) = three();
    let size = |w: &Window| look(w, ids[2]).size;
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    w.ctx.set_os(egui::os::OperatingSystem::Mac);
    press(&mut w, "ribbon.Home.AlignLeft");
    w.rest();
    key(&mut w, Key::Pipe, command);
    key(&mut w, Key::C, command | Modifiers::SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Left, "Command+| and Shift+Command+C do not center on a Mac");
    key(&mut w, Key::E, command | Modifiers::SHIFT);
    assert_eq!(look(&w, ids[2]).align, Align::Center, "Shift+Command+E centers");
    assert!((size(&w) - 12.0).abs() < 1e-3, "{}", size(&w));
    key(&mut w, Key::Period, command | Modifiers::SHIFT);
    key(&mut w, Key::Comma, command | Modifiers::SHIFT);
    assert!((size(&w) - 12.0).abs() < 1e-3, "Shift+Command+. and , are no keys on a Mac: {}", size(&w));
    key(&mut w, Key::Period, command | Modifiers::ALT);
    assert!((size(&w) - 14.0).abs() < 1e-3, "Option+Command+. is bigger: {}", size(&w));
    key(&mut w, Key::Comma, command | Modifiers::ALT);
    key(&mut w, Key::Comma, command | Modifiers::ALT);
    assert!((size(&w) - 11.0).abs() < 1e-3, "Option+Command+, is smaller: {}", size(&w));
    // Elsewhere the keys are the other systems' as before.
    for os in [egui::os::OperatingSystem::Windows, egui::os::OperatingSystem::Nix] {
        w.ctx.set_os(os);
        press(&mut w, "ribbon.Home.AlignLeft");
        w.rest();
        key(&mut w, Key::E, CTRL_SHIFT);
        assert_eq!(look(&w, ids[2]).align, Align::Left, "{os:?}: Ctrl+Shift+E is nothing");
        key(&mut w, Key::C, CTRL_SHIFT);
        assert_eq!(look(&w, ids[2]).align, Align::Center, "{os:?}: Ctrl+Shift+C centers");
        let before = size(&w);
        key(&mut w, Key::Period, CTRL | Modifiers::ALT);
        assert!((size(&w) - before).abs() < 1e-3, "{os:?}: Ctrl+Alt+. is nothing");
        key(&mut w, Key::Period, CTRL_SHIFT);
        assert!(size(&w) > before + 0.5, "{os:?}: Ctrl+Shift+> is bigger");
        key(&mut w, Key::Comma, CTRL_SHIFT);
        assert!((size(&w) - before).abs() < 1e-3, "{os:?}: Ctrl+Shift+< is smaller");
    }
}

/// Bring to Front and Send to Back, from their menus and their keys.
#[test]
fn bring_to_front_and_send_to_back_reorder_the_page() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F, CTRL_SHIFT);
    assert_eq!(shape_ids(&w), [ids[1], ids[2], ids[0]], "Ctrl+Shift+F");
    key(&mut w, Key::B, CTRL_SHIFT);
    assert_eq!(shape_ids(&w), ids, "Ctrl+Shift+B");
    hit(&mut w, "ribbon.Home.ObjectBringToFrontMenu");
    hit(&mut w, "menu.ObjectBringForward");
    assert_eq!(shape_ids(&w), [ids[1], ids[0], ids[2]], "one place forward");
    hit(&mut w, "ribbon.Home.ObjectSendToBackMenu");
    hit(&mut w, "menu.ObjectSendToBack");
    assert_eq!(shape_ids(&w), ids);
}

/// Align moves the shapes selected after the first to it.
#[test]
fn align_moves_the_others_to_the_shape_selected_first() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    click_with(&mut w, (5.0, 3.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [ids[0], ids[2]]);
    hit(&mut w, "ribbon.Home.AlignGallery");
    hit(&mut w, "menu.AlignShapesLeft");
    let (first, other) = (frame_of(&w, ids[0]), frame_of(&w, ids[2]));
    assert!((first.0 - other.0).abs() < 1e-6, "the ellipse's left edge is the rectangle's: {first:?} {other:?}");
    assert!((frame_of(&w, ids[0]).1 - 5.5).abs() < 1e-6, "the first did not move");
    hit(&mut w, "ribbon.Home.AlignGallery");
    hit(&mut w, "menu.AlignShapesMiddle");
    let (first, other) = (frame_of(&w, ids[0]), frame_of(&w, ids[2]));
    assert!(((first.1 + first.3 / 2.0) - (other.1 + other.3 / 2.0)).abs() < 1e-6, "{first:?} {other:?}");
}

// ---- the rotation handle, turned shapes, groups ----------------------------------------------------------------------------------------------

/// The rotation handle is above a selected shape; dragging it turns the shape about its pin in steps that get finer as the pointer goes away, and
/// Escape drops the turn.
#[test]
fn the_rotation_handle_turns_the_shape_in_finer_steps_the_further_the_pointer_is() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    // The rectangle is 1.5 by 1 about (3, 6): the handle is 36 points above the middle of its top edge, at (3, 6.875).
    let before = w.state().session().revision();
    drag_with(&mut w, (3.0, 6.875), (4.0, 6.05), Modifiers::NONE);
    let angle = angle_of(&w, ids[0]);
    assert!((angle + 90.0).abs() < 1e-6, "a quarter turn clockwise, to a step of 15 degrees: {angle}");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    assert_eq!(w.state().selection_summary().unwrap(), "Width: 1.50 in   Height: 1.00 in   Angle: 270\u{b0}");
    // Turned to -90 degrees the rectangle's top is on the right, so its handle is 36 points beyond the right edge, at (3.875, 6). Far from the pin
    // the steps are fine: 4.5 inches out, 8 degrees above the level, is a turn of -82 degrees to a tenth.
    let (s, c) = 8.0_f64.to_radians().sin_cos();
    drag_with(&mut w, (3.875, 6.0), (3.0 + 4.5 * c, 6.0 + 4.5 * s), Modifiers::NONE);
    let fine = angle_of(&w, ids[0]);
    assert!((fine + 82.0).abs() < 0.11, "{fine}");
    assert!((fine / 15.0).fract().abs() > 1e-3, "not a multiple of 15 degrees: {fine}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A shape that is turned has its handles on its own box, a drag works along its own axes, and the edge opposite the handle stays where it is.
#[test]
fn a_turned_shape_is_resized_from_its_handles_along_its_own_axes() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::L, CTRL);
    assert!((angle_of(&w, ids[0]) - 90.0).abs() < 1e-6);
    // Turned a quarter to the left about its pin (3, 6): its own x axis, 1.5 long, goes up the page from (3, 5.25) to (3, 6.75).
    let c0 = shape_corners(w.state().session().document().unwrap(), page_id(&w), ids[0]).unwrap()[0];
    let before = w.state().session().revision();
    drag_with(&mut w, (3.0, 6.75), (3.0, 7.75), Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    let f = shape_frame(w.state().session().document().unwrap(), page_id(&w), ids[0]).unwrap();
    assert!((f.width - 2.5).abs() < 1e-6 && (f.height - 1.0).abs() < 1e-6, "{} x {}: wider along its own axis", f.width, f.height);
    assert!((f.angle.to_degrees() - 90.0).abs() < 1e-6, "still turned");
    let corners = shape_corners(w.state().session().document().unwrap(), page_id(&w), ids[0]).unwrap();
    assert!((corners[0].0 - c0.0).abs() < 1e-6 && (corners[0].1 - c0.1).abs() < 1e-6, "the far edge stays: {:?} {:?}", corners[0], c0);
    assert!((corners[1].1 - 7.75).abs() < 1e-6, "and the near edge went where the pointer did: {:?}", corners[1]);
    // The handle drawn on the screen is where the box is: a click on the turned shape's middle selects it, one beside it does not.
    assert_eq!(w.state().ui_panics(), 0);
}

/// A flipped shape's handles are on its box too.
#[test]
fn a_flipped_shape_keeps_its_handles() {
    let (mut w, ids) = three();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::H, CTRL);
    let before = frame_of(&w, ids[0]);
    // The east edge at the page's right is still the one at x = 3.75 for a shape flipped about its middle.
    drag_with(&mut w, (3.75, 6.0), (4.75, 6.0), Modifiers::NONE);
    let after = frame_of(&w, ids[0]);
    assert!(after.2 > before.2 + 0.9, "{before:?} {after:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

fn new_drawing() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session
}

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

fn demo_window(dir: &TempDir) -> Window {
    let path = fixture::write(dir.path(), "demo.vsdx", &fixture::demo_vsdx());
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// A click selects the group; a second click on it selects the member under the pointer, and one on a member selects a sibling. The member moves
/// by itself, in its group's frame.
#[test]
fn a_second_click_on_a_selected_group_selects_the_member_and_the_member_moves_alone() {
    let dir = TempDir::new("group");
    let mut w = demo_window(&dir);
    // The group (shape 7) is 2.4 by 2 inches from (4.6, 3.4): a box (8) and an ellipse (9) in its middle. (A pause between the clicks, or two
    // clicks in a row are a double click, which opens the text box.)
    click_at(&mut w, 5.8, 4.3);
    assert_eq!(w.state().selection(), [7], "the first click selects the group");
    pause(&mut w);
    click_at(&mut w, 5.8, 4.3);
    assert_eq!(w.state().selection(), [9], "the second selects the ellipse under the pointer");
    pause(&mut w);
    click_at(&mut w, 4.8, 3.6);
    assert_eq!(w.state().selection(), [8], "and one beside it, on the box, selects that");
    // A member can be dragged out of its group and moved alone; the group's own cells do not change.
    let group_before = frame_of(&w, 7);
    let member_before = frame_of(&w, 9);
    pause(&mut w);
    click_at(&mut w, 5.8, 4.3);
    assert_eq!(w.state().selection(), [9]);
    pause(&mut w);
    // Alt held: no snap, so it goes by the drag, 0.45 to the right (with the engine's snap on, as a new drawing has it, the ellipse's box is pulled to
    // the points of the outlines near it, its group's among them).
    drag_with(&mut w, (5.8, 4.3), (6.25, 4.3), Modifiers::ALT);
    let member_after = frame_of(&w, 9);
    assert!((member_after.0 - member_before.0 - 0.45).abs() < 1e-6, "{member_before:?} {member_after:?}");
    assert_eq!(frame_of(&w, 7), group_before);
    // A click away selects what is under it, not a member.
    pause(&mut w);
    click_at(&mut w, 2.0, 4.4);
    assert_eq!(w.state().selection(), [6]);
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- a text box that is open ------------------------------------------------------------------------------------------------------------------

/// The box stays open while its characters are formatted: the range selected in it is sent with `start` and `end`, a press on the ribbon does not
/// end the edit, and the same characters are selected afterwards.
#[test]
fn characters_selected_in_the_text_box_are_formatted_and_the_box_stays_open() {
    let dir = TempDir::new("range");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(w.state().edited_text_selection(), Some((0, 2)), "a and b are selected");
    let runs = |w: &Window| {
        let shape = &w.state().session().document().unwrap().pages.get(0).unwrap().shapes.iter().find(|s| s.id == 1).unwrap().clone();
        shape.text.as_ref().unwrap().parts.iter().filter(|p| matches!(p, TextPart::Cp(_))).count()
    };
    assert_eq!(runs(&w), 2);
    let before = w.state().session().revision();
    // The first run is bold (the file says so), so a press on Bold takes it off the two characters: the run is the range and keeps its row.
    key(&mut w, Key::B, CTRL);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is still open");
    assert_eq!(w.state().edited_text_selection(), Some((0, 2)), "and the same characters are selected");
    assert_eq!(w.state().session().revision(), before + 1, "one command, and nothing was typed");
    // Now a single character, from the ribbon: Italic on 'a'. A press on the ribbon does not end the edit.
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(w.state().edited_text_selection(), Some((0, 1)));
    press(&mut w, "ribbon.Home.Italic");
    w.rest();
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "a press on the ribbon leaves the box open");
    assert_eq!(w.state().edited_text_selection(), Some((0, 1)));
    assert!(runs(&w) >= 3, "the run was split at the range: {} runs", runs(&w));
    assert_eq!(w.state().edited_text(), Some("abcd"), "and the characters are the same");
    // The look shows the first run, 'a', which is italic now.
    assert!(look(&w, 1).italic);
    // A caret with nothing selected formats the whole text.
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    key(&mut w, Key::U, CTRL);
    assert!(look(&w, 1).underline);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap());
}

/// `shape.formatting` for the characters `a..b` of the label (shape 1), as the command answers.
fn answer(w: &mut Window, a: usize, b: usize) -> serde_json::Value {
    let page = page_id(w);
    let mut answered = w.state_mut().execute("shape.formatting", &json!({"ids": [1], "page": page, "start": a, "end": b})).unwrap();
    answered["shapes"][0].take()
}

/// The names `mixed` lists in an answer.
fn mixed_of(answered: &serde_json::Value) -> Vec<String> {
    answered["mixed"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_owned()).collect()
}

/// A colour of an answer, `#rrggbb`.
fn rgb_of(hex: &str) -> Rgb {
    let part = |from: usize| u8::from_str_radix(&hex[from..from + 2], 16).unwrap();
    Rgb(part(1), part(3), part(5))
}

/// Opens the label's text box (F2) and selects the characters `from..to` (a caret when they are equal).
fn select_chars(w: &mut Window, from: usize, to: usize) {
    if !w.state().window_state()["editingText"].as_bool().unwrap() {
        click_at(w, 3.0, 6.0);
        key(w, Key::F2, Modifiers::NONE);
    }
    key(w, Key::ArrowUp, Modifiers::NONE);
    key(w, Key::Home, Modifiers::NONE);
    for _ in 0..from {
        key(w, Key::ArrowRight, Modifiers::NONE);
    }
    for _ in from..to {
        key(w, Key::ArrowRight, Modifiers::SHIFT);
    }
    assert_eq!(w.state().edited_text_selection(), Some((from, to)));
}

/// Every control of the Font and Paragraph groups shows what `shape.formatting` answers for the characters `a..b`: the value when the runs share it, a
/// half pressed toggle, a blank box or a slash when they differ (the list `mixed`, not the word).
fn controls_agree_with_the_command(w: &mut Window, a: usize, b: usize) {
    let said = answer(w, a, b);
    let mixed = mixed_of(&said);
    let is_mixed = |name: &str| mixed.iter().any(|n| n == name);
    let toggle = |name: &str| {
        if is_mixed(name) {
            Pressed::Part
        } else if said[name].as_bool().unwrap() {
            Pressed::On
        } else {
            Pressed::Off
        }
    };
    let st = w.state();
    assert_eq!(st.press_state(Press::Bold), toggle("bold"), "bold over {a}..{b}: {said}");
    assert_eq!(st.press_state(Press::Italic), toggle("italic"), "italic over {a}..{b}: {said}");
    assert_eq!(st.press_state(Press::Underline), toggle("underline"), "underline over {a}..{b}: {said}");
    assert_eq!(st.press_state(Press::Strike), toggle("strike"), "strike over {a}..{b}: {said}");
    let font = if is_mixed("font") { Mix::Mixed } else { Mix::One(said["font"].as_str().map(str::to_owned)) };
    assert_eq!(st.font_mix(), font, "font over {a}..{b}: {said}");
    let size = if is_mixed("size") { Mix::Mixed } else { Mix::One((said["size"].as_f64().unwrap() * 1000.0).round() as i64) };
    assert_eq!(st.size_mix(), size, "size over {a}..{b}: {said}");
    let colour = if is_mixed("color") { Mix::Mixed } else { Mix::One(rgb_of(said["color"].as_str().unwrap())) };
    assert_eq!(st.text_colour_mix(), colour, "colour over {a}..{b}: {said}");
    for (press, word) in [(Press::Left, "left"), (Press::Center, "center"), (Press::Right, "right"), (Press::Justify, "justify")] {
        let want = if !is_mixed("align") && said["align"] == word { Pressed::On } else { Pressed::Off };
        assert_eq!(st.press_state(press), want, "{word} over {a}..{b}: {said}");
    }
}

/// The label with `cd` given a face, a size and a colour of its own, in a window wide enough for the Home tab.
fn label_with_two_looks(dir: &TempDir) -> Window {
    let (mut w, _) = crate::text_edit::labelled_window(dir, |app| app);
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    let page = page_id(&w);
    run_command(&mut w, "text.format", json!({"ids": [1], "page": page, "start": 2, "end": 4, "font": "Arial", "size": 20, "color": "#336699"}));
    w
}

/// With characters selected in the open box the Font group reads `shape.formatting` for them (gap U19): across runs that differ the toggles are half
/// pressed and the boxes blank, over one run they show it. The file's label is `ab` bold then
/// `cd` italic; `cd` is given a face, a size and a colour of its own.
#[test]
fn the_font_group_reads_the_range_across_differing_runs() {
    let dir = TempDir::new("range-read");
    let mut w = label_with_two_looks(&dir);
    select_chars(&mut w, 0, 4);
    // Across both runs.
    let both = answer(&mut w, 0, 4);
    assert_eq!(mixed_of(&both), ["font", "size", "bold", "italic", "color"], "the runs differ in these and agree in the others");
    controls_agree_with_the_command(&mut w, 0, 4);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Part);
    assert_eq!(pressed(&w, Press::Italic), Pressed::Part);
    assert_eq!(pressed(&w, Press::Underline), Pressed::Off);
    assert_eq!(w.state().font_mix(), Mix::Mixed);
    assert_eq!(w.state().size_mix(), Mix::Mixed);
    assert_eq!(w.state().text_colour_mix(), Mix::Mixed);
    // The first run alone, then the second alone.
    select_chars(&mut w, 0, 2);
    controls_agree_with_the_command(&mut w, 0, 2);
    assert_eq!(pressed(&w, Press::Bold), Pressed::On, "ab is bold");
    assert_eq!(pressed(&w, Press::Italic), Pressed::Off);
    select_chars(&mut w, 2, 4);
    controls_agree_with_the_command(&mut w, 2, 4);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Off, "cd is not bold, though the shape's first run is");
    assert_eq!(pressed(&w, Press::Italic), Pressed::On);
    assert_eq!(w.state().font_mix(), Mix::One(Some("Arial".to_owned())));
    assert_eq!(w.state().size_mix(), Mix::One(20_000));
    assert_eq!(w.state().text_colour_mix(), Mix::One(Rgb(0x33, 0x66, 0x99)));
    // A range across the join, and a range inside a run.
    for (a, b) in [(1, 3), (1, 2), (3, 4), (0, 3)] {
        select_chars(&mut w, a, b);
        controls_agree_with_the_command(&mut w, a, b);
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// Presses over a range: Bold over `cd` turns it on and a second press turns it off, `ab` as it was; a press on a mixed toggle turns it on for the whole
/// range; Underline from the ribbon and Increase Font Size act on the characters selected and step from the size the box shows.
#[test]
fn a_press_over_a_range_acts_on_those_characters() {
    let dir = TempDir::new("range-press");
    let mut w = label_with_two_looks(&dir);
    // Bold over `cd` turns it on and a second press turns it off, `ab` as it was (the old reading sent the first run's value every time).
    select_chars(&mut w, 2, 4);
    key(&mut w, Key::B, CTRL);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is still open");
    assert_eq!(answer(&mut w, 2, 4)["bold"], true);
    assert_eq!(pressed(&w, Press::Bold), Pressed::On);
    key(&mut w, Key::B, CTRL);
    assert_eq!(answer(&mut w, 2, 4)["bold"], false);
    assert_eq!(answer(&mut w, 0, 2)["bold"], true);
    // A press on a toggle that is mixed turns it on for every character of the range.
    select_chars(&mut w, 0, 4);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Part);
    key(&mut w, Key::B, CTRL);
    let now = answer(&mut w, 0, 4);
    assert_eq!(now["bold"], true);
    assert!(!mixed_of(&now).contains(&"bold".to_owned()), "bold is whole now");
    assert_eq!(pressed(&w, Press::Bold), Pressed::On);
    key(&mut w, Key::B, CTRL);
    assert_eq!(answer(&mut w, 0, 4)["bold"], false);
    // Underline from the ribbon on the second run alone.
    select_chars(&mut w, 2, 4);
    press(&mut w, "ribbon.Home.Underline");
    w.rest();
    assert_eq!(answer(&mut w, 2, 4)["underline"], true);
    assert_eq!(answer(&mut w, 0, 2)["underline"], false);
    assert_eq!(pressed(&w, Press::Underline), Pressed::On);
    // Increase Font Size steps from the size the box shows: `cd` is 20 points, the next of the list is 24.
    press(&mut w, "ribbon.Home.FontSizeIncrease");
    w.rest();
    assert_eq!(answer(&mut w, 2, 4)["size"], 24.0);
    assert_eq!(answer(&mut w, 0, 2)["size"], 14.4, "ab keeps its size");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A caret shows the run before it (the first character's at the start), as the engine answers for a point; a press still formats the whole text, with
/// the value that run gives. Characters typed and not sent leave the controls as they were.
#[test]
fn a_caret_shows_the_run_before_it() {
    let dir = TempDir::new("range-point");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    for at in [0, 1, 2, 3, 4] {
        select_chars(&mut w, at, at);
        controls_agree_with_the_command(&mut w, at, at);
    }
    select_chars(&mut w, 2, 2);
    assert_eq!(pressed(&w, Press::Bold), Pressed::On, "the caret is after `ab`, which is bold");
    assert_eq!(pressed(&w, Press::Italic), Pressed::Off);
    select_chars(&mut w, 3, 3);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Off, "after `c`, which is in the italic run");
    assert_eq!(pressed(&w, Press::Italic), Pressed::On);
    select_chars(&mut w, 0, 0);
    assert_eq!(pressed(&w, Press::Bold), Pressed::On, "at the start, the first character's run");
    // Typing is not sent until something needs it: the controls show what the caret was in.
    select_chars(&mut w, 3, 3);
    type_text(&mut w, "X");
    assert!(w.state().has_unsent_text());
    assert_eq!(pressed(&w, Press::Italic), Pressed::On);
    assert_eq!(pressed(&w, Press::Bold), Pressed::Off);
    // A press with the caret in `cd` (bold off there) turns bold on for the whole text, what was typed sent first.
    key(&mut w, Key::B, CTRL);
    assert!(!w.state().has_unsent_text(), "what was typed was sent");
    assert_eq!(w.state().edited_text(), Some("abcXd"));
    let whole = answer(&mut w, 0, 5);
    assert_eq!(whole["bold"], true);
    assert!(!mixed_of(&whole).contains(&"bold".to_owned()));
    assert_eq!(w.state().ui_panics(), 0);
}

/// Paragraphs that differ in alignment: no alignment button is pressed over both, and the paragraph's over one.
#[test]
fn the_alignment_buttons_read_the_paragraphs_of_the_range() {
    let dir = TempDir::new("range-align");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    let page = page_id(&w);
    run_command(&mut w, "shape.set_text", json!({"id": 1, "page": page, "text": "\n", "start": 2, "end": 2}));
    run_command(&mut w, "text.format", json!({"ids": [1], "page": page, "start": 3, "end": 5, "align": "right"}));
    select_chars(&mut w, 0, 5);
    assert!(mixed_of(&answer(&mut w, 0, 5)).contains(&"align".to_owned()));
    controls_agree_with_the_command(&mut w, 0, 5);
    for press in [Press::Left, Press::Center, Press::Right, Press::Justify] {
        assert_eq!(pressed(&w, press), Pressed::Off, "no button is pressed over paragraphs that differ");
    }
    select_chars(&mut w, 3, 5);
    controls_agree_with_the_command(&mut w, 3, 5);
    assert_eq!(pressed(&w, Press::Right), Pressed::On);
    select_chars(&mut w, 0, 2);
    controls_agree_with_the_command(&mut w, 0, 2);
    assert_ne!(pressed(&w, Press::Right), Pressed::On);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A style cell that is the theme's is answered by the engine, which the window no longer reads for itself: the controls show what the command says.
#[test]
fn a_themed_style_cell_is_read_through_the_engine() {
    let dir = TempDir::new("themed-style");
    let label = diacraft_testkit::package::shape(
        1,
        &[
            fixture::placed(2.0, 5.5, 2.0, 1.0),
            r#"<Section N="Character"><Row IX="0"><Cell N="Style" V="Themed" F="THEMEVAL()"/></Row></Section>"#.to_owned(),
            r#"<Text><cp IX="0"/>abcd</Text>"#.to_owned(),
        ]
        .concat(),
    );
    let path = fixture::write(dir.path(), "themed.vsdx", &fixture::page_with(&label));
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None), WIDE);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    select_chars(&mut w, 1, 3);
    let said = answer(&mut w, 1, 3);
    assert_eq!(mixed_of(&said), Vec::<String>::new(), "one run");
    controls_agree_with_the_command(&mut w, 1, 3);
    assert_eq!(w.state().ui_panics(), 0);
}

/// What is typed in the box is sent before the formatting, so the characters formatted are the ones on the screen.
#[test]
fn what_was_typed_is_sent_before_the_characters_are_formatted() {
    let dir = TempDir::new("typed");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::End, Modifiers::NONE);
    type_text(&mut w, "XY");
    assert_eq!(w.state().edited_text(), Some("abcdXY"));
    key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
    key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(w.state().edited_text_selection(), Some((4, 6)));
    key(&mut w, Key::I, CTRL);
    assert_eq!(w.state().edited_text(), Some("abcdXY"), "the box has what was typed");
    assert_eq!(w.state().edited_text_selection(), Some((4, 6)));
    let chars = diacraft_engine::draw::text_view(w.state().session().document().unwrap(), page_id(&w), 1).unwrap().chars;
    assert_eq!(chars, "abcdXY", "and so has the drawing");
}

// ---- the keys ---------------------------------------------------------------------------------------------------------------------------------

/// With the shape's own text box open, a text field that is not that box (the Font list's) keeps its keys: Ctrl+B and Ctrl+Shift+L, which format
/// the label while the box has the keyboard, format nothing while the list's box has it.
#[test]
fn the_font_lists_box_keeps_its_keys_while_the_shapes_text_box_is_open() {
    let dir = TempDir::new("two-boxes");
    let scan: diacraft_ui_egui::font_list::Scan = Arc::new(|| vec!["Test Sans".to_owned()]);
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app.with_font_scan(scan));
    w.set_size(egui::vec2(WIDE[0], WIDE[1]));
    w.rest();
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::Home, Modifiers::NONE);
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    // The shape's box has the keyboard: Ctrl+B formats the character (and the box stays open).
    let before = w.state().session().revision();
    key(&mut w, Key::B, CTRL);
    assert_eq!(w.state().session().revision(), before + 1, "the box has the keyboard, so Ctrl+B formats");
    // A press on the ribbon opens the Font list, whose box takes the keyboard; the shape's box is still open.
    press(&mut w, "ribbon.Home.Font");
    w.rest();
    assert!(w.state().element("font.filter").is_some(), "the list is open");
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "and the shape's box is open under it");
    let before = w.state().session().revision();
    key(&mut w, Key::B, CTRL);
    key(&mut w, Key::L, CTRL_SHIFT);
    key(&mut w, Key::I, CTRL);
    assert_eq!(w.state().session().revision(), before, "the list's box has the keyboard: its keys do not format the label");
}

/// A key that belongs to a text field is not taken by the drawing: while the font list's box has the keyboard, Ctrl+B and Ctrl+L format nothing.
#[test]
fn a_box_that_has_the_keyboard_keeps_its_keys() {
    let (mut w, ids) = three();
    hit(&mut w, "ribbon.Home.Font");
    key(&mut w, Key::B, CTRL);
    key(&mut w, Key::L, CTRL);
    assert!(!look(&w, ids[2]).bold, "Ctrl+B in the font list's box is not Bold");
    assert!((angle_of(&w, ids[2])).abs() < 1e-9, "nor is Ctrl+L Rotate Left");
}

// ---- the ribbon at small widths --------------------------------------------------------------------------------------------------------------

/// With a shape selected the Home tab fits at every width: no group leaves the window or lies on another, and the window comes to rest.
#[test]
fn the_home_tab_with_a_selection_fits_at_narrow_and_wide_windows() {
    for width in [480.0_f32, 560.0, 640.0, 800.0, 1024.0, 1280.0, 1700.0, 2400.0] {
        // (Wide windows are drawn too: the page's picture is held to the harness's 2,048 pixel textures, which a wider one would have panicked on.)
        let mut w = fast_window(DiacraftApp::new(new_drawing(), None).with_inline_pictures(bundled_drawer()), [width, 700.0]);
        w.rest();
        three_shapes(&mut w);
        w.rest();
        let mut groups = elements_named(&w, "ribbon.Home.group.");
        groups.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
        assert_eq!(groups.len(), 7, "{width}: every group is drawn");
        for (name, rect, _) in &groups {
            assert!(rect.min.x >= 0.0 && rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        for pair in groups.windows(2) {
            assert!(pair[0].1.max.x <= pair[1].1.min.x + 0.5, "{} lies on {} at {width}", pair[0].0, pair[1].0);
        }
        // Every control drawn is inside its group's strip.
        for (name, rect, _) in
            elements_named(&w, "ribbon.Home.").into_iter().filter(|(n, ..)| !n.starts_with("ribbon.Home.group.") && !n.contains(".fold."))
        {
            assert!(rect.max.x <= width + 0.5, "{name} at {width}: {rect:?}");
        }
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// The page's picture is held to the back end's texture limit (egui's `max_texture_side`, 2,048 in this harness; a WebGL context can be lower still),
/// not to a fixed 8,192: a page with shapes in a window wider than that used to make egui panic, which on the web nothing catches.
#[test]
fn the_pages_picture_is_held_to_the_back_ends_texture_limit() {
    for size in [[2400.0_f32, 700.0], [3000.0, 1200.0]] {
        let mut w = fast_window(DiacraftApp::new(new_drawing(), None).with_inline_pictures(bundled_drawer()), size);
        w.rest();
        three_shapes(&mut w);
        run_command(&mut w, "view.zoom", json!({"percent": 100}));
        w.rest();
        let picture = w.state().canvas_report().picture.expect("a picture of the page is shown");
        assert!(picture.size[0] <= 2048 && picture.size[1] <= 2048, "{size:?}: a {}x{} picture", picture.size[0], picture.size[1]);
        assert!(picture.lit > 0, "and it has the page in it");
        assert_eq!(w.state().ui_panics(), 0, "{size:?}");
    }
}

/// Mirrors the ribbon table: the controls that send parameters name a window command that takes them.
#[test]
fn every_control_that_carries_parameters_names_a_command_that_takes_them() {
    let mut seen = 0;
    for tab in diacraft_ui_egui::ribbon::spec::all_tabs() {
        for group in tab.groups {
            group.walk(&mut |c| {
                if let Some(palette) = c.palette {
                    let command = diacraft_ui_egui::commands::find_ui_command(c.command.unwrap()).unwrap();
                    assert!(diacraft_engine::param_keys(command.params).unwrap().contains(&palette.key), "{}: {}", c.id, palette.key);
                    seen += 1;
                }
                if c.args.is_empty() {
                    return;
                }
                // The window's commands and the engine's (the page's orientation is the engine's own `page.orientation`).
                let id = c.command.unwrap_or_default();
                let params = diacraft_ui_egui::commands::find_ui_command(id)
                    .map(|command| command.params)
                    .or_else(|| diacraft_engine::find_command(id).map(|command| command.params))
                    .unwrap_or_else(|| panic!("{} names no command", c.id));
                let args: serde_json::Value = serde_json::from_str(c.args).unwrap_or_else(|e| panic!("{}: {e}", c.id));
                let keys = diacraft_engine::param_keys(params).unwrap();
                assert!(args.as_object().unwrap().keys().all(|k| keys.contains(&k.as_str())), "{}: {id} does not take {}", c.id, c.args);
                seen += 1;
            });
        }
    }
    assert!(seen > 70, "the walk found {seen} controls with parameters");
}

/// A caption the map does not give is marked as ours (never derived from an id): every new control of the Home tab is among them, and the ones
/// the map names are confirmed.
#[test]
fn the_new_controls_keep_their_captions_marked_as_ours_unless_the_map_gives_them() {
    let ours: Vec<&str> = diacraft_ui_egui::ribbon::spec::unconfirmed_captions().into_iter().map(|(_, _, id)| id).collect();
    for id in [
        "Bold",
        "AlignLeft",
        "AlignShapesLeft",
        "ShapesRotateRight",
        "ShapesFlipVertical",
        "LineWeight075",
        "LinePattern4",
        "EndArrow13",
        "BeginArrowMenu",
    ] {
        assert!(ours.contains(&id), "{id} has a caption that is ours");
    }
    for id in
        ["FontColorGallery", "FillColorGallery", "LineColorGallery", "ShapesRotateGallery", "ObjectBringToFront", "ObjectSendToBack", "ObjectsGroup"]
    {
        assert!(!ours.contains(&id), "the map gives the caption of {id}");
    }
}
