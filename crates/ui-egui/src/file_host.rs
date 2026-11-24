//! Files in a window that has no file system and no threads of its own: the browser.
//!
//! On the desktop the window asks the app's native dialog for a path (`FilePicker`, `SavePicker`, `ExportPicker`: each runs on a thread of its own and blocks), the engine reads and writes that path, and a
//! file is read and made on a thread so that the window is never held. A browser has none of that: a file is chosen by a control the page makes (a file input, a drop, the File System Access API), its bytes
//! arrive later and in pieces, a result is handed to the person as a download (or written to a file they chose), and work done on the page's one thread holds the page. So the window is given a
//! [`FileHost`] (`DiacraftApp::with_file_host`), the page's side of all this, and **the rest of the window is the same code**:
//!
//! * The host answers a request to choose a file with a path **in the engine's memory** ([`diacraft_engine::memfs`]): the bytes are put there, under a name [`place`] makes, before the path is
//!   given, and the engine's commands (`file.open`, `stencil.open`, `insert.picture`) read that path as they read a path on a disk, with the same budgets and the same refusals in words.
//! * A save, an export and a print to a PDF are run by the engine's commands as they are on the desktop, onto a path in memory; what a command wrote is then **handed over** ([`FileHost::deliver`]): the
//!   host takes the bytes out and downloads them or writes them to the file the person chose.
//! * Where the host cannot name a file for a save itself (a browser without the File System Access API has no save dialog, only a download), **the window asks for the name** in a card of its own
//!   ([`NameCard`]): the name, and the kind of file where there is a choice.
//! * Work that a thread does on the desktop (reading a file, drawing an export) is done on the page's thread **one frame after the window says what it is doing** ([`DiacraftApp::run_in_background`]), so
//!   that the progress card is on the screen while the page is held.
//!
//! The window knows nothing of the browser: the host is a trait, and the tests of this flow run on the desktop with a host of their own.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use egui::{Context, Id, Key, Modifiers, vec2};

use crate::app::DiacraftApp;
use crate::save_kinds::{SaveKind, SaveRequest};
use crate::theme::{self, Tokens, size};

/// What a file is chosen for: the kind decides what the host lets the person choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// A drawing, a stencil or a template to open (`file.open`), and what is dropped on the page.
    Drawing,
    /// A stencil to open beside the drawing (`stencil.open`).
    Stencil,
    /// A picture to insert (`insert.picture`).
    Picture,
}

/// How a host handed a file over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivered {
    /// The browser downloaded it.
    Downloaded,
    /// It was written to the file the person chose.
    Wrote,
    /// It could not be written to the file the person chose (the reason, in words) and was downloaded instead.
    Fallback(String),
}

/// The page's side of the window's files. Every call returns at once: an answer that needs the person or the browser comes later, on the channel, and the host wakes the window (`Context::request_repaint`)
/// when it has sent it.
pub trait FileHost: Send + Sync {
    /// Asks the person to choose a file for `want`. The answer is the path in the engine's memory the file was put at ([`place`]), or `None` when nothing was chosen. A file that cannot be
    /// taken (it is past what a read may hold, it could not be read) is told in words through [`FileHost::refusals`] and is `None`.
    fn choose(&self, ctx: &Context, want: Want) -> Receiver<Option<PathBuf>>;

    /// Whether the host has a dialog of its own to name a file to save to (the File System Access API): if it has not, the window asks for the name itself ([`NameCard`]).
    fn names_saves(&self) -> bool {
        false
    }

    /// Asks the person, with the host's own dialog, where to save: the name to start with and the kinds offered, **the first the one selected**. The answer is the path in the engine's
    /// memory ([`place`]) with the name the person chose, or `None` when nothing was chosen. Called only when [`FileHost::names_saves`] says so.
    fn choose_save(&self, ctx: &Context, request: &SaveRequest) -> Receiver<Option<PathBuf>> {
        let _ = (ctx, request);
        let (_, never) = channel();
        never
    }

    /// Hands over what a command wrote at `path` in the engine's memory: the host takes the bytes out and downloads them, or writes them to the file the person chose for this path. The answer
    /// says how it went.
    fn deliver(&self, ctx: &Context, path: &Path) -> Receiver<Result<Delivered, String>>;

    /// The files dropped on the page since the last call, each already put in the engine's memory ([`place`]), the first dropped first.
    fn dropped(&self) -> Vec<PathBuf>;

    /// What the host has to say of a file it could not take (one dropped, or chosen), in words for the person; each is said once.
    fn refusals(&self) -> Vec<String>;

    /// Lets go of whatever the host keeps for `path`: the file in memory that was not opened, the handle to the file a save writes in place.
    fn release(&self, path: &Path);
}

thread_local! {
    static PLACES: Cell<u64> = const { Cell::new(0) };
}

/// The longest name a place keeps, in bytes: a browser's file name can be any length, and a path is no place for a megabyte.
const NAME_BYTES: usize = 160;

/// The path in the engine's memory for a file called `name`: `/web/N/name`, N a number no other place has, so that two files called alike never meet, and **the name never holds a separator or a
/// control character** (they become `_`) and is never empty, so the path is one name in one folder and nothing in it leads anywhere else. A name longer than [`NAME_BYTES`] is cut in its stem and
/// keeps its extension.
pub fn place(name: &str) -> PathBuf {
    let n = PLACES.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n
    });
    PathBuf::from(format!("/web/{n}/{}", safe_name(name)))
}

/// `name` as one file name: separators and control characters made `_`, ends trimmed, `file` for what is left empty or only dots, and cut to [`NAME_BYTES`] in its stem.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().all(|c| c == '.') {
        return "file".to_owned();
    }
    if cleaned.len() <= NAME_BYTES {
        return cleaned.to_owned();
    }
    // The extension is what follows the last dot, when it is short: it is kept, and the stem is cut to what room is left.
    let (stem, extension) = match cleaned.rfind('.') {
        Some(dot) if cleaned.len() - dot <= 24 => cleaned.split_at(dot),
        _ => (cleaned, ""),
    };
    let room = NAME_BYTES - extension.len();
    let mut end = room.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{extension}", &stem[..end])
}

/// What a name card is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// Save As (and Save of a drawing with no file).
    Save,
    /// Export a page as a picture.
    Export,
    /// Save what would be printed as a PDF.
    PrintPdf,
}

impl Purpose {
    fn heading(self) -> &'static str {
        match self {
            Purpose::Save => "Save the drawing",
            Purpose::Export => "Export the page as a picture",
            Purpose::PrintPdf => "Save what would be printed as a PDF",
        }
    }
}

/// The card that asks for the name of a file, and its kind where there is a choice, when the host has no dialog of its own for it.
pub(crate) struct NameCard {
    purpose: Purpose,
    request: SaveRequest,
    name: String,
    kind: usize,
    /// The name field takes the keyboard when the card first appears.
    fresh: bool,
}

impl NameCard {
    fn new(purpose: Purpose, request: SaveRequest) -> NameCard {
        let kind = Path::new(&request.name)
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|e| request.kinds.iter().position(|k| k.extension.eq_ignore_ascii_case(e)))
            .unwrap_or(0);
        NameCard { purpose, name: request.name.clone(), request, kind, fresh: true }
    }

    /// The offered kind the name's extension names, if it names one.
    fn kind_named(&self) -> Option<usize> {
        let extension = Path::new(&self.name).extension()?.to_str()?;
        self.request.kinds.iter().position(|k| k.extension.eq_ignore_ascii_case(extension))
    }

    /// Chooses kind `n`: the name's extension becomes the kind's (added when the name has none of the offered kinds').
    fn choose_kind(&mut self, n: usize) {
        let Some(kind) = self.request.kinds.get(n) else { return };
        let stem = match self.kind_named() {
            Some(_) => Path::new(&self.name).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            None => self.name.trim_end_matches('.').to_owned(),
        };
        self.name = format!("{stem}.{}", kind.extension);
        self.kind = n;
    }

    /// The name the card gives: as typed when its extension is one of the kinds', else with the chosen kind's added. `None` when the name is empty.
    fn answer(&self) -> Option<String> {
        let typed = self.name.trim();
        if typed.is_empty() || typed.chars().all(|c| c == '.') {
            return None;
        }
        if self.kind_named().is_some() {
            return Some(typed.to_owned());
        }
        let kind = self.request.kinds.get(self.kind)?;
        Some(format!("{}.{}", typed.trim_end_matches('.'), kind.extension))
    }
}

/// A file handed to the host, and what to say when it has been delivered.
struct Delivery {
    /// The first sentence of what the save, export or print said ("Saved plan.vsdx."), which the way the file was delivered follows.
    head: String,
    /// What the save said after that (a note about the file), which follows the way it was delivered.
    tail: String,
    rx: Receiver<Result<Delivered, String>>,
}

/// Work to do on the page's thread, and the frame it may be done from.
struct Deferred {
    /// The name `run_in_background` was given: what `cancel_background` finds the job by.
    name: String,
    from_frame: u64,
    body: Box<dyn FnOnce() + Send>,
}

/// What the window keeps of files in a place with no file system.
#[derive(Default)]
pub(crate) struct Hosting {
    host: Option<Arc<dyn FileHost>>,
    card: Option<NameCard>,
    deliveries: Vec<Delivery>,
    deferred: Vec<Deferred>,
}

/// The height of the list of kinds in the name card: nine rows of a radio button, the most kinds a save is offered.
const KINDS_HEIGHT: f32 = 232.0;

/// The frames that are drawn between work being asked for and being done: one at least, so the progress card is on the screen before the page is held.
const FRAMES_BEFORE_WORK: u64 = 2;

/// The most deliveries and deferred jobs the window keeps: a person cannot save faster than the page hands over, and none is kept past this.
const MOST_PENDING: usize = 16;

impl DiacraftApp {
    /// Gives the window the page's side of its files ([`FileHost`]). With one, the window opens, saves, exports and prints with no dialog of the desktop's and no thread: the file controls work.
    pub fn with_file_host(mut self, host: Arc<dyn FileHost>) -> DiacraftApp {
        self.hosting.host = Some(host);
        self
    }

    /// Whether the window has a host for files.
    pub fn has_file_host(&self) -> bool {
        self.hosting.host.is_some()
    }

    /// Asks the host to choose a file for `want`. `None` when the window has no host.
    pub(crate) fn host_choose(&self, ctx: &Context, want: Want) -> Option<Receiver<Option<PathBuf>>> {
        self.hosting.host.as_ref().map(|host| host.choose(ctx, want))
    }

    /// Whether the name card is on the screen for `purpose`.
    pub(crate) fn name_card_for(&self, purpose: Purpose) -> bool {
        self.hosting.card.as_ref().is_some_and(|card| card.purpose == purpose)
    }

    /// Whether the name card is on the screen.
    pub fn name_card_open(&self) -> bool {
        self.hosting.card.is_some()
    }

    /// Runs `body` where the window runs its slow work: **on a thread of its own on the desktop**, and **on the page's thread, once the window has drawn what it is doing, with a host** (a browser has no
    /// thread to give). `name` is the thread's. A body that cannot be started is an error, as a thread that will not start is.
    pub(crate) fn run_in_background(&mut self, name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        if self.hosting.host.is_none() {
            return std::thread::Builder::new().name(name.to_owned()).spawn(body).map(|_| ());
        }
        if self.hosting.deferred.len() >= MOST_PENDING {
            return Err(std::io::Error::other("too much work is waiting"));
        }
        self.hosting.deferred.push(Deferred {
            name: name.to_owned(),
            from_frame: self.frames.saturating_add(FRAMES_BEFORE_WORK),
            body: Box::new(body),
        });
        Ok(())
    }

    /// Drops the work `run_in_background` was given under `name` that has not begun (a file whose opening was cancelled before it was read). Work that is under way on a thread cannot be stopped, and
    /// work done on the page's thread cannot be interrupted: this is for what is still waiting. What the work held (a read's count) is let go with it.
    pub(crate) fn cancel_background(&mut self, name: &str) {
        self.hosting.deferred.retain(|job| job.name != name);
    }

    /// Lets go of what the host keeps for `path`, if the window has a host.
    pub(crate) fn release_file(&self, path: &Path) {
        if let Some(host) = &self.hosting.host {
            host.release(path);
        }
    }

    /// Does the work that has waited long enough: one job in a frame, so that what the first leaves (a card that says "Opening...") is drawn before the second holds the page. Runs before every frame.
    pub(crate) fn run_deferred(&mut self, ctx: &Context) {
        let due = self.hosting.deferred.first().is_some_and(|job| self.frames >= job.from_frame);
        if !due {
            if !self.hosting.deferred.is_empty() {
                ctx.request_repaint();
            }
            return;
        }
        let job = self.hosting.deferred.remove(0);
        (job.body)();
        ctx.request_repaint();
    }

    /// Takes what the host has for the window: files dropped on the page, what it could not take, and the end of what it was handing over. Runs before every frame.
    pub(crate) fn advance_host(&mut self, ctx: &Context) {
        let Some(host) = self.hosting.host.clone() else { return };
        for words in host.refusals() {
            self.opening.banner = Some(words);
        }
        let dropped = host.dropped();
        if !dropped.is_empty() {
            self.opened_by_drop(ctx, &host, dropped);
        }
        let mut waiting = Vec::new();
        for delivery in std::mem::take(&mut self.hosting.deliveries) {
            match delivery.rx.try_recv() {
                Ok(Ok(how)) => {
                    let Delivery { head, tail, .. } = delivery;
                    let clause = match &how {
                        Delivered::Downloaded | Delivered::Fallback(_) => "Your browser has downloaded the file.",
                        Delivered::Wrote => "It was written to the file you chose.",
                    };
                    self.set_drawing_notice(&if tail.is_empty() { format!("{head} {clause}") } else { format!("{head} {clause} {tail}") });
                    // What went wrong with the file the person chose is too long for the status bar: it is said over the drawing, where the words wrap.
                    if let Delivered::Fallback(why) = how {
                        self.opening.banner = Some(format!(
                            "The file could not be written where you chose ({}), so your browser downloaded it instead.",
                            diacraft_engine::shown(&why, 160)
                        ));
                    }
                }
                Ok(Err(why)) => {
                    self.opening.banner =
                        Some(format!("The file could not be handed over: {why} The drawing is saved in this window, and was not changed."));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => waiting.push(delivery),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.opening.banner = Some("The file could not be handed over: the browser gave no answer.".to_owned());
                }
            }
        }
        if !waiting.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.hosting.deliveries = waiting;
    }

    /// A file dropped on the page opens, after the question about unsaved changes: the first one. The others are let go, and said.
    fn opened_by_drop(&mut self, ctx: &Context, host: &Arc<dyn FileHost>, dropped: Vec<PathBuf>) {
        let mut files = dropped.into_iter();
        let Some(first) = files.next() else { return };
        let rest: Vec<PathBuf> = files.collect();
        for path in &rest {
            host.release(path);
        }
        // Another file is being read, or a question is on the screen: the file is not opened under it, and is let go.
        if self.is_modal_open() {
            host.release(&first);
            self.set_notice(
                "A file dropped on the page was not opened: answer the question on the screen, or wait for the file being read, and drop it again.",
            );
            return;
        }
        if !rest.is_empty() {
            self.set_notice("More than one file was dropped. The first was opened; drop the others one at a time.");
        }
        self.ask_then(ctx, crate::save::Then::Open(first));
    }

    /// Hands over the file a command wrote at `path`: the host takes the bytes out of the engine's memory and downloads them or writes them to the file the person chose. `head` is the first sentence of
    /// what the save said and `tail` the rest (a note about the file); what the host reports goes between them. Without a host, nothing happens: the command wrote a file on a disk.
    pub(crate) fn hand_over(&mut self, ctx: &Context, path: &Path, head: &str, tail: &str) {
        let Some(host) = self.hosting.host.clone() else { return };
        if self.hosting.deliveries.len() >= MOST_PENDING {
            // A host that has not answered sixteen files: the bytes are let go, and the person is told.
            host.release(path);
            self.opening.banner = Some(
                "The browser has not finished handing over the earlier files, so this one was not handed over. Wait, and save again.".to_owned(),
            );
            return;
        }
        let rx = host.deliver(ctx, path);
        self.hosting.deliveries.push(Delivery { head: head.to_owned(), tail: tail.to_owned(), rx });
        ctx.request_repaint();
    }

    /// Asks for the name of a file to write for `purpose`: with the host's own dialog when it has one, or with the window's card. A save's answer is taken by `advance_save`, an export's by
    /// `advance_export` and a print's by `advance_print`, through the channel the host's dialog answers on; the card's answer goes to the same place by `name_card`.
    /// → the host's answer, when the host's dialog was asked; `None` when the card was opened (it answers itself) or the window has no host.
    pub(crate) fn ask_for_name(&mut self, ctx: &Context, purpose: Purpose, request: SaveRequest) -> Option<Receiver<Option<PathBuf>>> {
        let host = self.hosting.host.clone()?;
        if host.names_saves() {
            return Some(host.choose_save(ctx, &request));
        }
        if request.kinds.is_empty() {
            self.set_notice("There is no type this can be saved as.");
            return None;
        }
        self.hosting.card = Some(NameCard::new(purpose, request));
        ctx.request_repaint();
        None
    }

    /// The name card, when there is one: a heading, the name, the kinds, and Save and Cancel. Escape cancels, Enter saves.
    pub(crate) fn name_card(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut card) = self.hosting.card.take() else { return };
        let screen = ctx.content_rect();
        let mut picked: Option<usize> = None;
        let mut saved = false;
        let mut cancelled = false;
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let answer = card.answer();
        // In a window too short for the whole list the list shrinks and scrolls, and the card still fits: the rest of the card is about 240 points.
        let kinds_height = (screen.height() - 240.0).clamp(72.0, KINDS_HEIGHT);
        let modal = egui::Modal::new(Id::unique("card_name")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_max_width((screen.width() - 80.0).clamp(240.0, 440.0));
            let heading = card.purpose.heading();
            let label = ui.add(egui::Label::new(egui::RichText::new(heading).font(theme::font(size::TAB + 1.0)).color(t.text)).wrap());
            label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, heading));
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Your browser saves the file to its downloads folder, under the name you give.")
                        .font(theme::font(size::LABEL))
                        .color(t.text_dim),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            ui.label(egui::RichText::new("File name").font(theme::font(size::LABEL)).color(t.text_dim));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut card.name).desired_width(ui.available_width()).char_limit(NAME_BYTES).font(theme::font(size::LABEL)),
            );
            edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "name_card.name"));
            seen.push(("name_card.name".to_owned(), edit.rect, true));
            if card.fresh {
                card.fresh = false;
                edit.request_focus();
            }
            if card.request.kinds.len() > 1 {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Type").font(theme::font(size::LABEL)).color(t.text_dim));
                // As wide as the card, so the scroll bar is at the card's edge and not over the end of the longest label; tall enough for every kind there is (nine) without scrolling.
                egui::ScrollArea::vertical().max_height(kinds_height).auto_shrink([false, true]).id_salt("name_card_kinds").show(ui, |ui| {
                    for (n, kind) in card.request.kinds.iter().enumerate() {
                        let chosen = card.kind_named().unwrap_or(card.kind) == n;
                        let row = ui.radio(chosen, egui::RichText::new(kind.label).font(theme::font(size::LABEL)));
                        seen.push((format!("name_card.kind.{}", kind.extension), row.rect, true));
                        if row.clicked() {
                            picked = Some(n);
                        }
                    }
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let save = ui.add_enabled(
                    answer.is_some(),
                    egui::Button::new(egui::RichText::new("Save").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)),
                );
                seen.push(("name_card.save".to_owned(), save.rect, answer.is_some()));
                saved = save.clicked();
                let cancel = ui.add(egui::Button::new(egui::RichText::new("Cancel").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                seen.push(("name_card.cancel".to_owned(), cancel.rect, true));
                cancelled = cancel.clicked();
            });
            // Enter in the name field saves, as it does in a dialog.
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && answer.is_some() {
                saved = true;
            }
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            cancelled = true;
        }
        if let Some(n) = picked {
            card.choose_kind(n);
        }
        if cancelled {
            // A save that waited on the name (a New, an Open or the close that asked about unsaved changes) does not happen.
            if card.purpose == Purpose::Save {
                self.saving.after = None;
            }
            return;
        }
        if saved && let Some(name) = card.answer() {
            let path = place(&name);
            match card.purpose {
                Purpose::Save => self.save_to(ctx, path),
                Purpose::Export => self.export_to(ctx, path),
                Purpose::PrintPdf => self.print_pdf_to(ctx, path),
            }
            return;
        }
        self.hosting.card = Some(card);
    }

    /// The kinds an export is offered as: what the engine writes, as the desktop's dialog lists them.
    pub(crate) fn export_kinds() -> Vec<SaveKind> {
        vec![
            SaveKind { label: "PNG image (.png)", extension: "png" },
            SaveKind { label: "SVG drawing (.svg)", extension: "svg" },
            SaveKind { label: "PDF document (.pdf)", extension: "pdf" },
        ]
    }

    /// The only kind a print to a file is offered as.
    pub(crate) fn pdf_kinds() -> Vec<SaveKind> {
        vec![SaveKind { label: "PDF document (.pdf)", extension: "pdf" }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_one_file_name_and_never_a_path() {
        assert_eq!(safe_name("plan.vsdx"), "plan.vsdx");
        assert_eq!(safe_name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(safe_name("a\\b/c.odg"), "a_b_c.odg");
        assert_eq!(safe_name("tab\there\u{0}.diacraft"), "tab_here_.diacraft");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name("   "), "file");
        assert_eq!(safe_name("  x.png  "), "x.png");
    }

    #[test]
    fn a_long_name_is_cut_in_its_stem_and_keeps_its_extension() {
        let long = format!("{}.vsdx", "é".repeat(400));
        let name = safe_name(&long);
        assert!(name.len() <= NAME_BYTES, "{}", name.len());
        assert!(name.ends_with(".vsdx"), "{name}");
        assert!(name.starts_with('é'));
        let no_extension = safe_name(&"x".repeat(1000));
        assert_eq!(no_extension.len(), NAME_BYTES);
        let odd_dot = safe_name(&format!("{}.{}", "y".repeat(300), "z".repeat(100)));
        assert!(odd_dot.len() <= NAME_BYTES, "a long 'extension' is no extension: {}", odd_dot.len());
    }

    #[test]
    fn two_places_for_one_name_are_two_paths() {
        let (a, b) = (place("plan.vsdx"), place("plan.vsdx"));
        assert_ne!(a, b);
        assert_eq!(a.file_name().and_then(|n| n.to_str()), Some("plan.vsdx"));
        assert_eq!(b.extension().and_then(|n| n.to_str()), Some("vsdx"));
        assert!(a.starts_with("/web/"));
        assert_eq!(a.components().count(), 4, "the root, `web`, a number and the name: {a:?}");
    }

    fn request() -> SaveRequest {
        SaveRequest {
            name: "Drawing1.diacraft".to_owned(),
            kinds: vec![
                SaveKind { label: "DiaCraft drawing (.diacraft)", extension: "diacraft" },
                SaveKind { label: "OpenDocument drawing (.odg)", extension: "odg" },
                crate::save_kinds::kind_of("vsdx").unwrap(),
            ],
        }
    }

    #[test]
    fn the_card_starts_on_the_kind_the_name_has_and_a_kind_chosen_changes_the_extension() {
        let mut card = NameCard::new(Purpose::Save, request());
        assert_eq!((card.kind, card.name.as_str()), (0, "Drawing1.diacraft"));
        card.choose_kind(2);
        assert_eq!((card.kind, card.name.as_str()), (2, "Drawing1.vsdx"));
        card.choose_kind(1);
        assert_eq!(card.name, "Drawing1.odg");
        // A name with an extension that is none of the kinds' gets the kind's added.
        card.name = "my.plan".to_owned();
        assert_eq!(card.kind_named(), None);
        card.choose_kind(2);
        assert_eq!(card.name, "my.plan.vsdx");
        let named_odg = NameCard::new(Purpose::Save, SaveRequest { name: "x.ODG".to_owned(), ..request() });
        assert_eq!(named_odg.kind, 1, "in any case");
    }

    #[test]
    fn the_name_the_card_gives_has_a_type_of_the_kinds() {
        let mut card = NameCard::new(Purpose::Save, request());
        assert_eq!(card.answer().as_deref(), Some("Drawing1.diacraft"));
        card.name = "plan".to_owned();
        card.kind = 2;
        assert_eq!(card.answer().as_deref(), Some("plan.vsdx"), "no type: the chosen kind's");
        card.name = "plan.".to_owned();
        assert_eq!(card.answer().as_deref(), Some("plan.vsdx"));
        card.name = "plan.odg".to_owned();
        assert_eq!(card.answer().as_deref(), Some("plan.odg"), "a type of the kinds: as typed");
        for empty in ["", "   ", ".", "..."] {
            card.name = empty.to_owned();
            assert_eq!(card.answer(), None, "{empty:?}");
        }
    }
}
