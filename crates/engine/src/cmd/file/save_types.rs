//! **Which types Save As may write** for the drawing a session holds: the save-path table ([`super::save_path`]) asked without saving.
//!
//! The window offers the person a list of file types in its Save dialog. The list must not be a table of its own, because the engine is what refuses a
//! save across formats (and lifts the refusal, one writer at a time): so the window asks the engine, for each type it might offer, and keeps what is
//! allowed. The answer is the one `file.save_as` would give for a name of that type, before anything is made: a type the table sends to [`SavePath::Refused`]
//! is not offered. A drawing that came from a flat file is offered what any other drawing is (it is written from the model, as a `.diacraft` file, an `.odg`
//! package or a VSDX type), and `.fodg`, which is no type of the table, is offered to none.

use super::{OpenFile, SavePath, Target, save_path};

/// Whether `file.save_as` takes a name that ends in `extension` (in any case, without the dot) for a drawing whose working file is `source` (`None`: a new drawing, or one made from a
/// template). The table's own answer, from the working file's format, whether the drawing is a stencil, and what the name says to write.
pub fn save_as_allows(source: Option<&OpenFile>, extension: &str) -> bool {
    let working = source.and_then(OpenFile::format);
    let stencil = source.is_some_and(OpenFile::is_stencil);
    !matches!(save_path(working, stencil, Target::of(Some(extension))), SavePath::Refused(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::FileFormat;

    /// The extensions a Save As may be asked for, whatever the drawing.
    const ALL: [&str; 12] =
        ["diacraft", "diacraft-stencil", "diacraft-template", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"];

    fn allowed(working: Option<FileFormat>, stencil: bool) -> Vec<&'static str> {
        ALL.into_iter().filter(|e| !matches!(save_path(working, stencil, Target::of(Some(e))), SavePath::Refused(_))).collect()
    }

    /// The query is the table: each working file allows exactly what `save_path` does not refuse, and the table's rows are the ones the design gives.
    #[test]
    fn a_type_is_allowed_exactly_when_the_save_path_table_does_not_refuse_it() {
        let own = ["diacraft", "diacraft-stencil", "diacraft-template"];
        let vsdx = ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"];
        // A new drawing: DiaCraft's own types, the VSDX types and the OpenDocument drawing and template (the `.odg` model writer is wired); no `.fodg` (never offered).
        let new: Vec<&str> = own.iter().chain(vsdx.iter()).chain(["odg", "otg"].iter()).copied().collect();
        assert_eq!(allowed(None, false), new);
        // An opened VSDX file: its own and `.diacraft`, and an OpenDocument type (a save across the formats).
        assert_eq!(allowed(Some(FileFormat::Vsdx), false), new);
        // A `.diacraft` drawing: the VSDX types are written from the model, and so are the `.odg` and `.otg`.
        assert_eq!(allowed(Some(FileFormat::Diacraft), false), new);
        // An opened `.odg`: itself, as a template, `.diacraft`, and a VSDX type (a save across the formats).
        assert_eq!(allowed(Some(FileFormat::Odg), false), new);
        // A stencil is no OpenDocument file, whatever its working file is.
        for working in [None, Some(FileFormat::Vsdx), Some(FileFormat::Diacraft), Some(FileFormat::Odg)] {
            let kinds = allowed(working, true);
            assert!(!kinds.contains(&"odg") && !kinds.contains(&"otg"), "{working:?}");
        }
    }

    /// A name with an extension in another case, and one that is no type at all.
    #[test]
    fn the_case_of_the_extension_does_not_matter_and_a_name_with_no_type_is_not_allowed() {
        assert!(save_as_allows(None, "DiaCraft"));
        assert!(save_as_allows(None, "VSDX"));
        assert!(!save_as_allows(None, "txt"));
        assert!(!save_as_allows(None, ""));
        assert!(!save_as_allows(None, "fodg"), "a flat drawing is never offered");
        assert!(!save_as_allows(None, "FODG"), "in any case");
    }
}
