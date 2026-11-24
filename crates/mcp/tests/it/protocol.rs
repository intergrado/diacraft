//! The server as a client sees it: JSON lines in, JSON lines out, over a headless session.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use diacraft_engine::{MAX_BATCH_STEPS, Session, command_specs};
use diacraft_mcp::{Backend, MAX_BATCH_MESSAGES, MAX_LINE_BYTES, PROTOCOL_VERSION, tool_definitions};
use serde_json::{Value, json};

use crate::{call, lines, notification, request, talk, talk_on, tool_json, tool_text};

fn initialize(id: u64, version: Value) -> Value {
    request(id, "initialize", json!({"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}))
}

/// The code of an error reply; fails if the reply is not one.
fn code(reply: &Value) -> i64 {
    reply["error"]["code"].as_i64().unwrap_or_else(|| panic!("not an error reply: {reply}"))
}

fn message(reply: &Value) -> String {
    reply["error"]["message"].as_str().unwrap_or_else(|| panic!("no error message in {reply}")).to_owned()
}

/// `text` has nothing a terminal or a log would act on.
fn is_plain(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}'))
}

// ---------- the limits ----------

#[test]
fn the_limits_are_the_documented_ones() {
    // docs/mcp.md and the task brief: 16 MiB a line, batches of 64, 256 steps in a command_batch, revision 2025-06-18.
    assert_eq!(MAX_LINE_BYTES, 16 * 1024 * 1024);
    assert_eq!(MAX_BATCH_MESSAGES, 64);
    assert_eq!(MAX_BATCH_STEPS, 256);
    assert_eq!(PROTOCOL_VERSION, "2025-06-18");
    let tools = tool_definitions();
    assert_eq!(tools[2]["name"], "command_batch");
    assert_eq!(tools[2]["inputSchema"]["properties"]["steps"]["maxItems"], 256);
}

// ---------- the lifecycle ----------

#[test]
fn initialize_then_tools_list() {
    let replies = talk(&[initialize(1, json!(PROTOCOL_VERSION)), notification("notifications/initialized"), request(2, "tools/list", json!({}))]);
    // The notification has no reply.
    assert_eq!(replies.len(), 2, "{replies:?}");
    let init = &replies[0];
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "diacraft");
    assert!(init["result"]["capabilities"]["tools"].is_object(), "{init}");
    // Only what is implemented is advertised.
    assert_eq!(init["result"]["capabilities"].as_object().map(|c| c.len()), Some(1), "{init}");
    assert!(init["result"]["instructions"].as_str().is_some_and(|i| i.contains("headless")), "{init}");
    let list = &replies[1];
    assert_eq!(list["id"], 2);
    assert!(list["result"]["tools"].as_array().is_some_and(|t| !t.is_empty()), "{list}");
}

#[test]
fn the_three_protocol_revisions_are_accepted_and_others_get_the_newest() {
    for version in ["2025-06-18", "2025-03-26", "2024-11-05"] {
        let replies = talk(&[initialize(1, json!(version))]);
        assert_eq!(replies[0]["result"]["protocolVersion"], version, "{replies:?}");
    }
    for version in [json!("2030-01-01"), json!(""), json!("2026-07-28")] {
        let replies = talk(&[initialize(1, version.clone())]);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18", "{version}: {replies:?}");
    }
    // A version that is not text is a bad request, not a silent default.
    let replies = talk(&[initialize(1, json!(20250618))]);
    assert_eq!(code(&replies[0]), -32602, "{replies:?}");
    // A client that sends no version at all is answered with the newest.
    let replies = talk(&[request(1, "initialize", json!({}))]);
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18", "{replies:?}");
}

#[test]
fn ping_is_answered_and_unknown_methods_are_not_found() {
    let replies = talk(&[request(1, "ping", json!({})), request(2, "resources/list", json!({})), request(3, "evil\n\u{1b}[2J\u{202e}", json!({}))]);
    assert_eq!(replies[0]["result"], json!({}));
    assert_eq!(code(&replies[1]), -32601, "{replies:?}");
    assert_eq!(code(&replies[2]), -32601, "{replies:?}");
    // The method name is the client's: it does not reach the reply as it is.
    assert!(is_plain(&message(&replies[2])), "{replies:?}");
    assert!(message(&replies[2]).contains("\\u{1b}"), "{replies:?}");
}

// ---------- the tools ----------

#[test]
fn tools_list_names_exactly_the_six_tools() {
    let replies = talk(&[request(1, "tools/list", json!({}))]);
    let tools = replies[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["command_list", "command_run", "command_batch", "doc_inspect", "ui_inspect", "ui_screenshot"]);
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(tool["description"].as_str().is_some_and(|d| d.len() > 20), "{name}: {tool}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        // Unknown arguments are refused, so the schema says so.
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
        assert!(tool["annotations"]["readOnlyHint"].is_boolean(), "{name}");
    }
    // The three that only look say so; the ones that change the document, or a file, do not.
    let read_only: Vec<(&str, bool)> = tools.iter().map(|t| (t["name"].as_str().unwrap(), t["annotations"]["readOnlyHint"] == true)).collect();
    assert_eq!(
        read_only,
        [
            ("command_list", true),
            ("command_run", false),
            ("command_batch", false),
            ("doc_inspect", true),
            ("ui_inspect", true),
            ("ui_screenshot", false)
        ]
    );
}

#[test]
fn command_list_lists_every_registered_command() {
    let replies = talk(&[call(1, "command_list", json!({}))]);
    let listed = tool_json(&replies[0]);
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    let registered: Vec<&str> = command_specs().iter().map(|c| c.id).collect();
    assert!(registered.len() >= 8, "the registry is smaller than it was: {registered:?}");
    assert_eq!(ids, registered);
}

#[test]
fn command_list_narrows_by_filter_enabled_and_brief() {
    let replies = talk(&[
        call(1, "command_list", json!({"filter": "PAGE"})),
        call(2, "command_list", json!({"enabled_only": true})),
        call(3, "command_list", json!({"brief": true})),
        call(4, "command_run", json!({"id": "file.new"})),
        call(5, "command_list", json!({"enabled_only": true, "filter": "page."})),
    ]);
    let ids = |reply: &Value| -> Vec<String> { tool_json(reply).as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_owned()).collect() };
    // The filter ignores case and matches the id or the label: every page command, and nothing else (the page setup commands came).
    let page_commands = ids(&replies[0]);
    assert!(page_commands.starts_with(&["page.add".to_owned(), "page.rename".to_owned()]), "{page_commands:?}");
    // The label counts too: Move off Page Breaks and the Page Breaks box are named for the page and are not `page.` commands.
    let by_label = ["shape.move_off_page_breaks", "page_breaks.settings", "page_breaks.set"];
    assert!(
        page_commands.iter().all(|id| id.starts_with("page.") || by_label.contains(&id.as_str())) && page_commands.contains(&"page.size".to_owned()),
        "{page_commands:?}"
    );
    // With no document open, what needs one is left out.
    let enabled = ids(&replies[1]);
    assert!(enabled.contains(&"file.new".to_owned()) && !enabled.contains(&"page.add".to_owned()), "{enabled:?}");
    // `brief` is the full listing with each command's `params` taken out: the same commands in the same order, every other
    // field as it was.
    let brief = tool_json(&replies[2]);
    let full = tool_json(&talk(&[call(1, "command_list", json!({}))])[0]);
    assert!(full.as_array().unwrap().iter().all(|c| c["params"].is_string()));
    let full_without_params: Vec<Value> = full
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let mut c = c.clone();
            c.as_object_mut().unwrap().remove("params");
            c
        })
        .collect();
    assert_eq!(brief, Value::Array(full_without_params));
    // Once a document is open the page commands are enabled, except the two that need a second page (a drawing keeps one, and one page has no other place).
    let enabled_pages = ids(&replies[4]);
    assert!(enabled_pages.starts_with(&["page.add".to_owned(), "page.rename".to_owned()]), "{enabled_pages:?}");
    assert!(enabled_pages.contains(&"page.size".to_owned()), "{enabled_pages:?}");
    assert!(!enabled_pages.contains(&"page.delete".to_owned()) && !enabled_pages.contains(&"page.move".to_owned()), "{enabled_pages:?}");
    // A filter that is not text is the tool's error, not a silent "no filter".
    let bad = talk(&[call(1, "command_list", json!({"filter": 3}))]);
    assert_eq!(bad[0]["result"]["isError"], true, "{bad:?}");
}

#[test]
fn command_run_changes_the_document() {
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "page.add", "params": {"name": "Second"}})),
        call(3, "doc_inspect", json!({})),
    ]);
    for reply in &replies {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    assert_eq!(tool_json(&replies[1]), json!({"id": 2, "name": "Second"}));
    // Read back through another tool: the document really has the page.
    let doc = tool_json(&replies[2]);
    let names: Vec<&str> = doc["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Page-1", "Second"]);
    assert_eq!(doc["history"]["undo"], 1);
}

#[test]
fn the_editing_commands_draw_a_diagram_through_command_run() {
    // The commands of the first editor are reached as every command is, by id and params.
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "shape.drop", "params": {"master": "Rectangle", "page": 1, "x": 2, "y": 2}})),
        call(3, "command_run", json!({"id": "shape.drop", "params": {"master": "Ellipse", "page": 1, "x": 6, "y": 2}})),
        call(4, "command_run", json!({"id": "connector.add", "params": {"from": {"id": 1}, "to": {"id": 2}}})),
        call(5, "command_run", json!({"id": "shape.move", "params": {"ids": [2], "dx": 1, "dy": 0}})),
        call(6, "command_run", json!({"id": "shape.set_text", "params": {"id": 1, "text": "Hi"}})),
        call(7, "command_run", json!({"id": "stencil.list"})),
        call(8, "command_run", json!({"id": "shape.drop", "params": {"master": "Heptagon", "page": 1, "x": 1, "y": 1}})),
        call(9, "doc_inspect", json!({})),
    ]);
    for reply in &replies[..7] {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    assert_eq!(tool_json(&replies[3])["begin"], json!({"id": 1, "point": 3}));
    assert_eq!(tool_json(&replies[3])["end"], json!({"id": 2, "point": 1}));
    assert_eq!(tool_json(&replies[6])["stencils"][0]["masters"].as_array().map(Vec::len), Some(12));
    // An unknown master is the engine's error, not a protocol one.
    assert_eq!(replies[7]["result"]["isError"], true, "{:?}", replies[7]);
    assert!(tool_text(&replies[7]).contains("Heptagon"), "{:?}", replies[7]);
    assert_eq!(tool_json(&replies[8])["history"]["undo"], 5, "two drops, a connector, a move and a text");
}

#[test]
fn shape_formatting_answers_for_a_range_of_characters_through_command_run() {
    // The window's toggles and boxes show the state of the characters selected, so `shape.formatting` takes `start` and `end` as every
    // command takes its params, by id.
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "shape.drop", "params": {"master": "Rectangle", "page": 1, "x": 2, "y": 2}})),
        call(3, "command_run", json!({"id": "shape.set_text", "params": {"id": 1, "text": "hello world"}})),
        call(4, "command_run", json!({"id": "text.format", "params": {"ids": [1], "start": 6, "end": 11, "bold": true, "size": 11}})),
        call(5, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1], "start": 6, "end": 11}})),
        call(6, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1], "start": 0, "end": 11}})),
        call(7, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1], "start": 0, "end": 5}})),
        call(8, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1], "start": 3}})),
        call(9, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1], "start": 0, "end": 12}})),
        call(10, "command_run", json!({"id": "shape.formatting", "params": {"ids": [1]}})),
        call(11, "command_list", json!({"filter": "shape.formatting"})),
        call(12, "doc_inspect", json!({})),
    ]);
    // The two refusals are the eighth and ninth replies; the rest ran.
    for reply in [&replies[..7], &replies[9..]].concat() {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    // The world is bold and 11 points; the characters of the range alone, with nothing mixed.
    let world = &tool_json(&replies[4])["shapes"][0];
    assert_eq!((&world["bold"], &world["size"], &world["mixed"]), (&json!(true), &json!(11.0), &json!([])), "{world}");
    // Over the whole text the two runs differ in bold and size, and the answer says the word and lists the properties.
    let whole = &tool_json(&replies[5])["shapes"][0];
    assert_eq!((&whole["bold"], &whole["size"], &whole["mixed"]), (&json!("mixed"), &json!("mixed"), &json!(["size", "bold"])), "{whole}");
    assert_eq!(whole["italic"], false);
    // The first word is as it was.
    let hello = &tool_json(&replies[6])["shapes"][0];
    assert_eq!((&hello["bold"], &hello["mixed"]), (&json!(false), &json!([])), "{hello}");
    // A start with no end, and a range past the text, are the engine's refusals, in words.
    assert_eq!(replies[7]["result"]["isError"], true, "{:?}", replies[7]);
    assert!(tool_text(&replies[7]).contains("go together"), "{:?}", replies[7]);
    assert_eq!(replies[8]["result"]["isError"], true, "{:?}", replies[8]);
    assert!(tool_text(&replies[8]).contains("not within the text, which has 11 characters"), "{:?}", replies[8]);
    // With no range the answer is the shape's, as before: no list of what is mixed.
    assert!(tool_json(&replies[9])["shapes"][0].get("mixed").is_none());
    // The command's params are listed with the others, for a program to find.
    let listed = tool_json(&replies[10]);
    let params = listed[0]["params"].as_str().unwrap();
    assert!(params.contains("start:") && params.contains("end:") && params.contains("mixed"), "{params}");
    // A query is not a step: the drop, the text and the format are the three.
    assert_eq!(tool_json(&replies[11])["history"]["undo"], 3);
}

#[test]
fn command_run_goes_through_the_registry_so_undo_and_params_checks_apply() {
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "page.add", "params": {"name": "Second"}})),
        call(3, "command_run", json!({"id": "edit.undo"})),
        call(4, "doc_inspect", json!({})),
        // A parameter the command does not have is the engine's error, with the nearest key it knows.
        call(5, "command_run", json!({"id": "page.add", "params": {"nme": "x"}})),
    ]);
    assert_eq!(tool_json(&replies[3])["pages"].as_array().map(Vec::len), Some(1), "undo took the page back");
    assert_eq!(replies[4]["result"]["isError"], true, "{replies:?}");
    assert!(tool_text(&replies[4]).contains("did you mean `name`"), "{replies:?}");
}

#[test]
fn unknown_argument_is_invalid_params() {
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new", "bogus": true})),
        // The same through a tool that takes nothing.
        call(2, "doc_inspect", json!({"depth": 1})),
        // Nothing ran: there is still no document.
        call(3, "doc_inspect", json!({})),
    ]);
    assert_eq!(code(&replies[0]), -32602, "{replies:?}");
    assert!(message(&replies[0]).contains("bogus"), "{replies:?}");
    assert_eq!(code(&replies[1]), -32602, "{replies:?}");
    assert_eq!(replies[2]["result"]["isError"], true, "{replies:?}");
    assert!(tool_text(&replies[2]).contains("no document open"), "{replies:?}");
}

#[test]
fn unknown_keys_elsewhere_in_a_message_are_invalid_params_and_run_nothing() {
    let file_new = json!({"name": "command_run", "arguments": {"id": "file.new"}});
    let mut in_call = file_new.clone();
    in_call["surprise"] = json!(1);
    let mut in_envelope = request(2, "tools/call", file_new.clone());
    in_envelope["extra"] = json!(1);
    let replies = talk(&[request(1, "tools/call", in_call), in_envelope, call(3, "doc_inspect", json!({}))]);
    assert_eq!(code(&replies[0]), -32602, "{replies:?}");
    assert!(message(&replies[0]).contains("surprise"), "{replies:?}");
    assert_eq!(code(&replies[1]), -32602, "{replies:?}");
    assert!(message(&replies[1]).contains("extra"), "{replies:?}");
    assert!(tool_text(&replies[2]).contains("no document open"), "neither message ran its command: {replies:?}");
    // `_meta` is the protocol's own key and is allowed.
    let with_meta = request(4, "tools/call", json!({"name": "doc_inspect", "arguments": {}, "_meta": {"progressToken": 1}}));
    assert_eq!(talk(&[with_meta])[0]["result"]["isError"], true, "answered as a tool call (there is no document), not refused");
}

/// Values for `_meta`, from what clients send (a progress token) to what a hostile one might: any JSON, deep and large.
fn metas() -> Vec<Value> {
    let deep = (0..100).fold(json!(1), |inner, _| json!({"a": inner}));
    vec![
        json!({"progressToken": "abc123"}),
        json!({"progressToken": 7}),
        json!({"progressToken": {"not": ["a", "token"]}, "io.modelcontextprotocol/related-task": {"taskId": "t"}}),
        json!({"io.modelcontextprotocol/protocolVersion": "2026-07-28"}),
        json!({}),
        Value::Null,
        json!("text"),
        json!(5),
        json!(-1.5e300),
        json!([1, [2, [3]]]),
        json!(true),
        deep,
        json!({"x": "y".repeat(200_000)}),
        json!({"\u{1b}[2J\n": "\u{202e}\u{2028}", "": ""}),
    ]
}

#[test]
fn meta_is_accepted_in_the_params_of_every_request_whatever_its_shape() {
    // MCP 2025-06-18 (basic/general fields, and the schema's JSONRPCRequest) reserves `_meta` in the `params` of any request,
    // and the progress token a client asks for goes there. It is accepted and its content is not looked at.
    for meta in metas() {
        let shown = meta.to_string().chars().take(60).collect::<String>();
        let replies = talk(&[
            // The params `initialize` defines still work beside it: the revision asked for is the revision answered.
            request(
                1,
                "initialize",
                json!({"protocolVersion": "2025-03-26", "capabilities": {"roots": {}}, "clientInfo": {"name": "t", "version": "0"}, "_meta": meta}),
            ),
            request(2, "ping", json!({"_meta": meta})),
            request(3, "tools/list", json!({"_meta": meta, "cursor": "ignored"})),
            request(4, "tools/call", json!({"name": "command_run", "arguments": {"id": "file.new"}, "_meta": meta})),
            request(5, "tools/call", json!({"name": "doc_inspect", "_meta": meta})),
            // A notification's params may hold it too: no reply, and nothing is run.
            json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {"_meta": meta}}),
        ]);
        assert_eq!(replies.len(), 5, "{shown}: {replies:?}");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26", "{shown}: {:?}", replies[0]);
        assert_eq!(replies[1]["result"], json!({}), "{shown}");
        assert_eq!(replies[2]["result"]["tools"].as_array().map(Vec::len), Some(6), "{shown}");
        assert_eq!(replies[3]["result"]["isError"], false, "{shown}: {:?}", replies[3]);
        // The call that carried it ran: the document it made is there to inspect.
        assert_eq!(tool_json(&replies[4])["pages"].as_array().map(Vec::len), Some(1), "{shown}");
    }
}

#[test]
fn meta_is_not_a_key_of_the_message_or_of_a_tools_arguments() {
    // The specification puts `_meta` in `params`, not beside it and not in a tool's own arguments: there it is an unknown key,
    // like any other.
    let mut at_the_top = request(1, "ping", json!({}));
    at_the_top["_meta"] = json!({"progressToken": 1});
    let replies = talk(&[at_the_top, call(2, "command_run", json!({"id": "file.new", "_meta": {}}))]);
    assert_eq!(code(&replies[0]), -32602, "{replies:?}");
    assert!(message(&replies[0]).contains("_meta"), "{replies:?}");
    assert_eq!(code(&replies[1]), -32602, "{replies:?}");
}

#[test]
fn a_nested_unknown_key_is_the_engines_error_not_a_protocol_error() {
    let replies = talk(&[call(1, "command_run", json!({"id": "file.new", "params": {"surprise": 1}}))]);
    assert!(replies[0].get("error").is_none(), "{replies:?}");
    assert_eq!(replies[0]["result"]["isError"], true);
    assert!(tool_text(&replies[0]).contains("surprise"), "{replies:?}");
}

#[test]
fn failed_command_is_iserror_not_rpc_error() {
    let replies = talk(&[
        // No document yet.
        call(1, "command_run", json!({"id": "page.add"})),
        call(2, "command_run", json!({"id": "no.such.command"})),
        call(3, "command_run", json!({"id": "file.new"})),
        call(4, "command_run", json!({"id": "page.rename", "params": {"id": 99, "name": "x"}})),
        call(5, "command_run", json!({})),
        call(6, "command_run", json!({"id": 7})),
        call(7, "command_run", json!({"id": "page.add", "params": [1]})),
    ]);
    for (i, expect) in [(0, "no document open"), (1, "unknown command"), (3, "99"), (4, "`id`"), (5, "`id`"), (6, "`params`")] {
        let reply = &replies[i];
        assert!(reply.get("error").is_none(), "step {i} is a protocol error: {reply}");
        assert_eq!(reply["result"]["isError"], true, "step {i}: {reply}");
        assert!(tool_text(reply).contains(expect), "step {i} should say {expect:?}: {reply}");
    }
    assert_eq!(replies[2]["result"]["isError"], false);
}

#[test]
fn ui_tool_without_app_is_iserror() {
    let shot = std::env::temp_dir().join(format!("diacraft-mcp-shot-{}.png", std::process::id()));
    let replies = talk(&[
        call(1, "ui_inspect", json!({})),
        call(2, "ui_screenshot", json!({"path": shot.to_str().unwrap()})),
        call(3, "ui_screenshot", json!({})),
    ]);
    for reply in &replies {
        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(tool_text(reply).contains("needs the app"), "{reply}");
        // And it says how to get one, which the backend's own answer for a `ui.` method does not.
        assert!(tool_text(reply).contains("--connect"), "{reply}");
    }
    assert!(!shot.exists(), "a headless server wrote a screenshot");
}

// ---------- command_batch ----------

#[test]
fn command_batch_is_one_undo_step() {
    let steps: Vec<Value> = (0..3).map(|i| json!({"id": "page.add", "params": {"name": format!("P{i}")}})).collect();
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_batch", json!({"steps": steps, "label": "Three pages"})),
        call(3, "doc_inspect", json!({})),
        call(4, "command_run", json!({"id": "edit.undo"})),
        call(5, "doc_inspect", json!({})),
    ]);
    let results = tool_json(&replies[1])["results"].clone();
    assert_eq!(results.as_array().map(Vec::len), Some(3), "{replies:?}");
    let doc = tool_json(&replies[2]);
    assert_eq!(doc["pages"].as_array().map(Vec::len), Some(4));
    assert_eq!(doc["history"]["undo"], 1, "the three edits are one step");
    assert_eq!(tool_json(&replies[3])["label"], "Three pages");
    assert_eq!(tool_json(&replies[4])["pages"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_failing_step_undoes_the_whole_batch_and_is_named() {
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(
            2,
            "command_batch",
            json!({"steps": [
                {"id": "page.add", "params": {"name": "Kept?"}},
                {"id": "page.rename", "params": {"id": 99, "name": "x"}},
                {"id": "page.add"},
            ]}),
        ),
        call(3, "doc_inspect", json!({})),
    ]);
    assert_eq!(replies[1]["result"]["isError"], true, "{replies:?}");
    assert!(tool_text(&replies[1]).contains("batch step 1"), "{replies:?}");
    let doc = tool_json(&replies[2]);
    assert_eq!(doc["pages"].as_array().map(Vec::len), Some(1), "nothing of the batch stayed");
    assert_eq!(doc["history"]["undo"], 0);
}

#[test]
fn command_batch_checks_its_arguments_before_running_anything() {
    let too_many: Vec<Value> = (0..257).map(|_| json!({"id": "page.add"})).collect();
    let exactly: Vec<Value> = (0..256).map(|_| json!({"id": "page.add"})).collect();
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_batch", json!({"steps": too_many})),
        call(3, "command_batch", json!({})),
        call(4, "command_batch", json!({"steps": "page.add"})),
        call(5, "command_batch", json!({"steps": [{"id": "page.add"}, {"command": "page.add"}]})),
        call(6, "command_batch", json!({"steps": [{"id": "page.add", "params": 3}]})),
        call(7, "command_batch", json!({"steps": [3]})),
        call(8, "doc_inspect", json!({})),
        call(9, "command_batch", json!({"steps": exactly})),
        call(10, "doc_inspect", json!({})),
    ]);
    for (i, reply) in replies.iter().enumerate().take(7).skip(1) {
        assert_eq!(reply["result"]["isError"], true, "step {i}: {reply:?}");
    }
    assert!(tool_text(&replies[1]).contains("at most 256"), "{:?}", replies[1]);
    // Nothing of the refused batches ran, not even the steps before a bad one.
    assert_eq!(tool_json(&replies[7])["pages"].as_array().map(Vec::len), Some(1));
    // Exactly the cap is fine.
    assert_eq!(replies[8]["result"]["isError"], false, "{:?}", replies[8]);
    assert_eq!(tool_json(&replies[9])["pages"].as_array().map(Vec::len), Some(257));
}

// ---------- hostile input ----------

/// A ping, one line of garbage, then `follow`, to a server with a headless session.
fn between(garbage: &[u8], follow: &[Value]) -> Vec<Value> {
    let mut input = lines(&[request(1, "ping", json!({}))]);
    input.extend_from_slice(garbage);
    input.push(b'\n');
    input.extend(lines(follow));
    talk_on(&mut Backend::Headless(Box::new(Session::new())), &input)
}

/// One line of garbage between two pings: the garbage gets one error and the pings are answered.
fn between_pings(garbage: &[u8]) -> Vec<Value> {
    between(garbage, &[request(2, "ping", json!({}))])
}

#[test]
fn malformed_json_is_one_parse_error_and_the_server_continues() {
    for garbage in ["{\"jsonrpc\":", "not json", "{'a':1}", "{\"a\":1} trailing", "\u{feff}{}", "{\"a\":1,}", "1 2"] {
        let replies = between_pings(garbage.as_bytes());
        assert_eq!(replies.len(), 3, "{garbage:?}: {replies:?}");
        assert_eq!(code(&replies[1]), -32700, "{garbage:?}: {replies:?}");
        assert_eq!(replies[1]["id"], Value::Null);
        assert_eq!(replies[2]["id"], 2, "{garbage:?}: the server went on");
        assert_eq!(replies[2]["result"], json!({}));
    }
}

#[test]
fn text_that_is_not_utf8_is_one_error_and_the_server_continues() {
    let replies = between_pings(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"\xff\xfe\"}");
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(code(&replies[1]), -32700, "{replies:?}");
    assert!(message(&replies[1]).contains("UTF-8"), "{replies:?}");
    assert_eq!(replies[2]["id"], 2);
}

#[test]
fn json_that_is_not_an_object_is_an_invalid_request() {
    for value in ["42", "\"text\"", "null", "true", "1.5"] {
        let replies = between_pings(value.as_bytes());
        assert_eq!(replies.len(), 3, "{value}: {replies:?}");
        assert_eq!(code(&replies[1]), -32600, "{value}: {replies:?}");
        assert_eq!(replies[2]["id"], 2, "{value}");
    }
    // In a batch, the entry is the error and the others are answered.
    let replies = between_pings(b"[1, {\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ping\"}, [], null]");
    let batch = replies[1].as_array().unwrap_or_else(|| panic!("a batch is answered with an array: {replies:?}"));
    assert_eq!(batch.len(), 4);
    assert_eq!(code(&batch[0]), -32600);
    assert_eq!(batch[1]["id"], 5);
    assert_eq!(code(&batch[2]), -32600);
    assert_eq!(code(&batch[3]), -32600);
}

#[test]
fn bad_envelopes_get_one_error_each() {
    let bad = [
        (json!({"id": 1, "method": "ping"}), -32600),
        (json!({"jsonrpc": "1.0", "id": 1, "method": "ping"}), -32600),
        (json!({"jsonrpc": 2, "id": 1, "method": "ping"}), -32600),
        (json!({"jsonrpc": "2.0", "id": 1}), -32600),
        (json!({"jsonrpc": "2.0", "id": 1, "method": 5}), -32600),
        (json!({"jsonrpc": "2.0", "id": null, "method": "ping"}), -32600),
        (json!({"jsonrpc": "2.0", "id": true, "method": "ping"}), -32600),
        (json!({"jsonrpc": "2.0", "id": [1], "method": "ping"}), -32600),
        (json!({"jsonrpc": "2.0", "id": {"a": 1}, "method": "ping"}), -32600),
        (json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": 3}), -32602),
        (json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": [1]}), -32602),
        (json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "extra": 1}), -32602),
    ];
    for (message, expected) in bad {
        let replies = talk(std::slice::from_ref(&message));
        assert_eq!(replies.len(), 1, "{message}: {replies:?}");
        assert_eq!(code(&replies[0]), expected, "{message}: {replies:?}");
    }
    // A string id and a number id are both echoed as they came.
    let replies = talk(&[json!({"jsonrpc": "2.0", "id": "abc", "method": "ping"}), json!({"jsonrpc": "2.0", "id": 1.5, "method": "ping"})]);
    assert_eq!(replies[0]["id"], "abc");
    assert_eq!(replies[1]["id"], 1.5);
}

#[test]
fn a_message_without_an_id_is_never_answered_and_never_run() {
    let replies = talk(&[
        json!({"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "command_run", "arguments": {"id": "file.new"}}}),
        json!({"jsonrpc": "2.0", "method": "no/such/notification"}),
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}}),
        // A response to a request the server never made.
        json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
        call(2, "doc_inspect", json!({})),
    ]);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(tool_text(&replies[0]).contains("no document open"), "the call without an id ran: {replies:?}");
}

#[test]
fn a_thousand_levels_is_one_error() {
    // 1,000 levels is far past serde_json's limit of 128. The limit is on unless code calls `disable_recursion_limit`
    // (which needs the `unbounded_depth` feature, and the xtask test `serde_json_keeps_its_recursion_limit` watches
    // for that), and without it a deeper value could overflow the stack. This pins the error, and the going on.
    let arrays = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
    let objects = format!("{}1{}", "{\"a\":".repeat(1000), "}".repeat(1000));
    let inside_a_call = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{{\"name\":\"command_run\",\"arguments\":{{\"id\":\"file.new\",\"params\":{}1{}}}}}}}",
        "{\"a\":".repeat(1000),
        "}".repeat(1000)
    );
    for (what, garbage) in [("arrays", arrays), ("objects", objects), ("inside a call", inside_a_call)] {
        let replies = between(garbage.as_bytes(), &[request(2, "ping", json!({})), call(3, "doc_inspect", json!({}))]);
        assert_eq!(replies.len(), 4, "{what}: {replies:?}");
        assert_eq!(code(&replies[1]), -32700, "{what}: {replies:?}");
        assert!(message(&replies[1]).contains("recursion limit"), "{what}: {replies:?}");
        assert_eq!(replies[2]["id"], 2, "{what}: the server went on");
        // The call inside the third one did not run.
        assert!(tool_text(&replies[3]).contains("no document open"), "{what}: {replies:?}");
    }
}

#[test]
fn deep_but_allowed_nesting_reaches_the_engine_and_is_refused_cleanly() {
    // Inside the limit: parsed, and then the engine's own parameter check answers.
    let nested = (0..100).fold(json!(1), |inner, _| json!({"a": inner}));
    let replies = talk(&[call(1, "command_run", json!({"id": "file.new", "params": {"a": nested}}))]);
    assert_eq!(replies[0]["result"]["isError"], true, "{replies:?}");
    assert!(tool_text(&replies[0]).contains("unknown parameter"), "{replies:?}");
}

#[test]
fn oversized_line_is_an_error_and_the_server_continues() {
    // A line over the limit that starts out as a complete, valid request: it must not run.
    let mut overlong = lines(&[call(1, "command_run", json!({"id": "file.new"}))]);
    overlong.pop();
    overlong.resize(MAX_LINE_BYTES + 1, b' ');
    let mut input = overlong;
    input.push(b'\n');
    input.extend(lines(&[request(2, "ping", json!({})), call(3, "doc_inspect", json!({}))]));
    let replies = talk_on(&mut Backend::Headless(Box::new(Session::new())), &input);
    assert_eq!(replies.len(), 3, "{:?}", replies.iter().map(|r| r.to_string().chars().take(200).collect::<String>()).collect::<Vec<_>>());
    assert_eq!(code(&replies[0]), -32600);
    assert_eq!(replies[0]["id"], Value::Null);
    assert!(message(&replies[0]).contains("longer than"), "{:?}", replies[0]);
    assert_eq!(replies[1]["result"], json!({}), "the server went on with the next line");
    assert!(tool_text(&replies[2]).contains("no document open"), "the oversized request ran: {:?}", replies[2]);
}

/// An input of `total` bytes of `x` with no newline, handed out in 8 KiB chunks, that counts what has been taken.
struct Endless {
    left: usize,
    taken: Arc<AtomicUsize>,
}

impl std::io::Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let chunk = std::io::BufRead::fill_buf(self)?;
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        std::io::BufRead::consume(self, n);
        Ok(n)
    }
}

impl std::io::BufRead for Endless {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        static CHUNK: [u8; 8192] = [b'x'; 8192];
        Ok(&CHUNK[..self.left.min(CHUNK.len())])
    }

    fn consume(&mut self, amount: usize) {
        self.left -= amount;
        self.taken.fetch_add(amount, Ordering::SeqCst);
    }
}

/// An output that notes how much of the input had been taken when the first reply was written.
struct Notes {
    taken: Arc<AtomicUsize>,
    first_reply_at: Arc<Mutex<Option<usize>>>,
    written: Vec<u8>,
}

impl std::io::Write for Notes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.first_reply_at.lock().unwrap().get_or_insert(self.taken.load(Ordering::SeqCst));
        self.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn an_oversized_line_is_answered_after_the_limit_is_read_and_the_rest_is_dropped_unheld() {
    let total = 3 * MAX_LINE_BYTES;
    let (taken, first_reply_at) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(None)));
    let input = Endless { left: total, taken: taken.clone() };
    let output = Notes { taken: taken.clone(), first_reply_at: first_reply_at.clone(), written: Vec::new() };
    let mut backend = Backend::Headless(Box::new(Session::new()));
    diacraft_mcp::serve_with(input, output, &mut backend).unwrap();
    // The error was written when the limit (and a line ending's worth) had been read, not when the line ended:
    // a client that sends a line without end is told before it, and the server holds no more than the limit.
    let at = first_reply_at.lock().unwrap().expect("a reply was written");
    assert!((MAX_LINE_BYTES + 1..=MAX_LINE_BYTES + 3 + 8192).contains(&at), "the first reply came after {at} bytes of {total}");
    // And then the whole of the line was read through, so that the end of the input is the end of the server.
    assert_eq!(taken.load(Ordering::SeqCst), total);
}

#[test]
fn an_oversized_line_with_no_end_is_one_error_and_the_server_stops_cleanly() {
    // Three times the limit, never a newline: one error, no more than the limit held in memory, and the end of
    // the input is the end of the server.
    let input = vec![b'x'; 3 * MAX_LINE_BYTES];
    let replies = talk_on(&mut Backend::Headless(Box::new(Session::new())), &input);
    assert_eq!(replies.len(), 1);
    assert_eq!(code(&replies[0]), -32600);
}

#[test]
fn a_line_exactly_at_the_limit_is_still_a_message() {
    for ending in ["\n", "\r\n"] {
        let mut line = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#.to_vec();
        line.resize(MAX_LINE_BYTES, b' ');
        line.extend_from_slice(ending.as_bytes());
        line.extend(lines(&[request(2, "ping", json!({}))]));
        let replies = talk_on(&mut Backend::Headless(Box::new(Session::new())), &line);
        assert_eq!(replies.len(), 2, "{ending:?}: {replies:?}");
        assert_eq!(replies[0]["result"], json!({}), "{ending:?}: {replies:?}");
        assert_eq!(replies[1]["id"], 2);
    }
}

#[test]
fn oversized_batch_is_refused() {
    let batch = |n: u64| Value::Array((0..n).map(|i| call(i, "command_run", json!({"id": "file.new"}))).collect());
    let over = (MAX_BATCH_MESSAGES + 1) as u64;
    let replies = talk(&[batch(over), call(1, "doc_inspect", json!({})), json!([])]);
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(code(&replies[0]), -32600, "one error for the whole batch: {replies:?}");
    assert!(message(&replies[0]).contains(&over.to_string()) && message(&replies[0]).contains("64"), "{replies:?}");
    assert!(tool_text(&replies[1]).contains("no document open"), "a message of the refused batch ran: {replies:?}");
    assert_eq!(code(&replies[2]), -32600, "an empty batch is invalid: {replies:?}");
    // Exactly the cap is answered, in order, one reply each.
    let replies = talk(&[Value::Array((0..MAX_BATCH_MESSAGES as u64).map(|i| request(i, "ping", json!({}))).collect())]);
    let answered = replies[0].as_array().unwrap();
    assert_eq!(answered.len(), MAX_BATCH_MESSAGES);
    assert!(answered.iter().enumerate().all(|(i, r)| r["id"] == i && r["result"] == json!({})));
}

#[test]
fn a_batch_of_only_notifications_gets_no_reply() {
    let replies = talk(&[json!([{"jsonrpc": "2.0", "method": "notifications/initialized"}]), request(1, "ping", json!({}))]);
    assert_eq!(replies.len(), 1, "{replies:?}");
}

#[test]
fn line_separators_in_a_reply_are_written_as_escapes() {
    // A page name may hold U+2028 and U+2029: legal in JSON, but a line reader that splits at them would cut the reply.
    let name = "a\u{2028}b\u{2029}c";
    let input = lines(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "page.rename", "params": {"id": 1, "name": name}})),
        call(3, "doc_inspect", json!({})),
    ]);
    let raw = crate::raw_on(&mut Backend::Headless(Box::new(Session::new())), &input);
    assert!(!raw.contains('\u{2028}') && !raw.contains('\u{2029}'), "a raw line separator in the output");
    assert!(raw.contains("\\u2028") && raw.contains("\\u2029"), "the separators are escapes: {raw}");
    assert_eq!(raw.lines().count(), 3);
    // And they say the same thing: the name comes back as it went in.
    let last: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
    assert_eq!(tool_json(&last)["pages"][0]["name"], name);
}

#[test]
fn client_text_in_errors_is_escaped() {
    let nasty = "evil\n\u{1b}[2J\u{202e}\u{2028}";
    let replies = talk(&[
        call(1, nasty, json!({})),
        call(2, "command_run", json!({"id": "file.new", nasty: 1})),
        request(3, "tools/call", json!({"name": "command_run", nasty: 1})),
        call(4, "command_run", json!({"id": nasty})),
        request(5, "tools/call", json!({"name": 5})),
        request(6, "tools/call", json!({"name": "command_run", "arguments": 5})),
    ]);
    for reply in &replies[..3] {
        assert_eq!(code(reply), -32602, "{reply}");
        assert!(is_plain(&message(reply)), "{reply}");
        assert!(message(reply).contains("\\u{1b}"), "{reply}");
    }
    // A command id that is not one: the engine's error, escaped, in a tool result.
    assert!(is_plain(&tool_text(&replies[3])), "{:?}", replies[3]);
    assert_eq!(code(&replies[4]), -32602);
    assert_eq!(code(&replies[5]), -32602);
}
