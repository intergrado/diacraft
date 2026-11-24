//! The native Open dialog: the desktop's own file chooser, through `rfd` (the portal on Linux, the system's dialog elsewhere).
//!
//! The window runs the chooser on a thread of its own (`FilePicker` in the window crate), because it blocks until the person
//! chooses or cancels. This file is the app's alone: the window crate depends on no dialog library, so it builds for the web, where
//! the Open controls stay disabled.

use std::sync::Arc;

use diacraft_ui_egui::ExportPicker;
use diacraft_ui_egui::FilePicker;
use diacraft_ui_egui::{SavePicker, SaveRequest};

/// The extensions of DiaCraft's own files: a drawing, a template and a stencil. The Open dialog lists them first, as "DiaCraft drawings".
const DIACRAFT_EXTENSIONS: [&str; 3] = ["diacraft", "diacraft-template", "diacraft-stencil"];

/// The extensions of the other formats `file.open` takes: VSDX drawings, stencils and templates, with and without macros, and OpenDocument drawings (`.odg`
/// part 2: opened and saved as they are, and not yet the type a new drawing is saved as; `.fodg`: a flat drawing, one XML document, which Save writes
/// back as a flat file, and which Save As does not offer).
const OTHER_EXTENSIONS: [&str; 9] = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"];

/// The filters of the Open dialog, in the order it lists them, the first the one it selects: DiaCraft's own files, then the other formats as they were before 0.2 (named by
/// the product of the VSDX types, as the Save dialog's labels are: owner, 2026-11-23), then every file so a file with another name can be tried (the window
/// says plainly if it is not a drawing).
fn open_filters() -> [(&'static str, &'static [&'static str]); 3] {
    [
        ("DiaCraft drawings", &DIACRAFT_EXTENSIONS),
        ("Visio and OpenDocument drawings, stencils and templates", &OTHER_EXTENSIONS),
        ("All files", &["*"]),
    ]
}

/// The chooser for File, Open: DiaCraft's own files first ([`open_filters`]).
pub fn open_picker() -> FilePicker {
    Arc::new(|| {
        let mut dialog = rfd::FileDialog::new().set_title("Open a drawing");
        for (label, extensions) in open_filters() {
            dialog = dialog.add_filter(label, extensions);
        }
        dialog.pick_file()
    })
}

/// The extensions of the stencil files `stencil.open` takes.
const STENCIL_EXTENSIONS: [&str; 3] = ["diacraft-stencil", "vssx", "vssm"];

/// The chooser for More Shapes, Open Stencil: the stencil kinds as the filter, DiaCraft's own first, and every file as a second so that a file with another name can be tried
/// (the engine says plainly if it is not a stencil).
pub fn stencil_picker() -> FilePicker {
    Arc::new(|| {
        rfd::FileDialog::new().set_title("Open a stencil").add_filter("Stencils", &STENCIL_EXTENSIONS).add_filter("All files", &["*"]).pick_file()
    })
}

/// The extensions of the pictures `insert.picture` takes: PNG, JPEG, GIF, BMP, TIFF, EMF and WMF. The engine tells a picture by its bytes and not by its name, so the list is the usual names.
const PICTURE_EXTENSIONS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "emf", "wmf"];

/// The chooser for Insert, Pictures: the picture types as the filter, and every file as a second so that a file with another name can be tried (the engine says plainly if it is not a
/// picture).
pub fn picture_picker() -> FilePicker {
    Arc::new(|| {
        rfd::FileDialog::new().set_title("Insert a picture").add_filter("Pictures", &PICTURE_EXTENSIONS).add_filter("All files", &["*"]).pick_file()
    })
}

/// The chooser for File, Save As: a name to start with (the drawing's) and the kinds of file the window asks it to offer, **the drawing's own first** (the dialog selects its
/// first kind). Which kinds, and in what order, is the window's request ([`SaveRequest`]): it asks the engine which saves are written, and this keeps no list of its own.
/// **The dialog keeps the name's extension in step with the kind chosen** (`rfd` is pinned to the fork that does this on each system, in `[patch.crates-io]`): the name starts with
/// the first kind's extension, so that is the one the dialog selects, and choosing another kind changes it to that kind's, unless the person typed an extension of their own. A name that
/// still comes back without an extension (a dialog that does not set one, such as the zenity fallback on Linux) gets the drawing's own type from the window.
pub fn save_picker() -> SavePicker {
    Arc::new(|request: SaveRequest| {
        let mut dialog = rfd::FileDialog::new().set_title("Save the drawing").set_file_name(request.name.as_str());
        for kind in &request.kinds {
            dialog = dialog.add_filter(kind.label, &[kind.extension]);
        }
        dialog.save_file()
    })
}

/// The chooser for File, Export: a name to start with (the drawing's, as a PNG) and the three kinds the engine writes as the filters. As in [`save_picker`], the dialog gives the name
/// the extension of the kind chosen; a name that comes back without an extension gets `.png` from the window, and one with another extension is told so in words.
pub fn export_picker() -> ExportPicker {
    Arc::new(|suggestion: String| {
        rfd::FileDialog::new()
            .set_title("Export the page as a picture")
            .set_file_name(suggestion)
            .add_filter("PNG image (.png)", &["png"])
            .add_filter("SVG drawing (.svg)", &["svg"])
            .add_filter("PDF document (.pdf)", &["pdf"])
            .save_file()
    })
}

/// The chooser for the Print view's Save as PDF: a name to start with (the drawing's, as a PDF) and PDF as the only kind. A name that comes back without an
/// extension gets `.pdf` from the window; one with another extension is told so in words.
pub fn print_pdf_picker() -> ExportPicker {
    Arc::new(|suggestion: String| {
        rfd::FileDialog::new()
            .set_title("Save what would be printed as a PDF")
            .set_file_name(suggestion)
            .add_filter("PDF document (.pdf)", &["pdf"])
            .save_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether the params doc of a command names the type `.extension` as a whole word (`.diacraft` is not found in `.diacraft-stencil`, and `.vss` in `.vssx`).
    fn names(doc: &str, extension: &str) -> bool {
        let needle = format!(".{extension}");
        doc.match_indices(&needle).any(|(at, _)| !doc[at + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-'))
    }

    #[test]
    fn the_filter_names_exactly_the_extensions_file_open_takes() {
        // The engine's command lists them in its parameter doc; a change in one must be a change in both.
        let command = diacraft_engine::find_command("file.open").map(|c| c.params).unwrap_or_default();
        for extension in DIACRAFT_EXTENSIONS.iter().chain(OTHER_EXTENSIONS.iter()) {
            assert!(names(command, extension), ".{extension} is not in the engine's doc of file.open: {command}");
        }
        assert!(OTHER_EXTENSIONS.contains(&"odg"), "the Open filter accepts an .odg");
        assert!(OTHER_EXTENSIONS.contains(&"otg"), "and an .otg, the drawing template: the engine opens it, and the dialog must list it");
        assert!(OTHER_EXTENSIONS.contains(&"fodg"), "and a flat .fodg, which the engine opens by what the file is");
    }

    /// The Open dialog lists "DiaCraft drawings" first (the filter it selects), with the three types of DiaCraft's own format, then the formats as they were,
    /// then every file; and between them the filters hold every type `file.open` takes, none twice.
    #[test]
    fn the_open_filter_lists_diacraft_drawings_first() {
        let filters = open_filters();
        assert_eq!(filters[0], ("DiaCraft drawings", &["diacraft", "diacraft-template", "diacraft-stencil"][..]));
        assert_eq!(
            filters[1].1,
            ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"],
            "the formats as today, and the OpenDocument template"
        );
        // The owner, 2026-11-23: the filter names the VSDX types by their product, as the Save dialog's labels do.
        assert_eq!(filters[1].0, "Visio and OpenDocument drawings, stencils and templates");
        assert_eq!(filters[2], ("All files", &["*"][..]), "and every file last");
        let mut listed: Vec<&str> = filters[..2].iter().flat_map(|(_, extensions)| extensions.iter().copied()).collect();
        let count = listed.len();
        listed.sort_unstable();
        listed.dedup();
        assert_eq!(listed.len(), count, "no type is in two filters");
        assert_eq!(count, 12, "the three of DiaCraft's own and the nine of the other formats");
    }

    /// The Open Stencil dialog takes DiaCraft's stencil beside the VSDX ones, and these are exactly the types `stencil.open` takes.
    #[test]
    fn the_stencil_filter_names_exactly_the_extensions_stencil_open_takes() {
        let command = diacraft_engine::find_command("stencil.open").map(|c| c.params).unwrap_or_default();
        for extension in STENCIL_EXTENSIONS {
            assert!(names(command, extension), ".{extension} is not in the engine's doc of stencil.open: {command}");
        }
        assert_eq!(STENCIL_EXTENSIONS[0], "diacraft-stencil", "DiaCraft's own first");
    }

    /// The Insert Pictures dialog lists the seven formats the engine takes, by their usual extensions, and the engine's own doc names each of them.
    #[test]
    fn the_picture_filter_names_the_formats_insert_picture_takes() {
        let command = diacraft_engine::find_command("insert.picture").map(|c| c.params).unwrap_or_default();
        for (format, extensions) in [
            ("PNG", &["png"][..]),
            ("JPEG", &["jpg", "jpeg"]),
            ("GIF", &["gif"]),
            ("BMP", &["bmp"]),
            ("TIFF", &["tif", "tiff"]),
            ("EMF", &["emf"]),
            ("WMF", &["wmf"]),
        ] {
            assert!(command.contains(format), "{format} is not in the engine's doc of insert.picture: {command}");
            assert!(extensions.iter().all(|e| PICTURE_EXTENSIONS.contains(e)), "{format}");
        }
        assert_eq!(PICTURE_EXTENSIONS.len(), 9);
    }

    /// The words check themselves: a type is found as a whole word only.
    #[test]
    fn a_type_is_found_as_a_whole_word_only() {
        assert!(names("opens .diacraft-stencil or .vssx", "vssx"));
        assert!(!names("opens .diacraft-stencil", "diacraft"), "`.diacraft` is not `.diacraft-stencil`");
        assert!(names("opens .diacraft, .vsdx", "diacraft"));
        assert!(!names("opens .vssx", "vss"));
        assert!(names("a drawing (.vsdx)", "vsdx"));
    }
}
