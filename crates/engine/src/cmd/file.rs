//! The document as a whole: making one, opening one, saving one and looking at it.

use std::path::Path;
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::Document;
use diacraft_render::export::{ExportError, Exported, TextMode, export_png};
use diacraft_render::{Parallel, Renderer, default_threads};
use diacraft_vsdx::{
    DiacraftPart, ErrorKind, Kind, LimitKind, OriginalBytes, ReadReport, Variant, VsdxError, WriteError, WriteOptions, check_extension,
    write_with_threads,
};
use serde_json::{Value, json};

use super::params::optional_whole;
use super::{CommandSpec, always, has_document, optional_text, required_text};
use crate::across::{self, Carried, Options};
use crate::check;
use crate::error::{OpenProblem, bad, cannot_open, changed_on_disk, failed, internal, macros_not_allowed, shown};
use crate::file::{FileError, FileFormat, FileStamp, OpenFile, package_file_cap, read_file_stamped, resolved_link, save_target, write_target};
use crate::memory::Reservation;
use crate::nativefile::{self, NativeState};
use crate::{HeldMemory, Result, Session, template};
use diacraft_native::Kind as NativeKind;

/// The most warnings an open reports one by one; the report counts the rest.
const WARNINGS_SHOWN: usize = 20;

/// The warnings an open lists: **at most [`WARNINGS_SHOWN`] in all**, the notes the command makes itself (`first`, put in front of the reader's, and `last`, behind them)
/// never cut, and the reader's own filling the room they leave (a note put in after the reader's 20 made 21, where the docs say "first 20"). The count that goes
/// with the list is the whole, and the caller makes it.
fn listed_warnings(first: Option<String>, reader: &[String], last: Option<String>) -> Vec<String> {
    let room = WARNINGS_SHOWN.saturating_sub(usize::from(first.is_some()) + usize::from(last.is_some()));
    first.into_iter().chain(reader.iter().take(room).cloned()).chain(last).collect()
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "document.inspect", "Inspect Document", [], None, "none", has_document, inspect),
        cmd!(
            "file.new",
            "New",
            ["File"],
            Some("Cmd+N"),
            "template: text, optional, the id of a template in template.list (basic_diagram, basic_flowchart, cross_functional_flowchart or organization_chart) or the path of a template file (.diacraft-template, .otg, .vstx or .vstm): \
             the new drawing has the template's page, styles and theme, and the stencils it opens (a blank drawing when left out)",
            always,
            new
        ),
        cmd!(
            "file.open",
            "Open",
            ["File"],
            Some("Cmd+O"),
            "path: the drawing, stencil or template file to open (.diacraft, .diacraft-stencil or .diacraft-template, DiaCraft's own, or .vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm), \
             or an OpenDocument drawing (.odg) or drawing template (.otg): the file's own type decides, not its extension, and a flat drawing .fodg is read, saved back as a flat file by file.save, and saved by file.save_as as a .diacraft file, an .odg or a VSDX type, as any drawing is, but never as a .fodg; \
             edit_template: true to open a template file itself, to change the template (false when left out: a template opens as a new drawing made from it, \
             which has no file until it is saved with file.save_as)",
            always,
            open
        ),
        cmd!(
            "file.save",
            "Save",
            ["File"],
            Some("Cmd+S"),
            "force: true to save over the file even if something else has changed it on the disk since the drawing was opened or last saved (false when left out). A drawing opened from an .odg is saved as an .odg (an .otg as the template it is), and a change the .odg writer does not write yet is refused with the reason and nothing is written. A drawing opened from a flat .fodg is saved back as a flat file, whatever the file is called. A drawing opened from a .diacraft file, or saved as one with file.save_as, is saved as a .diacraft file of the same kind (a drawing, a stencil or a template), whatever the file is called, and one opened from a VSDX file is saved as that. A file is never turned into another format by its name: a file whose name is another format's (a VSDX package called a.odg, an .odg called a.diacraft) is refused with the way out, which is file.save_as",
            has_document,
            save
        ),
        cmd!(
            "file.save_as",
            "Save As",
            ["File"],
            Some("Cmd+Shift+S"),
            "path: where to write the drawing, and its extension says which kind of file it is: .diacraft, .diacraft-stencil or .diacraft-template (DiaCraft's own format, which any drawing can be saved as, and the default for a new drawing), \
             .vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm (a drawing opened from a VSDX file is written into that file's own parts as before, while a new drawing and a .diacraft drawing have no VSDX file to write into and are written from the drawing alone), \
             or .odg (a drawing) or .otg (a drawing template), the OpenDocument package (a drawing opened from an .odg is written into that file's own package, while a new drawing and a .diacraft drawing are written from the drawing alone: a stencil is never saved as one). The file written becomes the drawing's own file, \
             and a drawing is saved in its own format unless this names another (a save across the VSDX and OpenDocument formats writes the file from the drawing alone, and what the type cannot say rides inside the file: the result's `carried` field says what), and a flat .fodg is never offered (save the drawing as an .odg). A drawing opened from a flat .fodg is saved by this command as any drawing is, written from the drawing alone (the .fodg is left as it is, and file.save writes it back as a flat file); \
             force: true to save over the drawing's own file even if something else has changed it on the disk since the drawing was opened or last saved \
             (false when left out); \
             drop_macros: true to leave the macro project out of the file when the drawing holds one and the type cannot hold it (a VSDX type that allows no macros: .vsdx, .vssx or .vstx, whatever language the project is in), or the file is of the other format (false when left out: a save to a type that cannot hold the project is refused, and nothing is written). A Save As that leaves it out leaves it out of every file.save into that file after it, and the drawing keeps it; \
             drop_objects: true to leave the embedded objects of the other format out of the file (each shape that showed one shows its picture, and the drawing still holds them: false when left out, and the objects are written where the type can hold them and ride in the file where it cannot). A Save As that leaves them out leaves them out of every file.save into that file after it. The result's `carried` says what the file carries",
            has_document,
            save_as
        ),
        cmd!(query "file.status", "File Status", ["File"], None, "none", has_document, status),
        cmd!(
            "file.export",
            "Export",
            ["File"],
            None,
            "path: where to write the picture, and its extension (.png, .svg or .pdf, in any case) says which; \
             page: the ID of the page to export (the first page when left out); \
             dpi: pixels to the inch of a PNG (96 when left out, above 0 and at most 2400); \
             text: for an SVG or a PDF, `text` (the default: letters are text where the font allows it) or `outlines` (every letter as paths); \
             transparent: true for no white paper under the drawing",
            has_document,
            export
        ),
    ]
}

/// The most pixels to the inch `file.export` takes for a PNG: the renderer's own limits (16,384 pixels a side, 64 million in all) stop a larger
/// picture of a larger page, and this stops a number that no page could use. The CLI's `--dpi` has the same bound.
const MAX_DPI: f64 = 2400.0;

/// The pixels to the inch of a PNG when `dpi` is left out.
const DEFAULT_DPI: f64 = 96.0;

/// What a person may name to export to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picture {
    Png,
    Svg,
    Pdf,
}

impl Picture {
    fn of(path: &Path) -> Option<Picture> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(Picture::Png),
            "svg" => Some(Picture::Svg),
            "pdf" => Some(Picture::Pdf),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Picture::Png => "png",
            Picture::Svg => "svg",
            Picture::Pdf => "pdf",
        }
    }
}

/// Whether writing to `target` would replace the drawing that is open: it is the file the drawing was opened from or last saved to, by its
/// path or by where the paths lead (a link, a `..`, another spelling).
pub(super) fn is_the_open_drawing(target: &Path, own: Option<&Path>) -> bool {
    let Some(own) = own else {
        return false;
    };
    if target == own {
        return true;
    }
    // A file in memory has one spelling: no link, `..` or other name leads to it.
    if crate::memfs::active() {
        return false;
    }
    matches!((std::fs::canonicalize(target), std::fs::canonicalize(own)), (Ok(a), Ok(b)) if a == b)
}

/// Writes one page of the drawing as a picture, a vector file or a PDF, by the extension of `path`. **It changes nothing in the drawing**:
/// it is not an undo step, does not mark the drawing saved, and does not give it a file. The page is drawn as the window draws it (the renderer's
/// own walk), so the grid, rulers and guides are not in it, and what the page left out or drew by a stand-in is returned, as the window counts it.
///
/// **What a path may be** is `file.save_as`'s rule, judged before anything is written: a link is refused unless it leads to the drawing's own file
/// (`save_target`), the extension is judged on the file that would be written, a folder, a device or a pipe is refused, and **the drawing's own file
/// is never written** (a link to it, or another spelling of its path, is refused as the drawing itself). The picture is made whole in memory, then
/// goes to a temporary file beside the target and is renamed over it, so a failure leaves a file that was there as it was.
///
/// → `{format, page: {id, name}, width, height, bytes, skipped: [{kind, count, first}]}`: the size in the format's own units (pixels, CSS pixels,
/// points), and `skipped` as `Exported::skipped` has it.
fn export(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.export";
    if s.in_group() {
        return Err(failed(ID, format!("`{ID}` cannot run inside a batch or other undo group")));
    }
    let path = Path::new(required_text(p, "path", ID)?);
    let page = optional_whole(p, "page", ID)?;
    let dpi = match p.get("dpi") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() && n > 0.0 && n <= MAX_DPI => Some(n),
            _ => return Err(bad(ID, format!("`dpi` must be a number above 0 and at most {MAX_DPI}"))),
        },
    };
    let text = match optional_text(p, "text", ID)? {
        None | Some("text") => TextMode::AsText,
        Some("outlines") => TextMode::AsOutlines,
        Some(_) => return Err(bad(ID, "`text` must be `text` or `outlines`")),
    };
    let transparent = match p.get("transparent") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(ID, "`transparent` must be true or false")),
    };
    // The file that would be written, and what it is: a link is refused unless it leads to the open drawing, and then the drawing itself is.
    let target = save_target(path, s.path()).map_err(|e| failed(ID, format!("the picture could not be saved: {e}")))?;
    let format = Picture::of(&target).ok_or_else(|| bad(ID, "the file name must end in .png, .svg or .pdf"))?;
    if is_the_open_drawing(&target, s.path()) {
        return Err(failed(ID, "the picture could not be saved: that is the drawing's own file, which an export never writes"));
    }
    if dpi.is_some() && format != Picture::Png {
        return Err(bad(ID, "`dpi` is for a PNG: an SVG is in CSS pixels and a PDF in points"));
    }
    if p.get("text").is_some_and(|t| !t.is_null()) && format == Picture::Png {
        return Err(bad(ID, "`text` is for an SVG or a PDF: a PNG has only pixels"));
    }
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let page = match page {
        Some(id) => doc.page(id).ok_or_else(|| bad(ID, format!("no page has id {id}")))?,
        None => doc.pages.iter().next().ok_or_else(|| bad(ID, "the drawing has no pages"))?,
    };
    let paper = (!transparent).then_some(Rgba::WHITE);
    let title = Some(page.name.to_string());
    // The machine's faces first, as the window lays text out, so that an export is set in the faces the person sees.
    let mut renderer = Renderer::with_shared_fonts(crate::measure::renderer_fonts());
    renderer.set_paper(paper);
    // A PNG is drawn on the machine's cores less one (`DIACRAFT_THREADS` says otherwise); the picture is the same bytes at any count.
    renderer.set_parallel(Parallel::with_threads(default_threads()));
    let made: std::result::Result<Exported, ExportError> = match format {
        Picture::Png => export_png(&mut renderer, doc, page.id, dpi.unwrap_or(DEFAULT_DPI)),
        Picture::Svg => {
            diacraft_svg::export_page(&mut renderer, doc, page.id, &diacraft_svg::SvgOptions { paper, text, title, ..Default::default() })
        }
        Picture::Pdf => {
            diacraft_pdf::export_page(&mut renderer, doc, page.id, &diacraft_pdf::PdfOptions { paper, text, title, ..Default::default() })
        }
    };
    let exported = made.map_err(|e| failed(ID, e.to_string()))?;
    write_target(&target, &exported.bytes).map_err(|e| failed(ID, format!("the picture could not be saved: {e}")))?;
    let skipped: Vec<Value> =
        exported.skipped.iter().map(|s| json!({ "kind": format!("{:?}", s.kind), "count": s.count, "first": shown(&s.first) })).collect();
    Ok(json!({
        "format": format.name(),
        "page": { "id": page.id, "name": page.name },
        "width": exported.width,
        "height": exported.height,
        "bytes": exported.bytes.len(),
        "skipped": skipped,
    }))
}

/// Writes the drawing to the file it was opened from or last saved to, keeping everything in that file the edits did not touch. A drawing
/// that has no file yet is an error (the frontend asks for a path and runs `file.save_as`). The file's extension is judged as `file.save_as`
/// judges it, and **a drawing opened through a link is written to the file the link led to when it was opened**, though the link may lead
/// elsewhere by now. **A file that has changed on the disk since the drawing was opened or last saved is not replaced** unless `force` is
/// true: the command ends with [`crate::EngineError::ChangedOnDisk`] and writes nothing. → `{bytes, variant, dirty, parts, warnings,
/// warning_count}`, as `file.save_as` gives it.
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.save";
    let force = flag(p, "force", ID)?;
    let path = s.path().map(Path::to_path_buf).ok_or_else(|| failed(ID, "this drawing has no file yet: use file.save_as to give it one"))?;
    save_to(s, ID, &path, false, force, Options::default())
}

/// The true or false `p` gives for `key`; false when it is left out.
fn flag(p: &Value, key: &str, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// Writes the drawing to `path`, which becomes the drawing's file; the kind of file follows its extension. A drawing that was opened from a
/// file keeps every part of that file the edits did not touch, byte for byte. A file that is already at `path` is replaced only when the new
/// one has been written whole beside it. **A link at `path` is refused** unless it leads to the drawing's own file (the one it was opened from or
/// last saved to): the file a link leads to is never written because a caller named the link, and the extension is judged on the file that
/// would be written. When `path` is the drawing's own file, a change made to it on the disk since the drawing was opened or last saved stops the
/// save, as for `file.save`, unless `force` is true. → `{bytes, variant, dirty: false, parts: {kept, rewritten, added, removed}, warnings: [..first
/// 20..], warning_count}`.
fn save_as(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.save_as";
    let path = Path::new(required_text(p, "path", ID)?);
    let force = flag(p, "force", ID)?;
    let options = Options { drop_macros: flag(p, "drop_macros", ID)?, drop_objects: flag(p, "drop_objects", ID)? };
    save_to(s, ID, path, true, force, options)
}

/// The saved state of the session, and the type of document a save writes: whether the drawing has unsaved changes, whether it has a file, and its **format**
/// (`diacraft`, `vsdx` or `odg`: what a save writes unless the person picks another), its **variant** (the extension of the type the file was read as or last saved as, without the
/// dot: `diacraft`, `diacraft-stencil`, `diacraft-template`, `vsdx`, `vsdm`, `vssx`, `vssm`, `vstx`, `vstm` or `odg`) and its **kind** (`drawing`, `stencil` or `template`).
/// → `{dirty, has_file, format, variant, kind}`; the last three are `null` for a drawing that has no file of a type (a new one, one made from a template). The path is the frontend's to
/// know (it gave it); [`Session::path`] has it for a frontend in this process.
fn status(s: &mut Session, _: &Value) -> Result<Value> {
    let source = s.source();
    let format = source.and_then(OpenFile::format);
    let variant = source.and_then(OpenFile::extension).filter(|_| format.is_some());
    let kind = variant.and_then(|extension| match extension {
        "odg" => Some("drawing"),
        "otg" => Some("template"),
        other => nativefile::kind_of_extension(other).map(|kind| kind.word()).or_else(|| Variant::from_extension(other).map(Variant::kind_word)),
    });
    Ok(json!({ "dirty": s.is_dirty(), "has_file": s.path().is_some(), "format": format.map(FileFormat::word), "variant": variant, "kind": kind }))
}

/// The save both commands share. The file that would be written is settled first ([`save_target`]: a link is refused unless it leads to the
/// drawing's own file), and for `file.save_as` the extension of that file says the kind. The file is made in memory, and the writer reads it back
/// before it is returned, so a document the writer cannot write leaves the disk untouched; then it goes to a temporary file beside the target and
/// is renamed over it, so a disk that cannot be written leaves the old file as it was. Only then is the session told the drawing is saved.
///
/// **The drawing's own file**. A path that is the drawing's own, opened or saved through a link, is written at the file the link led to
/// then, resolved once when it was opened or saved: a link pointed elsewhere since does not move the save. Before anything is written, that
/// file is compared with how it looked when the drawing was opened or last saved (its size and modified time, `FileStamp`); if something else has
/// changed it, the save is [`crate::EngineError::ChangedOnDisk`] and nothing is written, unless `force` is true. Both commands judge the extension
/// of the file that would be written; `file.save_as` takes the kind of file from it, and `file.save` keeps the kind the file had.
fn save_to(s: &mut Session, cmd: &str, path: &Path, by_extension: bool, force: bool, options: Options) -> Result<Value> {
    if s.in_group() {
        return Err(failed(cmd, format!("`{cmd}` cannot run inside a batch or other undo group")));
    }
    // A drawing that came from a flat file (`.fodg`) is **saved in place as a flat file**: `file.save` writes the file's own form, whatever the file
    // is called. **A Save As is another matter**: it writes the drawing from the model, as a `.diacraft` file, an `.odg` package or a VSDX type, as for any
    // drawing, and the flat file is left as it is. A `.fodg` is never a Save As target (below), and a flat document is never written under another name or the name of an `.odg`.
    let flat = s.source().is_some_and(OpenFile::is_flat);
    // A new drawing, or one made from a template, has no file of its own (and keeps no template's bytes, NP10): nothing it is saved to is "its own file".
    let own = s.source().filter(|f| f.is_named()).map(|f| (f.path().to_path_buf(), f.target().to_path_buf(), f.stamp()));
    let unsaved = |e| failed(cmd, format!("the drawing could not be saved: {e}"));
    // The drawing's own path, opened through a link, is the file the link led to then. Any other path is judged as it is.
    let target = match &own {
        Some((own_path, resolved, _)) if own_path == path && resolved != own_path => save_target(resolved, None).map_err(unsaved)?,
        _ => save_target(path, own.as_ref().map(|(_, resolved, _)| resolved.as_path())).map_err(unsaved)?,
    };
    // Save As does not offer `.fodg`: judged on the file that would be written, as the other extensions are.
    if by_extension && target.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("fodg")) {
        return Err(bad(cmd, crate::odgfile::FODG_NOT_OFFERED));
    }
    // The type of file a save writes is decided by the drawing's working file and by the name it is given (`save_path`, the design's 4.1): a `.diacraft` file is written as one whatever the
    // name of the file it is saved over (the bytes decided what it is when it was opened, 1.2), and Save As goes by the extension. A refusal is in words, before anything is made.
    let working = s.source().and_then(OpenFile::format);
    let stencil = s.source().is_some_and(OpenFile::is_stencil);
    let wanted = match s.source().and_then(OpenFile::native) {
        Some(native) if !by_extension => Target::Diacraft(native.kind()),
        _ => Target::of(target.extension().and_then(|e| e.to_str())),
    };
    // **`file.save` keeps the format of the file the drawing has** (Call 16), whatever the table says of another one: a name that is another format's is no name for this file, and is refused
    // with the words of the drawing's own file (use `file.save_as`). Without this, once the writers across the formats are there, saving a VSDX package called
    // `a.odg`, or an `.odg` called `a.diacraft`, would turn the file into another format by its name. (Save As is the way to change the format, and names it.)
    let wanted = match (by_extension, working, wanted) {
        (false, Some(FileFormat::Vsdx), Target::Diacraft(_) | Target::Odg(_))
        | (false, Some(FileFormat::Odg), Target::Diacraft(_) | Target::Vsdx(_)) => Target::Other,
        _ => wanted,
    };
    // (A flat file is saved as it is by `file.save`, whatever it is called: its own file is not named for a type. A Save As goes by the table, but a flat file is **no package to splice into**:
    // an `.odg` or `.otg` that it is saved as is written from the model, as for a drawing that has no `.odg` of its own.)
    let route = match save_path(working, stencil, wanted) {
        _ if flat && !by_extension => SavePath::OdgSplice(diacraft_odg::Variant::Drawing),
        SavePath::OdgSplice(variant) if flat => SavePath::OdgModel(variant),
        other => other,
    };
    // **A Save As from an `.odg` to another `.odg` that is asked to leave out what the file carries of another format** (a VBA project, VSDX objects that ride in it) is written from the model, as a
    // Save As of a VSDX file to an `.odg` is (it ignored the options): a splice into the file cannot take out of it what it holds.
    let route = match route {
        SavePath::OdgSplice(kind)
            if by_extension
                && !options.is_empty()
                && s.document().is_some_and(|doc| {
                    let earlier = s.source().and_then(OpenFile::odg).map(|state| state.left_out().asked).unwrap_or_default();
                    let new = Options {
                        drop_macros: options.drop_macros && !earlier.drop_macros,
                        drop_objects: options.drop_objects && !earlier.drop_objects,
                    };
                    across::dropped::odg_leaves_out(doc, new).unwrap_or(false)
                }) =>
        {
            SavePath::OdgModel(kind)
        }
        other => other,
    };
    let named = match route {
        SavePath::Refused(why) => return Err(refused_save(cmd, why, by_extension)),
        // A `.diacraft` drawing saved as a VSDX type is written from the model alone (`write_with(doc, None, ..)`): a `.diacraft` file keeps no VSDX file to write into.
        SavePath::VsdxModel => match wanted {
            Target::Vsdx(named) => Some(named),
            _ => return Err(internal(cmd, "a save from the model to a type that is no VSDX type")),
        },
        SavePath::VsdxSplice(named) => Some(named),
        SavePath::Native(_) | SavePath::OdgSplice(_) | SavePath::OdgModel(_) => None,
    };
    let variant = named.filter(|_| by_extension);
    if let Some((_, resolved, Some(then))) = &own
        && !force
        && is_the_open_drawing(&target, Some(resolved))
        && FileStamp::of_path(&target).is_some_and(|now| now != *then)
    {
        return Err(changed_on_disk(
            cmd,
            "the file on the disk has been changed by something else since this drawing was opened or last saved, so it was not replaced; \
             save again with `force` to replace it, or save as another file",
        ));
    }
    let named = match route {
        SavePath::Native(kind) => return save_native(s, cmd, path, target, kind),
        // The kind a Save As names (`.odg` a drawing, `.otg` a template) is asked of the writer, and a Save keeps the kind the file has.
        SavePath::OdgSplice(variant) => return save_odg(s, cmd, path, target, by_extension.then_some(variant), options),
        // An `.odg` or `.otg` written from the model alone: the drawing's working file is a `.diacraft` file, or it has none.
        SavePath::OdgModel(variant) => return save_odg_model(s, cmd, path, target, variant, options),
        _ => named.ok_or_else(|| internal(cmd, "a save of a VSDX file with no type"))?,
    };
    // (No content stamp is made here: a pending one keeps the document the drawing was read as, and is made when something asks for it; and the blocker of its re-review.)
    let crate::session::SaveInputs { doc, source, baseline } = s.save_inputs().ok_or_else(|| failed(cmd, "no document open"))?;
    // **From the model alone** (`SavePath::VsdxModel`), there is no file to splice into: the working file is a `.diacraft` one, whose bytes are no VSDX package. Whether the drawing is the
    // one its VSDX file was read as is the content stamp's to say (the design's 4.4: the thumbnail, `Application` and `modified` are kept when it is, and written as an edited save
    // writes them when it is not, or when there is no stamp to ask), and never the dirty flag's, which a reopen does not keep. A save into a VSDX file judges for itself.
    let from_model = route == SavePath::VsdxModel;
    // **The objects the model writer wrote as embeddings ride no more**: the file holds each once, as an embedding, and the drawing, which still holds the part the embedding was written from,
    // is given to a splice without it, as is the baseline it is compared with. Otherwise an edit rewrites DiaCraft's part from the drawing and puts the object's file in it a second time.
    let natively: Arc<[Arc<str>]> = if from_model { Arc::default() } else { source.as_ref().map(|f| Arc::clone(f.natively())).unwrap_or_default() };
    let baseline_given: Option<std::borrow::Cow<'_, Document>> =
        baseline.as_deref().map(|held| across::natively::without_parts(std::borrow::Cow::Borrowed(held), &natively));
    let original = source.as_ref().filter(|_| !from_model).map(|f| OriginalBytes { bytes: f.bytes(), baseline: baseline_given.as_deref() });
    let content_unchanged = from_model && s.content_unchanged() == Some(true);
    // **What a Save As left out stays out of every save into the file it wrote**: `earlier` is what the file lacks already, and `asked` what it will lack when this save is
    // written. A file written from the model has no earlier, and a file the drawing was read from has none.
    let earlier = if from_model { Options::default() } else { source.as_ref().map(OpenFile::asked).unwrap_or_default() };
    let asked = earlier.union(options);
    // Whether the type written holds a macro project: the type a Save As names, and for a Save the type the file has.
    let macro_enabled = if by_extension {
        named.macro_enabled
    } else {
        source.as_ref().and_then(OpenFile::extension).and_then(Variant::from_extension).map_or(named.macro_enabled, |v| v.macro_enabled)
    };
    // `recalc_document` stays false: no command changes geometry without recomputing what depends on it yet (revisit when one does).
    // A drawing with no file yet (a new one, or one made from a template: it keeps no file's bytes, NP10) is written as a new document, with times of its own. (A source with no path is the
    // bytes of a template an `.odg` drawing was made from, which no command makes now.)
    let new_copy = source.as_ref().is_none_or(|f| !f.is_named());
    // **A Save with no edit into a file this session wrote writes the bytes the last save wrote**, the program's name in its properties included: the model writer keeps the
    // name of the program a drawing was read from while its content is as read (the unedited visit), and the splice named DiaCraft on every save.
    let resave_unedited = !from_model && !by_extension && !s.is_dirty() && source.as_ref().is_some_and(OpenFile::written_here);
    let write_options =
        WriteOptions { variant, now: s.now(), recalc_document: false, drop_macros: asked.drop_macros, new_copy, content_unchanged, resave_unedited };
    let (to_write, not_recorded) = with_template_stencils(s, &doc, named.kind == Kind::Template);
    // **A save across**: a drawing that stands on an `.odg` takes the package data of that file as it is now (as a save as `.diacraft` does), and the model writer is given the drawing
    // as the other format needs it: the objects left out when asked, and the picture box of a picture an `.odg` held (`across::onto`).
    // The drawing a Save As that leaves something out puts what it left out back on (`across::dropped::restore`): the drawing as the writer was given it, with the package data of the `.odg` it stands on
    // when it was given that (the session's drawing held an object's file that the `.odg` no longer did, its frame deleted, which the Save As did not write and `restore` put back, and the
    // next edit wrote it into the file the Save As wrote without it). `None` when that is the drawing itself.
    let mut restore_from: Option<Document> = None;
    let (to_write, given) = if from_model {
        let to_write = with_file_package_data(cmd, source.as_ref(), to_write)?;
        if took_the_files_package_data(source.as_ref(), matches!(to_write, std::borrow::Cow::Owned(_))) && !asked.is_empty() {
            restore_from = Some(Document::clone(&to_write));
        }
        // The crossing refuses what the plain visit refuses, by the `.odg` writer's own rule.
        if let Some(words) = across::crossing_refusal(&to_write) {
            return Err(failed(cmd, words));
        }
        let mut given = across::onto::given(&to_write, diacraft_doc::Format::Vsdx, options).map_err(|why| failed(cmd, why))?;
        let to_write = match given.doc.take() {
            Some(changed) => std::borrow::Cow::Owned(changed),
            None => to_write,
        };
        (to_write, given)
    } else if asked.is_empty() {
        (to_write, across::onto::Given::default())
    } else {
        // A save into a file that lacks what a Save As left out: the writer is given the drawing as it was given then (`across::dropped`).
        let (made, left) = across::dropped::given(&to_write, asked, earlier.drop_macros && !macro_enabled).map_err(|why| failed(cmd, why))?;
        let doc = match made {
            std::borrow::Cow::Owned(changed) => Some(changed),
            std::borrow::Cow::Borrowed(_) => None,
        };
        let to_write = match doc {
            Some(changed) => std::borrow::Cow::Owned(changed),
            None => to_write,
        };
        (to_write, across::onto::Given { doc: None, left, boxed: 0 })
    };
    let to_write = if from_model { to_write } else { across::natively::without_parts(to_write, &natively) };
    let room = check_save_peak(cmd, s, &to_write)?;
    let mut written =
        write_with_threads(&to_write, original.as_ref(), &write_options, crate::threads::save_threads_within(room)).map_err(|e| match e {
            // The caller's choice, which a frontend asks the person for, and not a failure of the writer.
            WriteError::MacroProject => macros_not_allowed(cmd, format!("the drawing could not be saved: {e}")),
            e => failed(cmd, crate::shown(&format!("the drawing could not be saved: {e}"), 300)),
        })?;
    // **The check after the save**: a file written from the model is compared with the drawing it was written from, by the model the writer read it back as. A splice into the
    // drawing's own file is not checked in 0.2.
    let checked = from_model.then(|| check::compare(&to_write, &written.document, options.drop_macros));
    write_target(&target, &written.bytes).map_err(|e| failed(cmd, format!("the drawing could not be saved: {e}")))?;
    // **What the file carries** (the design's 5.6): a structured field and never a warning, for a file written from the model. The warning the writer makes of the signatures is the field's.
    let carried = if from_model {
        let signatures = across::classes::signatures_of(&to_write, diacraft_doc::Format::Vsdx)
            .saturating_add(across::classes::signatures_of(&to_write, diacraft_doc::Format::Odf));
        written.warnings.retain(|w| !w.contains("digital signature(s) in this file no longer match it"));
        across::carried_into_vsdx(&to_write, written.carried, signatures).map_err(|why| failed(cmd, why))?
    } else {
        Carried::none()
    };
    // Said in words, before the writer's own: the template is saved without the list of stencils that are open (the file's DiaCraft data is of a later build's).
    let mut said: Vec<String> = Vec::new();
    if not_recorded {
        said.push(
            "the stencils open now were not recorded in the template: the file holds DiaCraft data of a version this build does not read, which it keeps as it is"
                .to_owned(),
        );
    }
    said.extend(across::onto::left_out_words(&given));
    said.extend(written.warnings.iter().cloned());
    let warnings: Vec<String> = said.iter().take(WARNINGS_SHOWN).cloned().collect();
    let result = json!({
        "bytes": written.bytes.len(),
        "variant": written.read_back.variant.map(|v| v.extension()),
        "macros_dropped": written.macros_dropped.len(),
        "dirty": false,
        "parts": {
            "kept": written.kept,
            "rewritten": written.rewritten.len(),
            "added": written.added.len(),
            "removed": written.removed.len(),
        },
        "carried": carried.to_json(),
        "checked": checked_json(checked.as_ref()),
        "warnings": warnings,
        "warning_count": said.len(),
    });
    // The file as it is now, and where the path led: the next save compares with this, and writes there.
    let stamp = FileStamp::of_path(&target);
    let led_to = (target != path).then_some(target);
    // **The drawing keeps what it holds when the check found a difference**: the model the file reads as is then a lesser drawing, and the one in the session is the one that has it all.
    let check_clean = checked.as_ref().is_some_and(|outcome| matches!(outcome, check::Outcome::Checked(found) if found.classes.is_empty()));
    let adopt = from_model && across::adopts_by_working_file(working, &given) && across::adopts(&written, &given) && check_clean;
    // **A Save As that left something out**: the drawing becomes what the file reads as, with what was left out put back on it (`across::dropped::restore`), when that holds all the
    // drawing did but what was left out, and the file's baseline is the drawing as the writer is given it. A save into the file that left the shapes of objects out gives the same again.
    let restored =
        (!adopt && !asked.is_empty() && (from_model || given.left.shapes > 0) && (!from_model || check_clean) && across::came_back(&written))
            .then(|| across::dropped::restore(written.document.clone(), restore_from.as_ref().unwrap_or(&doc), given.left, &written.natively))
            .flatten();
    let file = OpenFile::new(path, written.bytes)
        .with_stamp(stamp)
        .with_target(led_to)
        .with_extension(Some(named.extension()))
        .with_written_here()
        .with_asked(asked)
        .with_natively(across::natively::union(&natively, &written.natively));
    // **The drawing becomes what the file reads as** after a save across to VSDX, when that holds everything the drawing did (`across`, "the model after a save across"): the next save into the file
    // is a splice that compares the drawing with the file as it reads, and a drawing that held an `.odg`'s frame where the file holds an embedding would be refused ("a change to the picture of a shape").
    if adopt {
        s.adopt_saved(written.document, file);
    } else if let Some(kept) = restored {
        // (The drawing as the writer is given it, of the drawing that is kept: the baseline the next save compares with.)
        match across::dropped::given(&kept, asked, !macro_enabled && asked.drop_macros) {
            Ok((baseline, _)) => {
                let baseline = baseline.into_owned();
                s.adopt_saved_with(kept, file, baseline);
            }
            Err(_) => s.mark_saved(&doc, file, written.document),
        }
    } else {
        s.mark_saved(&doc, file, written.document);
    }
    Ok(result)
}

/// Our rule: a template that is saved keeps the built-in stencils open at that moment, as the list in DiaCraft's part that names them, when they are not the ones its
/// document already implies (its own list, or the stencils a new drawing has). A template opened and saved again with its stencils as it opened them is then written as it was, and one
/// that has no list, whoever made it, is not given one for nothing. The list goes into the copy that is written and not into the drawing, which is no different for it (not dirty, no undo
/// step); a list that is the stencils a new drawing has takes the child out of the copy, and the part with it when nothing else is in it. Stencils opened from files are not kept: a
/// template never names a path. → the document to write, and `true` when the list could not be put in (the file's DiaCraft data is of a version this build does not read, which it keeps as
/// it is and does not change). A copy shares every page and master with the drawing, so a `.diacraft` package copies what the copy did not change.
fn with_template_stencils<'d>(s: &Session, doc: &'d Arc<Document>, is_template: bool) -> (std::borrow::Cow<'d, Document>, bool) {
    if !is_template || s.stencils().open_builtins() == template::stencils_opened_by(doc) {
        return (std::borrow::Cow::Borrowed(&**doc), false);
    }
    let mut copy = Document::clone(doc);
    let not_recorded = !template::set_stencil_names(&mut copy, s.stencils().open_builtins());
    if not_recorded { (std::borrow::Cow::Borrowed(&**doc), true) } else { (std::borrow::Cow::Owned(copy), false) }
}

/// **The package data of the file a drawing stands on goes with the drawing**. A drawing opened from an `.odg` holds the package data of the file as it read it (the
/// generator, the thumbnail, the manifest's rows, `meta.xml`'s fields, `settings.xml`, the entries nothing reads), and a splice save writes some of it anew, so the model is not the
/// file's reading of itself, and neither is an earlier version of the drawing that undo brings back. A model writer is given the package data of the file **as it is now**: the one
/// the drawing was opened from, or the one the last save wrote (`diacraft_odg::read_package_data`, put on the drawing it writes), whatever version of the drawing that is. A drawing that
/// stands on no `.odg` (a VSDX file, a `.diacraft`, a new drawing, a flat drawing) is given as it is. A file that cannot be read again beside the drawing refuses the save, in words,
/// with nothing written.
fn with_file_package_data<'d>(
    cmd: &str,
    source: Option<&OpenFile>,
    to_write: std::borrow::Cow<'d, Document>,
) -> Result<std::borrow::Cow<'d, Document>> {
    let Some(data) = source.map(crate::odgfile::file_package_data).transpose().map_err(|words| failed(cmd, words))?.flatten() else {
        return Ok(to_write);
    };
    // **What the Save As that wrote the file left out is not the file's to take from the drawing**: the file lacks the parts of the macro project and of the objects it was asked to leave
    // out, and the package data replaces the drawing's parts with the file's. The drawing keeps them.
    let asked = source.and_then(OpenFile::odg).map(|state| state.left_out().asked).filter(|asked| !asked.is_empty());
    let before = asked.map(|_| to_write.parts.clone());
    let mut copy = to_write.into_owned();
    data.put(&mut copy);
    if let (Some(asked), Some(before)) = (asked, before) {
        across::dropped::keep_parts(&mut copy, &before, asked);
    }
    Ok(std::borrow::Cow::Owned(copy))
}

/// What a save writes, by the name it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// `.diacraft`, `.diacraft-stencil` or `.diacraft-template`.
    Diacraft(NativeKind),
    /// One of the six VSDX types.
    Vsdx(Variant),
    /// An OpenDocument drawing (`.odg`) or drawing template (`.otg`), by the kind the name says.
    Odg(diacraft_odg::Variant),
    /// A name that is no type of any of them.
    Other,
}

impl Target {
    /// The type a file name's extension names, in any case.
    fn of(extension: Option<&str>) -> Target {
        let Some(extension) = extension else { return Target::Other };
        if let Some(kind) = nativefile::kind_of_extension(extension) {
            Target::Diacraft(kind)
        } else if let Some(variant) = odg_variant_of_extension(extension) {
            Target::Odg(variant)
        } else {
            Variant::from_extension(extension).map_or(Target::Other, Target::Vsdx)
        }
    }
}

/// The OpenDocument type an extension (without its dot) names, in any case: `.odg` a drawing, `.otg` a template.
pub(crate) fn odg_variant_of_extension(extension: &str) -> Option<diacraft_odg::Variant> {
    if extension.eq_ignore_ascii_case("odg") {
        Some(diacraft_odg::Variant::Drawing)
    } else if extension.eq_ignore_ascii_case("otg") {
        Some(diacraft_odg::Variant::Template)
    } else {
        None
    }
}

/// Why a save is refused before anything is made: what the drawing is, and what it was asked to be. (A `.diacraft` drawing and a new one are saved as an `.odg` or an `.otg` since
/// wired the model writer, which lifted Call 18's refusal of them.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// A stencil is no OpenDocument file: OpenDocument has no stencil format.
    StencilToOdg,
    /// A name that is no type, for a drawing of this working format.
    NoType(Option<FileFormat>),
}

/// The path a save takes (the design's 4.1): the two writers that splice into the file the drawing was opened from, as before, the model writers that write the file from the drawing
/// alone, and the refusals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SavePath {
    /// DiaCraft's own format, a package written from the model. It copies what did not change from the package the drawing was read from or last saved as.
    Native(NativeKind),
    /// A VSDX file, spliced into the bytes the drawing was opened from, as it always was (a new drawing has none, and the writer makes the skeleton).
    VsdxSplice(Variant),
    /// A VSDX file written from the model of a `.diacraft` drawing, by the VSDX model writer: there is no file to splice into.
    VsdxModel,
    /// An `.odg` or `.otg`, spliced into the bytes the drawing was opened from, in the kind a Save As names (a Save keeps the file's own).
    OdgSplice(diacraft_odg::Variant),
    /// An `.odg` or `.otg` written from the model of a `.diacraft` drawing or a new one by the `.odg` model writer (wired): there is no file to splice into.
    OdgModel(diacraft_odg::Variant),
    /// Refused in words, and nothing is made.
    Refused(Why),
}

/// **The path of a save** (the design's 4.1; Calls 15, 16 and 18). Pure: what the drawing's working file is (`None` for a new drawing and one made from a template), whether the drawing is
/// a stencil, and what the name says to write.
///
/// | Working file | Target | Path |
/// |---|---|---|
/// | `.diacraft` | `.diacraft` (any kind) | `Native` |
/// | `.diacraft` | a VSDX type | `VsdxModel` |
/// | `.diacraft` | `.odg` or `.otg` | `OdgModel` |
/// | VSDX | a VSDX type, or `.diacraft` | `VsdxSplice`, `Native` |
/// | `.odg` or `.otg` | `.odg`, `.otg`, `.diacraft` | `OdgSplice`, `Native` |
/// | VSDX | `.odg` or `.otg` | `OdgModel` (a save across) |
/// | `.odg` or `.otg` | a VSDX type | `VsdxModel` (a save across) |
/// | a new drawing (NP10: no file's bytes) | a VSDX type, `.diacraft`, `.odg` or `.otg` | `VsdxModel`, `Native`, `OdgModel` |
/// | a stencil | `.odg` or `.otg` | refused: OpenDocument has no stencil format |
pub(crate) fn save_path(working: Option<FileFormat>, stencil: bool, target: Target) -> SavePath {
    match (working, target) {
        (_, Target::Odg(_)) if stencil => SavePath::Refused(Why::StencilToOdg),
        (_, Target::Other) => SavePath::Refused(Why::NoType(working)),
        (Some(FileFormat::Diacraft), Target::Diacraft(kind)) => SavePath::Native(kind),
        // A `.diacraft` drawing has no VSDX file to splice into, and neither has a new drawing (NP10: it keeps no template's bytes): both are written from the model.
        (Some(FileFormat::Diacraft) | None, Target::Vsdx(_)) => SavePath::VsdxModel,
        (Some(FileFormat::Diacraft) | None, Target::Odg(variant)) => SavePath::OdgModel(variant),
        (Some(FileFormat::Vsdx), Target::Vsdx(variant)) => SavePath::VsdxSplice(variant),
        (Some(FileFormat::Vsdx) | None, Target::Diacraft(kind)) => SavePath::Native(kind),
        // **A save across the two formats** (the design's section 6): the drawing's working file is the other format, so there is no file of this format to splice into, and
        // the model writer of the target writes the file from the model, with what the target cannot say riding in its carrier.
        (Some(FileFormat::Vsdx), Target::Odg(variant)) => SavePath::OdgModel(variant),
        (Some(FileFormat::Odg), Target::Odg(variant)) => SavePath::OdgSplice(variant),
        (Some(FileFormat::Odg), Target::Diacraft(kind)) => SavePath::Native(kind),
        (Some(FileFormat::Odg), Target::Vsdx(_)) => SavePath::VsdxModel,
    }
}

/// The VSDX types a name may end in, for the words of a refusal.
const VSDX_TYPES: &str = ".vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm";

/// Every type a name may end in, for the words of a refusal: DiaCraft's own, the VSDX types and the OpenDocument drawing.
const ALL_TYPES: &str = ".diacraft, .diacraft-stencil, .diacraft-template, .vsdx, .vsdm, .vssx, .vssm, .vstx, .vstm, .odg or .otg";

/// The words of a refusal of a save, in two forms: the words `file.save_as` gives (the name is wrong, a bad parameter) and the words `file.save` gives, which takes no parameter and has a
/// file of its own already (a failure that says to use `file.save_as`), or `None` where the words are the same.
fn save_words(why: Why) -> (String, Option<String>) {
    let odg_name = "this drawing's file name must end in .odg or .otg: use file.save_as to give it one";
    match why {
        Why::StencilToOdg => {
            ("a stencil is not saved as an .odg: OpenDocument has no stencil format. Save it as .diacraft-stencil, .vssx or .vssm".to_owned(), None)
        }
        // A drawing opened from an `.odg` has a name that is no type: Save As takes every type (a save across the formats is written), and Save, which keeps the file's own format,
        // says what the file's name must end in.
        Why::NoType(Some(FileFormat::Odg)) => (format!("the file name must end in {ALL_TYPES}"), Some(odg_name.to_owned())),
        Why::NoType(Some(FileFormat::Vsdx)) => (
            format!("the file name must end in {ALL_TYPES}"),
            Some(format!("the drawing's file name must end in {VSDX_TYPES}: use file.save_as to give it one")),
        ),
        // A new drawing and a `.diacraft` drawing have no file name to be wrong for `file.save` (a `.diacraft` file is saved as the kind it is, and a new drawing has no file yet): the words are
        // `file.save_as`'s.
        Why::NoType(_) => (format!("the file name must end in {ALL_TYPES}"), None),
    }
}

/// The refusal of a save, in words that name what is missing and the way out ([`save_words`]): a bad parameter for `file.save_as`, whose name is wrong, and a failure for `file.save`.
fn refused_save(cmd: &str, why: Why, by_extension: bool) -> crate::EngineError {
    let (words, own_file) = save_words(why);
    if by_extension { bad(cmd, words) } else { failed(cmd, own_file.unwrap_or(words)) }
}

/// The save of a drawing as a `.diacraft` file (`nativefile::save`): the bytes are made whole in memory, and what the package cannot hold is refused in its words, leaving the disk and the
/// drawing as they were; then they go to a temporary file beside the target and are renamed over it. Only then is the session told the drawing is saved, and it keeps the package just written
/// with what the next save copies from it: **an unchanged drawing saved again is the same file**.
/// → `{bytes, variant, kind, format, macros_dropped: 0, dirty: false, copied, warnings, warning_count}`: `variant` is the extension of the type written (`diacraft`, `diacraft-stencil` or
/// `diacraft-template`), `format` the version of the format (`1.1`) and `copied` how many entries of the package were copied as they were from the file the drawing was read from or last saved as.
pub(crate) fn save_native(s: &mut Session, cmd: &str, path: &Path, target: std::path::PathBuf, kind: NativeKind) -> Result<Value> {
    let crate::session::SaveInputs { doc, source, .. } = s.save_inputs().ok_or_else(|| failed(cmd, "no document open"))?;
    // A template keeps the built-in stencils open at that moment, as a VSDX template does.
    let (to_write, not_recorded) = with_template_stencils(s, &doc, kind == NativeKind::Template);
    let edited = s.is_dirty() || matches!(to_write, std::borrow::Cow::Owned(_));
    // A drawing that stands on an `.odg` takes the package data of that file as it is now: it is no edit of the drawing.
    let to_write = with_file_package_data(cmd, source.as_ref(), to_write)?;
    let took_file_data = took_the_files_package_data(source.as_ref(), matches!(to_write, std::borrow::Cow::Owned(_)));
    let saved = nativefile::save::save(cmd, s, &to_write, source.as_ref(), kind, edited)?;
    // **The check after the save**: a `.diacraft` file written from a drawing that came from a file of another format is read back and compared with the drawing. One written from
    // a `.diacraft` drawing, or a new one, is not: it holds the whole model, and copies what did not change. **A drawing made from a template file** has no file of its own,
    // and came from the VSDX or the `.odg` reader all the same: it is checked as one opened from such a file is.
    let checked =
        native_save_checked(source.as_ref().and_then(OpenFile::format), &to_write).then(|| check::check_native(&to_write, &saved.bytes, false));
    write_target(&target, &saved.bytes).map_err(|e| failed(cmd, format!("the drawing could not be saved: {e}")))?;
    let mut said: Vec<String> = Vec::new();
    if not_recorded {
        said.push(
            "the stencils open now were not recorded in the template: the file holds DiaCraft data of a version this build does not read, which it keeps as it is".to_owned(),
        );
    }
    said.extend(saved.warnings.iter().cloned());
    let warnings: Vec<String> = said.iter().take(WARNINGS_SHOWN).cloned().collect();
    let result = json!({
        "bytes": saved.bytes.len(),
        "variant": nativefile::variant_word(kind),
        "kind": kind.word(),
        "format": format!("{}.{}", diacraft_native::FORMAT.0, diacraft_native::FORMAT.1),
        "macros_dropped": 0,
        "dirty": false,
        "carried": Carried::none().to_json(),
        "checked": checked_json(checked.as_ref()),
        "copied": saved.copied,
        "warnings": warnings,
        "warning_count": said.len(),
    });
    // The file as it is now, and where the path led: the next save compares with this, and writes there. The drawing's model is the one that was saved: no second model is kept (the
    // baseline shares what the drawing holds), so the process counts it once.
    let stamp = FileStamp::of_path(&target);
    let led_to = (target != path).then_some(target);
    let file = OpenFile::new(path, saved.bytes).with_stamp(stamp).with_target(led_to).with_native(saved.state);
    // **The drawing leaves the `.odg` it stood on as the model the file holds**: the file was written with the package data of the `.odg` as it is now, which the drawing did not hold
    // (a splice into the `.odg` writes some of it anew), and the next save, which stands on this file, wrote the drawing's own: other bytes with no edit. The very model written becomes the
    // drawing, no undo step, so that the next save copies every entry of it.
    if took_file_data {
        let written = to_write.into_owned();
        let baseline = Document::clone(&written);
        s.adopt_saved_with(written, file, baseline);
    } else {
        let baseline = Document::clone(&doc);
        s.mark_saved(&doc, file, baseline);
    }
    Ok(result)
}

/// Whether a save was given the package data of the `.odg` the drawing stands on ([`with_file_package_data`]) rather than the drawing as it is: what it was given is a copy (`copied`), for a drawing
/// whose working file is an `.odg`.
fn took_the_files_package_data(source: Option<&OpenFile>, copied: bool) -> bool {
    copied && source.and_then(OpenFile::format) == Some(FileFormat::Odg)
}

/// The arithmetic of [`check_save_peak`] for a session that holds `held` bytes and a copy of `copy`: what the save holds beside the hold (the peak less it), or, for a peak past the address
/// space less its sixty-fourth, the figures of the refusal: what the drawing and the copy are, the peak and the most a save may hold.
pub(crate) fn save_peak(held: u64, copy: u64) -> std::result::Result<u64, (u64, u64, u64)> {
    let need = held.saturating_add(copy);
    let peak = (need / 7).saturating_mul(8).saturating_add(350 << 20);
    let space = diacraft_sheetxml::model::ADDRESS_SPACE_BYTES;
    let most = space - space / 64;
    // What the save holds beside the session's hold, which the threads of the save are held beside (`crate::threads`).
    if peak <= most { Ok(peak.saturating_sub(held)) } else { Err((need, peak, most)) }
}

/// **The peak of a save of a `.vsdx`**: the writer makes the file and **reads it back to check it**, which holds a second copy of the drawing beside the first,
/// so the save holds what the session holds (the drawing, its steps and the model the last save kept) and the copy that the read-back makes. Measured on the reader's own diagram shapes
/// (14 cells, 2 formulas, a path of 5 rows, a text), a save peaks at 1.14 times the sum of the two as the floor counts them and 350 MiB (60,000 shapes over three pages: 861 MiB held, 2,311
/// MiB at the peak; 76,000: 1,090 MiB, 2,833 MiB). A drawing whose peak would pass the address space (less its sixty-fourth, 2,883 MiB: main saves 76,000 of these shapes at 2,894 MiB, and the check refuses from 78,000, where main aborts) is refused **before anything is made**, in words, and not
/// left to abort the process. **Gives what the save holds beside what the session holds** (the peak less the session's hold: the copy and the writer's own), beside which the threads of the
/// save are held to the address space where the process has a limit ([`crate::threads::save_threads_within`]: a drawing near the edge of what a save may hold is saved on one thread).
fn check_save_peak(cmd: &str, s: &Session, doc: &Document) -> Result<u64> {
    let held = s.model_held().unwrap_or(0);
    let copy = diacraft_sheetxml::model::held_bytes_of([doc], u64::MAX);
    let (need, peak, most) = match save_peak(held, copy) {
        Ok(beside) => return Ok(beside),
        Err(figures) => figures,
    };
    Err(failed(
        cmd,
        format!(
            "the drawing could not be saved: the save reads the file it makes back to check it, which holds a second copy of the drawing beside the first (the drawing and the copy are {} MiB \
             of model, and the save would hold about {} MiB, more than the {} MiB a save may hold); remove shapes or pages, or save the drawing in parts",
            need >> 20,
            peak >> 20,
            most >> 20
        ),
    ))
}

/// The save of a drawing that came from an `.odg` (`odgfile::save`): the bytes are made whole in memory and the writer reads them back before they are
/// returned, so a change it cannot write is refused in its own words and leaves the disk and the drawing as they were; then they go to a temporary
/// file beside the target and are renamed over it. Only then is the session told the drawing is saved, and it keeps the file just written with the
/// index the writer gave for it: the next save compares the drawing with that, and an unchanged one is no edit.
/// → `{bytes, variant: "odg" | "otg", kind: "drawing" | "template", form: "package" | "flat", edited, macros_dropped: 0, dirty: false, warnings, warning_count, notes}`: `notes` is the save note of design
/// decision 8 (`odgfile/note.rs`), an array of `{class, words, cross_shape_formulas, shape_data}`, empty when the drawing holds nothing a LibreOffice save would drop; it is not a warning.
///
/// **The kind of document**. `wanted` is the kind the extension of a Save As names (`.odg` a drawing, `.otg` a template), `None` for a Save, which keeps the kind the
/// package has. The kind is asked of the writer only when it is not the kind the package says it is (a package saved as the kind it is is saved as it was, byte for byte when nothing
/// changed): the writer changes the media type in the two places a package states it and nothing else, and a package with no place to state the other kind is refused in words.
fn save_odg(
    s: &mut Session,
    cmd: &str,
    path: &Path,
    target: std::path::PathBuf,
    wanted: Option<diacraft_odg::Variant>,
    options: Options,
) -> Result<Value> {
    let crate::session::SaveInputs { doc, source, baseline } = s.save_inputs().ok_or_else(|| failed(cmd, "no document open"))?;
    let source = source.ok_or_else(|| internal(cmd, "an .odg drawing with no file"))?;
    let own = diacraft_odg::sniff(source.bytes());
    let ask = wanted.filter(|kind| Some(*kind) != own);
    let mut saved = crate::odgfile::save::save(cmd, &doc, &source, s.is_dirty(), s.history(), ask, options)?;
    // A field made since the file was read is written as the text it shows: an `.odg` edited in place holds a field only where the file had one, and the save says so.
    if let Some(words) = crate::draw::fields::new_fields_note(&doc, baseline.as_deref()) {
        saved.warnings.push(words);
    }
    // The comments of the drawing: an `.odg` does not hold them yet, and the save says so.
    saved.warnings.extend(crate::draw::comments::odg_words(&doc));
    write_target(&target, &saved.bytes).map_err(|e| failed(cmd, format!("the drawing could not be saved: {e}")))?;
    // A flat drawing has no document file of DiaCraft's, which is where an `.odg` keeps the window settings: one whose settings were set is told so on every save.
    if source.is_flat() && doc.window_flags().is_some() {
        saved.warnings.push(crate::view::NOT_IN_A_FLAT_FILE.to_owned());
    }
    let warnings: Vec<String> = saved.warnings.iter().take(WARNINGS_SHOWN).cloned().collect();
    // What the package says now, which the writer's own check of what it made has settled.
    let kind = diacraft_odg::sniff(&saved.bytes).or(ask).or(own).unwrap_or(diacraft_odg::Variant::Drawing);
    let result = json!({
        "bytes": saved.bytes.len(),
        "variant": kind.extension(),
        "kind": odg_kind_word(kind),
        "form": if source.is_flat() { "flat" } else { "package" },
        "edited": saved.edited,
        "macros_dropped": 0,
        "dirty": false,
        "carried": saved.carried.to_json(),
        // A save into the file the drawing was opened from is not checked in 0.2: `null`.
        "checked": Value::Null,
        "warnings": warnings,
        "warning_count": saved.warnings.len(),
        "notes": saved.notes,
    });
    let stamp = FileStamp::of_path(&target);
    let led_to = (target != path).then_some(target);
    let baseline = Document::clone(&doc);
    let file = OpenFile::new(path, saved.bytes).with_stamp(stamp).with_target(led_to).with_odg(saved.state).with_extension(Some(kind.extension()));
    s.mark_saved(&doc, file, baseline);
    Ok(result)
}

/// The save of a drawing that has no `.odg` to splice into as an `.odg` or an `.otg` (the design's 4.1): a `.diacraft` drawing, or a new one. The bytes are made whole in memory by the
/// model writer (`odgfile::save::save_model`: `diacraft_odg::write_with(doc, None, ..)`, the thumbnail and the generator following the content stamp), which reads them back before they are
/// returned, so a drawing it cannot write is refused in words with nothing written; then they go to a temporary file beside the target and are renamed over it. Only then is the session told the
/// drawing is saved, and **the file written is the drawing's own** from then on: it keeps the file just written with the index the writer gave, so the next save splices into it and an unchanged
/// drawing is no edit.
/// → `{bytes, variant: "odg" | "otg", kind: "drawing" | "template", form: "package", edited, macros_dropped: 0, dirty: false, warnings, warning_count, notes}`, as `file.save` gives it for an `.odg`:
/// `edited` is whether the content is not what a file it was read from held (a drawing made new has no such file, and is edited).
fn save_odg_model(
    s: &mut Session,
    cmd: &str,
    path: &Path,
    target: std::path::PathBuf,
    variant: diacraft_odg::Variant,
    options: Options,
) -> Result<Value> {
    let crate::session::SaveInputs { doc, source, .. } = s.save_inputs().ok_or_else(|| failed(cmd, "no document open"))?;
    // A drawing that stands on an `.odg` (a Save As of it to another `.odg` that leaves something out) takes the package data of that file as it is now, as every save from the model of one does, and
    // what the file was written without stays out of the file written.
    let working = source.as_ref().filter(|f| f.format() == Some(FileFormat::Odg));
    let earlier = working.and_then(OpenFile::odg).map(|state| state.left_out().asked).unwrap_or_default();
    let choice = earlier.union(options);
    let given_doc = match working {
        Some(file) => Arc::new(with_file_package_data(cmd, Some(file), std::borrow::Cow::Borrowed(&*doc))?.into_owned()),
        None => Arc::clone(&doc),
    };
    let mut saved = crate::odgfile::save::save_model(cmd, s, &given_doc, variant, choice)?;
    // The comments of the drawing: an `.odg` does not hold them yet, and the save says so.
    saved.warnings.extend(crate::draw::comments::odg_words(&doc));
    write_target(&target, &saved.bytes).map_err(|e| failed(cmd, format!("the drawing could not be saved: {e}")))?;
    let warnings: Vec<String> = saved.warnings.iter().take(WARNINGS_SHOWN).cloned().collect();
    let result = json!({
        "bytes": saved.bytes.len(),
        "variant": variant.extension(),
        "kind": odg_kind_word(variant),
        "form": "package",
        "edited": saved.edited,
        "macros_dropped": saved.macros_dropped,
        "dirty": false,
        "carried": saved.carried.to_json(),
        "checked": checked_json(saved.checked.as_ref()),
        "warnings": warnings,
        "warning_count": saved.warnings.len(),
        "notes": saved.notes,
    });
    // The file as it is now, and where the path led: the next save compares with this, and writes there. The content stamp the drawing has is the drawing's, and the file written keeps it
    // (`Session::mark_saved`).
    let stamp = FileStamp::of_path(&target);
    let led_to = (target != path).then_some(target);
    let file = OpenFile::new(path, saved.bytes).with_stamp(stamp).with_target(led_to).with_odg(saved.state).with_extension(Some(variant.extension()));
    // **The drawing leaves the `.odg` it stood on as the model the file was written from**, as a Save As to `.diacraft` does (`save_native`): the model writer was given the package data
    // of that `.odg` as it is now, with its parts, relationships and properties, which the drawing does not hold, and every save into the new file is given the drawing. A Save after a Save As with
    // `drop_objects` was an edit: the first file holds the VSDX object as its own, so the model given lacked the part and kept the relationship that named it, and the drawing has the part, which
    // `drop_objects` leaves out with the relationship. No undo step.
    if working.is_some() {
        let written = Arc::try_unwrap(given_doc).unwrap_or_else(|shared| Document::clone(&shared));
        let baseline = Document::clone(&written);
        s.adopt_saved_with(written, file, baseline);
    } else {
        let baseline = Document::clone(&doc);
        s.mark_saved(&doc, file, baseline);
    }
    Ok(result)
}

/// Whether a save as `.diacraft` is checked: the drawing's working file is a VSDX file or an `.odg`, or it has none and **holds the data of one** (a drawing
/// made from a `.vstx`, `.vstm` or `.otg` template, or from a built-in template, which is a `.vstx`): it came from that format's reader. A `.diacraft` drawing, and one made new, which holds no
/// other format's data, are not checked: the file holds the whole model.
fn native_save_checked(working: Option<FileFormat>, doc: &Document) -> bool {
    match working {
        Some(FileFormat::Vsdx | FileFormat::Odg) => true,
        Some(FileFormat::Diacraft) => false,
        None => {
            doc.ext.get().is_some()
                || doc.parts.iter().any(|p| matches!(p.format, diacraft_doc::Format::Vsdx | diacraft_doc::Format::Odf))
                || doc.pages.iter().any(|p| p.ext.get().is_some())
        }
    }
}

/// The save result's `checked` field: what the check after a save found, or `null` for a save that is not checked (a save into the drawing's own file, and one that is no model writer's).
fn checked_json(checked: Option<&check::Outcome>) -> Value {
    checked.map_or(Value::Null, check::Outcome::to_json)
}

/// The word for an OpenDocument package's kind, as the VSDX types' are (`drawing`, `template`).
fn odg_kind_word(kind: diacraft_odg::Variant) -> &'static str {
    match kind {
        diacraft_odg::Variant::Drawing => "drawing",
        diacraft_odg::Variant::Template => "template",
    }
}

/// → the document: `{title, version, pages: [{id, name}], history: {undo, redo}}`.
fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    describe("document.inspect", s)
}

/// The longest path a command takes, in bytes: no system names a longer one.
const MAX_PATH_BYTES: usize = 4_096;

/// Replaces the document, if any, with a new one and a fresh history, and does not go on the history:
/// there is no save yet, so nothing the old document held can be lost to a step that undo could not bring back.
///
/// **Blank**, when `template` is left out: one page and the root style, and every built-in stencil open. **From a template**:
/// `template` is the id of a built-in template (`template.list`: it is made into the bytes of a `.vstx` and read as a template file is, so there is one
/// way to start from a template) or the path of a template file (`.diacraft-template`, `.vstx` or `.vstm`, read as `file.open` reads a file; a drawing or a stencil is refused,
/// in words). The new drawing is the template's content with its page setup, styles and theme, **unsaved and with no file** (`file.save` says to use
/// `file.save_as`), **keeping its model and no file's bytes** (NP10: the package it was read from is let go, and the model holds every part of it), so its first save is `.diacraft`
/// (the window's default) or, by Save As to a VSDX type, written from the model; and **the stencils the template names are open**
/// A template that cannot be read leaves the open document as it is.
/// → the new document, as `document.inspect`; and, when a template was used, `template` (`{id, name, builtin: true}` or `{id: null, name: null,
/// builtin: false}`), `stencils` (`{open: [ids], ignored}`: the built-in stencils now open, and how many names in the template were not opened),
/// `warnings`, and for a template file `file`: `{variant, kind, bytes, warning_count, skipped_count}`.
fn new(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.new";
    // Before any work: a batch cannot hold a new document, and a template would otherwise be made, or a file read, before `replace_document` says so.
    if s.in_group() {
        return Err(failed(ID, format!("`{ID}` cannot run inside a batch or other undo group")));
    }
    let started = match optional_text(p, "template", ID)? {
        None => {
            s.replace_document(ID, crate::snap::blank_drawing())?;
            s.stencils_mut().reset_builtins();
            None
        }
        Some(key) => Some(start_from(s, ID, key)?),
    };
    let mut result = describe(ID, s)?;
    // A blank drawing is answered as it always was; the keys below are there when a template was used.
    if let Some(started) = started {
        result["template"] = started.template;
        result["stencils"] = stencils_json(s, started.ignored);
        result["warnings"] = json!(started.warnings);
        if let Some(file) = started.file {
            result["file"] = file;
        }
    }
    Ok(result)
}

/// What starting a drawing from a template told.
struct Started {
    template: Value,
    /// How many stencil names in the template were not opened (not a built-in id).
    ignored: usize,
    warnings: Vec<String>,
    file: Option<Value>,
}

/// The built-in stencils that are open, and how many names the template that opened them gave that were not opened.
fn stencils_json(s: &Session, ignored: usize) -> Value {
    json!({ "open": s.stencils().open_builtins(), "ignored": ignored })
}

/// Opens the built-in stencils a template names → how many names were not opened. `None`, a document with no
/// list in DiaCraft's part (or a file that is not a template), opens the stencils a new drawing has.
fn open_template_stencils(s: &mut Session, names: Option<template::StencilNames>) -> usize {
    match names {
        Some(names) => {
            s.stencils_mut().set_builtins(&names.opens());
            names.ignored
        }
        None => {
            s.stencils_mut().reset_builtins();
            0
        }
    }
}

/// Makes `doc`, read from a template, the document as **a new unsaved drawing**, and opens the stencils it names. **A new drawing keeps no file's bytes** (NP10):
/// the template's package is let go by the caller before this is called, and the model carries everything the package held (the parts the model has slots for, held whole). The drawing has no file,
/// so its first save is `.diacraft` (the window's default) or, by Save As to a VSDX type, the VSDX model writer's. → how many names were not opened.
fn begin_from_template(s: &mut Session, cmd: &str, doc: Document) -> Result<usize> {
    let names = template::stencil_names(&doc);
    s.replace_document(cmd, doc)?;
    Ok(open_template_stencils(s, names))
}

/// The words for names a template gave that were not opened.
fn ignored_words(ignored: usize) -> String {
    format!(
        "{ignored} stencil name{} in the template {} not opened: a name is opened only when it is the id of a built-in stencil, and is never followed to a file (open a stencil file with stencil.open)",
        if ignored == 1 { "" } else { "s" },
        if ignored == 1 { "was" } else { "were" },
    )
}

/// The template `key` names (an id, or a file) as a new drawing.
fn start_from(s: &mut Session, cmd: &str, key: &str) -> Result<Started> {
    if key.len() > MAX_PATH_BYTES {
        return Err(bad(cmd, format!("`template` is longer than {MAX_PATH_BYTES} bytes")));
    }
    if let Some(spec) = template::find(key) {
        let built = template::build(cmd, spec)?;
        // The package the template was read back from is let go: a new drawing keeps its model and no bytes (NP10).
        drop(built.bytes);
        let ignored = begin_from_template(s, cmd, built.document)?;
        let warnings = listed_warnings(None, &built.warnings, (ignored > 0).then(|| ignored_words(ignored)));
        return Ok(Started { template: json!({ "id": spec.id, "name": spec.name, "builtin": true }), ignored, warnings, file: None });
    }
    // Not an id: a path. A word with nothing of a path in it is most likely an id that is wrong, and is told so (`template::is_path`, the window's rule too).
    if !template::is_path(key) {
        return Err(bad(cmd, format!("no template `{}` (see template.list), and to start from a file give its path", shown(key))));
    }
    // The room for the file is taken **before it is loaded**, and the file is held to it: two reads at once do not load two files beside a budget a read already holds.
    // (The reservation is bound before the bytes, so it is dropped after them: the room is given back to the next read only once the file is out of memory.)
    let (reservation, _stamp, bytes) = load_for_read(s, cmd, Path::new(key), || crate::memory::reserve_read_waiting(crate::memory::read_wait()))?;
    // The file's own bytes decide: a `.diacraft` file is read by its own reader, under the same reservation of the budget as a VSDX read takes.
    if diacraft_native::sniff(&bytes).is_some() {
        return start_from_native(s, cmd, key, &bytes, &reservation);
    }
    // An OpenDocument drawing template is started from by its own reader (the package's own type decides, as for `file.open`).
    if let Some(variant) = diacraft_odg::sniff(&bytes) {
        return start_from_odg(s, cmd, key, bytes, variant);
    }
    // The reservation of the read is kept until the template is held (the session's file makes its own hold): `_reading`.
    let (doc, report) = read_package(s, cmd, &bytes, &reservation)?;
    match report.variant {
        Some(v) if v.kind == Kind::Template => {}
        Some(v) => {
            return Err(failed(
                cmd,
                format!("this is a {}, not a template: open it with file.open, or choose a .vstx or .vstm file", v.kind_word_macros()),
            ));
        }
        None => return Err(failed(cmd, "this is not a template: choose a .vstx or .vstm file")),
    }
    let mut warning_count: u64 = report.warning_counts.values().sum();
    let extension_note = Path::new(key).extension().and_then(|e| e.to_str()).and_then(|extension| check_extension(report.variant, extension));
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let (variant, size, skipped_count) = (report.variant.map(|v| v.extension()), bytes.len(), report.skipped_counts.values().sum::<u64>());
    // A new drawing keeps no file's bytes (NP10): the template's package is let go before the drawing is made, and not held beside it until the command ends.
    drop(bytes);
    let ignored = begin_from_template(s, cmd, doc)?;
    // The words about names that were not opened are one more warning, and are counted with the others (N3 of the re-review).
    let ignored_note = (ignored > 0).then(|| ignored_words(ignored));
    if ignored_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    // The notes are listed with the reader's, at most `WARNINGS_SHOWN` in all, and none of them is cut.
    let warnings = listed_warnings(extension_note, &report.warnings, ignored_note);
    let file = json!({ "variant": variant, "kind": "template", "bytes": size, "warning_count": warning_count, "skipped_count": skipped_count });
    Ok(Started { template: json!({ "id": null, "name": null, "builtin": false }), ignored, warnings, file: Some(file) })
}

/// The template file `key` (an `.otg`, told by its own type) as a new drawing (harvested): read by the `odg` reader, and **a new drawing keeps no file's bytes**
/// (NP10): the model holds everything the package held, so the first save is `.diacraft` (the window's default) or an `.odg` or `.otg` written from the model. A drawing is refused in words,
/// and so is the flat form of a template. A template package names the stencils it opens in DiaCraft's own data when it holds a list, and every built-in stencil is open otherwise.
fn start_from_odg(s: &mut Session, cmd: &str, key: &str, bytes: Vec<u8>, variant: diacraft_odg::Variant) -> Result<Started> {
    if variant != diacraft_odg::Variant::Template {
        return Err(failed(
            cmd,
            "this is an OpenDocument drawing, not a template: open it with file.open, or choose a .diacraft-template, .otg, .vstx or .vstm file",
        ));
    }
    refuse_a_flat_template(cmd, &bytes)?;
    let limits = diacraft_odg::Limits { threads: crate::open_threads::open_threads(), ..diacraft_odg::Limits::default() };
    let (doc, report) = crate::odgfile::read_odg(&bytes, &limits).map_err(|e| cannot_open(cmd, odg_problem(&e), odg_refused(&e)))?;
    let mut warning_count: u64 = report.warning_counts.values().sum();
    let extension_note = odg_extension_note(key, variant);
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let (size, skipped_count) = (bytes.len(), report.kept.values().sum::<u64>());
    // A new drawing keeps no file's bytes: the package is let go before the drawing is made.
    drop(bytes);
    let ignored = begin_from_template(s, cmd, doc)?;
    let ignored_note = (ignored > 0).then(|| ignored_words(ignored));
    if ignored_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let warnings = listed_warnings(extension_note, &report.warnings, ignored_note);
    let file =
        json!({ "variant": variant.extension(), "kind": "template", "bytes": size, "warning_count": warning_count, "skipped_count": skipped_count });
    Ok(Started { template: json!({ "id": null, "name": null, "builtin": false }), ignored, warnings, file: Some(file) })
}

/// A `.diacraft-template` as a new drawing (`file.new {template: path}`): read as `file.open` reads one, **a new drawing keeps no file's bytes** (NP10), and the stencils it names are open. A
/// `.diacraft` drawing or stencil is refused in words, by its content and not its name.
fn start_from_native(s: &mut Session, cmd: &str, key: &str, bytes: &[u8], reservation: &Reservation) -> Result<Started> {
    let held = nativefile::open::held_by(s, reservation);
    let read = nativefile::read_native(bytes, &nativefile::open::limits_of(reservation))
        .map_err(|e| cannot_open(cmd, nativefile::open::problem(&e, &held), nativefile::open::refused(&e, &held, reservation.beside_reads())))?;
    let nativefile::Read { doc, opened, warnings: reader_warnings, .. } = read;
    if opened.kind != NativeKind::Template {
        return Err(failed(
            cmd,
            format!(
                "this is a DiaCraft {}, not a template: open it with file.open, or choose a .diacraft-template, .vstx or .vstm file",
                opened.kind.word()
            ),
        ));
    }
    let mut warning_count = u64::try_from(reader_warnings.len()).unwrap_or(u64::MAX);
    let extension_note = Path::new(key).extension().and_then(|e| e.to_str()).and_then(|extension| nativefile::extension_note(opened.kind, extension));
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let names = template::stencil_names(&doc);
    s.replace_document(cmd, doc)?;
    s.seed_ledger(&opened.state.ledger);
    let ignored = open_template_stencils(s, names);
    let ignored_note = (ignored > 0).then(|| ignored_words(ignored));
    if ignored_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let warnings = listed_warnings(extension_note, &reader_warnings, ignored_note);
    let file = json!({
        "variant": nativefile::variant_word(opened.kind),
        "kind": "template",
        "bytes": bytes.len(),
        "warning_count": warning_count,
        "skipped_count": opened.kept_parts,
    });
    Ok(Started { template: json!({ "id": null, "name": null, "builtin": false }), ignored, warnings, file: Some(file) })
}

/// Reads the file at `path` and makes it the document, with a fresh history, as `file.new` does: the old document is replaced, and not
/// put on the history, since M2 does not save. **A file that cannot be opened leaves the open document as it is**: the file is read whole
/// and checked before anything is replaced, and every way it can fail (no such file, a folder, no permission, too large, not a package,
/// damaged, past a limit of the reader) is an error with words for a person, the kind of trouble as a typed field
/// ([`crate::EngineError::CannotOpen`]), and no text from the file.
///
/// **The file's own bytes decide what it is**, in this order (the design's 1.2): a `.diacraft` file ([`open_native`]), an OpenDocument drawing ([`open_odg`]), then a VSDX package, and the extension
/// only gets a warning. **A drawing read from a VSDX file is fingerprinted once** (the content stamp, the design's 4.4: [`nativefile::stamp_of`]).
///
/// **The session keeps the file's bytes and its path** ([`Session::source`]): the model holds what it has slots for, the
/// reader counts the rest in `skipped_count`, and the writer of M3 takes every part it does not edit from the bytes. The bytes are the
/// file's size, held once behind an `Arc` while the document is open.
/// → the document as `document.inspect` gives it, and `file`: `{variant, bytes, warnings: [..first 20..], warning_count, skipped_count,
/// diacraft}`, where `diacraft` is `{state: "absent"}`, `{state: "loaded", version, children}` or `{state: "unreadable", why}`.
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.open";
    // Before any work: a batch cannot hold a new document, and the file would otherwise be read and checked before `replace_document_from` says so.
    if s.in_group() {
        return Err(failed(cmd, format!("`{cmd}` cannot run inside a batch or other undo group")));
    }
    let path = required_text(p, "path", cmd)?;
    let edit_template = flag(p, "edit_template", cmd)?;
    // The room for the file is taken **before it is loaded**: the file's size is looked at first (a path that names nothing is refused at once, and a file the room cannot
    // hold is refused from its size, in words, before a byte of it is read), the reservation of what is left of the budget is taken (waiting for a read that is running), and the file is
    // loaded held to it. Two large opens at the same moment no longer load two files beside a budget one of them holds.
    // **The reservation is bound before the bytes, so it is dropped after them** (locals are dropped in the reverse order they are bound): the room is given back to the next read only once
    // the file is out of memory, on every way out, errors included.
    let (reservation, stamp, bytes) = load_for_read(s, cmd, Path::new(path), || crate::memory::reserve_read_waiting(crate::memory::read_wait()))?;
    // The file's own bytes decide, not its extension (as for a VSDX file), and in this order (the design's 1.2): a `.diacraft` file first (neither of the others can claim one), then an
    // OpenDocument drawing, then a VSDX package.
    if diacraft_native::sniff(&bytes).is_some() {
        return open_native(s, cmd, path, bytes, stamp, edit_template, &reservation);
    }
    if let Some(variant) = diacraft_odg::sniff(&bytes) {
        // An `.odg` is read by its own reader within its own limits; the file it holds is held under the reservation all the same, which is given back after it.
        return open_odg(s, cmd, path, bytes, stamp, variant, edit_template);
    }
    // The read holds the memory it may use while it runs, and the session's file makes its own hold: the reservation is kept until it has (`_reading`, dropped when this ends).
    let (doc, report, as_read) = read_package_as_read(s, cmd, &bytes, &reservation)?;
    let size = bytes.len();
    let mut warning_count: u64 = report.warning_counts.values().sum();
    // The extension against what the package is. The bytes carry no name, so this is the one place that can say so.
    let extension_note = Path::new(path).extension().and_then(|e| e.to_str()).and_then(|extension| check_extension(report.variant, extension));
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let skipped_count: u64 = report.skipped_counts.values().sum();
    let variant = report.variant.map(|v| v.extension());
    let diacraft = match &report.diacraft {
        None | Some(DiacraftPart::Absent) => json!({ "state": "absent" }),
        Some(DiacraftPart::Loaded { version, children }) => json!({ "state": "loaded", "version": shown(version), "children": children }),
        Some(DiacraftPart::Unreadable(why)) => json!({ "state": "unreadable", "why": why }),
    };
    // A template opens as a new drawing made from it (leaves it to the command layer, and the file holds no flag):
    // the document is the template's content, the drawing keeps no file's bytes (NP10: the model holds every part the package held), and it has no file until `file.save_as` gives
    // it one. The package's own type decides, not the extension; a template opened to be changed (`edit_template`) is the file itself.
    let is_template = report.variant.is_some_and(|v| v.kind == Kind::Template);
    let from_template = !edit_template && is_template;
    // The stencils the template names are open in it, whether it is started from or opened to be changed, and a file that is
    // not a template has every built-in stencil open again.
    let ignored = if from_template {
        // A new drawing keeps no file's bytes (NP10): the template's package is let go before the drawing is made.
        drop(bytes);
        begin_from_template(s, cmd, doc)?
    } else {
        let names = if is_template { template::stencil_names(&doc) } else { None };
        // The file the path leads to now, if it is a link, is the file a save writes: resolved once, here.
        // **The content stamp** (the design's 4.4): the fingerprint of the model as it was read from this foreign file is **pending** here, and made by the first thing that
        // needs it, from the document the drawing was read as, which the pending stamp keeps (`Session::content_stamp`, `Session::replace_document_from` gives it the `Arc`): a Save As to
        // `.diacraft`, and the decision of a model writer once the drawing is not that document. **No open and no save of a VSDX file makes the pass** (it is a third of the open's time,
        // and a drawing that is only looked at, or only saved as VSDX, pays nothing). A named VSDX file is the one place it is armed: a template opened as a new drawing, and a new
        // drawing saved as VSDX, are not read from a foreign file.
        let source = OpenFile::new(Path::new(path), bytes)
            .with_stamp(Some(stamp))
            .with_target(resolved_link(Path::new(path)))
            .with_extension(report.variant.map(|v| v.extension()))
            .with_pending_content_stamp()
            .with_model_bytes(report.stats.memory_bytes);
        s.replace_document_from(cmd, doc, source)?;
        // The open broke a loop of containers: the file as it is read is the baseline, so a save into it writes the repaired ties.
        if let Some(read) = as_read {
            s.set_read_baseline(read);
        }
        open_template_stencils(s, names)
    };
    let ignored_note = (ignored > 0).then(|| ignored_words(ignored));
    if ignored_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    // The notes are listed with the reader's, at most `WARNINGS_SHOWN` in all, and none of them is cut.
    let warnings = listed_warnings(extension_note, &report.warnings, ignored_note);
    let mut result = describe(cmd, s)?;
    if is_template {
        result["stencils"] = stencils_json(s, ignored);
    }
    result["file"] = json!({
        "variant": variant,
        "kind": report.variant.map(|v| v.kind_word()),
        "new_from_template": from_template,
        "bytes": size,
        "warnings": warnings,
        "warning_count": warning_count,
        "skipped_count": skipped_count,
        "diacraft": diacraft,
    });
    Ok(result)
}

/// Opens a `.diacraft` file (`nativefile`): the package is read within what the process leaves of the memory budget, under the read's reservation (`nativefile::open::limits_of`,
/// never more than `READ_BUDGET`), the themes are read from their bytes,
/// the identifier ledger raises the session's marks, and the drawing keeps the package with what the next save copies from it. **A template is a new drawing made from it**, as a VSDX
/// template is, unless `edit_template` is true, and **a new drawing keeps no file's bytes** (NP10): the model carries everything. A file that cannot be opened leaves the open drawing as it is.
/// → the document as `document.inspect` gives it, and `file`: `{variant: "diacraft" | "diacraft-stencil" | "diacraft-template", kind, format: "1.1", new_from_template, bytes, warnings:
/// [..first 20..], warning_count, skipped_count, diacraft: {state: "absent"}}`; `skipped_count` is how many parts of a role this build does not know were kept as bytes.
fn open_native(
    s: &mut Session,
    cmd: &str,
    path: &str,
    bytes: Vec<u8>,
    stamp: FileStamp,
    edit_template: bool,
    reservation: &Reservation,
) -> Result<Value> {
    // The read holds what is left of the budget while it runs, as a VSDX read does, and the reservation is kept until the session's file has made its own hold.
    let held = nativefile::open::held_by(s, reservation);
    let read = nativefile::read_native(&bytes, &nativefile::open::limits_of(reservation))
        .map_err(|e| cannot_open(cmd, nativefile::open::problem(&e, &held), nativefile::open::refused(&e, &held, reservation.beside_reads())))?;
    let nativefile::Read { doc, opened, warnings: reader_warnings, model_bytes } = read;
    let (kind, size) = (opened.kind, bytes.len());
    let mut warning_count = u64::try_from(reader_warnings.len()).unwrap_or(u64::MAX);
    let extension_note = Path::new(path).extension().and_then(|e| e.to_str()).and_then(|extension| nativefile::extension_note(kind, extension));
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let is_template = kind == NativeKind::Template;
    let from_template = !edit_template && is_template;
    // The stencils the template names are open in it, whether it is started from or opened to be changed, and a file that is not a template has every built-in stencil open again.
    let names = if is_template { template::stencil_names(&doc) } else { None };
    if from_template {
        // A new drawing keeps no file's bytes: the package is let go before the drawing is made, not held beside it until the command ends.
        drop(bytes);
        s.replace_document(cmd, doc)?;
        // The template's ledger still stands (the design's 2.7): an `ID` that was a shape of one of its pages and is not now is not given to a shape of the new drawing.
        s.seed_ledger(&opened.state.ledger);
    } else {
        // The file the path leads to now, if it is a link, is the file a save writes: resolved once, here.
        let source = OpenFile::new(Path::new(path), bytes)
            .with_stamp(Some(stamp))
            .with_target(resolved_link(Path::new(path)))
            .with_content_stamp(opened.state.content_stamp)
            .with_model_bytes(model_bytes)
            .with_native(NativeState::new(kind, opened.decoded));
        s.replace_document_from(cmd, doc, source)?;
        s.seed_ledger(&opened.state.ledger);
    }
    let ignored = open_template_stencils(s, names);
    let ignored_note = (ignored > 0).then(|| ignored_words(ignored));
    if ignored_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    let warnings = listed_warnings(extension_note, &reader_warnings, ignored_note);
    let mut result = describe(cmd, s)?;
    if is_template {
        result["stencils"] = stencils_json(s, ignored);
    }
    result["file"] = json!({
        "variant": nativefile::variant_word(kind),
        "kind": kind.word(),
        "format": format!("{}.{}", opened.format.0, opened.format.1),
        "new_from_template": from_template,
        "bytes": size,
        "warnings": warnings,
        "warning_count": warning_count,
        "skipped_count": opened.kept_parts,
        "diacraft": { "state": "absent" },
    });
    Ok(result)
}

/// Opens an OpenDocument drawing or drawing template (`odgfile`): read by the `odg` crate, kept with the index that maps its shapes to its bytes, so that a save splices an
/// edit into them. **A drawing template (`.otg`) opens as a new drawing made from it** (`.otg` is a format to open and save, and no built-in template), as a VSDX template does:
/// the document is the template's content, the drawing keeps no file's bytes (NP10) and has no file until `file.save_as` gives it one. With `edit_template` the template file itself is opened,
/// to change it, and a save writes a template. **An `.odg` read from a file has its content stamp pending** (the design's 4.4: the fingerprint of the model as it was read, made when a
/// Save As to `.diacraft` or a model writer needs it), so that a drawing that visits `.diacraft` and comes back keeps the thumbnail and the generator it was read with.
/// → the document as `document.inspect` gives it, and `file`: `{variant: "odg" | "otg", kind: "drawing" | "template", form: "package" | "flat", new_from_template, bytes, warnings: [..first
/// 20..], warning_count, skipped_count, diacraft: {state: "absent"}}`, and, for a template, `stencils`. `skipped_count` is what the reader kept in the file's bytes without modelling it.
/// **A flat drawing (`.fodg`) is told by its content and not by its name** (`sniff`), is read as the package that holds the same elements, says `form: "flat"` and says first, among the
/// warnings, what Save does with it. A flat template is not opened.
fn open_odg(
    s: &mut Session,
    cmd: &str,
    path: &str,
    bytes: Vec<u8>,
    stamp: FileStamp,
    variant: diacraft_odg::Variant,
    edit_template: bool,
) -> Result<Value> {
    refuse_a_flat_template(cmd, &bytes)?;
    let limits = diacraft_odg::Limits { threads: crate::open_threads::open_threads(), ..diacraft_odg::Limits::default() };
    // The one way an `.odg` is read: the `odg` crate's read, and the themes the document file carries (`odgfile::read_odg`).
    let (doc, report) = crate::odgfile::read_odg(&bytes, &limits).map_err(|e| cannot_open(cmd, odg_problem(&e), odg_refused(&e)))?;
    let size = bytes.len();
    let is_template = variant == diacraft_odg::Variant::Template;
    let from_template = is_template && !edit_template;
    let mut warning_count: u64 = report.warning_counts.values().sum();
    let skipped_count: u64 = report.kept.values().sum();
    // A flat file (`.fodg`) is told by what it is, whatever it is called, and Save writes it back as a flat file: the person is told first, at the
    // open, what Save does with it and that Save As is not offered.
    let flat = report.form == diacraft_odg::Form::Flat;
    if flat {
        warning_count = warning_count.saturating_add(1);
    }
    // A name that says one OpenDocument type for a package that is the other is told, last: the package's own type decides, as for a VSDX file.
    let extension_note = odg_extension_note(path, variant);
    if extension_note.is_some() {
        warning_count = warning_count.saturating_add(1);
    }
    // The notes are listed with the reader's, at most `WARNINGS_SHOWN` in all.
    let warnings = listed_warnings(flat.then(|| crate::odgfile::FLAT_OPEN_NOTE.to_owned()), &report.warnings, extension_note);
    let ignored = if from_template {
        // A new drawing keeps no file's bytes (NP10): the template's package is let go before the drawing is made.
        drop(bytes);
        begin_from_template(s, cmd, doc)?
    } else {
        // The file the path leads to now, if it is a link, is the file a save writes: resolved once, here. **The content stamp** (the design's 4.4) is pending: the
        // fingerprint of the model as it was read is made when something needs it, from the document the drawing was read as. A flat drawing has one as a package has (plan
        // A Save As takes it to `.diacraft` and to the model writers, which keep the generator and write no new thumbnail while the drawing is what the file held).
        let model_bytes = report.index.read_cost().model_bytes();
        let source = OpenFile::new(Path::new(path), bytes)
            .with_stamp(Some(stamp))
            .with_target(resolved_link(Path::new(path)))
            .with_model_bytes(model_bytes)
            .with_odg(crate::odgfile::OdgState::new(report.index))
            .with_extension(Some(variant.extension()))
            .with_pending_content_stamp();
        s.replace_document_from(cmd, doc, source)?;
        // Like any drawing that is not a template with a list: every built-in stencil is open again.
        open_template_stencils(s, None)
    };
    let mut result = describe(cmd, s)?;
    if is_template {
        result["stencils"] = stencils_json(s, ignored);
    }
    result["file"] = json!({
        "variant": variant.extension(),
        "kind": odg_kind_word(variant),
        "form": if flat { "flat" } else { "package" },
        "new_from_template": from_template,
        "bytes": size,
        "warnings": warnings,
        "warning_count": warning_count,
        "skipped_count": skipped_count,
        "diacraft": { "state": "absent" },
    });
    Ok(result)
}

/// A flat drawing template is not opened (a template is the package `.otg`: a flat one has no reader's route of its own yet), in words that name no part of the file.
fn refuse_a_flat_template(cmd: &str, bytes: &[u8]) -> Result<()> {
    if diacraft_odg::Form::of(bytes) == diacraft_odg::Form::Flat && diacraft_odg::sniff(bytes) == Some(diacraft_odg::Variant::Template) {
        return Err(cannot_open(
            cmd,
            OpenProblem::NotAPackage,
            "this is a flat OpenDocument drawing template, which is not opened: open a template as a package (.otg), or a drawing (.odg or .fodg)",
        ));
    }
    Ok(())
}

/// What a person is told when a file's name says one OpenDocument type and the package is the other (`.odg` for a template, `.otg` for a drawing): the package's own type decides, as for a VSDX
/// file, and the name is only noted. A name that says neither (`.fodg`, none) is told nothing: the bytes decide.
fn odg_extension_note(path: &str, variant: diacraft_odg::Variant) -> Option<String> {
    let named = odg_variant_of_extension(Path::new(path).extension()?.to_str()?)?;
    let is = |kind: diacraft_odg::Variant| match kind {
        diacraft_odg::Variant::Drawing => "an OpenDocument drawing",
        diacraft_odg::Variant::Template => "an OpenDocument drawing template",
    };
    (named != variant).then(|| format!("the extension says {}, and the package is {}", is(named), is(variant)))
}

/// The kind of trouble a refusal by the `.odg` reader is.
fn odg_problem(e: &diacraft_odg::OdgError) -> OpenProblem {
    match e.kind() {
        diacraft_odg::ErrorKind::NotAPackage => OpenProblem::NotAPackage,
        diacraft_odg::ErrorKind::TooBig => OpenProblem::TooBig,
        diacraft_odg::ErrorKind::Damaged => OpenProblem::Damaged,
    }
}

/// What a person is told when the `.odg` is not a drawing we open: by what kind of trouble it is, and then the reader's own words, which hold numbers,
/// class words and part names escaped, and no content.
fn odg_refused(e: &diacraft_odg::OdgError) -> String {
    let how = match odg_problem(e) {
        OpenProblem::NotAPackage => "this is not a drawing file",
        OpenProblem::TooBig => "this file is too big to open safely",
        _ => "this file is damaged",
    };
    shown(&format!("{how} ({}): {e}", e.class()))
}

/// What a person is told when the file could not be read from disk. Nothing in it is the path.
pub(super) fn can_not_open(e: &FileError) -> String {
    format!("cannot open the file: {e}")
}

/// The kind of trouble a disk error is.
pub(super) fn file_problem(e: &FileError) -> OpenProblem {
    match e {
        FileError::NotFound => OpenProblem::NotFound,
        FileError::Denied => OpenProblem::Denied,
        FileError::NotAFile => OpenProblem::NotAFile,
        FileError::TooLarge { .. } => OpenProblem::FileTooLarge,
        FileError::OutOfMemory => OpenProblem::OutOfMemory,
        FileError::Failed => OpenProblem::Unreadable,
    }
}

/// The kind of trouble a refusal by the reader is.
pub(super) fn package_problem(e: &VsdxError) -> OpenProblem {
    match e.kind() {
        ErrorKind::NotAPackage => OpenProblem::NotAPackage,
        ErrorKind::TooBig => OpenProblem::TooBig,
        ErrorKind::Damaged => OpenProblem::Damaged,
    }
}

/// What a person is told when the file is not a drawing we open: by what kind of trouble it is, and then the reader's own words, which hold
/// numbers, class words and part names escaped, and no content.
pub(super) fn refused(e: &VsdxError) -> String {
    let how = match package_problem(e) {
        OpenProblem::NotAPackage => "this is not a drawing file",
        OpenProblem::TooBig => "this file is too big to open safely",
        _ => "this file is damaged",
    };
    shown(&format!("{how} ({}): {e}", e.class()))
}

/// What a file that was refused for memory beside what is held would have been refused as **alone**, as far as the report of the refusal can say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Alone {
    /// Too big for the whole budget too: closing what is open would not let it in.
    TooBig,
    /// It may be read alone: what is held is in the way (or the report cannot say how far the file goes).
    MayFit,
}

/// The share of the XML of the package that a refused read has taken in before its figures are taken for the whole: an eighth. A read that was refused before that has told too little to scale.
const SEEN_AT_LEAST: u64 = 8;

/// The estimate of what a file needs is held to be past the budget when it is more than this much past it: a thirty-second. The estimate is of a read that was cut short and is
/// no measure: on drawings of 50,000 and 60,000 shapes in pages that are alike, refused at a fifth to nine tenths of the way, it ran 4 to 24 percent under what a read of the same file
/// alone charged (the earlier the refusal, the further under; measured on beast, 2026-11-21), so it errs toward the file, and a file is told too big only when it is clearly past.
const CLEARLY_PAST: u128 = 32;

/// The inflated bytes of the parts of the package in `bytes` that the reader parses as XML (the parts and relationships named `.xml` and `.rels`), from the central directory, which is
/// read and nothing is inflated. `None` when the directory does not read: a refusal is then judged on the file and the package alone.
pub(super) fn xml_part_bytes(bytes: &[u8]) -> Option<u64> {
    let archive = diacraft_container::zip::Archive::read(bytes, &diacraft_container::Limits::default()).ok()?;
    let is_xml = |name: &str| {
        let name = name.as_bytes();
        [b".xml".as_slice(), b".rels".as_slice()]
            .iter()
            .any(|ext| name.len() >= ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext))
    };
    Some(archive.entries().iter().filter(|entry| !entry.folder && is_xml(&entry.name)).fold(0u64, |total, entry| total.saturating_add(entry.size)))
}

/// **Whether a file that was refused for memory beside what is held would be read alone**, from the report of the refusal ([`diacraft_vsdx::read_partial`]): the reader
/// does not say what a file needs, only how far it got. What a read holds before it builds anything is the file and the package inflated, so a file whose bytes and parts are the whole
/// budget are too big for it. Otherwise **only the XML is scaled**: what the read had charged for it when it was refused (`memory_peak` less the pictures it had kept), taken at the same
/// rate to the end of the XML of the package (`xml_package`, [`xml_part_bytes`]; the bytes of XML the read had taken in are `xml_bytes`), with the file and the package beside it and the
/// pictures it had kept **once, unscaled**: pictures that come after are not counted, since they are not known to be shown, and the figure is the lower. A read that was refused before an
/// eighth of the XML was in, and one whose XML is not known, say nothing, and are [`Alone::MayFit`]: **the doubt is for the file**, since a file told it is too big is a file given up.
pub(super) fn judged_alone(report: &ReadReport, xml_package: Option<u64>) -> Alone {
    let stats = &report.stats;
    let (file, package, budget) = (stats.file_bytes, stats.uncompressed_bytes, diacraft_vsdx::MAX_MEMORY);
    if file.saturating_add(package) >= budget {
        return Alone::TooBig;
    }
    let Some(xml_package) = xml_package.filter(|total| *total > 0) else { return Alone::MayFit };
    let seen = stats.xml_bytes.min(xml_package);
    if seen.saturating_mul(SEEN_AT_LEAST) < xml_package {
        return Alone::MayFit;
    }
    let pictures = stats.media_bytes;
    let charged_for_all = u128::from(stats.memory_peak.saturating_sub(pictures)) * u128::from(xml_package) / u128::from(seen);
    let needs = u128::from(file) + u128::from(package) + charged_for_all + u128::from(pictures);
    if needs * CLEARLY_PAST > u128::from(budget) * (CLEARLY_PAST + 1) { Alone::TooBig } else { Alone::MayFit }
}

/// [`package_problem`], for a read made beside what the process holds: a refusal for memory when something is held is that there is not enough memory beside it, **unless the file is too
/// big for the whole budget alone** (`alone`), when closing what is open would not help and the problem is the one it is without it (the window says which).
pub(super) fn problem_beside(e: &VsdxError, held: &crate::HeldMemory, alone: Alone) -> OpenProblem {
    if e.class() == "too_much_memory" && held.total() > 0 && alone == Alone::MayFit { OpenProblem::OutOfMemory } else { package_problem(e) }
}

/// [`refused`], for a read made beside what the process holds: a refusal for memory says that it is beside what is open, what is held and what to do (`HeldMemory`), or,
/// when a file is being read at this moment and holds the budget (`reads` bytes of it), to wait for that to finish; and **a file that is too big alone is told as it is told alone**.
pub(super) fn refused_beside(e: &VsdxError, held: &crate::HeldMemory, alone: Alone, reads: u64) -> String {
    if e.class() != "too_much_memory" || held.total() == 0 || alone == Alone::TooBig {
        return refused(e);
    }
    // What is held is told in the engine's own words and numbers, none of the file's, and is not cut as the reader's text is.
    let head = shown(&format!("not enough memory beside what is open ({}): {e}", e.class()));
    if reads > 0 {
        format!(
            "{head}: another file is being read, and holds {reads} of the {} bytes that reading a file may use at once until it has been read: wait for it to finish, and open this file again",
            diacraft_vsdx::MAX_MEMORY
        )
    } else {
        format!("{head}: {}", held.words())
    }
}

/// Loads the file at `path` **for a read that holds the room for it**: the answer is the bytes, how the file looked, and the reservation `take` made, to be given to the read.
///
/// 1. The size of the file is looked at first, with no wait: a path that names nothing, a folder or a device is refused at once (before a read that is running is waited for), and so is a file past
///    the package cap.
/// 2. The reservation is taken (`take`: `file.open` and a template wait for a read that is running, `stencil.open` waits for none). **A file whose size is more than what it holds is refused from
///    its size, in words, and not a byte of it is read**: `out_of_memory` beside what is held, or `too_big` when the file is more than the whole budget alone.
/// 3. The file is loaded held to what was reserved, so a file that grew meanwhile is refused too.
///
/// Before this, the file was loaded whole to the package cap before the read took or waited for its room, and two large opens at the same moment held two files beside a budget one of them held.
pub(super) fn load_for_read(s: &Session, cmd: &str, path: &Path, take: impl FnOnce() -> Reservation) -> Result<(Reservation, FileStamp, Vec<u8>)> {
    let unreadable = |e: FileError| cannot_open(cmd, file_problem(&e), can_not_open(&e));
    let cap = package_file_cap(&diacraft_vsdx::Limits::default());
    let size = crate::file::file_len(path).map_err(unreadable)?;
    if size > cap {
        return Err(unreadable(FileError::TooLarge { limit: cap }));
    }
    let reservation = take();
    let room = reservation.bytes();
    let no_room = |size: u64| {
        // What the process held when the reservation was made: the session's own drawing and stencils, other sessions and the reads that were running, and not this read's own.
        let held = HeldMemory { process: reservation.beside(), ..s.held_memory() };
        let e = VsdxError::Limit { kind: LimitKind::Memory, max: room };
        let alone = if size >= diacraft_vsdx::MAX_MEMORY { Alone::TooBig } else { Alone::MayFit };
        cannot_open(cmd, problem_beside(&e, &held, alone), refused_beside(&e, &held, alone, reservation.beside_reads()))
    };
    // The read needs the file and more (the reader counts the file in its budget), so a file that is all the room is refused as the read refuses it.
    if size >= room {
        return Err(no_room(size));
    }
    match read_file_stamped(path, cap.min(room)) {
        Ok((bytes, stamp)) => Ok((reservation, stamp, bytes)),
        Err(FileError::TooLarge { limit }) if limit < cap => Err(no_room(size.max(limit))),
        Err(e) => Err(unreadable(e)),
    }
}

/// Reads the package in `bytes` with what is left of the process's budget, **reserved while the read runs**: the reader is given the reservation's room as the memory it
/// may use, a read made at the same moment finds none and is refused in words, and a file the reader refuses for memory is told by what it was beside (`problem_beside`). → the
/// document, the report and **the reservation, to be kept until whoever keeps the document has made its own hold** (the session's file, the stencil): it is given back when it is dropped.
/// The file is loaded before this is called, and not under it, so that a disk that does not answer holds nothing. **The reservation is the caller's** ([`crate::memory`]): `file.open` and a
/// template read wait for a read that is running ([`crate::memory::reserve_read_waiting`], [`crate::memory::read_wait`]: they run on a thread of their own in the window), and are read
/// against what that read left, or refused in words where the two do not fit together or the wait ends; `stencil.open`, which the window runs on its own thread, waits for none and is
/// given a slice beside the read that runs ([`crate::memory::reserve_read_brief`]).
pub(super) fn read_package(s: &Session, cmd: &str, bytes: &[u8], reservation: &Reservation) -> Result<(Document, ReadReport)> {
    read_package_as_read(s, cmd, bytes, reservation).map(|(doc, report, _)| (doc, report))
}

/// [`read_package`], and **the document as the file holds it** where the open changed the one it gives (a loop of containers broken): `file.open` makes it the
/// drawing's baseline, so that a save into the file writes the ties the open repaired. `None` when the open changed nothing. The two share every page the
/// break did not touch.
pub(super) fn read_package_as_read(
    s: &Session,
    cmd: &str,
    bytes: &[u8],
    reservation: &Reservation,
) -> Result<(Document, ReadReport, Option<Document>)> {
    // What the process held when the reservation was made: the session's own drawing and stencils, other sessions and the reads that were running, and not this read's own.
    let held = crate::HeldMemory { process: reservation.beside(), ..s.held_memory() };
    let (file, room) = (u64::try_from(bytes.len()).unwrap_or(u64::MAX), reservation.bytes());
    if room <= file {
        // The reader counts the file in its budget, and would inflate the package before it found that nothing is left: it is not read (a file's bytes past what is left of the budget
        // cannot be read, whatever the file is).
        let e = VsdxError::Limit { kind: LimitKind::Memory, max: room };
        return Err(cannot_open(cmd, problem_beside(&e, &held, Alone::MayFit), refused_beside(&e, &held, Alone::MayFit, reservation.beside_reads())));
    }
    let limits = diacraft_vsdx::Limits { threads: crate::open_threads::open_threads(), ..reservation.limits() };
    let (read, mut report) = diacraft_vsdx::read_partial(bytes, &limits);
    match read {
        Ok(mut doc) => {
            // A loop of containers the file holds is broken, and said first. The document as read is kept beside it when the break changed it (a clone shares
            // every page and part, and the break unshares only the shapes whose ties it takes out).
            let read = doc.clone();
            let notes = crate::draw::container::cycles::untangle(&mut doc);
            // (A note of a page too large to look at changes nothing: the document kept is then the same as the one given, and a save finds nothing to write.)
            let read = (!notes.is_empty()).then_some(read);
            if !notes.is_empty() {
                let count = report.warning_counts.entry("container_loop").or_insert(0);
                *count = count.saturating_add(u64::try_from(notes.len()).unwrap_or(u64::MAX));
                report.warnings.splice(0..0, notes);
            }
            Ok((doc, report, read))
        }
        Err(e) => {
            let alone = judged_alone(&report, xml_part_bytes(bytes));
            Err(cannot_open(cmd, problem_beside(&e, &held, alone), refused_beside(&e, &held, alone, reservation.beside_reads())))
        }
    }
}

fn describe(cmd: &str, s: &Session) -> Result<Value> {
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    let history = s.history().ok_or_else(|| internal(cmd, "a document without a history"))?;
    let pages: Vec<Value> = doc.pages.iter().map(|p| json!({ "id": p.id, "name": p.name })).collect();
    Ok(json!({
        "title": doc.title,
        "version": doc.version,
        "pages": pages,
        "history": { "undo": history.undo_len(), "redo": history.redo_len() },
    }))
}

mod save_types;
pub use save_types::save_as_allows;

#[cfg(test)]
mod package_data_tests;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::PathBuf;

    #[test]
    fn a_refusal_for_memory_says_what_is_held_when_something_is_and_is_otherwise_as_it_always_was() {
        use diacraft_vsdx::{LimitKind, VsdxError};
        let memory = VsdxError::Limit { kind: LimitKind::Memory, max: 1_000 };
        let other = VsdxError::Limit { kind: LimitKind::Items, max: 1_000 };
        let nothing = crate::HeldMemory::default();
        let held = crate::HeldMemory { drawing: 700, stencils: 30, process: 900 };
        // Nothing held: the words of `refused`, to the letter, and the problem it always was, whatever the refusal is.
        for alone in [Alone::MayFit, Alone::TooBig] {
            assert_eq!(refused_beside(&memory, &nothing, alone, 0), refused(&memory), "{alone:?}");
            assert_eq!(problem_beside(&memory, &nothing, alone), OpenProblem::TooBig, "{alone:?}");
        }
        assert_eq!(refused_beside(&other, &held, Alone::MayFit, 0), refused(&other), "only a refusal for memory is told what is held");
        assert_eq!(problem_beside(&other, &held, Alone::MayFit), package_problem(&other));
        // Something held (by this session or by another in the process): there is not enough memory beside it, what is held and what to do, in the engine's own numbers.
        let text = refused_beside(&memory, &held, Alone::MayFit, 0);
        assert!(text.starts_with("not enough memory beside what is open (too_much_memory)"), "{text}");
        assert!(text.ends_with(&held.words()), "{text}");
        assert!(text.contains("hold 700 bytes, the open stencils 30 and the rest of the process 170 of the 2600000000 bytes"), "{text}");
        assert_eq!(problem_beside(&memory, &held, Alone::MayFit), OpenProblem::OutOfMemory);
        let elsewhere = crate::HeldMemory { drawing: 0, stencils: 0, process: 5 };
        assert_eq!(problem_beside(&memory, &elsewhere, Alone::MayFit), OpenProblem::OutOfMemory, "what another session holds is held");
        // A file that is too big for the whole budget alone is told as it is told alone, and is the problem it is alone: closing what is open does not help.
        assert_eq!(problem_beside(&memory, &held, Alone::TooBig), OpenProblem::TooBig);
        let alone_text = refused_beside(&memory, &held, Alone::TooBig, 0);
        assert_eq!(alone_text, refused(&memory));
        assert!(
            alone_text.starts_with("this file is too big to open safely (too_much_memory)") && !alone_text.contains("close a stencil"),
            "{alone_text}"
        );
        // A file that is being read holds the budget: wait for it, and the words are not about closing the drawing.
        let waiting = refused_beside(&memory, &held, Alone::MayFit, 1_234);
        assert!(waiting.starts_with("not enough memory beside what is open (too_much_memory)"), "{waiting}");
        assert!(waiting.contains("another file is being read, and holds 1234 of the 2600000000 bytes"), "{waiting}");
        assert!(waiting.ends_with("wait for it to finish, and open this file again") && !waiting.contains("close a stencil"), "{waiting}");
        assert_eq!(problem_beside(&memory, &held, Alone::MayFit), OpenProblem::OutOfMemory);
    }

    use super::*;

    /// The report of a read that was refused: the file and the package it inflated, how far it got (bytes of XML taken in and of pictures kept), and the most it had charged.
    fn refused_report(file: u64, package: u64, xml: u64, media: u64, peak: u64) -> ReadReport {
        let stats = diacraft_vsdx::Stats {
            file_bytes: file,
            uncompressed_bytes: package,
            xml_bytes: xml,
            media_bytes: media,
            memory_peak: peak,
            ..Default::default()
        };
        ReadReport { stats, ..Default::default() }
    }

    #[test]
    fn a_refused_read_is_judged_too_big_alone_when_its_xml_scaled_to_the_whole_xml_of_the_package_passes_the_budget_clearly_and_otherwise_the_doubt_is_for_the_file()
     {
        const BUDGET: u64 = diacraft_vsdx::MAX_MEMORY;
        // The file and the package inflated are what a read holds before it builds anything: the whole budget or more is too big, whatever else is said.
        assert_eq!(judged_alone(&refused_report(1, BUDGET - 1, 0, 0, 0), None), Alone::TooBig);
        assert_eq!(judged_alone(&refused_report(u64::MAX, 5, 0, 0, 0), Some(1)), Alone::TooBig);
        // The XML of the package not known, or none: nothing is scaled, and the doubt is for the file.
        assert_eq!(judged_alone(&refused_report(1, BUDGET - 2, 5, 0, BUDGET), None), Alone::MayFit);
        assert_eq!(judged_alone(&refused_report(1, 100, 100, 0, BUDGET), Some(0)), Alone::MayFit);
        assert_eq!(judged_alone(&refused_report(1, BUDGET - 2, 0, 0, 0), Some(10)), Alone::MayFit, "a read that took nothing in says nothing");
        assert_eq!(judged_alone(&ReadReport::default(), Some(10)), Alone::MayFit, "and one that never began says nothing");
        // Less than an eighth of the XML in is too little to scale from: 100 MB of 800 MB is the eighth.
        let xml = 800_000_000;
        assert_eq!(judged_alone(&refused_report(0, xml, 99_999_999, 0, 2_000_000_000), Some(xml)), Alone::MayFit);
        assert_eq!(
            judged_alone(&refused_report(0, xml, 100_000_000, 0, 2_000_000_000), Some(xml)),
            Alone::TooBig,
            "an eighth in and 2 GB charged: eight times that is more than the budget"
        );
        // Scaled at the same rate to the end of the XML, with the file and the package beside it: a half in and 1.4 GB charged is 2.8 GB, and 1.2 GB is 2.4 GB.
        assert_eq!(judged_alone(&refused_report(2_000_000, 100_000_000, 50_000_000, 0, 1_400_000_000), Some(100_000_000)), Alone::TooBig);
        assert_eq!(judged_alone(&refused_report(2_000_000, 100_000_000, 50_000_000, 0, 1_200_000_000), Some(100_000_000)), Alone::MayFit);
        // **Pictures are not XML**: a package of 100 MB of XML and 100 MB of pictures, refused 60 MB of XML in and with no picture kept, is 1.0 GB charged for
        // 60 MB of XML. Scaled against the whole package it was told too big (3.3 GB); against the XML it is 1.67 GB, and the file opens alone.
        assert_eq!(judged_alone(&refused_report(100_000_000, 200_000_000, 60_000_000, 0, 1_000_000_000), Some(100_000_000)), Alone::MayFit);
        // The pictures that were kept are added once, and not scaled: 50 MB of them in 1.25 GB charged is 1.2 GB for the XML, which is 2.4 GB for all of it, and the pictures are 50 MB.
        assert_eq!(
            judged_alone(&refused_report(0, 150_000_000, 50_000_000, 50_000_000, 1_250_000_000), Some(100_000_000)),
            Alone::MayFit,
            "2.6 GB, which is the budget"
        );
        assert_eq!(judged_alone(&refused_report(0, 150_000_000, 50_000_000, 50_000_000, 1_450_000_000), Some(100_000_000)), Alone::TooBig, "3.0 GB");
        // **The pictures kept are in the figure** (the review, J2): 500 MB of them in 2.0 GB charged is 1.5 GB for the XML, which is all of it (it is all in), and the
        // package of 700 MB and the 500 MB of pictures beside it are 2.7 GB, past the budget and the margin; without the pictures it is 2.2 GB, and the file would not be told too big.
        assert_eq!(judged_alone(&refused_report(0, 700_000_000, 100_000_000, 500_000_000, 2_000_000_000), Some(100_000_000)), Alone::TooBig);
        // And they are added once: the same pictures with the XML at 1.0 GB are 2.2 GB (700 MB, 1.0 GB and 500 MB), which fits.
        assert_eq!(judged_alone(&refused_report(0, 700_000_000, 100_000_000, 500_000_000, 1_500_000_000), Some(100_000_000)), Alone::MayFit);
        // More XML taken in than the package has is all of it, and not more: the figure is the charge and the package.
        assert_eq!(judged_alone(&refused_report(0, 100_000_000, 900_000_000, 0, 2_600_000_000), Some(100_000_000)), Alone::TooBig);
        // A file is told too big only when it is clearly past the budget: not at it, not within a thirty-second of it.
        let at = |peak: u64| judged_alone(&refused_report(0, 100_000_000, 100_000_000, 0, peak), Some(100_000_000));
        assert_eq!(at(BUDGET - 100_000_000), Alone::MayFit, "exactly the budget");
        assert_eq!(at(BUDGET - 100_000_000 + BUDGET / 32), Alone::MayFit, "a thirty-second past it");
        assert_eq!(at(BUDGET - 100_000_000 + BUDGET / 32 + BUDGET / 20), Alone::TooBig, "clearly past it");
        // Nothing wraps, and nothing divides by nothing.
        assert_eq!(judged_alone(&refused_report(0, u64::MAX / 2, u64::MAX, u64::MAX, u64::MAX), Some(u64::MAX)), Alone::TooBig);
        assert_eq!(judged_alone(&refused_report(0, 0, 0, 0, u64::MAX), Some(1)), Alone::MayFit);
        assert_eq!(judged_alone(&refused_report(0, 5, 5, u64::MAX, u64::MAX), Some(u64::MAX)), Alone::MayFit);
    }

    #[test]
    fn the_xml_of_a_package_is_the_inflated_bytes_of_its_xml_and_relationships_parts_and_no_picture_is_in_it() {
        use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, typical_shape};
        let package = |picture: usize| {
            let page = PageSpec::new(1, "P").shapes(&(0..30).map(typical_shape).collect::<String>()).rel(
                "rId1",
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                "../media/image1.png",
            );
            // Bytes that do not compress, so that the file is as big as the picture.
            let mut seed = 0x2545_F491_4F6C_DD1D_u64;
            let noise: Vec<u8> = (0..picture)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 24) as u8
                })
                .collect();
            VsdxBuilder::new().page(page).with(Pkg::new().part("/visio/media/image1.png", "image/png", noise)).build()
        };
        let (small, big) = (package(10), package(500_000));
        let (xml_small, xml_big) = (xml_part_bytes(&small).expect("a package"), xml_part_bytes(&big).expect("a package"));
        assert_eq!(xml_small, xml_big, "a picture of 500,000 bytes instead of 10 changes none of the XML");
        assert!(xml_small > 30 * 100, "the 30 shapes of the page are in it: {xml_small}");
        assert!(big.len() > 400_000 && xml_big < 400_000, "and the pictures are not: {} bytes in all, {xml_big} of XML", big.len());
        assert_eq!(xml_part_bytes(&[7u8; 100]), None, "bytes that are no archive have no figure");
    }

    /// The smallest flat drawing the reader takes: the root says it is a drawing, and the body holds a drawing with one page.
    const SMALL_FLAT: &str = concat!(
        r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"office:mimetype="application/vnd.oasis.opendocument.graphics" office:version="1.3">"#,
        r#"<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body></office:document>"#
    );

    /// A session that has opened `SMALL_FLAT` as a file of its own in a scratch folder: the session, the file's path and the folder.
    fn flat_session(name: &str) -> (Session, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("diacraft-engine-cmdfile-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plan.fodg");
        std::fs::write(&file, SMALL_FLAT).unwrap();
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": file.to_str().unwrap() })).unwrap();
        (s, file, dir)
    }

    /// `save_to` of a Save As of a flat drawing writes it as the name says, from the model (a `.diacraft` file, an `.odg` package, a VSDX type), and the flat
    /// file is left as it is; a `.fodg` is no Save As target, for a flat drawing as for any other, and writes nothing. Save, the file's own, writes the flat file.
    #[test]
    fn save_to_writes_a_save_as_of_a_flat_drawing_by_the_name_and_refuses_fodg() {
        let (mut s, file, dir) = flat_session("save-to");
        for target in ["other.fodg", "plan.fodg", "OTHER.FODG"] {
            match save_to(&mut s, "file.save_as", &dir.join(target), true, false, Options::default()) {
                Err(crate::EngineError::BadParams { cmd, msg }) => {
                    assert_eq!((cmd.as_str(), msg.as_str()), ("file.save_as", crate::odgfile::FODG_NOT_OFFERED), "{target}");
                }
                other => panic!("{target}: {other:?}"),
            }
        }
        assert_eq!(std::fs::read(&file).unwrap(), SMALL_FLAT.as_bytes(), "the file is as it was");
        assert!(!dir.join("other.fodg").exists());
        for (target, variant) in [("other.odg", "odg"), ("other.diacraft", "diacraft"), ("other.vsdx", "vsdx")] {
            let (mut s, file, dir) = flat_session(target);
            let saved =
                save_to(&mut s, "file.save_as", &dir.join(target), true, false, Options::default()).unwrap_or_else(|e| panic!("{target}: {e}"));
            assert_eq!(saved["variant"], variant, "{target}: {saved}");
            assert_eq!(std::fs::read(&file).unwrap(), SMALL_FLAT.as_bytes(), "{target}: the flat file is as it was");
            assert!(dir.join(target).exists(), "{target}");
            assert!(!s.source().is_some_and(OpenFile::is_flat), "{target}: the drawing stands on the file it was saved as");
            let _ = std::fs::remove_dir_all(&dir);
        }
        // The file's own save is not a Save As: an unedited flat drawing writes its own bytes.
        let saved = save_to(&mut s, "file.save", &file, false, false, Options::default()).unwrap();
        assert_eq!((saved["form"].clone(), saved["edited"].clone()), (json!("flat"), json!(false)));
        assert_eq!(std::fs::read(&file).unwrap(), SMALL_FLAT.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `file.save_as` is enabled wherever a document is open, a flat drawing included: its `enabled` is `has_document`, which says "no document open" for none.
    #[test]
    fn save_as_is_enabled_for_a_flat_drawing_and_says_no_document_open_for_none() {
        let mut s = Session::new();
        assert_eq!(has_document(&s), Err("no document open".to_owned()));
        let (flat, _, dir) = flat_session("can");
        assert_eq!(has_document(&flat), Ok(()));
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(has_document(&s), Ok(()));
        let listed = flat.commands().into_iter().find(|c| c.id == "file.save_as").expect("the command");
        assert!(listed.enabled && listed.disabled_reason.is_none(), "{listed:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `file.open`, a template read and `stencil.open` bind the reservation of the budget **before** the bytes of the file (`let (reservation, stamp, bytes) = load_for_read(..)`), so that on
    /// every way out the bytes are dropped first and the room is given to the next read only after: a read that waits for the room starts to load its file the moment it is given back, and
    /// two files of 900 MB were in memory at once when the reservation went first (a child of `read_room.rs` failed on it). This pins the rule of the language the order rests on.
    #[test]
    fn what_a_pattern_binds_last_is_dropped_first_so_the_reservation_bound_before_the_bytes_outlives_them() {
        use std::cell::RefCell;
        struct Probe<'a>(&'static str, &'a RefCell<Vec<&'static str>>);
        impl Drop for Probe<'_> {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }
        let order = RefCell::new(Vec::new());
        {
            let (_reservation, _stamp, _bytes) = (Probe("reservation", &order), 0u8, Probe("bytes", &order));
        }
        assert_eq!(*order.borrow(), ["bytes", "reservation"]);
        // The same on an early return by `?`.
        let early = || -> std::result::Result<(), ()> {
            let (_reservation, _stamp, _bytes) = (Probe("reservation", &order), 0u8, Probe("bytes", &order));
            Err(())?;
            Ok(())
        };
        order.borrow_mut().clear();
        assert!(early().is_err());
        assert_eq!(*order.borrow(), ["bytes", "reservation"]);
    }

    fn vsdx(ext: &str) -> Variant {
        Variant::from_extension(ext).unwrap()
    }

    /// The table of the design's 4.1 and of the plan, row by row: what a save does for each working file and each name.
    #[test]
    fn the_save_path_table() {
        use FileFormat::{Diacraft, Odg, Vsdx};
        let own = |kind| Target::Diacraft(kind);
        // A `.diacraft` drawing is saved as `.diacraft` of any kind, as a VSDX type by the model writer, and as an `.odg` or `.otg` by the `.odg` model writer
        // (which lifted Call 18's refusal).
        let (drawing, template) = (diacraft_odg::Variant::Drawing, diacraft_odg::Variant::Template);
        for kind in NativeKind::ALL {
            assert_eq!(save_path(Some(Diacraft), false, own(kind)), SavePath::Native(kind));
            assert_eq!(save_path(Some(Diacraft), true, own(kind)), SavePath::Native(kind), "a stencil as any kind");
        }
        for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
            assert_eq!(save_path(Some(Diacraft), false, Target::Vsdx(vsdx(ext))), SavePath::VsdxModel, "{ext}");
        }
        assert_eq!(save_path(Some(Diacraft), false, Target::Odg(drawing)), SavePath::OdgModel(drawing));
        assert_eq!(save_path(Some(Diacraft), false, Target::Odg(template)), SavePath::OdgModel(template));
        // A VSDX drawing: a VSDX type is the splice writer as before, `.diacraft` the package, and an `.odg` or `.otg` is written from the model by the `.odg` model writer, with what the
        // `.odg` cannot say riding in its carrier (a save across).
        assert_eq!(save_path(Some(Vsdx), false, Target::Vsdx(vsdx("vstx"))), SavePath::VsdxSplice(vsdx("vstx")));
        assert_eq!(save_path(Some(Vsdx), false, own(NativeKind::Template)), SavePath::Native(NativeKind::Template));
        for variant in [drawing, template] {
            assert_eq!(save_path(Some(Vsdx), false, Target::Odg(variant)), SavePath::OdgModel(variant));
        }
        // An `.odg`: its own type is the splice writer, `.diacraft` is the package (the model holds what the `.odg` holds), and a VSDX type is written from the model by the VSDX model writer
        // (a save across).
        assert_eq!(save_path(Some(Odg), false, Target::Odg(drawing)), SavePath::OdgSplice(drawing));
        assert_eq!(save_path(Some(Odg), false, Target::Odg(template)), SavePath::OdgSplice(template), "the kind a Save As names");
        assert_eq!(save_path(Some(Odg), false, own(NativeKind::Drawing)), SavePath::Native(NativeKind::Drawing));
        for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
            assert_eq!(save_path(Some(Odg), false, Target::Vsdx(vsdx(ext))), SavePath::VsdxModel, "an .odg as {ext}");
        }
        // A new drawing (and one made from a template) keeps its model and no file's bytes (NP10): it is `.diacraft` in its own right, a VSDX type is written from the model
        // by the model writer (there is no file to splice into, as for a `.diacraft` drawing), and so is an `.odg` or `.otg`.
        for ext in ["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm"] {
            assert_eq!(save_path(None, false, Target::Vsdx(vsdx(ext))), SavePath::VsdxModel, "a new drawing as {ext}");
        }
        for kind in NativeKind::ALL {
            assert_eq!(save_path(None, false, own(kind)), SavePath::Native(kind));
        }
        assert_eq!(save_path(None, false, Target::Odg(drawing)), SavePath::OdgModel(drawing));
        assert_eq!(save_path(None, false, Target::Odg(template)), SavePath::OdgModel(template));
        // A stencil is no OpenDocument file, whatever it was.
        for working in [Some(Diacraft), Some(Vsdx), Some(Odg), None] {
            for variant in [drawing, template] {
                assert_eq!(save_path(working, true, Target::Odg(variant)), SavePath::Refused(Why::StencilToOdg), "{working:?}");
            }
        }
        // A name that is no type is refused, for each working file.
        for working in [Some(Diacraft), Some(Vsdx), Some(Odg), None] {
            assert_eq!(save_path(working, false, Target::Other), SavePath::Refused(Why::NoType(working)), "{working:?}");
        }
    }

    #[test]
    fn a_name_says_the_type_it_is_in_any_case() {
        assert_eq!(Target::of(Some("diacraft")), Target::Diacraft(NativeKind::Drawing));
        assert_eq!(Target::of(Some("DiaCraft-Stencil")), Target::Diacraft(NativeKind::Stencil));
        assert_eq!(Target::of(Some("diacraft-template")), Target::Diacraft(NativeKind::Template));
        assert_eq!(Target::of(Some("ODG")), Target::Odg(diacraft_odg::Variant::Drawing));
        assert_eq!(Target::of(Some("Otg")), Target::Odg(diacraft_odg::Variant::Template));
        assert_eq!(Target::of(Some("VSSM")), Target::Vsdx(vsdx("vssm")));
        // The engine's own list of OpenDocument types is the two this names, drawing first.
        let listed: Vec<Target> = crate::odgfile::SAVE_TYPES.iter().map(|extension| Target::of(Some(extension))).collect();
        assert_eq!(listed, [Target::Odg(diacraft_odg::Variant::Drawing), Target::Odg(diacraft_odg::Variant::Template)]);
        for (extension, variant) in crate::odgfile::SAVE_TYPES.iter().zip([diacraft_odg::Variant::Drawing, diacraft_odg::Variant::Template]) {
            assert_eq!(*extension, variant.extension());
        }
        for other in [None, Some(""), Some("txt"), Some("fodg"), Some("fotg"), Some("odt"), Some("diacraft-x"), Some("vsdx ")] {
            assert_eq!(Target::of(other), Target::Other, "{other:?}");
        }
    }

    /// Every refusal has words, in the two forms, and the words are the ones the save across formats has always had where it had them: Save As says the name is wrong and Save
    /// says to use Save As, and **each says `.diacraft` where it is a way out**.
    #[test]
    fn every_refusal_has_words_that_name_the_way_out() {
        use FileFormat::{Diacraft, Odg, Vsdx};
        let whys = [Why::StencilToOdg, Why::NoType(None), Why::NoType(Some(Diacraft)), Why::NoType(Some(Vsdx)), Why::NoType(Some(Odg))];
        for why in whys {
            let (as_param, as_failure) = save_words(why);
            assert!(!as_param.is_empty() && as_param.len() < 400, "{why:?}: {as_param}");
            assert!(as_failure.as_ref().is_none_or(|w| !w.is_empty() && w.len() < 400), "{why:?}");
            assert!(
                matches!(refused_save("file.save_as", why, true), crate::EngineError::BadParams { .. }),
                "{why:?}: Save As says the parameter is wrong"
            );
            assert!(matches!(refused_save("file.save", why, false), crate::EngineError::Failed { .. }), "{why:?}: Save is a failure");
        }
        // The ways out named in words.
        // A name that is no type says every type there is, the VSDX ones and `.odg` among them (the saves across are written).
        assert!(save_words(Why::NoType(Some(Odg))).0.contains(".diacraft") && save_words(Why::NoType(Some(Odg))).0.contains(".vsdx"));
        assert!(save_words(Why::NoType(Some(Vsdx))).0.contains(".diacraft") && save_words(Why::NoType(Some(Vsdx))).0.contains(".odg"));
        assert!(save_words(Why::StencilToOdg).0.contains("OpenDocument has no stencil format"));
        // `file.save` of an `.odg` named like another type, as it was.
        assert_eq!(
            save_words(Why::NoType(Some(Odg))).1.as_deref(),
            Some("this drawing's file name must end in .odg or .otg: use file.save_as to give it one")
        );
        assert_eq!(
            save_words(Why::NoType(Some(Vsdx))).1.as_deref(),
            Some("the drawing's file name must end in .vsdx, .vsdm, .vssx, .vssm, .vstx or .vstm: use file.save_as to give it one")
        );
    }

    fn reader(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("reader {i}")).collect()
    }

    /// An open lists at most [`WARNINGS_SHOWN`] warnings (the docs say "first 20"), **the notes the command makes itself in front of and behind the reader's, never cut**,
    /// and the reader's fill the room that is left (the review: a note added after the cap made 21).
    #[test]
    fn an_open_lists_at_most_twenty_warnings_and_never_cuts_a_note_of_its_own() {
        let (first, last) = (Some("first note".to_owned()), Some("last note".to_owned()));
        for n in [0, 1, 17, 18, 19, 20, 21, 40, 1000] {
            let words = reader(n);
            let none = listed_warnings(None, &words, None);
            assert_eq!(none.len(), n.min(20), "no note, {n} of the reader's");
            assert_eq!(none, words.iter().take(20).cloned().collect::<Vec<_>>(), "the reader's, in order, as before");
            let one = listed_warnings(first.clone(), &words, None);
            assert_eq!(one.len(), (n + 1).min(20), "a first note, {n}");
            assert_eq!(one.first().map(String::as_str), Some("first note"), "{n}");
            assert_eq!(&one[1..], &words[..one.len() - 1], "the reader's follow the note: {n}");
            let end = listed_warnings(None, &words, last.clone());
            assert_eq!(end.len(), (n + 1).min(20), "a last note, {n}");
            assert_eq!(end.last().map(String::as_str), Some("last note"), "{n}");
            assert_eq!(&end[..end.len() - 1], &words[..end.len() - 1], "{n}");
            let both = listed_warnings(first.clone(), &words, last.clone());
            assert_eq!(both.len(), (n + 2).min(20), "both notes, {n}");
            assert_eq!((both.first().map(String::as_str), both.last().map(String::as_str)), (Some("first note"), Some("last note")), "{n}");
            assert_eq!(&both[1..both.len() - 1], &words[..both.len() - 2], "{n}");
        }
    }
}
