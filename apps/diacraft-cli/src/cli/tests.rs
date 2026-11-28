//! The command description: that it holds together, that it says what the help page says, and that the app's own options and the types of file are what the programs take.

use std::collections::BTreeSet;

use clap::ValueHint;

use super::*;
use crate::help::{Entry, PAGE};

fn registry() -> Vec<CommandId> {
    ["file.new", "page.add", "shape.drop"].iter().map(|id| CommandId { id: (*id).to_owned(), label: format!("The {id} command") }).collect()
}

/// The command line built, every subcommand with the arguments it takes from the program's.
fn built() -> Command {
    let mut command = command(None);
    command.build();
    command
}

#[test]
fn the_description_is_one_clap_accepts_with_and_without_the_registry() {
    // clap checks a description for mistakes (a name used twice, a short option taken) only in a debug build: this is the check.
    command(None).debug_assert();
    command(Some(&registry())).debug_assert();
    app_command().debug_assert();
}

#[test]
fn the_commands_are_the_ten_in_the_order_the_help_lists_them() {
    let command = built();
    let names: Vec<&str> = command.get_subcommands().map(Command::get_name).collect();
    assert_eq!(names, COMMANDS);
    assert_eq!(command.get_name(), PROGRAM);
    // The usage lines of the page begin with the same commands, in the same order.
    let mut listed: Vec<&str> = Vec::new();
    for entry in PAGE {
        if let Entry::Synopsis { text } = entry {
            let name = text.split_whitespace().nth(1).unwrap_or_default();
            if listed.last() != Some(&name) {
                listed.push(name);
            }
        }
    }
    assert_eq!(listed, COMMANDS);
}

/// The long options in `text`: each word that begins with two dashes and a lower case letter.
fn options_in(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .filter(|word| word.starts_with("--") && word.len() > 2 && !word.ends_with('-'))
        .map(str::to_owned)
        .collect()
}

/// What the page names for `command`: the options of its usage lines and of its rows.
fn page_options(command: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for entry in PAGE {
        match *entry {
            Entry::Synopsis { text } if text.split_whitespace().nth(1) == Some(command) => found.extend(options_in(text)),
            Entry::Row { key, label, .. } if key.strip_prefix(command).is_some_and(|rest| rest.starts_with('.')) => found.extend(options_in(label)),
            _ => {}
        }
    }
    found
}

#[test]
fn each_command_takes_the_options_its_usage_lines_and_rows_name_and_no_other() {
    let command = built();
    for sub in command.get_subcommands() {
        let grammar: BTreeSet<String> = sub
            .get_arguments()
            .filter_map(|arg| arg.get_long())
            .filter(|long| !["help", "version", "color"].contains(long))
            .map(|long| format!("--{long}"))
            .collect();
        assert_eq!(grammar, page_options(sub.get_name()), "`{}`: the grammar and the page differ", sub.get_name());
    }
    // `--color`, `--help` and `--version` are the program's: every command takes them.
    for sub in command.get_subcommands() {
        let longs: Vec<&str> = sub.get_arguments().filter_map(|arg| arg.get_long()).collect();
        for every in ["color", "help", "version"] {
            assert!(longs.contains(&every), "`{}` does not take --{every}", sub.get_name());
        }
    }
}

#[test]
fn every_command_and_every_option_has_words() {
    let command = built();
    assert!(command.get_about().is_some_and(|about| !about.to_string().is_empty()));
    for sub in command.get_subcommands() {
        assert!(sub.get_about().is_some_and(|about| !about.to_string().is_empty()), "`{}` has no description", sub.get_name());
        assert!(
            sub.get_long_about().is_some_and(|about| about.to_string().len() >= sub.get_about().map_or(0, |a| a.to_string().len())),
            "`{}`",
            sub.get_name()
        );
        for arg in sub.get_arguments() {
            assert!(arg.get_help().is_some_and(|help| !help.to_string().is_empty()), "`{}` {}: no words", sub.get_name(), arg.get_id());
        }
    }
}

#[test]
fn a_value_is_the_next_word_whatever_it_begins_with_and_an_option_is_given_once() {
    let command = built();
    for sub in command.get_subcommands() {
        for arg in sub.get_arguments().filter(|arg| arg.get_long().is_some() && arg.get_num_args().is_some_and(|n| n.takes_values())) {
            assert!(arg.is_allow_hyphen_values_set(), "`{}` {}: a value that begins with a dash is a value", sub.get_name(), arg.get_id());
        }
    }
    // Only the options that stand for steps (`run`'s) and for folders (`mcp`'s roots) may be given again.
    let repeatable: Vec<String> = command
        .get_subcommands()
        .flat_map(|sub| {
            sub.get_arguments()
                .filter(|arg| arg.get_long().is_some() && matches!(arg.get_action(), ArgAction::Append))
                .map(|arg| format!("{} {}", sub.get_name(), arg.get_id()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(repeatable, ["mcp automation-read-root", "mcp automation-write-root", "run cmd", "run params", "run commands"]);
}

/// **The roots complete folders** (meets): the two flags of `mcp` and the app's, given again for more folders, name a folder, so the scripts offer folders and no file.
#[test]
fn the_root_flags_of_mcp_and_of_the_app_name_folders_and_may_be_given_again() {
    let mcp = built();
    let app = app_command();
    let described = [mcp.find_subcommand("mcp").expect("mcp").get_arguments().collect::<Vec<_>>(), app.get_arguments().collect::<Vec<_>>()];
    for arguments in described {
        for id in ["automation-read-root", "automation-write-root"] {
            let arg = arguments.iter().find(|arg| arg.get_id() == id).unwrap_or_else(|| panic!("{id} is described"));
            assert_eq!(arg.get_value_hint(), ValueHint::DirPath, "{id}: a folder");
            assert!(matches!(arg.get_action(), ArgAction::Append), "{id}: given again for more folders");
        }
    }
}

#[test]
fn run_suggests_every_command_of_the_registry_and_accepts_any_text() {
    let command = command(Some(&registry()));
    let run = command.find_subcommand("run").expect("run is a command");
    let cmd = run.get_arguments().find(|arg| arg.get_id() == "cmd").expect("run has --cmd");
    let suggested: Vec<String> = cmd.get_possible_values().iter().map(|value| value.get_name().to_owned()).collect();
    assert_eq!(suggested, ["file.new", "page.add", "shape.drop"]);
    assert_eq!(cmd.get_possible_values()[0].get_help().map(ToString::to_string).as_deref(), Some("The file.new command"));
    // Nothing is checked against the list: an id that is not in it is the registry's to refuse, when the step runs.
    let matches = command.try_get_matches_from([PROGRAM, "run", "--cmd", "no.such.command"]).expect("any text is a value");
    assert_eq!(matches.subcommand_matches("run").and_then(|run| run.get_one::<String>("cmd")).map(String::as_str), Some("no.such.command"));
    // Without the registry the option suggests nothing.
    let bare = built();
    let cmd = bare.find_subcommand("run").and_then(|run| run.get_arguments().find(|arg| arg.get_id() == "cmd")).expect("run has --cmd");
    assert!(cmd.get_possible_values().is_empty());
}

#[test]
fn the_values_an_option_takes_are_suggested() {
    let command = built();
    let values = |sub: &str, id: &str| -> Vec<String> {
        let arg = command.find_subcommand(sub).and_then(|c| c.get_arguments().find(|a| a.get_id() == id)).expect("the option exists");
        arg.get_possible_values().iter().map(|value| value.get_name().to_owned()).collect()
    };
    assert_eq!(values("export", "text"), ["text", "outlines"]);
    assert_eq!(values("convert", "text"), ["text", "outlines"]);
    assert_eq!(values("roundtrip", "via"), ["diacraft"]);
    assert_eq!(values("roundtrip", "edit"), ["cell", "move", "retext", "delete", "add"]);
    assert_eq!(values("completions", "shell"), SHELLS);
    assert_eq!(values("completions", "program"), [PROGRAM, APP]);
    let color = command.get_arguments().find(|arg| arg.get_id() == "color").expect("the program takes --color");
    assert_eq!(color.get_possible_values().iter().map(|value| value.get_name()).collect::<Vec<_>>(), ["auto", "always", "never"]);
}

#[test]
fn the_file_types_name_arguments_that_exist_and_take_files() {
    let command = built();
    let app = app_command();
    for (command_name, arg_id, extensions) in FILE_TYPES {
        let args: Vec<&Arg> = if *command_name == APP {
            app.get_arguments().collect()
        } else {
            command.find_subcommand(command_name).unwrap_or_else(|| panic!("no command {command_name}")).get_arguments().collect()
        };
        let arg = args.iter().find(|arg| arg.get_id() == *arg_id).unwrap_or_else(|| panic!("{command_name} has no argument {arg_id}"));
        assert_eq!(arg.get_value_hint(), ValueHint::FilePath, "{command_name} {arg_id}");
        assert!(!extensions.is_empty(), "{command_name} {arg_id}");
        for extension in *extensions {
            assert!(extension.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{extension}");
        }
    }
    // A file the program reads or writes by one of these types is a drawing, a picture or a report: no argument lists a type it does not take.
    let pairs: BTreeSet<(&str, &str)> = FILE_TYPES.iter().map(|(command, arg, _)| (*command, *arg)).collect();
    assert_eq!(pairs.len(), FILE_TYPES.len(), "an argument is listed once");
}

/// The quoted words of the array `name` in `source`: `const NAME: [&str; 3] = ["a", "b", "c"];`.
fn quoted(source: &str, name: &str) -> Vec<String> {
    let at = source.find(&format!("const {name}")).unwrap_or_else(|| panic!("no {name}"));
    let rest = &source[at..];
    let open = rest.find("= [").map(|i| i + 3).expect("an array");
    let close = rest[open..].find(']').expect("the end of the array");
    rest[open..open + close].split('"').skip(1).step_by(2).map(str::to_owned).collect()
}

/// The types a drawing is read as are the types the app's Open dialog lists, which is what `file.open` takes (apps/diacraft/src/dialogs.rs).
#[test]
fn the_drawing_types_are_the_ones_the_apps_open_dialog_lists() {
    let source = include_str!("../../../diacraft/src/dialogs.rs");
    let mut dialog = quoted(source, "DIACRAFT_EXTENSIONS");
    dialog.extend(quoted(source, "OTHER_EXTENSIONS"));
    // The dialog lists the template before the stencil; the set is what counts.
    let mut drawings: Vec<String> = DRAWINGS.iter().map(|extension| (*extension).to_owned()).collect();
    drawings.sort();
    dialog.sort();
    assert_eq!(drawings, dialog);
}

/// The drawing types `convert` writes are the engine's, and the list of its `--out` here names them and the three pictures.
#[test]
fn the_types_convert_writes_are_the_types_its_out_completes() {
    let (_, _, listed) = FILE_TYPES.iter().find(|(command, arg, _)| *command == "convert" && *arg == "out").expect("convert --out is listed");
    let mut written: Vec<&str> = crate::convert::drawing_types().to_vec();
    written.extend(PICTURES);
    assert_eq!(*listed, written.as_slice());
}

/// The options in the help text of the app's parser (`apps/diacraft/src/args.rs`): the first word of each row, `--` aside.
fn app_usage_options() -> BTreeSet<String> {
    let source = include_str!("../../../diacraft/src/args.rs");
    let start = source.find("pub const USAGE: &str = \"\\").expect("the app's usage text");
    let usage = &source[start..start + source[start..].find("\";\n").expect("its end")];
    usage
        .lines()
        .filter(|line| line.starts_with("  -"))
        .flat_map(|line| {
            line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .filter(|word| word.starts_with('-') && word.len() > 2 && word.as_bytes().get(1) == Some(&b'-'))
        })
        .map(str::to_owned)
        .collect()
}

/// **The app's script and the app's parser say the same options.** `apps/diacraft/src/args.rs` is not a dependency of this crate (the window's crates are not the command line's), so a test
/// reads its help: an option added there, `--automation-read-root` and the like, fails this until the description of the app here has it too.
#[test]
fn the_apps_description_has_the_options_the_apps_help_has() {
    let described: BTreeSet<String> = app_command().get_arguments().filter_map(|arg| arg.get_long()).map(|long| format!("--{long}")).collect();
    assert_eq!(described, app_usage_options());
    assert_eq!(app_command().get_name(), APP);
}

#[test]
fn the_apps_file_is_one_and_every_file_takes_a_drawing() {
    let app = app_command();
    let file = app.get_arguments().find(|arg| arg.get_id() == "file").expect("the app takes a file");
    assert_eq!(file.get_value_hint(), ValueHint::FilePath);
}
