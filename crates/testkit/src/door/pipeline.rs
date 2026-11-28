//! What a mutant is put through once it is a file: the engine's open, one page drawn, an edit, the saves and the opens of what they wrote.
//!
//! A **door** is the reader: a mutant that the reader takes in has gone through it, and what follows is the rest of the program meeting a drawing that a hostile file made. Every file
//! lives in the engine's files in memory ([`diacraft_engine::memfs`]), so a sweep touches no disk. The outcome of one mutant is one of three:
//!
//! * [`Outcome::Refused`]: the reader or a later step said no, in words that name the trouble and not the file's path or its text;
//! * [`Outcome::Passed`]: the file opened, a page was drawn, and every save that was made reads again as the same kind of file;
//! * [`Outcome::Failed`]: a bug. A panic that the engine's guard caught (an `Internal` error), a frame that met a bug (the renderer's `Recovered`), a refusal with no words, or a
//!   file the engine wrote that the engine will not read.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use diacraft_doc::Document;
use diacraft_engine::memfs::{self, Keep};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

/// What became of one mutant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Refused in words: the words, with no path and no text of the file in them.
    Refused(String),
    /// Opened, drawn, saved and opened again.
    Passed(Passed),
    /// A bug, in words that say which step.
    Failed(String),
}

/// What a mutant that got through did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passed {
    /// Whether the page was drawn (a page too large to draw is refused in words, and the case goes on).
    pub drawn: bool,
    /// How many saves wrote a file, and how many of those files were opened again.
    pub saved: u8,
    pub reopened: u8,
    /// The steps that were refused in words, and why: the words with their numbers taken out (what refusals are counted by) and the words whole.
    pub refused: Vec<(String, String)>,
    /// How long each step took, in the order they ran (the open first), for the report's account of where a sweep spends its time.
    pub times: Vec<(String, Duration)>,
}

/// Memory files for the thread, emptied when the case ends, even by a failure.
struct Memory;

impl Memory {
    fn on() -> Memory {
        memfs::use_on_this_thread(true);
        memfs::clear();
        Memory
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        memfs::clear();
        memfs::use_on_this_thread(false);
    }
}

fn place(name: &str) -> PathBuf {
    PathBuf::from(format!("/web/1/{name}"))
}

/// The words of an error, with the digits taken out, for a count of what the sweep was refused for.
pub fn words_key(words: &str) -> String {
    let flat: String = words.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
    let mut key = String::new();
    for c in flat.chars() {
        if c == '#' && key.ends_with('#') {
            continue;
        }
        key.push(c);
    }
    key.chars().take(110).collect()
}

/// Whether `words` say something and leak nothing a refusal must not: not empty, not the path of the file.
fn in_words(words: &str) -> bool {
    words.trim().chars().filter(|c| c.is_alphabetic()).count() >= 6 && !words.contains("/web/1/")
}

/// How an engine error reads for the sweep: `Err(bug)` for what is a bug, `Ok(words)` for a refusal in words.
fn judge(step: &str, error: &EngineError) -> Result<String, String> {
    let words = error.to_string();
    match error {
        EngineError::Internal { msg, .. } => Err(format!("{step}: a panic the guard caught: {msg}")),
        EngineError::UnknownCommand(_) => Err(format!("{step}: {words}")),
        _ if !in_words(&words) => Err(format!("{step}: a refusal with no words: {words:?}")),
        _ => Ok(words),
    }
}

fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    memfs::put_copy(path, bytes, Keep::Once).map_err(|e| format!("the sweep could not put the file in memory: {e:?}"))
}

/// Opens `bytes` as a file called `case.<extension>` in a new session. `Err(Outcome)` is the end of the case (a refusal or a bug).
fn open_in_new_session(bytes: &[u8], extension: &str) -> Result<Session, Outcome> {
    let path = place(&format!("case.{extension}"));
    put(&path, bytes).map_err(Outcome::Failed)?;
    let mut session = Session::new();
    match session.execute("file.open", &json!({ "path": path.to_string_lossy() })) {
        Ok(_) => Ok(session),
        Err(e) => Err(match judge("file.open", &e) {
            Ok(words) => Outcome::Refused(words),
            Err(bug) => Outcome::Failed(bug),
        }),
    }
}

/// The first shape of the first page, if there is one.
fn first_shape(session: &Session) -> Option<u32> {
    session.document()?.pages.iter().next()?.shapes.iter().next().map(|s| s.id)
}

/// The step: `Ok(Some(value))` when it ran, `Ok(None)` when it was refused in words (noted in `passed`), `Err` for a bug. How long it took is noted in `passed`.
fn step(passed: &mut Passed, label: &str, run: impl FnOnce() -> Result<Value, EngineError>) -> Result<Option<Value>, String> {
    let started = Instant::now();
    let result = run();
    passed.times.push((label.to_owned(), started.elapsed()));
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            let words = judge(label, &e)?;
            passed.refused.push((format!("{label}: {}", words_key(&words)), format!("{label}: {words}")));
            Ok(None)
        }
    }
}

/// The frame of `session`'s first page as a PNG at a small size: a bug in drawing is a failure, a page too large to draw is a refusal.
fn draw(session: &mut Session, passed: &mut Passed) -> Result<(), String> {
    let path = place("frame.png");
    let answer = step(passed, "file.export", || session.execute("file.export", &json!({ "path": path.to_string_lossy(), "dpi": 24 })))?;
    let Some(answer) = answer else { return Ok(()) };
    if let Some(Value::Array(skipped)) = answer.get("skipped")
        && let Some(bug) = skipped.iter().find(|s| s.get("kind").and_then(Value::as_str) == Some("Recovered"))
    {
        return Err(format!("file.export: a frame that met a bug: {}", bug.get("first").and_then(Value::as_str).unwrap_or("")));
    }
    let png = memfs::take(&path).ok_or("file.export wrote no file")?;
    if !png.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err("file.export wrote a file that is no PNG".to_owned());
    }
    passed.drawn = true;
    Ok(())
}

/// Opens what a save wrote, in a session of its own, and gives how many pages it holds.
fn reopen(bytes: &[u8], extension: &str, what: &str) -> Result<usize, String> {
    match open_in_new_session(bytes, extension) {
        Ok(session) => Ok(session.document().map_or(0, |d| d.pages.iter().count())),
        Err(Outcome::Refused(words)) => Err(format!("{what}: a file the engine wrote does not open again: {words}")),
        Err(Outcome::Failed(bug)) => Err(format!("{what}: opening it again: {bug}")),
        Err(Outcome::Passed(_)) => Err(format!("{what}: reopen is unreachable")),
    }
}

/// Keeps a file the engine wrote and will not read, in the dump folder when `DIACRAFT_DOOR_DUMP` names one, to look at.
fn keep_written(case: usize, label: &str, bytes: &[u8]) {
    let Some(folder) = super::run::dump_folder_from_env() else { return };
    let name: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    // The dump is a convenience: a file that cannot be written is no failure of the sweep.
    let _ = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(folder.join(format!("written-case{case}-{name}.bin")), bytes));
}

/// One save (`file.save` of the drawing's own file, or `file.save_as` to `to`), and the open of what it wrote.
fn save_and_reopen(session: &mut Session, passed: &mut Passed, label: &str, command: &str, to: Option<&str>, case: usize) -> Result<(), String> {
    let path = place(&format!("saved.{}", to.unwrap_or("own")));
    let params = match to {
        Some(extension) => json!({ "path": place(&format!("saved.{extension}")).to_string_lossy() }),
        None => json!({}),
    };
    let own_path = session.path().map(PathBuf::from);
    let pages = session.document().map_or(0, |d| d.pages.iter().count());
    let Some(_) = step(passed, label, || session.execute(command, &params))? else { return Ok(()) };
    let written = match (to, own_path) {
        (Some(extension), _) => place(&format!("saved.{extension}")),
        (None, Some(own)) => own,
        (None, None) => path,
    };
    let bytes = memfs::take(&written).ok_or_else(|| format!("{label}: it said it wrote the file and no file is there"))?;
    passed.saved += 1;
    let extension = written.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_owned();
    let started = Instant::now();
    let again = reopen(&bytes, &extension, label).inspect_err(|_| keep_written(case, label, &bytes))?;
    passed.times.push((format!("reopen after {label}"), started.elapsed()));
    passed.reopened += 1;
    if to == Some("diacraft") && again != pages {
        return Err(format!("{label}: the drawing had {pages} page(s) and the file it saved opens with {again}"));
    }
    Ok(())
}

/// What is done to a drawing that was let in: a frame, an edit, and the saves, each followed by the open of its file. `round` picks the second foreign type the drawing is saved as.
fn after_open(session: &mut Session, own: &str, round: usize, opened_in: Duration) -> Outcome {
    let mut passed = Passed { times: vec![("file.open".to_owned(), opened_in)], ..Passed::default() };
    let result = (|| -> Result<(), String> {
        draw(session, &mut passed)?;
        if let Some(id) = first_shape(session) {
            step(&mut passed, "shape.move", || session.execute("shape.move", &json!({ "ids": [id], "dx": 0.25, "dy": -0.125 })))?;
        }
        save_and_reopen(session, &mut passed, "file.save", "file.save", None, round)?;
        save_and_reopen(session, &mut passed, "file.save_as diacraft", "file.save_as", Some("diacraft"), round)?;
        let other = if own == "odg" || round.is_multiple_of(2) { "vsdx" } else { "odg" };
        save_and_reopen(session, &mut passed, &format!("file.save_as {other}"), "file.save_as", Some(other), round)
    })();
    match result {
        Err(bug) => Outcome::Failed(bug),
        Ok(()) if session.panics_caught() > 0 => Outcome::Failed(format!("the guard caught {} panic(s)", session.panics_caught())),
        Ok(()) => Outcome::Passed(passed),
    }
}

/// The door of the formats the engine opens (`.vsdx`, `.odg`, `.fodg`, `.diacraft`): the file is opened by the engine, whatever its name, and put through the rest.
pub fn engine_door(bytes: &[u8], extension: &str, round: usize) -> Outcome {
    let _memory = Memory::on();
    let started = Instant::now();
    match open_in_new_session(bytes, extension) {
        Ok(mut session) => after_open(&mut session, extension, round, started.elapsed()),
        Err(outcome) => outcome,
    }
}

/// The file the engine's `file.save` writes after the first shape of the first page is moved (as the sweep's own edit is): the bytes, to look into. `Err` is the words of the step that
/// failed, a refusal included: the save of a drawing the reader let in is made, or refused in words, and never made damaged (looks at what it made).
pub fn engine_move_and_save(bytes: &[u8], extension: &str) -> Result<Vec<u8>, String> {
    let _memory = Memory::on();
    let mut session = open_in_new_session(bytes, extension).map_err(|o| format!("the file does not open: {o:?}"))?;
    let id = first_shape(&session).ok_or("the drawing has no shape to move")?;
    session.execute("shape.move", &json!({ "ids": [id], "dx": 0.25, "dy": -0.125 })).map_err(|e| format!("shape.move: {e}"))?;
    let own = session.path().map(PathBuf::from).ok_or("the drawing has no file of its own")?;
    session.execute("file.save", &json!({})).map_err(|e| format!("file.save: {e}"))?;
    memfs::take(&own).ok_or_else(|| "it said it wrote the file and no file is there".to_owned())
}

/// The door of a reader the engine does not open yet: `parse` is the reader; the drawing it makes is written as a `.diacraft` file and that goes through [`engine_door`].
pub fn document_door(parse: &dyn Fn(&[u8]) -> Result<Document, String>, bytes: &[u8], round: usize) -> Outcome {
    let doc = match parse(bytes) {
        Ok(doc) => doc,
        Err(words) if in_words(&words) => return Outcome::Refused(words),
        Err(words) => return Outcome::Failed(format!("the reader refused with no words: {words:?}")),
    };
    let written = diacraft_native::write(&doc, &diacraft_native::State::default(), None, &diacraft_native::WriteOptions::default());
    match written {
        Err(e) if in_words(&e.to_string()) => Outcome::Refused(format!("saving what the reader made: {e}")),
        Err(e) => Outcome::Failed(format!("saving what the reader made was refused with no words: {e:?}")),
        Ok(file) => match engine_door(&file.bytes, "diacraft", round) {
            Outcome::Refused(words) => Outcome::Failed(format!("a drawing the reader made, saved, does not open: {words}")),
            other => other,
        },
    }
}

/// A file made by the engine from `bytes` (which are `from_extension`): opened, and saved as `to_extension`. The error is the engine's words: a seed that cannot be made fails the
/// test that asked for it, as a fixture does, and is never dropped in silence (a sweep with fewer seeds looks at less).
pub fn engine_convert(bytes: &[u8], from_extension: &str, to_extension: &str) -> Result<Vec<u8>, String> {
    let _memory = Memory::on();
    let mut session = open_in_new_session(bytes, from_extension).map_err(|o| format!("the seed does not open: {o:?}"))?;
    // The time a save writes into a file is pinned, so that a seed is the same bytes on every build.
    session.pin_clock(Some(1_700_000_000));
    let out = place(&format!("made.{to_extension}"));
    session
        .execute("file.save_as", &json!({ "path": out.to_string_lossy() }))
        .map_err(|e| format!("the seed does not save as .{to_extension}: {e}"))?;
    memfs::take(&out).ok_or_else(|| "the save wrote no file".to_owned())
}
