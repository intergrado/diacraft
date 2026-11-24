//! The steps of `diacraft-cli run`: read, checked and counted before the first one runs.
//!
//! The steps come from the command line (`--cmd`) and from files or standard input (`--commands`), which are
//! untrusted: a run takes at most [`MAX_STEPS`] steps and [`MAX_BYTES`] of `--commands` text in all, however many
//! sources they come from. A `--commands` source is a JSON array of `{"command": ID, "params": {...}}` objects
//! (`"id"` may stand for `"command"`, as in the MCP `command_batch` tool) or one such object per line (JSON Lines).
//! Anything wrong in any source is an error naming the source and the step, and no step has run by then.
//!
//! Parsing JSON costs far more memory than the text is long (a JSON value is some tens of times its text for small
//! objects: about 1.1 to 1.5 GiB for a 16 MiB line, and about 2 GiB when the objects are nested deeply, measured), and an
//! array is parsed whole before its steps can be counted. So the array form is limited to
//! [`MAX_ARRAY_BYTES`] in all, and a line of JSON Lines to [`MAX_LINE_BYTES`], the size of one line of the MCP server;
//! a larger input is written as JSON Lines, where only a line at a time is parsed.

use std::io::Read;

use diacraft_engine::shown;
use serde_json::{Map, Value};

use crate::args::Source;

/// Most steps in one run.
pub const MAX_STEPS: usize = 100_000;

/// Most bytes of `--commands` text in one run: 64 MiB.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// Most bytes of JSON array text in one run, all the arrays together: 16 MiB.
pub const MAX_ARRAY_BYTES: usize = 16 * 1024 * 1024;

/// Most bytes in one line of JSON Lines: 16 MiB.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The most characters of a file name an error repeats.
const MOST_PATH: usize = 300;

/// One command to run.
#[derive(Debug, PartialEq)]
pub struct Step {
    pub command: String,
    pub params: Value,
}

/// Every step of `sources`, in order, or why the run cannot start.
pub fn collect(sources: Vec<Source>) -> Result<Vec<Step>, String> {
    let mut steps: Vec<Step> = Vec::new();
    let mut bytes_left = MAX_BYTES;
    let mut array_left = MAX_ARRAY_BYTES;
    for source in sources {
        match source {
            Source::Cmd { id, params } => {
                if steps.len() >= MAX_STEPS {
                    return Err(too_many("the command line"));
                }
                steps.push(Step { command: id, params });
            }
            Source::Commands(name) => {
                let text = read_source(&name, &mut bytes_left)?;
                let shown_name = shown(&name, MOST_PATH);
                let read = parse(&shown_name, &text, steps.len(), &mut array_left)?;
                if steps.len().saturating_add(read.len()) > MAX_STEPS {
                    return Err(too_many(&format!("--commands {shown_name}")));
                }
                steps.extend(read);
            }
        }
    }
    Ok(steps)
}

fn too_many(whose: &str) -> String {
    format!("{whose}: the run would have more than {MAX_STEPS} steps")
}

/// The text of `--commands name` (the file, or standard input for `-`), which may use what is left of the byte budget.
fn read_source(name: &str, bytes_left: &mut usize) -> Result<String, String> {
    let shown_name = shown(name, MOST_PATH);
    let mut bytes = Vec::new();
    // One byte more than is left shows a source over the budget without reading all of it.
    let limit = u64::try_from(*bytes_left).unwrap_or(u64::MAX).saturating_add(1);
    let read = if name == "-" {
        std::io::stdin().lock().take(limit).read_to_end(&mut bytes)
    } else {
        std::fs::File::open(name).and_then(|file| file.take(limit).read_to_end(&mut bytes))
    };
    read.map_err(|e| format!("--commands {shown_name}: {e}"))?;
    if bytes.len() > *bytes_left {
        return Err(format!("--commands {shown_name}: more than {} MiB of steps in all the --commands together", MAX_BYTES / (1024 * 1024)));
    }
    *bytes_left -= bytes.len();
    String::from_utf8(bytes).map_err(|_| format!("--commands {shown_name}: the steps are not UTF-8 text"))
}

/// The steps in `text`, numbered from `first`: a JSON array, which may use what is left of `array_left` bytes, or JSON Lines
/// (blank lines skipped).
fn parse(source: &str, text: &str, first: usize, array_left: &mut usize) -> Result<Vec<Step>, String> {
    // Some editors start a UTF-8 file with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Each entry, with its line when the source is JSON Lines.
    let mut entries: Vec<(Option<usize>, Value)> = Vec::new();
    if text.trim_start().starts_with('[') {
        // Before it is parsed, which is where the memory goes.
        if text.len() > *array_left {
            return Err(format!(
                "--commands {source}: JSON arrays may be at most {} MiB in all, and this one does not fit; put the steps one to a line (JSON Lines) instead",
                MAX_ARRAY_BYTES / (1024 * 1024)
            ));
        }
        *array_left -= text.len();
        let Value::Array(items) = serde_json::from_str::<Value>(text).map_err(|e| format!("--commands {source}: {}", shown(&e.to_string(), 200)))?
        else {
            return Err(format!("--commands {source}: not a JSON array"));
        };
        if first.saturating_add(items.len()) > MAX_STEPS {
            return Err(too_many(&format!("--commands {source}")));
        }
        entries.extend(items.into_iter().map(|item| (None, item)));
    } else {
        for (index, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            if first.saturating_add(entries.len()) >= MAX_STEPS {
                return Err(too_many(&format!("--commands {source}")));
            }
            let step = first.saturating_add(entries.len());
            if line.len() > MAX_LINE_BYTES {
                return Err(format!(
                    "--commands {source}: step {step} (line {}): the line is longer than {} MiB; one step may not be that long",
                    index + 1,
                    MAX_LINE_BYTES / (1024 * 1024)
                ));
            }
            let value = serde_json::from_str::<Value>(line)
                .map_err(|e| format!("--commands {source}: step {step} (line {}): {}", index + 1, shown(&e.to_string(), 200)))?;
            entries.push((Some(index + 1), value));
        }
    }
    entries
        .into_iter()
        .enumerate()
        .map(|(k, (line, value))| {
            let step = first.saturating_add(k);
            command_step(value).map_err(|e| {
                let place = line.map_or_else(|| format!("entry {}", k + 1), |l| format!("line {l}"));
                format!("--commands {source}: step {step} ({place}): {e}")
            })
        })
        .collect()
}

/// The step an entry stands for: `{"command": ID, "params": {...}}`.
fn command_step(value: Value) -> Result<Step, String> {
    let Value::Object(mut entry) = value else {
        return Err("a step must be an object such as {\"command\": \"file.new\"}".to_owned());
    };
    let command = match (entry.remove("command"), entry.remove("id")) {
        (Some(Value::String(id)), None) | (None, Some(Value::String(id))) => id,
        (Some(_), Some(_)) => return Err("give `command` or `id`, not both".to_owned()),
        (None, None) => return Err("no `command`".to_owned()),
        _ => return Err("`command` must be text: a command id".to_owned()),
    };
    let params = match entry.remove("params") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(object @ Value::Object(_)) => object,
        Some(_) => return Err(format!("`params` of `{}` must be a JSON object", shown(&command, 64))),
    };
    if let Some(key) = entry.keys().next() {
        return Err(format!("unknown key `{}`: a step has `command` (or `id`) and `params`", shown(key, 64)));
    }
    Ok(Step { command, params })
}
