//! The terminal: whether the help and the messages are coloured, how wide the page is, and the streams they are written to.
//!
//! **Colour is for people and never for programs.** It goes into two things only: the help page (standard output) and the words that tell a person a command failed or was
//! mistyped (standard error). Neither is read by a program: the JSON answers, the MCP stream and the exit statuses are the same bytes with every switch below, which the tests
//! check. The rules, in the order they are asked:
//!
//! 1. `--color always` colours and `--color never` does not, whatever the environment says.
//! 2. With `auto` (the default): `NO_COLOR` set and not empty means plain; then `CLICOLOR_FORCE` set, not empty and not `0` means colour, even for a pipe; then `CLICOLOR=0` means
//!    plain; then colour only when the stream is a terminal and `TERM` is neither empty nor `dumb` (a Windows console has no `TERM` and is fine without one).
//!
//! Each stream is judged on its own, so a person who sends the JSON to a file still sees the failures in colour. The page is wrapped to the terminal's width only when standard
//! output is a terminal: a pipe, a file and a script always get the page as it is written, which is also what `docs/cli.md` shows.
//!
//! The streams are `anstream`'s: where colour is on, a Windows console is put in the mode that understands the codes (or, on an old console that has none, the codes are turned
//! into its own calls), and where it is off every escape is stripped on the way out, so a message that held one could never reach a terminal as one.

use std::io::{self, IsTerminal, Write};

use anstream::AutoStream;
use anstyle::{AnsiColor, Color, Style};

/// What `--color` says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum When {
    /// The environment and the stream decide.
    #[default]
    Auto,
    Always,
    Never,
}

impl When {
    /// The words `--color` takes, in the order the help lists them.
    pub const WORDS: [&'static str; 3] = ["auto", "always", "never"];

    /// The choice a word names: exactly, in lower case.
    pub fn of(word: &str) -> Option<When> {
        match word {
            "auto" => Some(When::Auto),
            "always" => Some(When::Always),
            "never" => Some(When::Never),
            _ => None,
        }
    }
}

/// The two streams a person reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

impl Stream {
    pub fn is_terminal(self) -> bool {
        match self {
            Stream::Out => io::stdout().is_terminal(),
            Stream::Err => io::stderr().is_terminal(),
        }
    }

    /// The width of the terminal this stream is, in columns; `None` when it is not a terminal or does not say.
    pub fn columns(self) -> Option<usize> {
        let size = match self {
            Stream::Out => terminal_size::terminal_size_of(io::stdout()),
            Stream::Err => terminal_size::terminal_size_of(io::stderr()),
        };
        size.map(|(width, _)| usize::from(width.0)).filter(|&columns| columns > 0)
    }
}

/// Whether to colour, by the rules in the module's text. `var` reads an environment variable (`None`: not set, or not text).
pub fn colours(when: When, var: &dyn Fn(&str) -> Option<String>, is_terminal: bool) -> bool {
    match when {
        When::Always => return true,
        When::Never => return false,
        When::Auto => {}
    }
    // A variable that is set to nothing is not set.
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    if set("NO_COLOR").is_some() {
        return false;
    }
    if set("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return true;
    }
    if set("CLICOLOR").is_some_and(|value| value == "0") {
        return false;
    }
    is_terminal && term_allows(set("TERM"))
}

/// Whether the terminal named by `TERM` takes colour: any but `dumb`; none named is a Windows console's case only.
fn term_allows(term: Option<String>) -> bool {
    match term {
        Some(term) => term != "dumb",
        None => cfg!(windows),
    }
}

/// [`colours`] for `stream` in this process's environment.
pub fn colours_for(when: When, stream: Stream) -> bool {
    colours(when, &|name| std::env::var(name).ok(), stream.is_terminal())
}

/// The widest the page is wrapped to, however wide the terminal: a line much longer than this is hard to read.
const WIDEST: usize = 120;
/// The narrowest it is wrapped to: below this a line cannot hold a few words, and the terminal wraps what is left.
const NARROWEST: usize = 40;

/// The width the help page is wrapped to on `stream`: the terminal's, kept between [`NARROWEST`] and [`WIDEST`]; `None` for what is not a terminal, which is not wrapped.
pub fn page_width(stream: Stream) -> Option<usize> {
    clamp_width(stream.columns())
}

/// The width the page is wrapped to for a terminal of `columns`: kept between [`NARROWEST`] and [`WIDEST`].
fn clamp_width(columns: Option<usize>) -> Option<usize> {
    columns.map(|columns| columns.clamp(NARROWEST, WIDEST))
}

/// What is shown in the help and the messages: headings, the names a person types, the places they fill in, and a failure.
pub struct Palette;

impl Palette {
    pub const HEADING: Style = Style::new().bold().fg_color(Some(Color::Ansi(AnsiColor::Green)));
    pub const LITERAL: Style = Style::new().bold().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
    pub const PLACEHOLDER: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
    pub const ERROR: Style = Style::new().bold().fg_color(Some(Color::Ansi(AnsiColor::Red)));
}

/// Writes `text` to `stream`, in colour or with every escape stripped, ignoring failure: these streams are for people, and a closed one has nobody left. Not `print!`, which
/// panics when its stream is closed.
pub fn write_to(stream: Stream, colour: bool, text: &str) {
    let choice = if colour { anstream::ColorChoice::Always } else { anstream::ColorChoice::Never };
    // Ignoring is right: a reader that went away (`| head`) has nobody to tell.
    let _ = match stream {
        Stream::Out => put(AutoStream::new(io::stdout().lock(), choice), text),
        Stream::Err => put(AutoStream::new(io::stderr().lock(), choice), text),
    };
}

fn put(mut out: impl Write, text: &str) -> io::Result<()> {
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// `text` in `style` when `colour`, else as it is.
pub fn paint(colour: bool, style: Style, text: &str) -> String {
    if colour { format!("{style}{text}{style:#}") } else { text.to_owned() }
}

/// A message for a person on standard error, `diacraft-cli: TEXT`: the program's name in the error colour when `colour`. The words are the same either way.
pub fn message(colour: bool, text: &str) -> String {
    format!("{} {text}\n", paint(colour, Palette::ERROR, "diacraft-cli:"))
}

#[cfg(test)]
mod tests;
