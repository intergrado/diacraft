//! Makes the file arguments of a completion script complete by their types: a drawing where a drawing is read, a picture where a picture is written, and always the
//! folders, so that a path can be walked. clap_complete knows a file argument only as "a file", and offers every file.
//!
//! The types are [`crate::cli::FILE_TYPES`]. **Bash** gets code added after the script, which looks at the line first and hands what it does not know to the script's own function:
//! nothing of clap's text is changed, so a new clap cannot break it. **Fish** gets the same, and the one line that forces every file for an option is made to ask for the types.
//! **Zsh** has the type in the action of the argument (`_files -g PATTERN`), which is a change to that line of clap's. **PowerShell** gets the values of the options that take a few (and
//! the command ids of `run --cmd`), which clap_complete leaves out of its script, from a block put before the script's own `switch`; its files are completed as the shell does, as are
//! Elvish's, which gets nothing: a script that Elvish would have to read here cannot be tried on this project's machines.
//! A change that finds nothing to change (clap wrote the script differently) is left out and a test fails, so that it is noticed when clap changes and not when a person tries.

use clap::{Arg, Command};
use clap_complete::Shell;

use crate::args::Program;
use crate::cli::{self, FILE_TYPES};

/// One argument that names a file, with the types that suit it.
struct FileArg {
    /// The command (empty for the app, which has none).
    command: &'static str,
    /// The long name of the option (`out`), or `None` for the words after the options.
    long: Option<String>,
    extensions: &'static [&'static str],
}

/// What a command's value options are, so that a word after one is known not to be a file: the long names of the options that take a value.
fn value_options(command: &Command) -> Vec<String> {
    command
        .get_arguments()
        .filter(|arg| arg.get_long().is_some() && arg.get_num_args().is_some_and(|n| n.takes_values()))
        .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
        .collect()
}

/// The file arguments of a program, by command.
fn file_args(program: Program, command: &Command) -> Vec<(FileArg, Vec<String>)> {
    let mut found = Vec::new();
    for (name, id, extensions) in FILE_TYPES {
        let scope = match program {
            Program::App if *name == cli::APP => Some(command),
            Program::Cli if *name != cli::APP => command.find_subcommand(name),
            _ => None,
        };
        let Some(scope) = scope else { continue };
        let Some(arg) = scope.get_arguments().find(|arg: &&Arg| arg.get_id() == *id) else { continue };
        let long = arg.get_long().map(str::to_owned);
        let command_name = if program == Program::App { "" } else { name };
        found.push((FileArg { command: command_name, long, extensions }, value_options(scope)));
    }
    found
}

/// `script` with the file arguments of `program` completed by their types, for `shell`.
pub fn files(shell: Shell, program: Program, command: &Command, script: String) -> String {
    let args = file_args(program, command);
    match shell {
        Shell::Bash => bash(script, program, &args),
        Shell::Zsh => zsh(script, program, &args),
        Shell::Fish => fish(script, program, command, &args),
        Shell::PowerShell => powershell(script, program, command),
        _ => script,
    }
}

fn name(program: Program) -> &'static str {
    match program {
        Program::Cli => cli::PROGRAM,
        Program::App => cli::APP,
    }
}

/// The glob of the files of these types, case blind: `*.[vV][sS]...` for each, as bash's `@(a|b)` and zsh's `(a|b)` write a choice. `open` is what begins the choice (`@` for bash).
fn glob(extensions: &[&str], open: &str) -> String {
    let choice: Vec<String> = extensions
        .iter()
        .map(|extension| {
            extension
                .chars()
                .map(|c| if c.is_ascii_alphabetic() { format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase()) } else { c.to_string() })
                .collect()
        })
        .collect();
    format!("*.{open}({})", choice.join("|"))
}

// ------------------------------------------------------------------------------------------------------------------------ bash

fn bash(mut script: String, program: Program, args: &[(FileArg, Vec<String>)]) -> String {
    let name = name(program);
    let mut cases = String::new();
    let mut commands: Vec<&str> = args.iter().map(|(arg, _)| arg.command).collect();
    commands.dedup();
    for command in commands {
        let of_command: Vec<&(FileArg, Vec<String>)> = args.iter().filter(|(arg, _)| arg.command == command).collect();
        let options = of_command.first().map(|(_, options)| options.clone()).unwrap_or_default();
        let mut body = String::new();
        let mut positional = String::new();
        for (arg, _) in &of_command {
            let pattern = glob(arg.extensions, "@");
            match &arg.long {
                Some(long) => body.push_str(&format!("            --{long}) _{name}_by_type \"$cur\" '{pattern}'; return 0 ;;\n")),
                None => positional = pattern,
            }
        }
        let others: Vec<&String> = options
            .iter()
            .filter(|option| !of_command.iter().any(|(arg, _)| arg.long.as_deref().is_some_and(|long| **option == format!("--{long}"))))
            .collect();
        if !others.is_empty() {
            body.push_str(&format!("            {}) ;;\n", others.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(" | ")));
        }
        if !positional.is_empty() {
            body.push_str(&format!("            *) if [[ $cur != -* ]]; then _{name}_by_type \"$cur\" '{positional}'; return 0; fi ;;\n"));
        }
        if command.is_empty() {
            cases.push_str(&format!("    case \"$prev\" in\n{}    esac\n", body.replace("            ", "        ")));
        } else {
            cases.push_str(&format!("        {command})\n            case \"$prev\" in\n{body}            esac\n            ;;\n"));
        }
    }
    let dispatch = if program == Program::App {
        cases
    } else {
        format!(
            "    local cmd=\"\" i word\n    for ((i = 1; i < COMP_CWORD; i++)); do\n        word=${{COMP_WORDS[i]}}\n        case $word in\n            --color) i=$((i + 1)) ;;\n            -*) ;;\n            *) cmd=$word; break ;;\n        esac\n    done\n    case \"$cmd\" in\n{cases}    esac\n"
        )
    };
    script.push_str(&format!(
        r#"
# File arguments by their types, which the function above leaves to bash: a drawing where one is read, a picture where one is written, and always the folders.
_{name}_by_type() {{
    local cur=$1 pattern=$2 oldifs=$IFS had_extglob=0
    shopt -q extglob && had_extglob=1 || shopt -s extglob
    IFS=$'\n'
    COMPREPLY=($(compgen -d -- "$cur") $(compgen -f -X "!$pattern" -- "$cur"))
    IFS=$oldifs
    [[ $had_extglob -eq 1 ]] || shopt -u extglob
    if [[ "${{BASH_VERSINFO[0]}}" -ge 4 ]]; then
        compopt -o filenames
    fi
}}

_{name}_typed() {{
    local cur=${{COMP_WORDS[COMP_CWORD]}} prev=${{COMP_WORDS[COMP_CWORD - 1]}}
{dispatch}    _{name} "$@"
}}

if [[ "${{BASH_VERSINFO[0]}}" -eq 4 && "${{BASH_VERSINFO[1]}}" -ge 4 || "${{BASH_VERSINFO[0]}}" -gt 4 ]]; then
    complete -F _{name}_typed -o nosort -o bashdefault -o default {name}
else
    complete -F _{name}_typed -o bashdefault -o default {name}
fi
"#
    ));
    script
}

// ------------------------------------------------------------------------------------------------------------------------ zsh

/// The `_arguments` line of a file argument ends in the action `_files`: with the types it is `_files -g PATTERN`, which lists the folders as well.
fn zsh(script: String, program: Program, args: &[(FileArg, Vec<String>)]) -> String {
    let mut block = String::new();
    let mut out = String::with_capacity(script.len() + 1024);
    for line in script.lines() {
        // The app has no commands: its arguments are in the first `_arguments`. A command's begin on the line `(command)`.
        if let Some(command) = line.strip_prefix('(').and_then(|rest| rest.strip_suffix(')')) {
            block = command.to_owned();
        }
        let in_block = match program {
            Program::App => true,
            Program::Cli => !block.is_empty(),
        };
        let mut edited = None;
        if in_block && line.ends_with(":_files' \\") {
            for (arg, _) in args.iter().filter(|(arg, _)| program == Program::App || arg.command == block) {
                let starts = match &arg.long {
                    Some(long) => line.starts_with(&format!("'--{long}=[")) || line.starts_with(&format!("'*--{long}=[")),
                    None => line.starts_with("'*::") || line.starts_with("'::") || line.starts_with("':"),
                };
                if starts {
                    let pattern = glob(arg.extensions, "");
                    edited = Some(format!("{}:_files -g \"{pattern}\"' \\", &line[..line.len() - ":_files' \\".len()]));
                    break;
                }
            }
        }
        out.push_str(edited.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------ powershell

/// A PowerShell single-quoted string.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The values of an argument, as PowerShell pairs of the word and what to say beside it.
fn pairs(arg: &Arg) -> Option<String> {
    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| {
            format!(
                "@({}, {})",
                quoted(value.get_name()),
                quoted(&value.get_help().map(ToString::to_string).unwrap_or_else(|| value.get_name().to_owned()))
            )
        })
        .collect();
    match values.len() {
        0 => None,
        1 => Some(format!("@(,{})", values.join(""))),
        _ => Some(format!("@({})", values.join(", "))),
    }
}

/// PowerShell: before the script's own `switch`, a block that looks at the word before the one being completed: after an option that takes a few values (`--text`), or at the
/// first word after `completions` and `man`, it answers with them and stops. `$command` is the commands so far joined by `;`, as the script builds it.
fn powershell(script: String, program: Program, command: &Command) -> String {
    let anchor = "    $completions = @(switch ($command) {";
    if !script.contains(anchor) {
        return script;
    }
    let name = name(program);
    let mut cases = String::new();
    let mut add = |pattern: String, arg: &Arg| {
        if let Some(values) = pairs(arg) {
            cases.push_str(&format!("        {} {{ return (& $offer {values}) }}\n", quoted(&pattern)));
        }
    };
    // The options of the whole program, once; then each command's own, and the words after the options of a command that are a few to choose from.
    let global = |arg: &&Arg| arg.is_global_set();
    for arg in command.get_arguments().filter(global) {
        if let Some(long) = arg.get_long() {
            add(format!("* --{long}"), arg);
        }
    }
    let scopes: Vec<(String, &Command)> = match program {
        Program::Cli => command.get_subcommands().map(|sub| (format!("{name};{}", sub.get_name()), sub)).collect(),
        Program::App => vec![(name.to_owned(), command)],
    };
    for (path, scope) in &scopes {
        for arg in scope.get_arguments().filter(|arg| !arg.is_global_set()) {
            match arg.get_long() {
                // The script's `$command` goes on with every word that is no option, a file included: the path is followed by any of them.
                Some(long) => add(format!("{path}* --{long}"), arg),
                None if program == Program::Cli => add(format!("{path} {}", scope.get_name()), arg),
                None => {}
            }
        }
    }
    let block = format!(
        r#"    # The values of the options that take a few, and the command ids of `run --cmd`, which the script below does not offer (added by the `completions` command).
    $words = @($commandElements | ForEach-Object {{ $_.Extent.Text }})
    $previous = if ($wordToComplete -eq '') {{ $words[-1] }} elseif ($words.Count -ge 2) {{ $words[-2] }} else {{ '' }}
    $offer = {{
        param($pairs)
        foreach ($pair in $pairs) {{
            if ($pair[0] -like "$wordToComplete*") {{
                [CompletionResult]::new($pair[0], $pair[0], [CompletionResultType]::ParameterValue, $pair[1])
            }}
        }}
    }}
    switch -Wildcard ("$command $previous") {{
{cases}    }}

"#
    );
    script.replacen(anchor, &format!("{block}{anchor}"), 1)
}

// ------------------------------------------------------------------------------------------------------------------------ fish

fn fish(mut script: String, program: Program, command: &Command, args: &[(FileArg, Vec<String>)]) -> String {
    let name = name(program);
    let function = format!("__fish_{}_by_type", name.replace('-', "_"));
    let using = format!("__fish_{}_using_subcommand", name.replace('-', "_"));
    let mut added = String::new();
    for (arg, _) in args {
        let words = arg.extensions.join(" ");
        match &arg.long {
            Some(long) => {
                // The line that forces every file for the option asks for the types instead.
                let head = format!("complete -c {name} -n \"{using} {}\" -l {long} ", arg.command);
                let lines: Vec<String> = script
                    .lines()
                    .map(|line| match line.strip_prefix(&head).and_then(|rest| rest.strip_suffix(" -r -F")) {
                        Some(middle) => format!("{head}{middle} -r -f -a \"({function} {words})\""),
                        None => line.to_owned(),
                    })
                    .collect();
                script = format!("{}\n", lines.join("\n"));
            }
            None if program == Program::App => added.push_str(&format!("complete -c {name} -f -a \"({function} {words})\"\n")),
            None => added.push_str(&format!("complete -c {name} -n \"{using} {}\" -f -a \"({function} {words})\"\n", arg.command)),
        }
    }
    // The words after the options that are a few to choose from (the shells of `completions`, the pages of `man`): clap_complete leaves them out of a fish script.
    for sub in command.get_subcommands() {
        for positional in sub.get_positionals() {
            let values: Vec<String> =
                positional.get_possible_values().iter().filter(|value| !value.is_hide_set()).map(|value| value.get_name().to_owned()).collect();
            if !values.is_empty() {
                added.push_str(&format!("complete -c {name} -n \"{using} {}\" -f -a \"{}\"\n", sub.get_name(), values.join(" ")));
            }
        }
    }
    script.push_str(&format!(
        r#"
# File arguments by their types: the folders, and the files that end in one of the types (in any case).
function {function}
    set -l token (commandline -ct | string replace -r -- '^-[^=]*=' '')
    set -l types (string escape --style=regex -- $argv | string join '|')
    for item in (complete -C "__fish_command_without_completions $token" 2>/dev/null)
        if string match -qr -- '/(\t|$)' $item; or string match -qir -- '\.('$types')(\t|$)' $item
            echo $item
        end
    end
end
"#
    ));
    script.push_str(&added);
    script
}
