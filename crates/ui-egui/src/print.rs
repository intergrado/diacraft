//! Printing from the window: the Print view of the File view, its settings, and the job that sends a drawing to a printer or to a PDF.
//!
//! **What the window does and what it asks.** The window plans what a print puts on paper (`diacraft_engine::print`, from the page's own cells), shows
//! it in the preview (`print_preview.rs`, `print_view.rs`), and when the person presses Print it **makes the PDF a printer is given** (`print::pdf`:
//! a page for each sheet, at the paper's size) and hands it to a [`PrintService`]. The service is the desktop app's: it is the one that knows the
//! system's spooler (`apps/diacraft/src/printing.rs`, over `craft-print`), as the Open dialog is the app's. A window with no service has no
//! printer, and its Print view offers the PDF alone, as the web does.
//!
//! **Off the window's thread.** Everything slow is done on a thread of its own and never waits on a frame: listing the printers (the spooler may be
//! slow to answer), choosing the file of a PDF, drawing the pages and sending the job. The window asks for a frame when each answers. A job in
//! progress can be cancelled until it reaches the spooler ([`PrintCancel`]).
//!
//! **Destinations.** A printer of the service's list, or **Save as PDF**, which writes what a printer would be given to a file, through the engine's
//! `file.print` (so its path has an export's rules: the drawing's own file is never written, a link is refused). A drawing whose sheets take
//! different papers is sent as one job to the printer for each run of sheets that share a paper, in order (a printer takes one paper to a job).
//!
//! **Settings** are the window's, not the drawing's (a setting of the print is not saved with it): the printer, copies, whether copies are collated,
//! two-sided printing, colour or black and white, and which pages. Paper, orientation, margins and scale are the **page's own** (`PaperKind`,
//! `PrintPageOrientation` and the rest, `diacraft_render::print`), changed by Page Setup and not here.
//!
//! **Black and white** is the preview's, the PDF's and the spooler's alike: the preview draws the sheet in greys as soon as it is chosen,
//! the PDF given to a printer or saved as a file has every colour as its grey (`PrintColor::Grayscale`, BT.601's weights), and the
//! job also asks the spooler for monochrome. It is chosen for Save as PDF as for a printer.
//!
//! The commands are `window.backstage` with page `Print` (the view), `print.set` (the settings and the sheet on view) and `print.start` (Print),
//! and the engine's `file.print_preview` and `file.print`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use diacraft_engine::doc::Document;
use diacraft_engine::print::{JobPlan, Pages, PrintColor, PrintOptions, TextMode, failure_words};
use diacraft_engine::{EngineError, Session, guard, shown};
use diacraft_fonts::FontDb;
use diacraft_render::Renderer;
use egui::Context;
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::export::ExportPicker;
use crate::print_preview::Preview;

/// The most copies of a print.
pub const MAX_COPIES: u32 = 999;

/// What the person picks to print to when the service lists no printer, or to save what a printer would be given.
pub const PDF_WORD: &str = "pdf";

/// A printer the service lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrinterInfo {
    /// What a job names the printer by.
    pub name: String,
    /// What the Print view shows; empty shows the name.
    pub label: String,
    /// The system's default printer.
    pub default: bool,
}

impl PrinterInfo {
    /// The words the Print view shows for it.
    pub fn shown(&self) -> &str {
        if self.label.trim().is_empty() { &self.name } else { &self.label }
    }
}

/// Two-sided printing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Duplex {
    /// Whatever the printer's own setting is.
    #[default]
    PrinterDefault,
    OneSided,
    LongEdge,
    ShortEdge,
}

impl Duplex {
    /// The word `print.set` takes and gives.
    pub const fn word(self) -> &'static str {
        match self {
            Duplex::PrinterDefault => "default",
            Duplex::OneSided => "off",
            Duplex::LongEdge => "long",
            Duplex::ShortEdge => "short",
        }
    }

    /// What the Print view calls it.
    pub const fn label(self) -> &'static str {
        match self {
            Duplex::PrinterDefault => "Printer default",
            Duplex::OneSided => "One-sided",
            Duplex::LongEdge => "Two-sided, flip on long edge",
            Duplex::ShortEdge => "Two-sided, flip on short edge",
        }
    }

    pub const ALL: [Duplex; 4] = [Duplex::PrinterDefault, Duplex::OneSided, Duplex::LongEdge, Duplex::ShortEdge];

    fn from_word(word: &str) -> Option<Duplex> {
        Duplex::ALL.into_iter().find(|d| d.word().eq_ignore_ascii_case(word))
    }
}

/// A job for the spooler: the PDF goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintJob {
    /// `None`: the system's default printer.
    pub printer: Option<String>,
    /// The job's name in the print queue.
    pub title: String,
    pub copies: u32,
    pub collate: bool,
    pub duplex: Duplex,
    pub grayscale: bool,
}

/// Stops a job before it reaches the spooler, from any thread.
#[derive(Default)]
pub struct PrintCancel {
    cancelled: AtomicBool,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl PrintCancel {
    pub fn new() -> PrintCancel {
        PrintCancel::default()
    }

    /// Asks for the job to stop. The service's hook, if it set one, runs now (once).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let hook = self.hook.lock().ok().and_then(|mut h| h.take());
        if let Some(hook) = hook {
            hook();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// For a service: `hook` runs when the job is cancelled, at once if it already was. A spooler that has a cancel of its own calls it here.
    pub fn on_cancel(&self, hook: impl FnOnce() + Send + 'static) {
        if self.is_cancelled() {
            hook();
        } else if let Ok(mut slot) = self.hook.lock() {
            *slot = Some(Box::new(hook));
            // Cancelled between the check and the lock: nobody else will run it.
            if self.is_cancelled()
                && let Some(hook) = slot.take()
            {
                drop(slot);
                hook();
            }
        }
    }
}

/// The system's printing, as the desktop app supplies it. Both calls block, and the window calls them on a thread of its own.
pub trait PrintService: Send + Sync {
    /// The printers the system knows, with the default marked.
    fn printers(&self) -> Vec<PrinterInfo>;

    /// Sends `pdf` (a page for each sheet, all of one paper) as `job`. Gives the spooler's message, or the words of why it did not.
    fn submit(&self, pdf: &[u8], job: &PrintJob, cancel: &PrintCancel) -> Result<String, String>;
}

/// Where the print goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Printer(String),
    /// A PDF file of what a printer would be given.
    Pdf,
}

/// Which pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageRange {
    /// Every page that prints, in file order.
    All,
    /// The page on view.
    Current,
}

impl PageRange {
    pub const fn word(self) -> &'static str {
        match self {
            PageRange::All => "all",
            PageRange::Current => "current",
        }
    }
}

/// What the person has chosen in the Print view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintChoices {
    /// `None` until the printers are listed and one is chosen for the person (the default printer, else the first, else a PDF).
    pub destination: Option<Destination>,
    pub copies: u32,
    pub collate: bool,
    pub duplex: Duplex,
    pub grayscale: bool,
    pub range: PageRange,
}

impl Default for PrintChoices {
    fn default() -> PrintChoices {
        PrintChoices { destination: None, copies: 1, collate: true, duplex: Duplex::PrinterDefault, grayscale: false, range: PageRange::All }
    }
}

impl PrintChoices {
    /// The colours the print is drawn in: the preview's, and the PDF's that a printer is given or a file saved.
    pub fn color(&self) -> PrintColor {
        if self.grayscale { PrintColor::Grayscale } else { PrintColor::Color }
    }
}

/// The printers: not asked for yet, being listed, or listed.
enum Printers {
    Unknown,
    Loading(Receiver<Vec<PrinterInfo>>),
    Ready(Vec<PrinterInfo>),
}

/// A plan, and what it is of.
struct Planned {
    key: (u64, u64, PageRange, Option<u32>),
    plan: Result<Arc<JobPlan>, String>,
}

/// A job under way.
struct Running {
    /// What it prints to, for the words that tell its end.
    to: String,
    /// The PDF it writes, when it writes one: a host (the browser) hands it over when the job ends.
    pdf: Option<PathBuf>,
    rx: Receiver<Result<Sent, String>>,
    cancel: Arc<PrintCancel>,
}

/// What a job did.
struct Sent {
    sheets: usize,
    jobs: usize,
    receipts: Vec<String>,
    /// How many kinds of thing the pages left out or drew approximately.
    approximated: usize,
}

/// What the window keeps of printing.
pub struct PrintState {
    pub(crate) service: Option<Arc<dyn PrintService>>,
    pub(crate) pdf_picker: Option<ExportPicker>,
    pub(crate) choices: PrintChoices,
    printers: Printers,
    planned: Option<Planned>,
    /// The sheet on view in the preview, from 0.
    pub(crate) sheet: usize,
    /// The preview's zoom in percent, or `None` for the whole sheet in the area.
    pub(crate) zoom: Option<f64>,
    pub(crate) pan: egui::Vec2,
    pub(crate) preview: Preview,
    /// The Save dialog of a PDF, on its thread.
    pdf_pick: Option<Receiver<Option<PathBuf>>>,
    running: Option<Running>,
    /// `print.start` asked for a print: the next frame begins it (the window command has no context to start a thread with).
    want: Option<Option<String>>,
    /// The faces the job's text is laid out in.
    pub(crate) fonts: PrintFonts,
    /// How the last job ended, in words.
    pub(crate) outcome: Option<String>,
}

impl Default for PrintState {
    fn default() -> PrintState {
        PrintState {
            service: None,
            pdf_picker: None,
            choices: PrintChoices::default(),
            printers: Printers::Unknown,
            planned: None,
            sheet: 0,
            zoom: None,
            pan: egui::Vec2::ZERO,
            preview: Preview::threaded(),
            pdf_pick: None,
            running: None,
            want: None,
            fonts: PrintFonts::System,
            outcome: None,
        }
    }
}

/// The faces a print is laid out in.
#[derive(Clone)]
pub(crate) enum PrintFonts {
    /// The faces the application found once and shares with its pictures, its previews and its commits (`DiacraftApp::with_fonts`).
    Shared(diacraft_engine::measure::SharedFonts),
    /// The machine's faces and the bundled ones, read for the job on its thread: a window that was given no shared database.
    System,
    /// The bundled faces alone, the same on every machine: a test.
    Bundled,
}

impl PrintFonts {
    /// The renderer of a print job. Where the machine's faces are read, or waited for, it is the job's thread that does it.
    pub(crate) fn renderer(self) -> Renderer {
        match self {
            PrintFonts::Shared(fonts) => Renderer::with_shared_fonts(fonts.get()),
            PrintFonts::System => Renderer::with_fonts(FontDb::system_and_bundled()),
            PrintFonts::Bundled => Renderer::with_fonts(FontDb::bundled()),
        }
    }
}

impl PrintState {
    /// The printers listed, if they have been.
    pub(crate) fn printer_list(&self) -> Option<&[PrinterInfo]> {
        match &self.printers {
            Printers::Ready(list) => Some(list),
            _ => None,
        }
    }

    pub(crate) fn printers_loading(&self) -> bool {
        matches!(self.printers, Printers::Loading(_))
    }

    /// Another drawing replaced the old one: its plan, its sheets and what was last said of printing it are forgotten, and the sheet on view is the first.
    pub(crate) fn began_another_drawing(&mut self) {
        self.planned = None;
        self.sheet = 0;
        self.pan = egui::Vec2::ZERO;
        self.preview.reset();
        self.outcome = None;
    }

    /// Forgets the printers, so that they are listed again.
    pub(crate) fn forget_printers(&mut self) {
        if !self.printers_loading() {
            self.printers = Printers::Unknown;
        }
    }

    /// What the choices come to, as `print.set` reads it back.
    pub(crate) fn destination_word(&self) -> Option<String> {
        self.choices.destination.as_ref().map(|d| match d {
            Destination::Printer(name) => name.clone(),
            Destination::Pdf => PDF_WORD.to_owned(),
        })
    }
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// Gives the window the system's printing. Without it the Print view offers the PDF alone.
    pub fn with_print_service(mut self, service: Arc<dyn PrintService>) -> DiacraftApp {
        self.ui.print.service = Some(service);
        self
    }

    /// Gives the window a native Save dialog to name the PDF of Save as PDF with. Without one, a PDF is written only to a path a program names.
    pub fn with_print_pdf_picker(mut self, picker: ExportPicker) -> DiacraftApp {
        self.ui.print.pdf_picker = Some(picker);
        self
    }

    /// What the person has chosen.
    pub fn print_choices(&self) -> &PrintChoices {
        &self.ui.print.choices
    }

    /// Whether anything of printing is still under way on another thread, or asked for and not begun: the printers being listed, the dialog of a PDF, a
    /// job, a preview being drawn. A test waits for it to be false.
    pub fn print_working(&self) -> bool {
        let p = &self.ui.print;
        p.printers_loading() || p.pdf_pick.is_some() || p.running.is_some() || p.want.is_some() || p.preview.busy()
    }

    /// Whether a print job (not a preview) is under way.
    pub fn printing_now(&self) -> bool {
        self.ui.print.running.is_some()
    }

    /// Whether the Save dialog of a PDF is on the screen.
    pub fn print_dialog_open(&self) -> bool {
        self.ui.print.pdf_pick.is_some()
    }

    /// Whether a print can begin now, by a program that names the file of a PDF itself: a drawing, a plan, nothing under way, and a service for a
    /// printer.
    fn print_ready(&mut self) -> bool {
        if self.ui.print.running.is_some()
            || self.print_dialog_open()
            || self.name_card_for(crate::file_host::Purpose::PrintPdf)
            || self.session().document().is_none()
        {
            return false;
        }
        if self.print_plan().is_err() {
            return false;
        }
        match self.ui.print.choices.destination {
            Some(Destination::Printer(_)) => self.ui.print.service.is_some(),
            Some(Destination::Pdf) => true,
            None => false,
        }
    }

    /// Whether Print can be pressed now: a print can begin, and a PDF has a dialog to name its file with.
    pub fn can_print(&mut self) -> bool {
        self.print_ready()
            && (self.ui.print.choices.destination != Some(Destination::Pdf) || self.ui.print.pdf_picker.is_some() || self.has_file_host())
    }

    /// Whether the File view can show the Print page: a drawing is open.
    pub fn can_open_print(&self) -> bool {
        self.session().document().is_some()
    }

    /// Shows the Print page of the File view (Ctrl+P): a label being typed is sent first, and the sheet on view starts at the first.
    pub(crate) fn open_print_view(&mut self) {
        if !self.can_open_print() {
            return;
        }
        self.commit_pending_text();
        if self.ui.backstage != Some(crate::state::BackstagePage::Print) {
            self.ui.print.sheet = 0;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        self.ui.backstage = Some(crate::state::BackstagePage::Print);
    }

    /// The pages the choices cover now.
    fn print_pages(&mut self) -> Pages {
        match (self.ui.print.choices.range, self.current_page()) {
            (PageRange::Current, Some(id)) => Pages::One(id),
            _ => Pages::All,
        }
    }

    /// What a print of the choices puts on paper now, kept until the drawing, the page on view or the range changes. The words of why not, when
    /// it cannot be planned.
    pub(crate) fn print_plan(&mut self) -> Result<Arc<JobPlan>, String> {
        if self.session().document().is_none() {
            return Err("There is no drawing to print.".to_owned());
        }
        let current = if self.ui.print.choices.range == PageRange::Current { self.ui.page } else { None };
        let key = (self.ui.pictures.generation(), self.session().revision(), self.ui.print.choices.range, current);
        if let Some(p) = &self.ui.print.planned
            && p.key == key
        {
            return p.plan.clone();
        }
        let pages = self.print_pages();
        let Some(doc) = self.session().document() else { return Err("There is no drawing to print.".to_owned()) };
        let plan = diacraft_engine::print::plan(doc, pages).map(Arc::new).map_err(|e| sentence(&e.to_string()));
        self.ui.print.planned = Some(Planned { key, plan: plan.clone() });
        plan
    }

    /// Starts listing the printers if they have not been (on a thread: the spooler may be slow). Without a service there are none to list.
    pub(crate) fn print_list_printers(&mut self, ctx: &Context) {
        if !matches!(self.ui.print.printers, Printers::Unknown) {
            return;
        }
        let Some(service) = self.ui.print.service.clone() else {
            self.ui.print.printers = Printers::Ready(Vec::new());
            self.choose_default_destination();
            return;
        };
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-printers".into()).spawn(move || {
            // A service that panics lists no printers.
            let list = guard::catch_panic(|| service.printers()).unwrap_or_default();
            let _ = tx.send(list);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.ui.print.printers = Printers::Loading(rx),
            Err(_) => {
                self.ui.print.printers = Printers::Ready(Vec::new());
                self.choose_default_destination();
            }
        }
    }

    /// The system's default printer, else the first, else a PDF: what the person starts with, unless they have chosen.
    fn choose_default_destination(&mut self) {
        if self.ui.print.choices.destination.is_some() {
            return;
        }
        let chosen = match self.ui.print.printer_list() {
            Some(list) => list.iter().find(|p| p.default).or(list.first()).map(|p| Destination::Printer(p.name.clone())),
            None => None,
        };
        self.ui.print.choices.destination = Some(chosen.unwrap_or(Destination::Pdf));
    }

    /// Takes the answers of the threads: the printers, the dialog of a PDF, the job. Runs before every frame.
    pub(crate) fn advance_print(&mut self, ctx: &Context) {
        if let Printers::Loading(rx) = &self.ui.print.printers {
            match rx.try_recv() {
                Ok(list) => {
                    self.ui.print.printers = Printers::Ready(list);
                    // A destination that is a printer the list no longer has is dropped.
                    if let (Some(Destination::Printer(name)), Some(list)) = (&self.ui.print.choices.destination, self.ui.print.printer_list())
                        && !list.iter().any(|p| &p.name == name)
                    {
                        self.ui.print.choices.destination = None;
                    }
                    self.choose_default_destination();
                    ctx.request_repaint();
                }
                Err(TryRecvError::Disconnected) => {
                    self.ui.print.printers = Printers::Ready(Vec::new());
                    self.choose_default_destination();
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        if let Some(rx) = &self.ui.print.pdf_pick {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.ui.print.pdf_pick = None;
                    self.print_pdf_to(ctx, path);
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.ui.print.pdf_pick = None,
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
            }
        }
        if let Some(path) = self.ui.print.want.take() {
            self.begin_print(ctx, path);
        }
        self.advance_print_job(ctx);
    }

    fn advance_print_job(&mut self, ctx: &Context) {
        let Some(running) = &self.ui.print.running else { return };
        let outcome = match running.rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the print stopped without an answer".to_owned()),
        };
        let (to, pdf) = (running.to.clone(), running.pdf.clone());
        self.ui.print.running = None;
        let name = self.display_name();
        match outcome {
            Ok(sent) => {
                let text = sent_text(&name, &to, &sent);
                self.opening.banner = None;
                self.ui.print.outcome = Some(text.clone());
                self.set_notice(&text);
                // In the browser the engine wrote the PDF in memory: it is handed to the person now.
                if let Some(pdf) = pdf {
                    self.hand_over(ctx, &pdf, &text, "");
                }
            }
            Err(why) => {
                if let Some(pdf) = pdf {
                    self.release_file(&pdf);
                }
                let text = format!("{name} was not printed to {to}. {}", sentence(&shown(&why, 300)));
                self.ui.print.outcome = Some(text.clone());
                self.opening.banner = Some(text);
            }
        }
        ctx.request_repaint();
    }

    /// Print: begins the job for the destination. A PDF with no path asks for one with the Save dialog; a program's path skips it.
    pub(crate) fn begin_print(&mut self, ctx: &Context, path: Option<String>) {
        self.commit_pending_text();
        // The button needs a dialog for a PDF; a program that names the file does not.
        if !(if path.is_some() { self.print_ready() } else { self.can_print() }) {
            return;
        }
        match self.ui.print.choices.destination.clone() {
            Some(Destination::Printer(name)) => self.print_to_printer(ctx, name),
            Some(Destination::Pdf) => match path {
                Some(path) => self.print_pdf_text(ctx, &path),
                None => self.ask_for_pdf(ctx),
            },
            None => {}
        }
    }

    /// The Save dialog of a PDF, on its thread.
    fn ask_for_pdf(&mut self, ctx: &Context) {
        let suggestion =
            format!("{}.pdf", Path::new(&self.display_name()).file_stem().map_or_else(|| "Drawing".into(), |s| s.to_string_lossy().into_owned()));
        // With a host (the browser) the page names the file, or the window's own card does (`file_host.rs`).
        if self.has_file_host() {
            let request = crate::save_kinds::SaveRequest { name: suggestion, kinds: DiacraftApp::pdf_kinds() };
            self.ui.print.pdf_pick = self.ask_for_name(ctx, crate::file_host::Purpose::PrintPdf, request);
            return;
        }
        let Some(picker) = self.ui.print.pdf_picker.clone() else { return };
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-print-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker(suggestion)).ok().flatten();
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.ui.print.pdf_pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// The file a person chose for the PDF: the type is `.pdf`, added when the name has none; any other type is told in words.
    pub(crate) fn print_pdf_to(&mut self, ctx: &Context, path: PathBuf) {
        let Some(text) = path.to_str() else {
            self.opening.banner = Some("That file name is not text DiaCraft can use, so nothing was printed.".to_owned());
            return;
        };
        let name = path.file_name().map_or_else(|| "the PDF".to_owned(), |n| shown(&n.to_string_lossy(), 64));
        let (text, typed) = match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) {
            Some(e) if e == "pdf" => (text.to_owned(), false),
            None => (format!("{text}.pdf"), true),
            Some(other) => {
                self.opening.banner = Some(format!(
                    "{name} was not printed: what a printer would be given is saved as a PDF, and \".{}\" is not one. Name the file with .pdf.",
                    shown(&other, 16)
                ));
                return;
            }
        };
        if crate::export::is_own_file(Path::new(&text), self.session().path()) {
            let name = shown(&Path::new(&text).file_name().map_or_else(|| "that file".to_owned(), |n| n.to_string_lossy().into_owned()), 64);
            self.opening.banner =
                Some(format!("{name} was not printed: that is the drawing's own file, which a print never writes. Choose another name."));
            return;
        }
        if typed && crate::ask::is_there(&text) {
            self.saving.replacing = Some(crate::ask::Replacing::Print(PathBuf::from(text)));
            ctx.request_repaint();
            return;
        }
        self.print_pdf_text(ctx, &text);
    }

    /// Writes what a printer would be given to `text`, a PDF, with the engine's `file.print`, on a thread and in a session of its own that holds a
    /// copy of the drawing (as an export does), so that the window is never held.
    pub(crate) fn print_pdf_text(&mut self, ctx: &Context, text: &str) {
        self.commit_pending_text();
        let name = shown(&Path::new(text).file_name().map_or_else(|| "the PDF".to_owned(), |n| n.to_string_lossy().into_owned()), 64);
        if crate::export::is_own_file(Path::new(text), self.session().path()) {
            self.opening.banner =
                Some(format!("{name} was not printed: that is the drawing's own file, which a print never writes. Choose another name."));
            return;
        }
        let Some(document) = self.session().document().cloned() else { return };
        let mut params = json!({ "path": text, "color": self.ui.print.choices.color().word() });
        if let (PageRange::Current, Some(page)) = (self.ui.print.choices.range, self.current_page())
            && let Some(map) = params.as_object_mut()
        {
            map.insert("page".to_owned(), json!(page));
        }
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = self.run_in_background("diacraft-print-pdf", move || {
            let outcome = guard::catch_panic(|| print_in_new_session(document, &params))
                .unwrap_or_else(|msg| Err(EngineError::Internal { cmd: "file.print".to_owned(), msg }));
            let outcome = outcome.map_err(|e| match e {
                EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } => msg,
                EngineError::Internal { .. } => "DiaCraft met an internal problem while printing. Please report this bug.".to_owned(),
                other => other.to_string(),
            });
            let _ = tx.send(outcome.map(|done| sent_from_command(&done)));
            wake.request_repaint();
        });
        if spawned.is_err() {
            self.opening.banner = Some(format!("{name} was not printed: this build cannot work in the background."));
            return;
        }
        self.ui.print.outcome = None;
        self.set_work_notice(&format!("Saving {name}..."));
        self.ui.print.running = Some(Running { to: name, pdf: Some(PathBuf::from(text)), rx, cancel: Arc::new(PrintCancel::new()) });
        ctx.request_repaint();
    }

    /// Sends the drawing to `printer`: the PDF of each run of sheets that share a paper, as a job each, on a thread.
    fn print_to_printer(&mut self, ctx: &Context, printer: String) {
        let Some(service) = self.ui.print.service.clone() else { return };
        let Some(document) = self.session().document().cloned() else { return };
        let pages = self.print_pages();
        let c = self.ui.print.choices.clone();
        let job = PrintJob {
            printer: Some(printer.clone()),
            title: shown(&self.display_name(), 120),
            copies: c.copies.clamp(1, MAX_COPIES),
            collate: c.collate,
            duplex: c.duplex,
            grayscale: c.grayscale,
        };
        let fonts = self.ui.print.fonts.clone();
        let cancel = Arc::new(PrintCancel::new());
        let (tx, rx) = channel();
        let (wake, held) = (ctx.clone(), Arc::clone(&cancel));
        let spawned = std::thread::Builder::new().name("diacraft-print".into()).spawn(move || {
            let outcome = guard::catch_panic(|| send_to_printer(service.as_ref(), &document, pages, &job, fonts, &held)).unwrap_or_else(|msg| {
                Err(format!("DiaCraft met an internal problem while printing ({}). Please report this bug.", shown(&msg, 120)))
            });
            let _ = tx.send(outcome);
            wake.request_repaint();
        });
        if spawned.is_err() {
            self.opening.banner = Some("The drawing was not printed: this build cannot work in the background.".to_owned());
            return;
        }
        let label =
            self.ui.print.printer_list().and_then(|l| l.iter().find(|p| p.name == printer)).map_or_else(|| printer.clone(), |p| p.shown().to_owned());
        self.ui.print.outcome = None;
        self.set_work_notice(&format!("Sending {} to {}...", self.display_name(), shown(&label, 64)));
        self.ui.print.running = Some(Running { to: shown(&label, 64), pdf: None, rx, cancel });
        ctx.request_repaint();
    }

    /// Stops the job under way, if it has not reached the spooler.
    pub fn cancel_print(&mut self) {
        if let Some(running) = &self.ui.print.running {
            running.cancel.cancel();
        }
    }

    /// `print.set`: the choices and the sheet on view.
    pub(crate) fn print_set(&mut self, p: &Map<String, Value>) -> Result<(), EngineError> {
        const ID: &str = "print.set";
        let text = |key: &str| match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(bad(ID, format!("`{key}` must be text"))),
        };
        let mut next = self.ui.print.choices.clone();
        if let Some(printer) = text("printer")? {
            next.destination = Some(if printer.eq_ignore_ascii_case(PDF_WORD) {
                Destination::Pdf
            } else {
                let list = self.ui.print.printer_list().ok_or_else(|| {
                    bad(ID, "the printers are not listed yet: open the Print view and wait for them (`print` in the window state says when)")
                })?;
                let found = list.iter().find(|q| q.name == printer).ok_or_else(|| {
                    let names: Vec<String> = list.iter().map(|q| shown(&q.name, 48)).collect();
                    bad(ID, format!("no printer `{}`; the printers are: {}, or `pdf`", shown(printer, 48), names.join(", ")))
                })?;
                Destination::Printer(found.name.clone())
            });
        }
        match p.get("copies") {
            None | Some(Value::Null) => {}
            Some(v) => {
                next.copies = v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (1..=MAX_COPIES).contains(n))
                    .ok_or_else(|| bad(ID, format!("`copies` must be a whole number from 1 to {MAX_COPIES}")))?;
            }
        }
        match p.get("collate") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(b)) => next.collate = *b,
            Some(_) => return Err(bad(ID, "`collate` must be true or false")),
        }
        if let Some(word) = text("two_sided")? {
            next.duplex = Duplex::from_word(word).ok_or_else(|| bad(ID, "`two_sided` must be default, off, long or short"))?;
        }
        if let Some(word) = text("color")? {
            next.grayscale = match word.to_ascii_lowercase().as_str() {
                "color" | "colour" => false,
                "grayscale" | "greyscale" | "black" => true,
                _ => return Err(bad(ID, "`color` must be color or grayscale")),
            };
        }
        if let Some(word) = text("pages")? {
            next.range = match word.to_ascii_lowercase().as_str() {
                "all" => PageRange::All,
                "current" => PageRange::Current,
                _ => return Err(bad(ID, "`pages` must be all or current")),
            };
        }
        let sheet = match p.get("sheet") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .filter(|n| *n >= 1)
                    .and_then(|n| usize::try_from(n - 1).ok())
                    .ok_or_else(|| bad(ID, "`sheet` must be a whole number from 1"))?,
            ),
        };
        let zoom = match p.get("zoom") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.eq_ignore_ascii_case("fit") => Some(None),
            Some(v) => Some(Some(
                v.as_f64()
                    .filter(|z| z.is_finite() && (MIN_ZOOM..=MAX_ZOOM).contains(z))
                    .ok_or_else(|| bad(ID, format!("`zoom` must be `fit` or a number of percent from {MIN_ZOOM} to {MAX_ZOOM}")))?,
            )),
        };
        // Nothing is changed until every part is judged.
        if next.range != self.ui.print.choices.range {
            self.ui.print.sheet = 0;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        self.ui.print.choices = next;
        if let Some(sheet) = sheet {
            let count = self.print_plan().map(|plan| plan.sheet_count()).unwrap_or(0);
            self.ui.print.sheet = sheet.min(count.saturating_sub(1));
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        if let Some(zoom) = zoom {
            self.ui.print.zoom = zoom;
            self.ui.print.pan = egui::Vec2::ZERO;
        }
        Ok(())
    }

    /// `print.start`: asks for the print; the next frame begins it.
    pub(crate) fn print_start(&mut self, p: &Map<String, Value>) -> Result<(), EngineError> {
        const ID: &str = "print.start";
        let path = match p.get("path") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(bad(ID, "`path` must be text")),
        };
        if self.ui.print.running.is_some() {
            return Err(EngineError::Failed { cmd: ID.to_owned(), msg: "a print is already under way".to_owned() });
        }
        if let Err(why) = self.print_plan() {
            return Err(EngineError::Failed { cmd: ID.to_owned(), msg: why });
        }
        match &self.ui.print.choices.destination {
            Some(Destination::Pdf) if path.is_none() => {
                return Err(bad(ID, "the destination is a PDF: give `path`, the file to write (a program opens no dialog)"));
            }
            Some(Destination::Printer(_)) if path.is_some() => return Err(bad(ID, "`path` is for a PDF; the destination is a printer")),
            Some(Destination::Printer(_)) if self.ui.print.service.is_none() => {
                return Err(EngineError::Failed { cmd: ID.to_owned(), msg: "this window has no printing service".to_owned() });
            }
            Some(_) => {}
            None => {
                return Err(EngineError::Failed {
                    cmd: ID.to_owned(),
                    msg: "no destination is chosen yet: the printers are still being listed, or `print.set` with `printer`".to_owned(),
                });
            }
        }
        self.ui.print.want = Some(path);
        Ok(())
    }

    /// The Print view's state as `window_state` gives it.
    pub(crate) fn print_state_json(&self) -> Value {
        let p = &self.ui.print;
        let (sheets, plan_error) = match p.planned.as_ref().map(|x| &x.plan) {
            Some(Ok(plan)) => (Some(plan.sheet_count()), None),
            Some(Err(why)) => (None, Some(why.clone())),
            None => (None, None),
        };
        json!({
            "destination": p.destination_word(),
            "copies": p.choices.copies,
            "collate": p.choices.collate,
            "two_sided": p.choices.duplex.word(),
            "color": if p.choices.grayscale { "grayscale" } else { "color" },
            "pages": p.choices.range.word(),
            "sheet": p.sheet + 1,
            "sheets": sheets,
            "zoom": p.zoom,
            "plan_error": plan_error,
            "printers": p.printer_list().map(|l| l.iter().map(|q| q.name.clone()).collect::<Vec<_>>()),
            "service": p.service.is_some(),
            "busy": self.print_working(),
            "last": p.outcome,
            "preview": {
                "requested": p.preview.requested,
                "received": p.preview.received,
                "drawn": p.preview.shown_for(self.ui.pictures.generation(), p.sheet).is_some(),
                "lost": p.preview.lost(),
            },
        })
    }
}

/// The least and most zoom of the preview, in percent. 100 is a sheet at its real size (96 points to an inch).
pub const MIN_ZOOM: f64 = 25.0;
pub const MAX_ZOOM: f64 = 400.0;

/// A sentence: the first letter up, a full stop at the end.
pub(crate) fn sentence(msg: &str) -> String {
    let mut chars = msg.trim().chars();
    let text = chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
    if text.ends_with('.') { text } else { format!("{text}.") }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// "Sent plan.vsdx to Office: 6 sheets." and, when pages left something out, how many kinds.
fn sent_text(name: &str, to: &str, sent: &Sent) -> String {
    let jobs = if sent.jobs > 1 { format!(" in {}", plural(sent.jobs, "job", "jobs")) } else { String::new() };
    let verb = if to.to_ascii_lowercase().ends_with(".pdf") { "Saved" } else { "Sent" };
    let what = if verb == "Saved" { format!("Saved the print of {name} as {to}") } else { format!("Sent {name} to {to}") };
    let mut text = format!("{what}: {}{jobs}.", plural(sent.sheets, "sheet", "sheets"));
    let receipts: Vec<&str> = sent.receipts.iter().map(String::as_str).filter(|r| !r.trim().is_empty()).collect();
    if !receipts.is_empty() {
        text.push_str(&format!(" ({})", shown(&receipts.join("; "), 160)));
    }
    match sent.approximated {
        0 => {}
        1 => text.push_str(" One kind of thing on the pages was left out or drawn approximately."),
        n => text.push_str(&format!(" {n} kinds of things on the pages were left out or drawn approximately.")),
    }
    text
}

/// What `file.print` answered, as what a job did.
fn sent_from_command(done: &Value) -> Sent {
    let kinds = done
        .get("skipped")
        .and_then(Value::as_array)
        .map_or(0, |list| list.iter().filter(|s| s.get("kind").and_then(Value::as_str).is_some_and(|k| k != "Guide")).count());
    Sent {
        sheets: done.get("sheet_count").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
        jobs: 1,
        receipts: Vec::new(),
        approximated: kinds,
    }
}

/// Runs `file.print` with `params` on a copy of the drawing, in a session of its own: the engine's command, and nothing else, makes the file.
fn print_in_new_session(document: Document, params: &Value) -> Result<Value, EngineError> {
    let mut session = Session::new();
    session.execute("file.new", &json!({}))?;
    session.edit("Copy of the drawing", |doc| {
        *doc = document;
        Ok(())
    })?;
    session.execute("file.print", params)
}

/// The work of a job to a printer, on its thread: plan, then for each run of sheets that share a paper make the PDF and hand it to the service.
fn send_to_printer(
    service: &dyn PrintService,
    document: &Document,
    pages: Pages,
    job: &PrintJob,
    fonts: PrintFonts,
    cancel: &PrintCancel,
) -> Result<Sent, String> {
    let plan = diacraft_engine::print::plan(document, pages).map_err(|e| e.to_string())?;
    let mut renderer = fonts.renderer();
    let color = if job.grayscale { PrintColor::Grayscale } else { PrintColor::Color };
    let options = PrintOptions { text: TextMode::AsText, title: Some(job.title.clone()), color, ..PrintOptions::default() };
    let runs = plan.runs();
    let mut sent = Sent { sheets: 0, jobs: runs.len(), receipts: Vec::new(), approximated: 0 };
    let mut kinds: Vec<diacraft_render::SkipKind> = Vec::new();
    for run in &runs {
        if cancel.is_cancelled() {
            return Err("the print was cancelled before anything was sent to the printer".to_owned());
        }
        let printed =
            diacraft_engine::print::pdf(&mut renderer, document, &plan, run.first..run.first + run.count, &options).map_err(|e| failure_words(&e))?;
        for s in &printed.skipped {
            if s.kind != diacraft_render::SkipKind::Guide && !kinds.contains(&s.kind) {
                kinds.push(s.kind);
            }
        }
        let receipt = service.submit(&printed.bytes, job, cancel)?;
        sent.receipts.push(receipt);
        sent.sheets += run.count;
    }
    sent.approximated = kinds.len();
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancel_runs_the_hook_once_whenever_it_comes() {
        let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let early = PrintCancel::new();
        let r = Arc::clone(&ran);
        early.on_cancel(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        early.cancel();
        early.cancel();
        assert_eq!(ran.load(Ordering::SeqCst), 1, "once");
        assert!(early.is_cancelled());
        // A hook set after the cancel runs at once.
        let r = Arc::clone(&ran);
        early.on_cancel(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(ran.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_words_of_a_result_are_sentences_with_no_command_in_them() {
        assert_eq!(sentence("the margins leave no room"), "The margins leave no room.");
        assert_eq!(sentence("Done."), "Done.");
        let sent = Sent { sheets: 6, jobs: 2, receipts: vec!["request id is Office-12".into(), String::new()], approximated: 2 };
        let text = sent_text("plan.vsdx", "Office", &sent);
        assert_eq!(
            text,
            "Sent plan.vsdx to Office: 6 sheets in 2 jobs. (request id is Office-12) 2 kinds of things on the pages were left out or drawn approximately."
        );
        let one = sent_text("a", "x.pdf", &Sent { sheets: 1, jobs: 1, receipts: vec![], approximated: 0 });
        assert_eq!(one, "Saved the print of a as x.pdf: 1 sheet.");
    }

    #[test]
    fn two_sided_words_round_trip() {
        for d in Duplex::ALL {
            assert_eq!(Duplex::from_word(d.word()), Some(d));
        }
        assert_eq!(Duplex::from_word("LONG"), Some(Duplex::LongEdge));
        assert_eq!(Duplex::from_word("both"), None);
    }
}
