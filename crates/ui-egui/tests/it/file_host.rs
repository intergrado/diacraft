//! The window with a host for files (the browser's way: no dialog of the desktop's, no thread, files held in the engine's memory), driven as a person drives it, with a host of the test's own that
//! stands in for the page. What these tests hold is the window's part: a file chosen is opened, what is saved, exported or printed is handed over, a drop opens, a refusal is told in words and the drawing that
//! was open stays, and no file touches the disk. The page's own code (a file input, a download, the File System Access API) is run in a browser (`apps/diacraft-web`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use diacraft_engine::memfs::{self, Keep};
use diacraft_ui_egui::{Delivered, FileHost, SaveRequest, Want, place};
use egui::{Context, Event, Key, Modifiers};
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key, key_without_waiting};
use crate::fixture;
use crate::saving::ctrl;
use crate::support::{Rest, Window, click, press, run_command};

/// Memory files for this thread while a test runs, and nothing left in the store after it.
struct Memory;

impl Memory {
    fn on() -> Memory {
        memfs::use_on_this_thread(true);
        memfs::clear();
        Memory
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        memfs::use_on_this_thread(false);
    }
}

/// A file the person picks: its name and its bytes.
type Picked = (String, Vec<u8>);

/// The page, as a test stands in for it.
#[derive(Default)]
struct Page {
    /// What the next choice answers: a file (name and bytes), or `None` for a chooser the person closed.
    picks: Mutex<VecDeque<Option<Picked>>>,
    wants: Mutex<Vec<Want>>,
    /// Whether the page has a dialog of its own to name a file to save (the File System Access API), and the names its next answers give.
    names_saves: bool,
    save_names: Mutex<VecDeque<Option<String>>>,
    asked: Mutex<Vec<SaveRequest>>,
    /// Files handed over: the name and the bytes.
    delivered: Mutex<Vec<(String, Vec<u8>)>>,
    /// Files dropped on the page, waiting for the window to ask.
    drops: Mutex<Vec<(String, Vec<u8>)>>,
    refusals: Mutex<Vec<String>>,
    released: Mutex<Vec<PathBuf>>,
    /// A reason the next delivery fails with.
    fail: Mutex<Option<String>>,
}

impl Page {
    fn new() -> Arc<Page> {
        Arc::new(Page::default())
    }

    fn naming() -> Arc<Page> {
        Arc::new(Page { names_saves: true, ..Page::default() })
    }

    fn will_pick(&self, name: &str, bytes: Vec<u8>) {
        self.picks.lock().unwrap().push_back(Some((name.to_owned(), bytes)));
    }

    fn delivered(&self) -> Vec<(String, Vec<u8>)> {
        self.delivered.lock().unwrap().clone()
    }

    fn delivered_names(&self) -> Vec<String> {
        self.delivered.lock().unwrap().iter().map(|(n, _)| n.clone()).collect()
    }
}

impl FileHost for Page {
    fn choose(&self, ctx: &Context, want: Want) -> Receiver<Option<PathBuf>> {
        self.wants.lock().unwrap().push(want);
        let (tx, rx) = channel();
        let answer = match self.picks.lock().unwrap().pop_front().flatten() {
            Some((name, bytes)) => {
                let path = place(&name);
                memfs::put(&path, bytes, if want == Want::Stencil { Keep::Held } else { Keep::Once }).unwrap();
                Some(path)
            }
            None => None,
        };
        tx.send(answer).unwrap();
        ctx.request_repaint();
        rx
    }

    fn names_saves(&self) -> bool {
        self.names_saves
    }

    fn choose_save(&self, ctx: &Context, request: &SaveRequest) -> Receiver<Option<PathBuf>> {
        self.asked.lock().unwrap().push(request.clone());
        let (tx, rx) = channel();
        let name = self.save_names.lock().unwrap().pop_front().flatten();
        tx.send(name.map(|n| place(&n))).unwrap();
        ctx.request_repaint();
        rx
    }

    fn deliver(&self, ctx: &Context, path: &Path) -> Receiver<Result<Delivered, String>> {
        let (tx, rx) = channel();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let answer = match (self.fail.lock().unwrap().take(), memfs::take(path)) {
            (Some(why), Some(bytes)) => {
                self.delivered.lock().unwrap().push((name, bytes));
                Ok(Delivered::Fallback(why))
            }
            (_, Some(bytes)) => {
                self.delivered.lock().unwrap().push((name, bytes));
                Ok(Delivered::Downloaded)
            }
            (_, None) => Err("there was nothing to hand over.".to_owned()),
        };
        tx.send(answer).unwrap();
        ctx.request_repaint();
        rx
    }

    fn dropped(&self) -> Vec<PathBuf> {
        let drops = std::mem::take(&mut *self.drops.lock().unwrap());
        drops
            .into_iter()
            .map(|(name, bytes)| {
                let path = place(&name);
                memfs::put(&path, bytes, Keep::Once).unwrap();
                path
            })
            .collect()
    }

    fn refusals(&self) -> Vec<String> {
        std::mem::take(&mut *self.refusals.lock().unwrap())
    }

    fn release(&self, path: &Path) {
        self.released.lock().unwrap().push(path.to_path_buf());
        memfs::remove(path);
    }
}

/// A window on a new drawing whose files go through `page`.
fn window_with(page: &Arc<Page>) -> Window {
    let host: Arc<dyn FileHost> = Arc::clone(page) as Arc<dyn FileHost>;
    edit_window_with([1280.0, 900.0], |app| app.with_file_host(host))
}

fn open_dialog(w: &mut Window) {
    key_without_waiting(w, Key::O, ctrl());
    w.rest();
}

fn page_names(w: &Window) -> Vec<String> {
    w.state().session().document().map(|d| d.pages.iter().map(|p| p.name.to_string()).collect()).unwrap_or_default()
}

/// A drawing the engine makes and saves in memory as `name` (the type by its extension), and the bytes it wrote.
fn engine_file(name: &str, pages: &[&str]) -> Vec<u8> {
    let mut session = diacraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    for page in pages {
        session.execute("page.add", &json!({ "name": page })).unwrap();
    }
    let path = place(name);
    session.execute("file.save_as", &json!({ "path": path.to_str().unwrap() })).unwrap();
    memfs::take(&path).unwrap()
}

/// A PNG the engine's own export makes of a new drawing.
fn engine_png() -> Vec<u8> {
    let mut session = diacraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 2, "y": 2})).unwrap();
    let path = place("made.png");
    session.execute("file.export", &json!({ "path": path.to_str().unwrap() })).unwrap();
    memfs::take(&path).unwrap()
}

#[test]
fn the_file_controls_work_with_a_host_and_are_quiet_without_one() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    for entry in ["Open", "Save", "Save As", "Export", "Print"] {
        let element = w.state().element(&format!("backstage.{entry}")).unwrap_or_else(|| panic!("the File view has {entry}"));
        assert!(element.enabled, "{entry} works where the page has a host");
    }
    assert!(
        w.state().can_pick_file()
            && w.state().can_save_as()
            && w.state().can_export()
            && w.state().can_pick_stencil()
            && w.state().can_pick_picture()
    );
    let mut bare = edit_window_with([1280.0, 900.0], |app| app);
    run_command(&mut bare, "window.backstage", json!({"page": "Home"}));
    assert!(!bare.state().element("backstage.Open").unwrap().enabled, "a window with no host and no dialog cannot choose a file");
    assert!(!bare.state().can_save_as());
}

#[test]
fn a_file_the_page_chose_is_opened_on_the_pages_thread_after_the_progress_card_was_drawn() {
    let _memory = Memory::on();
    let page = Page::new();
    page.will_pick("plans.vsdx", fixture::two_pages_vsdx());
    let mut w = window_with(&page);
    key_without_waiting(&mut w, Key::O, ctrl());
    // The chooser answered, and the window took the answer: the file is being read, and the card that says so is on the screen before the read holds the page.
    let mut frames = 0;
    while w.state().opening_now().is_none() && frames < 20 {
        w.step();
        frames += 1;
    }
    assert_eq!(w.state().opening_now().map(|(name, _)| name), Some("plans.vsdx"));
    assert!(page_names(&w) != ["One", "Two"], "the file has not been read yet: {:?}", page_names(&w));
    w.step();
    assert!(w.state().element("opening.cancel").is_some(), "the progress card is drawn, with its Cancel");
    w.rest();
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert_eq!(w.state().session().path(), page.released.lock().unwrap().last().map(PathBuf::as_path), "it is the drawing's own file now");
    assert_eq!(*page.wants.lock().unwrap(), [Want::Drawing]);
    assert_eq!(memfs::entries(), 0, "the file is in the engine, and the store holds nothing of it");
    assert_eq!(w.state().window_title(), "plans.vsdx - DiaCraft", "the drawing has the file's name");
    assert!(!w.state().is_dirty());
}

#[test]
fn a_chooser_the_person_closed_changes_nothing_and_the_open_control_works_again() {
    let _memory = Memory::on();
    let page = Page::new();
    page.picks.lock().unwrap().push_back(None);
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let before = w.state().session().revision();
    open_dialog(&mut w);
    assert!(w.state().open_error().is_none() && w.state().opening_now().is_none() && !w.state().prompt_open());
    assert_eq!(w.state().session().revision(), before);
    assert!(w.state().can_pick_file(), "the control is not stuck");
}

#[test]
fn what_the_page_could_not_take_is_said_in_its_words_and_the_drawing_stays() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let before = w.state().session().revision();
    page.refusals.lock().unwrap().push("big.vsdx is too big to open safely: it holds more than DiaCraft will read.".to_owned());
    w.step();
    assert_eq!(w.state().open_error(), Some("big.vsdx is too big to open safely: it holds more than DiaCraft will read."));
    assert_eq!(w.state().session().revision(), before);
}

#[test]
fn a_hostile_file_is_refused_in_words_and_the_drawing_that_was_open_is_still_open() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let shapes = w.state().session().document().map(|d| d.pages.iter().map(|p| p.shapes.len()).sum::<usize>());
    // Not a zip, a package cut short, and a package that claims more than it holds: each is refused, and none is a crash.
    let mut cut = fixture::demo_vsdx();
    cut.truncate(cut.len() / 3);
    for (name, bytes) in
        [("junk.vsdx", b"this is not a package".to_vec()), ("cut.vsdx", cut), ("empty.odg", Vec::new()), ("zeros.diacraft", vec![0; 4096])]
    {
        page.will_pick(name, bytes);
        // The unsaved drawing asks first, and the person says to go on.
        open_dialog(&mut w);
        if w.state().prompt_open() {
            press(&mut w, "prompt.dont_save");
            w.rest();
        }
        let words = w.state().open_error().unwrap_or_else(|| panic!("{name} was not refused")).to_owned();
        assert!(words.starts_with(name) && words.contains("still open"), "{name}: {words}");
        assert!(!words.contains("/web/") && !words.contains("file.open") && !words.contains("::"), "{name}: {words}");
        assert_eq!(memfs::entries(), 0, "{name}: what the page kept of it is let go");
        assert_eq!(w.state().ui_panics(), 0);
        let now = w.state().session().document().map(|d| d.pages.iter().map(|p| p.shapes.len()).sum::<usize>());
        assert_eq!(now, shapes, "{name}: the drawing is as it was");
    }
}

#[test]
fn a_drawing_opened_edited_and_saved_is_handed_over_and_opens_again() {
    let _memory = Memory::on();
    let page = Page::new();
    page.will_pick("plans.vsdx", fixture::two_pages_vsdx());
    let mut w = window_with(&page);
    open_dialog(&mut w);
    run_command(&mut w, "page.rename", json!({"id": 1, "name": "Renamed"}));
    assert!(w.state().is_dirty());
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "the engine saved it");
    assert_eq!(page.delivered_names(), ["plans.vsdx"], "the file the drawing came from is the file handed over");
    assert!(page.delivered()[0].1.starts_with(b"PK"));
    let said = w.state().notice().unwrap_or_default().to_owned();
    assert!(said.starts_with("Saved plans.vsdx. Your browser has downloaded the file."), "{said}");
    assert_eq!(memfs::entries(), 0, "nothing is left in the store");

    // A second save hands over the next file: the engine does not find the first one changed on the disk.
    run_command(&mut w, "page.rename", json!({"id": 2, "name": "Again"}));
    key(&mut w, Key::S, ctrl());
    assert_eq!(page.delivered_names(), ["plans.vsdx", "plans.vsdx"]);
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());

    // What was handed over is what a drawing is read as: opened again, it is the edited drawing.
    let (_, bytes) = page.delivered().pop().unwrap();
    page.will_pick("again.vsdx", bytes);
    open_dialog(&mut w);
    assert_eq!(page_names(&w), ["Renamed", "Again"]);
}

#[test]
fn a_new_drawing_is_named_by_a_card_and_saved_as_the_kind_the_person_chose() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    // No file yet, and the page has no dialog of its own: the window asks for the name.
    assert!(w.state().name_card_open(), "the card is up");
    assert!(page.delivered().is_empty());
    for kind in ["diacraft", "odg", "vsdx"] {
        assert!(w.state().element(&format!("name_card.kind.{kind}")).is_some(), "the card lists {kind}");
    }
    assert!(w.state().element("name_card.name").is_some() && w.state().element("name_card.cancel").is_some());
    // The first kind is the drawing's own, and the name starts as the dialog's would: the drawing's name with its type.
    press(&mut w, "name_card.kind.vsdx");
    w.rest();
    press(&mut w, "name_card.save");
    w.rest();
    assert!(!w.state().name_card_open());
    assert_eq!(page.delivered_names(), ["Drawing1.vsdx"]);
    assert!(page.delivered()[0].1.starts_with(b"PK"));
    assert_eq!(w.state().window_title(), "Drawing1.vsdx - DiaCraft");
    assert_eq!(w.state().notice(), Some("Saved Drawing1.vsdx. Your browser has downloaded the file."));
}

#[test]
fn the_name_card_is_cancelled_with_escape_and_a_name_can_be_typed() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(w.state().name_card_open());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().name_card_open() && page.delivered().is_empty());
    assert!(w.state().session().path().is_none(), "nothing was saved");

    key(&mut w, Key::S, ctrl());
    let field = w.state().element("name_card.name").unwrap().rect.center();
    click(&mut w, field);
    w.input_mut().events.push(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: ctrl() });
    w.step();
    w.input_mut().events.push(Event::Text("My plan".to_owned()));
    w.step();
    press(&mut w, "name_card.save");
    w.rest();
    // A name with no type has the kind that was chosen: the drawing's own, `.diacraft`.
    assert_eq!(page.delivered_names(), ["My plan.diacraft"]);
}

#[test]
fn a_page_with_a_dialog_of_its_own_is_asked_with_the_kinds_and_its_answer_names_the_file() {
    let _memory = Memory::on();
    let page = Page::naming();
    page.save_names.lock().unwrap().push_back(Some("chosen.odg".to_owned()));
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().name_card_open(), "the page asks, and the window has no card");
    let asked = page.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].name, "Drawing1.diacraft");
    let kinds: Vec<&str> = asked[0].kinds.iter().map(|k| k.extension).collect();
    assert_eq!(kinds[0], "diacraft", "the drawing's own kind first");
    assert!(kinds.contains(&"vsdx") && kinds.contains(&"odg"), "{kinds:?}");
    assert_eq!(page.delivered_names(), ["chosen.odg"]);
    assert!(page.delivered()[0].1.starts_with(b"PK"), "an .odg is a package");

    // A person who closes the page's dialog saves nothing.
    page.save_names.lock().unwrap().push_back(None);
    key(&mut w, Key::S, Modifiers { shift: true, ..ctrl() });
    assert_eq!(page.delivered_names(), ["chosen.odg"]);
}

#[test]
fn a_delivery_that_could_not_be_written_where_the_person_chose_is_said_and_the_file_is_downloaded() {
    let _memory = Memory::on();
    let page = Page::new();
    page.will_pick("plans.vsdx", fixture::two_pages_vsdx());
    let mut w = window_with(&page);
    open_dialog(&mut w);
    run_command(&mut w, "page.rename", json!({"id": 1, "name": "Renamed"}));
    *page.fail.lock().unwrap() = Some("the file is read-only".to_owned());
    key(&mut w, Key::S, ctrl());
    let said = w.state().notice().unwrap_or_default().to_owned();
    assert!(said.starts_with("Saved plans.vsdx. Your browser has downloaded the file."), "{said}");
    // The reason is too long for the status bar: it is said over the drawing.
    let words = w.state().open_error().unwrap_or_default().to_owned();
    assert_eq!(words, "The file could not be written where you chose (the file is read-only), so your browser downloaded it instead.");
    assert_eq!(page.delivered_names(), ["plans.vsdx"]);
}

#[test]
fn an_export_is_named_by_the_card_and_handed_over_as_the_kind_chosen() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(w.state().name_card_open());
    for kind in ["png", "svg", "pdf"] {
        assert!(w.state().element(&format!("name_card.kind.{kind}")).is_some(), "{kind}");
    }
    assert!(w.state().element("name_card.kind.vsdx").is_none(), "a picture is no drawing file");
    press(&mut w, "name_card.kind.svg");
    w.rest();
    press(&mut w, "name_card.save");
    w.rest();
    assert_eq!(page.delivered_names(), ["Drawing1.svg"]);
    assert!(page.delivered()[0].1.starts_with(b"<?xml"));
    assert_eq!(w.state().notice(), Some("Exported Drawing1.svg. Your browser has downloaded the file."));
    assert!(!w.state().is_dirty() || w.state().session().path().is_none(), "an export is no save");
    assert!(w.state().session().path().is_none(), "and gives the drawing no file");

    // A PNG and a PDF, by the type of the name typed.
    for (kind, magic) in [("png", &b"\x89PNG"[..]), ("pdf", b"%PDF")] {
        run_command(&mut w, "window.backstage", json!({"page": "Home"}));
        press(&mut w, "backstage.Export");
        w.rest();
        press(&mut w, &format!("name_card.kind.{kind}"));
        w.rest();
        press(&mut w, "name_card.save");
        w.rest();
        let (name, bytes) = page.delivered().pop().unwrap();
        assert_eq!(name, format!("Drawing1.{kind}"));
        assert!(bytes.starts_with(magic), "{kind}");
    }
    assert_eq!(memfs::entries(), 0);
}

#[test]
fn what_would_be_printed_is_saved_as_a_pdf_and_handed_over() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    run_command(&mut w, "window.backstage", json!({"page": "Print"}));
    w.rest();
    assert_eq!(w.state().window_state()["print"]["destination"], "pdf", "a window with no printer service offers the PDF");
    assert!(w.state().element("print.start").is_some_and(|e| e.enabled), "Save as PDF works where the page has a host");
    press(&mut w, "print.start");
    w.rest();
    assert!(w.state().name_card_open(), "the card names the PDF");
    assert!(w.state().element("name_card.name").is_some(), "the name is asked for, and the one kind needs no list");
    assert!(w.state().element("name_card.kind.pdf").is_none());
    press(&mut w, "name_card.save");
    w.rest();
    assert_eq!(page.delivered_names(), ["Drawing1.pdf"]);
    assert!(page.delivered()[0].1.starts_with(b"%PDF"));
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
}

#[test]
fn a_file_dropped_on_the_page_opens_and_a_drawing_with_changes_is_asked_about_first() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    page.drops.lock().unwrap().push(("dropped.vsdx".to_owned(), fixture::two_pages_vsdx()));
    w.step();
    w.rest();
    assert!(w.state().prompt_open(), "the drawing has changes: the question comes first");
    assert_eq!(memfs::entries(), 1, "the file waits in the store for the answer");
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert!(w.state().open_error().is_none(), "{:?} / {:?} / {:?}", w.state().open_error(), w.state().notice(), w.state().opening_now());
    assert_eq!(page_names(&w), ["One", "Two"]);
    assert_eq!(w.state().window_title(), "dropped.vsdx - DiaCraft");
    assert_eq!(memfs::entries(), 0);

    // A drop while another is being read or a question is up is told, and let go.
    page.drops.lock().unwrap().push(("second.vsdx".to_owned(), fixture::two_pages_vsdx()));
    run_command(&mut w, "page.rename", json!({"id": 1, "name": "Changed"}));
    page.drops.lock().unwrap().push(("third.vsdx".to_owned(), fixture::two_pages_vsdx()));
    w.step();
    w.rest();
    assert!(w.state().prompt_open());
    let released_before = page.released.lock().unwrap().len();
    page.drops.lock().unwrap().push(("fourth.vsdx".to_owned(), fixture::two_pages_vsdx()));
    w.step();
    assert!(w.state().notice().is_some_and(|n| n.contains("was not opened: answer the question on the screen")), "{:?}", w.state().notice());
    assert!(page.released.lock().unwrap().len() > released_before, "the page lets go of the file that was not opened");
    press(&mut w, "prompt.cancel");
    w.rest();
    assert_eq!(page_names(&w), ["Changed", "Two"], "cancelling leaves the drawing as it was");
}

#[test]
fn more_than_one_file_dropped_opens_the_first_and_says_so() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    page.drops.lock().unwrap().push(("a.vsdx".to_owned(), fixture::two_pages_vsdx()));
    page.drops.lock().unwrap().push(("b.vsdx".to_owned(), fixture::two_pages_vsdx()));
    w.step();
    w.rest();
    assert_eq!(w.state().window_title(), "a.vsdx - DiaCraft");
    assert_eq!(memfs::entries(), 0, "the second was let go");
    assert!(page.released.lock().unwrap().iter().any(|p| p.ends_with("b.vsdx")));
}

#[test]
fn a_stencil_is_kept_in_memory_while_it_is_open_and_opens_again_in_the_next_drawing() {
    let _memory = Memory::on();
    let page = Page::new();
    let stencil = engine_file("shapes.vssx", &[]);
    page.will_pick("shapes.vssx", stencil);
    let mut w = window_with(&page);
    run_command(&mut w, "shapes.openStencil", json!({}));
    w.rest();
    assert_eq!(*page.wants.lock().unwrap(), [Want::Stencil]);
    let ids: Vec<String> = w.state().session().stencils().iter().map(|s| s.id().to_owned()).collect();
    assert!(ids.iter().any(|id| id.starts_with("file-")), "the stencil file is open: {ids:?}");
    assert_eq!(memfs::entries(), 1, "a stencil that is open stays in the store, read by copy");
    // Another drawing is read in a session of its own, and the stencil is opened again in it from the store.
    page.will_pick("other.vsdx", fixture::two_pages_vsdx());
    open_dialog(&mut w);
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(w.state().session().stencils().iter().any(|s| s.id().starts_with("file-")), "the stencil is still open in the new drawing's session");
    assert_eq!(memfs::entries(), 1, "and still held");
    // Closing it lets go of the file.
    let id = w.state().session().stencils().iter().map(|s| s.id().to_owned()).find(|id| id.starts_with("file-")).expect("the stencil file is open");
    run_command(&mut w, "stencil.close", json!({ "id": id }));
    assert_eq!(memfs::entries(), 0, "closed: let go");
}

#[test]
fn a_picture_is_chosen_through_the_page_and_inserted() {
    let _memory = Memory::on();
    let page = Page::new();
    let mut w = window_with(&page);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    page.will_pick("pic.png", engine_png());
    let before = w.state().session().document().map(|d| d.pages.iter().map(|p| p.shapes.len()).sum::<usize>()).unwrap();
    run_command(&mut w, "window.insertPicture", json!({}));
    w.rest();
    assert_eq!(*page.wants.lock().unwrap(), [Want::Picture]);
    let after = w.state().session().document().map(|d| d.pages.iter().map(|p| p.shapes.len()).sum::<usize>()).unwrap();
    assert_eq!(after, before + 1, "the picture is a shape of the page");
    assert_eq!(memfs::entries(), 0);
}
