//! Exporting a page as a picture from the window: Export in the File view and in the View tab's Export group.
//!
//! The person chooses a file with the app's native Save dialog (`ExportPicker`, filtered to the three kinds the engine writes: PNG, SVG and
//! PDF), and the window runs the engine's `file.export {path, page}` for the page on view. **The export is made off the UI thread**: it
//! scans the machine's fonts and draws the whole page, which can take seconds on a crowded one. The thread works in a session of its own
//! that holds a copy of the document (cheap: pages are shared), so the drawing in the window is never touched, locked or waited for; the
//! command changes nothing in a drawing in any case ("an export is not an undo step, does not mark the drawing saved").
//!
//! Failures are told in words for a person, with the file's name and no command id, class or path ([`export_failure_text`]).
//!
//! As Open and Save do, the dialog runs on a thread of its own; the browser has none, so there the page's host (or the window's card) names the file, and the export is made on the page's thread
//! a frame after the work notice is drawn (`file_host.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use diacraft_engine::{EngineError, Session, guard, shown};
use egui::Context;
use serde_json::{Value, json};

use crate::app::DiacraftApp;

/// What chooses the file to export to: the desktop app's native Save dialog, with the three kinds as its filters. It is given the name to
/// suggest (the drawing's, with `.png`), blocks until the person chooses or cancels, and answers the path or `None`.
pub type ExportPicker = Arc<dyn Fn(String) -> Option<PathBuf> + Send + Sync>;

/// The kinds of picture the engine writes, by extension. The Save dialog's filters and the window's check of a chosen name are these.
pub const EXPORT_EXTENSIONS: [&str; 3] = ["png", "svg", "pdf"];

/// The extension a name chosen without one gets: a picture of the page the people can open anywhere.
pub const DEFAULT_EXTENSION: &str = "png";

/// An export under way: the file's name for the words that tell its end, the path it writes (a host hands that file over when it is done, `file_host.rs`), and the answer to come.
struct ExportJob {
    name: String,
    path: PathBuf,
    rx: Receiver<Result<Value, EngineError>>,
}

/// What the window keeps of exporting.
#[derive(Default)]
pub(crate) struct Exporting {
    picker: Option<ExportPicker>,
    /// The Save dialog, running on its thread.
    pick: Option<Receiver<Option<PathBuf>>>,
    /// The export, running on its thread: the file's name, and the answer to come.
    job: Option<ExportJob>,
}

impl DiacraftApp {
    /// Gives the window a native Save dialog for Export. Without one (and without a host for files, `file_host.rs`), Export stays disabled.
    pub fn with_export_picker(mut self, picker: ExportPicker) -> DiacraftApp {
        self.exporting.picker = Some(picker);
        self
    }

    /// Whether the window has a dialog to name the picture with now: the app gave one, and neither it nor an export is under way. (A program
    /// that runs `file.export` with a path needs none.)
    pub(crate) fn export_dialog_available(&self) -> bool {
        (self.exporting.picker.is_some() || self.has_file_host())
            && !self.export_dialog_open()
            && !self.name_card_for(crate::file_host::Purpose::Export)
            && !self.exporting_now()
    }

    /// Whether Export can be used now: a drawing is open (the engine's check, `command_enabled`) and the window has a dialog for it.
    pub fn can_export(&self) -> bool {
        self.command_enabled("file.export").is_ok()
    }

    /// Whether the Export dialog is on the screen (its thread has not answered yet).
    pub fn export_dialog_open(&self) -> bool {
        self.exporting.pick.is_some()
    }

    /// Whether a picture is being made and written.
    pub fn exporting_now(&self) -> bool {
        self.exporting.job.is_some()
    }

    /// Export: asks for a file with the dialog, then writes the page on view there.
    pub fn export_dialog(&mut self, ctx: &Context) {
        // The picture is of the drawing with the label being typed in it (M1).
        self.commit_pending_text();
        if !self.can_export() {
            return;
        }
        let suggestion = self.suggested_picture_name();
        // With a host (the browser) the page names the file, or the window's own card does (`file_host.rs`).
        if self.has_file_host() {
            if self.export_dialog_available() {
                let request = crate::save_kinds::SaveRequest { name: suggestion, kinds: DiacraftApp::export_kinds() };
                self.exporting.pick = self.ask_for_name(ctx, crate::file_host::Purpose::Export, request);
            }
            return;
        }
        let Some(picker) = self.exporting.picker.clone() else { return };
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-export-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker(suggestion)).ok().flatten();
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.exporting.pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// The name the dialog starts with: the drawing's name without its extension, and `.png`.
    fn suggested_picture_name(&self) -> String {
        let name = self.display_name();
        let stem = Path::new(&name).file_stem().map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned());
        format!("{stem}.{DEFAULT_EXTENSION}")
    }

    /// Takes the dialog's answer and the export's when they come. Runs before every frame.
    pub(crate) fn advance_export(&mut self, ctx: &Context) {
        if let Some(rx) = &self.exporting.pick {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.exporting.pick = None;
                    self.export_to(ctx, path);
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.exporting.pick = None,
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
            }
        }
        let Some(ExportJob { name, path, rx }) = &self.exporting.job else { return };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
                return;
            }
            Err(TryRecvError::Disconnected) => {
                Err(EngineError::Internal { cmd: "file.export".to_owned(), msg: "the export stopped without an answer".to_owned() })
            }
        };
        let (name, path) = (name.clone(), path.clone());
        self.exporting.job = None;
        match outcome {
            Ok(done) => {
                self.opening.banner = None;
                let said = exported_text(&name, &done);
                self.set_notice(&said);
                // In the browser the engine wrote the picture in memory: it is handed to the person now.
                let head = format!("Exported {name}.");
                let tail = said.strip_prefix(&head).unwrap_or_default().trim().to_owned();
                self.hand_over(ctx, &path, &head, &tail);
            }
            Err(e) => {
                self.opening.banner = Some(export_failure_text(&name, &e));
                self.release_file(&path);
            }
        }
    }

    /// Starts writing the page on view to `path`, on a thread. **A name with no file type gets `.png` here**, after the dialog's own check of the
    /// name it was given, so when that file is there the person is asked before it is replaced (M2 of the M3 review).
    pub(crate) fn export_to(&mut self, ctx: &Context, path: PathBuf) {
        let name_of = |path: &Path| path.file_name().map_or_else(|| "the picture".to_owned(), |n| shown(&n.to_string_lossy(), 64));
        let Some(text) = path.to_str() else {
            self.opening.banner = Some("That file name is not text DiaCraft can use, so nothing was exported.".to_owned());
            return;
        };
        // A name with no extension gets the default; one with an extension the engine does not write is told so, in words.
        let (text, typed) = match chosen_extension(&path) {
            Chosen::Kind => (text.to_owned(), false),
            Chosen::None => (format!("{text}.{DEFAULT_EXTENSION}"), true),
            Chosen::Other(extension) => {
                self.opening.banner = Some(other_extension_text(&name_of(&path), &extension));
                return;
            }
        };
        let name = name_of(Path::new(&text));
        // The export runs on a copy of the drawing in a session that has no file, so the engine cannot know which file is the drawing's own:
        // the window does, and never writes over it (the engine's rule: "an export never writes the drawing's own file").
        if is_own_file(Path::new(&text), self.session().path()) {
            self.opening.banner = Some(own_file_text(&name));
            return;
        }
        if typed && crate::ask::is_there(&text) {
            self.saving.replacing = Some(crate::ask::Replacing::Export(PathBuf::from(text)));
            ctx.request_repaint();
            return;
        }
        self.export_text_to(ctx, &text);
    }

    /// Writes the page on view to `text`, whose file type is settled and which is not the drawing's own file, on a thread.
    pub(crate) fn export_text_to(&mut self, ctx: &Context, text: &str) {
        let name = shown(&Path::new(text).file_name().map_or_else(|| "the picture".to_owned(), |n| n.to_string_lossy().into_owned()), 64);
        // The picture is of the drawing with the label being typed in it (M1).
        self.commit_pending_text();
        let Some(page) = self.current_page() else { return };
        let Some(document) = self.session().document().cloned() else { return };
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let params = json!({ "path": text, "page": page });
        let spawned = self.run_in_background("diacraft-export", move || {
            let outcome = guard::catch_panic(|| export_in_new_session(document, &params))
                .unwrap_or_else(|msg| Err(EngineError::Internal { cmd: "file.export".to_owned(), msg }));
            // The window may have been closed meanwhile; there is nobody to tell.
            let _ = tx.send(outcome);
            wake.request_repaint();
        });
        if spawned.is_err() {
            self.opening.banner = Some(format!("{name} was not exported: this build cannot work in the background."));
            return;
        }
        self.set_work_notice(&format!("Exporting {name}..."));
        self.exporting.job = Some(ExportJob { name, path: PathBuf::from(text), rx });
        ctx.request_repaint();
    }
}

/// Runs `file.export` with `params` on a copy of the drawing, in a session of its own: the engine's command, and nothing else, makes the
/// file. The session has no file of its own, so a link named like a picture that leads to the drawing's file is refused as any link is.
fn export_in_new_session(document: diacraft_engine::doc::Document, params: &Value) -> Result<Value, EngineError> {
    let mut session = Session::new();
    session.execute("file.new", &json!({}))?;
    session.edit("Copy of the drawing", |doc| {
        *doc = document;
        Ok(())
    })?;
    session.execute("file.export", params)
}

/// Whether writing to `target` would replace the drawing that is open: it is the file the drawing came from or was last saved to, by its path
/// or by where the paths lead (a link, a `..`, another spelling). The engine's own rule, which a session with no file cannot apply.
pub(crate) fn is_own_file(target: &Path, own: Option<&Path>) -> bool {
    let Some(own) = own else { return false };
    target == own || matches!((std::fs::canonicalize(target), std::fs::canonicalize(own)), (Ok(a), Ok(b)) if a == b)
}

/// What a person is told when the file they chose is the drawing's own.
pub(crate) fn own_file_text(name: &str) -> String {
    format!("{name} was not exported: that is the drawing's own file, which an export never writes. Choose another name.")
}

/// What the name the person chose ends in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Chosen {
    /// `.png`, `.svg` or `.pdf`, in any case.
    Kind,
    /// No extension: the window adds the default.
    None,
    /// Another extension, which the engine would refuse.
    Other(String),
}

pub(crate) fn chosen_extension(path: &Path) -> Chosen {
    let Some(extension) = path.extension() else { return Chosen::None };
    let extension = extension.to_string_lossy();
    if EXPORT_EXTENSIONS.iter().any(|k| k.eq_ignore_ascii_case(&extension)) { Chosen::Kind } else { Chosen::Other(shown(&extension, 16)) }
}

/// What a person is told when the name they chose ends in something a picture cannot be written as.
pub(crate) fn other_extension_text(name: &str, extension: &str) -> String {
    format!(
        "{name} was not exported: a picture of a page is saved as PNG, SVG or PDF, and \".{extension}\" is none of them. Name the file with .png, .svg or .pdf."
    )
}

/// "Exported plan.png." and, when the page has things DiaCraft left out or drew approximately, how many kinds.
pub(crate) fn exported_text(name: &str, done: &Value) -> String {
    let kinds = done
        .get("skipped")
        .and_then(Value::as_array)
        .map_or(0, |list| list.iter().filter(|s| s.get("kind").and_then(Value::as_str).is_some_and(|k| k != "Guide")).count());
    match kinds {
        0 => format!("Exported {name}."),
        1 => format!("Exported {name}. One kind of thing on the page was left out or drawn approximately."),
        n => format!("Exported {name}. {n} kinds of things on the page were left out or drawn approximately."),
    }
}

/// What a person is told when an export did not work: the engine's own words (they hold no path and no class), as a sentence.
pub fn export_failure_text(name: &str, error: &EngineError) -> String {
    let sentence = |msg: &str| {
        let mut chars = msg.trim().chars();
        let text = chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
        if text.ends_with('.') { text } else { format!("{text}.") }
    };
    match error {
        EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } => {
            format!("{name} was not exported. {} The drawing is as it was.", sentence(&shown(msg, 300)))
        }
        EngineError::Internal { .. } => {
            format!(
                "DiaCraft met an internal problem while exporting {name}, so it was not written. The drawing is as it was. Please report this bug."
            )
        }
        other => format!("{name} was not exported: {}.", shown(&other.to_string(), 300)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_name_is_judged_by_its_extension_in_any_case() {
        assert_eq!(chosen_extension(Path::new("/a/b.PNG")), Chosen::Kind);
        assert_eq!(chosen_extension(Path::new("plan.Svg")), Chosen::Kind);
        assert_eq!(chosen_extension(Path::new("plan.pdf")), Chosen::Kind);
        assert_eq!(chosen_extension(Path::new("plan")), Chosen::None);
        assert_eq!(chosen_extension(Path::new("plan.vsdx")), Chosen::Other("vsdx".to_owned()));
        assert_eq!(chosen_extension(Path::new("plan.jpg")), Chosen::Other("jpg".to_owned()));
    }

    #[test]
    fn the_drawings_own_file_is_known_by_its_path_and_by_where_a_link_leads() {
        let dir = std::env::temp_dir().join(format!("diacraft-export-own-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.join("plan.png");
        std::fs::write(&own, b"x").unwrap();
        assert!(is_own_file(&own, Some(&own)));
        assert!(is_own_file(&dir.join("..").join(dir.file_name().unwrap()).join("plan.png"), Some(&own)), "another spelling of the same file");
        assert!(!is_own_file(&dir.join("other.png"), Some(&own)));
        assert!(!is_own_file(&own, None), "a drawing with no file has none to protect");
        #[cfg(unix)]
        {
            let link = dir.join("link.png");
            std::os::unix::fs::symlink(&own, &link).unwrap();
            assert!(is_own_file(&link, Some(&own)), "a link to it is it");
        }
        std::fs::remove_dir_all(&dir).ok();
        assert!(own_file_text("plan.png").contains("never writes"));
    }

    #[test]
    fn the_words_of_a_refusal_name_the_file_and_no_command() {
        let link = EngineError::Failed {
            cmd: "file.export".into(),
            msg: "the picture could not be saved: that is the drawing's own file, which an export never writes".into(),
        };
        let text = export_failure_text("plan.png", &link);
        assert!(text.starts_with("plan.png was not exported. The picture could not be saved:"), "{text}");
        assert!(text.ends_with("The drawing is as it was."), "{text}");
        assert!(!text.contains("file.export") && !text.contains('`'), "no command id: {text}");
        let bug = export_failure_text("a.pdf", &EngineError::Internal { cmd: "file.export".into(), msg: "index out of bounds: secret".into() });
        assert!(bug.contains("internal problem") && !bug.contains("secret"), "{bug}");
        let other = other_extension_text("plan.jpg", "jpg");
        assert!(other.contains(".png, .svg or .pdf") && other.contains("plan.jpg"), "{other}");
    }

    #[test]
    fn what_was_left_out_is_counted_by_kind_and_a_guide_is_no_gap() {
        let done = json!({"skipped": [{"kind": "Guide", "count": 4, "first": ""}, {"kind": "MetafilePlaceholder", "count": 2, "first": "x"}]});
        assert_eq!(exported_text("a.png", &done), "Exported a.png. One kind of thing on the page was left out or drawn approximately.");
        assert_eq!(exported_text("a.png", &json!({"skipped": []})), "Exported a.png.");
        assert_eq!(exported_text("a.png", &json!({})), "Exported a.png.");
    }
}
