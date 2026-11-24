//! How a template names the stencils it opens (ours; moved it into DiaCraft's own part).
//!
//! The list lives in **DiaCraft's solution part** (`Document::diacraft`, the same part the Snap & Glue options live in),
//! in a child `stencils` of the `Data` element whose attribute `open` is a list of **built-in stencil ids** (`stencil.list`: `basic`,
//! `flowchart`) separated by `;`, in the order the Shapes window lists them; the first is the one it shows. It is window state, so it is not in the
//! ShapeSheet, where the first version of this rule put it (a user cell of the document sheet): another application would show it, copy it into every
//! drawing and never keep it up to date. The part is one that application leaves alone and DiaCraft rewrites (data of a version we do not read is
//! kept and not interpreted).
//!
//! * A template with **no such child** opens the stencils a new drawing has (every built-in stencil). That is every template another application made,
//!   whose stencil windows name files on its owner's machine and are not followed, and every template DiaCraft saved with
//!   those stencils open.
//! * A template with the child opens **exactly** the stencils it names: an empty list opens none. A list that names stencils and none that can be opened
//!   (a template made for stencils this app does not have) opens the stencils a new drawing has, and not an empty Shapes window (`cmd/file.rs`).
//! * **A name is never a path.** An entry that is not the id of a built-in stencil is counted as ignored and nothing is read for it: a template is a
//!   file somebody sent, and opening what it names would let it choose which of the reader's files DiaCraft reads. A stencil file is opened by
//!   `stencil.open`, by the person.
//! * Bounded: only the first 4 KiB of the text and the first [`MAX_NAMES`] entries are looked at, whatever the attribute holds.
//! * **Written only when it says something**: a list that is the stencils a new drawing has is no child (and a part with nothing else in it is no part),
//!   so a template with the default stencils is the file it would have been without this rule.

use diacraft_doc::Document;

use crate::stencil;

/// The child of DiaCraft's data that names a template's stencils.
pub const STENCILS_CHILD: &str = "stencils";

/// The attribute of that child that holds the list.
const OPEN_ATTR: &str = "open";

/// The most entries of the list that are looked at.
pub const MAX_NAMES: usize = 64;

/// The most bytes of the attribute's text that are looked at: no list of built-in stencil ids is longer.
const MAX_SCAN_BYTES: usize = 4_096;

/// What a template's list names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StencilNames {
    /// The built-in stencils to open, in the order named, each once.
    pub ids: Vec<&'static str>,
    /// How many entries named something else (a path, a stencil that does not exist, a repeat): counted and not followed.
    pub ignored: usize,
}

impl StencilNames {
    /// The built-in stencils a template with this list opens: the ones it names, or, when it names stencils and none of them can be opened (a template
    /// made for stencils this app does not have), the stencils a new drawing has and not an empty Shapes window. An empty list opens none.
    pub fn opens(&self) -> Vec<&'static str> {
        if self.ids.is_empty() && self.ignored > 0 { default_ids() } else { self.ids.clone() }
    }
}

/// The stencils a new drawing has open: every built-in stencil, in the order of the catalogue.
fn default_ids() -> Vec<&'static str> {
    stencil::stencils().iter().map(|s| s.id).collect()
}

/// The built-in stencils that are open in a drawing started from `doc` as a template: what its list names (`StencilNames::opens`), or the stencils a new
/// drawing has when it has none.
pub fn stencils_opened_by(doc: &Document) -> Vec<&'static str> {
    stencil_names(doc).map_or_else(default_ids, |names| names.opens())
}

/// The stencils the document names, or `None` if it has no list (see the module documentation): no DiaCraft part, one of a version this build does not
/// read, or no `stencils` child with an `open` attribute in it.
pub fn stencil_names(doc: &Document) -> Option<StencilNames> {
    let text = doc.diacraft_child(STENCILS_CHILD)?.attrs.get(OPEN_ATTR)?;
    Some(parse(text))
}

/// The entries of `text` as [`StencilNames`] says (bounded).
pub(super) fn parse(text: &str) -> StencilNames {
    // The cut is made at a character boundary: a multi-byte letter across it is dropped, not split.
    let end = (0..=text.len().min(MAX_SCAN_BYTES)).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
    let mut names = StencilNames::default();
    for entry in text.get(..end).unwrap_or_default().split(';').take(MAX_NAMES) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        match stencil::find_stencil(entry) {
            Some(found) if !names.ids.contains(&found.id) => names.ids.push(found.id),
            _ => names.ignored += 1,
        }
    }
    names
}

/// Makes the document name `ids` (built-in stencil ids; a name that is not one is not written): the list goes into the `stencils` child of DiaCraft's
/// part, and **a list that is the stencils a new drawing has takes the child out**, and the part with it when nothing else is in it. Used to make a
/// template, to save one, and by the tests that make hostile ones by hand (with `Document::set_diacraft_child`).
///
/// → `false`, and nothing changed, when the document's DiaCraft data is of a version this build does not read: it is kept as it is and not interpreted.
pub fn set_stencil_names(doc: &mut Document, ids: &[&str]) -> bool {
    let known: Vec<&'static str> = ids.iter().filter_map(|id| stencil::find_stencil(id).map(|s| s.id)).collect();
    if known == default_ids() {
        return doc.set_diacraft_child(STENCILS_CHILD, None);
    }
    doc.set_diacraft_child(STENCILS_CHILD, Some(&[(OPEN_ATTR, known.join(";").as_str())]))
}
