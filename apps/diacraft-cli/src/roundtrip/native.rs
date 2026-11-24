//! `diacraft-cli roundtrip --via diacraft` and `diacraft-cli compare --content`.
//!
//! **The visit.** A drawing, stencil or template of either foreign format makes the trip the design's section 5 is about, in the engine and not beside it:
//!
//! 1. `file.open` the input in a session (a template as the file itself, as `roundtrip` always opens one);
//! 2. with `--edit` (or `--move`), the salted edit of plan ruling NP13, by the command the table of [`EditKind`] names, on the first shape of the first page that has one;
//! 3. `file.save_as` a `.diacraft` (`.diacraft-stencil` for a stencil, `.diacraft-template` for a template) in a scratch folder of this run, which is removed when it ends;
//! 4. `file.open` that file in a **new** session, so that nothing the first session kept (its bytes, its history, its content stamp) reaches the second;
//! 5. `file.save_as` the output in the format the input came in, and the content check (`diacraft_vsdx::compare_content` or `diacraft_odg::compare_content`) of the input and the output.
//!
//! **The line is an instrument** (a harness reads it over many files), so it is one shape every time: `via`, `format`, `input {sha256, bytes}`, `opened`, `variant`, `edit`,
//! `diacraft {bytes}`, `output {sha256, bytes}`, `content {compared, same, count, differences}`, `model {same}` and, when a leg of the trip was refused, `refusal {leg, error, problem, words}`.
//! **A line that did not finish never says same.** `content.same` and `model.same` are `null` and `content.compared` is `false` until the comparison was made, so a leg the engine refuses
//! (today a `.diacraft` drawing saved as a VSDX type or as an `.odg`, and an `.odg` saved as `.diacraft`, which the engine refuses in words until the writers are there) is a report with the
//! leg and the engine's words, and a harness that counts `same` true counts it as a failure, and one that counts `same` false does not count it as a difference.
//!
//! **What may cross to a person who must not see the file** is what the plain round trip lets cross: the hash, sizes, counts, a part's name when it is package structure (`part_label`), the
//! content check's paths (element names and positions, never a value) and its kinds. The engine's words are told for a refusal: they are the words `file.open` and `file.save_as` give a
//! person, which carry no text from the file and no path (`EngineError`); a bug (`internal`) has no words here.
//!
//! **`compare --content`** runs the same check on two files of one format. It does not open them as drawings: the check reads packages.

use std::path::{Path, PathBuf};

use diacraft_container::canon::ContentDiff;
use diacraft_doc::Document;
use diacraft_engine::file::{package_file_cap, read_file};
use diacraft_engine::memory::{self, Hold};
use diacraft_engine::{EngineError, Session, shown};
use diacraft_opc::CONTENT_TYPES_ITEM;
use diacraft_vsdx::{Limits, Variant, part_label};
use serde_json::{Value, json};

use super::{Written, compare_models, engine_error, first_shape, line, odg};
use crate::args::{CompareArgs, EditKind, RoundtripArgs};
use crate::oracle::sha256_hex;
use crate::{Failure, write_line};

/// The line weight `--edit cell` writes, in points: a number no file has by chance.
const SALT_WEIGHT: f64 = 1.23;
/// The inches `--edit move` moves the shape right and up: a power of two, so that the sum with any pin is exact in binary and the cell spells it the same on every machine.
const SALT_SHIFT: f64 = 0.125;
/// The text `--edit retext` gives the shape.
const SALT_TEXT: &str = "salt";
/// The master `--edit add` drops.
const SALT_MASTER: &str = "rectangle";
/// The most characters of the engine's words a line repeats.
const MOST_WORDS: usize = 400;

/// The two foreign formats a file can visit from, and the two the content check has lists for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// A VSDX package (any of the six types), and what is not another format: a file that is nothing is refused by the reader, in its own words.
    Vsdx,
    /// An OpenDocument drawing: a package or a flat file.
    Odg,
}

impl Format {
    fn word(self) -> &'static str {
        match self {
            Format::Vsdx => "vsdx",
            Format::Odg => "odg",
        }
    }
}

/// What a file is, as the engine would sniff it (`file.open`: DiaCraft's own first, then an OpenDocument drawing, then a VSDX package): `None` for a `.diacraft` file.
fn format_of(path: &Path, bytes: &[u8]) -> Option<Format> {
    if diacraft_engine::nativefile::sniff(bytes).is_some() {
        None
    } else if odg::is_odg(path, bytes) {
        Some(Format::Odg)
    } else {
        Some(Format::Vsdx)
    }
}

/// Whether `out` names a type of `format`: a VSDX type, or `.odg`. The trip saves back in the format the input came in, and a name of another one would compare two formats.
fn names_a_type_of(format: Format, out: &str) -> bool {
    let Some(extension) = Path::new(out).extension().and_then(|e| e.to_str()) else { return false };
    match format {
        Format::Vsdx => Variant::from_extension(extension).is_some(),
        Format::Odg => extension.eq_ignore_ascii_case("odg"),
    }
}

/// The words a name that is no type of the input's format is refused in.
fn wrong_type_words(format: Format) -> &'static str {
    match format {
        Format::Vsdx => "--out must end in .vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm: the trip saves back in the format the file came in",
        Format::Odg => "--out must end in .odg: the trip saves back in the format the file came in",
    }
}

// ------------------------------------------------------------------------------------------------------------------------ the content line

/// A part's name for a line that a person who must not see the file reads: the format's package structure by its name, and any other part by its role.
fn part_word(format: Format, name: &str) -> String {
    match format {
        Format::Vsdx if name == CONTENT_TYPES_ITEM => name.to_owned(),
        Format::Vsdx => part_label("a part", name).to_string(),
        Format::Odg => diacraft_odg::part_label(name).to_owned(),
    }
}

/// What the content check found, for the line: whether the two are the same, how many differences there are (exactly), and the first of them (200 at the most, the check's own cap) by part,
/// path and kind.
fn content_json(format: Format, diff: &ContentDiff) -> Value {
    let differences: Vec<Value> =
        diff.shown.iter().map(|d| json!({ "part": part_word(format, &d.part), "path": d.path, "found": d.found.word() })).collect();
    json!({ "compared": true, "same": diff.same(), "count": diff.count, "differences": differences, "class": null })
}

/// The content line of a check that was not made: it never says same, and it says why when the check itself refused.
fn not_compared(class: Option<&'static str>) -> Value {
    json!({ "compared": false, "same": null, "count": null, "differences": [], "class": class })
}

/// The content check on two files of `format`, as the format's crate makes it: the diff, or the class of the refusal.
fn check(format: Format, a: &[u8], b: &[u8]) -> Result<ContentDiff, &'static str> {
    match format {
        Format::Vsdx => diacraft_vsdx::compare_content(a, b, &Limits::default()).map_err(|e| e.class()),
        Format::Odg => diacraft_odg::compare_content(a, b, &diacraft_odg::Limits::default()).map_err(|e| e.class()),
    }
}

/// The content line of two files: the check made, or not made and why.
fn content_of(format: Format, a: &[u8], b: &[u8]) -> Value {
    match check(format, a, b) {
        Ok(diff) => content_json(format, &diff),
        Err(class) => not_compared(Some(class)),
    }
}

// ------------------------------------------------------------------------------------------------------------------------ compare --content

/// `compare --content FIRST SECOND`: the content check on two files of one format.
pub fn compare_content(args: &CompareArgs) -> Result<(), Failure> {
    let cap = package_file_cap(&Limits::default());
    let from_disk = |path: &str| read_file(Path::new(path), cap).map_err(|e| Failure::Message(format!("cannot read the file: {e}")));
    let (first, second) = (from_disk(&args.first)?, from_disk(&args.second)?);
    let (a, b) = (format_of(Path::new(&args.first), &first), format_of(Path::new(&args.second), &second));
    let format = match (a, b) {
        (Some(a), Some(b)) if a == b => a,
        (Some(a), Some(b)) => {
            return Err(Failure::Message(format!(
                "the two files are of different formats ({} and {}): the content check compares two files of one format",
                a.word(),
                b.word()
            )));
        }
        _ => {
            return Err(Failure::Message(
                "a .diacraft file has no content check of its own: compare the files it came from, or the files a trip through .diacraft wrote"
                    .to_owned(),
            ));
        }
    };
    let report = json!({
        "format": format.word(),
        "first": { "sha256": sha256_hex(&first), "bytes": first.len() },
        "second": { "sha256": sha256_hex(&second), "bytes": second.len() },
        "content": content_of(format, &first, &second),
    });
    write_line(&line(&report)?)
}

// ------------------------------------------------------------------------------------------------------------------------ the visit

/// The folder of one run, made for it alone and removed when it is dropped, with the `.diacraft` file in it. It is made in the system's temporary folder (`TMPDIR`), with a name that holds
/// the process's id and is never one that was there already, and with a mode that only its maker can use.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, Failure> {
        let base = std::env::temp_dir();
        for attempt in 0..64u32 {
            let dir = base.join(format!("diacraft-cli-via-{}-{attempt}", std::process::id()));
            match make_private_folder(&dir) {
                Ok(()) => return Ok(Scratch(dir)),
                // A folder that was there is never used: it is not ours.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Failure::Message(format!("cannot make a scratch folder: {}", io_words(&e)))),
            }
        }
        Err(Failure::Message("cannot make a scratch folder: every name tried was taken".to_owned()))
    }

    /// The path of a file in the folder, as the text a command takes.
    fn file(&self, name: &str) -> Result<String, Failure> {
        self.0.join(name).to_str().map(str::to_owned).ok_or_else(|| Failure::Message("the scratch folder's path is not text".to_owned()))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // The folder is litter at worst: a failure here has nobody to be told to.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn make_private_folder(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn make_private_folder(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

/// What the system said, as words: its kind, and not its text, which carries the path.
fn io_words(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "a folder in the path is not there",
        std::io::ErrorKind::PermissionDenied => "permission was refused",
        std::io::ErrorKind::StorageFull => "the disk is full",
        _ => "the system refused",
    }
}

/// The edit a run makes before the trip.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edit {
    /// A salted edit of the plan's table.
    Kind(EditKind),
    /// `--move DX,DY`: the move edit with a delta of the caller's own.
    Shift(f64, f64),
}

impl Edit {
    fn of(args: &RoundtripArgs) -> Option<Edit> {
        match (args.edit, args.shift) {
            (Some(kind), _) => Some(Edit::Kind(kind)),
            (None, Some((dx, dy))) => Some(Edit::Shift(dx, dy)),
            (None, None) => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Edit::Kind(kind) => kind.word(),
            Edit::Shift(..) => EditKind::Move.word(),
        }
    }
}

/// The refusal of a leg, for the line: where in the trip it was, the engine's kind of error and of trouble (fixed words), and the engine's words.
fn refusal(leg: &'static str, e: &EngineError) -> Value {
    let (error, problem) = engine_error(e);
    json!({ "leg": leg, "error": error, "problem": problem, "words": words(e) })
}

/// The words of an error as a person is given them. A bug's text is not told: it may be anything the engine broke on.
fn words(e: &EngineError) -> Option<String> {
    match e {
        EngineError::BadParams { msg, .. }
        | EngineError::Failed { msg, .. }
        | EngineError::CannotOpen { msg, .. }
        | EngineError::ChangedOnDisk { msg, .. }
        | EngineError::MacrosNotAllowed { msg, .. } => Some(shown(msg, MOST_WORDS)),
        EngineError::Disabled { reason, .. } => Some(shown(reason, MOST_WORDS)),
        EngineError::UnknownCommand(_) | EngineError::Internal { .. } => None,
    }
}

/// The page the edit acts on and the shape: the first top-level shape of the first page that has one (`--move`'s own pick), and for a new shape, the first page when none has a shape.
fn target(doc: &Document, edit: Edit) -> Option<(u32, Option<u32>)> {
    match first_shape(doc) {
        Some((page, shape)) => Some((page, Some(shape))),
        None if edit == Edit::Kind(EditKind::Add) => doc.pages.iter().next().map(|page| (page.id, None)),
        None => None,
    }
}

/// Half of a page's extent, in inches: the centre of that side. A figure that is missing, not finite or not above 0 is no extent, and the centre is then the corner (0).
pub(crate) fn half_of(extent: Option<f64>) -> f64 {
    extent.filter(|v| v.is_finite() && *v > 0.0).map_or(0.0, |v| v / 2.0)
}

/// The command of an edit, with its parameters, for the page and shape. A new shape is dropped at the page's centre, which the page's own setup says (inches on the paper).
fn edit_command(session: &mut Session, edit: Edit, page: u32, shape: Option<u32>) -> Result<(&'static str, Value), EngineError> {
    let ids = shape.map(|id| vec![id]).unwrap_or_default();
    Ok(match edit {
        Edit::Kind(EditKind::Cell) => ("shape.format", json!({ "ids": ids, "page": page, "weight": SALT_WEIGHT })),
        Edit::Kind(EditKind::Move) => ("shape.move", json!({ "ids": ids, "page": page, "dx": SALT_SHIFT, "dy": SALT_SHIFT })),
        Edit::Shift(dx, dy) => ("shape.move", json!({ "ids": ids, "page": page, "dx": dx, "dy": dy })),
        Edit::Kind(EditKind::Retext) => ("shape.set_text", json!({ "id": shape, "page": page, "text": SALT_TEXT })),
        Edit::Kind(EditKind::Delete) => ("shape.delete", json!({ "ids": ids, "page": page })),
        Edit::Kind(EditKind::Add) => {
            let setup = session.execute("page.setup", &json!({ "page": page }))?;
            let (x, y) = (half_of(setup["width"].as_f64()), half_of(setup["height"].as_f64()));
            ("shape.drop", json!({ "master": SALT_MASTER, "page": page, "x": x, "y": y }))
        }
    })
}

/// Makes the edit: what was done for the line (`kind`, the page, the shape, and a delta for a move), or the line's `edit` with the class of what stopped it, and the refusal.
fn make_edit(session: &mut Session, edit: Edit) -> Result<Value, (Value, Value)> {
    let kind = edit.word();
    let Some((page, shape)) = session.document().and_then(|doc| target(doc, edit)) else {
        let words = "the file has no shape on any page to edit";
        return Err((json!({ "kind": kind, "error": "no_shape" }), json!({ "leg": "edit", "error": "no_shape", "problem": null, "words": words })));
    };
    let stopped = |e: &EngineError| (json!({ "kind": kind, "page": page, "shape": shape, "error": engine_error(e).0 }), refusal("edit", e));
    let (command, params) = edit_command(session, edit, page, shape).map_err(|e| stopped(&e))?;
    let done = session.execute(command, &params).map_err(|e| stopped(&e))?;
    let mut told = json!({ "kind": kind, "page": page, "shape": if edit == Edit::Kind(EditKind::Add) { done["id"].clone() } else { json!(shape) } });
    if let Some((dx, dy)) = match edit {
        Edit::Shift(dx, dy) => Some((dx, dy)),
        Edit::Kind(EditKind::Move) => Some((SALT_SHIFT, SALT_SHIFT)),
        Edit::Kind(_) => None,
    } {
        told["dx"] = json!(dx);
        told["dy"] = json!(dy);
    }
    Ok(told)
}

/// The extension of the `.diacraft` type that holds what the session has: a stencil's own, a template's own, and the drawing's for everything else.
fn native_extension(session: &mut Session) -> &'static str {
    match session.execute("file.status", &json!({})).ok().and_then(|status| status["kind"].as_str().map(str::to_owned)).as_deref() {
        Some("stencil") => "diacraft-stencil",
        Some("template") => "diacraft-template",
        _ => "diacraft",
    }
}

/// `roundtrip FILE --out PATH --via diacraft`.
pub fn run(args: &RoundtripArgs, bytes: &[u8]) -> Result<(), Failure> {
    let Some(format) = format_of(Path::new(&args.file), bytes) else {
        return Err(Failure::Message(
            "the file is a .diacraft file already: --via diacraft sends a VSDX package or an OpenDocument drawing through it".to_owned(),
        ));
    };
    if !names_a_type_of(format, &args.out) {
        return Err(Failure::Message(wrong_type_words(format).to_owned()));
    }
    let mut report = json!({
        "via": "diacraft",
        "format": format.word(),
        "input": { "sha256": sha256_hex(bytes), "bytes": bytes.len() },
        "opened": false,
        "variant": null,
        "edit": null,
        "diacraft": null,
        "output": null,
        "content": not_compared(None),
        "model": { "same": null },
        "refusal": null,
    });
    // The input is held for the whole run, beside the sessions' reads and the read of what came back (`diacraft_engine::memory`).
    let _held = Hold::new(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    let scratch = Scratch::new()?;
    visit(args, bytes, format, &scratch, &mut report)?;
    write_line(&line(&report)?)
}

/// The legs of the trip, filling `report` as each is done. A leg the engine refuses ends the visit with `refusal` in the report, and the visit is then a report and not a failure; what
/// is not a refusal (a file that cannot be read back, a scratch folder) is a failure of the command.
fn visit(args: &RoundtripArgs, bytes: &[u8], format: Format, scratch: &Scratch, report: &mut Value) -> Result<(), Failure> {
    // 1. Open the input, as the file itself.
    let mut first = Session::new();
    if let Err(e) = first.execute("file.open", &json!({ "path": args.file, "edit_template": true })) {
        report["refusal"] = refusal("open", &e);
        return Ok(());
    }
    report["opened"] = json!(true);
    let status = first.execute("file.status", &json!({})).unwrap_or(Value::Null);
    report["variant"] = status["variant"].clone();
    // 2. The salted edit, if one was asked for.
    if let Some(edit) = Edit::of(args) {
        match make_edit(&mut first, edit) {
            Ok(told) => report["edit"] = told,
            Err((told, stopped)) => {
                report["edit"] = told;
                report["refusal"] = stopped;
                return Ok(());
            }
        }
    }
    // 3. Save as `.diacraft`, in the scratch folder.
    let native = scratch.file(&format!("visit.{}", native_extension(&mut first)))?;
    if let Err(e) = first.execute("file.save_as", &json!({ "path": native })) {
        report["refusal"] = refusal("save_diacraft", &e);
        return Ok(());
    }
    let diacraft_bytes =
        std::fs::metadata(&native).map(|m| m.len()).map_err(|e| Failure::Message(format!("cannot look at what was saved: {}", io_words(&e))))?;
    report["diacraft"] = json!({ "bytes": diacraft_bytes });
    // What the first session holds of the drawing, as it was saved: the model the output is compared with. The session lets go of its own when it ends, and this is held in its place.
    let before: Option<Document> = first.document().cloned();
    let _model_held = Hold::new(first.model_held().unwrap_or(0));
    drop(first);
    // 4. Open it in a session of its own, and 5. save it back in the format the input came in.
    let mut second = Session::new();
    if let Err(e) = second.execute("file.open", &json!({ "path": native, "edit_template": true })) {
        report["refusal"] = refusal("open_diacraft", &e);
        return Ok(());
    }
    if let Err(e) = second.execute("file.save_as", &json!({ "path": args.out })) {
        report["refusal"] = refusal("save_back", &e);
        return Ok(());
    }
    drop(second);
    let cap = package_file_cap(&Limits::default());
    let after = read_file(Path::new(&args.out), cap).map_err(|e| Failure::Message(format!("cannot read what was saved: {e}")))?;
    report["output"] = json!({ "sha256": sha256_hex(&after), "bytes": after.len() });
    report["content"] = content_of(format, bytes, &after);
    if let Some(before) = &before {
        report["model"] = model_of(format, before, &after);
    }
    Ok(())
}

/// Whether the model the output file reads as is the drawing that was saved as `.diacraft` (the first session's, after the edit): the equality `roundtrip` reports for a save, with the
/// differences by name (a page, a master, the styles) and never by content. `same` is `null` when the output cannot be read back.
fn model_of(format: Format, before: &Document, after: &[u8]) -> Value {
    let read = match format {
        Format::Vsdx => {
            // The read of what came back leaves room for the input, the model held and what a session holds, and holds what it is given while it runs.
            let reading = memory::reserve_read();
            diacraft_vsdx::read(after, &reading.limits()).map(|(doc, _)| doc).map_err(|e| e.class())
        }
        Format::Odg => diacraft_odg::read(after, &diacraft_odg::Limits::default()).map(|(doc, _)| doc).map_err(|e| e.class()),
    };
    match read {
        Ok(read) => {
            let compared = compare_models(before, &read, Written::Trip);
            json!({ "same": compared["equal"], "differ": compared["differ"], "differ_count": compared["differ_count"] })
        }
        Err(class) => json!({ "same": null, "class": class }),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_container::canon::{Difference, Found, MAX_SHOWN};
    use diacraft_engine::OpenProblem;

    use super::*;

    #[test]
    fn a_name_is_a_type_of_the_format_it_ends_in_whatever_its_case() {
        for name in ["a.vsdx", "a.VSDM", "a.vssx", "dir.d/a.Vssm", "a.vstx", "a.vstm"] {
            assert!(names_a_type_of(Format::Vsdx, name), "{name}");
            assert!(!names_a_type_of(Format::Odg, name), "{name}");
        }
        for name in ["a.odg", "a.ODG", "dir.d/a.Odg"] {
            assert!(names_a_type_of(Format::Odg, name), "{name}");
            assert!(!names_a_type_of(Format::Vsdx, name), "{name}");
        }
        // Not a type of either: another extension, none, a hidden name that is only an extension, a type that is a prefix of a name, and the other formats the trip is not to.
        for name in ["a", "a.", ".vsdx", "a.vsdxx", "a.diacraft", "a.fodg", "a.otg", "a.txt", "a.vsd", ""] {
            assert!(!names_a_type_of(Format::Vsdx, name) && !names_a_type_of(Format::Odg, name), "{name:?}");
        }
    }

    #[test]
    fn half_of_an_extent_is_the_centre_and_a_figure_that_is_no_extent_is_the_corner() {
        assert_eq!(half_of(Some(11.0)), 5.5);
        assert_eq!(half_of(Some(0.25)), 0.125);
        for none in [None, Some(0.0), Some(-3.0), Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY)] {
            assert_eq!(half_of(none), 0.0, "{none:?}");
        }
        assert!(half_of(Some(f64::MAX)).is_finite());
    }

    #[test]
    fn a_refusal_is_told_by_the_leg_the_kind_the_trouble_and_the_engines_words() {
        let msg = "the drawing could not be saved: nothing was written".to_owned();
        let cmd = "file.save_as".to_owned();
        let failed = refusal("save_back", &EngineError::Failed { cmd: cmd.clone(), msg: msg.clone() });
        assert_eq!(failed, json!({ "leg": "save_back", "error": "failed", "problem": null, "words": msg }));
        let open =
            refusal("open", &EngineError::CannotOpen { cmd: "file.open".to_owned(), problem: OpenProblem::Damaged, msg: "damaged".to_owned() });
        assert_eq!(open, json!({ "leg": "open", "error": "cannot_open", "problem": "damaged", "words": "damaged" }));
        let bad = refusal("save_diacraft", &EngineError::BadParams { cmd: cmd.clone(), msg: "the file name must end in .odg".to_owned() });
        assert_eq!((bad["error"].clone(), bad["words"].clone()), (json!("bad_params"), json!("the file name must end in .odg")));
        let disabled = refusal("save_diacraft", &EngineError::Disabled { cmd: cmd.clone(), reason: "not for a flat drawing".to_owned() });
        assert_eq!((disabled["error"].clone(), disabled["words"].clone()), (json!("disabled"), json!("not for a flat drawing")));
        // A bug's text, and an unknown command's, are not told: they may be anything the engine broke on or was given.
        for (error, kind) in [
            (EngineError::Internal { cmd: cmd.clone(), msg: "index out of bounds in Secret".to_owned() }, "internal"),
            (EngineError::UnknownCommand("Secret".to_owned()), "unknown_command"),
        ] {
            let told = refusal("save_back", &error);
            assert_eq!((told["error"].clone(), told["words"].clone()), (json!(kind), Value::Null));
            assert!(!told.to_string().contains("Secret"), "{told}");
        }
        // The words are cut, as the engine cuts what it repeats.
        let long = refusal("save_back", &EngineError::Failed { cmd, msg: "w".repeat(10 * MOST_WORDS) });
        assert!(long["words"].as_str().unwrap().chars().count() <= MOST_WORDS + 1, "{}", long["words"]);
    }

    #[test]
    fn a_part_is_named_by_its_structure_or_its_role_and_never_by_a_name_a_person_chose() {
        assert_eq!(part_word(Format::Vsdx, "/visio/pages/page1.xml"), "/visio/pages/page1.xml");
        assert_eq!(part_word(Format::Vsdx, CONTENT_TYPES_ITEM), CONTENT_TYPES_ITEM);
        assert_eq!(part_word(Format::Vsdx, "/foo/Secret name.dat"), "a part", "a space makes it a name a person typed");
        assert_eq!(part_word(Format::Odg, "content.xml"), "content.xml");
        assert_eq!(part_word(Format::Odg, "Pictures/Secret plan.png"), "picture");
        assert_eq!(part_word(Format::Odg, "Secret.xml"), "a part");
    }

    #[test]
    fn the_content_of_a_check_is_the_count_exactly_and_the_first_two_hundred_differences() {
        let shown: Vec<Difference> = (0..MAX_SHOWN)
            .map(|n| Difference {
                part: "/visio/pages/page1.xml".to_owned(),
                path: format!("/PageContents[0]/Shapes[0]/Shape[{n}]"),
                found: Found::Value,
            })
            .collect();
        let diff = ContentDiff { shown, count: 5_000_000 };
        let told = content_json(Format::Vsdx, &diff);
        assert_eq!((told["compared"].clone(), told["same"].clone(), told["count"].clone()), (json!(true), json!(false), json!(5_000_000)));
        assert_eq!(told["differences"].as_array().unwrap().len(), MAX_SHOWN);
        assert_eq!(
            told["differences"][1],
            json!({ "part": "/visio/pages/page1.xml", "path": "/PageContents[0]/Shapes[0]/Shape[1]", "found": "value" })
        );
        assert_eq!(told["class"], Value::Null);
        // Every kind the check finds has a word of its own.
        let words: std::collections::BTreeSet<&str> = [
            Found::MissingPart,
            Found::ExtraPart,
            Found::Bytes,
            Found::Element,
            Found::Attribute,
            Found::Value,
            Found::Text,
            Found::Instruction,
            Found::Order,
        ]
        .into_iter()
        .map(Found::word)
        .collect();
        assert_eq!(words.len(), 9);
        // No difference is the same, and only then.
        assert_eq!(content_json(Format::Odg, &ContentDiff::default())["same"], json!(true));
        assert_eq!(content_json(Format::Odg, &ContentDiff { shown: Vec::new(), count: 1 })["same"], json!(false));
    }

    #[test]
    fn a_check_that_was_not_made_never_says_same_and_says_why_when_the_check_refused() {
        for told in [not_compared(None), not_compared(Some("xml_too_complex"))] {
            assert_eq!(
                (told["compared"].clone(), told["same"].clone(), told["count"].clone(), told["differences"].clone()),
                (json!(false), Value::Null, Value::Null, json!([]))
            );
        }
        assert_eq!(not_compared(Some("xml_too_complex"))["class"], "xml_too_complex");
        assert_eq!(not_compared(None)["class"], Value::Null);
        // Two files that are not packages are not compared, and the class is the reader's.
        let told = content_of(Format::Vsdx, b"nope", b"nope");
        assert_eq!((told["compared"].clone(), told["same"].clone()), (json!(false), Value::Null));
        assert!(told["class"].is_string(), "{told}");
        let told = content_of(Format::Odg, b"nope", b"nope");
        assert_eq!((told["compared"].clone(), told["same"].clone()), (json!(false), Value::Null));
    }

    /// A drawing saved in `folder` as `name`, made by commands: one page with a rectangle.
    fn made(folder: &Scratch, name: &str) -> String {
        let path = folder.file(name).ok().unwrap();
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("shape.drop", &json!({ "master": "rectangle", "page": 1, "x": 2.0, "y": 3.0 })).unwrap();
        session.execute("file.save_as", &json!({ "path": path })).unwrap();
        path
    }

    #[test]
    fn the_type_of_the_diacraft_file_follows_what_the_input_is() {
        let folder = Scratch::new().ok().unwrap();
        for (name, extension) in [("a.vsdx", "diacraft"), ("a.vsdm", "diacraft"), ("a.vssx", "diacraft-stencil"), ("a.vstx", "diacraft-template")] {
            let path = made(&folder, name);
            let mut session = Session::new();
            session.execute("file.open", &json!({ "path": path, "edit_template": true })).unwrap();
            assert_eq!(native_extension(&mut session), extension, "{name}");
        }
        // A session with no drawing says nothing of what it holds: a drawing's type.
        assert_eq!(native_extension(&mut Session::new()), "diacraft");
    }

    #[test]
    fn the_edits_are_the_commands_of_the_plans_table_with_its_numbers() {
        let mut session = Session::new();
        let mut command = |edit: Edit, shape: Option<u32>| edit_command(&mut session, edit, 3, shape).unwrap();
        assert_eq!(command(Edit::Kind(EditKind::Cell), Some(7)), ("shape.format", json!({ "ids": [7], "page": 3, "weight": 1.23 })));
        assert_eq!(command(Edit::Kind(EditKind::Move), Some(7)), ("shape.move", json!({ "ids": [7], "page": 3, "dx": 0.125, "dy": 0.125 })));
        assert_eq!(command(Edit::Shift(-1.5, 2.0), Some(7)), ("shape.move", json!({ "ids": [7], "page": 3, "dx": -1.5, "dy": 2.0 })));
        assert_eq!(command(Edit::Kind(EditKind::Retext), Some(7)), ("shape.set_text", json!({ "id": 7, "page": 3, "text": "salt" })));
        assert_eq!(command(Edit::Kind(EditKind::Delete), Some(7)), ("shape.delete", json!({ "ids": [7], "page": 3 })));
        assert_eq!(Edit::Shift(1.0, 1.0).word(), "move", "a move of the caller's own is a move");
        assert_eq!(Edit::Kind(EditKind::Add).word(), "add");
    }

    #[test]
    fn the_target_of_an_edit_is_the_first_shape_of_the_first_page_that_has_one() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let doc = session.document().unwrap();
        assert_eq!(target(doc, Edit::Kind(EditKind::Cell)), None, "no shape, nothing to edit");
        let page = doc.pages.iter().next().unwrap().id;
        assert_eq!(target(doc, Edit::Kind(EditKind::Add)), Some((page, None)), "a new shape needs only a page");
        session.execute("shape.drop", &json!({ "master": "rectangle", "page": page, "x": 1.0, "y": 1.0 })).unwrap();
        session.execute("shape.drop", &json!({ "master": "ellipse", "page": page, "x": 3.0, "y": 1.0 })).unwrap();
        let doc = session.document().unwrap();
        let first = first_shape(doc).unwrap();
        assert_eq!(target(doc, Edit::Kind(EditKind::Delete)), Some((first.0, Some(first.1))));
        assert_eq!(target(doc, Edit::Kind(EditKind::Add)), Some((first.0, Some(first.1))), "a new shape goes to the page of the first shape");
    }

    #[test]
    fn the_scratch_folder_is_new_private_and_gone_with_its_owner() {
        let (first, second) = (Scratch::new().ok().unwrap(), Scratch::new().ok().unwrap());
        assert_ne!(first.0, second.0, "two runs of one process do not share a folder");
        assert!(first.0.is_dir());
        let file = first.file("visit.diacraft").ok().unwrap();
        std::fs::write(&file, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&first.0).unwrap().permissions().mode() & 0o077, 0, "only its maker may use it");
        }
        let path = first.0.clone();
        drop(first);
        assert!(!path.exists(), "the folder and what is in it are gone");
        assert!(second.0.is_dir(), "another folder is not touched");
        // A name that was taken is never used: the folder that is there is not ours.
        assert_eq!(make_private_folder(&second.0).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    }
}
