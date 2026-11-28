//! The command line as clap describes it: the one description that the parser ([`crate::args`]), the completion scripts ([`crate::completions`]) and the manual pages
//! ([`crate::man`]) all read, so that what the program completes and documents is what it accepts.
//!
//! Only clap's builder is used, and none of what it prints: the help page is DiaCraft's own table (`crate::help`), the errors are said in `args.rs`'s words, and the colour is
//! `crate::term`'s. What clap is asked for is the grammar: which words are commands, which are options and what each takes, with the sentences of the table as descriptions.
//!
//! **Strictness is the grammar's.** An option a command does not have, a value left out and an option given twice are errors (clap says so, and `args.rs` puts it in words); a value is
//! the next word whatever it begins with, as it has always been (`--out --bundled-fonts` names a file called `--bundled-fonts`); `--` ends the options, for every command. What
//! clap does not check, the values (`--page`, `--dpi`, `--text`...) and the combinations, `args.rs` checks after it, in the words the docs have always had: every value option takes
//! any text here and says what it suggests ([`Suggest`]), which is all a completion script needs.
//!
//! **The apps' options.** `diacraft` has options of its own ([`app_command`]), and its completion script comes from here too, so that one command writes both. The app keeps its
//! parser in `apps/diacraft/src/args.rs`, which this crate does not depend on (the window's crates are not the command line's): a test reads that file's help and holds the two together.

use std::ffi::OsStr;

use clap::builder::{PossibleValue, TypedValueParser};
use clap::{Arg, ArgAction, Command, ValueHint};

use crate::help::{describe, summary};
use crate::term::When;

/// The program's name: the command line's.
pub const PROGRAM: &str = "diacraft-cli";

/// The desktop app's name.
pub const APP: &str = "diacraft";

/// The commands, in the order the help lists them.
pub const COMMANDS: [&str; 10] = ["mcp", "run", "commands", "oracle", "export", "convert", "roundtrip", "compare", "completions", "man"];

/// The shells the completions are written for, in the order the help names them.
pub const SHELLS: [&str; 5] = ["bash", "zsh", "fish", "powershell", "elvish"];

/// The types of drawing, stencil and template that `file.open` reads.
pub const DRAWINGS: [&str; 12] =
    ["diacraft", "diacraft-stencil", "diacraft-template", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"];

/// The types of picture `export` writes.
pub const PICTURES: [&str; 3] = ["png", "svg", "pdf"];

/// What a completion script offers where a command takes a file, by the type of file, as (command, argument, extensions in lower case without the dot). The arguments of no row
/// complete every file: a script of steps, which can be called anything. A drawing is any type [`DRAWINGS`] names; `oracle` and `compare` read the types their help says.
pub const FILE_TYPES: &[(&str, &str, &[&str])] = &[
    ("export", "file", &DRAWINGS),
    ("export", "out", &PICTURES),
    ("convert", "file", &DRAWINGS),
    (
        "convert",
        "out",
        &["diacraft", "diacraft-stencil", "diacraft-template", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "png", "svg", "pdf"],
    ),
    ("roundtrip", "file", &DRAWINGS),
    ("roundtrip", "out", &["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"]),
    ("oracle", "file", &["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "jsonl", "json"]),
    ("compare", "file", &["vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "odg", "otg", "fodg"]),
    (APP, "file", &DRAWINGS),
];

/// A command of the registry: its id and its label, which a completion script lists as a value of `run --cmd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandId {
    pub id: String,
    pub label: String,
}

/// A value parser that takes any text and says which texts it would suggest: all a completion script reads of it. The value is checked, in its own words, after the line is read.
#[derive(Clone, Debug)]
pub struct Suggest(Vec<PossibleValue>);

impl Suggest {
    /// Suggests each of `words`.
    pub fn words(words: &[&'static str]) -> Suggest {
        Suggest(words.iter().map(|word| PossibleValue::new(*word)).collect())
    }
}

impl Suggest {
    /// Suggests the names of the manual pages (`man.rs` lists them, and this is where they are made).
    pub fn pages() -> Suggest {
        let mut names = vec![PROGRAM.to_owned(), APP.to_owned()];
        names.extend(COMMANDS.iter().map(|command| format!("{PROGRAM}-{command}")));
        Suggest(names.into_iter().map(PossibleValue::new).collect())
    }
}

impl TypedValueParser for Suggest {
    type Value = String;

    fn parse_ref(&self, _command: &Command, _arg: Option<&Arg>, value: &OsStr) -> Result<String, clap::Error> {
        Ok(value.to_string_lossy().into_owned())
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(self.0.iter().cloned()))
    }
}

/// `arg` described by the table's words for `key`: the first sentence as its short help, which a completion script lists, and the whole as its long help, which a manual page gives.
fn described(arg: Arg, key: &str) -> Arg {
    let (short, full) = (summary(key), describe(key));
    let arg = arg.help(short.clone());
    if full == short { arg } else { arg.long_help(full) }
}

/// A flag of `command`: given once, takes no value.
fn flag(command: &str, id: &'static str) -> Arg {
    described(Arg::new(id).long(id).action(ArgAction::SetTrue), &format!("{command}.{id}"))
}

/// An option of `command` that takes one value, the next word whatever it begins with. It completes no file unless it is told it names one.
fn option(command: &str, id: &'static str, name: &'static str) -> Arg {
    described(
        Arg::new(id).long(id).value_name(name).num_args(1).action(ArgAction::Set).allow_hyphen_values(true).value_hint(ValueHint::Other),
        &format!("{command}.{id}"),
    )
}

/// An option that may be given again, its values in order.
fn repeated(command: &str, id: &'static str, name: &'static str) -> Arg {
    option(command, id, name).action(ArgAction::Append)
}

/// The words after the options: any number of them, and the command counts them and says what is wrong in its own words.
fn words(id: &'static str, name: &'static str, help: &'static str) -> Arg {
    Arg::new(id).value_name(name).num_args(0..).action(ArgAction::Append).value_hint(ValueHint::Other).help(help)
}

fn bundled_fonts(command: &str) -> Arg {
    flag(command, "bundled-fonts")
}

/// A command of the program, with the settings every one has.
fn sub(name: &'static str, about: &'static str) -> Command {
    Command::new(name)
        .about(about)
        .long_about(paragraphs(name, about))
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
}

/// The words of a command's description: its paragraphs of the page, or its one line.
fn paragraphs(name: &str, about: &'static str) -> String {
    let paragraphs = crate::help::paragraphs(name);
    if paragraphs.is_empty() { about.to_owned() } else { paragraphs.join("\n\n") }
}

/// The type of file an argument names, as a completion hint: files, and the folders to walk to them.
fn file_arg(arg: Arg) -> Arg {
    arg.value_hint(ValueHint::FilePath)
}

/// The whole command line. With `registry`, `run --cmd` suggests each command id of the registry (what `commands` lists, with its label); without, it suggests none: the parser has
/// no use for them and a run of the program need not build a session to list them.
pub fn command(registry: Option<&[CommandId]>) -> Command {
    let ids: Option<Vec<PossibleValue>> =
        registry.map(|ids| ids.iter().map(|command| PossibleValue::new(command.id.clone()).help(command.label.clone())).collect());
    let run_cmd = match ids {
        Some(ids) => repeated("run", "cmd", "ID").value_parser(Suggest(ids)).hide_possible_values(true),
        None => repeated("run", "cmd", "ID"),
    };
    Command::new(PROGRAM)
        .bin_name(PROGRAM)
        .version(env!("CARGO_PKG_VERSION"))
        .about("DiaCraft's command line: an MCP server, headless runs of commands, and tools to convert, export and check drawings")
        .long_about(
            "diacraft-cli runs DiaCraft's commands without a window, for scripts and for agents. It reaches the same registry of commands as the menus and the control channel. \
             Results go to standard output and nothing else does: messages for people, and the log, go to standard error.",
        )
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .arg(described(Arg::new("help").short('h').long("help").action(ArgAction::SetTrue).global(true), "options.help"))
        .arg(described(Arg::new("version").short('V').long("version").action(ArgAction::SetTrue).global(true), "options.version"))
        .arg(described(
            Arg::new("color")
                .long("color")
                .value_name("WHEN")
                .num_args(1)
                .action(ArgAction::Set)
                .allow_hyphen_values(true)
                .global(true)
                .value_hint(ValueHint::Other)
                .value_parser(Suggest::words(&When::WORDS)),
            "options.color",
        ))
        .subcommand(
            sub("mcp", "Run the MCP server on standard input and output")
                .arg(flag("mcp", "headless"))
                .arg(option("mcp", "connect", "ADDR").value_parser(Suggest::words(&["127.0.0.1:7997"])).hide_possible_values(true))
                .arg(repeated("mcp", "automation-read-root", "DIR").value_hint(ValueHint::DirPath))
                .arg(repeated("mcp", "automation-write-root", "DIR").value_hint(ValueHint::DirPath))
                .arg(bundled_fonts("mcp")),
        )
        .subcommand(
            sub("run", "Run commands one after another in a new headless session")
                .arg(run_cmd)
                .arg(repeated("run", "params", "JSON"))
                .arg(repeated("run", "commands", "SRC").value_hint(ValueHint::FilePath))
                .arg(bundled_fonts("run")),
        )
        .subcommand(sub("commands", "Print every command, with its parameter doc and whether it can run, as JSON"))
        .subcommand(
            sub("oracle", "Score the recalculation of drawings against the values they saved")
                .arg(file_arg(words("file", "FILE", "the drawings to report on, or with --aggregate the reports to total")))
                .arg(flag("oracle", "json"))
                .arg(option("oracle", "source", "NAME"))
                .arg(flag("oracle", "aggregate"))
                .arg(option("oracle", "cap", "N").help("with --aggregate, the most files one source adds to the capped figure (5 by default)")),
        )
        .subcommand(
            sub("export", "Write one page of a drawing as a PNG, an SVG or a PDF")
                .arg(file_arg(words("file", "FILE", "the drawing to read")))
                .arg(option("export", "out", "PATH").value_hint(ValueHint::FilePath))
                .arg(option("export", "page", "N"))
                .arg(option("export", "dpi", "N").value_parser(Suggest::words(&["96", "150", "300", "600"])).hide_possible_values(true))
                .arg(option("export", "text", "MODE").value_parser(Suggest::words(&["text", "outlines"])))
                .arg(flag("export", "transparent"))
                .arg(bundled_fonts("export")),
        )
        .subcommand(
            sub("convert", "Write a drawing as another type, or as a picture")
                .arg(file_arg(words("file", "FILE", "the drawing, stencil or template to read")))
                .arg(option("convert", "out", "PATH").value_hint(ValueHint::FilePath))
                .arg(flag("convert", "drop-macros"))
                .arg(flag("convert", "drop-objects"))
                .arg(option("convert", "page", "N"))
                .arg(option("convert", "dpi", "N").value_parser(Suggest::words(&["96", "150", "300", "600"])).hide_possible_values(true))
                .arg(option("convert", "text", "MODE").value_parser(Suggest::words(&["text", "outlines"])))
                .arg(flag("convert", "transparent"))
                .arg(bundled_fonts("convert")),
        )
        .subcommand(
            sub("roundtrip", "Open a drawing, save it, and report what the save changed")
                .arg(file_arg(words("file", "FILE", "the drawing, stencil or template to open")))
                .arg(option("roundtrip", "out", "PATH").value_hint(ValueHint::FilePath))
                .arg(option("roundtrip", "move", "DX,DY"))
                .arg(option("roundtrip", "via", "FORMAT").value_parser(Suggest::words(&["diacraft"])))
                .arg(option("roundtrip", "edit", "KIND").value_parser(Suggest::words(&["cell", "move", "retext", "delete", "add"])))
                .arg(bundled_fonts("roundtrip")),
        )
        .subcommand(
            sub("compare", "Compare two drawings")
                .arg(file_arg(words("file", "FILE", "the two drawings, first and second")))
                .arg(flag("compare", "content").help("check the two files for the same content: two VSDX packages, or two OpenDocument drawings")),
        )
        .subcommand(
            sub("completions", "Print the completion script of a shell")
                .arg(Arg::new("shell").value_name("SHELL").num_args(0..).action(ArgAction::Append).value_parser(Suggest::words(&SHELLS)).help("bash, zsh, fish, powershell or elvish"))
                .arg(option("completions", "program", "NAME").value_parser(Suggest::words(&[PROGRAM, APP]))),
        )
        .subcommand(
            sub("man", "Print a manual page")
                .arg(words("page", "PAGE", "the page: diacraft-cli, a command of it, or diacraft").value_parser(Suggest::pages()))
                .arg(flag("man", "list")),
        )
}

/// The desktop app's command line, for its completion script: the options `apps/diacraft/src/args.rs` takes.
pub fn app_command() -> Command {
    Command::new(APP)
        .bin_name(APP)
        .version(env!("CARGO_PKG_VERSION"))
        .about("DiaCraft, the diagram editor: opens the window on a drawing, and can start the control server")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .arg(
            Arg::new("control")
                .long("control")
                .value_name("PORT")
                .num_args(1)
                .action(ArgAction::Set)
                .value_parser(Suggest::words(&["7997"]))
                .value_hint(ValueHint::Other)
                .hide_possible_values(true)
                .help("Start the control server on 127.0.0.1:PORT, a port from 1 to 65535 (or DIACRAFT_CONTROL_PORT)"),
        )
        .arg(
            Arg::new("automation-read-root")
                .long("automation-read-root")
                .value_name("FOLDER")
                .num_args(1)
                .action(ArgAction::Append)
                .value_hint(ValueHint::DirPath)
                .help(
                    "Keep the files the control channel reads to this folder and the folders in it; give it again for more folders (needs --control)",
                ),
        )
        .arg(
            Arg::new("automation-write-root")
                .long("automation-write-root")
                .value_name("FOLDER")
                .num_args(1)
                .action(ArgAction::Append)
                .value_hint(ValueHint::DirPath)
                .help("The same for the files the control channel writes: a save, an export, a print or a screenshot (needs --control)"),
        )
        .arg(
            Arg::new("reset-graphics")
                .long("reset-graphics")
                .action(ArgAction::SetTrue)
                .help("Start after forgetting the graphics adapters that earlier starts left out, so that every one is tested again"),
        )
        .arg(Arg::new("version").long("version").action(ArgAction::SetTrue).help("Print the version and exit"))
        .arg(Arg::new("help").short('h').long("help").action(ArgAction::SetTrue).help("Print this help and exit"))
        .arg(Arg::new("file").value_name("FILE").num_args(0..).action(ArgAction::Append).value_hint(ValueHint::FilePath).help("the drawing to open"))
}

#[cfg(test)]
mod tests;
