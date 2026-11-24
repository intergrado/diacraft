//! `diacraft-cli convert FILE --out PATH`: a drawing from one type to another.
//!
//! **The type written is the extension of `--out`, in any case.** It is a picture (`.png`, `.svg`, `.pdf`) or a drawing type: DiaCraft's own three (`.diacraft`,
//! `.diacraft-stencil`, `.diacraft-template`), the six of a VSDX package and the OpenDocument drawing and drawing template (`.odg`, `.otg`). [`drawing_types`] is that list, and it is made from what the
//! formats' own crates and the engine name (`diacraft_engine::odgfile::SAVE_TYPES`), not typed here. Which of them the engine writes **for the drawing at hand** is the engine's to say, and it says it when it is asked to write
//! (`Session::save_as_allows` is the same table asked without writing: the tests hold this list to it).
//!
//! **A drawing type is a Save As.** The input is opened by `file.open` and written by `file.save_as`, run through the registry as `run` runs them, so every refusal, every warning and the
//! way the file is written are the engine's own: the file is made whole in memory, goes to a temporary file beside the target, and is renamed over it, a link at the target is refused unless
//! it leads to the drawing's own file, and the words of a refusal are the engine's (a stencil is never saved as an `.odg`; a macro project is not written into a VSDX type that allows none). **A save across the
//! VSDX and OpenDocument formats is written**: `--drop-macros` and `--drop-objects` are the engine's options of the same names, passed through, and the line's `carried` is the save result's field. **A template is converted as the
//! file itself** (`edit_template`, as `roundtrip` opens one): a convert is about the file, not about a new drawing made from it. The command makes no edit and writes nothing of its own.
//! What it adds is the one rule a Save As does not have: **`--out` is never the input** (a link to it, or another spelling of its path, is caught too), since a Save As over a drawing's own file is a
//! Save, and a convert never writes its input.
//!
//! **A picture type goes through [`export`]'s own path** and its options (`--page`, `--dpi`, `--text`, `--transparent`, `--bundled-fonts`): the bytes are the ones `export` writes.
//!
//! **The line** is one JSON object: `in` (the type of the input: the type `file.open` says it is, told by its bytes and not its name), `out` (the path as given), `format` (the type written)
//! and `bytes`. For a drawing, the save's own counts follow: `variant`, `macros_dropped`, `parts` (`kept`, `rewritten`, `added`, `removed`: a VSDX save counts parts, any other is `null`), `copied`
//! (the entries of a `.diacraft` package copied as they were: any other save is `null`), `carried` (what the file carries) and `checked` (what the check after the save found: the file read back and compared
//! with the drawing, `{done, classes: [{class, words, count}], as_values, reason}`, or `null` for a save that is not through a model writer), `warning_count` and `warnings`, each told by the class word `roundtrip` tells it by. For a picture, `export`'s
//! own fields follow.
//!
//! **Exit status** is `export`'s: a mistake in the line (no `--out`, a type that is not written, an option for another type) is 2 and nothing ran; an input that cannot be opened, a refusal of the
//! engine, a page the drawing lacks and a file that cannot be written are 1, named on standard error and never with the path.

use std::path::Path;

use diacraft_engine::{EngineError, Session, nativefile, shown};
use diacraft_vsdx::{Kind, Variant};
use serde_json::{Value, json};

use crate::args::{ConvertArgs, ConvertTo};
use crate::{Failure, export, roundtrip, write_line};

/// The most characters of the engine's words an error repeats: a refusal of a save is a sentence and a way out, and is not cut short of it.
const MOST_WORDS: usize = 1_000;

/// Every type a drawing is written as, by the extension that names it (lower case, no dot), in the order the help lists them: the three kinds of `.diacraft` file (the formats' own words, `nativefile`),
/// the six types of a VSDX package (each kind with and without macros, `diacraft_vsdx::Variant`), and the OpenDocument drawing and template, `.odg` and `.otg` (the engine's own list,
/// `diacraft_engine::odgfile::SAVE_TYPES`).
pub fn drawing_types() -> Vec<&'static str> {
    let mut types: Vec<&'static str> = nativefile::Kind::ALL.into_iter().map(nativefile::variant_word).collect();
    for kind in [Kind::Drawing, Kind::Stencil, Kind::Template] {
        for macro_enabled in [false, true] {
            types.push(Variant { kind, macro_enabled }.extension());
        }
    }
    types.extend(diacraft_engine::odgfile::SAVE_TYPES);
    types
}

/// The drawing type the extension of `path` names, in any case: `None` for any other (a picture type among them).
pub fn drawing_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?;
    drawing_types().into_iter().find(|name| name.eq_ignore_ascii_case(extension))
}

/// The types written, as a sentence names them: `.diacraft, .vsdx, ... .pdf`, with the last joined by `or`, for the words of a mistake in the line.
pub fn type_words() -> String {
    let mut names: Vec<String> = drawing_types().into_iter().chain(["png", "svg", "pdf"]).map(|name| format!(".{name}")).collect();
    match names.pop() {
        Some(last) => format!("{} or {last}", names.join(", ")),
        None => String::new(),
    }
}

pub fn run(args: ConvertArgs) -> Result<(), Failure> {
    let line = match args.to {
        ConvertTo::Picture(picture) => {
            let (mut line, input) = export::make(&picture)?;
            line["in"] = json!(input);
            line
        }
        ConvertTo::Drawing(extension) => drawing(&args.file, &args.out, extension, (args.drop_macros, args.drop_objects))?,
    };
    write_line(&serde_json::to_string(&line).map_err(|e| Failure::Message(format!("cannot encode the result: {e}")))?)
}

/// The words of an engine error as a person is given them: its own, and not the command's name round them. A bug (`Internal`) is told whole, with the request to report it.
fn engine_words(error: &EngineError) -> String {
    match error {
        EngineError::BadParams { msg, .. }
        | EngineError::Failed { msg, .. }
        | EngineError::CannotOpen { msg, .. }
        | EngineError::ChangedOnDisk { msg, .. }
        | EngineError::MacrosNotAllowed { msg, .. } => shown(msg, MOST_WORDS),
        EngineError::Disabled { reason, .. } => shown(reason, MOST_WORDS),
        EngineError::UnknownCommand(_) | EngineError::Internal { .. } => shown(&error.to_string(), MOST_WORDS),
    }
}

/// A drawing type: `file.open`, then `file.save_as`, in a session of this process; the line is the save's.
fn drawing(file: &str, out: &str, extension: &'static str, (drop_macros, drop_objects): (bool, bool)) -> Result<Value, Failure> {
    export::refuse_the_drawing_itself(file, out)?;
    let mut session = Session::new();
    // A template is the file itself, as `roundtrip` opens one: the file is what is converted, and not a new drawing made from it.
    let opened = session
        .execute("file.open", &json!({ "path": file, "edit_template": true }))
        .map_err(|e| Failure::Message(format!("cannot open the drawing: {}", engine_words(&e))))?;
    let saved = session
        .execute("file.save_as", &json!({ "path": out, "drop_macros": drop_macros, "drop_objects": drop_objects }))
        .map_err(|e| Failure::Message(format!("cannot save the drawing as .{extension}: {}", engine_words(&e))))?;
    // The save's own fields, picked: a warning is told by its class (the table of `roundtrip`), as it is there.
    let warnings: Vec<&'static str> =
        saved["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).map(roundtrip::warning_class).collect()).unwrap_or_default();
    Ok(json!({
        "in": opened["file"]["variant"],
        "out": out,
        "format": extension,
        "bytes": saved["bytes"],
        "variant": saved["variant"],
        "macros_dropped": saved["macros_dropped"],
        "parts": saved.get("parts").cloned().unwrap_or(Value::Null),
        "copied": saved.get("copied").cloned().unwrap_or(Value::Null),
        // What the file carries: the engine's own field, never a warning.
        "carried": saved.get("carried").cloned().unwrap_or(Value::Null),
        // What the check after the save found: the file read back and compared with the drawing, class by class; `null` for a save that is not checked.
        "checked": saved.get("checked").cloned().unwrap_or(Value::Null),
        "warning_count": saved["warning_count"],
        "warnings": warnings,
    }))
}
