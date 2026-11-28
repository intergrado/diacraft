//! Stencil files: open one, close one, and ask a master for its icon. `stencil.list` and `shape.drop` are in `shape.rs`.
//!
//! **A stencil is read like a drawing** (`file.open`): the path goes through `file::read_file_stamped` (a folder, a device and a pipe are
//! refused before a byte is read, a pipe swapped in later is opened without waiting and refused, a file past the cap is refused from its size
//! and from the bytes that came), then `diacraft_vsdx::read` with its default limits. A link is read through to the file it leads to, as
//! `file.open` reads it (gaps A2: with the automation roots in force the path must lie in a read root, `file_access`, and with none a program names any path the user can read; unlike a
//! save, a read changes nothing on the disk). **Macros never run**: a `.vssm`'s project is a part the reader does not take, and nothing here
//! looks at one. Nothing in an error holds the path.
//!
//! What a stencil is kept as, and what it answers, is `stencil/opened.rs`.

use std::path::Path;

use diacraft_vsdx::{Kind, check_extension};
use serde_json::{Value, json};

use super::file::{load_for_read, read_package};
use super::params::optional_whole;
use super::{CommandSpec, always, has_document, optional_text, required_text};
use crate::Session;
use crate::error::{Result, bad, cannot_open, failed, shown};
use crate::memory::Reservation;
use crate::nativefile;
use crate::stencil::quick::{self, Held, View};
use crate::stencil::{self, IconError, MAX_OPEN_ITEMS, MAX_OPEN_MEDIA_BYTES, MAX_OPEN_STENCILS, MAX_OPEN_XML_BYTES, ModelCost, OpenedStencil};

/// The most warnings an open reports one by one; the report counts the rest.
const WARNINGS_SHOWN: usize = 20;

/// The longest path a command takes, in bytes: no system names a longer one.
const MAX_PATH_BYTES: usize = 4_096;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "stencil.open",
            "Open Stencil",
            ["Home", "Shapes", "More Shapes"],
            None,
            "path: the stencil file to open (.diacraft-stencil, .vssx or .vssm), and not a drawing or a template; \
             id: text, the id of a built-in stencil that is not open (`available` in stencil.list: basic, flowchart, decorative, arrows, gane-sarson, cross-functional, orgchart, uml-class, uml-usecase, uml-sequence, uml-activity, uml-state, uml-component, bpmn, c4, network, computers, cloud, crows-foot or chen, the thirteen after orgchart being there once they have masters) to open it, and give path or id and not both",
            always,
            open
        ),
        cmd!(
            "stencil.close",
            "Close Stencil",
            ["Home", "Shapes", "More Shapes"],
            None,
            "id: text, the id of an opened stencil (see stencil.list), and not a built-in one",
            always,
            close
        ),
        cmd!(
            query "stencil.search",
            "Search for Shapes",
            ["Home", "Shapes", "More Shapes"],
            None,
            "text: text, the words to look for, at most 256 characters and 16 different words of at most 64 characters (a word is a run of letters and digits and case does not matter), \
             and a word matches a whole word of a master's name or of its search words (a stencil file's `ShapeKeywords`) and never a part of one: every built-in stencil, open or not, \
             and every open stencil file is searched, a master that is hidden is not found, at most 100,000 masters and 16 MiB of names and search words are read (the answer's \
             `complete` says whether the search reached the end) and at most 500 masters are listed (`matches` counts them all); \
             mode: text, optional, `all` (the default: a master must have every word, AND) or `any` (one word is enough, OR)",
            always,
            search
        ),
        cmd!(
            query "stencil.quick",
            "Quick Shapes",
            ["Home", "Shapes"],
            None,
            "stencil: text, optional, the id of a stencil in stencil.list, and `quick` or nothing for the Quick Shapes stencil, which gathers the groups of every open stencil",
            always,
            quick
        ),
        cmd!(
            "stencil.add_quick",
            "Add to Quick Shapes",
            ["Home", "Shapes"],
            None,
            "stencil: text, the id of a stencil in stencil.list, built in or opened from a file; master: text or whole number, the master's id or its name in that stencil; \
             at: whole number, optional, the place in the stencil's Quick Shapes from 0 (the end when it is left out, and a master that is there already is moved to it)",
            has_document,
            add_quick
        ),
        cmd!(
            "stencil.remove_quick",
            "Remove from Quick Shapes",
            ["Home", "Shapes"],
            None,
            "stencil: text, the id of a stencil in stencil.list; master: text or whole number, the master's id or its name in that stencil, which goes below the divider again",
            has_document,
            remove_quick
        ),
        cmd!(
            "stencil.reset",
            "Reset Stencil",
            ["Home", "Shapes"],
            None,
            "stencil: text, the id of a stencil in stencil.list: its Quick Shapes are the stencil's own again",
            has_document,
            reset_stencil
        ),
        cmd!(
            query "stencil.icon",
            "Master Icon",
            [],
            None,
            "stencil: text, the id of an opened stencil; master: the master's id (a whole number or its text) or its name",
            always,
            icon
        ),
    ]
}

/// The master a command names: `master` as text, or as a whole number (its `ID`).
pub(super) fn master_key(p: &Value, cmd: &str) -> Result<String> {
    match p.get("master") {
        None | Some(Value::Null) => Err(bad(cmd, "missing `master`")),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Number(n)) if n.is_u64() => Ok(n.to_string()),
        Some(_) => Err(bad(cmd, "`master` must be text (an id or a name) or a whole number (an id)")),
    }
}

/// Reads the stencil file at `path` and adds it to the opened stencils → `{id, name, already_open, masters, masters_total, file: {variant, bytes,
/// warnings: [..first 20..], warning_count, skipped_count}}`; `masters` is as `stencil.list` has them. **The same file opened again is the stencil
/// that is open** (`already_open` true, `file` has only its size): it is not read again. A file that is not a stencil (a drawing or a template, by
/// its content type and not its name), that is damaged or past a limit of the reader, or that cannot be read, is refused in words that hold no
/// text of the file, and nothing is changed.
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.open";
    let text = match (optional_text(p, "path", ID)?, optional_text(p, "id", ID)?) {
        (Some(_), Some(_)) => return Err(bad(ID, "give `path` (a stencil file) or `id` (a built-in stencil), not both")),
        (None, Some(id)) => return open_builtin(s, id),
        (Some(path), None) => path,
        (None, None) => return Err(bad(ID, "missing `path` (a stencil file to open) or `id` (a built-in stencil to open)")),
    };
    if text.len() > MAX_PATH_BYTES {
        return Err(bad(ID, format!("`path` is longer than {MAX_PATH_BYTES} bytes")));
    }
    let path = Path::new(text);
    // The file a link leads to names the stencil: the same file by two names is one stencil. A path that leads nowhere is read below and refused there.
    // A file in memory (the browser's) has one name, which is its place in the store.
    let canonical = if crate::memfs::active() { Some(path.to_path_buf()) } else { std::fs::canonicalize(path).ok() };
    if let Some(found) = canonical.as_deref().and_then(|c| s.stencils().of_file(c)) {
        return Ok(answer(found, true, json!({ "bytes": found.file_bytes() })));
    }
    refuse_if_full(s)?;
    // The room for the file is taken before it is loaded, a slice beside the read that runs and no wait: a file the room cannot hold is refused from its size.
    // (The reservation is bound before the bytes, so it is dropped after them: the room is given back only once the file is out of memory.)
    let (reservation, _stamp, bytes) = load_for_read(s, ID, path, crate::memory::reserve_read_brief)?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    // The file's own bytes decide, and a `.diacraft` file first (the design's 1.2). The read holds what is left of the process's budget while it runs, and the stencil makes its own
    // hold: the reservation is kept until it has (`_reading`, dropped when this ends), for either reader.
    let read = if diacraft_native::sniff(&bytes).is_some() {
        read_native_stencil(s, &bytes, path, &reservation)?
    } else {
        read_vsdx_stencil(s, &bytes, path, &reservation)?
    };
    let ReadStencil { doc, cost, warnings, warning_count, skipped_count, variant } = read;
    drop(bytes);
    if s.stencils().fits(&cost).is_err() {
        return Err(failed(
            ID,
            format!(
                "the open stencils together would hold more than one drawing file may ({} MiB of XML, {} million items and {} MiB of pictures): close one with stencil.close first",
                MAX_OPEN_XML_BYTES / (1024 * 1024),
                MAX_OPEN_ITEMS / 1_000_000,
                MAX_OPEN_MEDIA_BYTES / (1024 * 1024)
            ),
        ));
    }
    let name = path.file_stem().and_then(|n| n.to_str()).map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| "Stencil".to_owned(), shown);
    let id = s.stencils_mut().next_id();
    let stencil = s.stencils_mut().add(OpenedStencil::new(id, name, canonical, size, cost, doc));
    let file = json!({ "variant": variant, "bytes": size, "warnings": warnings, "warning_count": warning_count, "skipped_count": skipped_count });
    Ok(answer(&stencil, false, file))
}

/// What reading a stencil file gave: the model, what it cost, the reader's warnings (the first [`WARNINGS_SHOWN`]), how many there were, how many parts the reader kept in the file's bytes
/// without modelling them, and the extension of the type the file is.
struct ReadStencil {
    doc: diacraft_doc::Document,
    cost: ModelCost,
    warnings: Vec<String>,
    warning_count: u64,
    skipped_count: u64,
    variant: Option<&'static str>,
}

/// A VSDX stencil (`.vssx` or `.vssm`): read by the VSDX reader under a brief reservation (a slice beside the read that runs, none waited for), and refused in words when it is a drawing
/// or a template.
fn read_vsdx_stencil(s: &Session, bytes: &[u8], path: &Path, reservation: &Reservation) -> Result<ReadStencil> {
    const ID: &str = "stencil.open";
    let (doc, report) = read_package(s, ID, bytes, reservation)?;
    match report.variant {
        Some(v) if v.kind == Kind::Stencil => {}
        Some(v) => {
            return Err(failed(
                ID,
                format!(
                    "this is a {}, not a stencil: open it with file.open, or choose a .diacraft-stencil, .vssx or .vssm file",
                    v.kind_word_macros()
                ),
            ));
        }
        None => return Err(failed(ID, "this is not a stencil: choose a .diacraft-stencil, .vssx or .vssm file")),
    }
    // What the model took of the reader's limits, and not the size of the file: a small file may inflate to a very large model (S6).
    let cost = ModelCost::of(&report.stats);
    let mut warnings: Vec<String> = report.warnings.iter().take(WARNINGS_SHOWN).cloned().collect();
    let mut warning_count: u64 = report.warning_counts.values().sum();
    if let Some(extension) = path.extension().and_then(|e| e.to_str())
        && let Some(why) = check_extension(report.variant, extension)
    {
        warnings.insert(0, why);
        warning_count = warning_count.saturating_add(1);
    }
    let skipped_count: u64 = report.skipped_counts.values().sum();
    Ok(ReadStencil { doc, cost, warnings, warning_count, skipped_count, variant: report.variant.map(|v| v.extension()) })
}

/// A `.diacraft` file: read by `nativefile` **under the same kind of reservation** as a VSDX stencil's (`reserve_read_brief`), and refused in words when it is a drawing or a template.
/// What it cost is what the codec's work says (the bytes that came out of the archive, the events the XML reader read) and what the model holds.
fn read_native_stencil(s: &Session, bytes: &[u8], path: &Path, reading: &Reservation) -> Result<ReadStencil> {
    const ID: &str = "stencil.open";
    let held = nativefile::open::held_by(s, reading);
    let read = nativefile::read_native(bytes, &nativefile::open::limits_of(reading))
        .map_err(|e| cannot_open(ID, nativefile::open::problem(&e, &held), nativefile::open::refused(&e, &held, reading.beside_reads())))?;
    let kind = read.opened.kind;
    if kind != diacraft_native::Kind::Stencil {
        return Err(failed(
            ID,
            format!("this is a DiaCraft {}, not a stencil: open it with file.open, or choose a .diacraft-stencil, .vssx or .vssm file", kind.word()),
        ));
    }
    let cost = ModelCost { xml_bytes: read.opened.work.inflated, items: read.opened.work.xml_events, media_bytes: 0, memory_bytes: read.model_bytes };
    let mut warnings: Vec<String> = read.warnings.iter().take(WARNINGS_SHOWN).cloned().collect();
    let mut warning_count = u64::try_from(read.warnings.len()).unwrap_or(u64::MAX);
    if let Some(extension) = path.extension().and_then(|e| e.to_str())
        && let Some(why) = nativefile::extension_note(kind, extension)
    {
        warnings.insert(0, why);
        warning_count = warning_count.saturating_add(1);
    }
    let skipped_count = u64::try_from(read.opened.kept_parts).unwrap_or(u64::MAX);
    Ok(ReadStencil { doc: read.doc, cost, warnings, warning_count, skipped_count, variant: Some(nativefile::variant_word(kind)) })
}

/// Opens a built-in stencil that a template (or a person) left out → `{id, name, builtin: true, already_open, masters_total}`.
fn open_builtin(s: &mut Session, id: &str) -> Result<Value> {
    const ID: &str = "stencil.open";
    let found = stencil::find_stencil(id).ok_or_else(|| bad(ID, format!("no built-in stencil `{}` (see `available` in stencil.list)", shown(id))))?;
    let newly = s.stencils_mut().open_builtin(found.id).unwrap_or(false);
    Ok(json!({ "id": found.id, "name": found.name, "builtin": true, "already_open": !newly, "masters_total": found.masters().len() }))
}

/// Refuses, in words, when no more stencils may be opened.
fn refuse_if_full(s: &Session) -> Result<()> {
    match s.stencils().room_for_one() {
        Ok(()) => Ok(()),
        Err(_) => {
            Err(failed("stencil.open", format!("{MAX_OPEN_STENCILS} stencils are open, and that is the most: close one with stencil.close first")))
        }
    }
}

fn answer(stencil: &OpenedStencil, already_open: bool, file: Value) -> Value {
    let mut v = stencil.to_json();
    if let Some(map) = v.as_object_mut() {
        map.remove("builtin");
        map.insert("already_open".to_owned(), json!(already_open));
        map.insert("file".to_owned(), file);
    }
    v
}

/// Closes the opened stencil `id` → `{id, closed: true}`. A drawing that took masters from it keeps them. A built-in stencil stays (it has no close
/// control in the window; a template decides which of them are open, and `stencil.open {id}` opens one that is not).
fn close(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.close";
    let id = required_text(p, "id", ID)?;
    if stencil::builtin_stencil(id).is_some() {
        return Err(bad(ID, format!("`{id}` is a built-in stencil, which stays")));
    }
    if !s.stencils_mut().close(id) {
        return Err(bad(ID, format!("no stencil `{}` is open (see stencil.list)", shown(id))));
    }
    Ok(json!({ "id": id, "closed": true }))
}

/// Searches the stencils for the words of `text` -> `{text, mode, words, matches, listed, truncated, searched, total, complete, results: [{id, name, builtin, open, masters}], notes?}`:
/// `results` has a group for each stencil that holds a match, in the order the stencils are searched (the built-in ones that are open, the other built-in ones, the stencil files),
/// and a master is as `stencil.list` has it, so it drops by its `id` (with `stencil` for a stencil file). A query that is not one (no words, too long, too many words) is refused in
/// words. Nothing is changed, and no document is needed.
fn search(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.search";
    let text = required_text(p, "text", ID)?;
    let all = match optional_text(p, "mode", ID)?.map(str::trim) {
        None | Some("all") => true,
        Some("any") => false,
        Some(_) => return Err(bad(ID, "`mode` must be `all` or `any`")),
    };
    let query = stencil::SearchQuery::parse(text, all).map_err(|problem| bad(ID, problem.words()))?;
    let outcome = stencil::search_stencils(s.stencils(), &query);
    Ok(stencil::search_answer(text, &query, &outcome))
}

/// The stencil `p` names as `stencil`, among the built-in stencils and the open files.
fn view_named<'a>(opened: &'a stencil::OpenedStencils, p: &Value, cmd: &str) -> Result<View<'a>> {
    let id = required_text(p, "stencil", cmd)?;
    View::find(opened, id).ok_or_else(|| bad(cmd, format!("no stencil `{}` (see stencil.list)", shown(id))))
}

/// The answer about one stencil's Quick Shapes: `{stencil, name, builtin, customized, masters}`.
fn group_json(view: &View<'_>, held: &Held) -> Value {
    let (items, customized) = view.group(held);
    let masters: Vec<Value> = items.into_iter().map(|i| i.json).collect();
    json!({ "stencil": view.id(), "name": view.name(), "builtin": view.is_builtin(), "customized": customized, "masters": masters })
}

/// The Quick Shapes of the stencils → `{stencils: [{stencil, name, builtin, customized, masters}], offer: {stencil, masters}, readable}`. With `stencil` it is that stencil's group (and its
/// `offer`: the first four masters of the group that the AutoConnect strip may offer, or the stencil's first four that it may where the group has none), and without it, or as `quick`, **the
/// Quick Shapes stencil**: the group of every open stencil in the order the window lists them, and the offer of the first four among them that the strip may offer. A strip may offer no
/// connector, no part of a structure and nothing of a stencil that does not feed it (`stencil/quick.rs`), and where that leaves nothing, `offer` is the first open
/// stencil's that feeds it, and its `stencil` says which. `readable` is false when the drawing holds data that cannot be read, which then shows the stencils' own groups. Works without a
/// drawing (it shows the stencils' own).
fn quick(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.quick";
    let read = Held::of(s.document());
    let readable = read.is_ok();
    let held = read.unwrap_or_default();
    let opened = s.stencils();
    let which = optional_text(p, "stencil", ID)?.map(str::trim).filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("quick"));
    let Some(id) = which else {
        let views = View::open(opened);
        let stencils: Vec<Value> = views.iter().map(|v| group_json(v, &held)).collect();
        let gathered: Vec<Value> = quick::offer_gathered(opened, &held).into_iter().map(|(_, item)| item.json).collect();
        // No Quick Shape of an open stencil feeds the strip: the first open stencil that feeds it does.
        let (from, offered) = if gathered.is_empty() {
            quick::first_offer(opened, &held)
                .map_or(("quick".to_owned(), Vec::new()), |(id, items)| (id, items.into_iter().map(|i| i.json).collect()))
        } else {
            ("quick".to_owned(), gathered)
        };
        return Ok(json!({ "stencils": stencils, "offer": { "stencil": from, "masters": offered }, "readable": readable }));
    };
    let view = View::find(opened, id).ok_or_else(|| bad(ID, format!("no stencil `{}` (see stencil.list)", shown(id))))?;
    let (from, items) = quick::offer_from(opened, &view, &held);
    let offered: Vec<Value> = items.into_iter().map(|i| i.json).collect();
    Ok(json!({ "stencils": [group_json(&view, &held)], "offer": { "stencil": from, "masters": offered }, "readable": readable }))
}

/// What a command that changes a group answers: the group, and whether the drawing changed.
fn changed_group(view: &View<'_>, held: &Held, changed: bool) -> Value {
    let mut v = group_json(view, held);
    if let Some(map) = v.as_object_mut() {
        map.insert("changed".to_owned(), json!(changed));
    }
    v
}

/// Adds a master to the Quick Shapes of a stencil (`at`: its place; a master that is in them is moved there) → the group as `stencil.quick` has it, and `changed`. No undo step: the
/// choice is a setting of the drawing, kept in every format.
fn add_quick(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.add_quick";
    let opened = s.stencils().clone();
    let view = view_named(&opened, p, ID)?;
    let key = master_key(p, ID)?;
    let at = optional_whole(p, "at", ID)?.map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    let master =
        view.resolve(&key).ok_or_else(|| bad(ID, format!("no master `{}` in the stencil `{}` (see stencil.list)", shown(&key), shown(view.id()))))?;
    s.edit_view(ID, |doc| {
        let mut held = Held::of(Some(doc)).map_err(|e| failed(ID, e.words()))?;
        quick::add(&mut held, &view, &master, at).map_err(|e| failed(ID, e.words()))?;
        let changed = held.write(doc).map_err(|e| failed(ID, e.words()))?;
        Ok((changed_group(&view, &held, changed), changed))
    })
}

/// Takes a master out of the Quick Shapes of a stencil (below the divider again) → the group, and `changed` (false for a master that was not in it).
fn remove_quick(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.remove_quick";
    let opened = s.stencils().clone();
    let view = view_named(&opened, p, ID)?;
    let key = master_key(p, ID)?;
    let master =
        view.resolve(&key).ok_or_else(|| bad(ID, format!("no master `{}` in the stencil `{}` (see stencil.list)", shown(&key), shown(view.id()))))?;
    s.edit_view(ID, |doc| {
        let mut held = Held::of(Some(doc)).map_err(|e| failed(ID, e.words()))?;
        quick::remove(&mut held, &view, &master).map_err(|e| failed(ID, e.words()))?;
        let changed = held.write(doc).map_err(|e| failed(ID, e.words()))?;
        Ok((changed_group(&view, &held, changed), changed))
    })
}

/// Reset Stencil: the stencil's Quick Shapes are its own again → the group, and `changed` (false when the drawing had not set them).
fn reset_stencil(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.reset";
    let opened = s.stencils().clone();
    let view = view_named(&opened, p, ID)?;
    s.edit_view(ID, |doc| {
        let mut held = Held::of(Some(doc)).map_err(|e| failed(ID, e.words()))?;
        quick::reset(&mut held, &view);
        let changed = held.write(doc).map_err(|e| failed(ID, e.words()))?;
        Ok((changed_group(&view, &held, changed), changed))
    })
}

/// A master's icon → `{format, bytes, data}`: `format` is `ico`, `png`, `bmp`, `gif`, `jpeg`, `emf`, `wmf` or `unknown`, `bytes` the size of the picture
/// and `data` its base64 text. A master with no icon, and one whose icon is not base64 or is larger than a quarter of a megabyte, is refused in words
/// (`stencil.list` says which masters have one). A window in this process reads the bytes with `OpenedStencil::icon_bytes`.
fn icon(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "stencil.icon";
    let sid = required_text(p, "stencil", ID)?;
    let key = master_key(p, ID)?;
    let stencil = s.stencils().get(sid).ok_or_else(|| bad(ID, format!("no stencil `{}` is open (see stencil.list)", shown(sid))))?;
    let entry = stencil.find(&key).ok_or_else(|| bad(ID, format!("no master `{}` in stencil `{}` (see stencil.list)", shown(&key), stencil.id())))?;
    let named = shown(&entry.name);
    match stencil.icon_bytes(entry) {
        Ok((format, bytes)) => {
            let data = stencil.icon_text(entry).unwrap_or_default();
            Ok(json!({ "format": format.word(), "bytes": bytes.len(), "data": data }))
        }
        Err(IconError::None) => Err(failed(ID, format!("master `{named}` has no icon"))),
        Err(IconError::Problem(problem)) => Err(failed(ID, format!("the icon of master `{named}` is {}", problem.words()))),
    }
}
