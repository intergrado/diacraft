//! `diacraft-cli roundtrip`: opens a drawing, stencil or template, optionally moves one shape, saves it, and says what the save changed.
//!
//! This is the tool for the sample-file round trip of the VSDX formats: a file is opened as `file.open` opens it (a template as the file itself,
//! not as a new drawing), saved as `file.save_as` saves it to `--out`, and the two packages are compared **part by part** (byte for byte, with
//! the parts a save regenerates told apart) and **model by model** (the file read back equals the file read, and with `--move` which cells of
//! which shapes differ). One line of JSON is printed.
//!
//! **What may cross to a person who must not see the file** (`AGENTS.md`: a hash, a category, cell and function names, an error class): the
//! SHA-256 of the input, the variant, counts, part names when they are package structure (`diacraft_vsdx::part_label`: any other part is called by
//! its role), page and shape `ID`s (numbers), the spec's own spelling of a cell name (any other is `(unlisted)`) and the 17 predefined sections.
//! **No formula, no cell value, no text, no name a person made up** (a row's name, a page's name, a part's name that is not package structure).
//! **No word the engine or the writer said is copied, cleaned or cut:** an error is told by its class (`error`, a fixed word for the kind of
//! `EngineError`, and `problem` for the kind of trouble opening was) and a warning by its class (a fixed word from the table of `warning_class.rs`, `other` for a
//! text it does not know). A part's name can hold a person's words and a message names parts; taking the names out of a message cannot be made
//! sure of (a name inside a name, a name that is a word of the message, the escaped forms), so no text of a message reaches the line at all.
//!
//! The file is read once for its hash and its parts, and again by the session; `--out` is never the input (the sample files are not to be written).
//! A file that does not open is a report with the class of the refusal and the exit status is 0, as for `oracle`: a run over a set of sample files goes on.
//!
//! **An OpenDocument drawing** (`.odg`, `.otg`, or bytes that say they are one) goes to [`odg`] instead: before its reader exists, it
//! is copied entry by entry through the package writer and compared with itself, part by part and element by element.
//!
//! **`--via diacraft`** goes to [`native`] for either format: the file is sent through `.diacraft` and back in the engine, and the line
//! reports the content check of the file and what came back. `compare --content` is [`native`]'s too.

mod native;
mod odg;

/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use native::half_of;

use std::path::Path;

use diacraft_doc::{Document, Page, Shape, ShapeList, Sheet};
use diacraft_engine::file::{package_file_cap, read_file};
use diacraft_engine::memory::{self, Hold};
use diacraft_engine::{EngineError, Session};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_vsdx::{Limits, part_label};
use serde_json::{Value, json};

use crate::args::{CompareArgs, RoundtripArgs, Via};
use crate::oracle::{cell_word, section_word, sha256_hex};
use crate::warning_class::warning_class;
use crate::{Failure, write_line};

/// The most entries a list in the report holds; a count says how many there were.
const MOST_LISTED: usize = 50;
/// The deepest group nesting the cell comparison walks (the model's own cap is 200).
const MOST_DEPTH: usize = 256;

/// A fixed word for the kind of error a command ended with, and for the kind of trouble when it was a file that would not open. Nothing of the
/// error's message is used: it may hold a part's name, a part's name can hold a person's words, and no way of taking names out of a text is sure
/// (a name inside another, a name that is a word of the message, the escaped forms).
fn engine_error(e: &EngineError) -> (&'static str, Option<&'static str>) {
    match e {
        EngineError::UnknownCommand(_) => ("unknown_command", None),
        EngineError::Disabled { .. } => ("disabled", None),
        EngineError::BadParams { .. } => ("bad_params", None),
        EngineError::Failed { .. } => ("failed", None),
        EngineError::CannotOpen { problem, .. } => ("cannot_open", Some(problem.word())),
        EngineError::ChangedOnDisk { .. } => ("changed_on_disk", None),
        EngineError::MacrosNotAllowed { .. } => ("macros_not_allowed", None),
        EngineError::Internal { .. } => ("internal", None),
    }
}

/// `compare`: the part-by-part comparison of two OpenDocument drawings, or with `--content` the content check of two files of one format.
pub fn compare(args: &CompareArgs) -> Result<(), Failure> {
    if args.content { native::compare_content(args) } else { odg::compare(args) }
}

pub fn run(args: RoundtripArgs) -> Result<(), Failure> {
    let limits = Limits::default();
    let bytes = read_file(Path::new(&args.file), package_file_cap(&limits)).map_err(|e| Failure::Message(format!("cannot read the file: {e}")))?;
    refuse_the_input_itself(&args)?;
    if args.via == Some(Via::Diacraft) {
        return native::run(&args, &bytes);
    }
    if odg::is_odg(Path::new(&args.file), &bytes) {
        return odg::run(&args, &bytes);
    }
    let mut report = json!({ "sha256": sha256_hex(&bytes), "bytes": bytes.len() });
    let (before, read_report) = match diacraft_vsdx::read(&bytes, &limits) {
        Ok(read) => read,
        Err(e) => {
            report["opened"] = json!(false);
            report["refusal"] = json!({ "class": e.class() });
            return write_line(&line(&report)?);
        }
    };
    // The model that was read and the file it was read from are held for the rest of the run, beside the session's read of the same file and the read of what it saved: the memory a read
    // may use is the process's, less what is held (`diacraft_engine::memory`). Three reads of a file of 80,000 shapes were a process of 5 GB.
    let _held = Hold::new(u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(read_report.stats.memory_bytes));
    report["variant"] = json!(read_report.variant.map(|v| v.extension()));
    report["opened"] = json!(true);
    report["read_warning_count"] = json!(read_report.warning_counts.values().sum::<u64>());

    let mut session = Session::new();
    // A template is opened as the file itself: a round trip is about the file, not about a new drawing made from it.
    if let Err(e) = session.execute("file.open", &json!({ "path": args.file, "edit_template": true })) {
        report["opened"] = json!(false);
        let (error, problem) = engine_error(&e);
        report["refusal"] = json!({ "class": "engine", "error": error, "problem": problem });
        return write_line(&line(&report)?);
    }
    if let Some((dx, dy)) = args.shift {
        match first_shape(&before) {
            Some((page, id)) => match session.execute("shape.move", &json!({ "ids": [id], "dx": dx, "dy": dy, "page": page })) {
                Ok(moved) => report["moved"] = json!({ "page": page, "shape": id, "dx": dx, "dy": dy, "moved": moved["moved"] }),
                Err(e) => report["moved"] = json!({ "page": page, "shape": id, "error": engine_error(&e).0 }),
            },
            None => report["moved"] = json!({ "error": "the file has no shape on any page to move" }),
        }
    }
    let saved = match session.execute("file.save_as", &json!({ "path": args.out })) {
        Ok(saved) => saved,
        Err(e) => {
            report["saved"] = json!(false);
            report["save_error"] = json!({ "error": engine_error(&e).0 });
            return write_line(&line(&report)?);
        }
    };
    report["saved"] = json!(true);
    // The fields are picked, not copied, and a warning is told by its class and never by its words: the writer's words name parts.
    let warnings: Vec<&'static str> =
        saved["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).map(warning_class).take(MOST_LISTED).collect()).unwrap_or_default();
    report["save"] = json!({
        "bytes": saved["bytes"],
        "variant": saved["variant"],
        "macros_dropped": saved["macros_dropped"],
        "parts": saved["parts"],
        "warning_count": saved["warning_count"],
        "warnings": warnings,
    });
    let after_bytes =
        read_file(Path::new(&args.out), package_file_cap(&limits)).map_err(|e| Failure::Message(format!("cannot read what was saved: {e}")))?;
    report["out_bytes"] = json!(after_bytes.len());
    report["parts"] = compare_packages(&bytes, &after_bytes);
    // The read of what was saved leaves room for the model that was read, the file, and the session's drawing, and holds what it is given while it runs (`diacraft_engine::memory`).
    let reading = memory::reserve_read();
    match diacraft_vsdx::read(&after_bytes, &reading.limits()) {
        Ok((after, _)) => {
            report["model"] = compare_models(&before, &after, Written::RelIds);
            report["changed"] = changed_cells(&before, &after);
        }
        Err(e) => report["model"] = json!({ "reads_back": false, "class": e.class() }),
    }
    drop(reading);
    write_line(&line(&report)?)
}

fn line(report: &Value) -> Result<String, Failure> {
    diacraft_mcp::json_line(report).map_err(|e| Failure::Message(format!("cannot encode the report: {e}")))
}

/// `--out` is never the file that was read: a round trip over a set of sample files must not be able to write it.
fn refuse_the_input_itself(args: &RoundtripArgs) -> Result<(), Failure> {
    let same = args.file == args.out || matches!((std::fs::canonicalize(&args.file), std::fs::canonicalize(&args.out)), (Ok(a), Ok(b)) if a == b);
    if same { Err(Failure::Message("--out is the file being read: a round trip never writes its input".to_owned())) } else { Ok(()) }
}

/// The first top-level shape of the first page that has one: the page's `ID` and the shape's.
fn first_shape(doc: &Document) -> Option<(u32, u32)> {
    doc.pages.iter().find_map(|page| page.shapes.iter().next().map(|shape| (page.id, shape.id)))
}

// ------------------------------------------------------------------------------------------------------------------------ the packages

/// What a person may be told of the parts of the two packages: how many are the same bytes, which differ, which are new and which are gone, and
/// whether the content types and the relationships say the same. The parts are compared where they are, and not copied.
fn compare_packages(before: &[u8], after: &[u8]) -> Value {
    let limits = OpcLimits::default();
    let (Ok(old), Ok(new)) = (Package::read(before, &limits), Package::read(after, &limits)) else { return json!({ "readable": false }) };
    let label = |name: &str| part_label("a part", name).to_string();
    let (mut changed, mut removed, mut added) = (Vec::new(), Vec::new(), Vec::new());
    let (mut identical, mut in_count, mut out_count) = (0usize, 0usize, 0usize);
    for name in old.parts() {
        in_count += 1;
        match (old.part(name), new.part(name)) {
            (Some(a), Some(b)) if a == b => identical += 1,
            (Some(_), Some(_)) => changed.push(label(name)),
            _ => removed.push(label(name)),
        }
    }
    for name in new.parts() {
        out_count += 1;
        if old.part(name).is_none() {
            added.push(label(name));
        }
    }
    let listed = |v: &[String]| v.iter().take(MOST_LISTED).cloned().collect::<Vec<_>>();
    let (types, relationships) = compare_rows(&old, &new);
    json!({
        "in": in_count,
        "out": out_count,
        "identical": identical,
        "changed": listed(&changed),
        "changed_count": changed.len(),
        "added": listed(&added),
        "added_count": added.len(),
        "removed": listed(&removed),
        "removed_count": removed.len(),
        "content_types": types,
        "relationships": relationships,
    })
}

/// How many rows of `a` are not in `b` and of `b` not in `a`: by sets, since a package may have tens of thousands of rows.
fn rows_differing<T: std::hash::Hash + Eq>(a: &[T], b: &[T]) -> usize {
    let (x, y): (std::collections::HashSet<&T>, std::collections::HashSet<&T>) = (a.iter().collect(), b.iter().collect());
    x.difference(&y).count() + y.difference(&x).count()
}

/// The content types and the relationships of two packages as sets of rows: whether they are equal, and which relationships parts differ.
fn compare_rows(a: &Package, b: &Package) -> (Value, Value) {
    let rows_of_types = |pkg: &Package| (pkg.content_types().defaults.clone(), pkg.content_types().overrides.clone());
    let (ta, tb) = (rows_of_types(a), rows_of_types(b));
    let (defaults, overrides) = (rows_differing(&ta.0, &tb.0), rows_differing(&ta.1, &tb.1));
    let types = json!({ "equal": defaults == 0 && overrides == 0, "default_rows_differing": defaults, "override_rows_differing": overrides });

    let rows = |pkg: &Package, source: &str| -> Option<Vec<(String, String, String, bool)>> {
        let rels = pkg.rels(source).ok()?;
        let mut v: Vec<_> =
            rels.items.iter().map(|r| (r.id.clone(), r.ty.clone(), r.target_raw.clone(), r.mode == diacraft_opc::TargetMode::External)).collect();
        v.sort();
        Some(v)
    };
    let mut sources: Vec<String> = vec!["/".to_owned()];
    for pkg in [a, b] {
        sources.extend(pkg.parts().filter(|p| !diacraft_opc::name::is_rels_part(p)).map(str::to_owned));
    }
    sources.sort();
    sources.dedup();
    let mut differ = Vec::new();
    let mut unreadable = 0usize;
    for source in &sources {
        match (rows(a, source), rows(b, source)) {
            (Some(x), Some(y)) if x == y => {}
            (Some(_), Some(_)) => differ.push(part_label("a part", source).to_string()),
            _ => unreadable += 1,
        }
    }
    let relationships = json!({
        "equal": differ.is_empty(),
        "differ": differ.iter().take(MOST_LISTED).collect::<Vec<_>>(),
        "differ_count": differ.len(),
        "unreadable": unreadable,
    });
    (types, relationships)
}

// ------------------------------------------------------------------------------------------------------------------------ the models

/// What a save writes of its own into a page and a master, and a comparison of two models leaves out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Written {
    /// The relationship `Id` of the part, which a save numbers anew (the round trip of a VSDX file over itself).
    RelIds,
    /// The trip through `.diacraft`, where a salted edit may add a shape and so a master: the relationship `Id` and the name of the part, too (a page or master that was added after the file was
    /// read has no part until a file is written, and the file read back gives it one), and **what the file says of itself** (the slot of the elements the model has no field for) **of a page or
    /// master that held none**: a thing made in DiaCraft has no file's bookkeeping until a file is written. A slot that held something must come back equal.
    Trip,
}

/// Whether the file read back is the file read, but for what a save writes of its own ([`Written`]; and the times).
fn compare_models(before: &Document, after: &Document, written: Written) -> Value {
    let mut differ: Vec<String> = Vec::new();
    let page_rows = |doc: &Document| -> Vec<Page> {
        doc.pages
            .iter()
            .map(|p| {
                let mut p = (**p).clone();
                p.rel_id = None;
                if written == Written::Trip {
                    p.part_name = None;
                }
                p
            })
            .collect()
    };
    let (a, mut b) = (page_rows(before), page_rows(after));
    if written == Written::Trip {
        for (x, y) in a.iter().zip(b.iter_mut()) {
            if x.ext.is_empty() {
                y.ext = Default::default();
            }
        }
    }
    if a != b {
        if a.len() != b.len() {
            differ.push("page_count".to_owned());
        }
        differ.extend(a.iter().zip(&b).filter(|(x, y)| x != y).map(|(x, _)| format!("page {}", x.id)));
    }
    let masters = |doc: &Document| -> Vec<diacraft_doc::Master> {
        doc.masters
            .iter()
            .map(|m| {
                let mut m = (**m).clone();
                m.rel_id = None;
                if written == Written::Trip {
                    m.part_name = None;
                }
                m
            })
            .collect()
    };
    let (a, mut b) = (masters(before), masters(after));
    if written == Written::Trip {
        for (x, y) in a.iter().zip(b.iter_mut()) {
            if x.ext.is_empty() {
                y.ext = Default::default();
            }
        }
    }
    if a != b {
        if a.len() != b.len() {
            differ.push("master_count".to_owned());
        }
        differ.extend(a.iter().zip(&b).filter(|(x, y)| x != y).map(|(x, _)| format!("master {}", x.id)));
    }
    for (word, same) in [
        ("styles", before.styles == after.styles),
        ("colors", before.colors == after.colors),
        ("fonts", before.fonts == after.fonts),
        ("document_sheet", before.sheet == after.sheet),
        ("themes", before.themes == after.themes),
        ("document_unknown_elements", before.passthrough == after.passthrough),
        ("title", before.title == after.title),
    ] {
        if !same {
            differ.push(word.to_owned());
        }
    }
    json!({ "reads_back": true, "equal": differ.is_empty(), "differ": differ.iter().take(MOST_LISTED).collect::<Vec<_>>(), "differ_count": differ.len() })
}

/// Pushes while the list has room: a report tells the first entries and counts the rest, and a hostile file must not make it as long as the file.
fn push_capped<T>(list: &mut Vec<T>, item: T) {
    if list.len() < MOST_LISTED {
        list.push(item);
    }
}

/// The shapes whose cells differ between two models of the same pages: for each, the page, the shape's `ID`, the `ID`s above it, the names of the
/// cells directly under it that differ, and the predefined sections (with the place of the row, never its name) in which a cell differs.
fn changed_cells(before: &Document, after: &Document) -> Value {
    let mut shapes: Vec<Value> = Vec::new();
    let mut count = 0usize;
    let mut other = Vec::new();
    for page in before.pages.iter() {
        let Some(now) = after.page(page.id) else {
            push_capped(&mut other, format!("page {} is gone", page.id));
            continue;
        };
        let mut work: Vec<(Vec<u32>, &ShapeList, &ShapeList)> = vec![(Vec::new(), &page.shapes, &now.shapes)];
        while let Some((path, old, new)) = work.pop() {
            if old.len() != new.len() {
                push_capped(&mut other, format!("page {}: the shapes under {:?} are {} and were {}", page.id, path, new.len(), old.len()));
            }
            for (a, b) in old.iter().zip(new.iter()) {
                if a.id != b.id {
                    push_capped(&mut other, format!("page {}: a shape changed its ID", page.id));
                    continue;
                }
                if let Some(entry) = shape_diff(page.id, &path, a, b) {
                    count += 1;
                    if shapes.len() < MOST_LISTED {
                        shapes.push(entry);
                    }
                }
                if path.len() < MOST_DEPTH && !a.children.is_empty() {
                    let mut deeper = path.clone();
                    deeper.push(a.id);
                    work.push((deeper, &a.children, &b.children));
                }
            }
        }
    }
    json!({ "shape_count": count, "shapes": shapes, "other": other.iter().take(MOST_LISTED).collect::<Vec<_>>() })
}

fn shape_diff(page: u32, path: &[u32], a: &Shape, b: &Shape) -> Option<Value> {
    let (cells, sections, others) = sheet_diff(&a.sheet, &b.sheet);
    let mut other = others;
    for (word, same) in
        [("text", a.text == b.text), ("data", (&a.data1, &a.data2, &a.data3) == (&b.data1, &b.data2, &b.data3)), ("picture", a.foreign == b.foreign)]
    {
        if !same {
            other.push(word.to_owned());
        }
    }
    if a.kind != b.kind || a.master != b.master || a.master_shape != b.master_shape || a.del != b.del || a.name != b.name || a.name_u != b.name_u {
        other.push("identity".to_owned());
    }
    if cells.is_empty() && sections.is_empty() && other.is_empty() {
        return None;
    }
    Some(json!({ "page": page, "shape": a.id, "under": path, "cells": cells, "sections": sections, "other": other }))
}

/// The names of the cells directly under the sheets that differ, the sections in which a cell or a row differs (`Section.cell` or `Section.row`,
/// with the place of the row, and `<other section>` for a section a person named), and what else differs (`links`, `unknown`).
fn sheet_diff(a: &Sheet, b: &Sheet) -> (Vec<String>, Vec<String>, Vec<String>) {
    // A set, so that a shape with a hundred thousand cells that differ costs what it holds; only the first of them are told.
    let mut differing: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    for key in a.cells.keys().chain(b.cells.keys()) {
        let name = key.as_str();
        if a.cells.get(name) != b.cells.get(name) {
            differing.insert(cell_word(name));
        }
    }
    let cells: Vec<String> = differing.into_iter().take(MOST_LISTED).map(str::to_owned).collect();
    let mut sections = Vec::new();
    if a.sections.len() != b.sections.len() {
        push_capped(&mut sections, "section_count".to_owned());
    }
    for (x, y) in a.sections.iter().zip(&b.sections) {
        if x == y {
            continue;
        }
        let word = section_word(x.name.as_str());
        if x.rows.len() != y.rows.len() {
            push_capped(&mut sections, format!("{word}.row_count"));
        }
        for (place, (r, s)) in x.rows.iter().zip(y.rows.iter()).enumerate() {
            if r != s {
                if sections.len() >= MOST_LISTED {
                    break;
                }
                // The name of a cell is told in the spec's spelling, and any other is a fixed word (a name a file made up is content).
                let first = r.cells.keys().chain(s.cells.keys()).map(|k| k.as_str()).find(|n| r.cells.get(*n) != s.cells.get(*n)).map(cell_word);
                push_capped(&mut sections, format!("{word}.row{place}.{}", first.unwrap_or("(row)")));
            }
        }
        if x.cells != y.cells {
            push_capped(&mut sections, format!("{word}.flags"));
        }
    }
    let mut other = Vec::new();
    if a.links != b.links {
        other.push("links".to_owned());
    }
    if a.passthrough != b.passthrough || a.raw != b.raw {
        other.push("unknown".to_owned());
    }
    (cells, sections, other)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_engine::OpenProblem;

    use super::*;

    #[test]
    fn an_error_is_told_by_its_kind_and_nothing_of_its_message_is_used() {
        // Messages that hold part names, one inside another: none of them is read.
        let msg = "the part /a~/b~cd (and /a~/b~, /a~) is damaged".to_owned();
        let cmd = "file.save_as".to_owned();
        for (error, word, problem) in [
            (EngineError::UnknownCommand(msg.clone()), "unknown_command", None),
            (EngineError::Disabled { cmd: cmd.clone(), reason: msg.clone() }, "disabled", None),
            (EngineError::BadParams { cmd: cmd.clone(), msg: msg.clone() }, "bad_params", None),
            (EngineError::Failed { cmd: cmd.clone(), msg: msg.clone() }, "failed", None),
            (EngineError::CannotOpen { cmd: cmd.clone(), problem: OpenProblem::Damaged, msg: msg.clone() }, "cannot_open", Some("damaged")),
            (EngineError::CannotOpen { cmd: cmd.clone(), problem: OpenProblem::TooBig, msg: msg.clone() }, "cannot_open", Some("too_big")),
            (EngineError::ChangedOnDisk { cmd: cmd.clone(), msg: msg.clone() }, "changed_on_disk", None),
            (EngineError::MacrosNotAllowed { cmd: cmd.clone(), msg: msg.clone() }, "macros_not_allowed", None),
            (EngineError::Internal { cmd: cmd.clone(), msg: msg.clone() }, "internal", None),
        ] {
            assert_eq!(engine_error(&error), (word, problem));
        }
    }

    /// What a save writes of its own is left out of a comparison of two models, and nothing else: the relationship `Id` always; and, only for the trip through `.diacraft`, the name of a part
    /// (a master that was added after a file was read has no part until a file is written, and the file read back gives it one) and the slot of what a file says of itself where the saved
    /// drawing's object held none.
    #[test]
    fn a_comparison_of_models_leaves_out_what_a_save_writes_of_its_own_and_only_that() {
        use std::sync::Arc;

        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("shape.drop", &json!({ "master": "rectangle", "page": 1, "x": 2.0, "y": 3.0 })).unwrap();
        let before = session.document().unwrap().clone();
        assert_eq!(compare_models(&before, &before, Written::RelIds)["equal"], true);
        // The same drawing after a file was written and read back: the page and the master have the names of their parts, and the master a relationship `Id`.
        let mut written = before.clone();
        Arc::make_mut(written.pages.get_mut(0).unwrap()).part_name = Some(Arc::from("/visio/pages/page1.xml"));
        let master = Arc::make_mut(written.masters.get_mut(0).unwrap());
        master.part_name = Some(Arc::from("/visio/masters/master1.xml"));
        master.rel_id = Some(Arc::from("rId9"));
        // A part's name is a difference in the plain round trip of a file over itself, and is left out only by the comparison of the trip through `.diacraft`.
        let strict = compare_models(&before, &written, Written::RelIds);
        assert_eq!((strict["equal"].clone(), strict["differ_count"].clone()), (json!(false), json!(2)), "{strict}");
        assert_eq!(compare_models(&before, &written, Written::Trip)["equal"], true);
        // What a file says of itself, in the slot of a master that held nothing, is the file's bookkeeping; in one that held something it is content, and must come back equal.
        Arc::make_mut(written.masters.get_mut(0).unwrap()).ext.make_mut().vsdx.0.push((Arc::from("Rel"), Default::default()));
        assert_eq!(compare_models(&before, &written, Written::Trip)["equal"], true, "the slot of an object that held none");
        assert_eq!(compare_models(&before, &written, Written::RelIds)["equal"], false);
        assert_eq!(compare_models(&written, &before, Written::Trip)["equal"], false, "a slot that held something and came back empty is a loss");
        // Nothing else is left out: a page without its shapes is another model.
        let mut emptied = written.clone();
        Arc::make_mut(emptied.pages.get_mut(0).unwrap()).shapes = ShapeList::default();
        assert_eq!(compare_models(&before, &emptied, Written::Trip)["equal"], false);
    }
}
