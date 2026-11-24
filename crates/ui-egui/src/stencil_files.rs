//! Stencil files from the window (over the `stencil.open`, `stencil.close` and `shape.drop {stencil}`).
//!
//! **More Shapes, Open Stencil** shows the desktop app's native chooser on a thread of its own (the app gives it with [`DiacraftApp::with_stencil_picker`]; the
//! crate depends on no dialog library; in the browser the page's host chooses the file and the engine's memory keeps it while the stencil is open, `file_host.rs`), and the file chosen goes to `stencil.open`. The stencil then has a row in the Shapes
//! window and is the one shown, with a tile for each master that is not hidden; **a close control on the row** sends `stencil.close`. A tile is dragged onto the page
//! or clicked as the built-in ones are, and sends `shape.drop` with the stencil's id; a master the engine refuses (a picture or another part of the file it
//! cannot carry across, gap F7) is told in the status bar in the engine's words, and the drawing is as it was.
//!
//! **The file is read on the window's thread** (the engine's `stencil.open` reads it there: the session is the window's, and the engine has no way to hand a
//! stencil read elsewhere to it), so a very large stencil holds the window while it is read, up to what a drawing file may be (gap U22). The
//! window commands are `shapes.openStencil {path?}` (no `path`: the chooser) and `shapes.closeStencil {id}`.
//!
//! **A drawing read from a file** is a new session, so the stencils opened are opened again in it by the thread that reads the file (`file_open.rs`), by the
//! paths the window opened them from, and a stencil whose file is gone, or does not fit the memory a read may use beside the drawing, is not in the list after and **is said**
//! (the window's message and `stencil_notes` in the answer of the open). A stencil opened by a program through the engine's own command
//! has no path the window knows: it is listed, and used, but not opened again after a drawing is read.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{TryRecvError, channel};
use std::time::Duration;

use diacraft_engine::{EngineError, Result, Session, guard};
use egui::Context;
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::file_open::FilePicker;

/// How often the window looks again at the dialog's thread if the thread has not woken it.
const POLL: Duration = Duration::from_millis(250);

/// Opens, in a session that was just made, the stencils the window had open (by the paths it opened them from). A file that cannot be opened now (it is
/// gone, it changed into something that is not a stencil, or it does not fit the memory a read may use beside the drawing just read) is left out, and the list shows what is
/// open. **What is left out is said** (a stencil near the budget dropped out unseen): one note for each, in words and by the file's name, for the window's
/// message and the answer of the open.
pub(crate) fn reopen(session: &mut Session, files: &[PathBuf]) -> Vec<String> {
    let mut notes = Vec::new();
    for path in files {
        let name = crate::file_open::name_of(path);
        let Some(text) = path.to_str() else {
            notes.push(format!("{name} was not opened again in the new drawing: its name is not text DiaCraft can use."));
            continue;
        };
        if let Err(e) = session.execute("stencil.open", &json!({ "path": text })) {
            notes.push(not_opened_again(&name, &e));
        }
    }
    notes
}

/// The words for a stencil that did not open again: why, by the kind of trouble, and that it is not in the new drawing's stencils.
fn not_opened_again(name: &str, error: &EngineError) -> String {
    match error {
        EngineError::CannotOpen { problem, msg, .. } => {
            format!("{} It was not opened again in the new drawing.", crate::file_open::refusal_words(name, *problem, msg))
        }
        other => format!("The stencil {name} was not opened again in the new drawing: {other}"),
    }
}

/// The most stencils a message names that were not opened again: the rest are counted.
const MOST_NAMED: usize = 3;

/// One message for the notes of [`reopen`]: the first few, and how many more.
pub(crate) fn notes_message(notes: &[String]) -> String {
    let mut text = notes.iter().take(MOST_NAMED).cloned().collect::<Vec<_>>().join(" ");
    if let Some(more) = notes.len().checked_sub(MOST_NAMED).filter(|more| *more > 0) {
        text.push_str(&format!(
            " {more} more stencil{} {} not opened again.",
            if more == 1 { "" } else { "s" },
            if more == 1 { "was" } else { "were" }
        ));
    }
    text
}

impl DiacraftApp {
    /// Gives the window a native chooser for stencil files. Without one, Open Stencil stays quiet.
    pub fn with_stencil_picker(mut self, picker: FilePicker) -> DiacraftApp {
        self.ui.stencils.picker = Some(picker);
        self
    }

    /// Whether there is a chooser for a stencil file and none is on the screen now: whether Open Stencil can be used.
    pub fn can_pick_stencil(&self) -> bool {
        (self.ui.stencils.picker.is_some() || self.has_file_host()) && !self.stencil_dialog_open()
    }

    /// Whether the chooser of a stencil file is asked for or on the screen: a command has asked for it and the next frame has not started it yet, or its
    /// thread has not answered yet (the answer is taken, and the file opened, in a frame after that). **A caller that waits for a chosen file waits on this**:
    /// until it is false the stencil is not open yet, and no frame is owed that would show it.
    pub fn stencil_dialog_open(&self) -> bool {
        self.ui.stencils.want_dialog || self.ui.stencils.pick.is_some()
    }

    /// Shows the chooser on a thread of its own, and opens what it chooses.
    pub fn pick_stencil(&mut self, ctx: &Context) {
        // With a host (the browser) the page chooses the file and keeps it in the engine's memory while the stencil is open (`file_host.rs`).
        if self.has_file_host() {
            if self.can_pick_stencil() {
                self.ui.stencils.pick = self.host_choose(ctx, crate::file_host::Want::Stencil);
            }
            return;
        }
        let Some(picker) = self.ui.stencils.picker.clone() else {
            self.set_notice("Opening a stencil needs a file dialog, which this build does not have.");
            return;
        };
        if !self.can_pick_stencil() {
            return;
        }
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-stencil-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker()).ok().flatten();
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.ui.stencils.pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// Starts the chooser a command asked for, takes its answer when it has come, and opens the file. Runs before every frame.
    pub(crate) fn advance_stencil_dialog(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.ui.stencils.want_dialog) {
            self.pick_stencil(ctx);
        }
        let Some(rx) = &self.ui.stencils.pick else { return };
        match rx.try_recv() {
            Ok(chosen) => {
                self.ui.stencils.pick = None;
                if let Some(path) = chosen
                    && let Err(e) = self.open_stencil_file(&path)
                {
                    self.set_notice(&format!("The stencil could not be opened: {e}"));
                    // A stencil that did not open is not kept.
                    self.release_file(&path);
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(POLL),
            Err(TryRecvError::Disconnected) => self.ui.stencils.pick = None,
        }
    }

    /// `stencil.open` of the file at `path`, and the stencil it makes is the one the Shapes window shows. → the engine's answer (`id`, `name`, `masters`, ...);
    /// a refusal is the engine's, in words that hold nothing of the file.
    pub fn open_stencil_file(&mut self, path: &Path) -> Result<Value> {
        let text = path
            .to_str()
            .ok_or_else(|| EngineError::BadParams { cmd: "shapes.openStencil".to_owned(), msg: "the name of the file is not text".to_owned() })?;
        // `execute` notes the file (`noted_stencil_open`), whoever sent the command.
        self.execute("stencil.open", &json!({ "path": text }))
    }

    /// `stencil.open` succeeded (from the window or from a program): the file it was opened from is kept, and the stencil is the one the Shapes window shows.
    pub(crate) fn noted_stencil_open(&mut self, params: &Value, answer: &Value) {
        let Some(id) = answer.get("id").and_then(Value::as_str) else { return };
        if let Some(path) = params.get("path").and_then(Value::as_str)
            && !self.ui.stencils.files.iter().any(|(known, _)| known == id)
        {
            self.ui.stencils.files.push((id.to_owned(), PathBuf::from(path)));
        }
        self.ui.stencils.current = Some(id.to_owned());
        self.ui.stencils.forget_opened();
        // Read the list now, so that what answers the next question about the window has the stencil in it.
        let _ = self.stencil_list();
    }

    /// `stencil.close` succeeded: what was kept of the stencil goes, and the Shapes window shows the first stencil if it was showing this one.
    pub(crate) fn noted_stencil_close(&mut self, params: &Value) {
        let Some(id) = params.get("id").and_then(Value::as_str) else { return };
        // A stencil that is closed no longer needs what the host kept of its file.
        if let Some((_, path)) = self.ui.stencils.files.iter().find(|(known, _)| known == id) {
            self.release_file(path);
        }
        self.ui.stencils.files.retain(|(known, _)| known != id);
        if self.ui.stencils.current.as_deref() == Some(id) {
            self.ui.stencils.current = None;
        }
        self.ui.stencils.forget_opened();
        let _ = self.stencil_list();
    }

    /// `stencil.close` of the stencil opened with this id; the Shapes window shows the first stencil if it was showing this one.
    pub fn close_stencil(&mut self, id: &str) -> Result<()> {
        self.execute("stencil.close", &json!({ "id": id })).map(|_| ())
    }

    /// The paths the window opened its stencils from, in the order opened: what a thread that reads a drawing opens again in the session it makes.
    pub(crate) fn stencil_paths(&self) -> Vec<PathBuf> {
        self.ui.stencils.files.iter().map(|(_, path)| path.clone()).collect()
    }

    /// A session read from a file replaced the old one, and opened the stencils again: the ids they have now are found by their files, and the stencil that was
    /// shown is shown again.
    pub(crate) fn stencils_after_open(&mut self) {
        let before = std::mem::take(&mut self.ui.stencils.files);
        let shown_before = self.ui.stencils.current.clone();
        for (old, path) in before {
            // The engine knows a stencil by the file its name leads to, links followed, and the window keeps the name it was given: the two differ through a
            // link, and on macOS every temporary folder is under one (`/var` is `/private/var`), where the stencil was lost here. A file that is gone
            // leads nowhere, and its stencil did not open again.
            // (A file in the engine's memory, the browser's, has one name and is no link: it is its own file, `memfs`.)
            let file = if diacraft_engine::memfs::active() { Some(path.clone()) } else { std::fs::canonicalize(&path).ok() };
            let found = file.and_then(|file| self.session.stencils().of_file(&file).map(|s| s.id().to_owned()));
            let Some(id) = found else { continue };
            if shown_before.as_deref() == Some(old.as_str()) {
                self.ui.stencils.current = Some(id.clone());
            }
            self.ui.stencils.files.push((id, path));
        }
        // A stencil whose file did not open again is not shown.
        let listed: Vec<String> = self.session.stencils().iter().map(|s| s.id().to_owned()).collect();
        if self.ui.stencils.current.as_ref().is_some_and(|id| id.starts_with("file-") && !listed.contains(id)) {
            self.ui.stencils.current = None;
        }
        self.ui.stencils.forget_opened();
        let _ = self.stencil_list();
    }

    /// The `shapes.openStencil` command: the chooser (shown by the next frame) without a `path`, the file at `path` with one.
    pub(crate) fn command_open_stencil(&mut self, path: Option<&str>) -> Result<Value> {
        if let Some(path) = path {
            return self.open_stencil_file(Path::new(path));
        }
        if !self.can_pick_stencil() {
            let why = if self.ui.stencils.picker.is_none() && !self.has_file_host() {
                "there is no file dialog in this build: give a `path`"
            } else {
                "a stencil file is being chosen"
            };
            return Err(EngineError::Disabled { cmd: "shapes.openStencil".to_owned(), reason: why.to_owned() });
        }
        self.ui.stencils.want_dialog = true;
        Ok(json!({"dialog": true}))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn notes(n: usize) -> Vec<String> {
        (1..=n).map(|k| format!("Note {k}.")).collect()
    }

    #[test]
    fn the_message_names_the_first_three_stencils_that_were_not_opened_again_and_counts_the_rest() {
        assert_eq!(notes_message(&[]), "");
        assert_eq!(notes_message(&notes(1)), "Note 1.");
        assert_eq!(notes_message(&notes(3)), "Note 1. Note 2. Note 3.", "three are named, and none is counted");
        assert_eq!(notes_message(&notes(4)), "Note 1. Note 2. Note 3. 1 more stencil was not opened again.");
        assert_eq!(notes_message(&notes(6)), "Note 1. Note 2. Note 3. 3 more stencils were not opened again.");
    }

    #[test]
    fn a_stencil_that_did_not_open_again_is_told_by_the_kind_of_trouble_and_by_name() {
        let refused =
            EngineError::CannotOpen { cmd: "stencil.open".to_owned(), problem: diacraft_engine::OpenProblem::OutOfMemory, msg: "words".to_owned() };
        assert_eq!(
            not_opened_again("Big.vssx", &refused),
            "There is not enough memory to open Big.vssx. It was not opened again in the new drawing."
        );
        let other = EngineError::Failed { cmd: "stencil.open".to_owned(), msg: "this is a drawing, not a stencil".to_owned() };
        let words = not_opened_again("Plan.vsdx", &other);
        assert!(words.starts_with("The stencil Plan.vsdx was not opened again in the new drawing: ") && words.contains("not a stencil"), "{words}");
    }
}
