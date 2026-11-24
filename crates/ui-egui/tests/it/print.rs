//! Print and Print Preview in the window: the Print view of the File view, with its settings at the left and the sheets at the right
//! The job that sends the PDF of a print to a printer or to a file, and what the window never does: hold a frame for the
//! spooler, change the drawing, or show a command's name.
//!
//! The system's printing is the desktop app's, so a **stand-in service** takes its place here (`Fake`): it lists printers when it is let, keeps the jobs
//! it is given, can be held or made to fail, and nothing in these tests reaches a real printer. The native Save dialog of a PDF is a closure, as the
//! file dialogs are in the other tests. Every wait is for the thing itself (`Rest::rest` waits while a thread of the window has an answer to give), never
//! for time.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::{DiacraftApp, Duplex, ExportPicker, PrintCancel, PrintJob, PrintService, PrinterInfo};
use egui::{Key, Modifiers, Rect};
use serde_json::{Value, json};

use crate::editing::key;
use crate::support::{Rest, TempDir, Window, elements_named, fast_window, fast_window_with, press, run_command};

// ---- the stand-ins ------------------------------------------------------------------------------------------------------------------------

/// A door a test holds shut and opens, that a thread waits at for at most twenty seconds (so that a test that forgets to open it fails and does not
/// hang).
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn shut() -> Arc<Gate> {
        Arc::new(Gate::default())
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn wait(&self) {
        let open = self.open.lock().unwrap();
        let _ = self.changed.wait_timeout_while(open, Duration::from_secs(20), |open| !*open).unwrap();
    }
}

/// The system's printing, faked.
#[derive(Default)]
struct Fake {
    printers: Vec<PrinterInfo>,
    /// `printers` waits at this.
    listing: Option<Arc<Gate>>,
    /// `submit` waits at this, and a cancel opens it.
    submitting: Option<Arc<Gate>>,
    /// `submit` fails with this.
    refuses: Option<String>,
    sent: Mutex<Vec<(Vec<u8>, PrintJob)>>,
    listed: AtomicBool,
}

impl PrintService for Fake {
    fn printers(&self) -> Vec<PrinterInfo> {
        if let Some(gate) = &self.listing {
            gate.wait();
        }
        self.listed.store(true, Ordering::SeqCst);
        self.printers.clone()
    }

    fn submit(&self, pdf: &[u8], job: &PrintJob, cancel: &PrintCancel) -> Result<String, String> {
        if let Some(gate) = &self.submitting {
            let opened = Arc::clone(gate);
            cancel.on_cancel(move || opened.release());
            gate.wait();
        }
        if cancel.is_cancelled() {
            return Err("the print job was cancelled before anything was sent to the printer".to_owned());
        }
        if let Some(why) = &self.refuses {
            return Err(why.clone());
        }
        let mut sent = self.sent.lock().unwrap();
        sent.push((pdf.to_vec(), job.clone()));
        Ok(format!("request id is Office-{}", sent.len()))
    }
}

impl Fake {
    fn jobs(&self) -> Vec<(Vec<u8>, PrintJob)> {
        self.sent.lock().unwrap().clone()
    }
}

fn printer(name: &str, label: &str, default: bool) -> PrinterInfo {
    PrinterInfo { name: name.to_owned(), label: label.to_owned(), default }
}

fn office() -> Fake {
    Fake { printers: vec![printer("Office", "Office laser", true), printer("Plotter", "", false)], ..Fake::default() }
}

/// How many pages a PDF written by the print has: its page objects, which the writer spells without a space.
fn pdf_pages(bytes: &[u8]) -> usize {
    bytes.windows(11).filter(|w| *w == b"/Type/Page/").count()
}

// ---- the drawings ---------------------------------------------------------------------------------------------------------------------------

fn blank() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session
}

fn cells(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(n, v)| format!(r#"<Cell N="{n}" V="{v}"/>"#)).collect()
}

/// The cells of a page that is one sheet of Letter sideways, as a new page is.
const SHEET: [(&str, &str); 6] = [
    ("PageWidth", "11"),
    ("PageHeight", "8.5"),
    ("PageLeftMargin", "0"),
    ("PageRightMargin", "0"),
    ("PageTopMargin", "0"),
    ("PageBottomMargin", "0"),
];

/// A page of 25.5 by 11 inches held upright on Letter with no margins: three sheets across.
const THREE_ACROSS: [(&str, &str); 7] = [
    ("PageWidth", "25.5"),
    ("PageHeight", "11"),
    ("PrintPageOrientation", "1"),
    ("PageLeftMargin", "0"),
    ("PageRightMargin", "0"),
    ("PageTopMargin", "0"),
    ("PageBottomMargin", "0"),
];

/// A drawing read from a file whose pages carry these cells; a page is `(name, cells)`.
fn drawing(dir: &TempDir, pages: &[(&str, &[(&str, &str)])]) -> Session {
    let mut builder = VsdxBuilder::new();
    for (i, (name, extra)) in pages.iter().enumerate() {
        let mut all = vec![("PageScale", "1"), ("DrawingScale", "1")];
        all.extend_from_slice(extra);
        builder = builder.page(PageSpec::new(i as u32 + 1, name).sheet(&cells(&all)));
    }
    std::fs::write(dir.file("print.vsdx"), builder.build()).unwrap();
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": dir.file("print.vsdx").to_str().unwrap()})).unwrap();
    session
}

fn window_on(session: Session, size: [f32; 2], service: Option<Arc<Fake>>, picker: Option<ExportPicker>) -> Window {
    let mut app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer());
    if let Some(service) = service {
        app = app.with_print_service(service);
    }
    if let Some(picker) = picker {
        app = app.with_print_pdf_picker(picker);
    }
    let mut w = fast_window(app, size);
    w.rest();
    w
}

fn open_print(w: &mut Window) {
    run_command(w, "window.backstage", json!({"page": "Print"}));
}

/// Opens the Print view for a test that holds the printers' list on purpose (a shut `Gate`): the window is rested as it is, and not until the thread answers.
fn open_print_holding(w: &mut Window) {
    w.state_mut().execute("window.backstage", &json!({"page": "Print"})).unwrap();
    w.rest_holding();
}

fn state(w: &Window) -> Value {
    w.state().window_state()["print"].clone()
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// A Save dialog that answers `path` and remembers the names it was asked to start with.
fn picker(path: Option<PathBuf>, asked: &Arc<Mutex<Vec<String>>>) -> ExportPicker {
    let asked = Arc::clone(asked);
    Arc::new(move |suggestion| {
        asked.lock().unwrap().push(suggestion);
        path.clone()
    })
}

/// What a print must leave as it was: the document, its history, its revision and whether it is unsaved.
fn untouched(w: &Window) -> (Option<diacraft_engine::doc::Document>, Option<(usize, usize)>, u64, bool) {
    let app = w.state();
    (app.session().document().cloned(), app.session().history().map(|h| (h.undo_len(), h.redo_len())), app.session().revision(), app.is_dirty())
}

// ---- the view -----------------------------------------------------------------------------------------------------------------------------

#[test]
fn ctrl_p_shows_the_print_view_and_the_rail_has_a_print_entry_after_save_as() {
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    // Print stands after Save As and before Export in the rail.
    let (save_as, print, export) = (rect_of(&w, "backstage.Save As"), rect_of(&w, "backstage.Print"), rect_of(&w, "backstage.Export"));
    assert!(save_as.top() < print.top() && print.top() < export.top(), "{save_as:?} {print:?} {export:?}");
    assert!(enabled(&w, "backstage.Print"));
    press(&mut w, "backstage.Print");
    w.rest();
    assert_eq!(w.state().window_state()["backstage"], "Print");
    // And Ctrl+P from the drawing, or from another page of the File view, shows it.
    run_command(&mut w, "window.backstage", json!({"page": "close"}));
    assert_eq!(w.state().window_state()["backstage"], Value::Null);
    key(&mut w, Key::P, Modifiers::COMMAND);
    assert_eq!(w.state().window_state()["backstage"], "Print");
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    key(&mut w, Key::P, Modifiers::COMMAND);
    assert_eq!(w.state().window_state()["backstage"], "Print");
    // Escape leaves it.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["backstage"], Value::Null);
}

#[test]
fn with_no_drawing_the_entry_is_dim_and_the_view_cannot_be_shown() {
    let mut w = window_on(Session::new(), [1280.0, 800.0], Some(Arc::new(office())), None);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    assert!(!enabled(&w, "backstage.Print"));
    let error = w.state_mut().execute("window.backstage", &json!({"page": "Print"})).unwrap_err();
    assert!(error.to_string().contains("no document open"), "{error}");
    key(&mut w, Key::P, Modifiers::COMMAND);
    assert_eq!(w.state().window_state()["backstage"], "Home", "Ctrl+P shows nothing where there is nothing to print");
}

#[test]
fn settings_are_at_the_left_the_preview_fills_the_right_and_the_controls_are_under_it() {
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    let (start, printer, pages, color) =
        (rect_of(&w, "print.start"), rect_of(&w, "print.printer"), rect_of(&w, "print.pages.all"), rect_of(&w, "print.color.color"));
    let (preview, sheet) = (rect_of(&w, "print.preview"), rect_of(&w, "print.sheet"));
    // Settings at the left and a preview at the right.
    for settings in [start, printer, pages, color] {
        assert!(settings.right() < preview.left(), "{settings:?} is not left of the preview {preview:?}");
    }
    assert!(preview.width() > 500.0 && preview.height() > 400.0, "the preview fills the right: {preview:?}");
    assert!(sheet.width() > 300.0 && preview.contains_rect(sheet), "the sheet is in the preview: {sheet:?}");
    // The sheet and zoom controls are under the preview.
    for name in ["print.prev", "print.next", "print.zoomOut", "print.zoomIn", "print.fit"] {
        let control = rect_of(&w, name);
        assert!(control.top() >= preview.bottom() - 1.0, "{name} {control:?} is not under the preview {preview:?}");
    }
    // The settings do not overlap each other.
    let names = [
        "print.start",
        "print.printer",
        "print.pages.all",
        "print.pages.current",
        "print.collate",
        "print.duplex",
        "print.color.color",
        "print.color.grayscale",
    ];
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            assert!(!rect_of(&w, a).intersects(rect_of(&w, b)), "{a} and {b} overlap");
        }
    }
}

#[test]
fn a_narrow_window_puts_the_preview_under_the_settings_and_nothing_leaves_the_window() {
    let mut w = window_on(blank(), [480.0, 700.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    let (start, color) = (rect_of(&w, "print.start"), rect_of(&w, "print.color.grayscale"));
    let preview = rect_of(&w, "print.preview");
    assert!(preview.top() > color.bottom(), "the preview {preview:?} is under the settings {color:?}");
    for name in [
        "print.start",
        "print.printer",
        "print.pages.all",
        "print.pages.current",
        "print.collate",
        "print.duplex",
        "print.color.color",
        "print.color.grayscale",
        "print.preview",
    ] {
        let r = rect_of(&w, name);
        assert!(r.left() >= 0.0 && r.right() <= 480.0, "{name} {r:?} is wider than the window");
    }
    assert!(start.width() > 100.0);
    let names = [
        "print.start",
        "print.printer",
        "print.pages.all",
        "print.pages.current",
        "print.collate",
        "print.duplex",
        "print.color.color",
        "print.color.grayscale",
    ];
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            assert!(!rect_of(&w, a).intersects(rect_of(&w, b)), "{a} and {b} overlap at 480 wide");
        }
    }
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the printers -------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_printers_are_listed_on_a_thread_and_a_slow_spooler_holds_no_frame() {
    let gate = Gate::shut();
    let service = Arc::new(Fake { listing: Some(Arc::clone(&gate)), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print_holding(&mut w);
    // The spooler has not answered, and the window draws on: frames pass, the view says it is finding printers, and nothing is chosen yet.
    for _ in 0..10 {
        w.step();
    }
    assert!(w.state().print_working());
    assert!(!service.listed.load(Ordering::SeqCst));
    let before = state(&w);
    assert_eq!(before["printers"], Value::Null);
    assert_eq!(before["destination"], Value::Null);
    assert!(!enabled(&w, "print.start"), "no printer to print to yet");
    // It answers: the default printer is chosen for the person.
    gate.release();
    w.rest();
    let after = state(&w);
    assert_eq!(after["printers"], json!(["Office", "Plotter"]));
    assert_eq!(after["destination"], "Office");
    assert!(enabled(&w, "print.start"));
    assert!(service.listed.load(Ordering::SeqCst));
}

#[test]
fn a_window_with_no_service_or_no_printer_offers_the_pdf() {
    // No service at all, as the web is.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(None, &asked)));
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["destination"], "pdf");
    assert_eq!(state(&w)["service"], false);
    assert!(enabled(&w, "print.start"), "Save as PDF works where there is a dialog for it");
    // A service that lists none.
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(Fake::default())), None);
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["destination"], "pdf");
    assert!(!enabled(&w, "print.start"), "no printer and no dialog to name a file with: nothing to press");
    // A printer is chosen from the list, and Save as PDF from it, with real clicks.
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.printer");
    press(&mut w, "print.printer.Plotter");
    w.rest();
    assert_eq!(state(&w)["destination"], "Plotter");
    press(&mut w, "print.printer");
    press(&mut w, "print.printer.pdf");
    w.rest();
    assert_eq!(state(&w)["destination"], "pdf");
}

// ---- the preview --------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_sheet_is_drawn_off_the_frame_that_asked_and_shows_when_it_arrives() {
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    run_command(&mut w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5, "y": 4}));
    open_print(&mut w);
    w.rest();
    let s = state(&w);
    assert_eq!(s["sheets"], 1);
    assert_eq!(s["preview"]["drawn"], true, "{s}");
    assert!(s["preview"]["requested"].as_u64().unwrap() >= 1 && s["preview"]["received"].as_u64().unwrap() >= 1);
    // The drawing is edited with the view open: the sheet is drawn again for the new revision.
    let before = s["preview"]["requested"].as_u64().unwrap();
    run_command(&mut w, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 2, "y": 2}));
    w.rest();
    assert!(state(&w)["preview"]["requested"].as_u64().unwrap() > before, "an edit asks for the sheet again");
    assert_eq!(state(&w)["preview"]["drawn"], true);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_page_of_several_sheets_has_a_map_and_the_controls_move_between_them() {
    // 25.5 by 11 inches on upright Letter with no margins: three sheets of 8.5 across.
    let dir = TempDir::new("print-tiles");
    let session = drawing(&dir, &[("Wide", &THREE_ACROSS)]);
    let mut w = window_on(session, [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["sheets"], 3);
    assert_eq!(state(&w)["sheet"], 1);
    assert_eq!(elements_named(&w, "print.map.").len(), 3, "a small square for each sheet of the page");
    assert!(!enabled(&w, "print.prev") && enabled(&w, "print.next"));
    press(&mut w, "print.next");
    w.rest();
    assert_eq!(state(&w)["sheet"], 2);
    assert_eq!(state(&w)["preview"]["drawn"], true, "the second sheet is drawn too");
    press(&mut w, "print.map.3");
    w.rest();
    assert_eq!(state(&w)["sheet"], 3);
    assert!(!enabled(&w, "print.next") && enabled(&w, "print.prev"));
    press(&mut w, "print.prev");
    assert_eq!(state(&w)["sheet"], 2);
    // The keys that turn the sheets.
    key(&mut w, Key::PageDown, Modifiers::NONE);
    assert_eq!(state(&w)["sheet"], 3);
    key(&mut w, Key::PageUp, Modifiers::NONE);
    key(&mut w, Key::PageUp, Modifiers::NONE);
    assert_eq!(state(&w)["sheet"], 1);
    // The command takes a sheet from 1, and one past the end is the last.
    run_command(&mut w, "print.set", json!({"sheet": 99}));
    assert_eq!(state(&w)["sheet"], 3);
    assert!(w.state_mut().execute("print.set", &json!({"sheet": 0})).is_err());
}

#[test]
fn the_zoom_control_sets_the_size_of_the_sheet_and_fit_puts_the_whole_of_it_back() {
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    let fit = rect_of(&w, "print.sheet");
    assert_eq!(state(&w)["zoom"], Value::Null, "the whole sheet to begin with");
    press(&mut w, "print.zoomIn");
    w.rest();
    let zoomed = state(&w)["zoom"].as_f64().expect("a zoom now");
    assert!(zoomed > 25.0 && zoomed <= 400.0);
    assert_ne!(rect_of(&w, "print.sheet").width().round(), fit.width().round(), "the sheet changed size");
    press(&mut w, "print.zoomOut");
    press(&mut w, "print.zoomOut");
    assert!(state(&w)["zoom"].as_f64().unwrap() < zoomed);
    press(&mut w, "print.fit");
    w.rest();
    assert_eq!(state(&w)["zoom"], Value::Null);
    assert!((rect_of(&w, "print.sheet").width() - fit.width()).abs() < 1.0, "back to the whole sheet");
    // The command: a number of percent, or fit; and nothing outside the range.
    run_command(&mut w, "print.set", json!({"zoom": 200}));
    assert_eq!(state(&w)["zoom"], 200.0);
    run_command(&mut w, "print.set", json!({"zoom": "fit"}));
    assert_eq!(state(&w)["zoom"], Value::Null);
    for zoom in [json!(10), json!(401), json!("big"), json!(-3)] {
        assert!(w.state_mut().execute("print.set", &json!({ "zoom": zoom })).is_err(), "{zoom}");
    }
}

#[test]
fn margins_that_leave_no_room_are_told_where_the_sheet_would_be_and_print_is_dim() {
    let dir = TempDir::new("print-margins");
    let session = drawing(&dir, &[("Cramped", &[("PageWidth", "5"), ("PageHeight", "5"), ("PageLeftMargin", "5"), ("PageRightMargin", "5")])]);
    let mut w = window_on(session, [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    let s = state(&w);
    let why = s["plan_error"].as_str().expect("the plan could not be made");
    assert!(why.contains("margins") && why.ends_with('.') && !why.contains("PageLeftMargin"), "{why}");
    assert!(!enabled(&w, "print.start"));
    let error = w.state_mut().execute("print.start", &json!({})).unwrap_err();
    assert!(error.to_string().contains("margins"), "{error}");
}

#[test]
fn another_drawing_forgets_the_plan_of_the_old_one() {
    let dir = TempDir::new("print-forget");
    let session = drawing(&dir, &[("Wide", &THREE_ACROSS)]);
    let mut w = window_on(session, [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    run_command(&mut w, "print.set", json!({"sheet": 3}));
    assert_eq!((state(&w)["sheets"].as_u64(), state(&w)["sheet"].as_u64()), (Some(3), Some(3)));
    run_command(&mut w, "file.new", json!({}));
    run_command(&mut w, "window.backstage", json!({"page": "Print"}));
    w.rest();
    assert_eq!((state(&w)["sheets"].as_u64(), state(&w)["sheet"].as_u64()), (Some(1), Some(1)), "the new drawing is one sheet, and the first");
    assert_eq!(state(&w)["preview"]["drawn"], true);
}

// ---- printing -----------------------------------------------------------------------------------------------------------------------------

/// A drawing of two pages: Letter sideways, and A4 upright.
fn two_papers(dir: &TempDir) -> Session {
    drawing(
        dir,
        &[
            ("First", &SHEET),
            ("Second", &[("PageWidth", "7"), ("PageHeight", "10"), ("PaperKind", "9"), ("PageLeftMargin", "0.25"), ("PageRightMargin", "0.25")]),
        ],
    )
}

#[test]
fn print_sends_the_pdf_of_every_sheet_to_the_printer_with_the_settings_chosen() {
    let service = Arc::new(office());
    let dir = TempDir::new("print-job");
    let mut w = window_on(drawing(&dir, &[("First", &SHEET), ("Second", &SHEET)]), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print(&mut w);
    w.rest();
    let before = untouched(&w);
    // Three copies, from the buttons; black and white, from its button; two-sided on the long edge, from the box.
    press(&mut w, "print.copies.up");
    press(&mut w, "print.copies.up");
    press(&mut w, "print.color.grayscale");
    press(&mut w, "print.duplex");
    press(&mut w, "print.duplex.long");
    w.rest();
    let s = state(&w);
    assert_eq!((s["copies"].as_u64(), s["color"].as_str(), s["two_sided"].as_str()), (Some(3), Some("grayscale"), Some("long")));
    assert!(enabled(&w, "print.collate"), "several copies can be collated");
    press(&mut w, "print.start");
    w.rest();
    let jobs = service.jobs();
    assert_eq!(jobs.len(), 1, "both pages are one paper, so one job");
    let (pdf, job) = &jobs[0];
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(pdf_pages(pdf), 2, "a page for each sheet");
    assert_eq!(
        *job,
        PrintJob {
            printer: Some("Office".to_owned()),
            title: "print.vsdx".to_owned(),
            copies: 3,
            collate: true,
            duplex: Duplex::LongEdge,
            grayscale: true
        }
    );
    assert_eq!(w.state().notice(), Some("Sent print.vsdx to Office laser: 2 sheets. (request id is Office-1)"));
    assert!(w.state().open_error().is_none());
    assert_eq!(untouched(&w), before, "printing changes nothing in the drawing");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn sheets_of_different_papers_go_as_a_job_for_each_run_in_order() {
    let service = Arc::new(office());
    let dir = TempDir::new("print-papers");
    let mut w = window_on(two_papers(&dir), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    run_command(&mut w, "window.backstage", json!({"page": "Print"}));
    w.rest();
    assert_eq!(state(&w)["sheets"], 2);
    press(&mut w, "print.start");
    w.rest();
    let jobs = service.jobs();
    assert_eq!(jobs.len(), 2, "Letter, then A4");
    assert!(jobs.iter().all(|(pdf, _)| pdf_pages(pdf) == 1));
    // Letter sideways is 792 by 612 points; A4 is 595.27 by 841.89.
    let media = |pdf: &[u8]| {
        let text = String::from_utf8_lossy(pdf).into_owned();
        let at = text.find("/MediaBox[").unwrap();
        text[at..].split(']').next().unwrap().to_owned()
    };
    assert_eq!(media(&jobs[0].0), "/MediaBox[0 0 792 612");
    assert!(media(&jobs[1].0).starts_with("/MediaBox[0 0 595.27"), "{}", media(&jobs[1].0));
    assert_eq!(w.state().notice(), Some("Sent print.vsdx to Office laser: 2 sheets in 2 jobs. (request id is Office-1; request id is Office-2)"));
}

#[test]
fn current_page_prints_only_the_page_on_view() {
    let service = Arc::new(office());
    let dir = TempDir::new("print-current");
    let mut w = window_on(two_papers(&dir), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    run_command(&mut w, "page.select", json!({"id": 2}));
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["sheets"], 2);
    press(&mut w, "print.pages.current");
    w.rest();
    assert_eq!((state(&w)["pages"].as_str(), state(&w)["sheets"].as_u64()), (Some("current"), Some(1)));
    press(&mut w, "print.start");
    w.rest();
    let jobs = service.jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(pdf_pages(&jobs[0].0), 1);
    assert!(String::from_utf8_lossy(&jobs[0].0).contains("/MediaBox[0 0 595.27"), "the second page's paper, A4");
}

#[test]
fn a_job_waiting_on_the_spooler_holds_no_frame_and_can_be_cancelled() {
    let gate = Gate::shut();
    let service = Arc::new(Fake { submitting: Some(Arc::clone(&gate)), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    // The spooler has the job and has not answered: the window draws on, says what it is doing, and offers Cancel.
    for _ in 0..10 {
        w.step();
    }
    assert!(w.state().printing_now());
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Sending ")), "{:?}", w.state().notice());
    assert!(w.state().element("print.cancel").is_some_and(|c| c.enabled));
    assert!(w.state().element("print.start").is_none(), "Print gives way to Cancel while the job goes");
    // Cancelled before it reaches the spooler: nothing is sent, and the person is told.
    press(&mut w, "print.cancel");
    w.rest();
    assert!(service.jobs().is_empty());
    let words = w.state().open_error().expect("the end is shown").to_owned();
    assert!(words.contains("was not printed to Office laser") && words.contains("cancelled"), "{words}");
    assert!(!w.state().printing_now());
    assert!(enabled(&w, "print.start"), "the button is back");
}

/// The notice of a job under way ("Sending ... to ...") has no lifetime of its own: it stays while the job runs, however long the window's clock goes on
/// (the status bar shows the state of a print in the background), and it goes when the job ends with no notice of its own (a
/// cancelled job is told in the banner). Before a change it stayed after the job, for good (a notice never expires).
#[test]
fn the_notice_of_a_job_stays_while_the_job_runs_and_goes_with_it() {
    let gate = Gate::shut();
    let service = Arc::new(Fake { submitting: Some(Arc::clone(&gate)), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    for _ in 0..5 {
        w.step();
    }
    assert!(w.state().printing_now());
    w.advance(Duration::from_secs(60));
    for _ in 0..5 {
        w.step();
    }
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Sending ")), "a minute on, the job still goes: {:?}", w.state().notice());
    press(&mut w, "print.cancel");
    w.rest();
    assert!(!w.state().printing_now() && w.state().open_error().is_some(), "the job ended, and the banner says how");
    assert_eq!(w.state().notice(), None, "and its notice went with it");
}

#[test]
fn a_job_that_is_let_through_ends_in_the_window_and_the_notice_says_so() {
    let gate = Gate::shut();
    let service = Arc::new(Fake { submitting: Some(Arc::clone(&gate)), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    for _ in 0..5 {
        w.step();
    }
    assert!(w.state().printing_now());
    gate.release();
    w.rest();
    assert!(!w.state().printing_now());
    assert_eq!(service.jobs().len(), 1);
    assert_eq!(w.state().notice(), Some("Sent Drawing1 to Office laser: 1 sheet. (request id is Office-1)"));
    assert_eq!(state(&w)["last"], "Sent Drawing1 to Office laser: 1 sheet. (request id is Office-1)");
}

#[test]
fn a_printer_that_refuses_is_told_in_words_and_the_drawing_is_as_it_was() {
    let service = Arc::new(Fake { refuses: Some("the printer is offline".to_owned()), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(service), None);
    open_print(&mut w);
    w.rest();
    let before = untouched(&w);
    press(&mut w, "print.start");
    w.rest();
    let words = w.state().open_error().expect("a refusal is shown over the drawing").to_owned();
    assert_eq!(words, "Drawing1 was not printed to Office laser. The printer is offline.");
    assert!(!words.contains("print.start") && !words.contains("::"), "{words}");
    assert_eq!(untouched(&w), before);
    assert_eq!(state(&w)["last"], words.as_str());
}

// ---- Save as PDF --------------------------------------------------------------------------------------------------------------------------

#[test]
fn save_as_pdf_writes_what_a_printer_would_be_given_and_leaves_the_drawing_alone() {
    let dir = TempDir::new("print-pdf");
    let target = dir.file("plan.pdf");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(Some(target.clone()), &asked)));
    run_command(&mut w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5, "y": 4}));
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["destination"], "pdf");
    let before = untouched(&w);
    press(&mut w, "print.start");
    w.rest();
    assert_eq!(asked.lock().unwrap().as_slice(), ["Drawing1.pdf"], "the dialog starts with the drawing's name as a PDF");
    let file = std::fs::read(&target).expect("the PDF was written");
    assert!(file.starts_with(b"%PDF-") && pdf_pages(&file) == 1);
    assert_eq!(w.state().notice(), Some("Saved the print of Drawing1 as plan.pdf: 1 sheet."));
    assert_eq!(untouched(&w), before, "not an edit, not unsaved, no undo step");
    assert!(w.state().session().path().is_none(), "and gives the drawing no file");
    assert!(w.state().open_error().is_none());
}

#[test]
fn a_name_with_no_type_gets_pdf_another_type_is_refused_and_a_cancelled_dialog_does_nothing() {
    let dir = TempDir::new("print-names");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(Some(dir.file("plain")), &asked)));
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    w.rest();
    assert!(std::fs::read(dir.file("plain.pdf")).unwrap().starts_with(b"%PDF-"), "plain became plain.pdf");
    assert!(!dir.file("plain").exists());

    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(Some(dir.file("photo.png")), &asked)));
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    w.rest();
    let words = w.state().open_error().expect("a refusal is shown").to_owned();
    assert!(words.starts_with("photo.png was not printed") && words.contains("Name the file with .pdf"), "{words}");
    assert!(!dir.file("photo.png").exists());

    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(None, &asked)));
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    w.rest();
    assert!(w.state().open_error().is_none() && w.state().notice().is_none(), "a cancelled dialog does nothing");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "only plain.pdf was written");
}

#[test]
fn a_pdf_is_never_written_over_the_drawings_own_file_and_a_typed_name_that_is_there_is_asked_about() {
    let dir = TempDir::new("print-own");
    // The drawing's own file is named like a PDF: the window knows which file it is.
    let session = {
        let mut builder = VsdxBuilder::new();
        builder = builder.page(PageSpec::new(1, "First").sheet(&cells(&SHEET)));
        std::fs::write(dir.file("own.pdf"), builder.build()).unwrap();
        let mut session = Session::new();
        session.execute("file.open", &json!({"path": dir.file("own.pdf").to_str().unwrap()})).unwrap();
        session
    };
    let own = std::fs::read(dir.file("own.pdf")).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_on(session, [1280.0, 800.0], None, Some(picker(Some(dir.file("own.pdf")), &asked)));
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    w.rest();
    assert!(w.state().open_error().is_some_and(|m| m.contains("drawing's own file")), "{:?}", w.state().open_error());
    assert_eq!(std::fs::read(dir.file("own.pdf")).unwrap(), own, "the drawing is as it was");

    // A name without a type that makes a file already there: the person is asked, and Replace writes it.
    std::fs::write(dir.file("there.pdf"), b"the old one").unwrap();
    let mut w = window_on(blank(), [1280.0, 800.0], None, Some(picker(Some(dir.file("there")), &asked)));
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.start");
    w.rest();
    assert!(w.state().replace_prompt_open());
    assert_eq!(std::fs::read(dir.file("there.pdf")).unwrap(), b"the old one", "nothing is written before the answer");
    press(&mut w, "replace.replace");
    w.rest();
    assert!(std::fs::read(dir.file("there.pdf")).unwrap().starts_with(b"%PDF-"));
}

// ---- the commands -------------------------------------------------------------------------------------------------------------------------

#[test]
fn print_set_and_print_start_do_what_the_controls_do_and_refuse_what_they_cannot() {
    let service = Arc::new(office());
    let dir = TempDir::new("print-commands");
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    open_print(&mut w);
    w.rest();
    run_command(
        &mut w,
        "print.set",
        json!({"printer": "Plotter", "copies": 2, "collate": false, "two_sided": "short", "color": "grayscale", "pages": "current"}),
    );
    let s = state(&w);
    assert_eq!(
        (s["destination"].as_str(), s["copies"].as_u64(), s["collate"].as_bool(), s["two_sided"].as_str(), s["color"].as_str(), s["pages"].as_str()),
        (Some("Plotter"), Some(2), Some(false), Some("short"), Some("grayscale"), Some("current"))
    );
    // Nothing is half set: a refusal leaves every choice as it was.
    for params in [
        json!({"copies": 0}),
        json!({"copies": 1000}),
        json!({"copies": 2.5}),
        json!({"copies": 5, "printer": "Nowhere"}),
        json!({"two_sided": "both"}),
        json!({"color": "sepia"}),
        json!({"pages": "some"}),
        json!({"collate": "yes"}),
        json!({"unknown": 1}),
        json!({"printer": 3}),
    ] {
        let error = w.state_mut().execute("print.set", &params).unwrap_err();
        assert!(!error.to_string().chars().any(char::is_control), "{params}: {error}");
        assert_eq!(state(&w)["copies"], 2, "{params}");
        assert_eq!(state(&w)["destination"], "Plotter", "{params}");
    }
    // Start: a printer takes no path, a program's start sends the job, and a second start while one goes is refused.
    assert!(w.state_mut().execute("print.start", &json!({"path": dir.file("x.pdf").to_str().unwrap()})).is_err());
    w.state_mut().execute("print.start", &json!({})).unwrap();
    assert!(w.state().print_working(), "asked for, begun by the next frame");
    w.rest();
    let jobs = service.jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        (jobs[0].1.printer.as_deref(), jobs[0].1.copies, jobs[0].1.duplex, jobs[0].1.grayscale),
        (Some("Plotter"), 2, Duplex::ShortEdge, true)
    );
    // A PDF needs the path.
    run_command(&mut w, "print.set", json!({"printer": "pdf"}));
    let error = w.state_mut().execute("print.start", &json!({})).unwrap_err();
    assert!(error.to_string().contains("path"), "{error}");
    run_command(&mut w, "print.start", json!({"path": dir.file("c.pdf").to_str().unwrap()}));
    w.rest();
    assert!(std::fs::read(dir.file("c.pdf")).unwrap().starts_with(b"%PDF-"), "a program writes the file with no dialog");
    assert_eq!(service.jobs().len(), 1, "and no job went to a printer");
}

#[test]
fn print_set_before_the_printers_are_listed_says_so_and_the_commands_are_listed() {
    let gate = Gate::shut();
    let service = Arc::new(Fake { listing: Some(Arc::clone(&gate)), ..office() });
    let mut w = window_on(blank(), [1280.0, 800.0], Some(service), None);
    open_print_holding(&mut w);
    for _ in 0..3 {
        w.step();
    }
    let error = w.state_mut().execute("print.set", &json!({"printer": "Office"})).unwrap_err();
    assert!(error.to_string().contains("not listed yet"), "{error}");
    assert!(w.state_mut().execute("print.set", &json!({"printer": "pdf"})).is_ok(), "a PDF needs no list");
    gate.release();
    w.rest();
    for id in ["print.set", "print.start", "window.backstage"] {
        assert!(diacraft_ui_egui::commands::find_ui_command(id).is_some(), "{id}");
    }
}

#[test]
fn escape_closes_an_open_list_of_printers_and_then_the_view() {
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::new(office())), None);
    open_print(&mut w);
    w.rest();
    press(&mut w, "print.printer");
    assert!(w.state().element("print.printer.Plotter").is_some(), "the list is open");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["backstage"], "Print", "the list closed and the view stayed");
    assert!(w.state().element("print.printer.Plotter").is_none(), "the list is closed");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().window_state()["backstage"], Value::Null, "the second Escape leaves the view");
}

// ---- black and white -----------------------------------------------------------------------------------------------------------

/// The textures the window hands its renderer, kept whole as they arrive and dropped when the window frees them: a test reads the pixels of a
/// picture the window shows, as a graphics card would be given them.
#[derive(Clone, Default)]
struct Uploads(Arc<Mutex<HashMap<egui::TextureId, Arc<egui::ColorImage>>>>);

impl egui_kittest::TestRenderer for Uploads {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut kept = self.0.lock().unwrap();
        for (id, images) in delta.set.drain() {
            for image in images {
                // A whole picture replaces what was kept; a patch (the font atlas grows by them) is no picture a test reads.
                let egui::ImageData::Color(pixels) = &image.image;
                if image.pos.is_none() {
                    kept.insert(id, Arc::clone(pixels));
                }
            }
        }
        for id in delta.free.drain() {
            kept.remove(&id);
        }
    }
}

/// The pixels of the sheet the preview shows: the one picture of a sheet the window holds.
fn preview_pixels(w: &Window, uploads: &Uploads) -> Arc<egui::ColorImage> {
    let sheets: Vec<egui::TextureId> =
        w.ctx.tex_manager().read().allocated().filter(|(_, meta)| meta.name.starts_with("print-sheet-")).map(|(id, _)| *id).collect();
    assert_eq!(sheets.len(), 1, "the window holds one picture of a sheet");
    Arc::clone(uploads.0.lock().unwrap().get(&sheets[0]).expect("the picture of the sheet was uploaded"))
}

/// How far apart a pixel's red, green and blue are: 0 for a grey.
fn spread(c: egui::Color32) -> u8 {
    let (r, g, b) = (c.r(), c.g(), c.b());
    r.max(g).max(b) - r.min(g).min(b)
}

/// The pixels that are coloured, past a tolerance of 2 for rounding, and the pixels that have ink.
fn coloured(image: &egui::ColorImage) -> usize {
    image.pixels.iter().filter(|c| spread(**c) > 2).count()
}

fn inked(image: &egui::ColorImage) -> usize {
    image.pixels.iter().filter(|c| c.a() > 0).count()
}

/// A drawing of colours: a red box with a thick blue line, a green ellipse with an orange line, and a blue box with yellow words.
fn colour_the_drawing(w: &mut Window) {
    run_command(w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 5.5}));
    run_command(w, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 7.5, "y": 5.5}));
    run_command(w, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.5, "y": 2.5}));
    run_command(w, "shape.format", json!({"ids": [1], "fill": "#E02020", "line": "#1030C0", "weight": 4}));
    run_command(w, "shape.format", json!({"ids": [2], "fill": "#20B040", "line": "#E0A000", "weight": 3}));
    run_command(w, "shape.format", json!({"ids": [3], "fill": "#2060F0"}));
    run_command(w, "shape.set_text", json!({"id": 3, "page": 1, "text": "Blue box"}));
    run_command(w, "text.format", json!({"ids": [3], "color": "#FFFF00", "size": 18, "bold": true}));
}

#[test]
fn black_and_white_draws_the_preview_in_greys_at_once_and_color_brings_the_colours_back() {
    // The owner's report (2026-11-21): with Black and white chosen the preview stayed in colour.
    let uploads = Uploads::default();
    let app = DiacraftApp::new(blank(), None).with_inline_pictures(bundled_drawer()).with_print_service(Arc::new(office()));
    let mut w = fast_window_with(app, [1280.0, 800.0], uploads.clone());
    w.rest();
    colour_the_drawing(&mut w);
    open_print(&mut w);
    w.rest();
    let colour = preview_pixels(&w, &uploads);
    assert!(coloured(&colour) > 2_000, "the preview in colour has colour: {} pixels", coloured(&colour));
    let asked = state(&w)["preview"]["requested"].as_u64().unwrap();
    // A real click on Black and white: the sheet is asked for again in the frames of the click, and drawn in greys.
    press(&mut w, "print.color.grayscale");
    assert!(state(&w)["preview"]["requested"].as_u64().unwrap() > asked, "the choice asks for the sheet again at once");
    w.rest();
    assert_eq!(state(&w)["color"], "grayscale");
    let grey = preview_pixels(&w, &uploads);
    assert_eq!(
        coloured(&grey),
        0,
        "no pixel of the preview is coloured (the worst is {})",
        grey.pixels.iter().map(|c| spread(*c)).max().unwrap_or(0)
    );
    assert_eq!(inked(&grey), inked(&colour), "the same shapes, every one of them, in greys");
    assert_eq!(grey.size, colour.size);
    // Color again: the colours come back, as they were.
    press(&mut w, "print.color.color");
    w.rest();
    assert_eq!(state(&w)["color"], "color");
    let back = preview_pixels(&w, &uploads);
    assert!(coloured(&back) > 2_000, "the colours are back: {} pixels", coloured(&back));
    assert_eq!(back.pixels, colour.pixels, "the colour preview is what it was");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn black_and_white_can_be_chosen_for_a_pdf_and_the_preview_follows_it() {
    // Save as PDF writes what a printer would be given, so its colour is chosen as a printer's is.
    let uploads = Uploads::default();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let app = DiacraftApp::new(blank(), None).with_inline_pictures(bundled_drawer()).with_print_pdf_picker(picker(None, &asked));
    let mut w = fast_window_with(app, [1280.0, 800.0], uploads.clone());
    w.rest();
    colour_the_drawing(&mut w);
    open_print(&mut w);
    w.rest();
    assert_eq!(state(&w)["destination"], "pdf");
    assert!(enabled(&w, "print.color.grayscale") && enabled(&w, "print.color.color"), "the colour is chosen for a PDF too");
    press(&mut w, "print.color.grayscale");
    w.rest();
    assert_eq!(state(&w)["color"], "grayscale");
    assert_eq!(coloured(&preview_pixels(&w, &uploads)), 0);
    // A narrow window puts the preview under the settings: the choice is there and works the same.
    let uploads = Uploads::default();
    let app = DiacraftApp::new(blank(), None).with_inline_pictures(bundled_drawer()).with_print_pdf_picker(picker(None, &asked));
    let mut w = fast_window_with(app, [640.0, 900.0], uploads.clone());
    w.rest();
    colour_the_drawing(&mut w);
    open_print(&mut w);
    w.rest();
    assert!(coloured(&preview_pixels(&w, &uploads)) > 500);
    press(&mut w, "print.color.grayscale");
    w.rest();
    assert_eq!(coloured(&preview_pixels(&w, &uploads)), 0);
}

/// The pixels of `pdf`'s first page as another reader draws them (poppler's `pdftoppm`), or `None` where the machine has no such reader.
fn raster(dir: &TempDir, name: &str, pdf: &[u8]) -> Option<Vec<[u8; 3]>> {
    if std::process::Command::new("pdftoppm").arg("-v").output().is_err() {
        eprintln!("SKIPPED: pdftoppm (poppler) is not on this machine");
        return None;
    }
    let file = dir.file(&format!("{name}.pdf"));
    std::fs::write(&file, pdf).unwrap();
    let prefix = dir.file(name);
    let run = std::process::Command::new("pdftoppm").args(["-r", "24", "-png", "-singlefile"]).arg(&file).arg(&prefix).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let bytes = std::fs::read(dir.file(&format!("{name}.png"))).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let channels = info.color_type.samples();
    Some(pixels.chunks_exact(channels).map(|p| [p[0], p[1], p[2]]).collect())
}

fn coloured_rgb(pixels: &[[u8; 3]]) -> usize {
    pixels.iter().filter(|[r, g, b]| r.max(g).max(b) - r.min(g).min(b) > 2).count()
}

#[test]
fn a_print_in_black_and_white_sends_and_saves_a_pdf_with_no_colour_in_it() {
    let service = Arc::new(office());
    let dir = TempDir::new("print-grey");
    let mut w = window_on(blank(), [1280.0, 800.0], Some(Arc::clone(&service)), None);
    colour_the_drawing(&mut w);
    open_print(&mut w);
    w.rest();
    // To the printer in colour, then in black and white, from the buttons.
    press(&mut w, "print.start");
    w.rest();
    press(&mut w, "print.color.grayscale");
    press(&mut w, "print.start");
    w.rest();
    let jobs = service.jobs();
    assert_eq!(jobs.len(), 2);
    assert_eq!((jobs[0].1.grayscale, jobs[1].1.grayscale), (false, true), "the spooler is told too");
    // And Save as PDF, with the colour chosen as it is.
    run_command(&mut w, "print.set", json!({"printer": "pdf"}));
    run_command(&mut w, "print.start", json!({"path": dir.file("grey.pdf").to_str().unwrap()}));
    w.rest();
    let saved = std::fs::read(dir.file("grey.pdf")).expect("the PDF was written");
    let (Some(colour), Some(sent), Some(file)) = (raster(&dir, "colour", &jobs[0].0), raster(&dir, "sent", &jobs[1].0), raster(&dir, "file", &saved))
    else {
        return;
    };
    assert!(coloured_rgb(&colour) > 100, "the colour print has colour: {}", coloured_rgb(&colour));
    assert_eq!(coloured_rgb(&sent), 0, "the PDF a printer is given in black and white has none");
    assert_eq!(coloured_rgb(&file), 0, "nor has the PDF saved in black and white");
    let ink = |p: &[[u8; 3]]| p.iter().filter(|c| **c != [255, 255, 255]).count();
    assert!(ink(&sent) > 100 && ink(&file) > 100, "the shapes are on the sheet, in greys");
}
