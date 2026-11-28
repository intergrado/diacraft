//! The questions the window asks before it does something that cannot be taken back, and what they have in common.
//!
//! * **Unsaved changes** (`save.rs`): New, Open and the window's close would throw them away.
//! * **A file that is there** ([`Replacing`]): the Save dialog or the Export dialog gave a name with no file type, the window added one
//!   (`.vsdx`, `.png`), and the file with that name is already in the folder. The dialog asked about the name it was given and could not know
//!   (M2 of the M3 review).
//! * **A file changed on the disk** ([`SaveAgain`]): the engine's `file.save` ended with `EngineError::ChangedOnDisk`, because something else
//!   wrote the drawing's file since the drawing was opened or last saved (M3 of the M3 review). The person overwrites it (the command is run
//!   again with `force`), saves the drawing as another file, or cancels.
//! * **A macro project the type chosen cannot hold** ([`SaveWithout`]): the engine's `file.save_as` ended with `EngineError::MacrosNotAllowed`, because
//!   the drawing holds one and the new type (`.vsdx`, `.vssx` or `.vstx`) has none. The card is "Leave the macros out?"
//!   with **Save without macros**, which runs the same `file.save_as` again with `drop_macros`; **Save as .vsdm** (`.vssm` for a stencil, `.vstm` for a template; the
//!   macro-enabled type of the one chosen, [`crate::save_kinds::macro_twin`]), which runs `file.save_as` onto that name with no option, so the project is kept, after asking about a file
//!   that is there ([`Replacing::Macros`]); and Cancel, which leaves the drawing and its file as they were and
//!   **ends what waited on the save** (a New, an Open or the close: it does not happen, and the person asks for it again). Its words are the engine's
//!   refusal, which names both ways out. A program that asks the same save gets the error and no card. **A drawing replaced under a save question** (`file.new` or `file.open`, by a
//!   program: `forget_save_questions`) closes the question and lets go what waited on the save, as Cancel does. **No card is asked for objects**: no engine refuses a save for an embedded
//!   object (they ride in the file, and `drop_objects` is a program's option that leaves them out; the note says what rides, `save_note.rs`).
//!
//! Each is a card in the middle of the window over a shade (`Tokens::scrim`). **While a card is on the screen, or a file is being read,
//! nothing under it takes the keys or the pointer** (L6 and L7 of the M3 review): [`DiacraftApp::is_modal_open`] is what the shortcuts, the
//! drawing keys and the drawing area ask. The card is egui's modal, which puts a layer over the window that senses every press and drag.
//!
//! **While a file is being read the window takes no edits from the person** (L7; ours, no specification says: the other way, asking at the
//! end, needs the file held back while a question is answered). The progress card is a modal too, with its Cancel. A program on the control
//! channel is not held: what it changes while the file is read is replaced when the file arrives, as `file.open` replaces a document.

use std::path::{Path, PathBuf};

use diacraft_engine::file_access::ScopeHandle;
use egui::{Context, CornerRadius, Frame, Id, Margin, Rect, Stroke, vec2};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// A name the window gave a file type to that is a file already.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Replacing {
    /// Save As, with the full name (the type added).
    Save(String),
    /// Export, with the full name.
    Export(PathBuf),
    /// Print to a PDF (`print.rs`), with the full name.
    Print(PathBuf),
    /// Save As, with the macro-enabled name the person chose to keep the macro project under: the window changed the type, and the file with that name is there.
    Macros(String),
}

/// What to run again, with `force`, if the person says to overwrite the file that changed on the disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SaveAgain {
    /// `file.save`.
    InPlace,
    /// `file.save_as` onto the drawing's own file, with this name, and with `drop_macros` when the person has already said to leave the macros out.
    As { path: String, drop_macros: bool },
}

/// A `file.save_as` the engine refused because the drawing holds a macro project and the type chosen cannot hold one
/// (`EngineError::MacrosNotAllowed`): what to run again, with `drop_macros`, if the person says to leave the macros out, and the engine's words the
/// card shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveWithout {
    /// The name the drawing was to be saved as.
    pub(crate) path: String,
    /// Whether the person had said to overwrite the drawing's own file that changed on the disk: the save runs again with the same choice.
    pub(crate) force: bool,
    /// The engine's refusal, as a sentence.
    pub(crate) words: String,
    /// The name with the macro-enabled type of the one chosen, and that type's extension, for the way out that keeps the project; `None` when the type has no twin the engine writes.
    pub(crate) keep_as: Option<(String, &'static str)>,
}

/// A card: a heading, a few words under it and buttons in a row.
pub(crate) struct Card<'a> {
    /// The card's own id, and the prefix of what its buttons are called in `ui.elements`.
    pub id: &'static str,
    pub heading: &'a str,
    pub detail: &'a str,
    /// Each button: its label, its name in `ui.elements`, and whether it can be pressed.
    pub buttons: &'a [(&'a str, &'static str, bool)],
    /// The button Escape stands for.
    pub cancel: usize,
}

impl DiacraftApp {
    /// Whether a card that must be answered is on the screen, or a file is being read: the window takes no edits from the keys or the
    /// pointer meanwhile (L6 and L7 of the M3 review).
    pub fn is_modal_open(&self) -> bool {
        self.saving.asking.is_some()
            || self.saving.replacing.is_some()
            || self.saving.changed.is_some()
            || self.saving.macros.is_some()
            || self.name_card_open()
            || self.opening_now().is_some()
            || self.ui.page_setup.dialog.is_some()
            || self.ui.page_setup.reorder.is_some()
            || self.ui.text_dialog.is_some()
            || self.ui.snap_dialog.is_some()
            || self.ui.layers.properties.is_some()
            || self.ui.layers.assign.is_some()
            || self.ui.shape_data.define.is_some()
            || self.ui.shape_data.ask.is_some()
            || self.ui.zoom_dialog.is_some()
            || self.ui.offset_dialog.is_some()
            || self.ui.fit_dialog.is_some()
            || self.ui.colors_dialog.is_some()
            || self.ui.relayout.dialog.is_some()
            || self.ui.relayout.spacing.is_some()
            || self.ui.options_dialog.is_some()
            || self.ui.select_by_type.is_some()
            || self.ui.insert.links.is_some()
            || self.ui.insert.field.is_some()
            || self.ui.insert.symbol.is_some()
            || self.sheet_card_open()
            || self.ui.licenses.is_some()
            || self.ui.splash.is_some()
    }

    /// Whether the question about replacing a file that is there (a name the window gave a file type to) is on the screen.
    pub fn replace_prompt_open(&self) -> bool {
        self.saving.replacing.is_some()
    }

    /// Whether the question about a file that changed on the disk is on the screen.
    pub fn changed_prompt_open(&self) -> bool {
        self.saving.changed.is_some()
    }

    /// Whether the question about a macro project the type chosen cannot hold ("Leave the macros out?") is on the screen.
    pub fn macros_prompt_open(&self) -> bool {
        self.saving.macros.is_some()
    }

    /// The frame of a card.
    pub(crate) fn card_frame(t: &Tokens) -> Frame {
        Frame::new()
            .fill(t.ribbon_fill)
            .stroke(Stroke::new(1.0, t.ribbon_border))
            .corner_radius(CornerRadius::same(6))
            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.page_shadow })
            .inner_margin(Margin::same(16))
    }

    /// Shows `card` over the window and returns the button pressed, or `card.cancel` when Escape is. The shade and the card are one modal:
    /// the window under them senses nothing.
    pub(crate) fn show_card(&mut self, ctx: &Context, t: Tokens, card: &Card<'_>) -> Option<usize> {
        let screen = ctx.content_rect();
        let mut answer = None;
        let mut drawn: Vec<(&'static str, Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique(card.id)).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_max_width((screen.width() - 80.0).clamp(220.0, 420.0));
            let label = ui.add(egui::Label::new(egui::RichText::new(card.heading).font(theme::font(size::TAB + 1.0)).color(t.text)).wrap());
            label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, card.heading));
            ui.add(egui::Label::new(egui::RichText::new(card.detail).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                for (n, (label, key, enabled)) in card.buttons.iter().enumerate() {
                    let button = ui.add_enabled(
                        *enabled,
                        egui::Button::new(egui::RichText::new(*label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)),
                    );
                    if button.clicked() {
                        answer = Some(n);
                    }
                    drawn.push((key, button.rect, *enabled));
                }
            });
        });
        keep_centred(ctx, modal.response.rect);
        for (key, rect, enabled) in drawn {
            self.note(key, rect, enabled);
        }
        // Escape is Cancel, as in a dialog.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            answer = Some(card.cancel);
        }
        answer
    }

    /// The cards over the drawing: at most one, the question that came last first.
    pub(crate) fn questions(&mut self, ctx: &Context, t: Tokens) {
        // The card is answered under the roots it was raised under, whoever answers it, and never under fewer than the frame that answers it is under. Each card has its own
        // (the one shown is the one that came last, in the order below).
        let scope = if self.name_card_open() {
            ScopeHandle::default()
        } else if self.saving.macros.is_some() {
            self.saving.macros_scope.clone()
        } else if self.saving.changed.is_some() {
            self.saving.changed_scope.clone()
        } else if self.saving.replacing.is_some() {
            self.saving.replacing_scope.clone()
        } else {
            self.saving.asking_scope.clone()
        };
        scope.run_beneath(|| self.questions_in_scope(ctx, t));
    }

    fn questions_in_scope(&mut self, ctx: &Context, t: Tokens) {
        if self.name_card_open() {
            self.name_card(ctx, t);
        } else if self.saving.macros.is_some() {
            self.macros_prompt(ctx, t);
        } else if self.saving.changed.is_some() {
            self.changed_prompt(ctx, t);
        } else if self.saving.replacing.is_some() {
            self.replace_prompt(ctx, t);
        } else {
            self.save_prompt(ctx, t);
        }
    }

    /// "x.vsdx already exists. Do you want to replace it?"
    fn replace_prompt(&mut self, ctx: &Context, t: Tokens) {
        let Some(replacing) = self.saving.replacing.clone() else { return };
        let (path, kind) = match &replacing {
            Replacing::Save(text) | Replacing::Macros(text) => {
                let path = Path::new(text);
                (path, path.extension().and_then(|e| e.to_str()).unwrap_or(crate::save::DEFAULT_EXTENSION))
            }
            Replacing::Export(path) => (path.as_path(), crate::export::DEFAULT_EXTENSION),
            Replacing::Print(path) => (path.as_path(), "pdf"),
        };
        let name = path.file_name().map_or_else(|| "That file".to_owned(), |n| diacraft_engine::shown(&n.to_string_lossy(), 64));
        let heading = format!("{name} already exists. Do you want to replace it?");
        let detail = if matches!(replacing, Replacing::Macros(_)) {
            format!(
                "You chose to keep the macros, so the drawing is saved as .{kind}, a type that holds them, and a file with that name is already in the folder. \
                 Replacing it cannot be undone."
            )
        } else {
            format!(
                "The name you chose had no file type, so DiaCraft added .{kind}, and a file with that name is already in the folder. \
                 Replacing it cannot be undone."
            )
        };
        let card = Card {
            id: "card_replace",
            heading: &heading,
            detail: &detail,
            buttons: &[("Replace", "replace.replace", true), ("Cancel", "replace.cancel", true)],
            cancel: 1,
        };
        match self.show_card(ctx, t, &card) {
            Some(0) => {
                self.saving.replacing = None;
                match replacing {
                    Replacing::Save(text) | Replacing::Macros(text) => self.save_text_to(ctx, &text, false),
                    Replacing::Export(path) => self.export_text_to(ctx, &path.to_string_lossy()),
                    Replacing::Print(path) => self.print_pdf_text(ctx, &path.to_string_lossy()),
                }
            }
            Some(_) => {
                self.saving.replacing = None;
                self.saving.after = None;
            }
            None => {}
        }
    }

    /// "x.vsdx has changed on the disk." Overwrite, Save As, or Cancel.
    fn changed_prompt(&mut self, ctx: &Context, t: Tokens) {
        let Some(again) = self.saving.changed.clone() else { return };
        let name = match &again {
            SaveAgain::InPlace => self.display_name(),
            SaveAgain::As { path, .. } => {
                Path::new(path).file_name().map_or_else(|| self.display_name(), |n| diacraft_engine::shown(&n.to_string_lossy(), 64))
            }
        };
        let heading = format!("{name} has changed on the disk.");
        let detail = "Something else changed this file after DiaCraft opened or last saved it. Saving now would replace those changes with this \
                      drawing. You can overwrite the file, save this drawing as another file, or cancel and keep working.";
        let can_save_as = self.can_save_as_now();
        let card = Card {
            id: "card_changed",
            heading: &heading,
            detail,
            buttons: &[("Overwrite", "changed.overwrite", true), ("Save As...", "changed.save_as", can_save_as), ("Cancel", "changed.cancel", true)],
            cancel: 2,
        };
        match self.show_card(ctx, t, &card) {
            Some(0) => {
                self.saving.changed = None;
                match again {
                    SaveAgain::InPlace => self.save_in_place(ctx, true),
                    SaveAgain::As { path, drop_macros } => self.save_text_with(ctx, &path, true, drop_macros),
                }
            }
            Some(1) => {
                self.saving.changed = None;
                self.save_as(ctx);
                // No dialog could be opened: nothing will come back, and nothing is waiting.
                if !self.save_dialog_open() {
                    self.saving.after = None;
                }
            }
            Some(_) => {
                self.saving.changed = None;
                self.saving.after = None;
            }
            None => {}
        }
    }

    /// "Leave the macros out?" The engine refused a Save As to a type that cannot hold the drawing's macro project, and said so in words, which are the
    /// card's, and which name both ways out. **Save without macros** runs the same `file.save_as` again with `drop_macros` (and `force`, if the person had said to overwrite a changed
    /// file); **Save as .vsdm** (when the type has a macro-enabled twin the engine writes) runs it onto the twin's name and keeps the project; Cancel and Escape leave the drawing, its file
    /// and its path as they were, and end what was waiting on the save (a New, an Open or a close).
    fn macros_prompt(&mut self, ctx: &Context, t: Tokens) {
        let Some(ask) = self.saving.macros.clone() else { return };
        let keep_label = ask.keep_as.as_ref().map(|(_, extension)| format!("Save as .{extension}"));
        let mut buttons: Vec<(&str, &'static str, bool)> = vec![("Save without macros", "macros.save_without", true)];
        if let Some(label) = &keep_label {
            buttons.push((label.as_str(), "macros.save_as_enabled", true));
        }
        buttons.push(("Cancel", "macros.cancel", true));
        let cancel = buttons.len() - 1;
        let card = Card { id: "card_macros", heading: "Leave the macros out?", detail: &ask.words, buttons: &buttons, cancel };
        match self.show_card(ctx, t, &card) {
            Some(0) => {
                self.saving.macros = None;
                self.save_text_with(ctx, &ask.path, ask.force, true);
            }
            Some(1) if cancel == 2 => {
                self.saving.macros = None;
                if let Some((path, _)) = &ask.keep_as {
                    self.save_macro_enabled(ctx, path, ask.force);
                }
            }
            Some(_) => {
                self.saving.macros = None;
                self.saving.after = None;
            }
            None => {}
        }
    }
}

/// Asks for another frame while a card is not in the middle of the window. An area is placed by the size it had in the last frame, so after the
/// window is resized in one step the card is a frame behind, off the middle or off the screen, and no frame would follow to put it right.
pub(crate) fn keep_centred(ctx: &Context, card: Rect) {
    if (card.center() - ctx.content_rect().center()).length() > 1.0 {
        ctx.request_repaint();
    }
}

/// Whether something is at `path` (a file, a folder, or a link, even one that leads nowhere): a save or an export there would replace it.
pub(crate) fn is_there(path: &str) -> bool {
    // A file in the engine's memory (the browser's) is there when the store has it.
    if diacraft_engine::memfs::active() {
        return diacraft_engine::memfs::contains(Path::new(path));
    }
    std::fs::symlink_metadata(path).is_ok()
}
