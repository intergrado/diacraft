//! Saving from the window: Save and Save As, the unsaved-changes mark in the title, and asking before a change of drawing throws changes away.
//!
//! The commands are the engine's: `file.save` writes the drawing to the file it came from or was last saved to, and `file.save_as {path}`
//! writes it where it is told, taking the kind of file from the extension. A drawing that has no file yet has none to save to, so Save
//! asks for one with the native Save dialog (the app's `SavePicker`, run on a thread of its own as Open's is) and runs `file.save_as`.
//! Whether there are unsaved changes is the engine's ([`Session::is_dirty`]): an edit and its undo leave the drawing clean again.
//!
//! **Asking.** `file.new` and `file.open` replace the document without asking (they are commands, and a program that runs one never gets a
//! dialog), so the window asks first when the drawing has unsaved changes: New, Open (once a file is chosen) and closing the window. The
//! question has three answers: Save (and then go on, if the save worked), Don't Save, and Cancel. A close asked for by `app.quit` through
//! the control channel is a program's, and is not asked about.
//!
//! **A label being typed is part of the drawing when any of this asks** (of the M3 review): Save, Save As, New, Open and the close
//! send the open text box first (`DiacraftApp::commit_pending_text`), so the question is about the drawing as the person sees it.
//!
//! **The kinds the Save dialog offers** (`save_kinds.rs`): the drawing's own type first (`.diacraft` for a new drawing, the default from 0.2), then the others that the
//! engine says it writes for this drawing, in the design's order. **The dialog keeps the name's extension in step with the kind chosen** (`dialogs.rs`): the name it starts with
//! has the first kind's extension, and choosing another kind gives the name that kind's. A name the dialog still returns with no type (one that does not set it, such as the zenity fallback
//! on Linux) gets the drawing's own type from the window.
//!
//! **Two more questions** (`ask.rs`). A name the Save dialog returned with no file type gets the drawing's own type, and if that file is there the window asks
//! before replacing it (M2). A save onto a file that changed on the disk since the drawing was opened or last saved is refused by the engine
//! (`EngineError::ChangedOnDisk`); the window asks whether to overwrite it (the command is run again with `force`), save as another file, or
//! cancel (M3).
//!
//! **A third**. A Save As the engine refuses because the drawing holds a macro project and the type chosen cannot hold one
//! (`EngineError::MacrosNotAllowed`) is asked about, in the engine's words: "Leave the macros out?" with Save without macros, which runs `file.save_as` again
//! with `drop_macros`, **Save as .vsdm** (or `.vssm`, `.vstm`: the macro-enabled type of the one chosen), which runs `file.save_as` onto that name and keeps the project, and Cancel. The
//! save is still under way while the card is up, so a New, an Open or a close that waits on it waits for the answer.
//!
//! **The save note** (`save_note.rs`). A save across the VSDX and OpenDocument formats that carries data the type has no place for, a macro project, objects or signatures is told in
//! a card over the drawing, in the engine's words for the save result's `carried` field, once for the save.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use diacraft_engine::{EngineError, guard, shown};
use egui::Context;
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::ask::{Card, Replacing, SaveAgain, SaveWithout, is_there};
use crate::save_kinds::{SavePicker, SaveRequest, macro_twin, save_kinds};
use crate::theme::Tokens;

/// The extension a name chosen without one gets, when the drawing has no file of a type to take it from: a new drawing is a `.diacraft` drawing, DiaCraft's own format and its
/// default from 0.2. A drawing that was opened from a file, or saved to one, is saved as the type it has ([`DiacraftApp::own_extension`]).
pub const DEFAULT_EXTENSION: &str = "diacraft";

/// What the window does once the question about unsaved changes is answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// A new, empty drawing.
    New,
    /// A new drawing made from the built-in template with this id (`file.new {template}`; File > New's tiles).
    Template(String),
    /// Open this file.
    Open(PathBuf),
    /// Close the window.
    Close,
}

/// The window's part of saving.
#[derive(Default)]
pub(crate) struct Saving {
    picker: Option<SavePicker>,
    /// The Save dialog, running on its thread, and what to do after the save that follows it.
    pick: Option<Receiver<Option<PathBuf>>>,
    /// The question about unsaved changes on the screen, and what it is about.
    pub(crate) asking: Option<Then>,
    /// What to do when the save that is under way has worked.
    pub(crate) after: Option<Then>,
    /// The question about a file that is there, when a name the window gave a file type to names one (M2).
    pub(crate) replacing: Option<Replacing>,
    /// The question about a file that changed on the disk, and the save to run again if the answer is to overwrite it (M3).
    pub(crate) changed: Option<SaveAgain>,
    /// The question about a macro project the type chosen cannot hold, and the save to run again if the answer is to leave the macros out.
    pub(crate) macros: Option<SaveWithout>,
    /// The window may close: the person said so, or a program did.
    pub(crate) may_close: bool,
    /// The save note of an `.odg` has been shown in this session: it is shown once.
    pub(crate) note_shown: bool,
    /// The save note of the last save, on the screen until it is dismissed, the next save replaces it, or another drawing takes it away.
    pub(crate) note: Option<crate::save_note::SaveNote>,
    /// How many saves have made a note in this session: each note has a number of its own.
    pub(crate) notes_made: u64,
}

impl DiacraftApp {
    /// Gives the window a native Save dialog. Without one, a drawing with no file yet cannot be saved from the window.
    pub fn with_save_picker(mut self, picker: SavePicker) -> DiacraftApp {
        self.saving.picker = Some(picker);
        self
    }

    /// Whether the drawing has changes that are not saved. The engine's answer ([`diacraft_engine::Session::is_dirty`]).
    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// Whether the question about unsaved changes is on the screen.
    pub fn prompt_open(&self) -> bool {
        self.saving.asking.is_some()
    }

    /// Whether the Save dialog is open.
    pub fn save_dialog_open(&self) -> bool {
        self.saving.pick.is_some()
    }

    /// Whether Save can be used now: a drawing is open and the engine has the commands.
    pub fn can_save(&self) -> bool {
        self.command_enabled("file.save").is_ok() && !self.save_dialog_open()
    }

    /// Whether Save As can be used now: as [`DiacraftApp::can_save`], and the app has a dialog to ask for the name.
    pub fn can_save_as(&self) -> bool {
        self.command_enabled("file.save_as").is_ok()
            && (self.saving.picker.is_some() || self.has_file_host())
            && !self.save_dialog_open()
            && !self.name_card_for(crate::file_host::Purpose::Save)
    }

    /// Save: writes the drawing to its file, or asks for one if it has none (Ctrl+S, the Quick Access Toolbar, the File view).
    pub fn save(&mut self, ctx: &Context) {
        self.commit_pending_text();
        if !self.can_save() {
            return;
        }
        if self.session.path().is_some() {
            self.save_in_place(ctx, false);
        } else {
            self.save_as(ctx);
        }
    }

    /// Save As: asks for a file with the dialog and writes the drawing there (Ctrl+Shift+S).
    pub fn save_as(&mut self, ctx: &Context) {
        self.commit_pending_text();
        // (A drawing that came from a flat `.fodg` is saved as any drawing is, made `file.save_as` usable for it, so nothing here is its own.)
        if self.command_enabled("file.save_as").is_err() || self.save_dialog_open() || self.name_card_for(crate::file_host::Purpose::Save) {
            return;
        }
        // With a host (the browser) the page names the file, or the window's own card does (`file_host.rs`).
        if self.has_file_host() {
            let request = self.save_request();
            if let Some(rx) = self.ask_for_name(ctx, crate::file_host::Purpose::Save, request) {
                self.saving.pick = Some(rx);
            }
            return;
        }
        let Some(picker) = self.saving.picker.clone() else {
            self.set_notice("Saving to a new file needs a file dialog, which this build does not have.");
            return;
        };
        let request = self.save_request();
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-save-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker(request)).ok().flatten();
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.saving.pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// The extension of the type the drawing is saved as: **the type it was read as, or last saved as** (what the package is, which the engine sniffed), and not what the
    /// file is called, which may be anything (an `.odg` called `a.vsdx`, a VSDX file called `v.odg`: the engine refuses to save either as the type its name says). A drawing
    /// with no file of a type (a new one, and one made from a template: the bytes it has are the template's, and are no file) is saved as [`DEFAULT_EXTENSION`], `.diacraft`.
    fn own_extension(&self) -> &'static str {
        self.session.source().filter(|f| f.format().is_some()).and_then(|f| f.extension()).unwrap_or(DEFAULT_EXTENSION)
    }

    /// What the Save dialog is asked: [the name it starts with](DiacraftApp::suggested_file_name), and the kinds of file it offers, **the drawing's own first**: the dialog
    /// selects its first kind. Which kinds are offered is the engine's answer (`Session::save_as_allows`, [`save_kinds`]), and their order the design's.
    pub(crate) fn save_request(&self) -> SaveRequest {
        let stencil = self.session.source().is_some_and(|f| f.is_stencil());
        SaveRequest {
            name: self.suggested_file_name(),
            kinds: save_kinds(self.own_extension(), stencil, |extension| self.session.save_as_allows(extension)),
        }
    }

    /// The name the Save dialog starts with: the drawing's own file name **with the extension of the type it is saved as** (`a.vsdx` of an `.odg` is `a.odg`), or, for a
    /// drawing with no file, its name with `.diacraft` when it has no extension.
    fn suggested_file_name(&self) -> String {
        let name = self.display_name();
        let has_file = self.session.source().is_some_and(|f| f.is_named());
        let own = self.own_extension();
        let path = std::path::Path::new(&name);
        if has_file && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            return format!("{stem}.{own}");
        }
        if path.extension().is_some() { name } else { format!("{name}.{own}") }
    }

    /// `file.save`, with `force` when the person has said to overwrite a file that changed on the disk. A file that changed is asked about.
    pub(crate) fn save_in_place(&mut self, ctx: &Context, force: bool) {
        let params = if force { json!({ "force": true }) } else { json!({}) };
        match self.execute("file.save", &params) {
            Ok(done) => self.after_save(ctx, &done),
            Err(EngineError::ChangedOnDisk { .. }) => self.ask_changed(ctx, SaveAgain::InPlace),
            Err(e) => self.save_failed(&e),
        }
    }

    /// Writes the drawing to the file the Save dialog returned. **A name with no file type gets the drawing's own type here** (`.diacraft` for a new drawing, `.vsdx` for one
    /// opened from a VSDX file, `.odg` for a drawing that came from an `.odg`): the fallback for a dialog that did not set the extension of the kind chosen (see the module
    /// docs). It comes after the dialog's own check of the name it was given, so when that file is there the person is asked before it is replaced (M2).
    pub(crate) fn save_to(&mut self, ctx: &Context, path: PathBuf) {
        let Some(text) = path.to_str() else {
            self.opening.banner = Some("That file name is not text DiaCraft can use, so the drawing was not saved.".to_owned());
            self.saving.after = None;
            return;
        };
        let typed = path.extension().is_none();
        let text = if typed { format!("{text}.{}", self.own_extension()) } else { text.to_owned() };
        if typed && is_there(&text) {
            self.saving.replacing = Some(Replacing::Save(text));
            ctx.request_repaint();
            return;
        }
        self.save_text_to(ctx, &text, false);
    }

    /// `file.save_as` to `text`, with `force` when the person has said to overwrite the drawing's own file that changed on the disk.
    pub(crate) fn save_text_to(&mut self, ctx: &Context, text: &str, force: bool) {
        self.save_text_with(ctx, text, force, false);
    }

    /// `file.save_as` to `text`, with `force` when the person has said to overwrite the drawing's own file that changed on the disk, and with
    /// `drop_macros` when the person has said to leave the macro project out. **A save the engine refuses for the macros is asked about, once**: the card
    /// ([`SaveWithout`]) keeps the name and `force`, and the save runs again with `drop_macros`; a refusal that comes after the person said to leave them
    /// out is a failure like another, told in the banner. The save is still under way while the card is up, so what waits on it (`saving.after`) is kept.
    pub(crate) fn save_text_with(&mut self, ctx: &Context, text: &str, force: bool, drop_macros: bool) {
        let mut params = serde_json::Map::new();
        params.insert("path".to_owned(), json!(text));
        if force {
            params.insert("force".to_owned(), json!(true));
        }
        if drop_macros {
            params.insert("drop_macros".to_owned(), json!(true));
        }
        match self.execute("file.save_as", &Value::Object(params)) {
            Ok(done) => self.after_save(ctx, &done),
            Err(EngineError::ChangedOnDisk { .. }) => self.ask_changed(ctx, SaveAgain::As { path: text.to_owned(), drop_macros }),
            Err(EngineError::MacrosNotAllowed { msg, .. }) if !drop_macros => {
                let keep_as = self.macro_enabled_name(text);
                self.saving.macros = Some(SaveWithout { path: text.to_owned(), force, words: sentence(&shown(&msg, WORDS_SHOWN)), keep_as });
                ctx.request_repaint();
            }
            Err(e) => self.save_failed(&e),
        }
    }

    /// The name the drawing is saved under to keep its macro project, when the type of `text` cannot hold one: the same name with the macro-enabled type of it (`.vsdx` is `.vsdm`, `.vssx`
    /// `.vssm`, `.vstx` `.vstm`), and the type. `None` when the type has no such twin, or the engine does not write the twin for this drawing (`Session::save_as_allows`).
    pub(crate) fn macro_enabled_name(&self, text: &str) -> Option<(String, &'static str)> {
        let path = std::path::Path::new(text);
        let twin = macro_twin(path.extension()?.to_str()?)?;
        if !self.session.save_as_allows(twin) {
            return None;
        }
        Some((path.with_extension(twin).to_str()?.to_owned(), twin))
    }

    /// The answer to the macros card to keep the project: `file.save_as` onto `path`, the macro-enabled name, with no `drop_macros`. **A file that is there is asked about first**
    /// (the dialog asked about the name it gave, and not this one), unless it is the drawing's own file, which a Save would replace as well. The save is still under way while it is
    /// asked, so what waits on it (`saving.after`) is kept.
    pub(crate) fn save_macro_enabled(&mut self, ctx: &Context, path: &str, force: bool) {
        let own = self.session.path().is_some_and(|own| own == std::path::Path::new(path));
        if is_there(path) && !own {
            self.saving.replacing = Some(Replacing::Macros(path.to_owned()));
            ctx.request_repaint();
        } else {
            self.save_text_with(ctx, path, force, false);
        }
    }

    /// The file changed on the disk: asks what to do, and keeps what to do afterwards (`saving.after`) until it is answered.
    fn ask_changed(&mut self, ctx: &Context, again: SaveAgain) {
        self.saving.changed = Some(again);
        ctx.request_repaint();
    }

    /// Whether the Save dialog the question about a changed file offers can be shown: the app has one.
    pub(crate) fn can_save_as_now(&self) -> bool {
        (self.saving.picker.is_some() || self.has_file_host()) && self.command_enabled("file.save_as").is_ok()
    }

    /// A save worked: say so, and go on with what the question was about.
    fn after_save(&mut self, ctx: &Context, done: &Value) {
        let notes = done.get("warning_count").and_then(Value::as_u64).unwrap_or(0);
        if notes > 0 {
            log::warn!("saving {} gave {notes} notes", self.display_name());
        }
        let name = self.display_name();
        // What the save says of what it did not write is in the status bar, the first note in its words and the number of the rest, and all of them in the log.
        let first = done.get("warnings").and_then(Value::as_array).and_then(|w| w.first()).and_then(Value::as_str);
        let suffix = match (notes, first) {
            (0, _) => String::new(),
            (1, Some(first)) => format!(" {}", sentence(first)),
            (n, Some(first)) => format!(" {} ({} more notes about the file are in the log.)", sentence(first), n - 1),
            (n, None) => format!(" ({n} notes about the file are in the log.)"),
        };
        let head = format!("Saved {name}.");
        self.set_drawing_notice(&format!("{head}{suffix}"));
        self.opening.banner = None;
        // In the browser the engine wrote the file in memory: it is handed to the person now (a download, or a write to the file they chose).
        if let Some(path) = self.session.path().map(std::path::Path::to_path_buf) {
            self.hand_over(ctx, &path, &head, suffix.trim());
        }
        // What the file carries (the design's 5.6): the note of this save, in the engine's words for the result's `carried` field.
        self.set_save_note(done);
        // The save note of an `.odg` (design decision 8): what a LibreOffice save would drop of the drawing, said once in a session in the banner, which a person
        // dismisses. A program is told in every result (`notes`).
        if !self.saving.note_shown
            && let Some(words) =
                done.get("notes").and_then(Value::as_array).and_then(|n| n.first()).and_then(|n| n.get("words")).and_then(Value::as_str)
        {
            self.opening.banner = Some(words.to_owned());
            self.saving.note_shown = true;
        }
        if let Some(then) = self.saving.after.take() {
            self.proceed(ctx, then);
        }
    }

    /// The drawing was replaced (`file.new`, an opened file: by the person or by a program on the control channel): a question about saving the drawing that
    /// is gone is closed, and what waited on that save is let go. Left up, Save without, Overwrite or Replace would write the new drawing, under words
    /// about the old one's macro project or file, and then do the New, the Open or the close that was waiting. The questions about an export and about
    /// unsaved changes are not about a save of the drawing, and stay.
    pub(crate) fn forget_save_questions(&mut self) {
        self.saving.macros = None;
        self.saving.changed = None;
        if matches!(self.saving.replacing, Some(Replacing::Save(_) | Replacing::Macros(_))) {
            self.saving.replacing = None;
        }
        self.saving.after = None;
        // The note is about the file the drawing that is gone was saved to.
        self.saving.note = None;
    }

    fn save_failed(&mut self, error: &EngineError) {
        self.saving.after = None;
        self.opening.banner = Some(save_failure_text(&self.display_name(), error));
    }

    /// Takes the Save dialog's answer when it comes. Runs before every frame.
    pub(crate) fn advance_save(&mut self, ctx: &Context) {
        self.watch_close(ctx);
        let Some(rx) = &self.saving.pick else { return };
        match rx.try_recv() {
            Ok(Some(path)) => {
                self.saving.pick = None;
                self.save_to(ctx, path);
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.saving.pick = None;
                self.saving.after = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
        }
    }

    /// Asks before the window closes with unsaved changes. A close a program asked for (`app.quit`) is not asked about.
    ///
    /// **A label being typed is sent first** (M1): a close from the desktop's frame or Alt+F4 finds the drawing changed, and asks. A close while
    /// another question is on the screen is held back (the person answers that one), and not asked about on top of it.
    pub(crate) fn watch_close(&mut self, ctx: &Context) {
        let close_asked = ctx.input(|i| i.viewport().close_requested());
        if !close_asked || self.saving.may_close || self.quit_requested() {
            return;
        }
        self.commit_pending_text();
        if self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.saving.replacing.is_none() && self.saving.changed.is_none() && self.saving.macros.is_none() {
                self.saving.asking = Some(Then::Close);
            }
        }
    }

    /// Does `then`, after asking first if the drawing has unsaved changes. A label being typed is sent first, so it counts (M1).
    pub fn ask_then(&mut self, ctx: &Context, then: Then) {
        self.commit_pending_text();
        if self.is_dirty() {
            self.saving.asking = Some(then);
            ctx.request_repaint();
        } else {
            self.proceed(ctx, then);
        }
    }

    fn proceed(&mut self, ctx: &Context, then: Then) {
        match then {
            Then::New => match self.execute("file.new", &json!({})) {
                Ok(_) => self.ui.backstage = None,
                Err(e) => self.set_notice(&e.to_string()),
            },
            Then::Template(id) => match self.execute("file.new", &json!({"template": id})) {
                Ok(_) => self.ui.backstage = None,
                Err(e) => self.set_notice(&e.to_string()),
            },
            Then::Open(path) => self.open_path(ctx, path),
            Then::Close => {
                self.saving.may_close = true;
                // In a browser the window is not closed by a viewport command (the web runner has none for it): the page is asked, and says when the browser keeps the window.
                if !self.close_in_browser() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// The answer to the question: Save, and then go on if it worked.
    fn answer_save(&mut self, ctx: &Context) {
        let Some(then) = self.saving.asking.take() else { return };
        self.saving.after = Some(then);
        if self.session.path().is_some() {
            self.save_in_place(ctx, false);
        } else {
            self.save_as(ctx);
            // No dialog could be opened: nothing will come back, and nothing is waiting. (The window's own card for a name is a dialog that answers itself.)
            if !self.save_dialog_open() && !self.name_card_for(crate::file_host::Purpose::Save) {
                self.saving.after = None;
            }
        }
    }

    /// The question about unsaved changes, over the window: three buttons, and a shade behind it that nothing under takes the pointer through.
    pub(crate) fn save_prompt(&mut self, ctx: &Context, t: Tokens) {
        let Some(then) = self.saving.asking.clone() else { return };
        let name = self.display_name();
        let words = match then {
            Then::New | Then::Template(_) => "A new drawing replaces this one.",
            Then::Open(_) => "The file you chose replaces this drawing.",
            Then::Close => "DiaCraft is closing.",
        };
        let heading = format!("Do you want to save changes to {name}?");
        let detail = format!("{words} Changes that are not saved will be lost.");
        let card = Card {
            id: "card_unsaved",
            heading: &heading,
            detail: &detail,
            buttons: &[("Save", "prompt.save", true), ("Don't Save", "prompt.dont_save", true), ("Cancel", "prompt.cancel", true)],
            cancel: 2,
        };
        match self.show_card(ctx, t, &card) {
            Some(0) => self.answer_save(ctx),
            Some(1) => {
                if let Some(then) = self.saving.asking.take() {
                    self.proceed(ctx, then);
                }
            }
            Some(_) => self.saving.asking = None,
            None => {}
        }
    }
}

/// The most characters of the engine's refusal the card about the macros shows: the whole of its words, which end with the way out.
const WORDS_SHOWN: usize = 700;

/// The engine's words as a sentence: a capital letter first and a full stop at the end.
fn sentence(msg: &str) -> String {
    let mut chars = msg.trim().chars();
    let text = chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
    if text.ends_with('.') { text } else { format!("{text}.") }
}

/// What a person is told when a save did not work: the engine's own words (they hold no path and no class), as a sentence.
pub fn save_failure_text(name: &str, error: &EngineError) -> String {
    match error {
        EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } => {
            format!("{name} was not saved. {} The drawing is as it was, and the file on the disk was not changed.", sentence(&shown(msg, 300)))
        }
        EngineError::Internal { .. } => {
            format!("DiaCraft met an internal problem while saving {name}, so it was not saved. The drawing is as it was. Please report this bug.")
        }
        other => format!("{name} was not saved: {}.", shown(&other.to_string(), 300)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_told_as_a_sentence_that_says_nothing_was_changed() {
        let link = EngineError::Failed {
            cmd: "file.save_as".into(),
            msg: "the drawing could not be saved: a link stands where the file should be, and it does not lead to this drawing's own file; name the file itself"
                .into(),
        };
        let text = save_failure_text("plan.vsdx", &link);
        assert!(text.starts_with("plan.vsdx was not saved. The drawing could not be saved: a link stands"), "{text}");
        assert!(text.contains("name the file itself.") && text.contains("not changed"), "{text}");
        assert!(!text.contains("file.save_as") && !text.contains('`'), "no command id: {text}");
    }

    /// The engine's words, as the card and the status bar show them: a capital letter first, a full stop at the end and not twice, the ends trimmed.
    #[test]
    fn the_engines_words_become_a_sentence() {
        assert_eq!(sentence("the drawing could not be saved: no room"), "The drawing could not be saved: no room.");
        assert_eq!(sentence("  already said.  "), "Already said.");
        assert_eq!(sentence("a part (/x/y.bin)"), "A part (/x/y.bin).");
        assert_eq!(sentence("éclair"), "Éclair.");
    }

    #[test]
    fn a_bug_is_told_without_its_details() {
        let text = save_failure_text("a.vsdx", &EngineError::Internal { cmd: "file.save".into(), msg: "index out of bounds: secret".into() });
        assert!(text.contains("internal problem") && !text.contains("secret"), "{text}");
    }
}
