//! The open of a `.diacraft` file: the read, the themes, and the words of a refusal.
//!
//! **The memory a read may use is the process's**, not a session's: `file.open`, a template path and `stencil.open` read under a reservation of what is left of the
//! process's budget beside everything the process holds (the open drawing and its file, the open stencils, every other session, the reads that are running, [`limits_of`]),
//! never more than [`READ_BUDGET`], so that two sessions and a read that is running share the one budget and never each take all of it. What the read may take of that is shared by the pools of the codec's budget **before anything is allocated** (`diacraft_native::limits`), and a file whose archive states
//! more than they hold is refused from the sizes the archive states, before a blob or a part is inflated.
//!
//! **A theme's derived data is not in the file** (the design's 1.5): its colours, fonts and scheme numbers are read from its bytes here, as the `.odg` open reads them, by the one reader of
//! a theme part. The content stamp, the ledger and the version are the file's own and are read by the package.

use diacraft_doc::Document;
use diacraft_native::{Limits, NativeError, Opened};
use diacraft_sheetxml::model::{CodecError, held_bytes_of};

use crate::error::OpenProblem;
use crate::memory::Reservation;
use crate::{HeldMemory, Session};

/// What reading a `.diacraft` file made.
pub struct Read {
    /// The model, with every theme read from its bytes.
    pub doc: Document,
    /// What the package learned of itself: its kind and version, the ledger and the content stamp, what a write copies from, and what was lost.
    pub opened: Opened,
    /// In words, what was lost or not understood (a count and no name from the file) and the themes that could not be read whole.
    pub warnings: Vec<String>,
    /// What the model holds, by the count a session keeps ([`held_bytes_of`]): what a read of another file must leave room for beside the file's bytes.
    pub model_bytes: u64,
}

/// Reads `bytes` as a `.diacraft` file within `limits`, **the one way a `.diacraft` file is opened** (the command line's export reads one with it too): the package's read, then the
/// themes it carries read from their bytes. A refusal is the package's, in words that name nothing from the file.
pub fn read_native(bytes: &[u8], limits: &Limits) -> Result<Read, NativeError> {
    let (mut doc, opened) = diacraft_native::read(bytes, limits)?;
    let mut warnings: Vec<String> = opened.notes.iter().map(ToString::to_string).collect();
    for note in crate::odgfile::read_themes(&mut doc) {
        warnings.extend(crate::odgfile::theme_note_words(&note).into_iter().map(|(_, words)| words));
    }
    // A loop of containers the file holds is broken, and said first.
    warnings.splice(0..0, crate::draw::container::cycles::untangle(&mut doc));
    let model_bytes = held_bytes_of([&doc], u64::MAX);
    Ok(Read { doc, opened, warnings, model_bytes })
}

/// The most a `.diacraft` read may use at once in this process, **the budget it is held to and the one its refusal names**: the process's budget, of which every read reserves what is left
/// ([`crate::memory`]: `diacraft_vsdx::MAX_MEMORY`, the VSDX reader's figure), and never more than the package's own ([`diacraft_native::MAX_MEMORY`]), so the smaller of the two. The two
/// are different figures, set apart and moved apart (made the package's the larger), and a person is told the one that was held against.
pub(crate) const READ_BUDGET: u64 =
    if diacraft_native::MAX_MEMORY < diacraft_vsdx::MAX_MEMORY { diacraft_native::MAX_MEMORY } else { diacraft_vsdx::MAX_MEMORY };

/// The limits of a `.diacraft` read made under `reservation` (**the read holds what is left of the process's budget while it runs**, as a VSDX read does): the memory it may
/// use is what was reserved, and never more than [`READ_BUDGET`]. The reservation counts the file the read holds, as the package's own pools do.
pub(crate) fn limits_of(reservation: &Reservation) -> Limits {
    // The threads are the engine's choice (`open_threads`) and no figure of the memory: `limits_with` is the integer code that a proof holds to the budget.
    Limits { threads: crate::open_threads::open_threads(), ..limits_with(reservation.bytes()) }
}

/// The limits of a `.diacraft` read given `reserved` bytes: the package's defaults, with the memory what was reserved and never more than [`READ_BUDGET`].
pub(crate) fn limits_with(reserved: u64) -> Limits {
    Limits { max_memory: reserved.min(READ_BUDGET), ..Limits::default() }
}

/// What the process held when `reservation` was made, for the words of a refusal: the session's own drawing and stencils, other sessions and the reads that were running, **and not this read's
/// own**.
pub(crate) fn held_by(s: &Session, reservation: &Reservation) -> HeldMemory {
    HeldMemory { process: reservation.beside(), ..s.held_memory() }
}

/// Whether the refusal is for want of memory: the package's own, or the codec's pools of the model and of the blobs, which shrink with what the process leaves.
fn is_memory(e: &NativeError) -> bool {
    match e {
        NativeError::TooLarge(words) => words.contains("memory"),
        NativeError::Codec(CodecError::Limit { what, .. }) => {
            what.starts_with("bytes of the model in memory") || what.starts_with("bytes of pictures and parts")
        }
        _ => false,
    }
}

/// The kind of trouble a refusal by the `.diacraft` reader is, for a read made beside `held` bytes that the process holds: a refusal for memory when something is held is that there is
/// not enough memory beside it (the file may be one that opens when nothing else is open), and not that the file is too big.
pub(crate) fn problem(e: &NativeError, held: &HeldMemory) -> OpenProblem {
    if is_memory(e) && held.total() > 0 {
        return OpenProblem::OutOfMemory;
    }
    match e {
        NativeError::NotNative => OpenProblem::NotAPackage,
        // A file of a later major version is a drawing this build does not know how to read: not damaged, and not too big. (The window's words for it are.)
        NativeError::NewerMajor { .. } | NativeError::RequiredRole(_) => OpenProblem::NotAPackage,
        NativeError::Damaged(_) | NativeError::Unwritable(_) => OpenProblem::Damaged,
        NativeError::TooLarge(_) => OpenProblem::TooBig,
        NativeError::Container(c) => match c.class() {
            "not_zip" => OpenProblem::NotAPackage,
            "zip_ratio_exceeded" | "part_too_large" | "package_too_large" | "too_many_parts" | "xml_too_complex" | "out_of_memory" => {
                OpenProblem::TooBig
            }
            _ => OpenProblem::Damaged,
        },
        NativeError::Codec(c) => match c.class() {
            "model_too_complex" => OpenProblem::TooBig,
            _ => OpenProblem::Damaged,
        },
    }
}

/// What a person is told when the file is not a drawing we open: by what kind of trouble it is, and then the reader's own words, which hold numbers, class words and nothing from the
/// file. A refusal for memory beside what is held says what is held and what to do ([`HeldMemory::words_of`]), **or, when a file is being read at this moment and holds the budget (`reads` bytes of
/// it), to wait for that to finish**, as the refusal of a VSDX read says. The budget either names is the one the read was held to, [`READ_BUDGET`].
pub(crate) fn refused(e: &NativeError, held: &HeldMemory, reads: u64) -> String {
    if let NativeError::NewerMajor { .. } = e {
        // The words of the format (the spec's 2.4): the version is named, and it is no damage.
        return crate::shown(&format!("{e}: this DiaCraft cannot open it, and a newer one can ({})", e.class()), 300);
    }
    if let NativeError::RequiredRole(_) = e {
        // A drawing file, that says it needs a part this DiaCraft does not know: said as it is, and not as a file that is no drawing.
        return crate::shown(&format!("{e}: this DiaCraft cannot open it ({})", e.class()), 300);
    }
    if is_memory(e) && held.total() > 0 {
        let head = crate::shown(&format!("not enough memory beside what is open ({}): {e}", e.class()), 300);
        return if reads > 0 {
            format!(
                "{head}: another file is being read, and holds {reads} of the {READ_BUDGET} bytes that reading a file may use at once until it has been read: wait for it to finish, and open this file again"
            )
        } else {
            format!("{head}: {}", held.words_of(READ_BUDGET))
        };
    }
    let how = match problem(e, held) {
        OpenProblem::NotAPackage => "this is not a drawing file",
        OpenProblem::TooBig => "this file is too big to open safely",
        _ => "this file is damaged",
    };
    crate::shown(&format!("{how} ({}): {e}", e.class()), 300)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_container::ContainerError;

    use super::*;

    fn nothing() -> HeldMemory {
        HeldMemory::default()
    }

    fn something() -> HeldMemory {
        HeldMemory { drawing: 700, stencils: 30, process: 900 }
    }

    #[test]
    fn each_refusal_is_a_kind_of_trouble_and_a_sentence_that_holds_nothing_of_the_file() {
        let cases: Vec<(NativeError, OpenProblem)> = vec![
            (NativeError::NotNative, OpenProblem::NotAPackage),
            (NativeError::NewerMajor { major: 2, minor: 0 }, OpenProblem::NotAPackage),
            (NativeError::RequiredRole("x".into()), OpenProblem::NotAPackage),
            (NativeError::Damaged("entry_missing"), OpenProblem::Damaged),
            (NativeError::TooLarge("the file is larger than a DiaCraft file may be"), OpenProblem::TooBig),
            (NativeError::Container(ContainerError::NotZip), OpenProblem::NotAPackage),
            (NativeError::Container(ContainerError::ZipCorrupt { detail: "x".into() }), OpenProblem::Damaged),
            (NativeError::Container(ContainerError::TooManyParts { limit: 5 }), OpenProblem::TooBig),
            (NativeError::Codec(CodecError::Limit { what: "pages", max: 1_000 }), OpenProblem::TooBig),
            (NativeError::Codec(CodecError::Malformed { part: "page".into(), why: "x" }), OpenProblem::Damaged),
        ];
        for (error, want) in cases {
            assert_eq!(problem(&error, &nothing()), want, "{error}");
            let words = refused(&error, &nothing(), 0);
            assert!(words.contains(error.class()), "the class is in the words: {words}");
            assert!(!words.contains('\n'), "{words}");
        }
    }

    #[test]
    fn a_file_that_requires_a_part_is_a_drawing_that_this_build_cannot_open_and_not_a_file_that_is_no_drawing() {
        let words = refused(&NativeError::RequiredRole("layers".into()), &nothing(), 0);
        assert!(words.contains("\"layers\"") && words.ends_with("this DiaCraft cannot open it (required_role)"), "{words}");
        assert!(!words.contains("not a drawing file") && !words.contains("damaged"), "{words}");
        // A file that is no package is the one that is called what it is.
        assert!(refused(&NativeError::NotNative, &nothing(), 0).starts_with("this is not a drawing file"));
    }

    #[test]
    fn a_newer_version_is_named_and_is_not_called_damage() {
        let words = refused(&NativeError::NewerMajor { major: 2, minor: 0 }, &nothing(), 0);
        assert!(words.contains("made by a newer DiaCraft (format 2.0)") && !words.contains("damaged"), "{words}");
    }

    #[test]
    fn a_refusal_for_memory_says_what_is_held_when_something_is_and_is_otherwise_as_it_always_was() {
        let memory = NativeError::TooLarge(
            "the file needs more memory to open than is free: the drawing and the other files that are open leave no room for it",
        );
        let model = NativeError::Codec(CodecError::Limit { what: "bytes of the model in memory", max: 1_000 });
        let blobs = NativeError::Codec(CodecError::Limit { what: "bytes of pictures and parts in the drawing", max: 1_000 });
        let items = NativeError::Codec(CodecError::Limit { what: "items in the drawing", max: 1_000 });
        for error in [&memory, &model, &blobs] {
            // Nothing held: the file is too big, as it always was.
            assert_eq!(problem(error, &nothing()), OpenProblem::TooBig, "{error}");
            assert!(refused(error, &nothing(), 0).starts_with("this file is too big to open safely"));
            // Something held: there is not enough memory beside it, and what is held is said, in the engine's numbers.
            assert_eq!(problem(error, &something()), OpenProblem::OutOfMemory, "{error}");
            let text = refused(error, &something(), 0);
            assert!(text.starts_with("not enough memory beside what is open") && text.ends_with(&something().words_of(READ_BUDGET)), "{text}");
            // The budget named is the one a `.diacraft` read is held to ([`READ_BUDGET`]), by its value, and not the other of the two figures it is the smaller of (the
            // process's budget, which is the VSDX reader's, and the package's own): they differ, so the words tell them apart.
            let other = diacraft_native::MAX_MEMORY.max(diacraft_vsdx::MAX_MEMORY);
            assert!(text.contains(&format!("of the {READ_BUDGET} bytes")) && !text.contains(&other.to_string()), "{text}");
        }
        // A refusal that is not for memory is not told as one, whatever is held.
        assert_eq!(problem(&items, &something()), OpenProblem::TooBig);
        assert!(refused(&items, &something(), 0).starts_with("this file is too big to open safely"));
        let elsewhere = HeldMemory { drawing: 0, stencils: 0, process: 5 };
        assert_eq!(problem(&memory, &elsewhere), OpenProblem::OutOfMemory, "what another session holds is held");
    }

    #[test]
    fn the_budget_a_refusal_names_is_the_most_a_read_is_held_to_and_is_never_more_than_the_process_or_the_package_allows() {
        // The two figures differ (the package's is the larger), so a sentence that names one is not the sentence that names the other.
        assert_ne!(diacraft_native::MAX_MEMORY, diacraft_vsdx::MAX_MEMORY);
        // The budget is the smaller of the two: never more than either, and one of them.
        assert_eq!(READ_BUDGET, diacraft_native::MAX_MEMORY.min(diacraft_vsdx::MAX_MEMORY));
        // A read that reserved all of the process's budget (nothing else is held) is held to the figure the words name, and one given more than that could ever be is too.
        assert_eq!(limits_with(diacraft_vsdx::MAX_MEMORY).max_memory, READ_BUDGET);
        assert_eq!(limits_with(u64::MAX).max_memory, READ_BUDGET);
        // Less than that is what was reserved, and every other limit is the package's own.
        assert_eq!(limits_with(1_000), Limits { max_memory: 1_000, ..Limits::default() });
        assert_eq!(limits_with(0).max_memory, 0);
        // A refusal beside a read that is running names the same figure as one beside what is held.
        let memory = NativeError::TooLarge(
            "the file needs more memory to open than is free: the drawing and the other files that are open leave no room for it",
        );
        let waiting = refused(&memory, &something(), 1_234);
        assert!(waiting.contains(&format!("holds 1234 of the {READ_BUDGET} bytes that reading a file may use at once")), "{waiting}");
        let other = diacraft_native::MAX_MEMORY.max(diacraft_vsdx::MAX_MEMORY);
        assert!(!waiting.contains(&other.to_string()), "{waiting}");
    }
}
