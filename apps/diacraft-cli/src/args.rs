//! The command line: `diacraft-cli mcp`, `run`, `commands`, `oracle`, `export`, `convert`, `roundtrip`, `compare`, `completions`, `man`, `--version` and `--help`.
//!
//! Nothing is ignored: an option the program does not have, a value left out, and an option given twice are
//! errors, so that a mistyped one never starts something other than what was asked for. `--version` and `--help`
//! win over everything else, even an error elsewhere on the line, but not after a `--`, where they are the names of files.
//!
//! clap reads the line ([`crate::cli`] describes it) and this module reads what each command was asked out of what clap found, checks the values and says what is wrong in the
//! words the command line has always used. The help page is not clap's: it is [`crate::help`]'s table.

use clap::ArgMatches;
use clap::error::{ContextKind, ErrorKind};
use clap_complete::Shell;
use diacraft_engine::file_access::{READ_FLAG, RootRequest, WRITE_FLAG};
use diacraft_engine::{guard, shown};
use serde_json::Value;

use crate::cli;
use crate::term::When;

/// The most characters of an argument an error repeats.
const MOST_SHOWN: usize = 64;

/// The faces a command measures and draws text in (`--bundled-fonts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontChoice {
    /// The machine's faces and the bundled ones, as the window sets text.
    #[default]
    System,
    /// The bundled faces alone, which are the same on every machine: a run whose numbers can be compared across machines.
    Bundled,
}

/// A command line, read: what to do, and the faces to do it in.
#[derive(Debug, PartialEq)]
pub struct Parsed {
    pub action: Action,
    pub fonts: FontChoice,
    /// The folders `mcp` was given with `--automation-read-root` and `--automation-write-root` (empty for every other command): a session of this process is kept to them. The
    /// variables are read where the program starts (`main`), not here, so that parsing a command line reads nothing but the command line.
    pub roots: RootRequest,
}

/// What to do, as the command line says.
#[derive(Debug, PartialEq)]
pub enum Action {
    Version,
    Help,
    /// Print the command catalogue as JSON.
    Commands,
    /// Run the MCP server on standard input and output.
    Mcp(Mode),
    /// Run steps in a fresh headless session, one JSON line each.
    Run(Vec<Source>),
    /// The oracle harness over files, or over reports it printed before.
    Oracle(OracleArgs),
    /// One page of a drawing as a picture, a vector file or a PDF.
    Export(ExportArgs),
    /// A drawing from one type to another: a Save As of the engine for a drawing type, `export` for a picture type.
    Convert(ConvertArgs),
    /// Open a file, optionally move one shape, save it, and report what the save changed.
    Roundtrip(RoundtripArgs),
    /// Compare two OpenDocument drawings, part by part and element by element, or with `--content` two files of one format for the same content.
    Compare(CompareArgs),
    /// The completion script of a shell, for the command line or for the app.
    Completions(CompletionsArgs),
    /// A manual page, or the names of the pages.
    Man(ManArgs),
}

/// What `convert` was asked: the file, where to write, and what that is.
#[derive(Debug, PartialEq)]
pub struct ConvertArgs {
    /// The drawing, stencil or template to read.
    pub file: String,
    /// `--out PATH`: where to write, and as what type (its extension).
    pub out: String,
    pub to: ConvertTo,
    /// `--drop-macros`: leave the macro project out of the file written: without it a project that the type cannot hold (a VSDX type that allows no macros) refuses the save.
    pub drop_macros: bool,
    /// `--drop-objects`: leave the embedded objects of the other format out of the file written; each shape that showed one shows its picture, and the drawing keeps them.
    pub drop_objects: bool,
}

/// What `convert` writes, by the extension of `--out`.
#[derive(Debug, PartialEq)]
pub enum ConvertTo {
    /// A drawing type, as `convert::drawing_types` names it (lower case, no dot): the engine's Save As, whatever it says of this drawing.
    Drawing(&'static str),
    /// A picture: what `export` was asked, for the same file and path.
    Picture(ExportArgs),
}

/// What `compare` was asked: the two files, in order.
#[derive(Debug, PartialEq)]
pub struct CompareArgs {
    pub first: String,
    pub second: String,
    /// `--content`: the content check (the design's section 4.3) on two files of one format, a VSDX package or an OpenDocument drawing, and not the part-by-part comparison of two OpenDocument
    /// drawings.
    pub content: bool,
}

/// What `roundtrip` was asked.
#[derive(Debug, PartialEq)]
pub struct RoundtripArgs {
    /// The file to open: a drawing, stencil or template.
    pub file: String,
    /// `--out PATH`: where to save it, and as what type (its extension).
    pub out: String,
    /// `--move DX,DY`: inches to move the first shape of the first page that has one, before the save.
    pub shift: Option<(f64, f64)>,
    /// `--via diacraft`: the trip through `.diacraft` and back, and the content check of what came back.
    pub via: Option<Via>,
    /// `--edit KIND`: the salted edit made before the trip (`--via diacraft` only).
    pub edit: Option<EditKind>,
}

/// What `roundtrip --via` names: the format a file visits on its way out and back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// DiaCraft's own format: the file is saved as `.diacraft`, opened again in a new session, and saved back in the format it came in.
    Diacraft,
}

/// The salted edits of `roundtrip --via diacraft --edit` (plan ruling NP13). Each acts on the first shape of the first page that has one, as `--move` picks it, and each has one
/// command and fixed numbers, so that the same file gets the same edit on every run and every machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// A cell written: `shape.format` with a line weight of 1.23 points.
    Cell,
    /// A move: `shape.move` by 0.125 inch right and up.
    Move,
    /// A retext: `shape.set_text` with the text `salt`.
    Retext,
    /// A delete: `shape.delete`.
    Delete,
    /// A new shape: `shape.drop` of the master `rectangle` at the page's centre.
    Add,
}

impl EditKind {
    /// Every kind, in the order the help lists them.
    pub const ALL: [EditKind; 5] = [EditKind::Cell, EditKind::Move, EditKind::Retext, EditKind::Delete, EditKind::Add];

    /// The word that names the kind on the command line and in the report.
    pub fn word(self) -> &'static str {
        match self {
            EditKind::Cell => "cell",
            EditKind::Move => "move",
            EditKind::Retext => "retext",
            EditKind::Delete => "delete",
            EditKind::Add => "add",
        }
    }

    /// The kind a word names: exactly, in lower case.
    pub fn of(word: &str) -> Option<EditKind> {
        EditKind::ALL.into_iter().find(|kind| kind.word() == word)
    }
}

/// The farthest `--move` goes in either direction, in inches: a number no page could use is a typing mistake.
pub const MAX_MOVE: f64 = 1_000_000.0;

/// The file formats `export` writes, by the extension of `--out`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Svg,
    Pdf,
}

impl Format {
    /// The format an output path's extension names, in any case; `None` for any other.
    pub fn of(path: &str) -> Option<Format> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "svg" => Some(Format::Svg),
            "pdf" => Some(Format::Pdf),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
        }
    }
}

/// What `export` was asked.
#[derive(Debug, PartialEq)]
pub struct ExportArgs {
    /// The drawing to read.
    pub file: String,
    /// `--page N`: the page's place in the drawing, from 1; `None` when it was left out, which is the first page that is not a background page (a file lists its background pages
    /// first, so the first page of the drawing is the frame and not the drawing).
    pub page: Option<usize>,
    /// `--out PATH`: where to write, and in what format (its extension).
    pub out: String,
    pub format: Format,
    /// `--dpi N`: pixels to the inch of a PNG; `None` is 96.
    pub dpi: Option<f64>,
    /// `--text outlines`: every letter as outlines, in an SVG or a PDF.
    pub outlines: bool,
    /// `--transparent`: no white paper under the drawing.
    pub transparent: bool,
}

/// The most pixels to the inch `--dpi` takes: the renderer's own limits (16,384 pixels a side, 64 million in all) stop a larger picture
/// of a larger page, and this stops a number that no page could use.
pub const MAX_DPI: f64 = 2400.0;

/// What `oracle` was asked.
#[derive(Debug, PartialEq)]
pub struct OracleArgs {
    /// The files to report on, or with `aggregate` the JSON Lines files of reports to aggregate.
    pub files: Vec<String>,
    /// `--json`: print each report as one line of JSON, and not as a few lines of words.
    pub json: bool,
    /// `--source NAME`: the repository or host the files came from, for the aggregation.
    pub source: Option<String>,
    /// `--aggregate`: the files are reports, and the output is their score by file, by source and capped.
    pub aggregate: bool,
    /// `--cap N`: the most files one source adds to the capped figure.
    pub cap: usize,
}

/// The default `--cap`: five files a source.
pub const DEFAULT_CAP: usize = 5;

/// Which backend `mcp` serves.
#[derive(Debug, PartialEq)]
pub enum Mode {
    /// Look for the app at the default address once; a headless session if it is not DiaCraft that answers.
    Auto,
    Headless,
    Connect(String),
}

/// Where `run` gets steps from, in the order given.
#[derive(Debug, PartialEq)]
pub enum Source {
    /// `--cmd ID`, with the `--params` that followed it.
    Cmd { id: String, params: Value },
    /// `--commands FILE`, or `-` for standard input.
    Commands(String),
}

/// What `completions` was asked: the shell, and the program the script is for.
#[derive(Debug, PartialEq, Eq)]
pub struct CompletionsArgs {
    pub shell: Shell,
    pub program: Program,
}

/// The program a completion script is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Program {
    /// `diacraft-cli`.
    #[default]
    Cli,
    /// `diacraft`, the app, which has options of its own.
    App,
}

/// What `man` was asked.
#[derive(Debug, PartialEq, Eq)]
pub enum ManArgs {
    /// `--list`: the names of the pages.
    List,
    /// A page, by name; `diacraft-cli` when none was given.
    Page(String),
}

/// Reads the command line (without the program name). `--bundled-fonts` is an option of the commands that set text (`mcp`, `run`, `export`,
/// `roundtrip` and `convert` to a picture); `oracle` has the faces of its own machine, which its report names, and `commands` sets none.
///
/// clap reads the words into commands, options and values ([`crate::cli`]) and this reads them out into what each command was asked, in the words the command line has always used.
pub fn parse(args: &[String]) -> Result<Parsed, String> {
    // `--` ends the options: after it `--version` and `--help` are the names of files, so they are looked for before it only.
    let options = args.split(|a| a == "--").next().unwrap_or_default();
    if options.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Parsed { action: Action::Version, fonts: FontChoice::System, roots: RootRequest::default() });
    }
    if options.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Parsed { action: Action::Help, fonts: FontChoice::System, roots: RootRequest::default() });
    }
    if args.is_empty() {
        return Err(format!("no command given\n\n{}", crate::help::plain()));
    }
    let command = locate(args)?;
    // clap takes a global option given before the command and again after it as one: given twice is given twice.
    if color_given(args) > 1 {
        return Err("--color was given twice".to_owned());
    }
    let matches = read(args, command)?;
    color_word(&matches)?;
    let sub = matches.subcommand_matches(command).ok_or_else(|| internal("the command was not read"))?;
    let mut fonts = FontChoice::System;
    let mut roots = RootRequest::default();
    let action = match command {
        "commands" => Ok(Action::Commands),
        "mcp" => mcp(sub, &mut fonts, &mut roots).map(Action::Mcp),
        "run" => run(sub, &mut fonts).map(Action::Run),
        "oracle" => oracle(sub).map(Action::Oracle),
        "export" => export(sub, &mut fonts).map(Action::Export),
        "convert" => convert(sub, &mut fonts).map(Action::Convert),
        "roundtrip" => roundtrip(sub, &mut fonts).map(Action::Roundtrip),
        "compare" => compare(sub).map(Action::Compare),
        "completions" => completions(sub).map(Action::Completions),
        "man" => man(sub).map(Action::Man),
        other => Err(unknown_command(other)),
    }?;
    Ok(Parsed { action, fonts, roots })
}

/// What `--color` asked for on this line, or `Auto` when it is not given or its value is no choice. Looked at alone, and loosely, so that the page and the messages can be
/// coloured as asked even when the rest of the line is a mistake (`parse` is what says it is).
pub fn scan_color(args: &[String]) -> When {
    let mut found = When::Auto;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = if arg == "--color" { rest.next().map(String::as_str) } else { arg.strip_prefix("--color=") };
        if let Some(when) = value.and_then(When::of) {
            found = when;
        }
    }
    found
}

/// How many times `--color` is on the line, before a `--` that ends the options.
fn color_given(args: &[String]) -> usize {
    args.iter().take_while(|arg| *arg != "--").filter(|arg| *arg == "--color" || arg.starts_with("--color=")).count()
}

fn unknown_command(word: &str) -> String {
    format!("unknown command `{}`; see --help", shown(word, MOST_SHOWN))
}

fn internal(what: &str) -> String {
    format!("internal error: {what} (please report this bug)")
}

/// The command a line is for: its first word, after `--color` and its value if the line starts with them.
fn locate(args: &[String]) -> Result<&str, String> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--color" {
            rest.next();
        } else if !arg.starts_with("--color=") {
            return if cli::COMMANDS.contains(&arg.as_str()) { Ok(arg) } else { Err(unknown_command(arg)) };
        }
    }
    Err(format!("no command given\n\n{}", crate::help::plain()))
}

/// The line read by clap, or what is wrong with it in the command line's own words.
fn read(args: &[String], command: &str) -> Result<ArgMatches, String> {
    let argv = std::iter::once(cli::PROGRAM.to_owned()).chain(args.iter().cloned());
    // clap does not panic on any words a person can type; if it ever did, the line is refused as a bug and not as a crash.
    match guard::catch_panic(|| cli::command(None).try_get_matches_from(argv)) {
        Ok(Ok(matches)) => Ok(matches),
        Ok(Err(error)) => Err(said(&error, command, args)),
        Err(panic) => Err(internal(&format!("reading the command line failed: {}", shown(&panic, 200)))),
    }
}

/// What clap found wrong, in the words the command line has always had. Nothing clap rendered is echoed (it holds the arguments as they were typed): the argument is `shown`.
fn said(error: &clap::Error, command: &str, args: &[String]) -> String {
    let given = |kind: ContextKind| error.get(kind).map(ToString::to_string).unwrap_or_default();
    // A word with one dash is read as short options, and clap names the first (`-o` of `-odd.vsdx`): the word is what was typed.
    let typed = || {
        let arg = given(ContextKind::InvalidArg);
        let word =
            if arg.len() == 2 && arg.starts_with('-') { args.iter().find(|word| word.starts_with(&arg) && !word.starts_with("--")) } else { None };
        word.cloned().unwrap_or(arg)
    };
    // The option as the definition spells it, `--out <PATH>`: its name is the first word.
    let option = || given(ContextKind::InvalidArg).split_whitespace().next().unwrap_or_default().to_owned();
    match error.kind() {
        ErrorKind::ArgumentConflict => format!("{} was given twice", option()),
        ErrorKind::InvalidValue => format!("{} needs a value", option()),
        ErrorKind::UnknownArgument if command == "commands" => format!("`commands` takes no arguments; unexpected `{}`", shown(&typed(), MOST_SHOWN)),
        ErrorKind::UnknownArgument => format!("unknown option `{}` for `{command}`; see --help", shown(&typed(), MOST_SHOWN)),
        // A value given to a flag (`--transparent=1`): the word is not an option the command has.
        ErrorKind::TooManyValues => format!(
            "unknown option `{}={}` for `{command}`; see --help",
            shown(&option(), MOST_SHOWN),
            shown(&given(ContextKind::InvalidValue), MOST_SHOWN)
        ),
        _ => {
            let rendered = error.render().to_string();
            let first = rendered.lines().next().unwrap_or_default();
            shown(first.strip_prefix("error: ").unwrap_or(first), 200)
        }
    }
}

/// The value of a flag.
fn flag(sub: &ArgMatches, id: &str) -> Result<bool, String> {
    sub.try_get_one::<bool>(id).map(|value| value.copied().unwrap_or(false)).map_err(|e| internal(&format!("{id}: {e}")))
}

/// The value of an option given once.
fn one(sub: &ArgMatches, id: &str) -> Result<Option<String>, String> {
    sub.try_get_one::<String>(id).map(|value| value.cloned()).map_err(|e| internal(&format!("{id}: {e}")))
}

/// The values of an option given as often as it was, or of the words after the options, in the order they came.
fn many(sub: &ArgMatches, id: &str) -> Result<Vec<String>, String> {
    sub.try_get_many::<String>(id).map(|values| values.into_iter().flatten().cloned().collect()).map_err(|e| internal(&format!("{id}: {e}")))
}

/// `--bundled-fonts`, if the command was given it.
fn bundled_fonts(sub: &ArgMatches, fonts: &mut FontChoice) -> Result<(), String> {
    if flag(sub, "bundled-fonts")? {
        *fonts = FontChoice::Bundled;
    }
    Ok(())
}

/// `--color`'s value, checked: the one choice it takes.
fn color_word(matches: &ArgMatches) -> Result<When, String> {
    match one(matches, "color")? {
        None => Ok(When::Auto),
        Some(word) => When::of(&word).ok_or_else(|| "--color must be `auto`, `always` or `never`".to_owned()),
    }
}

/// The value of `--page`: the page's place in the drawing, a whole number from 1.
fn page_value(value: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(n) if n > 0 && value.bytes().all(|b| b.is_ascii_digit()) => Ok(n),
        _ => Err("--page must be a whole number from 1: the page's place in the drawing".to_owned()),
    }
}

/// The value of `--dpi`: pixels to the inch, a finite number above 0 and at most [`MAX_DPI`].
fn dpi_value(value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(n) if n.is_finite() && n > 0.0 && n <= MAX_DPI => Ok(n),
        _ => Err(format!("--dpi must be a number above 0 and at most {MAX_DPI}")),
    }
}

/// The value of `--text`: `text` or `outlines`.
fn text_value(value: &str) -> Result<&str, String> {
    if matches!(value, "text" | "outlines") { Ok(value) } else { Err("--text must be `text` or `outlines`".to_owned()) }
}

/// The options of a picture: the page, the pixels to the inch and the text, each checked as it is read.
struct PictureOptions {
    page: Option<usize>,
    dpi: Option<f64>,
    text: Option<String>,
}

fn picture_options(sub: &ArgMatches) -> Result<PictureOptions, String> {
    let page = one(sub, "page")?.map(|value| page_value(&value)).transpose()?;
    let dpi = one(sub, "dpi")?.map(|value| dpi_value(&value)).transpose()?;
    let text = one(sub, "text")?.map(|value| text_value(&value).map(str::to_owned)).transpose()?;
    Ok(PictureOptions { page, dpi, text })
}

fn export(sub: &ArgMatches, fonts: &mut FontChoice) -> Result<ExportArgs, String> {
    bundled_fonts(sub, fonts)?;
    let PictureOptions { page, dpi, text } = picture_options(sub)?;
    let transparent = flag(sub, "transparent")?;
    let file = single_file(sub, "`export` takes one drawing: a second file was given")?;
    let file = file.ok_or_else(|| "nothing to export: give the drawing's file".to_owned())?;
    let out = one(sub, "out")?.ok_or_else(|| "nowhere to write: give --out PATH".to_owned())?;
    let format = Format::of(&out).ok_or_else(|| "--out must end in .png, .svg or .pdf: that says what to write".to_owned())?;
    if dpi.is_some() && format != Format::Png {
        return Err("--dpi is for a PNG: an SVG is in CSS pixels and a PDF in points".to_owned());
    }
    if text.is_some() && format == Format::Png {
        return Err("--text is for an SVG or a PDF: a PNG has only pixels".to_owned());
    }
    Ok(ExportArgs { file, page, out, format, dpi, outlines: text.as_deref() == Some("outlines"), transparent })
}

/// The one file a command was given, if it was given one, and the refusal `second` if it was given more.
fn single_file(sub: &ArgMatches, second: &str) -> Result<Option<String>, String> {
    let mut files = many(sub, "file")?.into_iter();
    let file = files.next();
    if files.next().is_some() {
        return Err(second.to_owned());
    }
    Ok(file)
}

fn convert(sub: &ArgMatches, fonts: &mut FontChoice) -> Result<ConvertArgs, String> {
    bundled_fonts(sub, fonts)?;
    let PictureOptions { page, dpi, text } = picture_options(sub)?;
    let transparent = flag(sub, "transparent")?;
    let (drop_macros, drop_objects) = (flag(sub, "drop-macros")?, flag(sub, "drop-objects")?);
    let file = single_file(sub, "`convert` takes one drawing: a second file was given")?;
    let file = file.ok_or_else(|| "nothing to convert: give the drawing's file".to_owned())?;
    let out = one(sub, "out")?.ok_or_else(|| "nowhere to write: give --out PATH".to_owned())?;
    // The type is the extension of `--out`: a picture is `export`'s, with its options; a drawing type is the engine's Save As, which has none of them.
    if let Some(format) = Format::of(&out) {
        if dpi.is_some() && format != Format::Png {
            return Err("--dpi is for a PNG: an SVG is in CSS pixels and a PDF in points".to_owned());
        }
        if text.is_some() && format == Format::Png {
            return Err("--text is for an SVG or a PDF: a PNG has only pixels".to_owned());
        }
        let to = ConvertTo::Picture(ExportArgs {
            file: file.clone(),
            page,
            out: out.clone(),
            format,
            dpi,
            outlines: text.as_deref() == Some("outlines"),
            transparent,
        });
        for (given, name) in [(drop_macros, "--drop-macros"), (drop_objects, "--drop-objects")] {
            if given {
                return Err(format!("{name} is for a drawing type: a picture holds no macro project and no object"));
            }
        }
        return Ok(ConvertArgs { file, out, to, drop_macros, drop_objects });
    }
    let Some(extension) = crate::convert::drawing_type(&out) else {
        return Err(format!("--out must end in {}: that says what to write", crate::convert::type_words()));
    };
    for (given, name) in [
        (page.is_some(), "--page"),
        (dpi.is_some(), "--dpi"),
        (text.is_some(), "--text"),
        (transparent, "--transparent"),
        (*fonts == FontChoice::Bundled, "--bundled-fonts"),
    ] {
        if given {
            return Err(format!(
                "{name} is for a picture (.png, .svg or .pdf): the type to write is a drawing, which holds every page and is not drawn"
            ));
        }
    }
    Ok(ConvertArgs { file, out, to: ConvertTo::Drawing(extension), drop_macros, drop_objects })
}

fn roundtrip(sub: &ArgMatches, fonts: &mut FontChoice) -> Result<RoundtripArgs, String> {
    bundled_fonts(sub, fonts)?;
    let via = match one(sub, "via")? {
        None => None,
        Some(value) if value == "diacraft" => Some(Via::Diacraft),
        Some(_) => return Err("--via must be `diacraft`: the only format a file can visit".to_owned()),
    };
    let edit = match one(sub, "edit")? {
        None => None,
        Some(value) => Some(EditKind::of(&value).ok_or_else(|| "--edit must be `cell`, `move`, `retext`, `delete` or `add`".to_owned())?),
    };
    let shift = match one(sub, "move")? {
        None => None,
        Some(value) => {
            let parsed = value.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?)));
            let Some(delta) = parsed.filter(|(x, y)| x.is_finite() && y.is_finite() && x.abs() <= MAX_MOVE && y.abs() <= MAX_MOVE) else {
                return Err(format!("--move must be two numbers in inches, such as 1.5,-0.25 (each at most {MAX_MOVE} either way)"));
            };
            Some(delta)
        }
    };
    let file = single_file(sub, "`roundtrip` takes one file: a second was given")?;
    let file = file.ok_or_else(|| "nothing to open: give the file".to_owned())?;
    let out = one(sub, "out")?.ok_or_else(|| "nowhere to save: give --out PATH".to_owned())?;
    if edit.is_some() && shift.is_some() {
        return Err(
            "--edit and --move are both edits: give one (--edit move is the move of 0.125,0.125, and --move DX,DY one of your own)".to_owned()
        );
    }
    if edit.is_some() && via.is_none() {
        return Err("--edit is for --via diacraft: a salted edit is made before the trip through .diacraft".to_owned());
    }
    Ok(RoundtripArgs { file, out, shift, via, edit })
}

fn compare(sub: &ArgMatches) -> Result<CompareArgs, String> {
    let content = flag(sub, "content")?;
    let files = many(sub, "file")?;
    match <[String; 2]>::try_from(files) {
        Ok([first, second]) => Ok(CompareArgs { first, second, content }),
        Err(files) => Err(format!("`compare` takes two files, and {} were given", files.len())),
    }
}

fn mcp(sub: &ArgMatches, fonts: &mut FontChoice, roots: &mut RootRequest) -> Result<Mode, String> {
    bundled_fonts(sub, fonts)?;
    let headless = flag(sub, "headless")?;
    let connect = one(sub, "connect")?;
    // The folders of the two root flags, each handed to the engine's own reader of them as `--flag=FOLDER`: its words for an empty folder and for too many are the ones said.
    for (id, name) in [("automation-read-root", READ_FLAG), ("automation-write-root", WRITE_FLAG)] {
        for folder in many(sub, id)? {
            let no_more: &[String] = &[];
            roots.flag(&format!("{name}={folder}"), &mut no_more.iter())?;
        }
    }
    match (headless, connect) {
        (true, Some(_)) => Err("give either --connect or --headless, not both".to_owned()),
        (true, None) => Ok(Mode::Headless),
        // The session is the running app's and has the app's faces: the option would do nothing, and nothing is ignored.
        (false, Some(_)) if *fonts == FontChoice::Bundled => {
            Err("--bundled-fonts is for a session of this process, and the app's session has the app's faces: give --headless or neither".to_owned())
        }
        // The running app keeps to the roots it was started with, and this process cannot keep it to others: asking for them would be asking for what is not done.
        (false, Some(_)) if !roots.is_empty() => Err(
            "--automation-read-root and --automation-write-root keep the files of a session of this process, and --connect names the running app, which keeps to the roots it was \
             started with: start the app with `diacraft --control <port> --automation-read-root <dir> --automation-write-root <dir>`, or give --headless"
                .to_owned(),
        ),
        (false, Some(addr)) => Ok(Mode::Connect(addr)),
        (false, None) => Ok(Mode::Auto),
    }
}

/// `run`'s steps in the order the line gave them: each `--cmd` and `--commands` is a source, and a `--params` belongs to the `--cmd` before it. clap keeps where each value stood
/// on the line, which is how the three options are put back in order.
fn run(sub: &ArgMatches, fonts: &mut FontChoice) -> Result<Vec<Source>, String> {
    bundled_fonts(sub, fonts)?;
    enum Given {
        Cmd(String),
        Params(String),
        Commands(String),
    }
    let mut given: Vec<(usize, Given)> = Vec::new();
    for (id, make) in [("cmd", Given::Cmd as fn(String) -> Given), ("params", Given::Params), ("commands", Given::Commands)] {
        let values = many(sub, id)?;
        let places: Vec<usize> = sub.indices_of(id).map(Iterator::collect).unwrap_or_default();
        if values.len() != places.len() {
            return Err(internal("the places of the values of run's options"));
        }
        given.extend(places.into_iter().zip(values.into_iter().map(make)));
    }
    given.sort_by_key(|(place, _)| *place);
    let mut sources: Vec<Source> = Vec::new();
    // Whether the last source is a `--cmd` that has not been given its `--params` yet.
    let mut params_open = false;
    let mut stdin_used = false;
    for (_, item) in given {
        match item {
            Given::Cmd(id) => {
                sources.push(Source::Cmd { id, params: Value::Object(serde_json::Map::new()) });
                params_open = true;
            }
            Given::Params(raw) => {
                let params = match serde_json::from_str::<Value>(&raw) {
                    Ok(object @ Value::Object(_)) => object,
                    Ok(_) => return Err("--params must be a JSON object".to_owned()),
                    Err(e) => return Err(format!("--params is not JSON: {}", shown(&e.to_string(), 200))),
                };
                match sources.last_mut() {
                    Some(Source::Cmd { params: slot, .. }) if params_open => {
                        *slot = params;
                        params_open = false;
                    }
                    Some(Source::Cmd { .. }) => return Err("--params was given twice for one --cmd".to_owned()),
                    _ => return Err("--params must follow the --cmd it is for".to_owned()),
                }
            }
            Given::Commands(source) => {
                if source == "-" && std::mem::replace(&mut stdin_used, true) {
                    return Err("--commands - was given twice: standard input is read once".to_owned());
                }
                sources.push(Source::Commands(source));
                params_open = false;
            }
        }
    }
    if sources.is_empty() {
        return Err("nothing to run: give --cmd or --commands".to_owned());
    }
    Ok(sources)
}

fn oracle(sub: &ArgMatches) -> Result<OracleArgs, String> {
    let mut out = OracleArgs {
        files: many(sub, "file")?,
        json: flag(sub, "json")?,
        source: one(sub, "source")?,
        aggregate: flag(sub, "aggregate")?,
        cap: DEFAULT_CAP,
    };
    let cap = one(sub, "cap")?;
    if let Some(cap) = &cap {
        out.cap = match cap.parse::<usize>() {
            Ok(n) if n > 0 && cap.bytes().all(|b| b.is_ascii_digit()) => n,
            _ => return Err("--cap must be a whole number of files, 1 or more".to_owned()),
        };
    }
    if out.files.is_empty() {
        return Err("nothing to report on: give one or more files".to_owned());
    }
    if out.aggregate && (out.source.is_some() || out.json) {
        return Err("--aggregate prints JSON and takes the sources from the reports: give neither --json nor --source".to_owned());
    }
    if cap.is_some() && !out.aggregate {
        return Err("--cap is for --aggregate".to_owned());
    }
    Ok(out)
}

fn completions(sub: &ArgMatches) -> Result<CompletionsArgs, String> {
    let named = format!("`completions` needs one shell: {}", shell_words());
    let mut shells = many(sub, "shell")?.into_iter();
    let (Some(word), None) = (shells.next(), shells.next()) else {
        return Err(named);
    };
    let shell = word
        .parse::<Shell>()
        .map_err(|_| format!("`{}` is not a shell the completions are written for: {}", shown(&word, MOST_SHOWN), shell_words()))?;
    let program = match one(sub, "program")?.as_deref() {
        None | Some(cli::PROGRAM) => Program::Cli,
        Some(cli::APP) => Program::App,
        Some(_) => return Err(format!("--program must be `{}` or `{}`", cli::PROGRAM, cli::APP)),
    };
    Ok(CompletionsArgs { shell, program })
}

/// The five shells, as the refusals name them: `bash, zsh, fish, powershell or elvish`.
fn shell_words() -> String {
    let (last, first) = cli::SHELLS.split_last().map_or(("", &[][..]), |(last, first)| (*last, first));
    format!("{} or {last}", first.join(", "))
}

fn man(sub: &ArgMatches) -> Result<ManArgs, String> {
    let mut pages = many(sub, "page")?.into_iter();
    let page = pages.next();
    if pages.next().is_some() {
        return Err("`man` takes one page: a second was given".to_owned());
    }
    match (flag(sub, "list")?, page) {
        (true, Some(_)) => Err("--list names the pages and takes no page".to_owned()),
        (true, None) => Ok(ManArgs::List),
        (false, None) => Ok(ManArgs::Page(cli::PROGRAM.to_owned())),
        (false, Some(page)) if crate::man::pages().contains(&page) => Ok(ManArgs::Page(page)),
        (false, Some(page)) => Err(format!("`{}` is not a manual page: `diacraft-cli man --list` names them", shown(&page, MOST_SHOWN))),
    }
}
