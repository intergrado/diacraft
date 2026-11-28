//! `diacraft-cli mcp --headless --automation-read-root DIR --automation-write-root DIR`: the shipped binary on a real pipe, kept to the folders it was given (gaps A2 and A5).
//! What the library tests cannot show: the flags and the variables are read, the roots are in force in the whole process, a refusal reaches the client in words, nothing outside
//! is written, and a folder that is not there is a mistake in the command line that stops the program before it serves.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::{BIN, TempDir, cli, json_lines, s, text};

/// `read`, `write` and `outside` folders, each with a drawing (made by the program's own `run`).
struct World {
    dir: TempDir,
}

impl World {
    fn new() -> World {
        let dir = TempDir::new("roots");
        for folder in ["read", "write", "outside"] {
            std::fs::create_dir_all(dir.path(folder)).unwrap();
            let target = dir.path(folder).join("drawing.diacraft");
            let params = json!({"path": s(&target)}).to_string();
            let made = cli(&["run", "--cmd", "file.new", "--cmd", "file.save_as", "--params", &params], None);
            assert_eq!(made.status.code(), Some(0), "{}", text(&made.stderr));
        }
        World { dir }
    }

    fn p(&self, relative: &str) -> String {
        s(&self.dir.path(relative)).to_owned()
    }

    fn flags(&self) -> Vec<String> {
        vec!["mcp".into(), "--headless".into(), "--automation-read-root".into(), self.p("read"), "--automation-write-root".into(), self.p("write")]
    }
}

fn request(id: u64, tool: &str, arguments: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}))
}

fn run_command(id: u64, command: &str, params: Value) -> String {
    request(id, "command_run", json!({"id": command, "params": params}))
}

/// Runs the program with `args` and `env`, gives it `input`, and waits.
fn serve(args: &[String], env: &[(&str, &str)], input: &str) -> std::process::Output {
    let mut command = Command::new(BIN);
    command.args(args).env_remove("RUST_LOG").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A test never reads or writes the person's settings: `run` and `mcp` open them unless this is set.
    command.env("DIACRAFT_NO_PREFS", "1");
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // The program may refuse and exit before it reads a byte: the test's assertions on its exit code say whether that was right.
    let _ = stdin.write_all(input.as_bytes());
    drop(stdin);
    child.wait_with_output().unwrap()
}

/// The text of a tool result.
fn tool_text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap().to_owned()
}

#[test]
fn the_shipped_server_keeps_files_to_the_folders_it_was_given() {
    let w = World::new();
    let before = std::fs::read(w.dir.path("outside").join("drawing.diacraft")).unwrap();
    let input = [
        run_command(1, "file.open", json!({"path": w.p("read/drawing.diacraft")})),
        run_command(2, "file.open", json!({"path": w.p("outside/drawing.diacraft")})),
        run_command(3, "file.save_as", json!({"path": w.p("write/new.vsdx")})),
        run_command(4, "file.save_as", json!({"path": w.p("outside/new.vsdx")})),
        run_command(5, "file.save_as", json!({"path": w.p("outside/drawing.diacraft")})),
        run_command(6, "file.export", json!({"path": w.p("outside/p.png")})),
        run_command(7, "file.export", json!({"path": w.p("write/p.png")})),
    ]
    .concat();
    let out = serve(&w.flags(), &[], &input);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    let refused = |n: usize| {
        assert_eq!(replies[n]["result"]["isError"], true, "{}", replies[n]);
        let words = tool_text(&replies[n]);
        assert!(words.contains("automation") && words.contains("--automation-"), "{words}");
        assert!(!words.contains(&*w.p("")), "holds the path: {words}");
    };
    let worked = |n: usize| assert_eq!(replies[n]["result"]["isError"], false, "{}", replies[n]);
    worked(0);
    refused(1);
    worked(2);
    refused(3);
    refused(4);
    refused(5);
    worked(6);
    assert!(w.dir.path("write").join("new.vsdx").exists() && w.dir.path("write").join("p.png").exists());
    assert!(!w.dir.path("outside").join("new.vsdx").exists() && !w.dir.path("outside").join("p.png").exists(), "nothing was written outside");
    assert_eq!(std::fs::read(w.dir.path("outside").join("drawing.diacraft")).unwrap(), before, "the file outside is as it was");
    // It says on stderr what it is kept to, and the client is told too.
    let stderr = text(&out.stderr);
    assert!(stderr.contains("read ") && stderr.contains(&w.p("read")) && stderr.contains(&w.p("write")), "{stderr}");
}

#[test]
fn the_server_says_to_the_client_which_folders_it_is_kept_to() {
    let w = World::new();
    let input = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}})
    );
    let out = serve(&w.flags(), &[], &input);
    let words = json_lines(&out.stdout)[0]["result"]["instructions"].as_str().unwrap().to_owned();
    assert!(words.contains(&w.p("read")) && words.contains(&w.p("write")), "{words}");
    // With no flag it says its files are not limited.
    let out = serve(&["mcp".into(), "--headless".into()], &[], &input);
    let words = json_lines(&out.stdout)[0]["result"]["instructions"].as_str().unwrap().to_owned();
    assert!(words.contains("not limited"), "{words}");
}

#[test]
fn with_no_flag_and_no_variable_the_server_reads_and_writes_any_path_as_it_did() {
    let w = World::new();
    let input = [
        run_command(1, "file.open", json!({"path": w.p("outside/drawing.diacraft")})),
        run_command(2, "file.save_as", json!({"path": w.p("outside/again.diacraft")})),
    ]
    .concat();
    let out = serve(&["mcp".into(), "--headless".into()], &[], &input);
    let replies = json_lines(&out.stdout);
    assert!(replies.iter().all(|r| r["result"]["isError"] == false), "{replies:?}");
    assert!(w.dir.path("outside").join("again.diacraft").exists());
}

#[test]
fn the_variables_give_the_roots_when_the_flags_do_not_and_a_flag_wins_for_its_kind() {
    let w = World::new();
    let input = [
        run_command(1, "file.open", json!({"path": w.p("read/drawing.diacraft")})),
        run_command(2, "file.open", json!({"path": w.p("outside/drawing.diacraft")})),
    ]
    .concat();
    let env = [("DIACRAFT_AUTOMATION_READ_ROOT", w.p("read")), ("DIACRAFT_AUTOMATION_WRITE_ROOT", w.p("write"))];
    let env: Vec<(&str, &str)> = env.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let out = serve(&["mcp".into(), "--headless".into()], &env, &input);
    let replies = json_lines(&out.stdout);
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    assert_eq!(replies[1]["result"]["isError"], true, "{}", replies[1]);
    // A flag for reading wins over the variable for reading: `outside` is now the read root, and `read` is not.
    let mut args = vec!["mcp".to_owned(), "--headless".to_owned(), "--automation-read-root".to_owned(), w.p("outside")];
    let out = serve(&args, &env, &input);
    let replies = json_lines(&out.stdout);
    assert_eq!((replies[0]["result"]["isError"].clone(), replies[1]["result"]["isError"].clone()), (json!(true), json!(false)), "{replies:?}");
    args.clear();
}

#[test]
fn a_root_that_is_not_there_is_a_mistake_in_the_command_line() {
    let w = World::new();
    let missing = w.p("nowhere");
    let out =
        serve(&["mcp".into(), "--headless".into(), "--automation-read-root".into(), missing.clone()], &[], &run_command(1, "file.new", json!({})));
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "nothing was served: {}", text(&out.stdout));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("cannot use the automation read root"), "{stderr}");
    // A file is no folder, and the same goes for a root from the variables.
    let file = w.p("read/drawing.diacraft");
    let out = serve(&["mcp".into(), "--headless".into()], &[("DIACRAFT_AUTOMATION_WRITE_ROOT", file.as_str())], "");
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("not a folder"), "{}", text(&out.stderr));
}

#[test]
fn roots_cannot_keep_an_app_that_is_not_this_process() {
    let w = World::new();
    // `--connect` names the running app, which keeps to its own roots: asking this process to limit it would be asking for what it cannot do.
    let out = serve(&["mcp".into(), "--connect".into(), "127.0.0.1:1".into(), "--automation-read-root".into(), w.p("read")], &[], "");
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("--connect") && stderr.contains("--automation-read-root") && stderr.contains("--control"), "{stderr}");
}

#[test]
fn a_hostile_path_is_one_refusal_and_the_server_goes_on() {
    let w = World::new();
    let long = format!("{}/{}", w.p("read"), "a".repeat(64 * 1024));
    let input = [
        run_command(1, "file.open", json!({"path": long})),
        run_command(2, "file.open", json!({"path": format!("{}/{}drawing.diacraft", w.p("read"), "../read/".repeat(400))})),
        run_command(3, "file.open", json!({"path": format!("{}/{}../outside/drawing.diacraft", w.p("read"), "../read/".repeat(400))})),
        run_command(4, "file.open", json!({"path": w.p("read/drawing.diacraft")})),
    ]
    .concat();
    let out = serve(&w.flags(), &[], &input);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    assert_eq!(replies.len(), 4, "{replies:?}");
    assert_eq!(replies[0]["result"]["isError"], true);
    assert!(tool_text(&replies[0]).contains("automation") && tool_text(&replies[0]).len() < 1000);
    assert_eq!(replies[2]["result"]["isError"], true, "{}", replies[2]);
    assert_eq!(replies[3]["result"]["isError"], false, "{}", replies[3]);
}

#[test]
fn the_help_says_what_the_flags_are() {
    let out = cli(&["--help"], None);
    let help = text(&out.stdout);
    for word in ["--automation-read-root", "--automation-write-root", "DIACRAFT_AUTOMATION_READ_ROOT", "DIACRAFT_AUTOMATION_WRITE_ROOT"] {
        assert!(help.contains(word), "the help does not say {word}");
    }
}

/// A link in a root that leads out is not followed, in the shipped program too.
#[cfg(unix)]
#[test]
fn a_link_that_leads_out_of_a_root_is_not_followed() {
    let w = World::new();
    std::os::unix::fs::symlink(Path::new(&w.p("outside/drawing.diacraft")), w.dir.path("read").join("leak.diacraft")).unwrap();
    std::os::unix::fs::symlink(Path::new(&w.p("outside")), w.dir.path("write").join("away")).unwrap();
    let input = [
        run_command(1, "file.open", json!({"path": w.p("read/leak.diacraft")})),
        run_command(2, "file.new", json!({})),
        run_command(3, "file.save_as", json!({"path": w.p("write/away/x.diacraft")})),
    ]
    .concat();
    let out = serve(&w.flags(), &[], &input);
    let replies = json_lines(&out.stdout);
    assert_eq!(replies[0]["result"]["isError"], true, "{}", replies[0]);
    assert_eq!(replies[2]["result"]["isError"], true, "{}", replies[2]);
    assert!(!w.dir.path("outside").join("x.diacraft").exists());
}
