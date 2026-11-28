//! Automation roots on the control channel (gaps A2 and A5): a window started with `--control` and `--automation-read-root` / `--automation-write-root` keeps the files its
//! control requests reach to the folders granted, and does not keep the person to them. Every request that reads a file or writes one is tried: the engine's commands through
//! `engine.execute`, the window's own (`window.insertPicture`, `shapes.openStencil`, `print.start`), `ui.screenshot`, and the files read on a thread of their own.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_engine::file_access::AutomationRoots;
use diacraft_ui_egui::{ControlRequest, DiacraftApp};
use egui::{Color32, ColorImage, ViewportCommand};
use serde_json::{Value, json};

use crate::stencil_files::write_stencil;
use crate::support::{Rest, TempDir, Window, call, commands, controlled_with, send};

/// A folder with `read`, `write` and `outside`, a drawing, a stencil and a picture in each, and the roots over the first two.
struct World {
    dir: TempDir,
    roots: Arc<AutomationRoots>,
}

fn png(path: &PathBuf) {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&[160u8; 8 * 8 * 4]).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

impl World {
    fn new() -> World {
        let dir = TempDir::new("roots");
        for folder in ["read", "write", "outside"] {
            std::fs::create_dir_all(dir.file(folder)).unwrap();
        }
        let roots = AutomationRoots::new(&[dir.file("read").as_path()], &[dir.file("write").as_path()]).unwrap().unwrap();
        let world = World { dir, roots };
        for folder in ["read", "write", "outside"] {
            let mut session = Session::new();
            session.execute("file.new", &json!({})).unwrap();
            session.execute("file.save_as", &json!({"path": world.p(folder, "drawing.diacraft")})).unwrap();
            session.execute("file.save_as", &json!({"path": world.p(folder, "template.diacraft-template")})).unwrap();
            let stencil = write_stencil(&world.dir, &format!("{folder}-stencil.vssx"));
            std::fs::rename(&stencil, world.dir.file(folder).join("stencil.vssx")).unwrap();
            png(&world.dir.file(folder).join("pic.png"));
        }
        world
    }

    fn p(&self, folder: &str, name: &str) -> String {
        self.dir.file(folder).join(name).to_str().unwrap().to_owned()
    }

    fn window(&self) -> (Window, Sender<ControlRequest>) {
        let roots = self.roots.clone();
        controlled_with(move |app| app.with_automation_roots(Some(roots)))
    }

    fn exists(&self, folder: &str, name: &str) -> bool {
        self.dir.file(folder).join(name).exists()
    }
}

/// Runs a request to its answer, stepping the window as long as it takes (a file is read on a thread of its own).
fn answer(window: &mut Window, tx: &Sender<ControlRequest>, method: &str, params: Value) -> Result<Value, String> {
    let reply = send(tx, method, params);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        window.step();
        if let Ok(reply) = reply.try_recv() {
            return reply;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("`{method}` was not answered in 30 s");
}

fn execute(window: &mut Window, tx: &Sender<ControlRequest>, command: &str, params: Value) -> Result<Value, String> {
    answer(window, tx, "engine.execute", json!({"command": command, "params": params}))
}

/// A refused request: its words name the rule and the flag and hold nothing of the path.
fn rule(world: &World, error: &str, flag: &str) {
    assert!(error.contains("automation") && error.contains(flag), "{error}");
    assert!(!error.contains(&*world.dir.path().to_string_lossy()), "holds the path: {error}");
}

fn revision(window: &Window) -> u64 {
    window.state().session().revision()
}

#[test]
fn a_drawing_is_read_through_the_channel_only_inside_a_read_root() {
    let world = World::new();
    let (mut window, tx) = world.window();
    let opened = execute(&mut window, &tx, "file.open", json!({"path": world.p("read", "drawing.diacraft")})).unwrap();
    assert_eq!(opened["file"]["variant"], "diacraft", "{opened}");
    let before = revision(&window);
    for folder in ["outside", "write"] {
        let error = execute(&mut window, &tx, "file.open", json!({"path": world.p(folder, "drawing.diacraft")})).unwrap_err();
        rule(&world, &error, "--automation-read-root");
    }
    let up = format!("{}/../outside/drawing.diacraft", world.p("read", ""));
    rule(&world, &execute(&mut window, &tx, "file.open", json!({"path": up})).unwrap_err(), "--automation-read-root");
    assert_eq!(revision(&window), before, "a refused open leaves the drawing as it was");
    assert_eq!(window.state().ui_panics(), 0);
}

#[test]
fn a_template_file_a_stencil_and_a_picture_are_read_only_inside_a_read_root() {
    let world = World::new();
    let (mut window, tx) = world.window();
    execute(&mut window, &tx, "file.new", json!({"template": world.p("read", "template.diacraft-template")})).unwrap();
    rule(
        &world,
        &execute(&mut window, &tx, "file.new", json!({"template": world.p("outside", "template.diacraft-template")})).unwrap_err(),
        "--automation-read-root",
    );
    execute(&mut window, &tx, "file.new", json!({})).unwrap();
    assert!(execute(&mut window, &tx, "stencil.open", json!({"path": world.p("read", "stencil.vssx")})).is_ok());
    rule(
        &world,
        &execute(&mut window, &tx, "stencil.open", json!({"path": world.p("outside", "stencil.vssx")})).unwrap_err(),
        "--automation-read-root",
    );
    assert!(execute(&mut window, &tx, "insert.picture", json!({"path": world.p("read", "pic.png")})).is_ok());
    rule(&world, &execute(&mut window, &tx, "insert.picture", json!({"path": world.p("outside", "pic.png")})).unwrap_err(), "--automation-read-root");
    // The window's own commands that read a path.
    assert!(execute(&mut window, &tx, "window.insertPicture", json!({"path": world.p("read", "pic.png")})).is_ok());
    rule(
        &world,
        &execute(&mut window, &tx, "window.insertPicture", json!({"path": world.p("outside", "pic.png")})).unwrap_err(),
        "--automation-read-root",
    );
    assert!(execute(&mut window, &tx, "shapes.openStencil", json!({"path": world.p("read", "stencil.vssx")})).is_ok());
    rule(
        &world,
        &execute(&mut window, &tx, "shapes.openStencil", json!({"path": world.p("outside", "stencil.vssx")})).unwrap_err(),
        "--automation-read-root",
    );
    assert_eq!(window.state().ui_panics(), 0);
}

#[test]
fn a_drawing_is_written_through_the_channel_only_inside_a_write_root() {
    let world = World::new();
    let (mut window, tx) = world.window();
    execute(&mut window, &tx, "file.new", json!({})).unwrap();
    execute(&mut window, &tx, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3.0, "y": 3.0})).unwrap();
    assert!(execute(&mut window, &tx, "file.save_as", json!({"path": world.p("write", "a.vsdx")})).is_ok());
    assert!(world.exists("write", "a.vsdx"));
    for folder in ["outside", "read"] {
        let error = execute(&mut window, &tx, "file.save_as", json!({"path": world.p(folder, "b.vsdx")})).unwrap_err();
        rule(&world, &error, "--automation-write-root");
        assert!(!world.exists(folder, "b.vsdx"), "nothing was written in {folder}");
    }
    // The file that is there is not replaced.
    let before = std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap();
    rule(
        &world,
        &execute(&mut window, &tx, "file.save_as", json!({"path": world.p("outside", "drawing.diacraft")})).unwrap_err(),
        "--automation-write-root",
    );
    assert_eq!(std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(), before);
    // A picture, a print and a dry run.
    for (command, name) in [("file.export", "p.png"), ("file.export", "p.svg"), ("file.print", "p.pdf")] {
        assert!(execute(&mut window, &tx, command, json!({"path": world.p("write", name)})).is_ok(), "{command} {name}");
        assert!(world.exists("write", name));
        rule(&world, &execute(&mut window, &tx, command, json!({"path": world.p("outside", name)})).unwrap_err(), "--automation-write-root");
        assert!(!world.exists("outside", name), "{command} {name} wrote outside");
    }
    assert!(execute(&mut window, &tx, "file.check_compatibility", json!({"path": world.p("write", "c.vsdx")})).is_ok());
    rule(
        &world,
        &execute(&mut window, &tx, "file.check_compatibility", json!({"path": world.p("outside", "c.vsdx")})).unwrap_err(),
        "--automation-write-root",
    );
    assert_eq!(window.state().ui_panics(), 0);
}

/// `file.save` writes the drawing's own file, wherever the person opened it from: outside a write root a program does not write it.
#[test]
fn file_save_is_refused_when_the_drawings_own_file_is_outside_the_write_roots() {
    let world = World::new();
    let (mut window, tx) = world.window();
    // The person opened it (the window's own open, which the roots do not keep), and a program asks for the save.
    window.state_mut().execute("file.new", &json!({})).unwrap();
    let ctx = window.ctx.clone();
    window.state_mut().open_path(&ctx, world.dir.file("outside").join("drawing.diacraft"));
    wait_for_the_open(&mut window);
    execute(&mut window, &tx, "page.add", json!({"name": "Edited"})).unwrap();
    let before = std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap();
    rule(&world, &execute(&mut window, &tx, "file.save", json!({})).unwrap_err(), "--automation-write-root");
    assert_eq!(std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(), before, "the person's file is as it was");
}

fn wait_for_the_open(window: &mut Window) {
    let started = Instant::now();
    loop {
        window.step();
        if window.state().opening_now().is_none() {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "the file was not opened in 30 s");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A print to a PDF that a program names is written on a thread of its own, and the roots of the request are in force there.
#[test]
fn a_print_to_a_pdf_a_program_names_is_kept_to_the_write_roots_on_its_thread_too() {
    let world = World::new();
    let (mut window, tx) = world.window();
    execute(&mut window, &tx, "file.new", json!({})).unwrap();
    execute(&mut window, &tx, "window.backstage", json!({"page": "Print"})).unwrap();
    execute(&mut window, &tx, "print.set", json!({"printer": "pdf"})).unwrap();
    let outcome = |window: &mut Window| -> String {
        let started = Instant::now();
        loop {
            window.step();
            let state = window.state().window_state();
            if state["print"]["busy"] == json!(false) && state["print"]["last"].is_string() {
                return state["print"]["last"].as_str().unwrap().to_owned();
            }
            assert!(started.elapsed() < Duration::from_secs(30), "the print did not end: {state}");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    execute(&mut window, &tx, "print.start", json!({"path": world.p("outside", "print.pdf")})).unwrap();
    let said = outcome(&mut window);
    assert!(said.contains("not printed") && said.contains("automation"), "{said}");
    assert!(!world.exists("outside", "print.pdf"), "nothing was written outside");
    execute(&mut window, &tx, "print.start", json!({"path": world.p("write", "print.pdf")})).unwrap();
    let said = outcome(&mut window);
    assert!(!said.contains("not printed"), "{said}");
    assert!(world.exists("write", "print.pdf"), "inside a write root it is written: {said}");
}

fn capture_request(window: &mut Window) -> egui::ScreenshotCallback {
    for _ in 0..20 {
        window.step();
        if let Some(callback) = commands(window).into_iter().find_map(|c| if let ViewportCommand::Screenshot(cb) = c { Some(cb) } else { None }) {
            return callback;
        }
    }
    panic!("the window never asked for a screenshot")
}

#[test]
fn a_screenshot_is_a_file_written_and_is_kept_to_the_write_roots() {
    let world = World::new();
    let (mut window, tx) = world.window();
    // Outside a write root it is refused when it is asked for, before any frame is captured.
    for folder in ["outside", "read"] {
        let error = call(&mut window, &tx, "ui.screenshot", json!({"path": world.p(folder, "shot.png")})).unwrap_err();
        rule(&world, &error, "--automation-write-root");
        assert!(!world.exists(folder, "shot.png"));
    }
    assert!(commands(&window).iter().all(|c| !matches!(c, ViewportCommand::Screenshot(_))), "no capture was asked for");
    // Inside, a frame is written.
    let reply = send(&tx, "ui.screenshot", json!({"path": world.p("write", "shot.png")}));
    let callback = capture_request(&mut window);
    callback.complete(Arc::new(ColorImage::new([2, 2], vec![Color32::from_rgb(1, 2, 3); 4])));
    for _ in 0..10 {
        window.step();
        if let Ok(done) = reply.try_recv() {
            assert_eq!(done.unwrap()["width"], 2);
            assert!(world.exists("write", "shot.png"));
            return;
        }
    }
    panic!("the screenshot was never answered");
}

#[test]
fn the_person_at_the_window_is_not_kept_to_the_roots() {
    let world = World::new();
    let (mut window, tx) = world.window();
    // The window's own open (a dialog's answer, a file named on the command line, a drop) reads a file outside every root, on a thread that has no roots.
    let ctx = window.ctx.clone();
    window.state_mut().open_path(&ctx, world.dir.file("outside").join("drawing.diacraft"));
    wait_for_the_open(&mut window);
    assert_eq!(window.state().session().path().map(|p| p.file_name().unwrap().to_owned()), Some("drawing.diacraft".into()));
    assert!(window.state().open_error().is_none(), "{:?}", window.state().open_error());
    // The window's own save, in the window's own commands.
    window.state_mut().execute("file.save_as", &json!({"path": world.p("outside", "mine.diacraft")})).unwrap();
    assert!(world.exists("outside", "mine.diacraft"));
    // And the control channel, on the same window, is kept.
    rule(
        &world,
        &execute(&mut window, &tx, "file.save_as", json!({"path": world.p("outside", "theirs.diacraft")})).unwrap_err(),
        "--automation-write-root",
    );
}

/// A stencil the person opened, outside every root, is opened again in the drawing a program has the window read: the window's own work with its own paths is not the program's.
#[test]
fn the_stencils_the_person_opened_are_opened_again_when_a_program_has_a_drawing_read() {
    let world = World::new();
    let (mut window, tx) = world.window();
    window.state_mut().execute("file.new", &json!({})).unwrap();
    window.state_mut().execute("shapes.openStencil", &json!({"path": world.p("outside", "stencil.vssx")})).unwrap();
    let opened = |window: &Window| window.state().session().stencils().iter().filter(|s| s.id().starts_with("file-")).count();
    assert_eq!(opened(&window), 1);
    let read = execute(&mut window, &tx, "file.open", json!({"path": world.p("read", "drawing.diacraft")})).unwrap();
    assert!(read.get("stencil_notes").is_none(), "the stencil was opened again: {read}");
    assert_eq!(opened(&window), 1, "and is open in the drawing that was read");
}

#[test]
fn the_window_says_which_roots_keep_the_channel() {
    let world = World::new();
    let (mut window, tx) = world.window();
    let state = call(&mut window, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(state["automation"]["read"], json!([world.roots.read_roots()[0]]), "{state}");
    assert_eq!(state["automation"]["write"], json!([world.roots.write_roots()[0]]), "{state}");
    // A window with no roots says none: any path the person can reach.
    let (mut open, tx) = controlled_with(|app: DiacraftApp| app);
    let state = call(&mut open, &tx, "ui.inspect", json!({})).unwrap();
    assert_eq!(state["automation"], Value::Null, "{state}");
}

#[cfg(unix)]
#[test]
fn a_link_in_a_root_that_leads_out_is_not_followed_by_the_channel() {
    let world = World::new();
    std::os::unix::fs::symlink(world.dir.file("outside").join("drawing.diacraft"), world.dir.file("read").join("leak.diacraft")).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside"), world.dir.file("write").join("away")).unwrap();
    let (mut window, tx) = world.window();
    rule(&world, &execute(&mut window, &tx, "file.open", json!({"path": world.p("read", "leak.diacraft")})).unwrap_err(), "--automation-read-root");
    execute(&mut window, &tx, "file.new", json!({})).unwrap();
    rule(
        &world,
        &execute(&mut window, &tx, "file.save_as", json!({"path": world.p("write", "away/x.diacraft")})).unwrap_err(),
        "--automation-write-root",
    );
    assert!(!world.exists("outside", "x.diacraft"));
    rule(&world, &call(&mut window, &tx, "ui.screenshot", json!({"path": world.p("write", "away/s.png")})).unwrap_err(), "--automation-write-root");
}

// ---- input sent through the channel, and what is answered later (the second security review) -------------------------------------------------------

/// The person opens `path` in the window, the way a dialog's answer does: on a thread that has no roots.
fn open_as_the_person(window: &mut Window, path: PathBuf) {
    let ctx = window.ctx.clone();
    window.state_mut().open_path(&ctx, path);
    wait_for_the_open(window);
    assert!(window.state().open_error().is_none(), "{:?}", window.state().open_error());
}

fn frames(window: &mut Window, n: usize) {
    for _ in 0..n {
        window.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Hands the input the channel queued to the window a frame at a time, as the desktop app's `raw_input_hook` does, until none is left.
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

fn centre_of(window: &Window, name: &str) -> egui::Pos2 {
    window.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center()
}

/// Input the channel sends is the channel's: a key that saves, pressed through `ui.key`, is consumed in a frame after the request, and that frame is under the request's roots.
#[test]
fn a_save_shortcut_sent_through_the_channel_is_kept_to_the_write_roots() {
    let world = World::new();
    let (mut window, tx) = world.window();
    open_as_the_person(&mut window, world.dir.file("outside").join("drawing.diacraft"));
    execute(&mut window, &tx, "page.add", json!({"name": "Edited"})).unwrap();
    let before = std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap();
    call(&mut window, &tx, "ui.key", json!({"key": "S", "ctrl": true})).unwrap();
    pump(&mut window);
    assert_eq!(
        std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(),
        before,
        "Ctrl+S, pressed by a program, wrote the person's file outside the roots"
    );
    let said = window.state().open_error().unwrap_or_default().to_owned();
    assert!(said.contains("not saved") && said.contains("automation"), "{said}");
    // The same press by the person (the window's own input, not the channel's) saves: the roots are not the person's.
    window.state_mut().dismiss_open_error();
    let pos = centre_of(&window, "qat.Save");
    crate::support::click(&mut window, pos);
    frames(&mut window, 10);
    assert_ne!(std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(), before, "the person's own Save is not kept to the roots");
}

/// A question the person raised and a program answers: the answer is the program's, and is kept.
#[test]
fn the_replace_prompt_answered_through_the_channel_is_kept_to_the_write_roots() {
    let world = World::new();
    let plain = world.dir.file("outside").join("plain");
    std::fs::write(world.dir.file("outside").join("plain.diacraft"), b"the person's own file").unwrap();
    let chosen = plain.clone();
    let picker: diacraft_ui_egui::SavePicker = Arc::new(move |_| Some(chosen.clone()));
    let roots = world.roots.clone();
    let (mut window, tx) = controlled_with(move |app| app.with_automation_roots(Some(roots)).with_save_picker(picker));
    execute(&mut window, &tx, "file.new", json!({})).unwrap();
    // The person saves under a name with no type, for a file that is there once the type is added: the window asks before it replaces it.
    let ctx = window.ctx.clone();
    window.state_mut().save_as(&ctx);
    let started = Instant::now();
    while !window.state().replace_prompt_open() {
        frames(&mut window, 1);
        assert!(started.elapsed() < Duration::from_secs(30), "the question was not asked");
    }
    // The card is let to settle where it stands, and a program answers it.
    window.rest();
    call(&mut window, &tx, "ui.click", json!({"element": "replace.replace"})).unwrap();
    pump(&mut window);
    assert_eq!(
        std::fs::read(world.dir.file("outside").join("plain.diacraft")).unwrap(),
        b"the person's own file".to_vec(),
        "a program replaced a file outside the roots"
    );
    let said = window.state().open_error().unwrap_or_default().to_owned();
    assert!(said.contains("automation"), "{said}");
}

/// A question a program raised and the person answers: the answer is under the roots the question was raised under, and a place that has become another since is refused.
#[cfg(unix)]
#[test]
fn a_card_the_channel_raised_keeps_its_roots_when_the_person_answers_it() {
    let world = World::new();
    let (mut window, tx) = world.window();
    open_as_the_person(&mut window, world.dir.file("write").join("drawing.diacraft"));
    execute(&mut window, &tx, "page.add", json!({"name": "Edited"})).unwrap();
    // Something else changed the file on the disk, so a save asks first.
    let mut changed = std::fs::read(world.dir.file("write").join("drawing.diacraft")).unwrap();
    changed.push(0);
    std::fs::write(world.dir.file("write").join("drawing.diacraft"), changed).unwrap();
    call(&mut window, &tx, "ui.key", json!({"key": "S", "ctrl": true})).unwrap();
    pump(&mut window);
    assert!(window.state().changed_prompt_open(), "the save asked about the file that changed: {:?}", window.state().open_error());
    // The folder is turned into a link to one outside the roots, and the person answers: Overwrite.
    std::fs::rename(world.dir.file("write"), world.dir.file("write-moved")).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside"), world.dir.file("write")).unwrap();
    let before = std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap();
    let pos = centre_of(&window, "changed.overwrite");
    crate::support::click(&mut window, pos);
    frames(&mut window, 10);
    assert_eq!(
        std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(),
        before,
        "the answer wrote outside the roots the question was raised under"
    );
    let said = window.state().open_error().unwrap_or_default().to_owned();
    assert!(said.contains("automation"), "{said}");
}

/// Cards do not share their roots: a card the person raises afterwards, with no roots, does not loosen the roots a card the channel raised is waiting under.
#[cfg(unix)]
#[test]
fn a_card_the_person_raises_later_does_not_loosen_the_roots_of_one_the_channel_raised() {
    let world = World::new();
    // The name the person will save under has no type, and a file of that name with the type is there: a Save As raises the replace card, with no roots.
    let plain = world.dir.file("outside").join("plain");
    std::fs::write(world.dir.file("outside").join("plain.diacraft"), b"the person's own file").unwrap();
    let chosen = plain.clone();
    let picker: diacraft_ui_egui::SavePicker = Arc::new(move |_| Some(chosen.clone()));
    let roots = world.roots.clone();
    let (mut window, tx) = controlled_with(move |app| app.with_automation_roots(Some(roots)).with_save_picker(picker));
    open_as_the_person(&mut window, world.dir.file("write").join("drawing.diacraft"));
    execute(&mut window, &tx, "page.add", json!({"name": "Edited"})).unwrap();
    // The file changes on the disk, and a program presses Ctrl+S: the card about a changed file is raised under the roots.
    let mut changed = std::fs::read(world.dir.file("write").join("drawing.diacraft")).unwrap();
    changed.push(0);
    std::fs::write(world.dir.file("write").join("drawing.diacraft"), changed).unwrap();
    call(&mut window, &tx, "ui.key", json!({"key": "S", "ctrl": true})).unwrap();
    pump(&mut window);
    assert!(window.state().changed_prompt_open(), "{:?}", window.state().open_error());
    // Meanwhile the person raises another card, from the window: Save As, under a name that is there.
    let ctx = window.ctx.clone();
    window.state_mut().save_as(&ctx);
    let started = Instant::now();
    while !window.state().replace_prompt_open() {
        frames(&mut window, 1);
        assert!(started.elapsed() < Duration::from_secs(30), "the second question was not asked");
    }
    // The folder is turned into a link to one outside the roots, and the person answers the first card: Overwrite.
    std::fs::rename(world.dir.file("write"), world.dir.file("write-moved")).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside"), world.dir.file("write")).unwrap();
    let before = std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap();
    window.rest();
    let pos = centre_of(&window, "changed.overwrite");
    crate::support::click(&mut window, pos);
    frames(&mut window, 10);
    assert_eq!(
        std::fs::read(world.dir.file("outside").join("drawing.diacraft")).unwrap(),
        before,
        "the answer ran under the roots of the card the person raised afterwards, and wrote outside the roots"
    );
    let said = window.state().open_error().unwrap_or_default().to_owned();
    assert!(said.contains("automation"), "{said}");
}

/// A screenshot is written a frame after it was asked for: the file is judged again then, under the request's roots.
#[cfg(unix)]
#[test]
fn a_screenshot_whose_folder_becomes_a_link_before_the_frame_is_written_is_refused() {
    let world = World::new();
    let (mut window, tx) = world.window();
    std::fs::create_dir_all(world.dir.file("write").join("sub")).unwrap();
    let reply = send(&tx, "ui.screenshot", json!({"path": world.p("write", "sub/shot.png")}));
    let callback = capture_request(&mut window);
    std::fs::remove_dir(world.dir.file("write").join("sub")).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside"), world.dir.file("write").join("sub")).unwrap();
    callback.complete(Arc::new(ColorImage::new([2, 2], vec![Color32::from_rgb(1, 2, 3); 4])));
    for _ in 0..10 {
        window.step();
        if let Ok(done) = reply.try_recv() {
            let error = done.unwrap_err();
            assert!(error.contains("automation"), "{error}");
            assert!(!world.exists("outside", "shot.png"), "the frame was written where the link leads");
            return;
        }
    }
    panic!("the screenshot was never answered");
}

/// The name is free when the screenshot is asked for, and a link to another file in the write root stands at it when the frame is written: the frame is not written through the
/// link (that would replace the other file in the name's stead).
#[cfg(unix)]
#[test]
fn a_screenshot_whose_name_becomes_a_link_before_the_frame_is_written_is_refused() {
    let world = World::new();
    let (mut window, tx) = world.window();
    std::fs::write(world.dir.file("write").join("other.png"), b"keep").unwrap();
    let reply = send(&tx, "ui.screenshot", json!({"path": world.p("write", "shot.png")}));
    let callback = capture_request(&mut window);
    std::os::unix::fs::symlink(world.dir.file("write").join("other.png"), world.dir.file("write").join("shot.png")).unwrap();
    callback.complete(Arc::new(ColorImage::new([2, 2], vec![Color32::from_rgb(1, 2, 3); 4])));
    for _ in 0..10 {
        window.step();
        if let Ok(done) = reply.try_recv() {
            let error = done.unwrap_err();
            assert!(error.contains("automation"), "{error}");
            assert_eq!(std::fs::read(world.dir.file("write").join("other.png")).unwrap(), b"keep", "the frame was written through the link");
            return;
        }
    }
    panic!("the screenshot was never answered");
}

/// A stencil a request opened is opened again, in the drawing a request has read, under that request's roots: a link put in its place since, to a file outside, is refused.
#[cfg(unix)]
#[test]
fn a_stencil_a_request_opened_is_opened_again_under_its_roots() {
    let world = World::new();
    let (mut window, tx) = world.window();
    execute(&mut window, &tx, "shapes.openStencil", json!({"path": world.p("read", "stencil.vssx")})).unwrap();
    let opened = |window: &Window| window.state().session().stencils().iter().filter(|s| s.id().starts_with("file-")).count();
    assert_eq!(opened(&window), 1);
    // As it was: it is opened again.
    let read = execute(&mut window, &tx, "file.open", json!({"path": world.p("read", "drawing.diacraft")})).unwrap();
    assert!(read.get("stencil_notes").is_none(), "{read}");
    assert_eq!(opened(&window), 1);
    // A link to a file outside stands where it was.
    std::fs::remove_file(world.dir.file("read").join("stencil.vssx")).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside").join("stencil.vssx"), world.dir.file("read").join("stencil.vssx")).unwrap();
    let read = execute(&mut window, &tx, "file.open", json!({"path": world.p("read", "drawing.diacraft")})).unwrap();
    let notes = read["stencil_notes"].to_string();
    assert!(notes.contains("outside the folders") && notes.contains("not opened again"), "the stencil was opened again through the link: {read}");
    assert_eq!(opened(&window), 0, "and it is not open in the drawing that was read");
}

/// The tiles of a stencil are drawn from the file read again: under the roots of the request that opened it, and under none for the person's.
#[cfg(unix)]
#[test]
fn the_tiles_of_a_stencil_are_read_again_under_the_roots_it_was_opened_under() {
    use diacraft_engine::file_access::{ScopeHandle, confine};
    let world = World::new();
    let stencil = world.dir.file("read").join("stencil.vssx");
    let request = confine(Some(&world.roots), ScopeHandle::capture);
    let person = ScopeHandle::capture();
    assert!(diacraft_ui_egui::stencils::Scratch::open(&stencil, &request).is_some());
    std::fs::remove_file(&stencil).unwrap();
    std::os::unix::fs::symlink(world.dir.file("outside").join("stencil.vssx"), &stencil).unwrap();
    assert!(
        diacraft_ui_egui::stencils::Scratch::open(&stencil, &request).is_none(),
        "a link to a file outside is not read again for a request's stencil"
    );
    assert!(diacraft_ui_egui::stencils::Scratch::open(&stencil, &person).is_some(), "and is for the person's");
}
