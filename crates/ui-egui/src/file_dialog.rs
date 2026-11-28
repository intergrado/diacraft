//! One file dialog at a time.
//!
//! The window has six ways to ask the person for a file or a name: Open, Save As, Export, a picture to insert, a stencil to open and a PDF to print to. The desktop shows a
//! system dialog on a thread of its own for each, and the browser a chooser the page answers later or the window's own card that names a file. Each had a guard against its own
//! second start and none against the others, so File, Open, then Save As, then Export started three dialogs at once (the desktop's dialog service held six requests).
//! **While one of them is on the screen, the others are refused**: their buttons are greyed, their keys do nothing but say why, and a program that asks for one through a command is
//! told which is open. `ui.inspect` names it (`fileDialog` in the window state).

use crate::app::DiacraftApp;
use crate::file_host::Purpose;

/// A file dialog of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialog {
    /// File, Open: the file to open.
    Open,
    /// Save As, and Save of a drawing with no file: the name to save under.
    Save,
    /// Export: the name of the picture.
    Export,
    /// Insert, Pictures: the picture file.
    Picture,
    /// Open Stencil: the stencil file.
    Stencil,
    /// Print to a PDF: the name of the PDF.
    Print,
}

impl FileDialog {
    /// The word `ui.inspect` gives for it.
    pub fn word(self) -> &'static str {
        match self {
            FileDialog::Open => "open",
            FileDialog::Save => "save",
            FileDialog::Export => "export",
            FileDialog::Picture => "picture",
            FileDialog::Stencil => "stencil",
            FileDialog::Print => "print",
        }
    }

    /// The dialog as a person names it, for a sentence.
    pub fn name(self) -> &'static str {
        match self {
            FileDialog::Open => "The Open dialog",
            FileDialog::Save => "The Save As dialog",
            FileDialog::Export => "The Export dialog",
            FileDialog::Picture => "The dialog that chooses a picture",
            FileDialog::Stencil => "The dialog that chooses a stencil",
            FileDialog::Print => "The dialog that names the PDF",
        }
    }
}

impl DiacraftApp {
    /// The file dialog that is on the screen now, or asked for and about to be shown: `None` when there is none. A dialog counts from the moment a command asks for it (the next frame
    /// shows it) until its answer has been taken, and a card of the window that names a file is a dialog as a system dialog is.
    pub fn file_dialog(&self) -> Option<FileDialog> {
        if self.open_dialog_open() {
            Some(FileDialog::Open)
        } else if self.save_dialog_open() || self.name_card_for(Purpose::Save) {
            Some(FileDialog::Save)
        } else if self.export_dialog_open() || self.name_card_for(Purpose::Export) {
            Some(FileDialog::Export)
        } else if self.picture_dialog_open() {
            Some(FileDialog::Picture)
        } else if self.stencil_dialog_open() {
            Some(FileDialog::Stencil)
        } else if self.print_dialog_open() || self.name_card_for(Purpose::PrintPdf) {
            Some(FileDialog::Print)
        } else {
            None
        }
    }

    /// The sentence that says why another file dialog cannot be shown now, if one is open: what a refusal tells the person (a notice) or a program (the error of its command).
    pub(crate) fn file_dialog_busy(&self) -> Option<String> {
        self.file_dialog().map(|open| format!("{} is open: finish it or cancel it first.", open.name()))
    }

    /// Tells the person, in a notice, that another file dialog is open, when one is. → whether one is.
    pub(crate) fn refuse_while_a_file_dialog_is_open(&mut self) -> bool {
        match self.file_dialog_busy() {
            Some(words) => {
                self.set_notice(&words);
                true
            }
            None => false,
        }
    }
}
