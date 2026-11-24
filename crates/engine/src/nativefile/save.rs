//! The save of a drawing as a `.diacraft` file: the state the file keeps, the picture of the first page, and the package, held to the memory the process leaves.
//!
//! The file that is written, the check of the path, the comparison of the file with what it was and the swap of a temporary file are `cmd/file.rs`'s, as for the other formats; this makes the
//! bytes. It writes no file and changes no session.
//!
//! **What a save keeps.** `state.xml` holds the identifier ledger of the session (for each page the highest shape `ID` ever given, so that a deleted shape's `ID` is not given again after a
//! reopen) and the content stamp of the drawing as it was read from a foreign file. **A save that changed nothing writes the file it read**: the package copies every entry whose model object
//! is still the one decoded and every picture it names the very bytes it was (`diacraft_native::write`), so a large drawing stays cheap to save.
//!
//! **Memory.** The package's own write counts what it makes (the serialized and deflated copies of a part in hand, the package as it grows, the directory of the one it copies from), and
//! **the model and the codec's trees are counted here, once**: the process's total ([`crate::memory::held`]) holds the open drawing and its file, the stencils and every other
//! session, **the model once** (the session's hold is what it has grown by since the file was read, less the model a save keeps), and the write is given `Limits::for_save` of it: what is
//! left of the package's budget (`diacraft_native::MAX_MEMORY`) beside it once the codec's own room for the tree of the child in hand (`save_child_items`), the tables
//! of its check and the bytes of a part are taken out, with the tree of a part held to the items that fit beside the model (`save_tree_items`), as the codec's own arithmetic holds it
//! when it is alone. A refusal for memory names that budget, the one the save was held against.

use diacraft_doc::Document;
use diacraft_native::{Ledger, NativeError, Previous, State, WriteOptions};

use super::NativeState;
use crate::error::{EngineError, failed};
use crate::file::OpenFile;
use crate::{HeldMemory, Session};

/// What a save made.
pub(crate) struct Saved {
    /// The package.
    pub bytes: Vec<u8>,
    /// What the next save copies from: the entries just written, and the model objects they hold.
    pub state: NativeState,
    /// How many entries of the package were copied as they were from the file the drawing was read from or last saved as.
    pub copied: usize,
    /// What the save says, in words: the picture of the first page when it could not be made.
    pub warnings: Vec<String>,
}

/// Who wrote the file, as the manifest says it: free text for people.
fn written_by() -> String {
    format!("DiaCraft {}", env!("CARGO_PKG_VERSION"))
}

/// The ledger the session keeps for `doc`: for each page, the highest `ID` the session has given a shape on it, and the highest the file it saved from or last saved to holds
/// (`Session::id_floor`). The write raises each mark to the highest `ID` the page holds now, and writes no row for a page the drawing lacks.
pub(crate) fn ledger_of(s: &Session, doc: &Document) -> Ledger {
    Ledger(doc.pages.iter().filter_map(|page| Some((page.id, s.id_floor(page.id))).filter(|(_, mark)| *mark > 0)).collect())
}

/// What a refusal of the package's write is told as: the words of the refusal, and for want of memory what the process holds besides.
fn refusal(cmd: &str, e: &NativeError, held: &HeldMemory) -> EngineError {
    let memory = matches!(e, NativeError::TooLarge(words) if words.contains("memory"));
    let mut words = crate::shown(&format!("the drawing could not be saved: {e}"), 300);
    if memory && held.total() > 0 {
        words = format!(
            "{}: the open drawing and its file hold {} bytes, the open stencils {} and the rest of the process {} of the {} bytes that a save may use at once: close a stencil, or another drawing, and save again",
            crate::shown(&format!("the drawing could not be saved: not enough memory beside what is open ({e})"), 300),
            held.drawing,
            held.stencils,
            held.elsewhere(),
            diacraft_native::MAX_MEMORY
        );
    }
    failed(cmd, words)
}

/// The most shapes the first page may hold for a save to make its picture. **A preview is a picture of at most 256 pixels for a file manager**: a page of thousands of shapes is a grey
/// blur at that size, and the render is a pass over every shape (83 ms for 20,000 shapes, a third of an edited save of them: the review, measured). A page past this has no
/// preview, silently (a file with none is a file like any other, and an edit that changed the page never copies a stale one). The count is of the page's own shapes, one level.
pub(crate) const PREVIEW_MOST_SHAPES: usize = 5_000;

/// Whether a save makes the picture of the first page: the drawing has a page, and it is not one a picture of 256 pixels would be noise of ([`PREVIEW_MOST_SHAPES`]). The page is the one
/// the picture is of: the first that is not a background, else the first.
fn preview_wanted(doc: &Document) -> bool {
    let page = doc.pages.iter().find(|p| !p.background).or_else(|| doc.pages.iter().next());
    page.is_some_and(|page| page.shapes.len() <= PREVIEW_MOST_SHAPES)
}

/// The bytes of `doc` as a `.diacraft` file of `kind`, copied from `source`'s package where nothing changed. `source` is the file the drawing was read from or last saved to, which is
/// copied from only when it is a `.diacraft` file. `edited` says whether `doc` is not what that file holds (the drawing has unsaved changes, or `doc` is a copy made for this save, as a
/// template's with the list of stencils): only then is the picture of the first page made again. A change the package cannot hold is `Err` with its words, and nothing is made.
pub(crate) fn save(
    cmd: &str,
    s: &Session,
    doc: &Document,
    source: Option<&OpenFile>,
    kind: diacraft_native::Kind,
    edited: bool,
) -> Result<Saved, EngineError> {
    let held = s.held_memory();
    // **`Limits::for_save`**: the figure the process holds in all, the model once, less the codec's own room for the tree of the child in hand, the
    // tables of its check and the bytes of a part, is what the package itself may take; a process that leaves no room is refused here, in words, before anything is made.
    let limits = diacraft_native::Limits::for_save(held.total()).map_err(|e| refusal(cmd, &e, &held))?;
    let state = State { ledger: ledger_of(s, doc), content_stamp: s.content_stamp() };
    // A stencil that is made of another kind of file has no picture to carry over: the package it would copy from has one of its own page, and a stencil has none. (A stencil saved over a
    // stencil, and every other kind, copies what did not change.)
    let previous = source
        .and_then(|f| f.native().map(|native| (f, native)))
        .filter(|(_, native)| kind != diacraft_native::Kind::Stencil || native.kind() == diacraft_native::Kind::Stencil)
        .map(|(f, native)| Previous { bytes: f.bytes(), decoded: native.decoded() });
    // The picture of the first page is made when the model is not what the package it would be copied from holds, when there is no such package, and when that package has no picture to copy: a clean `.diacraft` drawing saved again
    // writes the picture the file has, which is the file's own, and costs no rendering. A stencil has no page to show.
    let unchanged = !edited && previous.as_ref().is_some_and(|p| p.decoded.has_preview() || kind == diacraft_native::Kind::Stencil);
    let mut warnings: Vec<String> = Vec::new();
    let preview = if unchanged || kind == diacraft_native::Kind::Stencil || !preview_wanted(doc) {
        None
    } else {
        match crate::odgfile::save::thumbnail(doc) {
            Ok(png) => Some(png),
            Err(why) => {
                warnings.push(format!("the picture of the first page could not be made, so no preview was written: {why}"));
                None
            }
        }
    };
    let options = WriteOptions { kind, written_by: written_by(), preview, limits };
    let written =
        diacraft_native::write_threads(doc, &state, previous.as_ref(), &options, crate::threads::save_threads_within(options.limits.max_memory))
            .map_err(|e| refusal(cmd, &e, &held))?;
    Ok(Saved { bytes: written.bytes, state: NativeState::new(kind, written.decoded), copied: written.copied, warnings })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_page_of_more_than_five_thousand_shapes_has_no_preview() {
        let mut doc = Document::new_blank();
        assert!(preview_wanted(&doc), "a blank page");
        let shape = std::sync::Arc::new(diacraft_doc::Shape::default());
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for _ in 0..PREVIEW_MOST_SHAPES {
            page.shapes.push(std::sync::Arc::clone(&shape));
        }
        assert!(preview_wanted(&doc), "as many as the most is enough");
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.shapes.push(shape);
        assert!(!preview_wanted(&doc), "one more is not");
        assert_eq!(PREVIEW_MOST_SHAPES, 5_000);
        // No page, no picture; the picture is of the first page that is not a background, so a background page of many shapes does not take it away.
        doc.pages = diacraft_doc::SharedVec::from_vec(Vec::new());
        assert!(!preview_wanted(&doc));
    }

    #[test]
    fn a_refusal_for_memory_says_what_is_held_and_any_other_is_not_told_as_one() {
        let held = HeldMemory { drawing: 700, stencils: 30, process: 900 };
        let memory = NativeError::TooLarge("the file needs more memory to save than is free");
        let words = refusal("file.save", &memory, &held).to_string();
        assert!(words.contains("not enough memory beside what is open") && words.contains("hold 700 bytes") && words.contains("170"), "{words}");
        assert!(
            !refusal("file.save", &memory, &HeldMemory::default()).to_string().contains("beside what is open"),
            "nothing held: the words as they are"
        );
        let other = NativeError::TooLarge("the package would be larger than a DiaCraft file may be");
        let words = refusal("file.save", &other, &held).to_string();
        assert!(words.contains("larger than a DiaCraft file may be") && !words.contains("beside what is open"), "{words}");
    }
}
