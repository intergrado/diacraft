//! `command.batch`: several commands as one undo step, all or nothing.

use diacraft_doc::check_text;
use serde_json::{Value, json};

use super::{CommandSpec, has_document, nearest, optional_text};
use crate::error::{bad, failed, shown};
use crate::{EngineError, Result, Session, find_command};

/// Most commands in one batch. A batch is one request, so the cap bounds the work one request can ask for.
pub const MAX_BATCH_STEPS: usize = 256;

/// The keys of a step.
const STEP_KEYS: [&str; 2] = ["command", "params"];

/// A step with no `params`.
static NO_PARAMS: Value = Value::Null;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "command.batch",
        "Run Commands Together",
        [],
        None,
        "commands: array of {command, params} objects, at most 256, run in order and numbered from 0 in errors; \
         label: text, the name of the one undo step they make (optional, default \"Batch\")",
        has_document,
        run
    )]
}

/// Runs the steps in order as one undo step → `{results}`, each step's result in order. If a step
/// fails, none of the steps has any effect and the error names it: `batch step 2 (`page.rename`) failed: …`.
/// A batch cannot hold a batch, an undo or redo, or a new document: they do not fit in one step.
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "command.batch";
    if s.in_group() {
        return Err(failed(ID, "batches cannot nest"));
    }
    let steps = steps(p)?;
    // The name of an undo step follows the rule page names do; `Session::grouped` checks it again.
    let label = match optional_text(p, "label", ID)? {
        None => "Batch",
        Some(label) => {
            check_text(label).map_err(|problem| bad(ID, format!("`label` {problem}")))?;
            label
        }
    };
    s.grouped(label, |s| {
        let mut results = Vec::with_capacity(steps.len());
        for (index, (id, params)) in steps.iter().enumerate() {
            let step_failed = |e: EngineError| failed(ID, format!("batch step {index} (`{}`) failed: {e}", shown(id)));
            let spec = find_command(id).ok_or_else(|| step_failed(EngineError::UnknownCommand(shown(id))))?;
            results.push(s.run_spec(spec, params).map_err(step_failed)?);
        }
        Ok(json!({ "results": results }))
    })
}

/// The `(command, params)` of each step, after checking the shape of all of them, before any runs.
fn steps(p: &Value) -> Result<Vec<(&str, &Value)>> {
    const ID: &str = "command.batch";
    let commands = match p.get("commands") {
        None => return Err(bad(ID, "missing `commands`")),
        Some(Value::Array(commands)) => commands,
        Some(_) => return Err(bad(ID, "`commands` must be an array")),
    };
    if commands.len() > MAX_BATCH_STEPS {
        return Err(bad(ID, format!("a batch holds at most {MAX_BATCH_STEPS} commands, not {}", commands.len())));
    }
    commands
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let Value::Object(step) = step else {
                return Err(bad(ID, format!("commands[{index}] must be an object {{command, params}}")));
            };
            if let Some(unknown) = step.keys().find(|k| !STEP_KEYS.contains(&k.as_str())) {
                let unknown = shown(unknown);
                return Err(bad(
                    ID,
                    match nearest(&unknown, &STEP_KEYS) {
                        Some(near) => format!("commands[{index}] has an unknown key `{unknown}`; did you mean `{near}`?"),
                        None => format!("commands[{index}] has an unknown key `{unknown}`; a step takes `command` and `params`"),
                    },
                ));
            }
            let Some(Value::String(command)) = step.get("command") else {
                return Err(bad(ID, format!("commands[{index}] needs a `command` (text): the id of the command to run")));
            };
            Ok((command.as_str(), step.get("params").unwrap_or(&NO_PARAMS)))
        })
        .collect()
}
