//! The shell completions, the colours and the width of the help, and the rule that colour never reaches what a program reads.
//!
//! The binary is run, as a person or a script runs it: its standard output is a pipe unless a test says it is a terminal (`pty`), so every test starts from "not a terminal"
//! and the variables that decide the colour are removed from the child's environment first.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

use crate::{BIN, TempDir, s, text};

/// The shells the completions are written for, in the order the help names them.
const SHELLS: [&str; 5] = ["bash", "zsh", "fish", "powershell", "elvish"];

/// Every command of the program, and the options each takes (the options every command takes are in [`EVERY_COMMAND`]).
const COMMANDS: [&str; 10] = ["mcp", "run", "commands", "oracle", "export", "convert", "roundtrip", "compare", "completions", "man"];
const OPTIONS: &[(&str, &[&str])] = &[
    ("mcp", &["--headless", "--connect", "--bundled-fonts"]),
    ("run", &["--cmd", "--params", "--commands", "--bundled-fonts"]),
    ("oracle", &["--json", "--source", "--aggregate", "--cap"]),
    ("export", &["--out", "--page", "--dpi", "--text", "--transparent", "--bundled-fonts"]),
    ("convert", &["--out", "--drop-macros", "--drop-objects", "--page", "--dpi", "--text", "--transparent", "--bundled-fonts"]),
    ("roundtrip", &["--out", "--move", "--via", "--edit", "--bundled-fonts"]),
    ("compare", &["--content"]),
    ("completions", &["--program"]),
    ("man", &["--list"]),
];
const EVERY_COMMAND: [&str; 3] = ["--color", "--help", "--version"];

/// Runs the program with `args` and `env` set, on a pipe, with the variables that decide the colour taken out of the environment first.
fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
    run_with(args, env, None)
}

fn run_with(args: &[&str], env: &[(&str, &str)], stdin: Option<&[u8]>) -> Output {
    let mut command = Command::new(BIN);
    command.args(args).env_remove("RUST_LOG").stdout(Stdio::piped()).stderr(Stdio::piped());
    // A test never reads or writes the person's settings: `run` and `mcp` open them unless this is set.
    command.env("DIACRAFT_NO_PREFS", "1");
    for variable in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "TERM", "COLUMNS"] {
        command.env_remove(variable);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = command.spawn().unwrap();
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // The program may stop reading early; that is for the assertions to notice.
        let _ = pipe.write_all(bytes);
    }
    child.wait_with_output().unwrap()
}

/// The text without its terminal escapes (the colour codes: `ESC [ ... letter`).
fn strip_ansi(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            continue;
        }
        plain.push(c);
    }
    plain
}

fn has_escape(text: &str) -> bool {
    text.contains('\u{1b}')
}

// ------------------------------------------------------------------------------------------------------------------------ completions

fn script(shell: &str) -> String {
    let out = run(&["completions", shell], &[]);
    assert!(out.status.success(), "completions {shell}: {}", text(&out.stderr));
    assert!(out.stderr.is_empty(), "completions {shell} printed on standard error: {}", text(&out.stderr));
    text(&out.stdout)
}

#[test]
fn completions_are_written_to_standard_output_for_every_shell() {
    for shell in SHELLS {
        let script = script(shell);
        assert!(script.len() > 500, "{shell}: only {} bytes", script.len());
        assert!(script.contains("diacraft-cli"), "{shell}: the script does not name the program");
        assert!(!has_escape(&script), "{shell}: a terminal escape in the script");
    }
}

/// Runs PowerShell on `command`; `None` when there is none here, or it does not start (a machine whose .NET is not the one it was built for says so and stops: that is no
/// script's mistake). A newer .NET than the one it asks for is allowed.
fn pwsh(command: &str) -> Option<Output> {
    let run = |command: &str| {
        Command::new("pwsh")
            .env("DOTNET_ROLL_FORWARD", "Major")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .stdin(Stdio::null())
            .output()
            .ok()
    };
    run("exit 0").filter(|probe| probe.status.success())?;
    run(command)
}

/// Runs `program args... script_path` and says whether it ended well; `None` when the program is not on this machine.
fn syntax_check(program: &str, args: &[&str], script_path: &Path) -> Option<(bool, String)> {
    let out = Command::new(program).args(args).arg(script_path).stdin(Stdio::null()).output().ok()?;
    Some((out.status.success(), text(&out.stderr)))
}

#[test]
fn each_script_parses_in_its_shell_where_the_shell_is_here() {
    let dir = TempDir::new("completions-parse");
    let mut checked = Vec::new();
    for shell in SHELLS {
        let path = dir.write(&format!("script.{shell}"), script(shell).as_bytes());
        let result = match shell {
            "bash" => syntax_check("bash", &["-n"], &path),
            "zsh" => syntax_check("zsh", &["-n"], &path),
            "fish" => syntax_check("fish", &["--no-execute"], &path),
            // PowerShell: the parser reads the file and reports its errors; it runs nothing.
            "powershell" => {
                let command = format!(
                    "$e=$null; [void][System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$null,[ref]$e); if ($e.Count -gt 0) {{ $e | ForEach-Object {{ $_.Message }}; exit 1 }}",
                    path.display()
                );
                pwsh(&command).map(|out| (out.status.success(), format!("{}{}", text(&out.stdout), text(&out.stderr))))
            }
            // Elvish has no syntax check of its own that does not run the file: it is read for what it must hold, below.
            _ => None,
        };
        if let Some((ok, said)) = result {
            assert!(ok, "{shell}: the script does not parse: {said}");
            checked.push(shell);
        }
    }
    // The three shells every developer machine and CI runner of this project has hold bash; the rest are checked when they are here.
    assert!(checked.contains(&"bash"), "bash must be here to test the scripts; checked {checked:?}");
}

/// The words of a script: runs of letters, digits, `.`, `_` and `-`.
fn words(script: &str) -> HashSet<String> {
    script.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// Whether `script` offers `option` (written `--name`) in the way of `shell`: fish writes a long option as `-l name`.
fn offers(shell: &str, script: &str, words: &HashSet<String>, option: &str) -> bool {
    if shell == "fish" {
        let name = option.trim_start_matches('-');
        return script.lines().any(|line| line.contains(&format!(" -l {name} ")) || line.ends_with(&format!(" -l {name}")));
    }
    words.contains(option)
}

/// A snapshot of what each script completes: every command, and every option of every command, in each of the five scripts.
#[test]
fn each_script_names_every_command_and_every_option() {
    for shell in SHELLS {
        let script = script(shell);
        let words = words(&script);
        for command in COMMANDS {
            assert!(words.contains(command), "{shell}: no command `{command}`");
        }
        for (command, options) in OPTIONS {
            for option in options.iter().chain(&EVERY_COMMAND) {
                assert!(offers(shell, &script, &words, option), "{shell}: no option `{option}` (of `{command}`)");
            }
        }
    }
}

/// The shells whose scripts carry the values an option takes and the command ids: clap_complete writes none of them into Elvish's, and PowerShell's gets them from a block added after.
const SHELLS_WITH_VALUES: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

#[test]
fn the_scripts_carry_the_values_the_options_take() {
    for shell in SHELLS_WITH_VALUES {
        let script = script(shell);
        let words = words(&script);
        for value in ["outlines", "retext", "diacraft", "always", "never", "powershell", "elvish", "delete", "diacraft-cli-run"] {
            assert!(words.contains(value), "{shell}: no value `{value}`");
        }
    }
}

/// `run --cmd` completes the command ids of the registry: the list `commands` prints.
#[test]
fn run_completes_the_command_ids_of_the_registry() {
    let listing: Value = serde_json::from_slice(&run(&["commands"], &[]).stdout).unwrap();
    let ids: Vec<&str> = listing.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert!(ids.len() > 100, "{} commands", ids.len());
    for shell in SHELLS_WITH_VALUES {
        let words = words(&script(shell));
        let missing: Vec<&&str> = ids.iter().filter(|id| !words.contains(**id)).collect();
        assert!(missing.is_empty(), "{shell}: {} of {} command ids are not completed, the first: {:?}", missing.len(), ids.len(), missing.first());
    }
}

#[test]
fn the_apps_flags_have_scripts_of_their_own() {
    for shell in SHELLS {
        let out = run(&["completions", shell, "--program", "diacraft"], &[]);
        assert!(out.status.success(), "{shell}: {}", text(&out.stderr));
        let script = text(&out.stdout);
        let words = words(&script);
        for option in ["--control", "--version", "--help"] {
            assert!(offers(shell, &script, &words, option), "{shell}: no `{option}` for the app");
        }
        assert!(!words.contains("roundtrip") && !words.contains("--bundled-fonts"), "{shell}: the app's script holds the command line's commands");
        assert!(script.contains("diacraft") && !script.contains("diacraft-cli"), "{shell}: the script is not for `diacraft` alone");
    }
}

#[test]
fn completions_refuse_what_is_not_a_shell_and_name_the_five() {
    for args in [
        &["completions"][..],
        &["completions", "tcsh"],
        &["completions", "Bash"],
        &["completions", "bash", "zsh"],
        &["completions", "bash", "--program", "other"],
        &["completions", "bash", "--program"],
        &["completions", "bash", "--program", "diacraft", "--program", "diacraft"],
        &["completions", "--bogus"],
    ] {
        let out = run(args, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}: nothing is written on a mistake");
        assert!(text(&out.stderr).starts_with("diacraft-cli: "), "{args:?}: {}", text(&out.stderr));
    }
    let said = text(&run(&["completions"], &[]).stderr);
    for shell in SHELLS {
        assert!(said.contains(shell), "the refusal names {shell}: {said}");
    }
}

/// Asks bash what it would complete after `words` (the last is the word being typed), in `dir`: the function the script registers, called as bash calls it.
fn bash_completes(script_path: &Path, words: &[&str], dir: &Path) -> Option<Vec<String>> {
    const DRIVER: &str = r#"
source "$1"; shift
COMP_WORDS=("$@")
COMP_CWORD=$(($# - 1))
COMP_LINE="$*"
COMP_POINT=${#COMP_LINE}
cur=${COMP_WORDS[COMP_CWORD]}
prev=${COMP_WORDS[COMP_CWORD - 1]}
# The function the script registered for the program, which is what bash calls.
function=$(complete -p diacraft-cli | sed -n 's/.* -F \([^ ]*\) .*/\1/p')
"$function" "${COMP_WORDS[0]}" "$cur" "$prev"
printf '%s\n' "${COMPREPLY[@]}"
"#;
    let out = Command::new("bash").current_dir(dir).args(["-c", DRIVER, "bash"]).arg(script_path).args(words).stdin(Stdio::null()).output().ok()?;
    assert!(out.status.success(), "bash: {}", text(&out.stderr));
    let mut found: Vec<String> = text(&out.stdout).lines().map(str::to_owned).collect();
    found.sort();
    Some(found)
}

/// A folder with drawings of two types, a text file, a picture and a folder.
fn folder_of_files(tag: &str) -> TempDir {
    let dir = TempDir::new(tag);
    for name in ["a.vsdx", "B.ODG", "notes.txt", "p.svg", "q.png", "steps.jsonl"] {
        dir.write(name, b"x");
    }
    std::fs::create_dir(dir.path("sub")).unwrap();
    dir
}

#[test]
fn bash_completes_the_commands_the_options_and_the_values() {
    let dir = TempDir::new("completions-bash");
    let path = dir.write("script.bash", script("bash").as_bytes());
    let at = dir.path("");
    let all: Vec<String> = bash_completes(&path, &["diacraft-cli", ""], &at).unwrap();
    for command in COMMANDS {
        assert!(all.iter().any(|c| c == command), "`{command}` among {all:?}");
    }
    // The options of the command the line is in, and of no other.
    let export = bash_completes(&path, &["diacraft-cli", "export", "--"], &at).unwrap();
    assert!(export.iter().any(|c| c == "--out") && export.iter().any(|c| c == "--dpi"), "{export:?}");
    assert!(!export.iter().any(|c| c == "--cmd" || c == "--connect"), "{export:?}");
    // (`--color` is among them: every command takes it.)
    let run = bash_completes(&path, &["diacraft-cli", "run", "--c"], &at).unwrap();
    assert_eq!(run, ["--cmd", "--color", "--commands"], "{run:?}");
    // The values of an option that takes one of a few.
    assert_eq!(bash_completes(&path, &["diacraft-cli", "export", "f.vsdx", "--text", ""], &at).unwrap(), ["outlines", "text"]);
    assert_eq!(bash_completes(&path, &["diacraft-cli", "--color", ""], &at).unwrap(), ["always", "auto", "never"]);
    let ids = bash_completes(&path, &["diacraft-cli", "run", "--cmd", "file.n"], &at).unwrap();
    assert_eq!(ids, ["file.new"], "{ids:?}");
}

/// **Files are completed by their types**: a drawing where a drawing is read, a picture type where a picture is written, and always the folders, so that a path can be walked.
#[test]
fn bash_completes_files_by_their_types_and_always_the_folders() {
    let dir = folder_of_files("completions-bash-files");
    // The script is kept apart from the files it completes.
    let kept = TempDir::new("completions-bash-script");
    let path = kept.write("script.bash", script("bash").as_bytes());
    let at = dir.path("");
    // Where a drawing is read: the drawings (in any case of the extension) and the folder (bash shows it with its slash), and not the text file or the pictures.
    for command in ["export", "convert", "roundtrip"] {
        let found = bash_completes(&path, &["diacraft-cli", command, ""], &at).unwrap();
        assert_eq!(found, ["B.ODG", "a.vsdx", "sub"], "{command}: {found:?}");
    }
    assert_eq!(bash_completes(&path, &["diacraft-cli", "export", "a"], &at).unwrap(), ["a.vsdx"]);
    // Where a picture is written: the picture types, and the folder.
    let found = bash_completes(&path, &["diacraft-cli", "export", "a.vsdx", "--out", ""], &at).unwrap();
    assert_eq!(found, ["p.svg", "q.png", "sub"], "{found:?}");
    // Where a file of any kind is read (the steps of `run`), everything.
    let found = bash_completes(&path, &["diacraft-cli", "run", "--commands", ""], &at).unwrap();
    assert_eq!(found, ["B.ODG", "a.vsdx", "notes.txt", "p.svg", "q.png", "steps.jsonl", "sub"], "{found:?}");
}

/// Asks PowerShell what it would complete after `line` with the script loaded; `None` when there is no PowerShell that starts here.
fn powershell_completes(script_path: &Path, line: &str) -> Option<Vec<String>> {
    let quote = |text: &str| text.replace('\'', "''");
    let command = format!(
        "$ErrorActionPreference = 'Stop'; . ([scriptblock]::Create((Get-Content -Raw -LiteralPath '{}'))); $r = [System.Management.Automation.CommandCompletion]::CompleteInput('{}', {}, $null); $r.CompletionMatches | ForEach-Object {{ $_.CompletionText }}",
        quote(&script_path.display().to_string()),
        quote(line),
        line.len()
    );
    let out = pwsh(&command)?;
    assert!(out.status.success(), "powershell: {}", text(&out.stderr));
    let mut found: Vec<String> = text(&out.stdout).lines().map(|l| l.trim_end_matches('\r').to_owned()).filter(|l| !l.is_empty()).collect();
    found.sort();
    Some(found)
}

#[test]
fn powershell_completes_the_commands_the_options_the_values_and_the_ids() {
    let kept = TempDir::new("completions-powershell");
    let path = kept.write("script.ps1", script("powershell").as_bytes());
    let Some(all) = powershell_completes(&path, "diacraft-cli ") else { return };
    for command in COMMANDS {
        assert!(all.iter().any(|c| c == command), "`{command}` among {all:?}");
    }
    let export = powershell_completes(&path, "diacraft-cli export --").unwrap();
    assert!(export.iter().any(|c| c == "--out") && export.iter().any(|c| c == "--dpi") && !export.iter().any(|c| c == "--cmd"), "{export:?}");
    assert_eq!(powershell_completes(&path, "diacraft-cli export f.vsdx --text ").unwrap(), ["outlines", "text"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli export f.vsdx --text o").unwrap(), ["outlines"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli convert f.vsdx --color ").unwrap(), ["always", "auto", "never"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli roundtrip f.vsdx --via ").unwrap(), ["diacraft"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli roundtrip f.vsdx --edit r").unwrap(), ["retext"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli completions ").unwrap(), ["bash", "elvish", "fish", "powershell", "zsh"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli completions --program ").unwrap(), ["diacraft", "diacraft-cli"]);
    assert_eq!(powershell_completes(&path, "diacraft-cli run --cmd file.n").unwrap(), ["file.new"]);
    let pages = powershell_completes(&path, "diacraft-cli man diacraft-cli-r").unwrap();
    assert_eq!(pages, ["diacraft-cli-roundtrip", "diacraft-cli-run"]);
}

/// Asks fish what it would complete after `line`, in `dir`, with the script loaded; `None` when fish is not here.
fn fish_completes(script_path: &Path, line: &str, dir: &Path) -> Option<Vec<String>> {
    let program = format!("source {}; complete -C '{}'", script_path.display(), line);
    let out = Command::new("fish").current_dir(dir).args(["--no-config", "-c", &program]).stdin(Stdio::null()).output().ok()?;
    assert!(out.status.success(), "fish: {}", text(&out.stderr));
    // Each candidate is the word and, after a tab, its description.
    let mut found: Vec<String> = text(&out.stdout).lines().map(|l| l.split('\t').next().unwrap_or("").to_owned()).collect();
    found.sort();
    Some(found)
}

#[test]
fn fish_completes_the_commands_the_values_and_the_files_by_their_types() {
    let dir = folder_of_files("completions-fish");
    let kept = TempDir::new("completions-fish-script");
    let path = kept.write("script.fish", script("fish").as_bytes());
    let at = dir.path("");
    let Some(all) = fish_completes(&path, "diacraft-cli ", &at) else { return };
    for command in COMMANDS {
        assert!(all.iter().any(|c| c == command), "`{command}` among {all:?}");
    }
    assert_eq!(fish_completes(&path, "diacraft-cli export f.vsdx --text ", &at).unwrap(), ["outlines", "text"]);
    assert_eq!(fish_completes(&path, "diacraft-cli export ", &at).unwrap(), ["B.ODG", "a.vsdx", "sub/"]);
    assert_eq!(fish_completes(&path, "diacraft-cli export a.vsdx --out ", &at).unwrap(), ["p.svg", "q.png", "sub/"]);
    let steps = fish_completes(&path, "diacraft-cli run --commands ", &at).unwrap();
    assert!(steps.iter().any(|c| c == "notes.txt"), "{steps:?}");
}

#[test]
fn zsh_and_the_other_scripts_hold_the_file_types_too() {
    // zsh is read for what it must hold, as no test here can ask it to complete: where a drawing is read its action lists the types (`_files -g PATTERN`, in which the case of a
    // letter is any), and where a picture is written the pictures.
    let zsh = script("zsh");
    assert!(zsh.contains("'*::file -- the drawing to read:_files -g \"*.([dD][iI][aA][cC][rR][aA][fF][tT]|"), "{zsh}");
    assert!(zsh.contains("[vV][sS][dD][xX]") && zsh.contains("[oO][dD][gG]"), "{zsh}");
    assert!(zsh.contains("'--out=[where to write (replaced if it is there; never the drawing itself)]:PATH:_files -g \"*.([pP][nN][gG]|[sS][vV][gG]|[pP][dD][fF])\"'"), "{zsh}");
    // Where any file will do (the steps of `run`) it is `_files` alone.
    assert!(zsh.contains(":SRC:_files' \\"), "{zsh}");
    assert!(script("fish").contains("__fish_diacraft_cli_by_type png svg pdf"));
}

// ------------------------------------------------------------------------------------------------------------------------ colour

#[test]
fn the_help_is_plain_when_standard_output_is_not_a_terminal() {
    let out = run(&["--help"], &[]);
    assert!(out.status.success() && out.stderr.is_empty());
    let help = text(&out.stdout);
    assert!(!has_escape(&help), "a pipe got colour");
    assert!(help.starts_with("Usage: diacraft-cli <command> [options]\n"), "{help}");
    assert!(help.contains("diacraft-cli completions SHELL") && help.contains("--color"), "the help names the new command and option");
}

#[test]
fn color_always_colours_the_help_and_nothing_but_the_colour_changes() {
    let plain = text(&run(&["--help"], &[]).stdout);
    let coloured = text(&run(&["--color", "always", "--help"], &[]).stdout);
    assert!(has_escape(&coloured), "--color always gave plain text");
    assert_eq!(strip_ansi(&coloured), plain, "the colours are all that differ");
    assert_eq!(text(&run(&["--help", "--color=always"], &[]).stdout), coloured, "either spelling, either place");
    assert_eq!(text(&run(&["export", "--color", "always", "-h"], &[]).stdout), coloured, "after a command too, and -h");
}

#[test]
fn the_environment_decides_the_colour_when_the_flag_does_not() {
    let plain = text(&run(&["--help"], &[]).stdout);
    // On a pipe only the forcing variable colours.
    assert!(has_escape(&text(&run(&["--help"], &[("CLICOLOR_FORCE", "1")]).stdout)));
    assert!(!has_escape(&text(&run(&["--help"], &[("CLICOLOR_FORCE", "0")]).stdout)), "CLICOLOR_FORCE=0 does not force");
    assert!(!has_escape(&text(&run(&["--help"], &[("CLICOLOR_FORCE", "")]).stdout)), "an empty CLICOLOR_FORCE is not set");
    // NO_COLOR, set and not empty, wins over the forcing variable; an empty one is not set.
    assert_eq!(text(&run(&["--help"], &[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")]).stdout), plain);
    assert!(has_escape(&text(&run(&["--help"], &[("NO_COLOR", ""), ("CLICOLOR_FORCE", "1")]).stdout)));
    // CLICOLOR=0 is plain, and the force wins over it.
    assert_eq!(text(&run(&["--help"], &[("CLICOLOR", "0")]).stdout), plain);
    // The flag wins over every variable.
    assert!(has_escape(&text(&run(&["--help", "--color", "always"], &[("NO_COLOR", "1")]).stdout)));
    assert_eq!(text(&run(&["--help", "--color", "never"], &[("CLICOLOR_FORCE", "1")]).stdout), plain);
    assert!(has_escape(&text(&run(&["--help", "--color", "auto"], &[("CLICOLOR_FORCE", "1")]).stdout)));
}

#[test]
fn a_mistake_is_said_on_standard_error_and_coloured_only_where_colour_is_asked() {
    let plain = run(&["export", "--bogus"], &[]);
    assert_eq!(plain.status.code(), Some(2));
    assert!(plain.stdout.is_empty());
    let said = text(&plain.stderr);
    assert!(said.starts_with("diacraft-cli: unknown option `--bogus` for `export`; see --help"), "{said}");
    let coloured = run(&["--color", "always", "export", "--bogus"], &[]);
    assert_eq!(coloured.status.code(), Some(2));
    assert!(coloured.stdout.is_empty());
    assert!(has_escape(&text(&coloured.stderr)), "no colour in the message");
    assert_eq!(strip_ansi(&text(&coloured.stderr)), said, "the words are the same");
    // A failure of a step (status 1) is said the same way.
    let failed = run(&["--color", "always", "run", "--cmd", "no.such.command"], &[]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(has_escape(&text(&failed.stderr)) && failed.stdout.is_empty(), "{}", text(&failed.stderr));
    assert!(strip_ansi(&text(&failed.stderr)).starts_with("diacraft-cli: step 0 (no.such.command)"), "{}", text(&failed.stderr));
}

/// **Colour never goes into anything a program reads**: the JSON answers, the MCP stream and the exit statuses are the same bytes with every colour switch on.
#[test]
fn colour_never_reaches_the_json_the_mcp_stream_or_the_exit_status() {
    let forced = [("CLICOLOR_FORCE", "1"), ("TERM", "xterm-256color")];
    for args in [&["commands"][..], &["run", "--cmd", "file.new", "--cmd", "document.inspect"], &["run", "--cmd", "app.version"]] {
        let plain = run(args, &[]);
        let mut with_flag = vec!["--color", "always"];
        with_flag.extend_from_slice(args);
        for coloured in [run(&with_flag, &[]), run(args, &forced), run(&with_flag, &forced)] {
            assert_eq!(coloured.stdout, plain.stdout, "{args:?}");
            assert_eq!(coloured.status.code(), plain.status.code(), "{args:?}");
            assert!(!has_escape(&text(&coloured.stdout)), "{args:?}");
        }
    }
    // The MCP server: a request in, a JSON line out, with colour asked for in every way. Its words on standard error are for people and are not coloured either.
    let initialize = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}\n";
    let plain = run_with(&["mcp", "--headless", "--bundled-fonts"], &[], Some(initialize));
    let coloured = run_with(&["--color", "always", "mcp", "--headless", "--bundled-fonts"], &forced, Some(initialize));
    assert_eq!(coloured.stdout, plain.stdout);
    assert!(text(&coloured.stdout).contains("\"result\""), "{}", text(&coloured.stdout));
    assert_eq!(coloured.stderr, plain.stderr, "the server's notes are plain");
    assert!(coloured.status.success());
}

#[test]
fn color_is_given_once_takes_auto_always_or_never_and_does_not_stop_help_or_version() {
    for args in [
        &["--color", "always", "--color", "never", "commands"][..],
        &["commands", "--color=always", "--color=always"],
        &["--color", "red", "commands"],
        &["--color", "Always", "commands"],
        &["--color", "commands"],
        &["--color"],
        &["commands", "--color"],
    ] {
        let out = run(args, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out.stderr));
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(strip_ansi(&text(&out.stderr)).starts_with("diacraft-cli: "), "{args:?}: {}", text(&out.stderr));
    }
    let said = text(&run(&["--color", "red", "commands"], &[]).stderr);
    assert!(said.contains("--color") && said.contains("auto") && said.contains("always") && said.contains("never"), "{said}");
    for args in [&["--color", "red", "--help"][..], &["--color", "--version"], &["--color=red", "-V"], &["commands", "--color", "red", "-h"]] {
        assert!(run(args, &[]).status.success(), "{args:?}");
    }
}

/// `--` ends the options, for every command: a file whose name begins with a dash is named after it.
#[test]
fn two_dashes_end_the_options_of_every_command() {
    let dir = TempDir::new("ux-dashes");
    let out = run(&["export", "--out", s(&dir.path("p.svg")), "--", "-odd.vsdx"], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("-odd.vsdx") || text(&out.stderr).contains("cannot"), "{}", text(&out.stderr));
    let out = run(&["export", "-odd.vsdx", "--out", s(&dir.path("p.svg"))], &[]);
    assert_eq!(out.status.code(), Some(2), "without the two dashes it is an option the program does not have");
    assert!(text(&out.stderr).contains("unknown option `-odd.vsdx`"), "{}", text(&out.stderr));
    // After the two dashes `--help` and `--version` are file names too, not a request for the help or the version.
    for name in ["--help", "--version"] {
        let out = run(&["export", "--out", s(&dir.path("p.svg")), "--", name], &[("DIACRAFT_NO_PREFS", "1")]);
        assert_eq!(out.status.code(), Some(1), "`{name}` is a drawing that does not exist: {}", text(&out.stderr));
        assert!(out.stdout.is_empty(), "`{name}` printed {}", text(&out.stdout));
        assert!(text(&out.stderr).contains("cannot read the drawing"), "{}", text(&out.stderr));
    }
}

// ------------------------------------------------------------------------------------------------------------------------ at a terminal

#[cfg(unix)]
mod terminal {
    use super::*;
    use crate::pty::in_terminal;

    #[test]
    fn at_a_terminal_the_help_is_coloured() {
        let (help, status) = in_terminal(&["--help"], &[], 100);
        assert_eq!(status, Some(0));
        assert!(has_escape(&help), "no colour at a terminal: {help}");
        // Headings, option names and the program's own words are marked; the page is the same words.
        assert!(help.contains("\u{1b}[") && strip_ansi(&help).contains("Usage: diacraft-cli <command> [options]"));
    }

    #[test]
    fn at_a_terminal_the_environment_and_the_flag_can_still_switch_it_off() {
        for (args, env) in [
            (&["--help"][..], &[("NO_COLOR", "1")][..]),
            (&["--help"], &[("TERM", "dumb")]),
            (&["--help"], &[("CLICOLOR", "0")]),
            (&["--help", "--color", "never"], &[]),
        ] {
            let (help, status) = in_terminal(args, env, 100);
            assert_eq!(status, Some(0));
            assert!(!has_escape(&help), "{args:?} {env:?}: {help}");
            assert!(help.contains("Usage: diacraft-cli <command> [options]"));
        }
        // And the flag switches it on for a dumb terminal.
        let (help, _) = in_terminal(&["--help", "--color", "always"], &[("TERM", "dumb")], 100);
        assert!(has_escape(&help));
    }

    /// **The help is wrapped to the terminal**: no line is wider than the terminal, however narrow, and the words are the same words.
    #[test]
    fn at_a_terminal_the_help_is_wrapped_to_its_width() {
        let words_of = |page: &str| -> Vec<String> { page.split_whitespace().map(str::to_owned).collect() };
        let plain = text(&run(&["--help"], &[]).stdout);
        for columns in [60u16, 80, 100] {
            let (help, status) = in_terminal(&["--help", "--color", "never"], &[], columns);
            assert_eq!(status, Some(0));
            let widest = help.lines().map(|l| l.chars().count()).max().unwrap();
            assert!(widest <= usize::from(columns), "{columns} columns: a line of {widest}");
            assert_eq!(words_of(&help), words_of(&plain), "{columns} columns: the words are the same");
            assert!(help.lines().count() > plain.lines().count() || columns >= 100, "{columns} columns");
        }
        // A pipe is never wrapped: the page is the same wherever the script runs.
        assert_eq!(plain, text(&run(&["--help"], &[("COLUMNS", "40")]).stdout));
    }

    #[test]
    fn at_a_terminal_a_mistake_is_coloured_on_the_message_only() {
        let (said, status) = in_terminal(&["export", "--bogus"], &[], 100);
        assert_eq!(status, Some(2));
        assert!(has_escape(&said), "{said}");
        assert!(strip_ansi(&said).starts_with("diacraft-cli: unknown option `--bogus` for `export`; see --help"), "{said}");
    }

    #[test]
    fn at_a_terminal_json_is_still_plain() {
        let (json, status) = in_terminal(&["run", "--cmd", "file.new"], &[("CLICOLOR_FORCE", "1")], 100);
        assert_eq!(status, Some(0));
        assert!(!has_escape(&json), "{json}");
        assert!(serde_json::from_str::<Value>(json.trim()).is_ok(), "{json}");
    }
}

// ------------------------------------------------------------------------------------------------------------------------ manual pages

#[test]
fn the_manual_pages_are_written_to_standard_output() {
    let out = run(&["man"], &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let page = text(&out.stdout);
    assert!(
        page.starts_with(".ie \\n(.g .ds Aq \\(aq") || page.contains(".TH diacraft-cli 1"),
        "{}",
        page.lines().take(5).collect::<Vec<_>>().join("\n")
    );
    assert!(page.contains("diacraft\\-cli"), "the page names the program");
    let list = text(&run(&["man", "--list"], &[]).stdout);
    let pages: Vec<&str> = list.lines().collect();
    assert_eq!(pages.first(), Some(&"diacraft-cli"), "{list}");
    for page in ["diacraft", "diacraft-cli-run", "diacraft-cli-export", "diacraft-cli-completions", "diacraft-cli-man"] {
        assert!(pages.contains(&page), "{page} among {pages:?}");
    }
    for name in pages {
        let out = run(&["man", name], &[]);
        assert!(out.status.success() && text(&out.stdout).contains(".TH"), "{name}: {}", text(&out.stderr));
        assert!(!has_escape(&text(&out.stdout)), "{name}");
    }
    for bad in [&["man", "nonesuch"][..], &["man", "run", "export"], &["man", "--list", "run"]] {
        let out = run(bad, &[]);
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
        assert!(out.stdout.is_empty(), "{bad:?}");
    }
}
