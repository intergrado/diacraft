//! A drawing that came from an `.odg`: what the engine keeps of it, and its save.
//!
//! **The design is the `.odg` format's** ("The writer that changes only what an edit touched"), and the
//! writer is `diacraft_odg::write`: a save compares the model with the file's own bytes and splices into them only what an edit touched. The engine's
//! part is to keep what that needs and to hand it over:
//!
//! * [`OdgState`], kept by the [`OpenFile`](crate::file::OpenFile) beside the file's bytes: the [`ShapeIndex`] that maps each shape of the model to
//!   its element in those bytes, and the shapes an earlier save flattened. **After each save the file is the bytes that save made and the index is
//!   the one it gave** (the writer holds the pages as it saved them, so the same model saved again is no edit and gives the same bytes,
//!   and the engine does not read the file again to find out).
//! * [`flatten`]: the copy of the drawing the writer is given, in which each new shape is what a new shape of an `.odg` is. The
//!   engine's model is never changed by a save.
//! * [`hold`]: what the engine holds beside the drawing (the undo history, the pages the last save was given, the shapes it flattened), which the writer's
//!   gate cannot see, **measured** and taken from the memory the save may use.
//! * [`save`]: the save itself: the copy, the thumbnail (an edited save writes a picture of the first page, rendered here, since the
//!   writer renders nothing), the writer, and the words of what it said or refused.
//!
//! **What is refused is said.** A change the writer does not write yet (a paragraph added or removed, a run's formatting, a dash, a gradient or a
//! hatch, a formula in a style cell, an arc or a NURBS in an outline, a new shape in a group or on a page that carries `draw:z-index`, and the
//! rest) ends the save with the writer's own words in the error, nothing is written, and the drawing and the file are as they were. A drawing is
//! saved in its own format: an `.odg` is not written as a VSDX file nor a VSDX drawing as an `.odg` (saving across formats waits), and
//! both are refused in words.
//!
//! **A drawing with no `.odg` to splice into** (a `.diacraft` drawing, or a new one) is saved as an `.odg` or an `.otg` **from the model alone**, [`save::save_model`]:
//! `diacraft_odg::write_with(doc, None, ..)` makes the package, with what the model holds of the file the drawing came from in its place. The shapes are handed over as a save into a file
//! hands its new ones ([`flatten::flatten_model`]), and the thumbnail and the generator follow the content stamp, never the dirty flag. The file written is then the drawing's own
//! file, the `.odg` its later saves splice into, with the index the writer gave.

pub(crate) mod flatten;
pub(crate) mod hold;
#[cfg(test)]
mod masters_cost;
pub(crate) mod note;
#[cfg(test)]
mod preview_cost;
pub(crate) mod save;
pub(crate) mod skeleton;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use diacraft_doc::Page;
use diacraft_odg::ShapeIndex;

use flatten::Flattened;

/// **What the save that wrote the file from the model left out of the drawing it gave the writer**, which every save into that file leaves out too: the drawing keeps it, and the
/// file never held it, so a save given the drawing whole differs from the file in what no save of it writes. The next save after a Save As, with no edit, was refused or counted as a change: a drawing
/// made from a built-in template ("a change of the document's own fields (the elements of its root)"), and one whose macro project the Save As was asked to leave out (the save put it back). A file
/// read from disk left nothing out: its drawing is what the file reads as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LeftOut {
    /// The package data of DiaCraft's own skeleton ([`skeleton::without_skeleton`]): a drawing made from a built-in template is written without it.
    pub skeleton: bool,
    /// What the Save As was asked to leave out (`file.save_as`'s `drop_macros` and `drop_objects`): the macro project of the other format, and the embedded objects (each shape that showed one shows
    /// its picture).
    pub asked: crate::across::Options,
    /// The embeddings of VSDX objects the model writer wrote as object frames (`diacraft_odg::without_carried_objects`): the file holds each object once, as the frame's own `Object N`, and
    /// no VSDX embedding rides in it. The drawing keeps the embedding, so a save into the file is given the drawing without it, or the splice finds a part riding that the file does not carry and writes the
    /// object a second time in DiaCraft's own folder.
    pub carried_objects: bool,
    /// **The file was written from the model of the drawing** (`save_model`, a Save As), and the saves into it since: the drawing is not the file's reading, and the file holds nothing of
    /// its own that the drawing lacks (what a splice writes of its own is bookkeeping, `meta:generator` and the thumbnail). So **a save from the model is given the drawing, not the file's package data**
    /// ([`file_package_data`] gives none): the file's parts are what the model writer made of the drawing's (a VSDX object as the `.odg`'s own object file, a VBA project as an entry of DiaCraft's), and
    /// put back on a drawing of VSDX shapes they named nothing it shows (a VSDX file written from it held the object as an entry of DiaCraft's own, and its shape named an embedding that was not there).
    /// `false` for a file read from the disk, whose package data the drawing follows.
    pub from_model: bool,
}

/// What the engine keeps of a drawing read from an `.odg` or saved as one: where each shape of the model is in the file's bytes, which shapes the saves so far
/// flattened, the pages the last save was given, and what the save that wrote the file from the model left out. Cloning it copies three pointers and three flags, and the
/// session clones its state for every command.
#[derive(Clone, Debug)]
pub struct OdgState {
    index: Arc<ShapeIndex>,
    flattened: Arc<Flattened>,
    /// The pages the last save gave the writer, which its index holds beside the model (the copy of the drawing with its new shapes flattened): memory the
    /// next save counts (`hold.rs`).
    saved: Arc<Vec<Arc<Page>>>,
    /// What the save that wrote the file from the model left out, which every save into the file leaves out too ([`LeftOut`]).
    left_out: LeftOut,
}

impl OdgState {
    /// The state of a file just read: the index the read gave, and nothing flattened or left out.
    pub fn new(index: ShapeIndex) -> OdgState {
        OdgState { index: Arc::new(index), flattened: Arc::new(Flattened::default()), saved: Arc::new(Vec::new()), left_out: LeftOut::default() }
    }

    /// The state after a save: the index the save gave, the shapes that save flattened (so that the save after gives their copies again when they have not
    /// changed, and does not say again what this one said), the pages the writer was given, and what the file was written without (`left_out`).
    pub(crate) fn saved(index: ShapeIndex, flattened: Flattened, pages: Vec<Arc<Page>>, left_out: LeftOut) -> OdgState {
        OdgState { index: Arc::new(index), flattened: Arc::new(flattened), saved: Arc::new(pages), left_out }
    }

    /// What the save that wrote the file from the model left out of the drawing (see [`LeftOut`]).
    pub(crate) fn left_out(&self) -> LeftOut {
        self.left_out
    }

    /// Where each shape of the model is in the file's bytes.
    pub fn index(&self) -> &ShapeIndex {
        &self.index
    }

    /// The shapes the saves so far flattened, with the copy of each and the bits of what it holds that is not written.
    pub(crate) fn flattened(&self) -> &Flattened {
        &self.flattened
    }

    /// The pages the last save gave the writer.
    pub(crate) fn saved_pages(&self) -> &[Arc<Page>] {
        &self.saved
    }
}

/// The package data of the file an `.odg` drawing stands on, as it is now: the file the drawing was opened from, or the one the last save wrote, which the
/// source holds as its bytes. `None` for a source that is not an `.odg` package (a VSDX file, a `.diacraft`, no file, a flat drawing, which has no package). The file is read again
/// beside the drawing, so it is refused, in words, when the process has not the room for the model of the read (what the read of the file cost, from its index, against what the
/// `.odg` writer may hold less what the process holds); nothing is made then.
pub(crate) fn file_package_data(source: &crate::file::OpenFile) -> Result<Option<diacraft_odg::PackageData>, String> {
    let Some(state) = source.odg() else { return Ok(None) };
    // A file written from the model of the drawing holds nothing the drawing lacks (`LeftOut::from_model`).
    if source.is_flat() || state.left_out().from_model {
        return Ok(None);
    }
    let cost = state.index().read_cost();
    let need = cost.model_bytes().saturating_add(cost.transient_bytes());
    let room = diacraft_odg::limits::MAX_WRITE_MEMORY.saturating_sub(crate::memory::held());
    if let Some(words) = no_room_words(need, room) {
        return Err(words);
    }
    diacraft_odg::read_package_data(source.bytes(), &diacraft_odg::Limits::default())
        .map(Some)
        .map_err(|e| crate::shown(&format!("the drawing could not be saved: the file it was opened from cannot be read again: {e}"), 300))
}

/// What a save says when the file it stands on cannot be read again beside the drawing: the model of that read takes `need` bytes and the process has `room`. `None` when it fits.
pub(crate) fn no_room_words(need: u64, room: u64) -> Option<String> {
    (need > room).then(|| {
        format!(
            "the drawing could not be saved: the file it was opened from cannot be read again beside the drawing, which takes about {} MiB, and the process has room for {} MiB. \
             Nothing was written",
            need.div_ceil(1024 * 1024),
            room / (1024 * 1024)
        )
    })
}

/// What reading a theme of the document file could not do: the part, why nothing could be read of it if so, and how many entries and lists were left undone (as the VSDX
/// reader names them).
#[derive(Debug)]
pub(crate) struct ThemeNote {
    part: String,
    problem: Option<&'static str>,
    unread: usize,
    capped: usize,
}

/// Reads the themes the document file carries. The `odg` crate keeps a theme as its part name and its bytes and reads nothing of DrawingML, so a theme
/// of a drawing read from an `.odg` has no colours, no fonts and no scheme numbers, and the theme functions, `theme.list` and `theme.apply` (which looks a theme up by
/// the number its components carry, and adds the part again when it finds none) answer as if the drawing had none. The part is read as the VSDX reader reads one
/// (`diacraft_vsdx::read_theme_part_reported`, whose XML is held to the limits it has), **bounded as that reader bounds themes**: at most [`diacraft_vsdx::MAX_THEMES`] of
/// them, and [`diacraft_vsdx::MAX_THEME_BYTES`] of bytes in all, in the order the file lists them; a theme past either is left as the bytes it is. What the writer
/// compares of a theme is its part name and its bytes, so reading it changes nothing a save writes. **What could not be read is told** (
/// a malformed theme opened with no word, where a VSDX file says `theme_unreadable`): a note for each theme that is not read whole.
pub(crate) fn read_themes(doc: &mut diacraft_doc::Document) -> Vec<ThemeNote> {
    let mut notes: Vec<ThemeNote> = Vec::new();
    if doc.themes.is_empty() {
        return notes;
    }
    let mut room = diacraft_vsdx::MAX_THEME_BYTES;
    let read: Vec<Arc<diacraft_doc::Theme>> = doc
        .themes
        .iter()
        .enumerate()
        .map(|(at, theme)| {
            let len = theme.bytes.len() as u64;
            if at >= diacraft_vsdx::MAX_THEMES || len > room {
                notes.push(ThemeNote { part: theme.part.to_string(), problem: Some("past_the_most_themes_or_bytes"), unread: 0, capped: 0 });
                return Arc::clone(theme);
            }
            room -= len;
            let done = diacraft_vsdx::read_theme_part_reported(&theme.part, &theme.bytes);
            if done.problem.is_some() || done.unread > 0 || done.capped > 0 {
                notes.push(ThemeNote { part: theme.part.to_string(), problem: done.problem, unread: done.unread, capped: done.capped });
            }
            // The theme keeps the very allocation of its bytes: the derived data is read from them and they are not changed, and a `.diacraft` save copies a blob as it is only when the model still
            // holds the allocation it was read as (`diacraft_native::write`: blobs are numbered by first use, and a part is never copied on the word of a hash).
            let mut derived = done.theme;
            derived.bytes = Arc::clone(&theme.bytes);
            Arc::new(derived)
        })
        .collect();
    doc.themes = diacraft_doc::SharedVec::from_vec(read);
    notes
}

/// Reads an `.odg` as `file.open` does, **the one way an `.odg` is opened** (the command line's export read one with no theme parsed, so
/// its colours were not what the window's): the `odg` crate's read, then the themes the document file carries read ([`read_themes`]), with a warning of the report for each
/// theme that could not be read whole, worded as the VSDX reader words them.
pub fn read_odg(bytes: &[u8], limits: &diacraft_odg::Limits) -> Result<(diacraft_doc::Document, diacraft_odg::ReadReport), diacraft_odg::OdgError> {
    let (mut doc, mut report) = diacraft_odg::read(bytes, limits)?;
    for note in read_themes(&mut doc) {
        for (class, words) in theme_note_words(&note) {
            warn(&mut report, limits, class, words);
        }
    }
    // A loop of containers the file holds is broken, and said first.
    let loops = crate::draw::container::cycles::untangle(&mut doc);
    if !loops.is_empty() {
        let count = report.warning_counts.entry("container_loop").or_insert(0);
        *count = count.saturating_add(u64::try_from(loops.len()).unwrap_or(u64::MAX));
        report.warnings.splice(0..0, loops);
    }
    Ok((doc, report))
}

/// What a person is told of a theme that could not be read whole, each with its class word, **worded as the VSDX reader words them** (`theme_unreadable`,
/// `theme_entry_unreadable`, `theme_list_capped`): the `.odg` open and the `.diacraft` open say the same of a theme.
pub(crate) fn theme_note_words(note: &ThemeNote) -> Vec<(&'static str, String)> {
    let part = note.part.escape_debug().to_string();
    let mut said: Vec<(&'static str, String)> = Vec::new();
    if let Some(class) = note.problem {
        said.push(("theme_unreadable", format!("nothing could be read from the theme part {part} ({class}); its bytes are kept")));
    }
    if note.unread > 0 {
        said.push((
            "theme_entry_unreadable",
            format!("{} entries of the theme part {part} could not be read and are left unread; its bytes are kept", note.unread),
        ));
    }
    if note.capped > 0 {
        said.push((
            "theme_list_capped",
            format!("{} lists of the theme part {part} are longer than the reader keeps; the rest are left unread", note.capped),
        ));
    }
    said
}

/// A warning of `class` in a report, counted and named in words while the report has room (as the reader's own).
fn warn(report: &mut diacraft_odg::ReadReport, limits: &diacraft_odg::Limits, class: &'static str, words: String) {
    let count = report.warning_counts.entry(class).or_insert(0);
    *count = count.saturating_add(1);
    if report.warnings.len() < limits.max_report_entries {
        report.warnings.push(words);
    }
}

/// What a person is told at the open of a flat OpenDocument file (`.fodg`: one XML document), as the first note of the open:
/// Save writes it back as a flat file, and Save As writes it as a `.diacraft` file or an `.odg` package, which the person would otherwise
/// learn only at the Save As.
pub(crate) const FLAT_OPEN_NOTE: &str = "this is a flat OpenDocument file (.fodg): Save writes it back as a flat file, and Save As writes it as a .diacraft file or an .odg package (a .fodg is not written by Save As)";

/// What a person is told when `file.save_as` is asked for a `.fodg` for a drawing that did not come from a flat file: a flat file is
/// written by Save alone, for a drawing that came from one, and **no other drawing is saved as one** (the design's section 7: Save As `.fodg` is refused in words that point to the `.odg`
/// package, which the model writer now writes from any drawing).
pub(crate) const FODG_NOT_OFFERED: &str = "a flat OpenDocument drawing (.fodg) is not written by Save As: a drawing opened from one is saved by Save, as a flat file. Save the drawing as an .odg, the OpenDocument package, instead. Nothing was written.";

/// **The types of OpenDocument drawing file the engine opens and saves**, by the extension that names each, in lower case and with no dot: `.odg` a drawing and `.otg` a drawing template
/// (`.otg` is a format to open and save, and no built-in template). A flat `.fodg` is not among them: it is opened, saved back as the flat file it is by Save, and never a Save As target (a drawing opened from one is saved by Save As as a `.diacraft` file or an `.odg` package). A
/// frontend that lists the types a convert or a Save As takes (the command line's `convert`) takes this list from the engine, and asks `Session::save_as_allows` which of them a drawing is
/// saved as.
pub const SAVE_TYPES: [&str; 2] = ["odg", "otg"];

/// Whether the drawing in `file` is one saved as an `.odg`: its working file's format is OpenDocument (every place that depends on the format asks
/// [`OpenFile::format`](crate::file::OpenFile::format), and not the presence of the `.odg` state, so that a drawing of another format that holds an `.odg`'s data is not taken for one).
pub(crate) fn is_odg(file: Option<&crate::file::OpenFile>) -> bool {
    file.is_some_and(|f| f.format() == Some(crate::file::FileFormat::Odg))
}
