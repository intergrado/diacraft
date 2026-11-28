//! `diacraft-cli mcp`: the binary on a real pipe. What it must keep that the library tests cannot: stdout
//! carries nothing but protocol, logs go to stderr, and the shipped build keeps serde_json's recursion limit.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::{BIN, cli, json_lines, text};

fn line(id: u64, method: &str, params: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
}

fn call(id: u64, tool: &str, arguments: Value) -> String {
    line(id, "tools/call", json!({"name": tool, "arguments": arguments}))
}

/// Runs the binary with `args` and `RUST_LOG` as given, feeds it `input` and closes its input.
fn run_with_log(args: &[&str], input: &str, rust_log: Option<&str>) -> std::process::Output {
    let mut command = Command::new(BIN);
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env_remove("RUST_LOG").env("DIACRAFT_NO_PREFS", "1");
    if let Some(level) = rust_log {
        command.env("RUST_LOG", level);
    }
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    match stdin.write_all(input.as_bytes()) {
        Ok(()) => {}
        // The program may refuse and exit before it reads a byte; the test's assertions on its exit code and stderr say whether that
        // was right. Any other failure to write is the test's own and stops it.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => panic!("cannot write to the program's input: {e}"),
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

/// A fake app on a free port that says it is `app`, answers `engine.execute` with `{"ran": <command>}` and records
/// what it was asked. Returns its address and the methods it saw.
fn fake_app(app: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let record = record.clone();
            std::thread::spawn(move || {
                let mut writer = stream.try_clone().unwrap();
                for request in BufReader::new(stream).lines().map_while(Result::ok) {
                    let Ok(request) = serde_json::from_str::<Value>(&request) else { return };
                    let method = request["method"].as_str().unwrap_or("").to_owned();
                    record.lock().unwrap().push(method.clone());
                    let result = match method.as_str() {
                        "ui.inspect" => json!({"app": app}),
                        _ => json!({"ran": request["params"]["command"]}),
                    };
                    if writeln!(writer, "{}", json!({"id": request["id"], "ok": true, "result": result})).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, seen)
}

#[test]
fn a_binary_that_exits_without_reading_its_input_is_not_a_test_failure() {
    // `--version` ends the program without a read of standard input. The input is larger than a pipe holds, so the write
    // cannot finish before the program is gone and always meets a closed pipe: the race `connect_refuses_another_app...`
    // lost now and then (the program refuses and exits before the test writes), made certain.
    let out = run_with_log(&["--version"], &"x".repeat(1024 * 1024), None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), format!("diacraft-cli {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn headless_mcp_writes_nothing_but_protocol_on_stdout() {
    let input = [
        line(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})),
        format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        line(2, "tools/list", json!({})),
        call(3, "command_run", json!({"id": "file.new"})),
        call(4, "command_run", json!({"id": "page.add", "params": {"name": "Second"}})),
        call(5, "doc_inspect", json!({})),
    ]
    .concat();
    let out = run_with_log(&["mcp", "--headless"], &input, None);
    assert_eq!(out.status.code(), Some(0), "the end of the input is a clean end: {}", text(&out.stderr));
    // Every byte of stdout is a JSON-RPC reply, one per request, in order.
    let replies = json_lines(&out.stdout);
    assert_eq!(replies.len(), 5, "{replies:?}");
    for (i, reply) in replies.iter().enumerate() {
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], i + 1, "{reply}");
        assert!(reply.get("result").is_some() && reply.get("error").is_none(), "{reply}");
    }
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "diacraft");
    assert_eq!(replies[1]["result"]["tools"].as_array().map(Vec::len), Some(6));
    let doc: Value = serde_json::from_str(replies[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(doc["pages"].as_array().map(Vec::len), Some(2));
    // What a person starting it by hand wants to read is on stderr.
    let stderr = text(&out.stderr);
    assert!(stderr.contains("MCP server on stdio") && stderr.contains("headless"), "{stderr}");
}

#[test]
fn an_empty_input_is_a_clean_end() {
    let out = run_with_log(&["mcp", "--headless"], "", None);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "{}", text(&out.stdout));
}

#[test]
fn logs_go_to_stderr_and_only_when_asked_for() {
    // An unknown notification is logged at the debug level.
    let input = format!("{}\n{}", json!({"jsonrpc": "2.0", "method": "notifications/made-up"}), line(1, "ping", json!({})));
    let quiet = run_with_log(&["mcp", "--headless"], &input, None);
    let loud = run_with_log(&["mcp", "--headless"], &input, Some("debug"));
    for out in [&quiet, &loud] {
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(json_lines(&out.stdout), [json!({"jsonrpc": "2.0", "id": 1, "result": {}})], "stdout is the protocol and only that");
    }
    assert!(!text(&quiet.stderr).contains("made-up"), "{}", text(&quiet.stderr));
    assert!(text(&loud.stderr).contains("made-up"), "{}", text(&loud.stderr));
}

#[test]
fn a_thousand_levels_is_one_error_in_the_shipped_binary() {
    // The library test pins this for the library; this is the program that ships, with the features its whole
    // dependency graph turns on, reading a real pipe.
    let deep = format!("{}1{}\n", "{\"a\":".repeat(1000), "}".repeat(1000));
    let input = format!("{}{deep}{}", line(1, "ping", json!({})), line(2, "ping", json!({})));
    let out = run_with_log(&["mcp", "--headless"], &input, None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[1]["error"]["code"], -32700, "{replies:?}");
    assert!(replies[1]["error"]["message"].as_str().unwrap().contains("recursion limit"), "{replies:?}");
    assert_eq!(replies[2]["id"], 2);
}

#[test]
fn connect_to_a_port_with_nothing_on_it_exits_with_1() {
    let gone = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    let out = cli(&["mcp", "--connect", &gone], Some(b""));
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("cannot reach"), "{}", text(&out.stderr));
}

#[test]
fn connect_refuses_another_app_in_the_shipped_binary() {
    let (addr, seen) = fake_app("deckcraft");
    let out = run_with_log(&["mcp", "--connect", &addr], &call(1, "command_run", json!({"id": "file.new"})), None);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("not DiaCraft") && stderr.contains("deckcraft"), "{stderr}");
    assert!(out.stdout.is_empty(), "the other app was served: {}", text(&out.stdout));
    assert_eq!(*seen.lock().unwrap(), ["ui.inspect"], "nothing but the question reached the other app");
}

#[test]
fn connect_to_diacraft_serves_through_the_app() {
    let (addr, seen) = fake_app("diacraft");
    let out = run_with_log(&["mcp", "--connect", &addr], &call(1, "command_run", json!({"id": "file.new"})), None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let replies = json_lines(&out.stdout);
    let ran: Value = serde_json::from_str(replies[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(ran, json!({"ran": "file.new"}));
    assert_eq!(*seen.lock().unwrap(), ["ui.inspect", "engine.execute"]);
    assert!(text(&out.stderr).contains(&addr), "the banner names the app: {}", text(&out.stderr));
}

/// The peak resident memory, in KiB, of `diacraft-cli mcp --headless` once it has taken `line` and answered it and a ping behind
/// it (`VmHWM` in `/proc/<pid>/status`: the process is still running, so it can be read).
#[cfg(target_os = "linux")]
fn peak_kib_after(line: &str) -> u64 {
    let mut child = Command::new(BIN)
        .args(["mcp", "--headless"])
        .env_remove("RUST_LOG")
        .env("DIACRAFT_NO_PREFS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    stdin.write_all(line.as_bytes()).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.write_all(self::line(2, "ping", json!({})).as_bytes()).unwrap();
    let mut reply = String::new();
    for _ in 0..2 {
        reply.clear();
        assert!(stdout.read_line(&mut reply).unwrap() > 0, "the server closed its output");
    }
    assert!(reply.contains("\"id\":2"), "the second reply is the ping's: {reply}");
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
    let kib = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or_else(|| panic!("no VmHWM in {status}"));
    drop(stdin);
    assert!(child.wait().unwrap().success());
    kib
}

/// A line just under the server's 16 MiB limit: `prefix`, as many copies of `element` as fit, `suffix`.
#[cfg(target_os = "linux")]
fn nearly_16_mib(prefix: &str, element: &str, suffix: &str) -> String {
    let room = 16 * 1024 * 1024 - 4096 - prefix.len() - suffix.len();
    format!("{prefix}{}{suffix}", vec![element; room / (element.len() + 1)].join(","))
}

#[cfg(target_os = "linux")]
#[test]
fn a_16_mib_line_costs_what_parsing_it_costs_and_no_more() {
    // One line of 16 MiB is what the protocol allows, and parsing it costs one to two gigabytes, by what is in it (a JSON
    // value is much bigger than its text). What the tools do with it must not add to that: a batch that builds a command for each of its
    // million and a half steps, or a command's parameters copied twice, took 6.5 GiB, and a failed allocation aborts the process
    // and the session with it. The baseline is the same bytes as the params of a `ping`, which only parses them.
    let ping = |element: &str| nearly_16_mib("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"params\":{\"x\":[", element, "]}}");
    let batch = nearly_16_mib(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"command_batch\",\"arguments\":{\"steps\":[",
        "{\"id\":\"a\"}",
        "]}}}",
    );
    let run = nearly_16_mib(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"command_run\",\"arguments\":{\"id\":\"file.new\",\"params\":{\"a\":[",
        "{\"a\":0}",
        "]}}}}",
    );
    for (what, line, baseline) in [("command_batch", batch, ping("{\"id\":\"a\"}")), ("command_run", run, ping("{\"a\":0}"))] {
        let (cost, parse) = (peak_kib_after(&line), peak_kib_after(&baseline));
        assert!(
            cost * 4 <= parse * 5,
            "{what}: a 16 MiB line peaked at {} MiB, and parsing the same bytes alone at {} MiB",
            cost / 1024,
            parse / 1024
        );
    }
}
