//! A program asks what a save would carry and lose through `command_run` of `file.check_compatibility`, and nothing is written (the command is the engine's, the server only reaches it).

use serde_json::json;

use crate::{call, talk, tool_json, tool_text};

/// A scratch folder under the system temp dir (`TMPDIR`), removed on drop.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let dir = std::env::temp_dir().join(format!("diacraft-mcp-check-before-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Litter in a scratch folder is harmless.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn command_run_answers_what_a_save_would_carry_and_writes_no_file() {
    let scratch = Scratch::new();
    let target = scratch.0.join("out.vsdx");
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "file.check_compatibility", "params": {"path": target}})),
        call(3, "command_run", json!({"id": "document.inspect"})),
    ]);
    for reply in &replies {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    let answer = tool_json(&replies[1]);
    assert_eq!((&answer["writer"], &answer["format"], &answer["variant"]), (&json!("vsdx_model"), &json!("vsdx"), &json!("vsdx")), "{answer}");
    assert!(answer["bytes"].as_u64().is_some_and(|n| n > 0), "{answer}");
    assert!(answer.get("carried").is_some() && answer.get("checked").is_some(), "{answer}");
    assert!(!target.exists(), "a dry run writes no file");
    // The drawing is as it was: no undo step, and it is not unsaved.
    assert_eq!(tool_json(&replies[2])["history"]["undo"], 0);
}

#[test]
fn command_run_fails_in_the_saves_words_and_is_listed_with_its_label() {
    let scratch = Scratch::new();
    let replies = talk(&[
        call(1, "command_run", json!({"id": "file.new"})),
        call(2, "command_run", json!({"id": "file.check_compatibility", "params": {"path": scratch.0.join("out.xyz")}})),
        call(3, "command_run", json!({"id": "file.save_as", "params": {"path": scratch.0.join("out.xyz")}})),
        call(4, "command_list", json!({})),
    ]);
    assert_eq!(replies[1]["result"]["isError"], true, "{}", replies[1]);
    assert_eq!(replies[2]["result"]["isError"], true, "{}", replies[2]);
    // The words are the save's, with the command's own id in front.
    let (asked, saved) = (tool_text(&replies[1]), tool_text(&replies[2]));
    let words = |text: &str| text.split_once("the file name must end in").map(|(_, rest)| rest.to_owned());
    assert!(words(&asked).is_some() && words(&asked) == words(&saved), "{asked} | {saved}");
    let listed = tool_json(&replies[3]);
    let command = listed.as_array().unwrap().iter().find(|c| c["id"] == "file.check_compatibility").expect("listed");
    assert_eq!(command["label"], "Check Compatibility");
    assert_eq!(command["menu"], json!(["File", "Info"]));
    assert_eq!(command["enabled"], true);
    assert!(!scratch.0.join("out.xyz").exists());
}
