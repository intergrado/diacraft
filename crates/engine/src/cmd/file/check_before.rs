//! `file.check_compatibility` (plan PL8): what a save would carry and lose, answered **with no file written and nothing in the session changed**.
//!
//! A save is two halves (`Made` in `cmd/file.rs`): the file made in memory (the target judged, the writer run, the file read back and compared, `carried` and `checked` made of what was read back),
//! and the file committed (the disk write through the crash-safe replacement, then the session told the drawing is saved). This command is the first half alone, so that **what it answers is what
//! the save then answers**, by the same code, and **it fails as the save would** (a macro project the type cannot hold, a changed file on the disk, a link, a `.fodg`, a stencil to `.odg`, a refusal
//! of a writer: the same kind of error in the same words). It is a query: no undo step, no journal entry, and the drawing is not marked as changed.
//!
//! **The cost** is the save's less the disk: one write in memory and, for a model writer, one read back and one comparison, within the save's own memory gate (the same functions run).
//! A window that asks before a Save As through a model writer therefore costs that save twice; it is the caller's to ask only where it matters.

use std::path::Path;

use serde_json::Value;

use super::{flag, make_save};
use crate::Result;
use crate::Session;
use crate::across::Options;
use crate::cmd::{CommandSpec, has_document, required_text};
use crate::error::{bad, failed};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "file.check_compatibility",
        "Check Compatibility",
        ["File", "Info"],
        None,
        "path: where the drawing would be written, as file.save_as takes it (its extension says which kind of file it is), or left out to mean the drawing's own file, as file.save saves it; \
         drop_macros: true to leave the macro project out of the file, as file.save_as takes it (false when left out, and only with a path); \
         drop_objects: true to leave the embedded objects of the other format out of the file, as file.save_as takes it (false when left out, and only with a path); \
         force: true to ask as a save with force would, which writes over the drawing's own file even if something else has changed it on the disk since the drawing was opened or last saved (false when left out)",
        has_document,
        check_compatibility
    )]
}

/// What a save of the drawing as `file.save_as` would write (or as `file.save` would, with no `path`) would carry and lose, **made in memory and not written**: the file is made by the writer the save
/// would use and read back and compared with the drawing, as the save does, and then let go of. The target is judged as the save judges it, and a save that would be refused (or refuse to
/// replace a file that changed on the disk, without `force`) is refused here with the same error in the same words. The drawing, its history, `file.status`, the working file, the dirty flag, the
/// identifier ledger and every baseline are as they were, and no file is made or changed. → `{writer, format, variant, carried, checked, bytes}`:
/// * `writer` is the writer that made the file: `diacraft` (DiaCraft's own format), `vsdx_model` and `odg_model` (the file written from the drawing alone: a `.diacraft` drawing, a new one, or a save
///   across the VSDX and OpenDocument formats), and `vsdx_splice` and `odg_splice` (the edit spliced into the file the drawing was opened from);
/// * `format` is the format of the file (`diacraft`, `vsdx` or `odg`) and `variant` its type (the extension without the dot: `vsdx`, `vsdm`, `odg`, `otg`, `diacraft`...);
/// * `carried` is the field the save's result holds (what the file carries of the other format's data, macro projects and objects, and the signatures that will no longer match);
/// * `checked` is the field the save's result holds: the check of the file read back (`{done, classes, ..}`), or `null` where the save is not checked (a splice into the drawing's own format);
/// * `bytes` is the size of the file the save would write.
fn check_compatibility(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.check_compatibility";
    let force = flag(p, "force", ID)?;
    let (drop_macros, drop_objects) = (flag(p, "drop_macros", ID)?, flag(p, "drop_objects", ID)?);
    let (path, by_extension, options) = match p.get("path") {
        None | Some(Value::Null) => {
            // As `file.save`: the drawing's own file, in its own format, and a Save has no choice of what to leave out.
            if drop_macros || drop_objects {
                return Err(bad(
                    ID,
                    "`drop_macros` and `drop_objects` go with a `path`: a save into the drawing's own file has no choice of what to leave out",
                ));
            }
            let own = s.path().map(Path::to_path_buf).ok_or_else(|| failed(ID, "this drawing has no file yet: use file.save_as to give it one"))?;
            (own, false, Options::default())
        }
        Some(_) => (Path::new(required_text(p, "path", ID)?).to_path_buf(), true, Options { drop_macros, drop_objects }),
    };
    Ok(make_save(s, ID, &path, by_extension, force, options)?.answer())
}
