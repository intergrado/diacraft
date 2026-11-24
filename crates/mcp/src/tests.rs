//! What the integration tests cannot reach: the nets under the tools and under the protocol code, tried with
//! closures that panic. No tool or message panics today, so only a closure can make one.

use serde_json::{Value, json};

use crate::protocol::guarded as protocol_guarded;
use crate::tools::guarded as tool_guarded;

#[test]
fn a_panic_in_a_tool_is_an_iserror_result_that_says_so() {
    let result = tool_guarded("command_run", || -> Result<Value, String> { panic!("boom\u{1b}[2J") });
    assert!(result.is_error);
    assert!(
        result.text.contains("internal error in `command_run`") && result.text.contains("boom") && result.text.contains("please report"),
        "{}",
        result.text
    );
    // The panic's message is a client's text, in effect: it does not reach the reply as a terminal escape.
    assert!(!result.text.contains('\u{1b}'), "{:?}", result.text);
    // Nothing is left behind: the next call is an ordinary one.
    let fine = tool_guarded("command_run", || Ok(json!({"a": 1})));
    assert_eq!((fine.text.as_str(), fine.is_error), ("{\"a\":1}", false));
    let failed = tool_guarded("command_run", || Err("no document open".to_owned()));
    assert_eq!((failed.text.as_str(), failed.is_error), ("no document open", true));
}

#[test]
fn a_panic_in_the_protocol_code_is_an_internal_error_for_that_message() {
    let reply = protocol_guarded(Some(json!(7)), || panic!("boom")).expect("a request is answered");
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], -32603);
    assert!(reply["error"]["message"].as_str().is_some_and(|m| m.contains("boom") && m.contains("please report")), "{reply}");
    // A notification has nobody to tell.
    assert_eq!(protocol_guarded(None, || panic!("boom")), None);
    // And nothing is changed for a handler that does not panic.
    assert_eq!(protocol_guarded(Some(json!(1)), || Some(json!(5))), Some(json!(5)));
    assert_eq!(protocol_guarded(Some(json!(1)), || None), None);
}
