//! `diacraft-cli`: DiaCraft's commands for scripts and agents.
//!
//! ```text
//! diacraft-cli mcp [--headless | --connect ADDR] [--bundled-fonts]
//! diacraft-cli run [--cmd ID [--params JSON] | --commands FILE|-]... [--bundled-fonts]
//! diacraft-cli commands
//! diacraft-cli oracle FILE... [--json] [--source NAME]
//! diacraft-cli oracle --aggregate REPORTS... [--cap N]
//! diacraft-cli export FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
//! diacraft-cli convert FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
//! diacraft-cli roundtrip FILE --out PATH [--move DX,DY] [--bundled-fonts]
//! diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move|retext|delete|add | --move DX,DY] [--bundled-fonts]
//! diacraft-cli compare [--content] FIRST SECOND
//! diacraft-cli --version
//! ```
//!
//! `mcp` is the MCP server on standard input and output; `run` runs commands in a fresh headless session and prints
//! a JSON line for each; `commands` lists them; `oracle` scores the recalculation of drawings against the values they cached; `export` writes one
//! page of a drawing as a PNG, an SVG or a PDF; `convert` writes a drawing as another type (a Save As of the engine) or as a picture (`export`'s); `roundtrip` opens a file, saves it (moving a shape if asked) and reports what the save changed, and with `--via diacraft` sends it
//! through DiaCraft's own format and back and reports the content check; `compare` says what differs between two OpenDocument drawings, and with `--content` whether two files of one format
//! have the same content. See `docs/cli.md` and `docs/mcp.md`.
//!
//! Standard output carries results and the protocol and nothing else; messages for people and logs go to standard
//! error. The exit status is 0 on success, 1 when a step, a connection or a file fails, and 2 for a mistake in the
//! command line.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod args;
mod convert;
mod export;
#[cfg(kani)]
mod kani_proofs;
mod logger;
mod oracle;
mod roundtrip;
mod steps;
#[cfg(test)]
mod tests;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use args::{Action, ConvertTo, FontChoice, Mode, Source, USAGE};
use diacraft_engine::{Session, guard, measure, shown};
use diacraft_mcp::{Backend, ConnectError, ControlClient, DEFAULT_ADDR, connect_or_headless, json_line, serve};
use serde_json::json;

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

fn main() -> ExitCode {
    // Not `std::env::args`, which panics on an argument that is not valid Unicode.
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let (action, fonts) = match args::parse(&args) {
        Ok(parsed) => (parsed.action, parsed.fonts),
        Err(e) => {
            say(io::stderr(), &format!("diacraft-cli: {e}\n"));
            return ExitCode::from(2);
        }
    };
    match action {
        Action::Version => {
            say(io::stdout(), &format!("diacraft-cli {}\n", env!("CARGO_PKG_VERSION")));
            return ExitCode::SUCCESS;
        }
        Action::Help => {
            say(io::stdout(), USAGE);
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    if let Some(note) = logger::init(std::env::var("RUST_LOG").ok().as_deref()) {
        say(io::stderr(), &format!("diacraft-cli: {note}\n"));
    }
    // The git release and the year this build says (`build.rs`): the engine answers them in `app.version`, to a script and to the MCP server.
    diacraft_engine::build_info::set(diacraft_engine::build_info::BuildInfo::new(env!("DIACRAFT_BUILD_GIT"), env!("DIACRAFT_BUILD_YEAR")));
    // A panic is a bug. It is caught here and reported as one, so a script sees an exit status and not an abort.
    let outcome = guard::catch_panic(|| {
        start_fonts(&action, fonts);
        match action {
            Action::Commands => commands(),
            Action::Mcp(mode) => mcp(mode, fonts),
            Action::Run(sources) => run(sources),
            Action::Oracle(args) => oracle::run(args),
            Action::Export(args) => export::run(args),
            Action::Convert(args) => convert::run(args),
            Action::Roundtrip(args) => roundtrip::run(args),
            Action::Compare(args) => roundtrip::compare(&args),
            Action::Version | Action::Help => Ok(()),
        }
    });
    match outcome {
        Ok(Ok(())) | Ok(Err(Failure::Closed)) => ExitCode::SUCCESS,
        Ok(Err(Failure::Message(e))) => {
            say(io::stderr(), &format!("diacraft-cli: {e}\n"));
            ExitCode::FAILURE
        }
        Err(panic) => {
            say(io::stderr(), &format!("diacraft-cli: internal error: {} (please report this bug)\n", shown(&panic, 400)));
            ExitCode::FAILURE
        }
    }
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
fn mcp(mode: Mode, fonts: FontChoice) -> Result<(), Failure> {
    mcp_on(mode, DEFAULT_ADDR, fonts, io::stdin().lock(), io::stdout().lock())
}

/// All of `mcp` on the streams given: picks the backend (the default mode looks at `default_addr`), says which on standard
/// error, and serves `input` to `output` until `input` ends. A test drives it against a fake app, with no real port.
///
/// **The faces are read only for a session of this process**, and before it runs a command: a session in the running app (`--connect`, or the
/// default mode that found it) has the app's faces and reads nothing here, and the default mode reads them once it has chosen the headless
/// session, not while it is still looking for the app.
fn mcp_on(mode: Mode, default_addr: &str, fonts: FontChoice, input: impl BufRead, output: impl Write) -> Result<(), Failure> {
    let (backend, why) = open_backend(mode, default_addr).map_err(Failure::Message)?;
    if !backend.has_app() {
        read_faces(fonts);
    }
    if let Some(why) = why {
        say(io::stderr(), &format!("diacraft-cli: {why}; using a headless session\n"));
    }
    say(io::stderr(), &format!("diacraft-cli: MCP server on stdio, backend: {}\n", backend.describe()));
    match serve(input, output, backend) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Err(Failure::Closed),
        Err(e) => Err(Failure::Message(format!("the MCP server stopped: {e}"))),
    }
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
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Err(Failure::Closed),
        Err(e) => Err(Failure::Message(format!("cannot write to standard output: {e}"))),
    }
}
