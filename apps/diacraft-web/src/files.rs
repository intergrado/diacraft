//! What the browser's file handling decides without touching the browser: which names a chooser offers, how a file is read in pieces, what a download is called and what type it is, and the
//! words a refused file is told in. It builds for every target, so the tests run it on the desktop; `web_files` (on `wasm32` only) is the part that meets the browser and uses all of this.
//!
//! The rules of what a file may be (the budgets, the refusals) are the engine's: [`diacraft_engine::memfs::admit`] judges a file from its size **before a byte of it is read**, with the rule `file.open`
//! applies, so a file the engine would refuse is refused here at once and the page never makes room for it (a WebAssembly memory only grows).

use diacraft_engine::OpenProblem;
use diacraft_engine::shown;
use diacraft_ui_egui::{Want, refusal_text};

/// The size of the pieces a file is read in: a piece is copied from the browser's own memory into the page's, so the browser holds this much of a file twice at most, and the page can draw a frame
/// between two pieces.
pub const CHUNK_BYTES: u64 = 8 << 20;

/// The most files one drop takes: the first opens, the others are told. A drop of a thousand files makes no thousand reads.
pub const MAX_DROPPED: usize = 8;

/// The most handles to chosen files the page keeps for writing in place: a handle is small, and each is of a file the person chose.
pub const MAX_HANDLES: usize = 16;

/// The longest extension (with its dot) a browser's file chooser accepts in its list of types: a longer one makes the whole request fail.
const MAX_ACCEPT_EXTENSION: usize = 16;

/// The extensions of DiaCraft's own files, and of VSDX and OpenDocument drawings, stencils and templates: what `file.open` takes (a test holds this to the engine's own list).
const DRAWING_EXTENSIONS: [&str; 12] =
    ["diacraft", "diacraft-template", "diacraft-stencil", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"];

/// The extensions of the stencil files `stencil.open` takes.
const STENCIL_EXTENSIONS: [&str; 3] = ["diacraft-stencil", "vssx", "vssm"];

/// The extensions of the pictures `insert.picture` takes (the engine tells a picture by its bytes: these are the usual names).
const PICTURE_EXTENSIONS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "emf", "wmf"];

/// The extensions a chooser for `want` lists first (every file can still be chosen: the engine says plainly if it is not what was wanted).
pub fn extensions(want: Want) -> &'static [&'static str] {
    match want {
        Want::Drawing => &DRAWING_EXTENSIONS,
        Want::Stencil => &STENCIL_EXTENSIONS,
        Want::Picture => &PICTURE_EXTENSIONS,
    }
}

/// What the chooser for `want` is called.
pub fn description(want: Want) -> &'static str {
    match want {
        Want::Drawing => "Drawings, stencils and templates",
        Want::Stencil => "Stencils",
        Want::Picture => "Pictures",
    }
}

/// The `accept` of a file input: the extensions with their dots, joined by commas.
pub fn input_accept(want: Want) -> String {
    extensions(want).iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(",")
}

/// The extensions with their dots that a chooser of the File System Access API may list: those it accepts (a longer one, as `.diacraft-template`, makes the whole call fail, so it is left out of the
/// list; the chooser still lets the person pick any file).
pub fn listable(extensions: &[&str]) -> Vec<String> {
    extensions.iter().map(|e| format!(".{e}")).filter(|e| e.len() <= MAX_ACCEPT_EXTENSION).collect()
}

/// The type a download of a file called `name` is given, by its extension. A drawing is a package or XML the browser has no use for: it is saved, never shown.
pub fn mime_of(name: &str) -> &'static str {
    match std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// The ranges of a file of `size` bytes, a piece each, in order: `(start, end)` with `end` past the last byte, none empty, together all of the file and no byte twice.
pub fn chunks(size: u64) -> impl Iterator<Item = (u64, u64)> {
    let mut start = 0u64;
    std::iter::from_fn(move || {
        if start >= size {
            return None;
        }
        let end = start.saturating_add(CHUNK_BYTES).min(size);
        let piece = (start, end);
        start = end;
        Some(piece)
    })
}

/// The size of a file in bytes, from the number the browser gives (`File.size`, a double): `None` for what no file has (a negative number, a fraction, not a number, more than a page's memory
/// could be indexed with).
pub fn size_of(reported: f64) -> Option<u64> {
    // 2^32 bytes is the most a WebAssembly memory holds, and the most `usize` is on that target.
    if reported.is_finite() && reported >= 0.0 && reported.fract() == 0.0 && reported < 4_294_967_296.0 {
        // The checks above make the cast exact.
        Some(reported as u64)
    } else {
        None
    }
}

/// How a file is called in a message: no folder, escaped, cut short.
pub fn name_in_words(name: &str) -> String {
    shown(name, 64)
}

/// What a person is told of a file that was refused from its size or for want of room: the desktop's words for the kind of trouble, with the file's name.
pub fn refused(name: &str, problem: OpenProblem) -> String {
    refusal_text(&name_in_words(name), problem)
}

/// What a person is told of a file the browser would not give: it could not be read (it was taken away, or changed while it was read).
pub fn unreadable(name: &str) -> String {
    format!("{} could not be read from your computer. It may have been moved, renamed or changed while it was being read.", name_in_words(name))
}

/// What a person is told of a file the page has no room to hold.
pub fn no_room(name: &str) -> String {
    format!("There is not enough memory to open {} in the browser. Close other tabs, or open a smaller file.", name_in_words(name))
}

/// What a person is told when the browser would not show its chooser (`why` is the browser's own words, escaped and cut short).
pub fn no_chooser(why: &str) -> String {
    format!("The browser would not show its file chooser: {}", shown(why, 160))
}

/// The name a download is saved under: the file's name in the engine's memory, which is the name the person gave it.
pub fn download_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| "drawing".to_owned(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pieces_of_a_file_are_all_of_it_in_order_and_none_twice() {
        assert_eq!(chunks(0).count(), 0, "an empty file has no piece");
        assert_eq!(chunks(1).collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(chunks(CHUNK_BYTES).collect::<Vec<_>>(), [(0, CHUNK_BYTES)]);
        assert_eq!(chunks(CHUNK_BYTES + 1).collect::<Vec<_>>(), [(0, CHUNK_BYTES), (CHUNK_BYTES, CHUNK_BYTES + 1)]);
        for size in [7, CHUNK_BYTES - 1, 3 * CHUNK_BYTES, 3 * CHUNK_BYTES + 5, 4_294_967_295] {
            let mut next = 0;
            for (start, end) in chunks(size) {
                assert_eq!(start, next, "{size}: no gap and no overlap");
                assert!(end > start && end - start <= CHUNK_BYTES, "{size}");
                next = end;
            }
            assert_eq!(next, size, "{size}: all of the file");
        }
        // The last piece of the largest file the page takes does not overflow.
        assert_eq!(chunks(u64::MAX).next(), Some((0, CHUNK_BYTES)));
    }

    #[test]
    fn a_size_the_browser_reports_is_a_whole_number_a_page_can_hold() {
        assert_eq!(size_of(0.0), Some(0));
        assert_eq!(size_of(1234.0), Some(1234));
        assert_eq!(size_of(4_294_967_295.0), Some(4_294_967_295));
        for bad in [-1.0, 0.5, 4_294_967_296.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
            assert_eq!(size_of(bad), None, "{bad}");
        }
    }

    /// The extensions a chooser offers are the ones the engine's own commands say they take, so a type added to a command is added here too.
    #[test]
    fn the_choosers_list_what_the_engines_commands_take() {
        let names = |doc: &str, extension: &str| {
            let needle = format!(".{extension}");
            doc.match_indices(&needle).any(|(at, _)| !doc[at + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-'))
        };
        let open = diacraft_engine::find_command("file.open").map(|c| c.params).unwrap_or_default();
        for extension in extensions(Want::Drawing) {
            assert!(names(open, extension), ".{extension} is not in the engine's doc of file.open: {open}");
        }
        let stencil = diacraft_engine::find_command("stencil.open").map(|c| c.params).unwrap_or_default();
        for extension in extensions(Want::Stencil) {
            assert!(names(stencil, extension), ".{extension} is not in the engine's doc of stencil.open: {stencil}");
        }
        assert_eq!(extensions(Want::Drawing).len(), 12);
        assert!(extensions(Want::Picture).contains(&"png") && extensions(Want::Picture).contains(&"jpeg"));
    }

    #[test]
    fn the_accept_list_of_an_input_has_dots_and_commas() {
        assert!(input_accept(Want::Drawing).starts_with(".diacraft,.diacraft-template,"));
        assert!(input_accept(Want::Drawing).ends_with(",.odg,.otg,.fodg"));
        assert_eq!(input_accept(Want::Stencil), ".diacraft-stencil,.vssx,.vssm");
    }

    #[test]
    fn a_list_for_the_file_system_api_leaves_out_what_it_would_refuse() {
        let all = listable(extensions(Want::Drawing));
        assert!(all.contains(&".diacraft".to_owned()) && all.contains(&".vsdx".to_owned()) && all.contains(&".fodg".to_owned()));
        assert!(!all.iter().any(|e| e.len() > MAX_ACCEPT_EXTENSION), "{all:?}");
        assert!(!all.contains(&".diacraft-template".to_owned()), "18 characters, with the dot");
        assert!(all.iter().all(|e| e.starts_with('.')));
    }

    #[test]
    fn a_download_is_given_the_type_of_its_extension() {
        assert_eq!(mime_of("page.png"), "image/png");
        assert_eq!(mime_of("Page.SVG"), "image/svg+xml");
        assert_eq!(mime_of("print.pdf"), "application/pdf");
        for other in ["plan.vsdx", "plan.diacraft", "plan.odg", "plan", ""] {
            assert_eq!(mime_of(other), "application/octet-stream", "{other}");
        }
    }

    #[test]
    fn a_refusal_is_the_desktops_words_with_the_files_name_escaped_and_cut_short() {
        assert_eq!(refused("big.vsdx", OpenProblem::TooBig), "big.vsdx is too big to open safely: it holds more than DiaCraft will read.");
        assert_eq!(refused("big.vsdx", OpenProblem::OutOfMemory), "There is not enough memory to open big.vsdx.");
        let hostile = refused(&format!("a\u{1b}[31m{}\u{202e}.vsdx", "x".repeat(500)), OpenProblem::FileTooLarge);
        assert!(!hostile.chars().any(|c| c.is_control() || c == '\u{202e}'), "{hostile:?}");
        assert!(hostile.chars().count() < 120, "{}", hostile.chars().count());
        assert!(unreadable("x.odg").starts_with("x.odg could not be read"));
        assert!(no_room("x.odg").contains("not enough memory"));
        assert!(no_chooser(&"w".repeat(1000)).chars().count() < 260);
    }

    #[test]
    fn a_download_is_called_by_the_name_the_person_gave() {
        assert_eq!(download_name(std::path::Path::new("/web/3/plan.vsdx")), "plan.vsdx");
        assert_eq!(download_name(std::path::Path::new("/")), "drawing");
    }
}
