//! `diacraft-cli run`, `commands`, `--version` and `--help`: the binary, run.

use std::process::{Command, Stdio};

use diacraft_engine::command_specs;
use serde_json::{Value, json};

use crate::{BIN, TempDir, cli, json_lines, s, text};

#[test]
fn run_prints_one_line_per_step() {
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "page.add", "--params", r#"{"name":"A"}"#, "--cmd", "document.inspect"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3, "{lines:?}");
    for (i, (line, command)) in lines.iter().zip(["file.new", "page.add", "document.inspect"]).enumerate() {
        assert_eq!(line["step"], i, "{line}");
        assert_eq!(line["command"], command, "{line}");
        assert!(line.get("result").is_some(), "{line}");
    }
    assert_eq!(lines[1]["result"], json!({"id": 2, "name": "A"}));
    // The steps ran in one session, in order.
    let pages: Vec<&str> = lines[2]["result"]["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(pages, ["Page-1", "A"]);
    assert_eq!(lines[2]["result"]["history"]["undo"], 1);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// `app.version` answers the git release and the year the binary was built at (`build.rs`), which the command line hands the engine at its start. What they say
/// depends on the tree it was built from, so only their shape is asserted: a release that is some text (`unknown` in a tree with no git) and a year of four digits.
#[test]
fn app_version_answers_the_git_release_and_the_year_the_binary_was_built_at() {
    let out = cli(&["run", "--cmd", "app.version"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let result = json_lines(&out.stdout)[0]["result"].clone();
    assert_eq!(result["app"], "diacraft");
    assert_eq!(result["version"], env!("CARGO_PKG_VERSION"));
    let git = result["git"].as_str().expect("a git release, or `unknown`");
    assert!(!git.is_empty() && git.chars().all(|c| (' '..='~').contains(&c)), "{git:?}");
    let year = result["year"].as_str().expect("a year");
    assert!(year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) && year >= "2026", "{year:?}");
    assert_eq!(result.as_object().unwrap().len(), 4, "{result}");
    // Built in a checkout (a folder or a worktree's file `.git` above this crate) with git installed, the release is what git said, and not the engine's `unknown`, which is
    // what the command line would answer if it never handed the engine its build.
    let in_a_checkout = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().any(|folder| folder.join(".git").exists());
    let overridden = std::env::var_os("DIACRAFT_BUILD_GIT").is_some();
    if in_a_checkout && !overridden {
        assert_ne!(git, "unknown", "the build was made in a checkout");
        // The line is read under the version, so it starts with it (`0.2.0` for a build of the tag, `0.2.0-dev+<commit>` before the tag exists, `0.2.0+12.<commit>` after
        // it) and never names the last release's tag (`v0.1.0-1555-g...`), whatever the checkout's tags are.
        assert!(git.starts_with(env!("CARGO_PKG_VERSION")), "{git:?}: the build line starts with the version {}", env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn run_writes_line_separators_as_escapes_so_one_step_is_one_line() {
    // A page name may hold U+2028, which some line readers (Python's `splitlines`) split a line at.
    let params = "{\"id\":1,\"name\":\"a\u{2028}b\u{2029}c\"}";
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "page.rename", "--params", params, "--cmd", "document.inspect"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(!stdout.contains('\u{2028}') && !stdout.contains('\u{2029}'), "a raw line separator in the output");
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2]["result"]["pages"][0]["name"], "a\u{2028}b\u{2029}c", "the escapes say the same text");
}

/// Find and Replace as steps of a run: `edit.find` lists the matches, `edit.replace` replaces them as one undo step, and `edit.undo` takes that step back.
#[test]
fn run_finds_and_replaces_text_as_steps_and_one_undo_takes_the_replace_back() {
    let drop = r#"{"master":"rectangle","page":1,"x":3,"y":3}"#;
    let say = r#"{"id":1,"page":1,"text":"Hello world, hello"}"#;
    let find = |what: &str| format!(r#"{{"what":"{what}","scope":"all"}}"#);
    let replace = r#"{"what":"hello","with":"bye","scope":"all"}"#;
    let (hello, bye) = (find("hello"), find("bye"));
    let out = cli(
        &[
            "run",
            "--cmd",
            "file.new",
            "--cmd",
            "shape.drop",
            "--params",
            drop,
            "--cmd",
            "shape.set_text",
            "--params",
            say,
            "--cmd",
            "edit.find",
            "--params",
            &hello,
            "--cmd",
            "edit.replace",
            "--params",
            replace,
            "--cmd",
            "edit.find",
            "--params",
            &bye,
            "--cmd",
            "edit.undo",
            "--cmd",
            "edit.find",
            "--params",
            &hello,
        ],
        None,
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 8, "{lines:?}");
    assert_eq!(lines[1]["result"]["id"], 1, "the shape the later steps name");
    let starts = |line: &Value| -> Vec<u64> { line["result"]["matches"].as_array().unwrap().iter().map(|m| m["start"].as_u64().unwrap()).collect() };
    assert_eq!(starts(&lines[3]), [0, 13], "two matches of `hello`, whatever the case");
    assert_eq!(lines[4]["result"]["replaced"], 2);
    assert_eq!(lines[4]["result"]["shapes"], 1);
    assert_eq!(starts(&lines[5]), [0, 11], "and now of `bye`, which is shorter by two");
    assert_eq!(lines[6]["result"]["label"], "Replace All", "one step took back both");
    assert_eq!(starts(&lines[7]), [0, 13]);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

#[test]
fn run_fails_naming_the_step() {
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "page.rename", "--params", r#"{"id":99,"name":"x"}"#, "--cmd", "page.add"], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 1 (page.rename)") && stderr.contains("99"), "{stderr}");
    // The step before it printed its line, and the step after it did not run.
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["command"], "file.new");
}

#[test]
fn run_names_a_step_that_is_not_a_command_and_does_not_echo_it_raw() {
    let out = cli(&["run", "--cmd", "no.such\u{1b}[2J\n"], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 0") && stderr.contains("unknown command"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "an escape sequence reached the terminal: {stderr:?}");
    assert_eq!(stderr.matches('\n').count(), 1, "one line: {stderr:?}");
}

#[test]
fn commands_prints_every_registered_id() {
    let out = cli(&["commands"], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<&str> = printed.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    let registered: Vec<&str> = command_specs().iter().map(|c| c.id).collect();
    assert!(registered.len() >= 8, "the registry is smaller than it was: {registered:?}");
    // Every id, once, in the registry's order: not a prefix, not a sample.
    assert_eq!(ids, registered);
    for command in printed.as_array().unwrap() {
        assert!(command["label"].is_string() && command["params"].is_string() && command["enabled"].is_boolean(), "{command}");
    }
}

#[test]
fn run_reads_steps_from_files_and_from_standard_input() {
    let dir = TempDir::new("steps");
    let array = dir.write("array.json", br#"[{"command": "page.add", "params": {"name": "B"}}, {"id": "page.add"}]"#);
    let lines =
        dir.write("lines.jsonl", b"\xef\xbb\xbf{\"command\":\"page.add\",\"params\":{\"name\":\"D\"}}\n\n  \n{\"id\":\"document.inspect\"}\n");
    let out = cli(
        &[
            "run",
            "--cmd",
            "file.new",
            "--commands",
            s(&array),
            "--cmd",
            "page.add",
            "--params",
            r#"{"name":"C"}"#,
            "--commands",
            "-",
            "--commands",
            s(&lines),
        ],
        Some(b"{\"command\":\"page.add\",\"params\":{\"name\":\"E\"}}\n[]"),
    );
    // Standard input starts with an object, so it is JSON Lines, and its second line, `[]`, is no step: the run is
    // refused whole, and names the source.
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("--commands -"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "nothing ran: {}", text(&out.stdout));

    let out = cli(
        &[
            "run",
            "--cmd",
            "file.new",
            "--commands",
            s(&array),
            "--cmd",
            "page.add",
            "--params",
            r#"{"name":"C"}"#,
            "--commands",
            "-",
            "--commands",
            s(&lines),
        ],
        Some(b"{\"command\":\"page.add\",\"params\":{\"name\":\"E\"}}\n"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    // file.new, B, unnamed, C, E, D, inspect: numbered across the sources, in the order given.
    assert_eq!(lines.len(), 7, "{lines:?}");
    assert_eq!(lines.iter().map(|l| l["step"].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6]);
    let names: Vec<&str> = lines[6]["result"]["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names[..2], ["Page-1", "B"]);
    assert_eq!(names[3..], ["C", "E", "D"]);
}

#[test]
fn run_refuses_a_bad_commands_file_before_running_anything() {
    let dir = TempDir::new("bad");
    let cases: [(&str, &[u8], &str); 8] = [
        ("not JSON", b"[{\"command\": ", "bad.json"),
        ("not an object", b"[{\"command\":\"app.version\"}, 7]", "step 2"),
        ("no command", b"[{\"params\":{}}]", "no `command`"),
        ("both keys", b"[{\"command\":\"app.version\",\"id\":\"app.version\"}]", "not both"),
        ("unknown key", b"[{\"command\":\"app.version\",\"parms\":{}}]", "unknown key `parms`"),
        ("params not an object", b"[{\"command\":\"app.version\",\"params\":[1]}]", "`params`"),
        ("command not text", b"[{\"command\":5}]", "`command`"),
        ("a line that is not JSON", b"{\"command\":\"app.version\"}\nnope\n", "line 2"),
    ];
    for (what, content, expect) in cases {
        let file = dir.write("bad.json", content);
        let out = cli(&["run", "--cmd", "app.version", "--commands", s(&file)], None);
        assert_eq!(out.status.code(), Some(1), "{what}: {}", text(&out.stderr));
        assert!(text(&out.stderr).contains(expect), "{what}: {}", text(&out.stderr));
        assert!(out.stdout.is_empty(), "{what}: the step before the bad file ran: {}", text(&out.stdout));
    }
    let out = cli(&["run", "--commands", s(&dir.path("missing.json"))], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("missing.json"), "{}", text(&out.stderr));
}

#[test]
fn run_takes_at_most_100000_steps() {
    let step = b"{\"command\":\"app.version\"}\n";
    let exactly: Vec<u8> = step.repeat(100_000);
    let out = cli(&["run", "--commands", "-"], Some(&exactly));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(json_lines(&out.stdout).len(), 100_000);

    let one_more: Vec<u8> = step.repeat(100_001);
    let out = cli(&["run", "--commands", "-"], Some(&one_more));
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("100000"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "a run over the limit started: {} bytes printed", out.stdout.len());

    // The limit is on the whole run, not on each source.
    let dir = TempDir::new("cap");
    let half = dir.write("half.jsonl", &step.repeat(60_000));
    let out = cli(&["run", "--commands", s(&half), "--commands", s(&half)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn run_takes_at_most_64_mib_of_steps() {
    let dir = TempDir::new("bytes");
    let limit = 64 * 1024 * 1024;
    // A valid step padded with blank lines to exactly 64 MiB is taken; one byte more is refused. (Blank lines, not
    // one long line: a line may not be over 16 MiB.)
    let mut content = b"{\"command\":\"app.version\"}\n".to_vec();
    while content.len() < limit {
        let room = (limit - content.len()).min(1024);
        content.extend(std::iter::repeat_n(b' ', room - 1));
        content.push(b'\n');
    }
    assert_eq!(content.len(), limit);
    let at_limit = dir.write("at.jsonl", &content);
    let out = cli(&["run", "--commands", s(&at_limit)], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(json_lines(&out.stdout).len(), 1);
    content.push(b' ');
    let over = dir.write("over.jsonl", &content);
    let out = cli(&["run", "--commands", s(&over)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("64 MiB"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    // The limit is on all the sources together.
    let out = cli(&["run", "--commands", s(&at_limit), "--commands", s(&at_limit)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

const MIB: usize = 1024 * 1024;

/// A JSON array of one valid step, padded with blanks to exactly `total` bytes.
fn padded_array(total: usize) -> Vec<u8> {
    let mut array = b"[{\"command\":\"app.version\"}".to_vec();
    array.resize(total - 1, b' ');
    array.push(b']');
    array
}

#[test]
fn the_array_form_takes_at_most_16_mib_in_all_and_says_to_use_json_lines() {
    let dir = TempDir::new("array");
    // 16 MiB exactly is taken; one byte more is refused before it is parsed, and the message names the way out.
    let at_limit = dir.write("at.json", &padded_array(16 * MIB));
    let out = cli(&["run", "--commands", s(&at_limit)], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(json_lines(&out.stdout).len(), 1);
    let over = dir.write("over.json", &padded_array(16 * MIB + 1));
    let out = cli(&["run", "--commands", s(&over)], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("16 MiB") && stderr.contains("JSON Lines"), "{stderr}");
    assert!(out.stdout.is_empty());

    // An array that would need more than the step limit once parsed is refused for its size, not parsed first: the
    // refusal that names JSON Lines is the one that comes, though the array also holds too many steps.
    let many = format!("[{}]", "{\"a\":0},".repeat(17 * MIB / 8));
    let many = dir.write("many.json", many.as_bytes());
    let out = cli(&["run", "--commands", s(&many)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("JSON Lines"), "{}", text(&out.stderr));

    // "In all": two arrays of 10 MiB are 20 MiB of the array form, and the second is refused, with nothing run.
    let ten = dir.write("ten.json", &padded_array(10 * MIB));
    let out = cli(&["run", "--commands", s(&ten), "--commands", s(&ten)], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("JSON Lines"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "a step ran: {}", text(&out.stdout));

    // JSON Lines is not the array form: a 10 MiB array and a 10 MiB JSON Lines file are both taken.
    let mut lines = b"{\"command\":\"app.version\"}\n".to_vec();
    lines.resize(10 * MIB, b' ');
    let lines = dir.write("ten.jsonl", &lines);
    let out = cli(&["run", "--commands", s(&ten), "--commands", s(&lines)], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(json_lines(&out.stdout).len(), 2);
}

#[test]
fn a_json_lines_line_takes_at_most_16_mib() {
    let dir = TempDir::new("line");
    let line = |total: usize| {
        let mut line = b"{\"command\":\"app.version\"}".to_vec();
        line.resize(total, b' ');
        line.push(b'\n');
        line
    };
    let at_limit = dir.write("at.jsonl", &line(16 * MIB));
    let out = cli(&["run", "--commands", s(&at_limit)], None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(json_lines(&out.stdout).len(), 1);
    let over = dir.write("over.jsonl", &line(16 * MIB + 1));
    let out = cli(&["run", "--commands", s(&over)], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("line 1") && stderr.contains("16 MiB"), "{stderr}");
    assert!(out.stdout.is_empty());
}

#[test]
fn run_stops_reading_standard_input_at_the_byte_limit() {
    // Standard input is a stream: a program that keeps sending must be cut off at the limit and not read to its end.
    // Up to 256 MiB are offered; a run that stopped reading at 64 MiB leaves the writer with a broken pipe soon after.
    let mut child =
        Command::new(BIN).args(["run", "--commands", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let mut block = vec![b' '; 1024 * 1024 - 1];
        block.push(b'\n');
        let mut written = 0_usize;
        while written < 256 * MIB {
            if std::io::Write::write_all(&mut stdin, &block).is_err() {
                break;
            }
            written += block.len();
        }
        written
    });
    let out = child.wait_with_output().unwrap();
    let written = writer.join().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("64 MiB"), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(written <= 64 * MIB + 4 * MIB, "the program read {written} bytes of an endless input before it stopped");
}

#[test]
fn a_usage_error_exits_with_2_and_says_what_is_wrong() {
    let nasty = "--no\u{1b}[2Jsuch\u{202e}";
    let cases: [(&[&str], &str); 11] = [
        (&["run", "--params", "{}"], "--params"),
        (&["run", "--cmd"], "--cmd"),
        (&["run", "--cmd", "app.version", "--params"], "--params"),
        (&["run", "--cmd", "app.version", "--params", "{\"a\":"], "--params"),
        (&["run", "--cmd", "app.version", "--params", "[1]"], "object"),
        (&["run", "--cmd", "app.version", "--params", "{}", "--params", "{}"], "twice"),
        (&["run", "--commands", "-", "--commands", "-"], "once"),
        (&["run"], "nothing to run"),
        (&["run", nasty], "unknown"),
        (&["frobnicate"], "unknown"),
        (&["mcp", "--headless", "--connect", "127.0.0.1:1"], "either"),
    ];
    for (args, expect) in cases {
        let out = cli(args, Some(b""));
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out.stderr));
        let stderr = text(&out.stderr);
        assert!(stderr.contains(expect), "{args:?}: {stderr}");
        assert!(!stderr.contains('\u{1b}') && !stderr.contains('\u{202e}'), "{args:?}: {stderr:?}");
        assert!(out.stdout.is_empty(), "{args:?}: printed on stdout: {}", text(&out.stdout));
    }
    // Without a subcommand: the usage, on stderr, and the same exit code.
    let out = cli(&[], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("Usage:") && out.stdout.is_empty());
}

#[test]
fn version_and_help_print_on_stdout_and_win_over_everything() {
    let version = format!("diacraft-cli {}\n", env!("CARGO_PKG_VERSION"));
    for args in [&["--version"][..], &["-V"], &["run", "--bogus", "--version"], &["mcp", "--headless", "--connect", "x", "--version"]] {
        let out = cli(args, None);
        assert!(out.status.success(), "{args:?}");
        assert_eq!(text(&out.stdout), version, "{args:?}");
    }
    for args in [&["--help"][..], &["-h"], &["run", "--bogus", "-h"], &["mcp", "--help"], &["commands", "--help"]] {
        let out = cli(args, None);
        assert!(out.status.success(), "{args:?}");
        let help = text(&out.stdout);
        for word in ["Usage:", "diacraft-cli mcp", "diacraft-cli run", "diacraft-cli commands", "--headless", "--connect", "--commands", "7997"] {
            assert!(help.contains(word), "{args:?}: no {word:?} in\n{help}");
        }
    }
}

#[test]
fn a_closed_stdout_ends_the_run_quietly() {
    let mut child =
        Command::new(BIN).args(["run", "--commands", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // The reader goes away at once, as in `diacraft-cli run ... | head -0`.
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    let step = b"{\"command\":\"app.version\"}\n".repeat(50_000);
    // The program stops reading when it ends; the broken pipe is not the test's concern.
    let _ = std::io::Write::write_all(&mut stdin, &step);
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "status {:?}: {}", out.status, text(&out.stderr));
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The AutoConnect commands run as steps. `connector.neighbour` says what a click would join, `connector.autoconnect` with `existing` joins it and makes no
/// shape (and with none makes a copy), with `to` joins a shape a drag was let go on, and `autoconnect.set` and `autoconnect.settings` hold the drawing's box.
#[test]
fn the_autoconnect_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":6.5,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":9.5,"y":7.0}}"#,
        r#"{"command":"connector.neighbour","params":{"from":1,"direction":"right"}}"#,
        r#"{"command":"connector.autoconnect","params":{"from":1,"direction":"right","existing":true}}"#,
        r#"{"command":"connector.autoconnect","params":{"from":2,"direction":"up","to":3}}"#,
        r#"{"command":"connector.autoconnect","params":{"from":3,"direction":"down","existing":true}}"#,
        r#"{"command":"autoconnect.settings"}"#,
        r#"{"command":"autoconnect.set","params":{"on":false}}"#,
        r#"{"command":"autoconnect.settings"}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 11, "{lines:?}");
    assert_eq!(lines[4]["result"]["neighbour"], 2, "the nearest shape on the right of shape 1");
    assert_eq!(lines[5]["result"]["created"], false, "a click joined shape 2: {}", lines[5]);
    assert_eq!(lines[5]["result"]["id"], 2);
    assert_eq!(lines[6]["result"]["created"], false, "a drag onto shape 3 joined it");
    assert_eq!(lines[6]["result"]["id"], 3);
    assert_eq!(lines[7]["result"]["created"], true, "shape 3 has nothing below it: a copy");
    assert_eq!(lines[8]["result"]["on"], true);
    assert_eq!(lines[9]["result"]["on"], false);
    assert_eq!(lines[10]["result"]["held"], false);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// `stencil.search` runs as a step, with no drawing open. A word is a whole word of a master's name or of its search words, all of them by default and any
/// one with `mode: "any"`, and a query that is not one is refused in words and stops the run.
#[test]
fn stencil_search_runs_as_a_step() {
    let steps = [
        r#"{"command":"stencil.search","params":{"text":"Rhombus"}}"#,
        r#"{"command":"stencil.search","params":{"text":"arrow decision","mode":"any"}}"#,
        r#"{"command":"stencil.search","params":{"text":"arrow decision"}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 3, "{lines:?}");
    let first = &lines[0]["result"];
    assert_eq!(first["mode"], "all");
    assert_eq!(first["results"][0]["name"], "Basic Shapes", "{first}");
    assert_eq!(first["results"][0]["masters"][0]["name"], "Diamond", "a rhombus is a diamond, by the search words");
    let any = &lines[1]["result"];
    assert!(any["matches"].as_u64().unwrap() > first["matches"].as_u64().unwrap(), "any word finds more than one: {any}");
    assert_eq!(lines[2]["result"]["matches"], 0, "no master is both an arrow and a decision");
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
    // A query that is not one is refused in words and the run stops there.
    let out = cli(&["run", "--cmd", "stencil.search", "--params", r#"{"text":"!!"}"#], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("step 0 (stencil.search)") && text(&out.stderr).contains("has no words in it"), "{}", text(&out.stderr));
}

/// The commands that edit a connector's path run as steps. `connector.edit_path` moves a leg (with `snap` it says the line it went to), `connector.reroute` sets when
/// the connector may take a new path, and `connector.reset` takes the path back to the routed one and the setting to freely.
#[test]
fn the_connector_path_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":9.0,"y":6.0}}"#,
        r#"{"command":"connector.add","params":{"from":{"id":1,"point":3},"to":{"id":2,"point":1}}}"#,
        r#"{"command":"connector.edit_path","params":{"id":3,"leg":1,"to":7.04,"snap":{"zoom":96}}}"#,
        r#"{"command":"connector.reroute","params":{"ids":[3],"mode":"never"}}"#,
        r#"{"command":"connector.reroute","params":{"ids":[3],"mode":"never"}}"#,
        r#"{"command":"connector.reset","params":{"ids":[3]}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 8, "{lines:?}");
    assert_eq!(lines[4]["result"]["changed"], true, "{}", lines[4]);
    assert_eq!(lines[4]["result"]["to"], 7.0, "the grid pulled the leg onto the line at 7: {}", lines[4]);
    assert_eq!(lines[5]["result"], serde_json::json!({"page": 1, "mode": "never", "changed": 1}));
    assert_eq!(lines[6]["result"]["changed"], 0, "it was set already");
    assert_eq!(lines[7]["result"]["reset"], 1, "{}", lines[7]);
    // A mode that is not one is refused in words, and the run stops there.
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "connector.reroute", "--params", r#"{"ids":[1],"mode":"sometimes"}"#], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 1 (connector.reroute)") && stderr.contains("freely, as_needed, never or on_crossover"), "{stderr}");
}

/// The selection queries run as steps and answer the `ID`s a window would select. `select.choices` lists what the Select by Type dialog offers, `select.by_type` takes
/// the shapes of a role or a type, `select.by_region` takes a net or a loop wholly inside or, with `partial`, touched, and none of them is an undo step.
#[test]
fn the_selection_queries_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":6.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":7.0,"y":6.0}}"#,
        r#"{"command":"connector.add","params":{"from":{"id":1},"to":{"id":2}}}"#,
        r#"{"command":"select.choices"}"#,
        r#"{"command":"select.by_type","params":{"roles":["connector"]}}"#,
        r#"{"command":"select.by_type","params":{"types":["shape"]}}"#,
        r#"{"command":"select.by_region","params":{"rect":[1.0,5.0,5.0,7.0]}}"#,
        r#"{"command":"select.by_region","params":{"rect":[1.0,5.0,5.0,7.0],"partial":true}}"#,
        r#"{"command":"select.by_region","params":{"polygon":[[0.0,4.0],[8.0,4.0],[0.0,9.0]]}}"#,
        r#"{"command":"document.inspect"}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 11, "{lines:?}");
    assert_eq!(lines[4]["result"]["roles"][0], serde_json::json!({"word": "connector", "label": "Connectors", "count": 1}));
    assert_eq!(lines[5]["result"]["ids"], serde_json::json!([3]), "the connector");
    assert_eq!(lines[6]["result"]["ids"], serde_json::json!([1, 2, 3]), "a connector is a shape too");
    assert_eq!(lines[7]["result"]["ids"], serde_json::json!([1]), "the net takes the first rectangle wholly: the connector runs out of it");
    assert_eq!(lines[8]["result"]["ids"], serde_json::json!([1, 3]), "and with partial the connector it crosses too, not the second rectangle");
    assert_eq!(lines[9]["result"]["ids"], serde_json::json!([1]), "a loop is a polygon");
    assert_eq!(lines[11 - 1]["result"]["history"]["undo"], 3, "the three queries made no undo step: only the two drops and the connector did");
    // A call that asks for nothing is refused in words, and the run stops there.
    let out = cli(&["run", "--cmd", "file.new", "--cmd", "select.by_type", "--params", r#"{"types":["triangle"]}"#], None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 1 (select.by_type)") && stderr.contains("no shape type `triangle`"), "{stderr}");
}

/// The layer commands run as steps. A layer is added, a shape put on it, the layer locked and hidden, listed, and removed with its shapes.
#[test]
fn the_layer_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r##"{"command":"layer.add","params":{"name":"Notes","color":"#ff0000"}}"##,
        r#"{"command":"layer.assign","params":{"ids":[1],"layers":["Notes"]}}"#,
        r#"{"command":"layer.set","params":{"layer":"Notes","visible":false,"lock":true}}"#,
        r#"{"command":"layer.list"}"#,
        r#"{"command":"layer.delete","params":{"layer":0,"shapes":"delete"}}"#,
        r#"{"command":"layer.list"}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 8, "{lines:?}");
    assert_eq!(lines[2]["result"]["id"], 0);
    assert_eq!(lines[3]["result"]["written"], 1);
    let listed = &lines[5]["result"]["layers"][0];
    assert_eq!(
        (listed["name"].clone(), listed["visible"].clone(), listed["lock"].clone(), listed["shapes"].clone()),
        (json!("Notes"), json!(false), json!(true), json!(1))
    );
    assert_eq!(listed["color"], "#ff0000");
    assert_eq!(lines[6]["result"]["shapes_deleted"], 1);
    assert_eq!(lines[7]["result"]["layers"], json!([]));
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The layout commands run as steps. `page.relayout` lays three joined shapes out down the page and writes the choice into the page, `page.layout` reads it back,
/// `shape.autospace` and `page.spacing` set the gaps, `page.breaks` says where the page passes the paper, `page.avoid_page_breaks` and `shape.move_off_page_breaks` keep a shape off a
/// break, and `page_breaks.set` turns the box on.
#[test]
fn the_relayout_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":2.0,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":5.0,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":8.0,"y":4.0}}"#,
        r#"{"command":"connector.add","params":{"from":{"id":1,"point":3},"to":{"id":2,"point":1}}}"#,
        r#"{"command":"connector.add","params":{"from":{"id":2,"point":3},"to":{"id":3,"point":1}}}"#,
        r#"{"command":"page.relayout","params":{"style":"hierarchy","direction":"top_to_bottom","spacing":1.0}}"#,
        r#"{"command":"page.layout"}"#,
        r#"{"command":"shape.autospace","params":{"vertical":0.5}}"#,
        r#"{"command":"page.spacing","params":{"horizontal":2.0,"vertical":0.75}}"#,
        r#"{"command":"page.size","params":{"width":20.0,"height":7.5}}"#,
        r#"{"command":"page.breaks"}"#,
        r#"{"command":"page.avoid_page_breaks","params":{"on":true}}"#,
        r#"{"command":"shape.move_off_page_breaks"}"#,
        r#"{"command":"page_breaks.set","params":{"on":true}}"#,
        r#"{"command":"page_breaks.settings"}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 16, "{lines:?}");
    let laid = &lines[6]["result"];
    assert_eq!((laid["nodes"].as_u64(), laid["joins"].as_u64(), laid["place_style"].as_u64()), (Some(3), Some(2), Some(17)), "{laid}");
    let layout = &lines[7]["result"];
    assert_eq!(
        (layout["style"].as_str(), layout["direction"].as_str(), layout["alignment"].as_str()),
        (Some("hierarchy"), Some("top_to_bottom"), Some("center")),
        "{layout}"
    );
    assert_eq!(layout["spacing"]["x"], 1.0);
    assert_eq!(lines[8]["result"]["rows"], 3, "three rows, one under another");
    assert_eq!((lines[9]["result"]["horizontal"].as_f64(), lines[9]["result"]["vertical"].as_f64()), (Some(2.0), Some(0.75)));
    assert_eq!(lines[11]["result"]["x"].as_array().map(Vec::len), Some(1), "two sheets across: one break");
    assert_eq!(lines[12]["result"]["on"], true);
    assert_eq!(lines[13]["result"]["shapes"], 3);
    assert_eq!(lines[14]["result"]["on"], true);
    assert_eq!(lines[15]["result"]["on"], true);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The org chart commands run as steps. A manager dropped on an executive is linked and the chart laid out (`shape.drop` says `reports_to`), `orgchart.info` tells the chart, `orgchart.layout`
/// and `orgchart.arrange` write the choices into the cells, `orgchart.relayout` keeps them, `orgchart.link` moves a position to another manager, and a reporting loop is refused in words.
#[test]
fn the_org_chart_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new","params":{"template":"organization_chart"}}"#,
        r#"{"command":"orgchart.info"}"#,
        r#"{"command":"shape.drop","params":{"master":"orgchart.manager","page":1,"x":5.5,"y":7.5}}"#,
        r#"{"command":"shape.drop","params":{"master":"orgchart.manager","page":1,"x":5.5,"y":7.5}}"#,
        r#"{"command":"orgchart.info","params":{"ids":[1,2,4]}}"#,
        r#"{"command":"orgchart.layout","params":{"layout":"left_to_right","spacing":0.5}}"#,
        r#"{"command":"orgchart.arrange","params":{"id":1,"arrangement":"vertical_right"}}"#,
        r#"{"command":"orgchart.relayout"}"#,
        r#"{"command":"page.layout"}"#,
        r#"{"command":"orgchart.link","params":{"id":4,"manager":2}}"#,
        r#"{"command":"orgchart.link","params":{"id":2,"manager":4}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    let lines = json_lines(&out.stdout);
    // The last step is refused in words (a loop), so the run ends there.
    assert!(!out.status.success(), "the loop is refused");
    assert!(text(&out.stderr).contains("loop"), "stderr: {}", text(&out.stderr));
    assert_eq!(lines.len(), 10, "{lines:?}");
    assert_eq!(lines[1]["result"]["positions"], 1, "the template starts with an Executive");
    assert_eq!(lines[3]["result"]["reports_to"], 1, "{}", lines[3]);
    let info = &lines[4]["result"];
    assert_eq!((info["positions"].as_u64(), info["links"].as_u64()), (Some(3), Some(2)), "{info}");
    assert_eq!(lines[5]["result"]["layout"], "left_to_right");
    assert_eq!(lines[6]["result"]["arrangement"], "vertical_right");
    assert_eq!(lines[7]["result"]["positions"], 3);
    assert_eq!(lines[8]["result"]["place_style"], 23, "Re-Layout kept the page's layout");
    assert_eq!(lines[9]["result"]["manager"], 2, "{}", lines[9]);
}

/// The callout commands run as steps. `callout.styles` lists the gallery, `callout.insert` adds a callout tied to a shape (or one tied to nothing at a place), `callout.list` says the
/// ties both ways and where the end of the leader is, `shape.move` takes the callout along with its target, `callout.detach` and `callout.attach` untie and tie, and a shape deleted takes its callouts.
#[test]
fn the_callout_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"callout.styles"}"#,
        r#"{"command":"callout.insert","params":{"ids":[1],"style":"speech_box","text":"Look"}}"#,
        r#"{"command":"callout.list","params":{"id":1}}"#,
        r#"{"command":"callout.list","params":{"id":2}}"#,
        r#"{"command":"shape.move","params":{"ids":[1],"dx":1.0,"dy":0.5}}"#,
        r#"{"command":"callout.list","params":{"id":2}}"#,
        r#"{"command":"callout.detach","params":{"id":2,"x":8.0,"y":9.0}}"#,
        r#"{"command":"callout.list","params":{"id":1}}"#,
        r#"{"command":"callout.attach","params":{"id":2,"target":1}}"#,
        r#"{"command":"shape.delete","params":{"ids":[1]}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 12, "{lines:?}");
    assert_eq!(lines[2]["result"]["styles"].as_array().map(Vec::len), Some(14));
    assert_eq!(lines[3]["result"]["ids"], serde_json::json!([2]));
    assert_eq!(lines[3]["result"]["targets"], serde_json::json!([1]));
    assert_eq!(lines[4]["result"]["callouts"], serde_json::json!([2]), "the shape names its callout");
    assert_eq!(lines[5]["result"]["target"], 1, "and the callout its shape");
    assert_eq!(lines[5]["result"]["style"], "speech_box");
    let (before, after) = (lines[5]["result"]["tip"].clone(), lines[7]["result"]["tip"].clone());
    assert!(
        (after[0].as_f64().unwrap() - before[0].as_f64().unwrap() - 1.0).abs() < 1e-6,
        "the end of the leader went with the shape: {before} -> {after}"
    );
    assert!((after[1].as_f64().unwrap() - before[1].as_f64().unwrap() - 0.5).abs() < 1e-6, "{before} -> {after}");
    assert_eq!(lines[8]["result"]["changed"], true);
    assert_eq!(lines[9]["result"]["callouts"], serde_json::json!([]), "untied: the shape names none");
    assert_eq!(lines[10]["result"]["changed"], true);
    assert_eq!(lines[11]["result"]["deleted"], 1);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
    // The callout went with its shape: asking after it is refused in words, and the run stops there.
    let out = cli(
        &[
            "run",
            "--cmd",
            "file.new",
            "--cmd",
            "shape.drop",
            "--params",
            r#"{"master":"rectangle","page":1,"x":3.0,"y":4.0}"#,
            "--cmd",
            "callout.insert",
            "--params",
            r#"{"ids":[1]}"#,
            "--cmd",
            "shape.delete",
            "--params",
            r#"{"ids":[1]}"#,
            "--cmd",
            "callout.list",
            "--params",
            r#"{"id":2}"#,
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 4 (callout.list)") && stderr.contains("no shape 2"), "{stderr}");
}

/// `shape.change` runs as a step. A shape with text and the connector glued to it are selected, the shape is changed to a decision and the connector is listed as left, and
/// a change that has nothing to change is refused in words and stops the run.
#[test]
fn shape_change_runs_as_a_step() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"process","page":1,"x":3.0,"y":3.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":7.0,"y":3.0}}"#,
        r#"{"command":"connector.add","params":{"from":{"id":1},"to":{"id":2}}}"#,
        r#"{"command":"shape.set_text","params":{"id":1,"text":"Check"}}"#,
        r#"{"command":"shape.change","params":{"ids":[1,3],"master":"decision"}}"#,
        r#"{"command":"shape.change","params":{"ids":[3],"master":"decision"}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    // The last change names only the connector, which a decision cannot replace: that step is refused in words and the run stops there.
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 6 (shape.change)") && stderr.contains("one-dimensional"), "{stderr}");
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 6, "{lines:?}");
    let answer = &lines[5]["result"];
    assert_eq!(answer["changed"], json!([1]), "{answer}");
    assert_eq!(answer["name"], "Decision");
    assert_eq!(answer["skipped"][0]["id"], 3, "{answer}");
    assert!(answer["skipped"][0]["why"].as_str().unwrap().contains("one-dimensional"), "{answer}");
}

/// The container commands run as steps. A container is put round two shapes, its settings changed, a shape dropped inside joins it, it is locked, listed, its
/// contents selected, and it is disbanded (the shapes stay) or deleted (they go).
#[test]
fn the_container_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":6.0,"y":4.0}}"#,
        r#"{"command":"container.add","params":{"ids":[1,2],"heading":"Stage","style":"tint-strip"}}"#,
        r#"{"command":"container.margin","params":{"ids":[3],"margin":"wide"}}"#,
        r#"{"command":"container.heading","params":{"ids":[3],"rotate":true}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":4.5,"y":4.0}}"#,
        r#"{"command":"container.lock","params":{"ids":[3]}}"#,
        r#"{"command":"container.info","params":{"ids":[3]}}"#,
        r#"{"command":"container.select_contents","params":{"ids":[3]}}"#,
        r#"{"command":"container.lock","params":{"ids":[3],"locked":false}}"#,
        r#"{"command":"container.disband","params":{"ids":[3]}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 12, "{lines:?}");
    assert_eq!(lines[3]["result"]["id"], 3);
    assert_eq!(lines[3]["result"]["members"], json!([1, 2]));
    assert_eq!(lines[7]["result"]["locked"], true);
    let listed = &lines[8]["result"]["containers"][0];
    assert_eq!(
        (listed["style"].clone(), listed["heading"].clone(), listed["locked"].clone(), listed["members"].clone()),
        (json!("tint-strip"), json!("right"), json!(true), json!([1, 2, 4])),
        "the shape dropped inside joined it"
    );
    assert_eq!(lines[9]["result"]["ids"], json!([1, 2, 4]));
    assert_eq!(lines[11]["result"]["disbanded"], 1);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The swimlane commands run as steps. A structure of two lanes and two phases is made, a shape dropped in the second lane, a lane and a separator inserted, a lane moved and made
/// thicker, the structure turned and a label rotated, and the page's structures listed.
#[test]
fn the_swimlane_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"swimlane.create","params":{"x":1.0,"y":8.0,"length":6.0,"thickness":1.5,"lanes":2,"phases":2}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":4.0,"y":5.75}}"#,
        r#"{"command":"swimlane.insert","params":{"lane":3}}"#,
        r#"{"command":"swimlane.insert_phase","params":{"phase":6}}"#,
        r#"{"command":"swimlane.move","params":{"lane":4,"to":0}}"#,
        r#"{"command":"swimlane.resize","params":{"id":3,"size":2.0}}"#,
        r#"{"command":"swimlane.orientation","params":{"orientation":"vertical"}}"#,
        r#"{"command":"swimlane.rotate_label","params":{"ids":[3]}}"#,
        r#"{"command":"swimlane.info","params":{"ids":[8]}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 10, "{lines:?}");
    assert_eq!(lines[1]["result"]["frame"], 1);
    assert_eq!(lines[1]["result"]["lanes"], json!([3, 4]));
    assert_eq!(lines[3]["result"]["index"], 1);
    assert_eq!(lines[4]["result"]["index"], 1);
    assert_eq!(lines[5]["result"]["index"], 0);
    let info = &lines[9]["result"];
    assert_eq!(info["structures"][0]["orientation"], "vertical");
    assert_eq!(info["structures"][0]["lanes"].as_array().unwrap().len(), 3);
    assert_eq!(info["structures"][0]["phases"].as_array().unwrap().len(), 3);
    assert_eq!(info["selected"], json!([]), "a shape that is no part of the structure is not listed: {info}");
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The Shape Data commands run as steps. The types are listed, a shape is given fields of three types and one hidden, a value is entered (a refusal stops the run in words), the list
/// is read with and without the hidden field, and one undo takes the entry back.
#[test]
fn the_shape_data_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"shapedata.types"}"#,
        r#"{"command":"shapedata.define","params":{"id":1,"fields":[{"label":"Owner","name":"Owner","sort_key":"2"},{"label":"Qty","name":"Qty","type":"number","format":"0.00","sort_key":"1"},{"label":"Code","name":"Code","invisible":true}]}}"#,
        r#"{"command":"shapedata.set","params":{"id":1,"values":[{"row":"Owner","value":"Ann"},{"row":"Qty","value":"12.5"}]}}"#,
        r#"{"command":"shapedata.list","params":{"id":1}}"#,
        r#"{"command":"shapedata.list","params":{"id":1,"hidden":true}}"#,
        r#"{"command":"edit.undo"}"#,
        r#"{"command":"shapedata.list","params":{"id":1}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 9, "{lines:?}");
    assert_eq!(lines[2]["result"]["types"].as_array().unwrap().len(), 8);
    assert_eq!(lines[3]["result"]["fields"].as_array().unwrap().len(), 3, "define answers every field, the hidden one too");
    assert_eq!(lines[4]["result"]["values"], 2);
    let listed: Vec<(&str, &str)> =
        lines[5]["result"]["fields"].as_array().unwrap().iter().map(|f| (f["row"].as_str().unwrap(), f["shown"].as_str().unwrap())).collect();
    assert_eq!(listed, [("Qty", "12.50"), ("Owner", "Ann")], "by sort key, the hidden field left out");
    assert_eq!(lines[5]["result"]["hidden"], 1);
    assert_eq!(lines[6]["result"]["fields"].as_array().unwrap().len(), 3);
    assert_eq!(lines[8]["result"]["fields"][0]["shown"], "", "one undo took both values back");
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
    // A value that is no value of its type stops the run, naming the step and the words.
    let bad = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"shapedata.define","params":{"id":1,"fields":[{"label":"Qty","name":"Qty","type":"number"}]}}"#,
        r#"{"command":"shapedata.set","params":{"id":1,"row":"Qty","value":"many"}}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(bad.as_bytes()));
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("step 3 (shapedata.set)") && stderr.contains("is not a number"), "{stderr}");
}

/// The format commands run as steps. A shape is given a transparency, a pattern and a cap by `shape.format`, a gradient, a shadow and (on a page that has a theme) a Quick
/// Style by their own commands, and `shape.formatting` and `shape.quick_styles` read it all back.
#[test]
fn the_format_effect_commands_run_as_steps() {
    let steps = [
        r##"{"command":"file.new"}"##,
        r##"{"command":"theme.apply","params":{"theme":"tidepool"}}"##,
        r##"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"##,
        r##"{"command":"shape.format","params":{"ids":[1],"fill":"#336699","fill_transparency":35,"line_cap":"square","compound":"double"}}"##,
        r##"{"command":"shape.gradient","params":{"ids":[1],"angle":45,"stops":[{"position":0,"color":"#FF0000"},{"position":1,"color":"#0000FF","transparency":20}]}}"##,
        r##"{"command":"shape.shadow","params":{"ids":[1],"preset":"bottom_right","blur":6}}"##,
        r##"{"command":"shape.formatting","params":{"ids":[1]}}"##,
        r##"{"command":"shape.quick_styles","params":{"ids":[1]}}"##,
        r##"{"command":"shape.quick_style","params":{"ids":[1],"row":2,"column":3}}"##,
        r##"{"command":"shape.formatting","params":{"ids":[1]}}"##,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 10, "{lines:?}");
    assert_eq!(lines[4]["result"]["target"], "fill");
    assert_eq!(lines[5]["result"]["formatted"], 1);
    let look = &lines[6]["result"]["shapes"][0];
    assert_eq!(look["fill_transparency"], 35.0);
    assert_eq!(look["line_cap"], "square");
    assert_eq!(look["compound"], "double");
    assert_eq!(look["gradient"]["angle"], 45.0);
    assert_eq!(look["gradient"]["stops"][1]["transparency"], 20.0);
    assert_eq!((look["shadow"]["angle"].clone(), look["shadow"]["blur"].clone()), (json!(45.0), json!(6.0)));
    assert_eq!(lines[7]["result"]["variants"].as_array().map(Vec::len), Some(4));
    assert_eq!(lines[7]["result"]["styles"].as_array().map(Vec::len), Some(42));
    assert_eq!(
        (lines[8]["result"]["matrix"].clone(), lines[8]["result"]["colour"].clone()),
        (json!(2), json!(102)),
        "row 2, column 3 is entry 2 in the third colour of the variant"
    );
    assert_ne!(lines[9]["result"]["shapes"][0]["fill"], look["fill"], "the style put the theme's colour in place of the one set");
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The palette and the preview run as steps. A page has a theme, `theme.colors` tells its palette, a shape is filled with a theme colour, a variant is
/// previewed on a copy (nothing changes), and the fill follows the theme applied after it.
#[test]
fn the_theme_colour_commands_run_as_steps() {
    let steps = [
        r##"{"command":"file.new"}"##,
        r##"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"##,
        r##"{"command":"theme.apply","params":{"theme":"tidepool"}}"##,
        r##"{"command":"theme.colors"}"##,
        r##"{"command":"shape.format","params":{"ids":[1],"fill":"theme:accent2:40","line":"theme:dark"}}"##,
        r##"{"command":"shape.formatting","params":{"ids":[1]}}"##,
        r##"{"command":"theme.preview","params":{"theme":"cinderglow","ids":[1]}}"##,
        r##"{"command":"shape.formatting","params":{"ids":[1]}}"##,
        r##"{"command":"theme.apply","params":{"theme":"cinderglow"}}"##,
        r##"{"command":"shape.formatting","params":{"ids":[1]}}"##,
        r##"{"command":"shape.format","params":{"ids":[1],"fill":"default"}}"##,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 11, "{lines:?}");
    let palette = &lines[3]["result"];
    assert_eq!((palette["themed"].clone(), palette["theme_colors"].as_array().map(Vec::len)), (json!(true), Some(10)));
    assert_eq!(palette["variant_colors"].as_array().map(Vec::len), Some(7));
    assert_eq!(palette["standard_colors"].as_array().map(Vec::len), Some(10));
    let swatch = &palette["theme_colors"][5]["swatches"][3];
    assert_eq!((swatch["pick"].clone(), swatch["tint"].clone()), (json!("theme:accent2:40"), json!(40)));
    let fill = lines[5]["result"]["shapes"][0]["fill"].clone();
    assert_eq!(fill, swatch["hex"], "the shape has the colour the palette showed");
    // The preview is of a copy: the drawing's own fill is as it was, and the preview's is the other theme's.
    assert_eq!(lines[6]["result"]["preview"], true);
    assert_ne!(lines[6]["result"]["shapes"][0]["fill"], fill);
    assert_eq!(lines[7]["result"]["shapes"][0]["fill"], fill);
    // Applied for real, the fill follows the theme, and it is what the preview showed.
    assert_eq!(lines[9]["result"]["shapes"][0]["fill"], lines[6]["result"]["shapes"][0]["fill"]);
    assert_eq!(lines[10]["result"]["formatted"], 1);
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}

/// The comment commands run as steps. A comment is added to a shape, answered, edited, listed (with where its marker is), stepped to, hidden by Show Markup, and deleted with its reply.
#[test]
fn the_comment_commands_run_as_steps() {
    let steps = [
        r#"{"command":"file.new"}"#,
        r#"{"command":"comment.author","params":{"name":"Ada Lovelace","initials":"AL"}}"#,
        r#"{"command":"shape.drop","params":{"master":"rectangle","page":1,"x":3.0,"y":4.0}}"#,
        r#"{"command":"comment.add","params":{"id":1,"text":"Check this size"}}"#,
        r#"{"command":"comment.reply","params":{"comment":1,"text":"Done"}}"#,
        r#"{"command":"comment.edit","params":{"comment":1,"text":"Check this size twice"}}"#,
        r#"{"command":"comment.list","params":{"details":true}}"#,
        r#"{"command":"comment.next"}"#,
        r#"{"command":"comment.markers","params":{"show":false}}"#,
        r#"{"command":"comment.delete","params":{"comment":1}}"#,
        r#"{"command":"comment.list"}"#,
    ]
    .join("\n");
    let out = cli(&["run", "--commands", "-"], Some(steps.as_bytes()));
    assert!(out.status.success(), "{}", text(&out.stderr));
    let lines = json_lines(&out.stdout);
    assert_eq!(lines.len(), 11, "{lines:?}");
    assert_eq!(lines[3]["result"]["comment"]["shape"], 1);
    assert_eq!(lines[3]["result"]["comment"]["author"], "Ada Lovelace");
    assert_eq!(lines[4]["result"]["comment"]["parent"], 1);
    assert_eq!(lines[5]["result"]["comment"]["text"], "Check this size twice");
    let listed = lines[6]["result"]["comments"].as_array().unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed[0]["anchor"]["x"].as_f64().unwrap() > 0.0, "the marker has a place");
    assert_eq!(lines[7]["result"]["comment"]["id"], 1);
    assert_eq!(lines[8]["result"]["markers"], false);
    assert_eq!(lines[9]["result"]["deleted"], 2);
    assert_eq!(lines[10]["result"]["comments"], serde_json::json!([]));
    assert!(text(&out.stderr).is_empty(), "stderr: {}", text(&out.stderr));
}
