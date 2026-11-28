//! The root style of a model written from nothing.
//!
//! A drawing the program made (a new one, one read from an `.odg`, one read from a `.diacraft` file) is written with a root style, style 0, that states what the renderer assumes where no cell
//! on a shape's way says: the whole of it for a new drawing (`diacraft_doc::Document::new_blank`), and for the other models **the cells whose absence a reader of a file reads as another value
//! than the assumed one** (`diacraft_doc::root_text`, [`Scope::Reading`]): a paragraph with no `HorzAlign` is centred here and is left-aligned for the reference application, which takes the
//! default of the cell's kind, 0, where the root holds nothing. A root that holds a cell is never changed, and a section it deletes is not brought back.
//!
//! **What is left alone.** A model that came from a VSDX package ([`came_from_a_package`]) holds the file's own root, which the writer does not touch, as a save into the file does not: whatever
//! that root lacks, the file lacked, and a drawing that reads centred here and left-aligned there is a matter of how the file is read, not of what is written. **But for one root: DiaCraft's own
//! earlier one** (`diacraft_doc::root_text::is_an_earlier_root`). A drawing made from a template holds the package data of DiaCraft's own skeleton, and a VSDX file a build before this one wrote holds
//! a package as well: both hold the root that build made, which no file of another program holds (every root of the census has its `Paragraph` section), so what it lacks is DiaCraft's to state.
//! A save into a file (`original` given) never reaches this module.
//!
//! **The model given back is the caller's.** The cells are added to the file and not to the model the caller keeps (the model handed back by the save is what the file reads as, and the baseline of
//! the next save into it): the caller's own drawing is not changed by a save, and if the baseline held cells the caller's drawing lacks, the next save into the file would take them out as an edit.
//! [`Stated::give_back`] puts the caller's root into the model handed back, as the skeleton puts the caller's font table (`write::run`): the root of the file is the writer's, and not an edit.

use std::sync::Arc;

use diacraft_doc::root_text::{Scope, complete, is_an_earlier_root};
use diacraft_doc::{Document, Style};

/// Whether `doc` was read from a VSDX package, or holds a part of one: the model writer's own definition of a drawing that has a VSDX file behind it (`held::wanted`), apart from the one that
/// is only a drawing with unknown elements of its own. A drawing whose window settings were set and that holds nothing else of a package ([`diacraft_doc::VsdxHeld::holds_windows_alone`]) came from no
/// file. (The test is `diacraft_doc::root_text::came_from_a_vsdx_package`, which the text rows of a save across use too.)
pub(super) fn came_from_a_package(doc: &Document) -> bool {
    diacraft_doc::root_text::came_from_a_vsdx_package(doc)
}

/// A model with its root completed, and the root as the caller had it.
pub(super) struct Stated {
    /// The model to write: the caller's with style 0 completed. Nothing else of it is touched, and what it holds is shared with the caller's.
    pub doc: Document,
    /// The root the caller had.
    given: Arc<Style>,
}

/// The model to write for `doc`, with its root completed to [`Scope::Reading`], or `None` when `doc` came from a package and its root is not DiaCraft's own earlier one, has no style 0, or holds
/// every cell already.
pub(super) fn stated(doc: &Document) -> Option<Stated> {
    let at = doc.styles.iter().position(|style| style.id == 0)?;
    let given = Arc::clone(doc.styles.get(at)?);
    // A drawing made from a template holds the package data of DiaCraft's own skeleton, and a VSDX file an earlier build wrote holds a package: both came from a package, and both hold the root
    // that build made, which is DiaCraft's own to state (`diacraft_doc::root_text::is_an_earlier_root`).
    if came_from_a_package(doc) && !is_an_earlier_root(&given) {
        return None;
    }
    let completed = complete(&given, Scope::Reading)?;
    let mut changed = doc.clone();
    changed.styles.set(at, Arc::new(completed)).ok()?;
    Some(Stated { doc: changed, given })
}

impl Stated {
    /// Puts the root the caller had in `read_back`, the model the file reads as, in place of the file's completed one.
    pub(super) fn give_back(&self, read_back: &mut Document) {
        if let Some(at) = read_back.styles.iter().position(|style| style.id == 0) {
            // `at` is a place the list holds, so `set` cannot refuse it.
            let _ = read_back.styles.set(at, Arc::clone(&self.given));
        }
    }
}
