//! Insert, Illustrations, Pictures: a picture from a file, through the desktop's own file chooser, as a Foreign shape (`insert.picture`).
//!
//! **Pictures...** shows the chooser (the desktop app's native dialog on a thread of its own, given with [`DiacraftApp::with_picture_picker`]; this crate depends on no dialog library; in the browser the page's host chooses the file, `file_host.rs`), and the file chosen goes to the engine's `insert.picture`, which reads it within the readers' budgets and refuses in words what is no picture, is too big or would make the
//! drawing hold more pictures than a file may. The new picture is the selection. A GIF and a TIFF are kept and drawn as a placeholder, and the engine's note says so in the status bar. The window
//! command is `window.insertPicture {path?}`: no `path` is the chooser, a `path` the file, with no dialog (a program's way).
//!
//! This file also holds [`InsertState`], the window's state for the Insert tab: the picture chooser, the dialogs of Link, Field and Symbol, and the address a click is to open.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use diacraft_engine::{EngineError, Result, guard};
use egui::Context;
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::file_open::FilePicker;

/// How often the window looks again at the chooser's thread if the thread has not woken it.
const POLL: Duration = Duration::from_millis(250);

/// What the window keeps for the Insert tab.
#[derive(Default)]
pub struct InsertState {
    /// The chooser of the picture file: the desktop app's native dialog (`with_picture_picker`).
    pub picker: Option<FilePicker>,
    /// The chooser on its thread, until it answers.
    pub pick: Option<Receiver<Option<PathBuf>>>,
    /// A command asked for the chooser: the next frame shows it.
    pub want_dialog: bool,
    /// The chooser is for the picture of this position of the org chart (`org_ui.rs`), and not for a picture of its own on the page.
    pub org_target: Option<u32>,
    /// The Hyperlinks dialog while it is open (`insert_links.rs`).
    pub links: Option<crate::insert_links::LinksDialog>,
    /// The Field dialog while it is open (`insert_field.rs`).
    pub field: Option<crate::insert_field::FieldDialog>,
    /// The Symbol dialog while it is open (`insert_symbol.rs`).
    pub symbol: Option<crate::insert_symbol::SymbolDialog>,
    /// A web or mail address a click on a link asked to open: the next frame hands it to the system (`egui`'s `OpenUrl`), which the window has a context for and a command has not.
    pub open_url: Option<String>,
    /// The last address handed to the system, for `ui.inspect` and the tests.
    pub last_url: Option<String>,
    /// What the engine said of the picture just inserted (a GIF or a TIFF is drawn as a placeholder): put in the status bar once the command that inserted it has ended, which would take a
    /// notice made inside it away.
    pub note: Option<String>,
}

impl DiacraftApp {
    /// Gives the window a native chooser for picture files. Without one, Pictures stays quiet.
    pub fn with_picture_picker(mut self, picker: FilePicker) -> DiacraftApp {
        self.ui.insert.picker = Some(picker);
        self
    }

    /// Whether there is a chooser for a picture file and none is on the screen now: whether Pictures can be used.
    pub fn can_pick_picture(&self) -> bool {
        (self.ui.insert.picker.is_some() || self.has_file_host()) && !self.picture_dialog_open()
    }

    /// Whether the chooser of a picture file is asked for or on the screen: a command has asked for it and the next frame has not started it yet, or its thread has not answered yet.
    /// **A caller that waits for a chosen file waits on this.**
    pub fn picture_dialog_open(&self) -> bool {
        self.ui.insert.want_dialog || self.ui.insert.pick.is_some()
    }

    /// Shows the chooser on a thread of its own.
    fn pick_picture(&mut self, ctx: &Context) {
        // With a host (the browser) the page chooses the file and puts it in the engine's memory (`file_host.rs`).
        if self.has_file_host() {
            if self.can_pick_picture() {
                self.ui.insert.pick = self.host_choose(ctx, crate::file_host::Want::Picture);
            }
            return;
        }
        let Some(picker) = self.ui.insert.picker.clone() else {
            self.set_notice("Inserting a picture needs a file dialog, which this build does not have.");
            return;
        };
        if !self.can_pick_picture() {
            return;
        }
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-picture-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker()).ok().flatten();
            // A chooser whose window was closed has nobody to answer: the send fails and nothing is lost.
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.ui.insert.pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// Starts the chooser a command asked for, takes its answer when it has come, and inserts the file. Runs before every frame; and hands a link's address to the system.
    pub(crate) fn advance_insert(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.ui.insert.want_dialog) {
            self.pick_picture(ctx);
        }
        if let Some(url) = self.ui.insert.open_url.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
            self.ui.insert.last_url = Some(url);
        }
        let Some(rx) = &self.ui.insert.pick else { return };
        match rx.try_recv() {
            Ok(chosen) => {
                self.ui.insert.pick = None;
                let target = self.ui.insert.org_target.take();
                if let Some(path) = chosen {
                    let done = match target {
                        Some(id) => self.org_picture_file(id, &path),
                        None => self.insert_picture_file(&path),
                    };
                    match done {
                        Ok(_) => self.say_picture_note(),
                        Err(e) => self.set_notice(&e.to_string()),
                    }
                    // The picture is in the drawing now, or was refused: what the host kept of the file is not needed.
                    self.release_file(&path);
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(POLL),
            Err(TryRecvError::Disconnected) => self.ui.insert.pick = None,
        }
    }

    /// `insert.picture` of the file at `path` on the page on view, and the new picture is the selection. → the engine's answer; a refusal is the engine's, in words that hold nothing of the file.
    /// A format that is kept and drawn as a placeholder says so in the status bar.
    pub fn insert_picture_file(&mut self, path: &Path) -> Result<Value> {
        const ID: &str = "window.insertPicture";
        let text = path.to_str().ok_or_else(|| EngineError::BadParams { cmd: ID.to_owned(), msg: "the name of the file is not text".to_owned() })?;
        // A text box that is open ends first: what was typed is kept.
        self.commit_pending_text();
        let mut params = json!({ "path": text });
        if let (Some(page), Some(map)) = (self.current_page(), params.as_object_mut()) {
            map.insert("page".to_owned(), json!(page));
        }
        let answer = self.execute("insert.picture", &params)?;
        if let Some(id) = answer.get("id").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
            self.select(&[id]);
        }
        self.ui.insert.note = answer.get("note").and_then(Value::as_str).map(capitalised);
        Ok(answer)
    }

    /// Puts what the engine said of the picture just inserted in the status bar, once.
    pub(crate) fn say_picture_note(&mut self) {
        if let Some(note) = self.ui.insert.note.take() {
            self.set_notice(&note);
        }
    }

    /// The `window.insertPicture` command: the chooser (shown by the next frame) without a `path`, the file at `path` with one.
    pub(crate) fn command_insert_picture(&mut self, path: Option<&str>) -> Result<Value> {
        self.command_insert_picture_for(None, path)
    }

    /// [`DiacraftApp::command_insert_picture`] for the picture of a position of the org chart (`target`), or for a picture of its own on the page (`None`).
    pub(crate) fn command_insert_picture_for(&mut self, target: Option<u32>, path: Option<&str>) -> Result<Value> {
        if let Some(path) = path {
            return match target {
                Some(id) => self.org_picture_file(id, Path::new(path)),
                None => self.insert_picture_file(Path::new(path)),
            };
        }
        if !self.can_pick_picture() {
            let why = if self.ui.insert.picker.is_none() && !self.has_file_host() {
                "there is no file dialog in this build: give a `path`"
            } else {
                "a picture file is being chosen"
            };
            return Err(EngineError::Disabled { cmd: "window.insertPicture".to_owned(), reason: why.to_owned() });
        }
        self.ui.insert.want_dialog = true;
        self.ui.insert.org_target = target;
        Ok(json!({"dialog": true}))
    }
}

/// `words` with its first letter a capital, for a notice that is a sentence.
fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str() + ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_becomes_a_sentence() {
        assert_eq!(capitalised("a GIF is kept"), "A GIF is kept.");
        assert_eq!(capitalised(""), "");
    }
}
