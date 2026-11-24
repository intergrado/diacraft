//! Stencil files in the window: More Shapes, Open Stencil with a stand-in for the native dialog, the stencil's row with its close control, the
//! tiles of its masters drawn from their icons or their own outlines (hidden ones left out), a tile dragged onto the page, a master the engine refuses
//! told in the status bar, and the stencils kept when a drawing is read from a file. Every stencil is built in code (`diacraft_testkit::package`): no
//! stencil from another application is used.

use std::path::PathBuf;
use std::sync::Arc;

use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
use diacraft_ui_egui::FilePicker;
use egui::Key;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key, shape_ids};
use crate::support::{Rest, TempDir, Window, click, elements_named, run_command};

/// The content type of a stencil.
const CT_STENCIL: &str = "application/vnd.ms-visio.stencil.main+xml"; // brand-ok: a content type the format defines

const WIDE: [f32; 2] = [1280.0, 900.0];

/// The cells of a box `w` by `h`: its size, its pin in the middle and a closed outline.
fn box_cells(w: f64, h: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{px}" F="Width*0.5"/><Cell N="PinY" V="{py}" F="Height*0.5"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/>
<Cell N="LocPinX" V="{px}" F="Width*0.5"/><Cell N="LocPinY" V="{py}" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
        px = w / 2.0,
        py = h / 2.0
    )
}

/// The cells of a triangle `w` by `h`.
fn triangle_cells(w: f64, h: f64) -> String {
    format!(
        r#"<Cell N="PinX" V="{px}" F="Width*0.5"/><Cell N="PinY" V="{py}" F="Height*0.5"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/>
<Cell N="LocPinX" V="{px}" F="Width*0.5"/><Cell N="LocPinY" V="{py}" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{px}" F="Width*0.5"/><Cell N="Y" V="{h}" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
        px = w / 2.0,
        py = h / 2.0
    )
}

/// Base64 text, as a file keeps an icon.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0_u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A 2 by 2 icon of 32-bit pixels, in an ICO file: four solid colours.
fn ico() -> Vec<u8> {
    let mut dib = Vec::new();
    for n in [40_u32, 2, 4, 0x0020_0001, 0, 0, 0, 0, 0, 0] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255]);
    dib.extend_from_slice(&[0; 8]);
    let mut out = vec![0, 0, 1, 0, 1, 0, 2, 2, 0, 0, 1, 0, 32, 0];
    out.extend_from_slice(&u32::try_from(dib.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&dib);
    out
}

/// A stencil of five masters: "Box" (2 by 1, with an icon), "Triangle" (no icon), "Secret" (hidden), "Bad icon" (an icon of a kind the window does not
/// read) and "Empty" (no shape, which a drop refuses).
fn stencil_bytes() -> Vec<u8> {
    let with_icon = MasterSpec::new(1, "Box").shapes(&shape(5, &box_cells(2.0, 1.0))).icon(&base64(&ico()));
    let triangle = MasterSpec::new(2, "Triangle").shapes(&shape(5, &triangle_cells(1.0, 1.0)));
    let hidden = MasterSpec::new(3, "Secret").attrs(r#" Hidden="1""#).shapes(&shape(5, &box_cells(1.0, 1.0)));
    let odd = MasterSpec::new(4, "Bad icon").shapes(&shape(5, &box_cells(1.0, 1.0))).icon(&base64(b"GIF89a not a picture the window reads"));
    let empty = MasterSpec::new(5, "Empty");
    VsdxBuilder::new().document_type(CT_STENCIL).master(with_icon).master(triangle).master(hidden).master(odd).master(empty).build()
}

pub(crate) fn write_stencil(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.file(name);
    std::fs::write(&path, stencil_bytes()).unwrap();
    path
}

/// A stand-in for the native dialog: it answers with `path` at once.
pub(crate) fn picker(path: PathBuf) -> FilePicker {
    Arc::new(move || Some(path.clone()))
}

pub(crate) fn window(path: PathBuf) -> Window {
    edit_window_with(WIDE, |app| app.with_stencil_picker(picker(path)))
}

pub(crate) fn tile_names(w: &Window) -> Vec<String> {
    let mut tiles = elements_named(w, "shapes.tile.");
    tiles.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y).then(a.1.min.x.total_cmp(&b.1.min.x)));
    tiles.into_iter().map(|(n, ..)| n.trim_start_matches("shapes.tile.").to_owned()).collect()
}

pub(crate) fn rows(w: &mut Window) -> Vec<String> {
    hit(w, "shapes.more");
    let mut rows = elements_named(w, "shapes.stencil.");
    rows.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
    let names = rows.into_iter().map(|(n, ..)| n.trim_start_matches("shapes.stencil.").to_owned()).collect();
    key(w, Key::Escape, egui::Modifiers::NONE);
    names
}

/// A click on the control `name`, then the window comes to rest (the dialog's thread and what follows are waited for).
fn hit(w: &mut Window, name: &str) {
    if name.starts_with("shapes.stencil") && w.state().element(name).is_none() {
        hit(w, "shapes.more");
    }
    if name.starts_with("shapes.stencil") {
        scroll_into_the_menu(w, name);
    }
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

/// A row of the stencil menu (or its close control) below or above what the menu's list shows is scrolled into it with the wheel, as a person does, before it
/// is clicked: the list is at most 440 points high, and the built-in stencils, their headings and the Quick Shapes row fill it before a stencil file's row
/// (the stencil and the row together pushed `Mine` past it, and the click landed outside the menu).
fn scroll_into_the_menu(w: &mut Window, name: &str) {
    use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, vec2};
    let Some(list) = w.state().element("shapes.catalogue").map(|e| e.rect) else { return };
    for _ in 0..40 {
        let row = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect;
        if list.contains_rect(row) {
            return;
        }
        let dy = if row.top() < list.top() { 44.0 } else { -44.0 };
        crate::support::move_to(w, list.center());
        w.input_mut().events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, dy),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: MouseWheelSource::Unknown,
        });
        // The list scrolls smoothly over several frames: they are stepped before the window is let come to rest.
        for _ in 0..12 {
            w.step();
        }
        w.rest();
    }
    panic!("{name} was not scrolled into the stencil menu");
}

/// More Shapes, Open Stencil, as a person does it.
pub(crate) fn open_from_the_flyout(w: &mut Window) {
    hit(w, "shapes.more");
    assert!(w.state().element("shapes.more.open").expect("Open Stencil is in the flyout").enabled, "a dialog is there to ask");
    hit(w, "shapes.more.open");
    // `hit` came to rest, and rest waits for the dialog (asked for, on the screen, answered): the stencil is open now, or the refusal is in the status bar.
    assert!(!w.state().stencil_dialog_open(), "the window came to rest with the dialog still asked for or on the screen");
}

pub(crate) fn opened_ids(w: &Window) -> Vec<String> {
    w.state().session().stencils().iter().map(|s| s.id().to_owned()).collect()
}

/// **The dialog is waited for from the moment it is asked for, not from the moment its thread starts.** A command asks for the chooser and the next frame
/// starts its thread; a test that came to rest in between (the click's last frame had asked, no frame had started the thread) found no stencil open, and
/// on a busy machine that was most runs. Nothing here depends on how fast anything is: the state is read between the command and the frame.
#[test]
fn the_dialog_is_waited_for_from_the_moment_it_is_asked_for() {
    let dir = TempDir::new("asked-for");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    assert!(!w.state().stencil_dialog_open() && w.state().can_pick_stencil());
    w.state_mut().execute("shapes.openStencil", &json!({})).unwrap();
    assert!(w.state().stencil_dialog_open(), "asked for, and no frame has started it: it is still the window's to wait for");
    assert!(!w.state().can_pick_stencil(), "and another one cannot be asked for");
    let again = w.state_mut().execute("shapes.openStencil", &json!({})).unwrap_err().to_string();
    assert!(again.contains("being chosen"), "{again}");
    assert!(opened_ids(&w).is_empty());
    w.rest();
    assert!(!w.state().stencil_dialog_open() && w.state().can_pick_stencil());
    assert_eq!(opened_ids(&w), ["file-1"], "rest waited for the thread, and for the frame that took its answer");
    assert_eq!(w.state().window_state()["stencil"], "file-1");
}

/// A dialog that is still open is waited for, however long it stays: the chooser here answers when the test lets it, so the answer comes in a frame the
/// test cannot predict, and `rest` is what waits for it, by the state of the dialog itself.
#[test]
fn a_dialog_that_answers_late_is_waited_for_by_what_it_is_and_not_by_a_time() {
    let dir = TempDir::new("late-dialog");
    let path = write_stencil(&dir, "Mine.vssx");
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Arc::new(std::sync::Mutex::new(held));
    let chooser: FilePicker = Arc::new(move || {
        held.lock().ok()?.recv().ok()?;
        Some(path.clone())
    });
    let mut w = edit_window_with(WIDE, |app| app.with_stencil_picker(chooser));
    w.state_mut().execute("shapes.openStencil", &json!({})).unwrap();
    // Frames, with the dialog's thread held: nothing is open and the dialog is on the screen however many frames are drawn.
    w.rest_holding();
    w.rest_holding();
    assert!(w.state().stencil_dialog_open());
    assert!(opened_ids(&w).is_empty());
    release.send(()).unwrap();
    w.rest();
    assert!(!w.state().stencil_dialog_open());
    assert_eq!(opened_ids(&w), ["file-1"]);
    assert!(tile_names(&w).contains(&"Box".to_owned()), "and its tiles are drawn");
}

/// Open Stencil from the flyout: the dialog's answer is opened, the stencil has a row and is the one shown, with a tile for each master that is not hidden.
#[test]
fn open_stencil_opens_the_file_the_dialog_chose_and_shows_its_masters() {
    let dir = TempDir::new("open-stencil");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes"
        ]
    );
    open_from_the_flyout(&mut w);
    assert_eq!(opened_ids(&w), ["file-1"]);
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes",
            "Mine"
        ],
        "the stencil has a row, named for the file"
    );
    assert_eq!(w.state().window_state()["stencil"], "file-1", "and is the one shown");
    assert_eq!(tile_names(&w), ["Box", "Triangle", "Bad icon", "Empty"], "the masters in the file's order, and the hidden one left out");
    assert!(elements_named(&w, "shapes.tile.").iter().all(|(_, _, enabled)| *enabled));
    hit(&mut w, "shapes.more");
    assert!(w.state().element("shapes.stencilClose.Mine").is_some_and(|c| c.enabled), "a stencil file can be closed");
    assert!(elements_named(&w, "shapes.stencilClose.").len() == 1, "the engine's own stencils cannot");
    key(&mut w, Key::Escape, egui::Modifiers::NONE);
    assert_eq!(w.state().notice(), None);
    assert_eq!(w.state().ui_panics(), 0);
    // The same file again is the same stencil: nothing is added.
    open_from_the_flyout(&mut w);
    assert_eq!(opened_ids(&w), ["file-1"]);
    assert_eq!(rows(&mut w).len(), 8);
}

/// Open Stencil is quiet where there is no dialog, and the window says why when it is asked to anyway.
#[test]
fn open_stencil_is_quiet_without_a_dialog_and_a_command_with_a_path_still_works() {
    let dir = TempDir::new("open-stencil-quiet");
    let path = write_stencil(&dir, "Mine.vssx");
    let mut w = edit_window_with(WIDE, |app| app);
    hit(&mut w, "shapes.more");
    assert!(!w.state().element("shapes.more.open").expect("drawn").enabled);
    let refusal = w.state_mut().execute("shapes.openStencil", &json!({})).unwrap_err().to_string();
    assert!(refusal.contains("no file dialog"), "{refusal}");
    // With a path no dialog is needed: the control channel's way in.
    run_command(&mut w, "shapes.openStencil", json!({"path": path.to_str().unwrap()}));
    assert_eq!(opened_ids(&w), ["file-1"]);
    assert_eq!(w.state().window_state()["stencil"], "file-1");
    assert!(tile_names(&w).contains(&"Box".to_owned()));
}

/// A tile of a stencil file is dragged onto the page like a built-in one: the shape is dropped from that stencil, selected, and one undo step takes it off;
/// the drawing then holds the master it was made from.
#[test]
fn a_tile_of_a_stencil_file_drops_its_shape_and_the_drop_is_one_undo_step() {
    let dir = TempDir::new("drop-stencil");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    open_from_the_flyout(&mut w);
    assert!(shape_ids(&w).is_empty());
    drag_tile_to(&mut w, "Box", 4.0, 4.0);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1, "one shape was dropped");
    assert_eq!(w.state().selection(), ids.as_slice(), "and selected");
    let doc = w.state().session().document().unwrap();
    assert_eq!(doc.masters.iter().filter_map(|m| m.name.as_deref()).collect::<Vec<_>>(), ["Box"], "the drawing took the master with it");
    let (x, y, width, height) = crate::editing::frame_of(&w, ids[0]);
    assert!((width - 2.0).abs() < 1e-9 && (height - 1.0).abs() < 1e-9, "the master's size: {width} by {height}");
    assert!((x + width / 2.0 - 4.0).abs() < 1e-6 && (y + height / 2.0 - 4.0).abs() < 1e-6, "centred where it was dropped");
    // A click on a tile drops one too.
    hit(&mut w, "shapes.tile.Triangle");
    assert_eq!(shape_ids(&w).len(), 2);
    run_command(&mut w, "edit.undo", json!({}));
    run_command(&mut w, "edit.undo", json!({}));
    assert!(shape_ids(&w).is_empty(), "each drop was one undo step");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A tile is drawn from the master's icon when it has one the window reads, else from the master's own outline; a master with an icon of another kind
/// is drawn from its outline too, and one with no outline is a box.
#[test]
fn a_tile_is_drawn_from_the_icon_when_there_is_one_and_from_the_master_when_there_is_not() {
    let dir = TempDir::new("tile-art");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    open_from_the_flyout(&mut w);
    w.rest();
    let art = |w: &Window, master: &str| w.state().tile_art(Some("file-1"), master);
    assert_eq!(art(&w, "1"), Some("icon"), "Box has an icon, a 2 by 2 ICO");
    assert_eq!(art(&w, "2"), Some("outline"), "Triangle has none: drawn from its own outline");
    assert_eq!(art(&w, "4"), Some("outline"), "an icon of a kind the window does not read: the master is drawn");
    assert_eq!(art(&w, "5"), Some("box"), "Empty has no outline");
    assert_eq!(art(&w, "3"), None, "the hidden master has no tile");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A master the engine refuses is told in the status bar in the engine's words, nothing is dropped, and another tile still works.
#[test]
fn a_master_the_engine_refuses_is_told_in_the_status_bar() {
    let dir = TempDir::new("refused-stencil");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    open_from_the_flyout(&mut w);
    let revision = w.state().session().revision();
    drag_tile_to(&mut w, "Empty", 4.0, 4.0);
    assert_eq!(w.state().session().revision(), revision, "nothing was dropped");
    assert!(shape_ids(&w).is_empty());
    let notice = w.state().notice().map(str::to_owned);
    assert!(notice.as_deref().is_some_and(|n| n.contains("Empty") && n.contains("no shapes")), "{notice:?}");
    drag_tile_to(&mut w, "Triangle", 6.0, 4.0);
    assert_eq!(shape_ids(&w).len(), 1, "the next tile drops");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A file that is not a stencil is refused in words, and the list is as it was.
#[test]
fn a_file_that_is_not_a_stencil_is_told_in_words_and_the_list_is_unchanged() {
    let dir = TempDir::new("not-a-stencil");
    let drawing = dir.file("plan.vsdx");
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save_as", &json!({"path": drawing.to_str().unwrap()})).unwrap();
    }
    let mut w = window(drawing);
    open_from_the_flyout(&mut w);
    assert!(opened_ids(&w).is_empty());
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes"
        ]
    );
    let notice = w.state().notice().map(str::to_owned);
    assert!(notice.as_deref().is_some_and(|n| n.contains("not a stencil") || n.contains("stencil")), "{notice:?}");
    assert!(notice.as_deref().is_some_and(|n| !n.contains(dir.path().to_str().unwrap_or("/"))), "the path is not in the words: {notice:?}");
    // A file that is not there, and a dialog that chose nothing.
    let refusal = w.state_mut().open_stencil_file(&dir.file("nothing-here.vssx")).unwrap_err().to_string();
    assert!(!refusal.is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

/// The close control sends `stencil.close`: the row and the tiles go, the Shapes window shows the first stencil, and the drawing keeps the masters it took.
#[test]
fn the_close_control_closes_the_stencil_and_the_drawing_keeps_its_masters() {
    let dir = TempDir::new("close-stencil");
    let mut w = window(write_stencil(&dir, "Mine.vssx"));
    open_from_the_flyout(&mut w);
    drag_tile_to(&mut w, "Box", 4.0, 4.0);
    hit(&mut w, "shapes.stencilClose.Mine");
    assert!(opened_ids(&w).is_empty());
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes"
        ]
    );
    assert_eq!(w.state().window_state()["stencil"], "basic");
    assert!(tile_names(&w).contains(&"Rectangle".to_owned()));
    assert_eq!(shape_ids(&w).len(), 1, "the shape stays");
    assert_eq!(w.state().session().document().unwrap().masters.len(), 1, "and so does its master");
    // The window command, with the id, and its refusals in words.
    run_command(&mut w, "shapes.openStencil", json!({"path": dir.file("Mine.vssx").to_str().unwrap()}));
    assert_eq!(opened_ids(&w), ["file-2"], "an id is not given again");
    let refusal = w.state_mut().execute("shapes.closeStencil", &json!({"id": "basic"})).unwrap_err().to_string();
    assert!(refusal.contains("built-in"), "{refusal}");
    run_command(&mut w, "shapes.closeStencil", json!({"id": "file-2"}));
    assert!(opened_ids(&w).is_empty());
    assert_eq!(w.state().ui_panics(), 0);
}

/// A drawing read from a file is a new session: the stencils opened are opened again in it, by their paths, and the one that was shown is shown again;
/// a stencil whose file is gone is not in the list.
#[test]
fn the_stencils_are_kept_when_a_drawing_is_read_from_a_file() {
    let dir = TempDir::new("keep-stencils");
    let mine = write_stencil(&dir, "Mine.vssx");
    let other = write_stencil(&dir, "Other.vssx");
    let drawing = dir.file("plan.vsdx");
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save_as", &json!({"path": drawing.to_str().unwrap()})).unwrap();
    }
    let picks = Arc::new(std::sync::Mutex::new(vec![other.clone(), mine.clone()]));
    let stencil_picker: FilePicker = {
        let picks = Arc::clone(&picks);
        Arc::new(move || picks.lock().unwrap().pop())
    };
    let drawing_picker = picker(drawing.clone());
    let mut w = edit_window_with(WIDE, |app| app.with_stencil_picker(stencil_picker).with_file_picker(drawing_picker));
    open_from_the_flyout(&mut w);
    open_from_the_flyout(&mut w);
    assert_eq!(opened_ids(&w), ["file-1", "file-2"]);
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes",
            "Mine",
            "Other"
        ]
    );
    // Show the first of the two, then lose the second's file.
    hit(&mut w, "shapes.stencil.Mine");
    std::fs::remove_file(&other).unwrap();
    key(&mut w, Key::O, crate::saving::ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "plan.vsdx - DiaCraft", "the drawing was read");
    assert_eq!(opened_ids(&w), ["file-1"], "the stencil whose file is still there was opened again");
    // A stencil that does not open again is said, in words and by name, and not left out without a word (one near the budget dropped out unseen).
    let notice = w.state().notice().unwrap_or("no notice").to_owned();
    assert!(notice.contains("Other.vssx") && notice.contains("was not found") && notice.contains("not opened again"), "{notice}");
    assert!(!notice.contains("Mine.vssx") && !notice.contains("keep-stencils"), "only the one that was left out, and no folder: {notice}");
    assert_eq!(
        rows(&mut w),
        [
            "Basic Shapes",
            "Decorative Shapes",
            "Arrow Shapes",
            "Basic Flowchart",
            "Cross-Functional Flowchart Shapes",
            "Gane-Sarson",
            "Organization Chart Shapes",
            "Mine"
        ]
    );
    assert_eq!(w.state().window_state()["stencil"], "file-1", "and is the one shown");
    drag_tile_to(&mut w, "Triangle", 4.0, 4.0);
    assert_eq!(shape_ids(&w).len(), 1, "its tiles still drop");
    assert_eq!(w.state().tile_art(Some("file-1"), "2"), Some("outline"), "and are still drawn from the master");
    assert_eq!(w.state().ui_panics(), 0);
}

/// **A stencil opened through a link to its folder is kept as well**. The window kept the name of the file it was given and the engine the name
/// the link leads to, and after a drawing was read the window looked for the one under the other's name and did not find it: the stencil's tiles were
/// boxes, and at the next read it was not opened again. On macOS every temporary folder is under such a link (`/var` is `/private/var`), so the two
/// tests above failed there.
#[cfg(unix)]
#[test]
fn a_stencil_opened_through_a_link_to_its_folder_is_kept_when_a_drawing_is_read() {
    let dir = TempDir::new("linked-stencils");
    std::fs::create_dir_all(dir.path().join("real")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("link")).unwrap();
    let mine = dir.path().join("link").join("Mine.vssx");
    std::fs::write(&mine, stencil_bytes()).unwrap();
    let drawing = dir.file("plan.vsdx");
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save_as", &json!({"path": drawing.to_str().unwrap()})).unwrap();
    }
    let mut w = edit_window_with(WIDE, |app| app.with_stencil_picker(picker(mine.clone())).with_file_picker(picker(drawing.clone())));
    open_from_the_flyout(&mut w);
    assert_eq!(opened_ids(&w), ["file-1"]);
    hit(&mut w, "shapes.stencil.Mine");
    for read in 1..=2 {
        key(&mut w, Key::O, crate::saving::ctrl());
        w.rest();
        assert_eq!(w.state().window_title(), "plan.vsdx - DiaCraft", "read {read}: the drawing was read");
        assert_eq!(opened_ids(&w), ["file-1"], "read {read}: the stencil was opened again");
        assert_eq!(w.state().window_state()["stencil"], "file-1", "read {read}: and is the one shown");
        assert_eq!(w.state().tile_art(Some("file-1"), "2"), Some("outline"), "read {read}: its tiles are drawn from the master");
    }
    assert_eq!(w.state().ui_panics(), 0);
}

const SCRATCH_CHILD: &str = "DIACRAFT_UI_SCRATCH_CHILD";
const SCRATCH_MODE: &str = "DIACRAFT_UI_SCRATCH_MODE";

/// The child of the tests below: a stencil is open and no tile has been drawn, and the frames that draw the tiles meet what `SCRATCH_MODE` says. `read`: a file is being read (a
/// reservation of the budget is held, as the open thread's read holds it). `crowded`: and the room a stencil may read beside it is taken too. `gone`: no read, and the file of the
/// stencil is gone.
#[test]
#[ignore = "run by the tests below, in a process of its own: it holds the budget of a read"]
fn a_stencil_asked_for_while_a_file_is_read_child() {
    let (Ok(path), Ok(mode)) = (std::env::var(SCRATCH_CHILD), std::env::var(SCRATCH_MODE)) else { return };
    let mut w = window(PathBuf::from(&path));
    w.state_mut().execute("shapes.openStencil", &json!({"path": path})).unwrap();
    let (reading, beside) = match mode.as_str() {
        "read" | "past" => (Some(diacraft_engine::memory::reserve_read()), None),
        "crowded" => (Some(diacraft_engine::memory::reserve_read()), Some(diacraft_engine::memory::reserve_read_brief())),
        _ => (None, None),
    };
    if mode == "gone" {
        std::fs::remove_file(&path).unwrap();
    }
    w.rest();
    // The frames that go on while the read runs: the window is drawn and the tiles are asked for in each.
    for _ in 0..5 {
        w.step();
    }
    println!("\nSCRATCH while={:?} opens={}", w.state().tile_art(Some("file-1"), "2"), w.state().scratch_opens());
    println!("SCRATCH first={:?}", w.state().tile_art(Some("file-1"), "1"));
    drop((reading, beside));
    w.rest();
    println!("SCRATCH after={:?} opens={}", w.state().tile_art(Some("file-1"), "2"), w.state().scratch_opens());
    println!("SCRATCH first_after={:?}", w.state().tile_art(Some("file-1"), "1"));
}

/// A stencil of `n` masters of a typical shape each, which a read of it takes some 25 KB of memory for each: 6,000 are past the room a stencil is given beside a read that runs (about 4,500
/// are the most that fit it). **Each master's shape has an `ID` of its own**, and with it a sheet and a path of its own: masters of equal shapes would be one master to a reader that shares
/// what is equal, and the stencil is meant to be the most that a read of masters costs.
fn many_masters(n: u32) -> Vec<u8> {
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for k in 1..=n {
        builder = builder.master(MasterSpec::new(k, &format!("M{k}")).shapes(&diacraft_testkit::package::typical_shape(k)));
    }
    builder.build()
}

/// Runs the child in `mode` on a stencil file, and what it printed.
fn scratch_child(mode: &str) -> String {
    let dir = TempDir::new(&format!("scratch-{mode}"));
    let path = dir.file("Mine.vssx");
    std::fs::write(&path, if mode == "past" { many_masters(6_000) } else { stencil_bytes() }).unwrap();
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "stencil_files::a_stencil_asked_for_while_a_file_is_read_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(SCRATCH_CHILD, &path)
        .env(SCRATCH_MODE, mode)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "the child failed: {stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    stdout
}

/// **A tile that cannot be drawn from its stencil's file because the room a stencil may read is taken is drawn again when it is not**: the tiles of a stencil file
/// are drawn from a session of their own that reads the file again, and a read made while the open thread holds the budget and another stencil is being read beside it is refused. It was
/// remembered as a stencil that cannot be read, for good, and its tiles stayed boxes.
#[test]
fn the_tiles_of_a_stencil_asked_for_while_the_room_for_it_is_taken_are_drawn_when_it_is_free() {
    let stdout = scratch_child("crowded");
    assert!(
        stdout.contains("SCRATCH while=None"),
        "while the room is taken no figure is remembered for the tile (it is a box for the frame): {stdout}"
    );
    assert!(stdout.contains("SCRATCH after=Some(\"outline\")"), "and it is drawn from the master when the read has ended: {stdout}");
}

/// **A stencil past the room it is given beside a read that runs is read for its tiles once, and not in every frame**: `Scratch::open` runs `stencil.open` on the
/// window's thread, which loads the file and parses it until the room refuses it, and every tile of the stencil asked again in every frame: three frames in 4.7 seconds with a stencil
/// of 5,000 masters, with Cancel and Escape not heard until the read ended. The tiles are boxes meanwhile, and are drawn when the read has ended.
#[test]
fn a_stencil_past_the_room_beside_a_read_is_read_for_its_tiles_once_and_not_in_every_frame() {
    let stdout = scratch_child("past");
    let while_line = stdout.lines().find(|l| l.starts_with("SCRATCH while=")).unwrap_or("no line");
    assert!(while_line.ends_with(" opens=1"), "the file was read again once, and not again in the frames that followed: {stdout}");
    assert!(stdout.contains("SCRATCH first=None"), "no figure is remembered for the tile while the read runs: {stdout}");
    assert!(stdout.contains("SCRATCH first_after=Some(\"outline\")"), "and the tile is drawn from its master when the read has ended: {stdout}");
}

/// A stencil is read beside the read that holds the budget (the room a stencil may read), so its tiles are drawn while a file is being read, and not after.
#[test]
fn the_tiles_of_a_stencil_are_drawn_while_a_file_is_being_read() {
    let stdout = scratch_child("read");
    assert!(stdout.contains("SCRATCH while=Some(\"outline\")"), "drawn from the master while the read holds the budget: {stdout}");
    assert!(stdout.contains("SCRATCH after=Some(\"outline\")"), "{stdout}");
}

/// A stencil whose file is gone is remembered as one that cannot be read, and its tile is a box: it is not asked for again in every frame, as a stencil that waits for the room is.
#[test]
fn a_stencil_whose_file_is_gone_is_remembered_as_unreadable_and_its_tiles_are_boxes() {
    let stdout = scratch_child("gone");
    assert!(stdout.contains("SCRATCH while=Some(\"box\")"), "{stdout}");
    assert!(stdout.contains("SCRATCH after=Some(\"box\")"), "{stdout}");
}

/// **The answer of a `file.open` on the control channel says the stencils that did not open again** (`stencil_notes`: one sentence for each), as the window says them in its notice.
#[test]
fn the_answer_of_an_open_on_the_control_channel_lists_the_stencils_that_were_not_opened_again() {
    let dir = TempDir::new("answer-notes");
    let (mine, other) = (write_stencil(&dir, "Mine.vssx"), write_stencil(&dir, "Other.vssx"));
    let drawing = dir.file("plan.vsdx");
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save_as", &json!({"path": drawing.to_str().unwrap()})).unwrap();
    }
    let (mut w, tx) = crate::support::controlled_with(|app| app);
    for path in [&mine, &other] {
        w.state_mut().execute("shapes.openStencil", &json!({"path": path.to_str().unwrap()})).unwrap();
    }
    let open = |w: &mut Window| {
        let answer = crate::support::send(&tx, "engine.execute", json!({"command": "file.open", "params": {"path": drawing.to_str().unwrap()}}));
        let started = std::time::Instant::now();
        loop {
            w.step();
            if let Ok(reply) = answer.try_recv() {
                break reply.expect("the drawing opens");
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(60), "the file was not opened in 60 s");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    // Every stencil opens again: the answer says nothing of them.
    let reply = open(&mut w);
    assert!(reply.get("stencil_notes").is_none(), "no stencil was left out: {reply}");
    std::fs::remove_file(&other).unwrap();
    let reply = open(&mut w);
    let notes = reply["stencil_notes"].as_array().expect("the answer lists the stencils that were not opened again");
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = notes[0].as_str().unwrap();
    assert!(note.contains("Other.vssx") && note.contains("was not found") && !note.contains("Mine.vssx") && !note.contains("answer-notes"), "{note}");
}

/// The Shapes window with a stencil file open at narrow and wide windows: the row's name does not run under its close control, the control is inside
/// the row, and no tile is wider than the window.
#[test]
fn the_row_of_a_stencil_file_fits_at_narrow_and_wide_windows() {
    let dir = TempDir::new("fit-stencil");
    let long = "A stencil with a long name that will not fit in the window";
    let path = write_stencil(&dir, &format!("{long}.vssx"));
    let mut w = window(path);
    open_from_the_flyout(&mut w);
    for size in [[480.0, 700.0], [800.0, 700.0], [1280.0, 900.0], [1900.0, 1000.0]] {
        w.set_size(egui::vec2(size[0], size[1]));
        w.rest();
        hit(&mut w, "shapes.more");
        let row = w.state().element(&format!("shapes.stencil.{long}")).expect("the row is drawn").rect;
        let close = w.state().element(&format!("shapes.stencilClose.{long}")).expect("the close control is drawn").rect;
        assert!(row.contains_rect(close), "{size:?}: the close control {close:?} is inside the row {row:?}");
        assert!(close.width() >= 16.0 && close.height() >= 16.0, "{size:?}: big enough to hit: {close:?}");
        key(&mut w, Key::Escape, egui::Modifiers::NONE);
        let chooser = w.state().element("shapes.more").unwrap().rect;
        for (name, rect, _) in elements_named(&w, "shapes.tile.") {
            assert!(rect.right() <= chooser.right() + 1.0, "{size:?}: {name} {rect:?} is inside the Shapes window {chooser:?}");
        }
    }
    assert_eq!(w.state().ui_panics(), 0);
}

/// A stencil of hundreds of masters reads the figures of the tiles in view and no more.
#[test]
fn a_stencil_of_hundreds_of_masters_reads_only_the_tiles_in_view() {
    let dir = TempDir::new("big-stencil");
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for n in 1..=300_u32 {
        builder = builder.master(MasterSpec::new(n, &format!("M{n}")).shapes(&shape(5, &triangle_cells(1.0, 1.0))));
    }
    let path = dir.file("Many.vssx");
    std::fs::write(&path, builder.build()).unwrap();
    let mut w = window(path);
    open_from_the_flyout(&mut w);
    assert_eq!(tile_names(&w).len(), 300, "every master has a tile, found by name");
    let read = w.state().tiles_read();
    assert!(read > 0 && read < 60, "only the tiles in view were read: {read}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The two window commands are known, listed with the others, and take the parameters their words name.
#[test]
fn the_window_commands_are_known_and_refuse_what_they_do_not_take() {
    let mut w = edit_window_with(WIDE, |app| app);
    for id in ["shapes.openStencil", "shapes.closeStencil"] {
        assert!(diacraft_ui_egui::commands::known(id), "{id}");
    }
    let refused = w.state_mut().execute("shapes.openStencil", &json!({"file": "x"})).unwrap_err().to_string();
    assert!(refused.contains("unknown parameter") && refused.contains("path"), "{refused}");
    let refused = w.state_mut().execute("shapes.closeStencil", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("id"), "{refused}");
    let refused = w.state_mut().execute("shapes.closeStencil", &json!({"id": "file-9"})).unwrap_err().to_string();
    assert!(refused.contains("no stencil"), "{refused}");
    let refused = w.state_mut().execute("shapes.openStencil", &json!({"path": 5})).unwrap_err().to_string();
    assert!(refused.contains("text"), "{refused}");
}

/// A 32 by 32 icon: a PNG inside an ICO, a coloured disc with a lighter ring, for the demonstration stencil.
fn demo_icon(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            let d = ((x - 16) * (x - 16) + (y - 16) * (y - 16)) as f64;
            let pixel = if d < 100.0 {
                [r, g, b, 255]
            } else if d < 169.0 {
                [r / 2 + 100, g / 2 + 100, b / 2 + 100, 255]
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&pixel);
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 32, 32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    let mut out = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
    out.extend_from_slice(&u32::try_from(png.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&png);
    out
}

/// Writes a stencil where the run of the real window can open it: `DIACRAFT_DEMO_DIR` names the folder. Not a test unless asked for.
#[test]
#[ignore = "writes the demonstration stencil to the folder in DIACRAFT_DEMO_DIR, for opening it in the running window"]
fn write_the_demo_stencil() {
    let dir = PathBuf::from(std::env::var("DIACRAFT_DEMO_DIR").expect("DIACRAFT_DEMO_DIR names the folder to write to"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut builder = VsdxBuilder::new().document_type(CT_STENCIL);
    for (n, (name, icon)) in
        [("Server", Some((200, 90, 40))), ("Client", Some((50, 120, 200))), ("Database", Some((60, 160, 90)))].into_iter().enumerate()
    {
        let id = u32::try_from(n).unwrap() + 1;
        let mut m = MasterSpec::new(id, name).shapes(&shape(5, &box_cells(1.5, 1.0)));
        if let Some((r, g, b)) = icon {
            m = m.icon(&base64(&demo_icon(r, g, b)));
        }
        builder = builder.master(m);
    }
    builder = builder.master(MasterSpec::new(4, "Wedge").shapes(&shape(5, &triangle_cells(1.0, 1.0))));
    builder = builder.master(MasterSpec::new(5, "Backstage").attrs(r#" Hidden="1""#).shapes(&shape(5, &box_cells(1.0, 1.0))));
    builder = builder.master(MasterSpec::new(6, "Nothing"));
    std::fs::write(dir.join("Network.vssx"), builder.build()).unwrap();
    std::fs::write(dir.join("notastencil.vssx"), b"this is not a stencil file").unwrap();
    eprintln!("wrote the stencil to {}", dir.display());
}

/// Semantic 1D metadata does not manufacture a right-arrow preview for imported artwork.
#[test]
fn imported_plain_left_arrow_and_vertical_lines_keep_their_own_outline() {
    use diacraft_ui_egui::stencils::{Scratch, parse};
    let dir = TempDir::new("imported-line-preview");
    for (name, angle, ex, ey, begin_arrow) in
        [("Plain", 0.0, 1.5, 0.0, 0), ("Left", 0.0, 1.5, 0.0, 4), ("Vertical", std::f64::consts::FRAC_PI_2, 0.0, 1.5, 0)]
    {
        let cells = format!(
            r#"<Cell N="Width" V="1.5"/><Cell N="Height" V="0"/>
<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="{ex}"/><Cell N="EndY" V="{ey}"/>
<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/><Cell N="LocPinX" V="0.75"/><Cell N="LocPinY" V="0"/>
<Cell N="Angle" V="{angle}"/><Cell N="BeginArrow" V="{begin_arrow}"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="1.5" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#,
            ex / 2.0,
            ey / 2.0
        );
        let bytes = VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, name).shapes(&shape(5, &cells))).build();
        let path = dir.file(&format!("{name}.vssx"));
        std::fs::write(&path, bytes).unwrap();
        let mut session = Session::new();
        session.execute("stencil.open", &json!({"path":path})).unwrap();
        let list = parse(&session.execute("stencil.list", &json!({})).unwrap());
        let master = &list.iter().find(|stencil| !stencil.builtin).unwrap().masters[0];
        assert!(master.one_d);
        assert!(!master.directed_preview());
        let figure = Scratch::open(&path).unwrap().figure(master).unwrap();
        assert!(figure.areas.is_empty());
        assert_eq!(figure.lines.len(), 1, "{name}: no right-arrow head was added");
        assert_eq!(figure.lines[0].len(), 2);
    }
}
