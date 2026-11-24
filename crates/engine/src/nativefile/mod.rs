//! A drawing that came from a `.diacraft` file, and the save of one: what the engine keeps of it, the content stamp and the open.
//!
//! **The format is `docs/diacraft-format.md`**, and the package is `diacraft_native`. The engine's part is to keep what a later save needs and to hand
//! it over:
//!
//! * [`NativeState`], kept by the [`OpenFile`](crate::file::OpenFile) beside the file's bytes: the kind of the file and what the next write copies from the package as it was read or
//!   written (`diacraft_native::Decoded`: the very `Arc` of each page and master, the entries that held them, and the blobs). **A save that changed nothing writes the file it read**, and one
//!   that changed a page rewrites that page's part only. Cloning it copies a pointer: the session clones its state for every command.
//! * [`open`]: the open: the memory the read may use is what the process leaves of the budget, held for it by a reservation while it runs and never more
//!   than `open::READ_BUDGET`, the smaller of the process's budget and the package's own; the package is read by `diacraft_native::read`, the themes are
//!   derived from their bytes (the codec stores no derived data: `odgfile::read_themes`, the one way a theme's colours and fonts are read), and the refusals are in words.
//! * [`save`]: the save: the identifier ledger the session keeps, the content stamp, the picture of the first page, and the writer, held to the memory the process leaves.
//!
//! **The content stamp** (the design's 4.4): the fingerprint of the model's canonical encoding as it was when the drawing was read from a foreign file. A model writer (the VSDX one, from
//!) writes the bookkeeping the foreign file had while the content's stamp is still it, and the bookkeeping of an edited save otherwise. The decision is
//! [`content_unchanged`]: **it compares fingerprints and never asks the dirty flag**, which does not survive a reopen (a drawing edited after it was read from a VSDX file and saved as
//! `.diacraft` is clean in the session that opens that, and its content is not what the VSDX file held).
//!
//! **The stamp is made when it is needed, not at the open and not at a save** (the pass was a third of the open's time, and a save of a VSDX file that nothing asks the stamp of
//! would pay for it). A named VSDX file arms it as *pending* ([`ContentStamp`](crate::file::ContentStamp)), which **keeps the document the drawing was read as** (a clone of the `Arc` taken
//! at the open: no pass, no copy; the session counts it with the documents it holds). The first thing that needs the stamp makes it, once, **from that document**, and lets go of it: a Save
//! As to `.diacraft`, or the decision of a model writer once the drawing is not that document (`Session::content_unchanged`). No save fingerprints, and the baseline a save replaces is no
//! longer the thing the stamp depends on. While it is pending, the drawing is unchanged when it is the very document the stamp keeps, and no pass is made ([`stamp_of`], one pass over the
//! model's encoding, one part at a time, only when it fits beside what the process holds at that moment: otherwise there is none, which reads as edited). **A `.diacraft` file keeps its
//! stamp in `state.xml`, and a save carries it: it is never taken again.**

pub(crate) mod open;
pub(crate) mod save;

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_doc::stamp::Stamp;
use diacraft_native::Decoded;
use diacraft_sheetxml::model::cost::{BASE_BYTES, TREE_ITEM_BYTES};
use diacraft_sheetxml::model::{ADDRESS_SPACE_BYTES, Budget, SECTION_STREAM_BYTES, StampCost, content_stamp_counted, open_limits};

pub use diacraft_native::{Kind, Limits, sniff};
pub use open::{Read, read_native};

/// What the engine keeps of a drawing read from a `.diacraft` file or saved as one: its kind, and what a later write copies from the package. Cloning it copies a pointer.
#[derive(Clone, Debug)]
pub struct NativeState {
    kind: Kind,
    decoded: Arc<Decoded>,
}

impl NativeState {
    /// The state of a package just read or written.
    pub(crate) fn new(kind: Kind, decoded: Decoded) -> NativeState {
        NativeState { kind, decoded: Arc::new(decoded) }
    }

    /// The kind of the file: a drawing, a stencil or a template.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The extension of the file's type, without its dot: `diacraft`, `diacraft-stencil` or `diacraft-template`.
    pub fn extension(&self) -> &'static str {
        variant_word(self.kind)
    }

    /// What the next write copies from.
    pub(crate) fn decoded(&self) -> &Decoded {
        &self.decoded
    }
}

/// The word `file.open`, `file.save` and `file.status` say for a `.diacraft` file of `kind`: its extension without the dot.
pub const fn variant_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Drawing => "diacraft",
        Kind::Stencil => "diacraft-stencil",
        Kind::Template => "diacraft-template",
    }
}

/// The kind whose extension is `extension` (without the dot, in any case), or `None` when it is not one of the three.
pub fn kind_of_extension(extension: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|kind| variant_word(*kind).eq_ignore_ascii_case(extension))
}

/// What a person is told when a `.diacraft` file is called by another type's name: the bytes decide, and the name is wrong. `None` when the name is the kind's.
pub fn extension_note(kind: Kind, extension: &str) -> Option<String> {
    if kind_of_extension(extension) == Some(kind) {
        return None;
    }
    Some(format!(
        "the file is called .{} but it is a DiaCraft {}, which is opened as one (its usual extension is {})",
        shown_extension(extension),
        kind.word(),
        kind.extension()
    ))
}

/// An extension as a message may repeat it: short, and with nothing a terminal would act on.
fn shown_extension(extension: &str) -> String {
    crate::shown(extension, 16)
}

/// **The decision a model writer takes** (4.4): whether the content is what it was when it was read from a foreign file. `Some(true)` when the drawing has the stamp of its content as
/// read and the fingerprint of its content now is that stamp: the bookkeeping is written as it was read. `Some(false)` when they differ: an edited save's. `None` when there is nothing to
/// compare, a drawing that never came from a foreign file (the bookkeeping of a new file) or whose content could not be fingerprinted (which is not known to be unchanged).
///
/// It reads the two stamps and **never the dirty flag**.
pub fn content_unchanged(stored: Option<Stamp>, now: Option<Stamp>) -> Option<bool> {
    Some(stored? == now?)
}

/// What a pass that fingerprints the model holds, beside the model: the tree of one part at the most items the XML reader takes, and the tables of streams the check keeps.
const STAMP_PEAK_BYTES: u64 = diacraft_container::limits::MAX_XML_NODES as u64 * TREE_ITEM_BYTES + SECTION_STREAM_BYTES + BASE_BYTES;

/// Whether a pass that fingerprints the model fits the address space beside `held` bytes that the process holds (a twentieth of the address space is spared, as the codec's own arithmetic
/// spares it). A pass that does not fit is not made: the drawing has no stamp, and a model writer then writes a new file's bookkeeping.
pub(crate) fn stamp_fits(held: u64) -> bool {
    held.saturating_add(STAMP_PEAK_BYTES) <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20
}

/// The content stamp of `doc`, and what it cost, **when a pass fits beside `held` bytes** the process holds and the codec takes the model within the budget of an open; `None` otherwise (a
/// drawing the codec cannot encode has no stamp, and is not refused for it: its open does not depend on the stamp).
pub(crate) fn stamp_of(doc: &Document, held: u64) -> Option<(Stamp, StampCost)> {
    if !stamp_fits(held) {
        return None;
    }
    let mut budget = Budget::for_open(&open_limits());
    content_stamp_counted(doc, &mut budget).ok()
}

/// What the passes that fingerprint a model have cost a session: how many were made, the parts they made, and the items of the parts' trees, summed. The tests that count the work read it
/// (`Session::stamp_work`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StampWork {
    /// Passes made.
    pub passes: u64,
    /// Parts made: the document, each page and each master, for each pass.
    pub parts: u64,
    /// Items of those parts' trees.
    pub items: u64,
}

impl StampWork {
    /// This work and one pass that cost `cost`.
    #[must_use]
    pub fn with(self, cost: StampCost) -> StampWork {
        StampWork {
            passes: self.passes.saturating_add(1),
            parts: self.parts.saturating_add(cost.parts),
            items: self.items.saturating_add(cost.items),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kinds_and_their_extensions_are_one_table() {
        for kind in Kind::ALL {
            assert_eq!(kind.extension(), format!(".{}", variant_word(kind)));
            assert_eq!(kind_of_extension(variant_word(kind)), Some(kind));
            assert_eq!(kind_of_extension(&variant_word(kind).to_ascii_uppercase()), Some(kind), "in any case");
        }
        assert_eq!(kind_of_extension("vsdx"), None);
        assert_eq!(kind_of_extension(""), None);
        assert_eq!(kind_of_extension("diacraft-stencils"), None);
    }

    #[test]
    fn a_name_that_is_the_kinds_says_nothing_and_another_is_told() {
        assert_eq!(extension_note(Kind::Drawing, "diacraft"), None);
        assert_eq!(extension_note(Kind::Stencil, "DIACRAFT-STENCIL"), None);
        let note = extension_note(Kind::Drawing, "vsdx").unwrap();
        assert!(note.contains(".vsdx") && note.contains("DiaCraft drawing") && note.contains(".diacraft"), "{note}");
        // A stencil called a drawing's name is a stencil, which a name that is another kind's does not change.
        assert!(extension_note(Kind::Stencil, "diacraft").unwrap().contains("stencil"));
        // Nothing from the name that a terminal would act on.
        let hostile = extension_note(Kind::Drawing, "\u{1b}[31mx").unwrap();
        assert!(!hostile.contains('\u{1b}'), "{hostile:?}");
    }

    #[test]
    fn the_decision_compares_two_stamps_and_has_nothing_else_to_look_at() {
        let (a, b) = (Stamp(7), Stamp(8));
        assert_eq!(content_unchanged(Some(a), Some(a)), Some(true));
        assert_eq!(content_unchanged(Some(a), Some(b)), Some(false));
        assert_eq!(content_unchanged(None, Some(a)), None);
        assert_eq!(content_unchanged(Some(a), None), None);
        assert_eq!(content_unchanged(None, None), None);
    }

    #[test]
    fn a_pass_that_does_not_fit_beside_what_is_held_is_not_made() {
        // The figures, as literals and not as the constants they come from, so that a change of one of them is seen: a tree of the most items the XML reader takes (6,000,000 at 160 bytes
        // each, 960,000,000), the tables of the check (64 MiB, 67,108,864) and what a process has (256 MiB, 268,435,456) are what a pass holds beside the model.
        assert_eq!(STAMP_PEAK_BYTES, 960_000_000 + 67_108_864 + 268_435_456);
        assert_eq!(STAMP_PEAK_BYTES, 1_295_544_320);
        // The address space is 3,072,000,000 bytes (a test's `ulimit -v 3000000`), less a twentieth to spare, 2,918,400,000: what is left of it beside a pass is 1,622,855,680.
        assert_eq!(ADDRESS_SPACE_BYTES, 3_072_000_000);
        assert!(stamp_fits(0));
        assert!(stamp_fits(1 << 30));
        assert!(stamp_fits(1_622_855_680), "as much as fits is enough");
        assert!(!stamp_fits(1_622_855_681), "one more is not");
        assert!(!stamp_fits(2_000_000_000));
        assert!(!stamp_fits(u64::MAX), "a count that cannot be held does not wrap");
        let doc = Document::new_blank();
        assert!(stamp_of(&doc, 1_622_855_681).is_none());
        let (stamp, cost) = stamp_of(&doc, 0).unwrap();
        assert_eq!(cost.parts, 1 + doc.pages.len() as u64 + doc.masters.len() as u64);
        assert_eq!(stamp_of(&doc, 1_622_855_680).map(|(s, _)| s), Some(stamp), "the stamp is a function of the content, and the same at the edge");
    }

    #[test]
    fn the_work_of_passes_adds_up() {
        let none = StampWork::default();
        let one = none.with(StampCost { parts: 3, items: 40, largest_part_items: 30 });
        assert_eq!(one, StampWork { passes: 1, parts: 3, items: 40 });
        assert_eq!(one.with(StampCost { parts: 2, items: 5, largest_part_items: 4 }), StampWork { passes: 2, parts: 5, items: 45 });
        let big = StampWork { passes: u64::MAX, parts: u64::MAX, items: u64::MAX }.with(StampCost { parts: 1, items: 1, largest_part_items: 1 });
        assert_eq!(big, StampWork { passes: u64::MAX, parts: u64::MAX, items: u64::MAX });
    }
}
