//! The completion scripts, as text; `tests/it/ux.rs` runs the shells on them.

use clap::ValueEnum;
use clap_complete::Shell;

use super::*;

fn script_for(shell: Shell, program: Program) -> String {
    script(&CompletionsArgs { shell, program }).unwrap_or_else(|e| panic!("{shell}: {e}"))
}

#[test]
fn every_shell_gets_a_script_that_names_the_program_and_its_commands() {
    for &shell in Shell::value_variants() {
        let text = script_for(shell, Program::Cli);
        assert!(text.contains("diacraft-cli") && text.contains("roundtrip") && text.contains("completions"), "{shell}");
        assert!(!text.contains('\u{1b}'), "{shell}: a terminal escape");
    }
    assert_eq!(Shell::value_variants().len(), 5, "the five shells the help names");
}

#[test]
fn the_scripts_list_every_command_of_the_registry_for_run() {
    let ids: Vec<String> = Session::new().commands().into_iter().map(|c| c.id).collect();
    assert!(ids.len() > 100);
    // Elvish gets no values from clap_complete, the ids among them.
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        let text = script_for(shell, Program::Cli);
        let missing: Vec<&String> = ids.iter().filter(|id| !text.contains(id.as_str())).collect();
        assert!(missing.is_empty(), "{shell}: {missing:?}");
    }
}

/// **The scripts complete the roots** (meets): the two flags are in the script of `mcp` and in the app's, for every shell, and in no other command's.
#[test]
fn every_script_has_the_root_flags_of_mcp_and_of_the_app() {
    for &shell in Shell::value_variants() {
        for program in [Program::Cli, Program::App] {
            let text = script_for(shell, program);
            assert!(text.contains("automation-read-root") && text.contains("automation-write-root"), "{shell} {program:?}");
        }
    }
    // Only `mcp` has them among the commands: the part of the description of any other command holds neither word.
    let command = cli::command(None);
    for sub in command.get_subcommands().filter(|sub| sub.get_name() != "mcp") {
        assert!(sub.get_arguments().all(|arg| !arg.get_id().as_str().starts_with("automation-")), "`{}` has no roots", sub.get_name());
    }
}

#[test]
fn the_apps_script_is_the_apps() {
    for &shell in Shell::value_variants() {
        let text = script_for(shell, Program::App);
        assert!(text.contains("control") && text.contains("diacraft"), "{shell}");
        assert!(!text.contains("roundtrip") && !text.contains("diacraft-cli"), "{shell}");
    }
}

#[test]
fn a_registry_of_labels_with_quotes_and_symbols_still_makes_a_script() {
    // The labels the registry has hold `&`; a label may hold more, and no shell may be made to run it.
    let hostile = [("a.b", "Snap & Glue"), ("c.d", "it's \"quoted\" $(touch x) `x`; \\ 100%"), ("e.f", "line\nbreak")];
    let ids: Vec<CommandId> = hostile.iter().map(|(id, label)| CommandId { id: (*id).to_owned(), label: (*label).to_owned() }).collect();
    for &shell in Shell::value_variants() {
        let mut command = cli::command(Some(&ids));
        let mut written = Vec::new();
        clap_complete::generate(shell, &mut command, cli::PROGRAM, &mut written);
        assert!(!written.is_empty(), "{shell}");
    }
}
