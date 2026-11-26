//! `run` asks what a save would carry and lose, with `file.check_compatibility`, and nothing is written: the command is the engine's, the command line only reaches it.

use serde_json::json;

use crate::{TempDir, cli, json_lines, s, text};

#[test]
fn run_answers_what_a_save_would_carry_and_writes_no_file() {
    let dir = TempDir::new("check-before");
    let target = dir.path("out.odg");
    let params = json!({ "path": s(&target) }).to_string();
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "file.check_compatibility", "--params", &params, "--cmd", "document.inspect"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[1]["command"], "file.check_compatibility");
    let answer = &lines[1]["result"];
    assert_eq!((&answer["writer"], &answer["format"], &answer["variant"]), (&json!("odg_model"), &json!("odg"), &json!("odg")), "{answer}");
    assert!(answer["bytes"].as_u64().is_some_and(|n| n > 0), "{answer}");
    assert!(answer.get("carried").is_some() && answer["checked"].is_object(), "{answer}");
    assert!(!target.exists(), "a dry run writes no file");
    assert_eq!(lines[2]["result"]["history"]["undo"], 0, "and no undo step");
}

#[test]
fn run_ends_in_the_saves_words_when_the_save_would_be_refused_and_commands_lists_it() {
    let dir = TempDir::new("check-before");
    let params = json!({ "path": s(&dir.path("out.fodg")) }).to_string();
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "file.check_compatibility", "--params", &params], None);
    assert_eq!(out.status.code(), Some(1), "a `.fodg` is no target of a Save As");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 1 (file.check_compatibility)") && stderr.contains("Save the drawing as an .odg"), "{stderr}");
    assert_eq!(json_lines(&out.stdout).len(), 1, "the step before it printed its line");
    assert!(!dir.path("out.fodg").exists());
    let out = cli(&["commands"], None);
    let printed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let command = printed.as_array().unwrap().iter().find(|c| c["id"] == "file.check_compatibility").expect("listed");
    assert_eq!(command["label"], "Check Compatibility");
}
