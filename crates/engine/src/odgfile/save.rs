//! The save of a drawing that came from an `.odg`: the copy the writer is given, the thumbnail, the writer, and the words.
//!
//! The file that is written, the check of the path, the comparison of the file with what it was and the swap of a temporary file are
//! `cmd/file.rs`'s, as for a VSDX file; this makes the bytes. It writes no file and changes no session.

use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::Document;
use diacraft_odg::{OriginalPackage, Variant, WriteError, WriteOptions, write_with};
use diacraft_render::export::{encode_png, unpremultiply};
use diacraft_render::{Clock, Limits, Parallel, Renderer, View, default_threads, page_paper_size};

use super::OdgState;
use super::flatten::{Flattened, flatten, flatten_model};
use super::hold::held_beside;
use super::note;
use crate::Session;
use crate::across::{self, Carried, Options};
use crate::error::{EngineError, failed};
use crate::file::OpenFile;
use crate::history::History;

/// The most pixels the thumbnail has on its long side (`WriteOptions::thumbnail`: a PNG of at most 256 pixels on its long side).
const THUMBNAIL_PIXELS: f64 = 256.0;

/// The longest message of the writer's the save repeats, in characters: its refusals are a sentence or two, with numbers, and no name from the file.
const MOST_SHOWN: usize = 600;

/// What a save made.
pub(crate) struct Saved {
    /// The package.
    pub bytes: Vec<u8>,
    /// Where each shape of the model is in `bytes`: what the next save maps the model by.
    pub state: OdgState,
    /// What the save said, in words: what a new shape had that is not written, what the writer noted.
    pub warnings: Vec<String>,
    /// The notes of the save, each `{class, words, ..}` (`note.rs`, design decision 8): what a LibreOffice save would drop of what the drawing holds. Empty when
    /// there is none. They are not warnings.
    pub notes: Vec<serde_json::Value>,
    /// Whether the model had changed since the file was read or saved. An unchanged one is the original, copied.
    pub edited: bool,
    /// What the file carries of the other format's data, and the digital signatures that no longer match it (the design's 5.6): a structured field, never a warning. Empty for a save into
    /// a file the drawing stands on, whose notes are as they were.
    pub carried: Carried,
    /// How many parts of the macro project the caller chose to leave out (`drop_macros`): none for a save into a file.
    pub macros_dropped: usize,
    /// What the check after the save found: the file read back and compared with the drawing, for a save from the model. `None` for a save into a file the drawing stands on,
    /// which is not checked in 0.2.
    pub checked: Option<crate::check::Outcome>,
}

/// What a refusal tells a person to do, after the cause: take the change back, or **save the drawing as a `.diacraft` file**, which holds all the drawing does (Save As writes a
/// drawing of an `.odg` as `.diacraft` and as a VSDX type so the words that said an `.odg` is saved only as an `.odg` "until saving in other formats is
/// written" were stale, for a flat file). Save into the file is still an `.odg`.
const WAY_OUT: &str = " Undo the change (edit.undo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from an .odg is saved only as an .odg by Save.";

/// The way out of a refusal when the drawing came from a flat file: the same words, for a flat file's own form, and the other
/// way, which a flat drawing has since Save As writes it as a `.diacraft` file: that holds all the drawing does, so what the flat writer refuses is written there.
const FLAT_WAY_OUT: &str = " Undo the change (edit.undo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from a flat file is saved only as a flat file by Save.";

/// The way out of a refusal whose cause is an undo, for a flat file: a redo gives the drawing the file has.
const FLAT_WAY_OUT_REDO: &str = " Redo the change you took back (edit.redo) to save, or delete the shape or the page again, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from a flat file is saved only as a flat file by Save.";

/// The way out of a refusal whose cause is an undo of a style sheet the file holds and the save cannot take out of it, for a flat file.
const FLAT_WAY_OUT_REDO_STYLE: &str = " Redo the change you took back (edit.redo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from a flat file is saved only as a flat file by Save.";

/// The way out of a refusal whose cause is an undo: a shape of the file that an undo put back, with a connector of the file glued to it. The change to take back
/// is the undo, and undoing once more does not save (it goes round: the refusal is the same), so the words name a redo, which gives the drawing the file has, and
/// the other way out, to delete the shape (or the page) again.
const WAY_OUT_REDO: &str = " Redo the change you took back (edit.redo) to save, or delete the shape or the page again, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from an .odg is saved only as an .odg by Save.";

/// What a refusal to save as another kind of document (the writer's `write_media_type`: the file has no place to say it) tells a person to do. The change
/// is not what the save cannot write, so taking it back (`WAY_OUT`) is no way out of it.
const KIND_WAY_OUT: &str = " Nothing was written, and undoing a change does not help: the file this drawing was made from cannot say it is another kind of document. \
     Save it as the kind it is, or start again from another file.";

/// The way out of a refusal whose cause is an undo of a style sheet the file holds and the save cannot take out of it (the writer's words say "taken back by an undo"): a
/// redo gives the drawing the file has (the undo of a saved `theme.apply` is taken out of the file; this is for what cannot be).
const WAY_OUT_REDO_STYLE: &str = " Redo the change you took back (edit.redo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from an .odg is saved only as an .odg by Save.";

/// What the writer says of a style sheet an undo took back that it cannot take out of the file.
const STYLE_TAKEN_BACK: &str = "taken back by an undo";

/// The words of a refusal of a save: the cause, cut to leave room for the way out, which is never cut.
pub(crate) fn refused_words(cause: &str, flat: bool) -> String {
    let way_out = match (cause.contains(super::flatten::PUT_BACK), cause.contains(STYLE_TAKEN_BACK), flat) {
        (true, _, false) => WAY_OUT_REDO,
        (true, _, true) => FLAT_WAY_OUT_REDO,
        (false, true, false) => WAY_OUT_REDO_STYLE,
        (false, true, true) => FLAT_WAY_OUT_REDO_STYLE,
        (false, false, false) => WAY_OUT,
        (false, false, true) => FLAT_WAY_OUT,
    };
    let mut words =
        crate::shown(&format!("the drawing could not be saved: {}.", cause.trim_end_matches('.')), MOST_SHOWN.saturating_sub(way_out.len()));
    words.push_str(way_out);
    words
}

/// The bytes of `doc` as an `.odg`, spliced into `source`'s. `dirty` says whether the drawing has changes that are not saved: only then is a
/// thumbnail rendered, since an unedited save keeps the package's own. `history` is the session's, whose earlier versions of the drawing the writer's gate
/// cannot see: what the engine holds beside `doc` (those versions, the pages the last save gave the writer and the shapes it flattened) is measured and taken
/// from the memory the save may hold (`hold.rs`). A change the writer cannot write is `Err` with its words, and nothing is made.
///
/// `ask` is the kind of document the save is to write when it is not the kind the package says it is (an `.otg` saved as a drawing is `Some(Variant::Drawing)`, and a
/// drawing saved as a template is `Some(Variant::Template)`): the writer changes the media type in the two places it is stated, and nothing else of the package. `None` keeps the kind
/// the package has, as every save did.
pub(crate) fn save(
    cmd: &str,
    doc: &Arc<Document>,
    source: &OpenFile,
    dirty: bool,
    history: Option<&History>,
    ask: Option<Variant>,
    choice: Options,
) -> Result<Saved, EngineError> {
    // A drawing that came from a flat file is saved as one: the writer keeps the form of the original (`WriteOptions::form: None`), a flat
    // file has no thumbnail to write, and a refusal says the way out of a flat file.
    let flat_file = source.is_flat();
    let state = source
        .odg()
        .ok_or_else(|| failed(cmd, "this drawing was not opened from an .odg, and an .odg is written from the file it was opened from"))?;
    let mut options = WriteOptions::default();
    // What the engine holds beside the drawing is memory the save may not use: the writer's gate counts the model and the file, and not that.
    let max = options.limits.max_write_memory;
    let sources = state.flattened().sources();
    let mut walked = 0;
    // Before the copy is made: what the engine holds beside the model, which bounds what the copy may take (it does not know yet which of the shapes the last save
    // flattened the copy will give again, so it counts them all: a bound, and not the figure the writer's gate is lowered by).
    let before = held_beside(doc, doc, history, state.saved_pages(), &sources, max, &mut walked);
    if before >= max {
        return Err(failed(cmd, memory_words(before, max)));
    }
    // What a save of this file costs on its own, which the copies must leave room for.
    let room = max.saturating_sub(before).saturating_sub(state.index().read_cost().save_bytes());
    // **What the save that wrote the file from the model left out is left out of every save into it** (`LeftOut`): the drawing keeps it and the file never held it, so the drawing
    // given whole differs from the file in what no save of it writes, and the save after the first, with no edit, was refused or counted as a change. DiaCraft's own skeleton (a drawing made from a
    // built-in template, `save_model`), and what the Save As was asked to leave out: the embedded objects here, the macro project by the writer (`WriteOptions::drop_macros`). A drawing read from
    // a file left nothing out, and is given as it is.
    // **A Save As is asked what to leave out, too** (a Save As from an `.odg` to another `.odg` ignored `drop_macros` and `drop_objects`): what the file was written without and what this
    // save is asked to leave out are both left out of the file it writes, and of every save into it after.
    let left_out = super::LeftOut { asked: state.left_out().asked.union(choice), ..state.left_out() };
    let bare = if left_out.skeleton { super::skeleton::without_skeleton(doc) } else { None };
    let base: &Document = bare.as_ref().unwrap_or(doc);
    let without = if left_out.asked.drop_objects {
        across::onto::given(base, diacraft_doc::Format::Odf, left_out.asked).map_err(|why| failed(cmd, why))?.doc
    } else {
        None
    };
    let base: &Document = without.as_ref().unwrap_or(base);
    // **The objects the model writer wrote as object frames ride no more**: the file holds each once, as the frame's `Object N`, and the drawing, which still holds the VSDX embedding, is given
    // without it, as the model writer was given what it carried: a save of it with no edit is then no edit, and an edit does not write the object a second time under DiaCraft's own folder.
    let carried = if left_out.carried_objects {
        diacraft_odg::without_carried_objects(base).map_err(|e| failed(cmd, refused_words(&e.to_string(), flat_file)))?
    } else {
        None
    };
    let base: &Document = carried.as_ref().unwrap_or(base);
    let copied = bare.is_some() || without.is_some() || carried.is_some();
    // The flattening goes through the index by the place of each page of the model: after pages were added, taken away or put in another order, the entry of a page is
    // found by the page's `ID`, and a page the file lacks has an entry that holds no shape (every shape of it is new).
    let page_ids: Vec<u32> = base.pages.iter().map(|p| p.id).collect();
    let aligned = state.index().aligned_to(&page_ids);
    let flat = flatten(base, aligned.as_ref().unwrap_or(state.index()), state.flattened(), room, flat_file)
        .map_err(|words| failed(cmd, refused_words(&words, flat_file)))?;
    // After it: what the engine holds beside the drawing the writer is given, which the writer's gate does not see: the shapes of the model that the copy replaced,
    // the versions undo keeps, the pages the last save was given and its shapes, **each shape counted once** wherever it is held (the writer counts the copy).
    let to_write = flat.doc.as_ref().unwrap_or(base);
    // (When no shape needed flattening and nothing was left out, the drawing given is the model, and what was counted before is what is held.)
    let held =
        if flat.doc.is_none() && !copied { before } else { held_beside(to_write, doc, history, state.saved_pages(), &sources, max, &mut walked) };
    if held >= max {
        return Err(failed(cmd, memory_words(held, max)));
    }
    let mut warnings = flat.notes;
    // A drawing with no file yet is saved for the first time (`OpenFile::is_named`), and R25 lists the thumbnail among the parts of a new drawing: its package, made
    // by DiaCraft or taken from a template, has none, so the picture is rendered and written even when nothing was changed. A drawing that has a file is
    // not: its save that is no edit is the file's own bytes, whatever the session says of changes.
    let first_save = !source.is_named();
    let thumbnail = if wants_thumbnail(dirty, first_save, flat_file) {
        match thumbnail(doc) {
            Ok(png) => Some(png),
            Err(why) => {
                warnings.push(thumbnail_words(first_save, &why));
                None
            }
        }
    } else {
        None
    };
    let original = OriginalPackage { bytes: source.bytes(), index: state.index() };
    options.first_save = first_save;
    options.thumbnail = thumbnail;
    options.variant = ask;
    options.drop_macros = left_out.asked.drop_macros;
    options.limits.max_write_memory = max.saturating_sub(held);
    let written = write_with(to_write, Some(&original), &options).map_err(|e| refusal(cmd, &e, held, max, flat_file))?;
    warnings.extend(written.notes.iter().map(ToString::to_string));
    let pages: Vec<Arc<diacraft_doc::Page>> = to_write.pages.iter().cloned().collect();
    // What a LibreOffice save would drop of the drawing as the file now holds it (the copy the writer was given), which a person is told once and a program is told always.
    let notes: Vec<serde_json::Value> = note::note(&note::find(to_write)).into_iter().collect();
    Ok(Saved {
        bytes: written.bytes,
        state: OdgState::saved(written.index, flat.made, pages, left_out),
        warnings,
        notes,
        edited: written.edited,
        carried: Carried::none(),
        macros_dropped: 0,
        checked: None,
    })
}

/// The way out a refusal of a save from the model alone tells a person: nothing was written, and a `.diacraft` file holds all that the drawing does (a VSDX type is the other way out: the model
/// writer of it writes a drawing made here). A change is not taken back to save: the drawing is not the file's, and the cause is what it holds, not what was last done to it.
const MODEL_WAY_OUT: &str = " Nothing was written. A .diacraft file holds everything the drawing does: save it as .diacraft, or as a VSDX type (.vsdx), or take out what an .odg cannot say.";

/// The words of a refusal of a save from the model alone: the cause, cut to leave room for the way out, which is never cut.
pub(crate) fn model_refused_words(cause: &str) -> String {
    let mut words =
        crate::shown(&format!("the drawing could not be saved: {}.", cause.trim_end_matches('.')), MOST_SHOWN.saturating_sub(MODEL_WAY_OUT.len()));
    words.push_str(MODEL_WAY_OUT);
    words
}

/// The error of a save from the model alone that the writer refused: a refusal for want of memory says what the engine holds beside the drawing, as the save into a file's does, and any
/// other is in the writer's words with the way out.
fn model_refusal(cmd: &str, e: &WriteError, beside: u64, max: u64) -> EngineError {
    match e {
        WriteError::Limit { what, .. } if what.contains("bytes of memory a save holds") => failed(cmd, memory_words(beside, max)),
        other => failed(cmd, model_refused_words(&other.to_string())),
    }
}

/// What part of the memory a save from the model alone may hold the flattened copies of the engine's shapes may take: a third (see [`save_model`]).
const COPY_SHARE: u64 = 3;

/// The bytes of `doc` as an `.odg` (`Variant::Drawing`) or `.otg` (`Variant::Template`) **written from the model alone** (the design's 4.1): the drawing's working file is a
/// `.diacraft` file or none (a new drawing), so there is no package to splice into, and `diacraft_odg::write_with(doc, None, ..)` makes one from the model, with what it holds of the file
/// the drawing came from in its place. **The kind is asked for always**: a drawing made from a template file is a drawing, and one saved as `.otg` is a template.
///
/// **The bookkeeping follows the content stamp, and never the dirty flag** (the design's 4.4; a reopen does not keep the flag): while the model is what it was when it was read from a foreign
/// file (`Session::content_unchanged`) the writer writes the generator and the thumbnail as the model holds them and no picture is rendered; otherwise, and for a drawing that was never read
/// from a file (it has no stamp), the generator is DiaCraft's and the first page is rendered for the thumbnail.
///
/// **The shapes are handed to the writer as a save into a file hands its new ones**: the engine's own shapes flattened to what a shape of an `.odg` is (`flatten_model`: a shape that holds
/// ODF data is in the file's form already, and is left as it is), which the writer's gate and this one count beside what the engine holds. Nothing the engine changes is the model's.
pub(crate) fn save_model(cmd: &str, s: &Session, doc: &Arc<Document>, variant: Variant, choice: Options) -> Result<Saved, EngineError> {
    let content_unchanged = s.content_unchanged() == Some(true);
    let mut options = WriteOptions::default();
    let max = options.limits.max_write_memory;
    let history = s.history();
    let mut walked = 0;
    // What the engine holds beside the drawing (the undo and redo steps): memory the save may not use, as for a save into a file. No earlier save of this drawing as an `.odg` stands behind
    // it (its working file is no `.odg`), so no page was given to a writer and no shape was flattened.
    let before = held_beside(doc, doc, history, &[], &[], max, &mut walked);
    if before >= max {
        return Err(failed(cmd, memory_words(before, max)));
    }
    // A drawing started from a built-in template holds the VSDX package data of DiaCraft's own skeleton, which no `.odg` says and which is no person's (`skeleton`): the writer is given the
    // drawing without it. The data a person's VSDX file gave stays, and the writer carries it inside the file.
    let bare = super::skeleton::without_skeleton(doc);
    let base: &Document = bare.as_ref().unwrap_or(doc);
    // **A save across**: a drawing of a VSDX file is given as it is but for the objects left out when asked (`across::onto`), which the drawing keeps.
    let mut given = across::onto::given(base, diacraft_doc::Format::Odf, choice).map_err(|why| failed(cmd, why))?;
    let without = given.doc.take();
    let base: &Document = without.as_ref().unwrap_or(base);
    // Whether the writer will write a VSDX object as an object frame and take its embedding out of what rides: every save into the file written leaves it out too (`LeftOut`).
    let carried_objects = diacraft_odg::without_carried_objects(base).map_err(|e| failed(cmd, model_refused_words(&e.to_string())))?.is_some();
    // The copies are held beside the shapes of the model they replace, and the writer holds its package and the file read again beside both (its gate counts the model more than once): so the
    // copies may take a third of what a save may hold, and a drawing whose shapes would pass that is refused in words with nothing made past it (a drawing that shares what its shapes hold, as
    // a pasted freeform of a thousand rows ten thousand times does, is a model of a few megabytes and copies to gigabytes).
    let room = max.saturating_sub(before) / COPY_SHARE;
    let flat = flatten_model(base, &Flattened::default(), room).map_err(|words| failed(cmd, model_refused_words(&words)))?;
    let to_write = flat.doc.as_ref().unwrap_or(base);
    let held = if flat.doc.is_none() && bare.is_none() && without.is_none() {
        before
    } else {
        held_beside(to_write, doc, history, &[], &[], max, &mut walked)
    };
    if held >= max {
        return Err(failed(cmd, memory_words(held, max)));
    }
    let mut warnings = flat.notes;
    warnings.extend(across::onto::left_out_words(&given));
    warnings.extend(across::style_link_words(base).map_err(|why| failed(cmd, why))?);
    // The thumbnail is rendered when the content is not what a file held (or no file did): the stamp decides, never the dirty flag.
    let thumbnail = if content_unchanged {
        None
    } else {
        match thumbnail(doc) {
            Ok(png) => Some(png),
            Err(why) => {
                warnings.push(thumbnail_words(true, &why));
                None
            }
        }
    };
    options.content_unchanged = content_unchanged;
    options.thumbnail = thumbnail;
    options.variant = Some(variant);
    options.drop_macros = choice.drop_macros;
    options.limits.max_write_memory = max.saturating_sub(held);
    let written = write_with(to_write, None, &options).map_err(|e| model_refusal(cmd, &e, held, max))?;
    // What the file carries is the save result's `carried` field, never a warning (the design's 5.6; ODF decision 8): the writer's notes of it, and of the signatures, are the field's.
    let is_carried = |note: &&diacraft_odg::SaveNote| matches!(note, diacraft_odg::SaveNote::Carried { .. } | diacraft_odg::SaveNote::SignatureStale);
    warnings.extend(written.notes.iter().filter(|note| !is_carried(note)).map(ToString::to_string));
    let signatures = if written.notes.contains(&diacraft_odg::SaveNote::SignatureStale) {
        across::classes::signatures_of(to_write, diacraft_doc::Format::Odf)
            .saturating_add(across::classes::signatures_of(to_write, diacraft_doc::Format::Vsdx))
            .max(1)
    } else {
        0
    };
    let carried = across::carried_into_odg(to_write, &written.notes, signatures);
    let macros_dropped = written
        .notes
        .iter()
        .map(|note| match note {
            diacraft_odg::SaveNote::MacroProjectLeftOut { parts } => *parts,
            _ => 0,
        })
        .sum();
    // **The check after the save**: the file is read back as an Open reads it, and compared with the drawing the writer was given before its shapes were flattened to what a shape of an
    // `.odg` is (so that what the flattening gave up, a link to a style sheet, is what the check sees).
    let mut checked = crate::check::check_odg(base, &written.bytes, choice.drop_macros);
    // A form of an earlier ODF version written in 1.4's: DiaCraft's read gives the source's back, and the card says it in the values group, as the allowances are.
    if let crate::check::Outcome::Checked(found) = &mut checked
        && written.work.forms_1_4
    {
        found.as_values = true;
    }
    let checked = Some(checked);
    let pages: Vec<Arc<diacraft_doc::Page>> = to_write.pages.iter().cloned().collect();
    let notes: Vec<serde_json::Value> = note::note(&note::find(to_write)).into_iter().collect();
    Ok(Saved {
        bytes: written.bytes,
        // What the file was written without, which every save into it leaves out too (`save`): DiaCraft's own skeleton when the drawing held it, and what the Save As was asked to leave out.
        state: OdgState::saved(
            written.index,
            flat.made,
            pages,
            super::LeftOut { skeleton: bare.is_some(), asked: choice, carried_objects, from_model: true },
        ),
        warnings,
        notes,
        edited: !content_unchanged,
        carried,
        macros_dropped,
        checked,
    })
}

/// Whether a save renders the picture of the first page: an edited save does, and the first save of a drawing with no file yet does, and **a flat file has no
/// thumbnail to write**, so none is rendered for it, whatever else is true.
fn wants_thumbnail(dirty: bool, first_save: bool, flat_file: bool) -> bool {
    (dirty || first_save) && !flat_file
}

/// What the save says when the picture of the first page cannot be made. A first save has nothing of its own to renew (the package may have a thumbnail of its
/// own, which it keeps, or none): what is true of it is that none was written. Any other save keeps the picture the file has.
fn thumbnail_words(first_save: bool, why: &str) -> String {
    if first_save {
        format!("the picture of the first page could not be made, so no thumbnail was written: {why}")
    } else {
        format!("the picture of the first page in the file was not renewed: {why}")
    }
}

/// What a save that would hold more than it may says, with what the engine holds beside the drawing when that is much: the limit is the whole of what a save may
/// hold, never what is left of it once the engine's share is taken (a refusal that said "more than 0 bytes" told a person nothing). It has its own way out,
/// which is not the undo of a change.
fn memory_words(held: u64, max: u64) -> String {
    let mib = |bytes: u64| bytes.div_ceil(1024 * 1024);
    let mut words = format!("the drawing could not be saved: the save would hold more than {} MiB of memory", mib(max));
    if held >= MUCH_HELD {
        words.push_str(&format!(
            ": the drawing and the file read again beside it, and about {} MiB that the engine keeps beside them (the earlier versions of the drawing that undo \
             keeps, and what the last save was given). A small change after undoing the big one drops the versions undone; opening the file again drops them all, \
             and the changes not saved",
            mib(held)
        ));
    } else {
        words.push_str(" (the drawing and the file read again beside it)");
    }
    words.push('.');
    crate::shown(&words, MOST_SHOWN)
}

/// What the engine holds beside the drawing that a refusal for memory names: less is not worth a sentence.
const MUCH_HELD: u64 = 16 * 1024 * 1024;

/// The error of a save the writer refused, in the writer's words. A refusal for want of memory says what the engine holds beside the drawing, against the limit
/// a save has (the writer's own figure is what is left of it, which says nothing).
fn refusal(cmd: &str, e: &WriteError, beside: u64, max: u64, flat: bool) -> EngineError {
    match e {
        WriteError::Limit { what, .. } if what.contains("bytes of memory a save holds") => failed(cmd, memory_words(beside, max)),
        WriteError::MediaType(_) => {
            let mut words = crate::shown(
                &format!("the drawing could not be saved: {}.", e.to_string().trim_end_matches('.')),
                MOST_SHOWN.saturating_sub(KIND_WAY_OUT.len()),
            );
            words.push_str(KIND_WAY_OUT);
            failed(cmd, words)
        }
        other => failed(cmd, refused_words(&other.to_string(), flat)),
    }
}

/// The first page as a PNG of at most [`THUMBNAIL_PIXELS`] pixels on its long side, on white paper, as the window draws it, **within the limits of a picture that goes into a
/// file** (`Limits::preview`): counts of work and no time, so that the same drawing gives the same bytes however loaded the machine is. A drawing past them has a
/// picture of what the counts drew, cut at a point the drawing sets.
pub(crate) fn thumbnail(doc: &Document) -> Result<Vec<u8>, String> {
    thumbnail_with(doc, Limits::preview(), thumbnail_clock())
}

/// The clock the picture is drawn against: the machine's, or in a test the one the test gave its thread (`tests::with_clock`).
fn thumbnail_clock() -> Clock {
    #[cfg(test)]
    if let Some(clock) = tests::CLOCK.with(|c| c.borrow().clone()) {
        return clock;
    }
    Clock::system()
}

/// [`thumbnail`] drawn within `limits`, reading the time from `clock`.
fn thumbnail_with(doc: &Document, limits: Limits, clock: Clock) -> Result<Vec<u8>, String> {
    let (rendered, _) = render_thumbnail(doc, limits, clock)?;
    encode_png(rendered.width, rendered.height, &unpremultiply(&rendered.rgba_premul), None).map_err(|e| e.to_string())
}

/// The frame of [`thumbnail_with`], and the work it was charged (`Renderer::work_done`).
pub(super) fn render_thumbnail(doc: &Document, limits: Limits, clock: Clock) -> Result<(diacraft_render::Rendered, u64), String> {
    let page = doc.pages.iter().find(|p| !p.background).or_else(|| doc.pages.iter().next()).ok_or("the drawing has no page")?;
    let (w, h) = page_paper_size(doc, page.id).ok_or("the page has no size")?;
    // A little under the bound, so that rounding the page's size up never passes it.
    let ppi = (THUMBNAIL_PIXELS - 1.0) / w.max(h);
    let view = View::fit_page(doc, page.id, ppi).ok_or("the page cannot be drawn at that size")?;
    let mut renderer = Renderer::with_shared_fonts(crate::measure::renderer_fonts());
    renderer.set_paper(Some(Rgba::WHITE));
    renderer.set_limits(limits);
    renderer.set_clock(clock);
    // The walk of a big page costs more than its few pixels: it is shared out over the machine's cores less one (`DIACRAFT_THREADS` says otherwise).
    renderer.set_parallel(Parallel::with_threads(default_threads()));
    let rendered = renderer.render(doc, page.id, &view);
    if rendered.rgba_premul.is_empty() {
        return Err("the page could not be drawn".to_owned());
    }
    Ok((rendered, renderer.work_done()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    thread_local! {
        /// The clock the thumbnails of this thread are drawn against, while a test holds one (`with_clock`): a save made inside it reads no other.
        pub(super) static CLOCK: std::cell::RefCell<Option<Clock>> = const { std::cell::RefCell::new(None) };
    }

    /// Runs `f` with the thumbnails of this thread drawn against `clock`, and puts the machine's clock back after.
    fn with_clock<T>(clock: Clock, f: impl FnOnce() -> T) -> T {
        CLOCK.with(|c| *c.borrow_mut() = Some(clock));
        let out = f();
        CLOCK.with(|c| *c.borrow_mut() = None);
        out
    }

    /// A clock for which every deadline has passed by the time a frame looks: each reading is an hour after the one before.
    fn past_every_deadline() -> Clock {
        Clock::stepping(std::time::Duration::from_secs(3_600))
    }

    /// A drawing whose picture has something to lose to a deadline: forty boxes, each with a label, on the first page.
    fn labelled_boxes() -> crate::Session {
        let mut s = crate::Session::new();
        s.execute("file.new", &serde_json::json!({})).unwrap();
        let page = s.document().unwrap().pages.iter().next().unwrap().id;
        for k in 0..40u32 {
            let (x, y) = (1.0 + f64::from(k % 8), 1.0 + f64::from(k / 8));
            let made = s.execute("shape.drop", &serde_json::json!({ "master": "rectangle", "page": page, "x": x, "y": y })).unwrap();
            s.execute("shape.set_text", &serde_json::json!({ "id": made["id"], "text": format!("Step {k}") })).unwrap();
        }
        s
    }

    /// **A save writes the same bytes whatever the clock says.** The picture of the first page that an `.odg` and a `.diacraft` save write is drawn within a budget of
    /// work, never of time: the same drawing saved with a clock for which every deadline has passed gives the file it gives with the machine's clock. (Drawn within the window's
    /// limits, the frame stopped at its first look at the clock and the picture was the bare paper.)
    #[test]
    fn a_save_writes_the_same_bytes_when_every_deadline_has_passed() {
        let s = labelled_boxes();
        let doc = Arc::new(s.document().unwrap().clone());
        let odg = || save_model("file.save_as", &s, &doc, Variant::Drawing, Options::default()).unwrap();
        let (plain, late) = (odg(), with_clock(past_every_deadline(), odg));
        assert!(plain.edited && plain.warnings.is_empty() && late.warnings.is_empty(), "{:?} {:?}", plain.warnings, late.warnings);
        let odg_same = plain.bytes == late.bytes;
        let native = || crate::nativefile::save::save("file.save_as", &s, &doc, None, diacraft_native::Kind::Drawing, true).unwrap();
        let (plain, late) = (native(), with_clock(past_every_deadline(), native));
        assert!(plain.warnings.is_empty() && late.warnings.is_empty(), "{:?} {:?}", plain.warnings, late.warnings);
        let native_same = plain.bytes == late.bytes;
        assert!(odg_same && native_same, "the same file with every deadline passed: .odg {odg_same}, .diacraft {native_same}");
    }

    /// A drawing of `count` boxes that each hold the same text of 16,200 characters: the text is laid out once, and painted for every box.
    fn the_same_long_text_in(count: usize) -> crate::Session {
        let mut s = crate::Session::new();
        s.execute("file.new", &serde_json::json!({})).unwrap();
        let page = s.document().unwrap().pages.iter().next().unwrap().id;
        let made = s.execute("shape.drop", &serde_json::json!({ "master": "rectangle", "page": page, "x": 4.0, "y": 4.0 })).unwrap();
        let id = u32::try_from(made["id"].as_u64().unwrap()).unwrap();
        let long = "The quick brown fox jumps over the lazy dog. ".repeat(360);
        s.execute("shape.set_text", &serde_json::json!({ "id": id, "text": long })).unwrap();
        s.edit("Test edit", |doc| {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let template = page.shapes.iter().find(|x| x.id == id).unwrap().clone();
            for k in 1..count {
                page.shapes.push(Arc::new(diacraft_doc::Shape { id: id + u32::try_from(k).unwrap(), ..(*template).clone() }));
            }
            Ok(())
        })
        .unwrap();
        s
    }

    /// The hostile drawing: the same long text in three hundred boxes is five million glyphs to paint (laid out once). Drawn by the clock, the picture stopped
    /// wherever the machine's speed left it, and with no time limit it took thirteen seconds for two thousand boxes. **The picture of a save is cut by its work, at
    /// the box the drawing sets, within the preview's budget**: the same bytes with every deadline passed, and the frame says what the budget left out.
    #[test]
    fn the_picture_of_a_hostile_drawing_is_cut_by_its_work_and_a_save_of_it_is_the_same_file_whatever_the_clock_says() {
        let s = the_same_long_text_in(300);
        let doc = Arc::new(s.document().unwrap().clone());
        let (rendered, work) = render_thumbnail(&doc, Limits::preview(), Clock::system()).unwrap();
        assert!(work <= diacraft_render::limits::PREVIEW_MAX_WORK, "{work}");
        assert!(rendered.count(diacraft_render::SkipKind::WorkLimit) >= 1, "{:?}", rendered.skipped);
        let timed = [diacraft_render::SkipKind::FrameBudget, diacraft_render::SkipKind::TextBudget];
        assert!(timed.iter().all(|k| rendered.count(*k) == 0), "{:?}", rendered.skipped);
        let odg = || save_model("file.save_as", &s, &doc, Variant::Drawing, Options::default()).unwrap();
        let (plain, late) = (odg(), with_clock(past_every_deadline(), odg));
        assert!(plain.warnings.is_empty(), "the picture was written: {:?}", plain.warnings);
        assert!(plain.bytes == late.bytes, "the .odg of the hostile drawing written with every deadline passed is another file");
        let native = || crate::nativefile::save::save("file.save_as", &s, &doc, None, diacraft_native::Kind::Drawing, true).unwrap();
        assert!(native().bytes == with_clock(past_every_deadline(), native).bytes, "the .diacraft is the same file");
    }

    /// The clock a test gives its thread is the one the picture is drawn against: within the window's limits, which hold times, the picture drawn with every deadline passed is
    /// the bare paper, and another picture than the one the machine's clock gives (so the test above has a clock that reaches the renderer).
    #[test]
    fn the_clock_a_test_holds_reaches_the_picture() {
        let s = labelled_boxes();
        let doc = s.document().unwrap();
        let window = |clock: Clock| thumbnail_with(doc, Limits::default(), clock).unwrap();
        assert_ne!(window(Clock::system()), window(past_every_deadline()));
        // And the clock the picture of a save is drawn against is the one the thread holds.
        assert_ne!(window(Clock::system()), with_clock(past_every_deadline(), || window(thumbnail_clock())));
    }

    /// The width and the height of a PNG, from its header: the signature, then the IHDR chunk with the two as big-endian numbers.
    fn size_of(png: &[u8]) -> (u32, u32) {
        (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]))
    }

    /// The thumbnail is at most 256 pixels on its long side whatever the page's size: a size that rounds up past it is a size the figure is made under.
    #[test]
    fn the_thumbnail_never_passes_256_pixels_whatever_the_size_of_the_page() {
        let mut s = crate::Session::new();
        s.execute("file.new", &serde_json::json!({})).unwrap();
        for (w, h) in
            [(3.3, 2.0), (7.1, 7.1), (11.0, 8.5), (0.7, 12.3), (33.3, 3.0), (4.4, 4.4), (5.5, 1.1), (6.6, 9.9), (2.2, 2.2), (8.3, 11.7), (1.9, 1.3)]
        {
            s.execute("page.size", &serde_json::json!({ "width": w, "height": h })).unwrap();
            let (width, height) = size_of(&thumbnail(s.document().unwrap()).unwrap());
            assert!(width.max(height) <= 256 && width.max(height) >= 250, "{w} x {h} inches: {width} x {height} pixels");
        }
    }

    /// The picture is of the first page that is not a background page, and of a background page only when there is no other.
    #[test]
    fn the_thumbnail_is_of_the_first_page_that_is_not_a_background() {
        let mut s = crate::Session::new();
        s.execute("file.new", &serde_json::json!({})).unwrap();
        s.execute("page.add", &serde_json::json!({})).unwrap();
        let second_page = s.document().unwrap().pages.get(1).unwrap().id;
        s.execute("page.size", &serde_json::json!({ "page": second_page, "width": 4.0, "height": 2.0 })).unwrap();
        let first = size_of(&thumbnail(s.document().unwrap()).unwrap());
        // The first page (the blank drawing's, taller than wide) is a background: the second, 4 by 2 inches, is the picture.
        s.edit("Test edit", |doc| {
            Arc::make_mut(doc.pages.get_mut(0).unwrap()).background = true;
            Ok(())
        })
        .unwrap();
        let second = size_of(&thumbnail(s.document().unwrap()).unwrap());
        assert_ne!(first, second);
        assert!(second.0 > second.1, "the page of 4 by 2 inches: {second:?}");
        // With every page a background, the first is drawn.
        s.edit("Test edit", |doc| {
            Arc::make_mut(doc.pages.get_mut(1).unwrap()).background = true;
            Ok(())
        })
        .unwrap();
        assert_eq!(size_of(&thumbnail(s.document().unwrap()).unwrap()), first);
    }

    /// The words of a render that fails say what is true of the save: on a first save nothing was written (there was nothing to renew),
    /// and on any other the file's own picture was kept; neither says what the other does, and both give the cause.
    #[test]
    fn the_words_of_a_picture_that_cannot_be_made_say_what_is_true_of_the_save() {
        let first = thumbnail_words(true, "the page has no size");
        assert_eq!(first, "the picture of the first page could not be made, so no thumbnail was written: the page has no size");
        let other = thumbnail_words(false, "the page has no size");
        assert_eq!(other, "the picture of the first page in the file was not renewed: the page has no size");
        assert!(!first.contains("renewed") && !other.contains("no thumbnail was written"));
    }

    /// A refusal for want of memory says what the engine holds beside the drawing when it holds much, and only then, against the limit a save has (never what
    /// is left of it); every other refusal says the way out, and the way out is never cut from a cause that is long.
    #[test]
    fn a_refusal_names_the_limit_and_what_is_held_beside_the_drawing_and_always_says_the_way_out() {
        let memory = WriteError::Limit { what: "bytes of memory a save holds (the model and the file read again beside it)", max: 0 };
        let max = 2900 * 1024 * 1024;
        let with = refusal("file.save", &memory, 300 * 1024 * 1024, max, false).to_string();
        assert!(with.contains("more than 2900 MiB") && with.contains("about 300 MiB") && with.contains("opening the file again"), "{with}");
        assert!(
            !with.contains("more than 0 ") && !with.contains("edit.undo"),
            "never the limit that is left, and the way out of a memory refusal is its own: {with}"
        );
        let little = refusal("file.save", &memory, 1024 * 1024, max, false).to_string();
        assert!(little.contains("more than 2900 MiB") && !little.contains("engine keeps"), "{little}");
        // Sixteen mebibytes is much, and less is not worth a sentence.
        for (held, named) in [(8 * 1024 * 1024, false), (16 * 1024 * 1024 - 1, false), (16 * 1024 * 1024, true), (17 * 1024 * 1024, true)] {
            let said = refusal("file.save", &memory, held, max, false).to_string();
            assert_eq!(said.contains("engine keeps"), named, "{held} bytes held: {said}");
        }
        let other = WriteError::Limit { what: "attributes written", max: 5 };
        let words = refusal("file.save", &other, 300 * 1024 * 1024, max, false).to_string();
        assert!(!words.contains("engine keeps") && words.contains("attributes written"), "{words}");
        let dashed = refusal("file.save", &WriteError::Unsupported("a dashed line"), 300 * 1024 * 1024, max, false).to_string();
        assert!(dashed.contains("saving does not yet write this change: a dashed line") && !dashed.contains("engine keeps"), "{dashed}");
        // Every other refusal says the way out, and a cause too long for the words is cut and the way out is not.
        for said in [words, dashed, refused_words(&"x".repeat(5_000), false)] {
            assert!(said.ends_with(WAY_OUT) && said.contains("edit.undo") && said.chars().count() <= MOST_SHOWN + 1, "{said}");
        }
        assert!(with.chars().count() <= MOST_SHOWN + 1 && little.chars().count() <= MOST_SHOWN + 1);
    }

    /// A save the file cannot make as another kind of document is not one a change of the drawing caused, so it does not say to undo
    /// a change; it says what the writer says, that nothing was written, and what to do.
    #[test]
    fn a_refusal_to_save_as_another_kind_does_not_tell_a_person_to_undo_a_change() {
        let cause =
            WriteError::MediaType("the manifest has no root entry (a row for the package itself, with the full path /) to state the media type in");
        let words = refusal("file.save", &cause, 300 * 1024 * 1024, 2900 * 1024 * 1024, false).to_string();
        assert!(words.contains("no root entry") && words.contains("Nothing was written") && words.contains("another file"), "{words}");
        assert!(!words.contains("edit.undo") && !words.contains("Undo"), "{words}");
        assert!(words.ends_with(KIND_WAY_OUT) && words.chars().count() <= MOST_SHOWN + 1, "{words}");
        // Every other cause of a refusal still says to undo.
        let other = refusal("file.save", &WriteError::Unsupported("a dashed line"), 0, 2900 * 1024 * 1024, false).to_string();
        assert!(other.contains("edit.undo"), "{other}");
    }

    /// The save of a drawing that came from a flat file writes a flat document: unedited, the file's own bytes and no edit, and the words of
    /// a refusal say the way out of a flat file. No thumbnail is rendered for it (a flat file has none), whatever the caller asked.
    #[test]
    fn a_drawing_that_came_from_a_flat_file_is_saved_as_a_flat_file_and_unedited_as_its_own_bytes() {
        const FLAT: &str = concat!(
            r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
            r#"office:mimetype="application/vnd.oasis.opendocument.graphics" office:version="1.3">"#,
            r#"<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body></office:document>"#
        );
        let (doc, report) = diacraft_odg::read(FLAT.as_bytes(), &diacraft_odg::Limits::default()).unwrap();
        let source = OpenFile::new(std::path::Path::new("/x/a.fodg"), FLAT.as_bytes().to_vec()).with_odg(OdgState::new(report.index));
        for dirty in [false, true] {
            let saved = save("file.save", &Arc::new(doc.clone()), &source, dirty, None, None, Options::default()).unwrap();
            assert_eq!(saved.bytes, FLAT.as_bytes(), "dirty {dirty}: the file's own bytes");
            assert!(!saved.edited && saved.warnings.is_empty(), "dirty {dirty}: {:?}", saved.warnings);
        }
        // The way out of a refusal is a flat file's, in the words of the odg way out and with its limit, and every cause says it.
        let flat = refused_words("a page added", true);
        assert!(flat.ends_with(FLAT_WAY_OUT) && flat.contains("edit.undo") && !flat.contains(".odg"), "{flat}");
        assert_eq!(
            FLAT_WAY_OUT,
            " Undo the change (edit.undo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from a flat file is saved only as a flat file by Save."
        );
        assert!(!FLAT_WAY_OUT.contains("until saving in other formats is written"), "Save As writes a flat drawing as a .diacraft file now");
        assert!(refused_words("a page added", false).ends_with(WAY_OUT));
        // **The way out of an `.odg` names Save As too**: no way out says saving in other formats is still to be written.
        assert_eq!(
            WAY_OUT,
            " Undo the change (edit.undo) to save, or use Save As to save the drawing as a .diacraft file, which holds it: a drawing opened from an .odg is saved only as an .odg by Save."
        );
        for words in [WAY_OUT, WAY_OUT_REDO, WAY_OUT_REDO_STYLE, FLAT_WAY_OUT, FLAT_WAY_OUT_REDO, FLAT_WAY_OUT_REDO_STYLE] {
            assert!(
                !words.contains("until saving in other formats is written") && words.contains("use Save As to save the drawing as a .diacraft file"),
                "{words}"
            );
        }
        let redo = refused_words(&format!("x {}", crate::odgfile::flatten::PUT_BACK), true);
        assert!(redo.contains("edit.redo") && redo.contains("flat file") && !redo.contains(".odg"), "{redo}");
        let style = refused_words(&format!("x {STYLE_TAKEN_BACK}"), true);
        assert!(style.contains("edit.redo") && style.contains("flat file") && !style.contains(".odg"), "{style}");
        // A flat file has no thumbnail: none is rendered for it, and for another file it is as it was.
        for (dirty, first_save, flat_file, wanted) in [
            (false, false, false, false),
            (true, false, false, true),
            (false, true, false, true),
            (true, true, false, true),
            (false, false, true, false),
            (true, false, true, false),
            (false, true, true, false),
            (true, true, true, false),
        ] {
            assert_eq!(wants_thumbnail(dirty, first_save, flat_file), wanted, "dirty {dirty}, first save {first_save}, flat file {flat_file}");
        }
        // The words of the two notes fit the room the window gives them (it shows 300 characters).
        for words in [crate::odgfile::FLAT_OPEN_NOTE, crate::odgfile::FODG_NOT_OFFERED] {
            assert!(words.chars().count() < 300, "{words}");
        }
    }
}
