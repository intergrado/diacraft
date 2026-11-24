//! The tools: their names, descriptions and schemas, and what each does on a [`Backend`].
//!
//! Six tools, which are the whole of what an agent needs at this stage. Everything the app can do is a command,
//! so three of them reach the command registry (`command_list`, `command_run`, `command_batch`) and the other
//! three look: at the document (`doc_inspect`) and, with the app attached, at its window (`ui_inspect`,
//! `ui_screenshot`).

use diacraft_engine::{MAX_BATCH_STEPS, guard, shown};
use serde_json::{Map, Value, json};

use crate::backend::Backend;

/// The most characters of a client's text an error repeats.
const MOST_SHOWN: usize = 64;

/// The most characters of a caught panic's message a reply repeats.
const MOST_PANIC: usize = 400;

/// What a tool is given: the `arguments` of the call.
pub(crate) type Args = Map<String, Value>;

/// One tool.
pub(crate) struct Tool {
    pub(crate) name: &'static str,
    title: &'static str,
    description: &'static str,
    /// Whether it only looks. A tool that writes a file (a screenshot) does not, though it leaves the document alone.
    read_only: bool,
    schema: fn() -> Value,
    /// Takes the arguments by value: they can be as big as a line may be, and the tool moves what it passes on.
    run: fn(&mut Backend, Args) -> Result<Value, String>,
}

/// Every tool, in the order `tools/list` gives them.
const TOOLS: &[Tool] = &[
    Tool {
        name: "command_list",
        title: "List commands",
        description: "List the commands: id, label, menu place, shortcut, parameter doc and whether each can run now (disabled_reason says why not). \
                      Every action in DiaCraft is a command. Narrow the list with filter (a case-insensitive part of the id or label), enabled_only, \
                      and brief, which leaves out the parameter docs.",
        read_only: true,
        schema: command_list_schema,
        run: command_list,
    },
    Tool {
        name: "command_run",
        title: "Run a command",
        description: "Run one command by id with its params (an object; the parameter doc from command_list says what it takes). \
                      An unknown parameter is an error that names the nearest known one. A command that fails changes nothing. \
                      The result is the command's own.",
        read_only: false,
        schema: command_run_schema,
        run: command_run,
    },
    Tool {
        name: "command_batch",
        title: "Run several commands",
        description: "Run commands in order as one undo step, all or nothing: if a step fails, none of them has any effect and the error names the step \
                      (counting from 0). At most 256 steps. A batch cannot hold file.new, edit.undo, edit.redo or another batch, and needs a document \
                      to be open. Results come back in order.",
        read_only: false,
        schema: command_batch_schema,
        run: command_batch,
    },
    Tool {
        name: "doc_inspect",
        title: "Inspect the document",
        description: "The open document: title, version, pages (id and name) and the undo and redo depth. An error says so when no document is open.",
        read_only: true,
        schema: no_arguments_schema,
        run: doc_inspect,
    },
    Tool {
        name: "ui_inspect",
        title: "Inspect the window",
        description: "The state of the DiaCraft window: frames drawn, size, status bar text, any notice, and how many panics the guards caught. \
                      Needs the app (diacraft-cli mcp --connect, or the default mode with the app running).",
        read_only: true,
        schema: no_arguments_schema,
        run: ui_inspect,
    },
    Tool {
        name: "ui_screenshot",
        title: "Screenshot the window",
        description: "Write the next frame the window presents to path as a PNG and answer its size. path must be absolute, end in .png and be in a \
                      folder that exists; a file already there is replaced. Needs the app. Take it after the commands whose effect you want to see.",
        read_only: false,
        schema: screenshot_schema,
        run: ui_screenshot,
    },
];

impl Tool {
    /// The names of the arguments this tool takes.
    pub(crate) fn arguments(&self) -> Vec<String> {
        match (self.schema)().get("properties") {
            Some(Value::Object(properties)) => properties.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }
}

/// The tool called `name`.
pub(crate) fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The names of all the tools, in order.
pub(crate) fn names() -> Vec<&'static str> {
    TOOLS.iter().map(|t| t.name).collect()
}

/// All the tools as `tools/list` gives them: name, title, description, input schema and annotations.
pub fn tool_definitions() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "title": t.title,
                    "readOnlyHint": t.read_only,
                    "destructiveHint": !t.read_only,
                    "idempotentHint": t.read_only,
                    "openWorldHint": false,
                },
            })
        })
        .collect()
}

/// What a `tools/call` answers: a block of text, and whether it is an error. An error in the tool's own work (a
/// command that fails, an argument of the wrong type, a bug) is this, with `isError`, so the model can read it
/// and correct itself; it is not a protocol error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    /// The result as a `tools/call` reply carries it.
    pub fn to_value(&self) -> Value {
        json!({"content": [{"type": "text", "text": self.text}], "isError": self.is_error})
    }
}

/// Runs `tool` with `args`, which the protocol layer has checked for unknown keys.
pub(crate) fn call(backend: &mut Backend, tool: &Tool, args: Args) -> ToolResult {
    guarded(tool.name, || (tool.run)(backend, args))
}

/// The result of `run`, which is a tool's work: its value as the result, its error as an `isError` result, and a
/// panic (a bug, caught) as an `isError` result that says so. Nothing escapes as a panic or as a protocol error.
pub(crate) fn guarded(name: &str, run: impl FnOnce() -> Result<Value, String>) -> ToolResult {
    match guard::catch_panic(run) {
        // Compact JSON: one line of it is a few tokens, and the model reads either.
        Ok(Ok(value)) => ToolResult { text: value.to_string(), is_error: false },
        Ok(Err(message)) => ToolResult { text: message, is_error: true },
        Err(panic) => {
            ToolResult { text: shown(&format!("internal error in `{name}`: {panic} (please report this bug)"), MOST_PANIC), is_error: true }
        }
    }
}

// ---------- schemas ----------

fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn boolean(description: &str) -> Value {
    json!({"type": "boolean", "description": description})
}

/// An object schema that takes only the properties it lists.
fn object(properties: &Value, required: &[&str]) -> Value {
    let mut schema = json!({"type": "object", "properties": properties, "additionalProperties": false});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn no_arguments_schema() -> Value {
    object(&json!({}), &[])
}

fn command_list_schema() -> Value {
    object(
        &json!({
            "filter": string("Case-insensitive text that the command's id or label contains"),
            "enabled_only": boolean("Only the commands that can run now"),
            "brief": boolean("Leave out each command's parameter doc, which is most of the list"),
        }),
        &[],
    )
}

fn command_run_schema() -> Value {
    object(
        &json!({"id": string("The command id, such as file.new or page.add"), "params": {"type": "object", "description": "The command's parameters"}}),
        &["id"],
    )
}

fn command_batch_schema() -> Value {
    let step = object(&json!({"id": string("The command id"), "params": {"type": "object", "description": "The command's parameters"}}), &["id"]);
    object(
        &json!({
            "steps": {"type": "array", "maxItems": MAX_BATCH_STEPS, "items": step, "description": "The commands to run, in order"},
            "label": string("The name of the one undo step the batch makes (default Batch)"),
        }),
        &["steps"],
    )
}

fn screenshot_schema() -> Value {
    object(&json!({"path": string("Absolute path of the PNG to write")}), &["path"])
}

// ---------- the tools' work ----------

/// The text `args` gives for `key`; `None` when it is left out or `null`.
fn optional_text<'a>(args: &'a Args, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("`{key}` must be text")),
    }
}

/// The text `args` must give for `key`.
fn required_text<'a>(args: &'a Args, key: &str) -> Result<&'a str, String> {
    optional_text(args, key)?.ok_or_else(|| format!("missing `{key}`: it must be text"))
}

/// The flag `args` gives for `key`, `false` when it is left out or `null`.
fn flag(args: &Args, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

/// Takes the text `args` must give for `key` out of it, so that it is moved on and not copied.
fn take_text(args: &mut Args, key: &str) -> Result<String, String> {
    match args.remove(key) {
        None | Some(Value::Null) => Err(format!("missing `{key}`: it must be text")),
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(format!("`{key}` must be text")),
    }
}

/// Takes the object `args` gives for `key` out of it (`{}` when it is left out or `null`), so that it is moved on and not
/// copied: it may be as big as a line.
fn take_params(args: &mut Args, key: &str) -> Result<Value, String> {
    match args.remove(key) {
        None | Some(Value::Null) => Ok(Value::Object(Map::new())),
        Some(object @ Value::Object(_)) => Ok(object),
        Some(_) => Err(format!("`{key}` must be an object")),
    }
}

/// The params of `engine.execute` for `command` with `params`, which are moved into it.
fn execute_params(command: String, params: Value) -> Value {
    let mut execute = Map::new();
    execute.insert("command".to_owned(), Value::String(command));
    execute.insert("params".to_owned(), params);
    Value::Object(execute)
}

/// Fails, saying what the tool needs, unless a window is attached.
fn need_app(backend: &Backend, tool: &str) -> Result<(), String> {
    if backend.has_app() {
        return Ok(());
    }
    Err(format!(
        "`{tool}` needs the app: this server runs a headless session, which has no window. Start DiaCraft with `diacraft --control 7997`, \
         then `diacraft-cli mcp --connect 127.0.0.1:7997` (or `diacraft-cli mcp` with no flag, which looks for the app there)"
    ))
}

fn command_list(backend: &mut Backend, args: Args) -> Result<Value, String> {
    let filter = optional_text(&args, "filter")?.map(str::to_lowercase).filter(|f| !f.is_empty());
    let (enabled_only, brief) = (flag(&args, "enabled_only")?, flag(&args, "brief")?);
    let Value::Array(commands) = backend.call("engine.commands", &json!({}))? else {
        return Err("the list of commands is not an array (please report this bug)".to_owned());
    };
    let text = |command: &Value, key: &str| command.get(key).and_then(Value::as_str).unwrap_or_default().to_lowercase();
    let listed = commands
        .into_iter()
        .filter(|c| !enabled_only || c.get("enabled").and_then(Value::as_bool) != Some(false))
        .filter(|c| filter.as_ref().is_none_or(|f| text(c, "id").contains(f.as_str()) || text(c, "label").contains(f.as_str())))
        .map(|mut command| {
            if let (true, Some(fields)) = (brief, command.as_object_mut()) {
                fields.remove("params");
            }
            command
        })
        .collect();
    Ok(Value::Array(listed))
}

fn command_run(backend: &mut Backend, mut args: Args) -> Result<Value, String> {
    let id = take_text(&mut args, "id")?;
    let params = take_params(&mut args, "params")?;
    backend.call("engine.execute", &execute_params(id, params))
}

fn command_batch(backend: &mut Backend, mut args: Args) -> Result<Value, String> {
    let steps = match args.remove("steps") {
        Some(Value::Array(steps)) => steps,
        Some(_) => return Err("`steps` must be an array of {id, params} objects".to_owned()),
        None => return Err("missing `steps`: an array of {id, params} objects".to_owned()),
    };
    // Refused before a command is built for any step, and before a request is sent: a line may hold more than a million steps.
    // The engine's `command.batch` limits the count as well, but only once the request has been built and, for the app, sent.
    if steps.len() > MAX_BATCH_STEPS {
        return Err(format!("`steps` holds {} steps; a batch takes at most {MAX_BATCH_STEPS}", steps.len()));
    }
    let mut commands = Vec::with_capacity(steps.len());
    for (index, step) in steps.into_iter().enumerate() {
        let Value::Object(mut step) = step else {
            return Err(format!("steps[{index}] must be an object {{id, params}}"));
        };
        if let Some(key) = step.keys().find(|k| *k != "id" && *k != "params") {
            return Err(format!("steps[{index}] has an unknown key `{}`; a step takes `id` and `params`", shown(key, MOST_SHOWN)));
        }
        let id = take_text(&mut step, "id").map_err(|e| format!("steps[{index}]: {e}"))?;
        let params = take_params(&mut step, "params").map_err(|e| format!("steps[{index}]: {e}"))?;
        let mut command = Map::new();
        command.insert("command".to_owned(), Value::String(id));
        command.insert("params".to_owned(), params);
        commands.push(Value::Object(command));
    }
    // Every step is checked before any runs, so a bad one at the end leaves nothing half done.
    let mut batch = Map::new();
    batch.insert("commands".to_owned(), Value::Array(commands));
    if let Some(label) = optional_text(&args, "label")? {
        batch.insert("label".to_owned(), Value::String(label.to_owned()));
    }
    backend.call("engine.execute", &execute_params("command.batch".to_owned(), Value::Object(batch)))
}

fn doc_inspect(backend: &mut Backend, _: Args) -> Result<Value, String> {
    backend.call("document.inspect", &json!({}))
}

fn ui_inspect(backend: &mut Backend, _: Args) -> Result<Value, String> {
    need_app(backend, "ui_inspect")?;
    backend.call("ui.inspect", &json!({}))
}

fn ui_screenshot(backend: &mut Backend, args: Args) -> Result<Value, String> {
    need_app(backend, "ui_screenshot")?;
    let path = required_text(&args, "path")?;
    backend.call("ui.screenshot", &json!({"path": path}))
}
