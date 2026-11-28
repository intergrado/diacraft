//! `diacraft-cli man`: the manual pages, from the command description of [`crate::cli`].
//!
//! The pages are `diacraft-cli` (the program and the list of its commands), one for each command (`diacraft-cli-run`, `diacraft-cli-export` and the rest, the names `git`'s pages
//! have) and `diacraft` (the app). Each is written in the format the `man` program reads, on standard output, and nothing else: `diacraft-cli man | man -l -` reads one, and the
//! Linux package writes them all (`packaging/linux/make-deb.sh`, which asks for the names with `--list`). The words are the help page's own sentences, by key: a page says what
//! the page says, and a change of the table changes both.

use crate::args::ManArgs;
use crate::cli;

/// The names of the pages: the program's first, the app's, and one for each command.
pub fn pages() -> Vec<String> {
    let mut names = vec![cli::PROGRAM.to_owned(), cli::APP.to_owned()];
    names.extend(cli::COMMANDS.iter().map(|command| format!("{}-{command}", cli::PROGRAM)));
    names
}

/// What `man` prints: the page, or the list of names, one on a line.
pub fn output(args: &ManArgs) -> Result<String, String> {
    match args {
        ManArgs::List => Ok(pages().iter().map(|name| format!("{name}\n")).collect()),
        ManArgs::Page(name) => page(name),
    }
}

/// The page called `name`, in the format `man` reads.
fn page(name: &str) -> Result<String, String> {
    let mut whole = if name == cli::APP { cli::app_command() } else { cli::command(None) };
    // Built, so that each command knows its place and its full name (`diacraft-cli-run`), which is the title of its page.
    whole.build();
    let command = if name == cli::PROGRAM || name == cli::APP {
        whole
    } else {
        let command = name.strip_prefix(&format!("{}-", cli::PROGRAM)).unwrap_or_default();
        whole.find_subcommand(command).cloned().ok_or_else(|| format!("`{name}` is not a manual page"))?
    };
    let mut roff = Vec::new();
    clap_mangen::Man::new(command).render(&mut roff).map_err(|e| format!("cannot write the manual page: {e}"))?;
    String::from_utf8(roff).map_err(|_| "the manual page is not text".to_owned())
}

#[cfg(test)]
mod tests;
