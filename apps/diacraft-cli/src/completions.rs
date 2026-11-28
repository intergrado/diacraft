//! `diacraft-cli completions SHELL`: the completion script of a shell, on standard output.
//!
//! clap_complete writes the script from the command description of [`crate::cli`] (held to 4.5: 4.6 breaks bash completion of the commands of a program with a hyphen in its
//! name, and a test runs bash on the script). The script completes the commands, their options, the values an option takes and, for `run --cmd`, every command id of the registry
//! (the list `commands` prints, with each label). `refine` then makes the file arguments complete by their types: a drawing where a drawing is read, a picture where a picture is
//! written, and always the folders.

use diacraft_engine::Session;

use crate::args::{CompletionsArgs, Program};
use crate::cli::{self, CommandId};

mod refine;

/// The script `args` asks for.
pub fn script(args: &CompletionsArgs) -> Result<String, String> {
    let (mut command, name) = match args.program {
        Program::Cli => (cli::command(Some(&registry())), cli::PROGRAM),
        Program::App => (cli::app_command(), cli::APP),
    };
    let mut written = Vec::new();
    clap_complete::generate(args.shell, &mut command, name, &mut written);
    let text = String::from_utf8(written).map_err(|_| "the completion script is not text".to_owned())?;
    Ok(refine::files(args.shell, args.program, &command, text))
}

/// The commands of the registry, in the order `commands` lists them.
fn registry() -> Vec<CommandId> {
    Session::new().commands().into_iter().map(|command| CommandId { id: command.id, label: command.label }).collect()
}

#[cfg(test)]
mod tests;
