//! What the window does with a request's params before it runs anything: it reads them where they are.
//!
//! The params of a request can be megabytes (a `command.batch` of 256 steps), and the frame thread is the thread a person is
//! looking at: a copy of them per request, or two, is time the window does not draw. These tests check by address that what
//! the helpers hand back is the request's own value, so a copy would show as a different address. (No other test can see a
//! clone: counting allocations needs an `unsafe` allocator, and this crate forbids `unsafe`.)

use serde_json::{Map, Value, json};

use super::{execute_args, params_object};

#[test]
fn the_params_object_is_the_requests_own_map() {
    let params = json!({"command": "page.add", "params": {"name": "A"}});
    let map = params_object(&params).unwrap();
    let map: &Map<String, Value> = &map;
    assert!(std::ptr::eq(map, params.as_object().unwrap()), "the params were copied");
    assert_eq!(map.len(), 2);
}

#[test]
fn null_params_are_an_empty_object_and_anything_else_is_an_error() {
    assert_eq!(params_object(&Value::Null).unwrap().len(), 0);
    for bad in [json!(1), json!("x"), json!([1]), json!(true)] {
        assert_eq!(params_object(&bad).unwrap_err(), "params must be a JSON object");
    }
}

/// The params `engine.execute` hands to the command are the ones inside the request, not a copy of them.
#[test]
fn the_commands_params_are_the_requests_own_value() {
    let request = json!({"command": "command.batch", "params": {"commands": [{"command": "page.add"}]}});
    let map = request.as_object().unwrap();
    let (command, params) = execute_args(map).unwrap();
    assert_eq!(command, "command.batch");
    let params: &Value = &params;
    assert!(std::ptr::eq(params, &map["params"]), "the command's params were copied");
}

#[test]
fn missing_or_null_command_params_are_an_empty_object() {
    for request in [json!({"command": "page.add"}), json!({"command": "page.add", "params": null})] {
        let (command, params) = execute_args(request.as_object().unwrap()).unwrap();
        assert_eq!((command, &*params), ("page.add", &json!({})));
    }
}

#[test]
fn a_bad_engine_execute_request_is_an_error_that_says_what_it_needs() {
    let error = |request: Value| execute_args(request.as_object().unwrap()).map(|_| ()).unwrap_err();
    assert!(error(json!({})).contains("needs a string `command`"));
    assert!(error(json!({"command": 5})).contains("needs a string `command`"));
    assert!(error(json!({"command": "x", "params": [1]})).contains("`params` must be an object"));
    let unknown = error(json!({"command": "x", "extra": 1}));
    assert!(unknown.contains("unknown key `extra`") && unknown.contains("command, params"), "{unknown}");
}
