//! Properties of the server over hostile input: whatever the bytes, it answers in protocol, and what it
//! answers is never the safety net's report of a bug.
//!
//! The server catches a panic and answers with `-32603` or an `isError` result saying "internal error", so
//! "does not panic" alone would hold for a server that panics on every line. Each property checks for those
//! replies, and for the engine's own count of panics it caught, which must stay at 0.

use diacraft_engine::{Session, command_specs};
use diacraft_mcp::Backend;
use diacraft_testkit::strategies::junk_values;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::{lines, talk_on};

/// The tools' names.
const TOOLS: [&str; 6] = ["command_list", "command_run", "command_batch", "doc_inspect", "ui_inspect", "ui_screenshot"];

/// Any JSON value, with hostile leaves (the shared junk values) among the random ones.
fn arb_json() -> impl Strategy<Value = Value> {
    let junk = junk_values();
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_map(Value::from),
        ".{0,16}".prop_map(Value::from),
        proptest::sample::select(junk),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec(("[a-z_]{0,8}", inner), 0..4).prop_map(|pairs| Value::Object(pairs.into_iter().collect())),
        ]
    })
}

/// The id of some command in the registry, or a name that is not one.
fn arb_command_id() -> impl Strategy<Value = Value> {
    let ids: Vec<&'static str> = command_specs().iter().map(|c| c.id).collect();
    prop_oneof![4 => proptest::sample::select(ids).prop_map(Value::from), 1 => arb_json()]
}

/// The arguments of one tool call: made for the tool (real keys, hostile values), or anything at all.
fn arb_arguments(tool: &'static str) -> BoxedStrategy<Value> {
    let key = proptest::sample::select(vec!["id", "params", "steps", "label", "filter", "enabled_only", "brief", "path", "depth", "extra"]);
    let keyed =
        prop::collection::vec((key, arb_json()), 0..4).prop_map(|pairs| Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()));
    let for_run = (arb_command_id(), arb_json()).prop_map(|(id, params)| json!({"id": id, "params": params}));
    let for_batch = prop::collection::vec((arb_command_id(), arb_json()).prop_map(|(id, params)| json!({"id": id, "params": params})), 0..6)
        .prop_map(|steps| json!({"steps": steps}));
    match tool {
        "command_run" => prop_oneof![3 => for_run, 1 => keyed, 1 => arb_json()].boxed(),
        "command_batch" => prop_oneof![3 => for_batch, 1 => keyed, 1 => arb_json()].boxed(),
        _ => prop_oneof![1 => Just(json!({})), 2 => keyed, 1 => arb_json()].boxed(),
    }
}

/// A `tools/call`: a tool, or now and then a made-up name, with arguments made for that tool (real keys, hostile
/// values) or now and then any JSON at all. The arguments are for the tool the call names: a call that gave
/// `command_run` the arguments of `ui_inspect` would seldom get as far as running a command.
fn arb_tool_call() -> impl Strategy<Value = Value> {
    let junk_name = prop_oneof![5 => Just(false), 1 => Just(true)];
    let made_for_the_tool = prop_oneof![3 => Just(true), 1 => Just(false)];
    (0..TOOLS.len(), junk_name, ".{0,12}", arb_json(), made_for_the_tool).prop_flat_map(
        |(tool, junk_name, other_name, any_args, made_for_the_tool)| {
            let real = TOOLS.get(tool).copied().unwrap_or("doc_inspect");
            let name = if junk_name { other_name } else { real.to_owned() };
            let args = if made_for_the_tool { arb_arguments(real) } else { Just(any_args).boxed() };
            args.prop_map(move |arguments| json!({"name": name.clone(), "arguments": arguments}))
        },
    )
}

/// What a reply must look like, whatever was asked: protocol, and not a bug report.
fn check_reply(reply: &Value) -> Result<(), TestCaseError> {
    if let Value::Array(batch) = reply {
        for r in batch {
            check_reply(r)?;
        }
        return Ok(());
    }
    prop_assert_eq!(&reply["jsonrpc"], "2.0", "{}", reply);
    prop_assert!(reply.get("result").is_some() != reply.get("error").is_some(), "exactly one of result and error: {}", reply);
    if let Some(code) = reply["error"]["code"].as_i64() {
        prop_assert!([-32700, -32600, -32601, -32602].contains(&code), "an error code a bug would give: {}", reply);
        prop_assert!(reply["error"]["message"].as_str().is_some_and(|m| !m.contains("internal error")), "{}", reply);
    }
    if reply.get("result").is_some_and(|r| r.get("isError").is_some()) {
        let text = reply["result"]["content"][0]["text"].as_str().unwrap_or_default();
        prop_assert!(reply["result"]["isError"].is_boolean() && !text.contains("internal error"), "a tool reported a bug: {}", reply);
        prop_assert!(!text.contains("please report"), "{}", reply);
    }
    Ok(())
}

/// What a headless backend must still be after anything: no panic caught on the way, a valid document.
fn check_backend(backend: &Backend) -> Result<(), TestCaseError> {
    let Backend::Headless(session) = backend else { return Ok(()) };
    prop_assert_eq!(session.panics_caught(), 0, "the engine caught a panic");
    if let Some(doc) = session.document() {
        prop_assert!(doc.validate().is_ok(), "the document is invalid: {:?}", doc.validate());
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Lines of any bytes: each gets at most one reply, which is in protocol, and the server ends cleanly.
    #[test]
    fn random_lines_never_panic(raw in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..160), 0..10)) {
        let mut input = Vec::new();
        for line in &raw {
            input.extend(line.iter().filter(|b| **b != b'\n'));
            input.push(b'\n');
        }
        let mut backend = Backend::Headless(Box::new(Session::new()));
        let replies = talk_on(&mut backend, &input);
        prop_assert!(replies.len() <= raw.len(), "more replies than lines");
        for reply in &replies {
            check_reply(reply)?;
        }
        check_backend(&backend)?;
    }

    /// JSON of any shape, as a line, and as the params of a request for any method.
    #[test]
    fn random_json_never_panics(values in prop::collection::vec(arb_json(), 0..8), methods in prop::collection::vec(".{0,24}", 0..4)) {
        let mut messages = values.clone();
        for (i, method) in methods.iter().enumerate() {
            let params = values.get(i).cloned().unwrap_or(Value::Null);
            messages.push(json!({"jsonrpc": "2.0", "id": i, "method": method, "params": params}));
            messages.push(json!({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": params}));
            messages.push(json!({"jsonrpc": "2.0", "id": i, "method": "initialize", "params": params}));
        }
        let mut backend = Backend::Headless(Box::new(Session::new()));
        let replies = talk_on(&mut backend, &lines(&messages));
        prop_assert!(replies.len() <= messages.len());
        for reply in &replies {
            check_reply(reply)?;
        }
        check_backend(&backend)?;
    }

    /// Tool calls with hostile names and arguments, in one session: each is answered, as a result or `-32602`, and
    /// whatever they did to the document left it valid.
    #[test]
    fn random_tool_calls_never_panic(calls in prop::collection::vec(arb_tool_call(), 1..12)) {
        let mut messages = vec![json!({"jsonrpc": "2.0", "id": 0, "method": "tools/call", "params": {"name": "command_run", "arguments": {"id": "file.new"}}})];
        messages.extend(calls.iter().enumerate().map(|(i, params)| json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": params})));
        let mut backend = Backend::Headless(Box::new(Session::new()));
        let replies = talk_on(&mut backend, &lines(&messages));
        prop_assert_eq!(replies.len(), messages.len(), "every call is answered once");
        for (i, reply) in replies.iter().enumerate() {
            prop_assert_eq!(&reply["id"], &json!(i), "replies come in order");
            check_reply(reply)?;
        }
        check_backend(&backend)?;
    }
}
