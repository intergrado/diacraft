//! `diacraft-cli`: DiaCraft's commands for scripts and agents.
//!
//! ```text
//! diacraft-cli mcp [--headless | --connect ADDR] [--automation-read-root DIR]... [--automation-write-root DIR]... [--bundled-fonts]
//! diacraft-cli run [--cmd ID [--params JSON] | --commands FILE|-]... [--bundled-fonts]
//! diacraft-cli commands
//! diacraft-cli oracle FILE... [--json] [--source NAME]
//! diacraft-cli oracle --aggregate REPORTS... [--cap N]
//! diacraft-cli export FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
//! diacraft-cli convert FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
//! diacraft-cli roundtrip FILE --out PATH [--move DX,DY] [--bundled-fonts]
//! diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move|retext|delete|add | --move DX,DY] [--bundled-fonts]
//! diacraft-cli compare [--content] FIRST SECOND
//! diacraft-cli completions SHELL [--program diacraft|diacraft-cli]
//! diacraft-cli man [--list | PAGE]
//! diacraft-cli --color auto|always|never ...
//! diacraft-cli --version
//! ```
//!
//! `mcp` is the MCP server on standard input and output; `run` runs commands in a fresh headless session and prints
//! a JSON line for each; `commands` lists them; `oracle` scores the recalculation of drawings against the values they cached; `export` writes one
//! page of a drawing as a PNG, an SVG or a PDF; `convert` writes a drawing as another type (a Save As of the engine) or as a picture (`export`'s); `roundtrip` opens a file, saves it (moving a shape if asked) and reports what the save changed, and with `--via diacraft` sends it
//! through DiaCraft's own format and back and reports the content check; `compare` says what differs between two OpenDocument drawings, and with `--content` whether two files of one format
//! have the same content; `completions` writes the completion script of a shell, and `man` a manual page. See `docs/cli.md` and `docs/mcp.md`.
//!
//! Standard output carries results and the protocol and nothing else; messages for people and logs go to standard
//! error. The exit status is 0 on success, 1 when a step, a connection or a file fails, and 2 for a mistake in the
//! command line. The help page and the words of a failure are coloured when a terminal can show it (`term.rs`) and never anything a program reads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod args;
mod cli;
mod completions;
mod convert;
mod export;
mod help;
#[cfg(kani)]
mod kani_proofs;
mod logger;
mod man;
mod oracle;
mod prefs;
mod roundtrip;
mod steps;
mod term;
#[cfg(test)]
mod tests;
mod warning_class;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;

use args::{Action, ConvertTo, FontChoice, Mode, Source};
use diacraft_engine::file_access::{self, AutomationRoots, READ_ENV, RootRequest, WRITE_ENV};
use diacraft_engine::{Session, guard, measure, shown};
use diacraft_mcp::{Backend, ConnectError, ControlClient, DEFAULT_ADDR, connect_or_headless, json_line, serve};
use serde_json::json;
use term::{Stream, When};

/// mimalloc is the global allocator of the command line on every target but the browser's (owner 2026-11-21). Opening, saving and re-reading a large
/// drawing is mostly many small allocations and frees, and mimalloc does them in less CPU time than the system's allocator. Our budgets count our own
/// allocations, not the allocator's overhead, so they are unchanged. The web build never compiles it (`Cargo.toml` takes it for the other targets only).
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Why a command did not finish.
enum Failure {
    /// Say this on standard error and exit with 1.
    Message(String),
    /// The reader of standard output went away (`| head`): there is nobody to tell, and it is not a failure.
    Closed,
}

/// Writes `text` to `out`, ignoring failure: these streams are for people, and a closed one has nobody left.
/// Not `print!`, which panics when its stream is closed.
fn say(mut out: impl Write, text: &str) {
    let _ = out.write_all(text.as_bytes());
}

/// The help page for a person at `stream`: wrapped to the terminal's width and painted where that is on, and as it is written for anything else.
fn help_page(when: When, stream: Stream) -> (String, bool) {
    let look = help::Look { width: term::page_width(stream), colour: term::colours_for(when, stream) };
    (help::render(help::PAGE, look), look.colour)
}

/// Says a failure to the person on standard error: `diacraft-cli: TEXT`, with the program's name in the error colour where colour is on for standard error.
fn complain(when: When, text: &str) {
    let colour = term::colours_for(when, Stream::Err);
    term::write_to(Stream::Err, colour, &term::message(colour, text));
}

/// Says that the line is a mistake (exit status 2). The mistake that is no command at all comes with the page, laid out for standard error.
fn usage_error(when: When, error: &str) -> ExitCode {
    match error.split_once("\n\n") {
        Some(("no command given", _page)) => {
            let (page, colour) = help_page(when, Stream::Err);
            let text = format!("no command given\n\n{page}");
            term::write_to(Stream::Err, colour, &term::message(colour, text.trim_end_matches('\n')));
        }
        _ => complain(when, error),
    }
    ExitCode::from(2)
}

fn main() -> ExitCode {
    // Not `std::env::args`, which panics on an argument that is not valid Unicode.
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    // What `--color` asks, read first and loosely: the help and the messages are coloured as asked even when the line is a mistake.
    let when = args::scan_color(&args);
    let (action, fonts, flags) = match args::parse(&args) {
        Ok(parsed) => (parsed.action, parsed.fonts, parsed.roots),
        Err(e) => return usage_error(when, &e),
    };
    match action {
        Action::Version => {
            say(io::stdout(), &format!("diacraft-cli {}\n", env!("CARGO_PKG_VERSION")));
            return ExitCode::SUCCESS;
        }
        Action::Help => {
            let (page, colour) = help_page(when, Stream::Out);
            term::write_to(Stream::Out, colour, &page);
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    if let Some(note) = logger::init(std::env::var("RUST_LOG").ok().as_deref()) {
        say(io::stderr(), &format!("diacraft-cli: {note}\n"));
    }
    // The folders `mcp` is kept to are made before anything is served: one that is not there is a mistake in the command line, and the server does not start without it, since a
    // server that was asked to keep to folders and does not would be worse than none.
    let roots = match open_roots(&action, flags) {
        Ok(roots) => roots,
        Err(e) => {
            complain(when, &e);
            return ExitCode::from(2);
        }
    };
    // The git release and the year this build says (`build.rs`): the engine answers them in `app.version`, to a script and to the MCP server.
    diacraft_engine::build_info::set(diacraft_engine::build_info::BuildInfo::new(env!("DIACRAFT_BUILD_GIT"), env!("DIACRAFT_BUILD_YEAR")));
    // The person's settings, for the commands that run a session of this process (`prefs.rs`): opened here, said once if something is wrong, saved when the command ends.
    let mut profile = prefs::wants_profile(&action).then(prefs::open).map(|(profile, notes)| {
        for note in notes {
            say(io::stderr(), &format!("diacraft-cli: {note}\n"));
        }
        profile
    });
    // A panic is a bug. It is caught here and reported as one, so a script sees an exit status and not an abort.
    let outcome = guard::catch_panic(|| {
        start_fonts(&action, fonts);
        match action {
            Action::Commands => commands(),
            Action::Mcp(mode) => mcp(mode, fonts, roots),
            Action::Run(sources) => run(sources),
            Action::Oracle(args) => oracle::run(args),
            Action::Export(args) => export::run(args),
            Action::Convert(args) => convert::run(args),
            Action::Roundtrip(args) => roundtrip::run(args),
            Action::Compare(args) => roundtrip::compare(&args),
            Action::Completions(args) => completions::script(&args).map_err(Failure::Message).and_then(|script| write_text(&script)),
            Action::Man(args) => man::output(&args).map_err(Failure::Message).and_then(|page| write_text(&page)),
            Action::Version | Action::Help => Ok(()),
        }
    });
    if let Some(note) = profile.as_mut().and_then(prefs::close) {
        say(io::stderr(), &format!("diacraft-cli: {note}\n"));
    }
    match outcome {
        Ok(Ok(())) | Ok(Err(Failure::Closed)) => ExitCode::SUCCESS,
        Ok(Err(Failure::Message(e))) => {
            complain(when, &e);
            ExitCode::FAILURE
        }
        Err(panic) => {
            complain(when, &format!("internal error: {} (please report this bug)", shown(&panic, 400)));
            ExitCode::FAILURE
        }
    }
}

/// The roots a session of this process is kept to: the flags' and, for a kind that has no flag, the variables'. Only `mcp` has roots: the other commands read and write the paths
/// their person types. `None`: no roots, so any path the person can reach, as it has been.
fn open_roots(action: &Action, flags: RootRequest) -> Result<Option<Arc<AutomationRoots>>, String> {
    if !matches!(action, Action::Mcp(_)) {
        return Ok(None);
    }
    flags.with_environment(std::env::var_os(READ_ENV).as_deref(), std::env::var_os(WRITE_ENV).as_deref()).open()
}

/// Reads the faces text is measured and drawn in, once and before the first command, for the commands that run a session of this process and
/// set text, and shares them with the engine: its commits measure a text block in the faces an export draws it in
/// (`diacraft_engine::measure`). The machine's faces and the bundled ones, as the window sets text; `--bundled-fonts` reads nothing from the
/// machine, so that a run is the same on every machine. `commands` sets no text, `oracle` reads the machine's faces for itself, and `mcp` does
/// not read them here: it knows only once it has a backend whether the session is its own ([`mcp_on`]).
fn start_fonts(action: &Action, fonts: FontChoice) {
    // A convert to a drawing type draws nothing and sets no text: it opens and saves, and reads no faces.
    let draws = matches!(action, Action::Convert(convert) if matches!(convert.to, ConvertTo::Picture(_)));
    if draws || matches!(action, Action::Run(_) | Action::Export(_) | Action::Roundtrip(_)) {
        read_faces(fonts);
    }
}

/// Reads the faces `fonts` asks for and shares them with the engine.
fn read_faces(fonts: FontChoice) {
    match fonts {
        FontChoice::System => drop(measure::use_system_fonts()),
        FontChoice::Bundled => drop(measure::use_bundled_fonts()),
    }
}

/// `commands`: the registry as JSON.
fn commands() -> Result<(), Failure> {
    let listing =
        serde_json::to_string_pretty(&Session::new().commands()).map_err(|e| Failure::Message(format!("cannot encode the command list: {e}")))?;
    write_line(&listing)
}

/// The backend `mode` asks for, and, when the default mode fell back to a headless session, why. `default_addr` is where
/// the default mode looks for the app (once): a parameter so that the tests never look at a real app.
fn open_backend(mode: Mode, default_addr: &str) -> Result<(Backend, Option<ConnectError>), String> {
    match mode {
        Mode::Headless => Ok((Backend::headless(), None)),
        // An address given is never second-guessed: if DiaCraft is not there, that is the error.
        Mode::Connect(addr) => ControlClient::connect(&addr).map(|client| (Backend::Remote(client), None)).map_err(|e| e.to_string()),
        Mode::Auto => Ok(connect_or_headless(default_addr)),
    }
}

/// `mcp`: serves standard input and output until the client closes its end. The default mode looks for the app at
/// [`DEFAULT_ADDR`], once.
fn mcp(mode: Mode, fonts: FontChoice, roots: Option<Arc<AutomationRoots>>) -> Result<(), Failure> {
    // The server is all agent, so the roots are in force in the whole process: a thread that is not told of them is kept to them too.
    if let Some(roots) = &roots {
        file_access::confine_process(roots.clone());
    }
    mcp_with_roots(mode, DEFAULT_ADDR, fonts, roots, io::stdin().lock(), io::stdout().lock())
}

/// All of `mcp` on the streams given: picks the backend (the default mode looks at `default_addr`), says which on standard
/// error, and serves `input` to `output` until `input` ends. A test drives it against a fake app, with no real port.
///
/// **The faces are read only for a session of this process**, and before it runs a command: a session in the running app (`--connect`, or the
/// default mode that found it) has the app's faces and reads nothing here, and the default mode reads them once it has chosen the headless
/// session, not while it is still looking for the app. (Only the tests call it, with no roots; the program calls [`mcp_with_roots`].)
#[cfg(test)]
fn mcp_on(mode: Mode, default_addr: &str, fonts: FontChoice, input: impl BufRead, output: impl Write) -> Result<(), Failure> {
    mcp_with_roots(mode, default_addr, fonts, None, input, output)
}

/// [`mcp_on`] kept to `roots`: the server loop runs with them in force on its thread, and says which they are to the person (standard error) and to the client (`initialize`).
/// **A session in the running app cannot be kept to roots from here** (the app keeps to the ones it was started with), so a default mode that finds the app with roots asked for
/// stops, and says what to give instead, and does not go on in an app it cannot keep.
fn mcp_with_roots(
    mode: Mode,
    default_addr: &str,
    fonts: FontChoice,
    roots: Option<Arc<AutomationRoots>>,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), Failure> {
    let (backend, why) = open_backend(mode, default_addr).map_err(Failure::Message)?;
    if roots.is_some() && backend.has_app() {
        return Err(Failure::Message(
            "the DiaCraft app answers, and this process cannot keep it to the folders asked for (--automation-read-root, --automation-write-root, or the variables of them): the app keeps to the \
             roots it was started with. Give --headless for a session of this process that is kept to them"
                .to_owned(),
        ));
    }
    if !backend.has_app() {
        read_faces(fonts);
    }
    if let Some(why) = why {
        say(io::stderr(), &format!("diacraft-cli: {why}; using a headless session\n"));
    }
    file_access::confine(roots.as_ref(), || {
        say(io::stderr(), &format!("diacraft-cli: MCP server on stdio, backend: {}\n", backend.describe()));
        match serve(input, output, backend) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Err(Failure::Closed),
            Err(e) => Err(Failure::Message(format!("the MCP server stopped: {e}"))),
        }
    })
}

/// `run`: every step, in a session of its own.
fn run(sources: Vec<Source>) -> Result<(), Failure> {
    let steps = steps::collect(sources).map_err(Failure::Message)?;
    let mut session = Session::new();
    for (n, step) in steps.iter().enumerate() {
        let result =
            session.execute(&step.command, &step.params).map_err(|e| Failure::Message(format!("step {n} ({}): {e}", shown(&step.command, 64))))?;
        let line = json_line(&json!({"step": n, "command": step.command, "result": result}))
            .map_err(|e| Failure::Message(format!("step {n}: cannot encode the result: {e}")))?;
        write_line(&line)?;
    }
    Ok(())
}

/// Writes `text` and a newline to standard output.
fn write_line(text: &str) -> Result<(), Failure> {
    write_text(&format!("{text}\n"))
}

/// Writes `text` to standard output as it is.
fn write_text(text: &str) -> Result<(), Failure> {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Err(Failure::Closed),
        Err(e) => Err(Failure::Message(format!("cannot write to standard output: {e}"))),
    }
}
