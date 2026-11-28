//! The command line's settings: `run` and `mcp` open the person's `config.toml` and save it, `DIACRAFT_NO_PREFS` and an empty `DIACRAFT_CONFIG_DIR` isolate them, and what
//! is wrong with a settings file is said once on standard error. Every program here is given a folder of its own as `DIACRAFT_CONFIG_DIR`, or isolated: none reads the real settings.

use std::process::{Command, Stdio};

use crate::{BIN, TempDir};

/// `diacraft-cli args` with `env`, no standard input, and what it printed and how it ended.
fn cli_with(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(BIN);
    command.args(args).env_remove("RUST_LOG").env_remove("DIACRAFT_NO_PREFS").env_remove("DIACRAFT_CONFIG_DIR");
    for (key, value) in env {
        command.env(key, value);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().unwrap()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const RUN: [&str; 3] = ["run", "--cmd", "app.version"];

#[test]
fn a_run_opens_the_settings_and_writes_them_where_the_variable_says() {
    let dir = TempDir::new("prefs-run");
    let folder = dir.path("settings");
    let out = cli_with(&RUN, &[("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).is_empty(), "nothing is wrong, so nothing is said: {}", stderr(&out));
    let text = std::fs::read_to_string(folder.join("config.toml")).unwrap();
    assert!(text.starts_with("schema_version = 1") && text.contains("[proofing]") && text.contains("[language]"), "{text}");
}

#[test]
fn the_editing_language_of_a_new_profile_is_the_systems() {
    let dir = TempDir::new("prefs-lang");
    let folder = dir.path("settings");
    // A made-up system: the language variables that come before LC_ALL are empty, which reads as unset.
    let env = [("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap()), ("LANGUAGE", ""), ("LC_ALL", "pt_BR.UTF-8"), ("LC_MESSAGES", ""), ("LANG", "")];
    let out = cli_with(&RUN, &env);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = std::fs::read_to_string(folder.join("config.toml")).unwrap();
    assert!(text.contains("editing = [\"pt-BR\"]"), "{text}");
}

/// A first run under `LANG=C` (or `POSIX`, or with no language variable) is a system that names no language, and the profile stores none: `en-US` saved by it would stay for
/// good, and a later run under a real locale would never decide. The later run takes its language, and one that has been decided is kept by a run under `C`.
#[test]
fn a_first_run_under_c_or_posix_stores_no_language_and_a_later_run_decides() {
    let dir = TempDir::new("prefs-lang-c");
    let systems: [(&str, [(&str, &str); 4]); 3] = [
        ("LC_ALL=C", [("LANGUAGE", ""), ("LC_ALL", "C"), ("LC_MESSAGES", ""), ("LANG", "")]),
        ("LANG=POSIX", [("LANGUAGE", ""), ("LC_ALL", ""), ("LC_MESSAGES", ""), ("LANG", "POSIX")]),
        ("no language variable", [("LANGUAGE", ""), ("LC_ALL", ""), ("LC_MESSAGES", ""), ("LANG", "")]),
    ];
    for (n, (name, system)) in systems.iter().enumerate() {
        let folder = dir.path(&format!("settings-{n}"));
        let run_under = |system: &[(&str, &str)]| {
            let mut env: Vec<(&str, &str)> = system.to_vec();
            env.push(("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap()));
            let out = cli_with(&RUN, &env);
            assert!(out.status.success(), "{name}: {}", stderr(&out));
            std::fs::read_to_string(folder.join("config.toml")).unwrap()
        };
        let first = run_under(system);
        assert!(!first.contains("en-US"), "{name}: the first run stores no language:\n{first}");
        let later = run_under(&[("LANGUAGE", ""), ("LC_ALL", "pt_BR.UTF-8"), ("LC_MESSAGES", ""), ("LANG", "")]);
        assert!(later.contains("editing = [\"pt-BR\"]"), "{name}: a later run under a real locale decides:\n{later}");
        let again = run_under(system);
        assert!(again.contains("editing = [\"pt-BR\"]"), "{name}: and a run under C keeps what was decided:\n{again}");
    }
}

#[test]
fn no_prefs_and_an_empty_config_dir_leave_everything_alone() {
    let dir = TempDir::new("prefs-none");
    let folder = dir.path("settings");
    for env in [
        vec![("DIACRAFT_NO_PREFS", "1"), ("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())],
        vec![("DIACRAFT_NO_PREFS", ""), ("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())],
        vec![("DIACRAFT_CONFIG_DIR", "")],
    ] {
        let out = cli_with(&RUN, &env);
        assert!(out.status.success(), "{env:?}: {}", stderr(&out));
        assert!(!folder.exists(), "{env:?}: nothing is made");
    }
}

#[test]
fn the_other_commands_open_no_settings() {
    let dir = TempDir::new("prefs-others");
    let folder = dir.path("settings");
    let env = [("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())];
    assert!(cli_with(&["commands"], &env).status.success());
    // `mcp --connect` drives another program's session and its settings: a connection that fails opens none either.
    let out = cli_with(&["mcp", "--connect", "127.0.0.1:1"], &env);
    assert!(!out.status.success(), "no program listens there");
    assert!(!folder.exists(), "no settings were opened");
}

#[test]
fn a_broken_file_is_said_once_and_the_run_goes_on() {
    let dir = TempDir::new("prefs-broken");
    let folder = dir.path("settings");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("config.toml"), "this is [[[ not toml").unwrap();
    let out = cli_with(&RUN, &[("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())]);
    assert!(out.status.success(), "{}", stderr(&out));
    let said = stderr(&out);
    assert_eq!(said.lines().count(), 1, "once: {said}");
    assert!(said.starts_with("diacraft-cli: ") && said.contains("not valid TOML") && said.contains("moved to"), "{said}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"command\":\"app.version\""), "the command ran: {:?}", out.stdout);
    assert!(folder.join("config.toml").is_file(), "a new file was written");
}

#[test]
fn a_file_from_a_newer_build_is_said_and_left_as_it_is() {
    let dir = TempDir::new("prefs-newer");
    let folder = dir.path("settings");
    std::fs::create_dir_all(&folder).unwrap();
    let newer = "schema_version = 99\n[proofing]\nignore_uppercase = false\n";
    std::fs::write(folder.join("config.toml"), newer).unwrap();
    let out = cli_with(&RUN, &[("DIACRAFT_CONFIG_DIR", folder.to_str().unwrap())]);
    assert!(out.status.success(), "{}", stderr(&out));
    let said = stderr(&out);
    assert_eq!(said.lines().count(), 1, "once, and not again at the end: {said}");
    assert!(said.contains("newer DiaCraft") && said.contains("left as it is"), "{said}");
    assert_eq!(std::fs::read_to_string(folder.join("config.toml")).unwrap(), newer);
}

#[test]
fn a_mcp_session_of_its_own_shares_the_settings() {
    let dir = TempDir::new("prefs-mcp");
    let folder = dir.path("settings");
    let mut child = Command::new(BIN)
        .args(["mcp", "--headless"])
        .env_remove("RUST_LOG")
        .env_remove("DIACRAFT_NO_PREFS")
        .env("DIACRAFT_CONFIG_DIR", &folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The client has nothing to say: the session ends at once.
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(folder.join("config.toml").is_file(), "the session opened and saved the settings");
}
