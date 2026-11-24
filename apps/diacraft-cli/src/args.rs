//! The command line: `diacraft-cli mcp`, `run`, `commands`, `oracle`, `export`, `convert`, `roundtrip`, `compare`, `--version` and `--help`.
//!
//! Nothing is ignored: an option the program does not have, a value left out, and an option given twice are
//! errors, so that a mistyped one never starts something other than what was asked for. `--version` and `--help`
//! win over everything else, even an error elsewhere on the line.

use diacraft_engine::shown;
use serde_json::Value;

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
}

/// Notes `--bundled-fonts`, once: a second one is a mistake like any option given twice.
fn bundled_fonts(fonts: &mut FontChoice) -> Result<(), String> {
    if std::mem::replace(fonts, FontChoice::Bundled) == FontChoice::Bundled {
        return Err("--bundled-fonts was given twice".to_owned());
    }
    Ok(())
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
    /// `--page N`: the page's place in the drawing, from 1.
    pub page: usize,
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

/// `diacraft-cli --help`.
pub const USAGE: &str = "\
Usage: diacraft-cli <command> [options]

Commands:
  diacraft-cli mcp [--headless | --connect ADDR] [--bundled-fonts]
      Run the MCP server on standard input and output (JSON-RPC lines; see docs/mcp.md).
        --headless      a session of its own, with no window
        --connect ADDR  the running app's control channel, such as 127.0.0.1:7997
        --bundled-fonts the bundled faces alone, for a session of its own (see Fonts below); refused with --connect
      With neither, it looks for the app at 127.0.0.1:7997 once and uses it if DiaCraft answers there;
      if nothing does, or another program does, it runs headless. It never drives another program.

  diacraft-cli run [--cmd ID [--params JSON] | --commands FILE|-]... [--bundled-fonts]
      Runs commands one after another in a new headless session, printing a JSON line per command:
      {\"step\": N, \"command\": ID, \"result\": ...}. The first command that fails ends the run: it is named on
      standard error and the exit status is 1. No document is open at the start, so begin with --cmd file.new.
        --cmd ID        a command id, such as file.new or page.add
        --params JSON   a JSON object: the parameters of the --cmd just before it
        --commands SRC  a file, or - for standard input, holding steps in one of two forms. A JSON array
                        of step objects (16 MiB at most in all), or JSON Lines, a step object on each line
                        (16 MiB at most a line). A step object is {\"command\": ID, \"params\": {...}}, and
                        \"id\" is accepted in place of \"command\". Every step of every source is read and
                        checked before the first one runs.
        --bundled-fonts the bundled faces alone (see Fonts below)
      A run holds at most 100000 steps and 64 MiB of --commands text, all sources together.

  diacraft-cli commands
      Print every command, with its parameter doc and whether it can run, as JSON.

  diacraft-cli oracle FILE... [--json] [--source NAME]
  diacraft-cli oracle --aggregate REPORTS... [--cap N]
      The oracle harness (docs/cli.md): reads each FILE as a drawing package, works its formulas out and compares them with the values and
      errors it saved, never opening it in anything else. It reports its SHA-256, its category, the cell and function names the spec lists and how
      often it uses them, what did not parse, the sizes that confirm the caps, the class of the refusal if it would not open, and the score:
      every cell is compared, left out (with the reason), not calculated, or holds no formula. No formula text and no text content. Every run
      first scores a built document with known-wrong values; when that control fails there is no score and the exit status is 1.
        --json          one line of JSON for each file, in place of a few lines of words
        --source NAME   the repository or host the file came from, kept in the report for the aggregation
        --aggregate     the files are lines of reports printed by --json, and the output is their totals (reports, files, refused, scored,
                        cells compared, left out and not calculated) and their score by file, by source, and with each source cut to
                        --cap files (default 5). A line that cannot be true stops it.
      A file that cannot be read from disk is named on standard error and the exit status is 1 once the rest are done; a file that is not a
      package is a report with its class, and the exit status is 0.

  diacraft-cli export FILE --out PATH [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
      Writes one page of the drawing FILE as a picture, a vector file or a PDF, by the extension of PATH: .png, .svg or .pdf. Prints one
      JSON line: where it wrote, the page, the size, and what was left out or drawn by a stand-in, as the window counts it.
        --out PATH      where to write (replaced if it is there; never the drawing itself). Written whole or not at all.
        --page N        the page's place in the drawing, from 1 (the default is 1)
        --dpi N         the pixels to the inch of a PNG (96 by default, at most 2400)
        --text MODE     in an SVG or a PDF, text (the default: letters as text where the font allows, else outlines) or outlines
        --transparent   no white paper under the drawing (a PDF is then written without the white rectangle)
        --bundled-fonts the bundled faces alone (see Fonts below)
      The file is opened and the page drawn as the window draws it; the grid, rulers and guides are not in the picture.

  diacraft-cli convert FILE --out PATH [--drop-macros] [--drop-objects] [--page N] [--dpi N] [--text text|outlines] [--transparent] [--bundled-fonts]
      Converts the drawing, stencil or template FILE to another type, by the extension of PATH (in any case), and prints one JSON line: {in, out,
      format, bytes, ...}: in is the type FILE is (told by its bytes), format the type written.
      The types written are DiaCraft's own .diacraft, .diacraft-stencil and .diacraft-template,
      the Visio types .vsdx, .vsdm, .vssx, .vssm, .vstx and .vstm, the OpenDocument drawing .odg and template .otg, and the pictures .png, .svg and
      .pdf. Any other extension is a mistake in the command line (exit status 2, nothing written).
      A drawing type is a Save As: FILE is opened (a template as the file itself) and saved by the engine's own file.open and file.save_as, so what the
      engine does not write for this drawing is refused in its words (exit status 1, nothing written, a file that was at PATH left as it was): for
      example a stencil is never saved as an .odg, and a macro project is not written into a VSDX type that allows none. A drawing of a VSDX file is
      saved as an .odg or an .otg, and a drawing of an .odg as a VSDX type: what the file written cannot say rides inside it. The line
      of a drawing adds the save's own counts: variant, macros_dropped, parts (kept, rewritten, added, removed: a VSDX save), copied (the entries of
      a .diacraft package copied as they were), carried (what the file carries: classes, document_level, macros, objects and signatures_stale, as the
      engine's save result gives it: never a warning), checked (what the check after the save found, for a save through a model writer: the file is read
      back and compared with the drawing, class by class, as {done, classes: [{class, words, count}], as_values, reason}, and null for a save into
      the drawing's own file), warning_count and warnings (by class word, as roundtrip tells them).
        --drop-macros   a drawing type: leave the macro project out of the file (without it, a type that cannot hold the project refuses the save)
        --drop-objects  a drawing type: leave the embedded objects of the other format out of the file; each shape shows its picture
      A picture type is export's: the page is drawn as export draws it, and the line is export's with in. The options below are the picture's, and
      for a drawing type each is a mistake in the line.
        --out PATH      where to write (replaced if it is there; never FILE itself, a link to it included). Written whole or not at all.
        --page N        a picture: the page's place in the drawing, from 1 (the default is 1)
        --dpi N         a PNG: the pixels to the inch (96 by default, at most 2400)
        --text MODE     an SVG or a PDF: text (the default) or outlines
        --transparent   a picture: no white paper under the drawing
        --bundled-fonts a picture: the bundled faces alone (see Fonts below)

  diacraft-cli roundtrip FILE --out PATH [--move DX,DY] [--bundled-fonts]
  diacraft-cli roundtrip FILE --out PATH --via diacraft [--edit cell|move|retext|delete|add | --move DX,DY] [--bundled-fonts]
      Opens the drawing, stencil or template FILE (a template as the file itself), optionally moves the first shape of the first page that has
      one, saves it to PATH (the type is PATH's extension), and prints one JSON line of what the save changed: the SHA-256 of FILE, its variant,
      the parts of the two packages (how many are the same bytes; which differ, are new or are gone; whether the content types and the
      relationships say the same), whether the file read back equals the file read, and with --move the shapes and cells that differ. No
      formula, no value, no text and no name a person made up. A file that does not open is a report with the class of the refusal, and the
      exit status is 0. An OpenDocument drawing (.odg, .otg) is, until its reader exists, copied entry by entry to PATH (which keeps its
      extension) and the line says whether every part is the same (parts_identical) and which elements differ, by path (element_diff).
      A flat OpenDocument drawing (.fodg, one XML document, told by what the file is) is read, and written again unedited through the flat
      writer, which is the file's own bytes: the line says form flat, what was read, saved and file_identical. PATH must be a .fodg (another
      name is other_type and is not made), and --move is not supported.
        --out PATH      where to write (never FILE itself)
        --move DX,DY    inches to the right and up, before the save
        --bundled-fonts the bundled faces alone (see Fonts below)
      With --via diacraft the file makes the trip through DiaCraft's own format: it is opened, edited if asked, saved as .diacraft (a stencil as
      .diacraft-stencil, a template as .diacraft-template) in a scratch folder that is removed at the end, opened again in a new session, and saved to
      PATH in the format FILE came in (PATH must end in a VSDX type for a VSDX package, or in .odg for an OpenDocument drawing). It prints one JSON
      line: {via, format, input: {sha256, bytes}, opened, edit, diacraft: {bytes}, output: {sha256, bytes}, content: {compared, same, count,
      differences}, model: {same}}. content is the content check of the design's section 4.3 on FILE and PATH, the first 200 differences by part,
      path and kind, and the exact count; model says whether the file read back equals the drawing that was saved as .diacraft. A leg the engine
      refuses is a report with refusal: {leg, error, words} and the exit status 0, content has compared false and same null, and nothing says same.
      The file's content is never in the line: no formula, value, text or name a person made up.
        --via diacraft the trip through .diacraft (the only value)
        --edit KIND     before the trip, a salted edit of the first shape of the first page that has one: cell (shape.format, line weight 1.23),
                        move (shape.move 0.125,0.125), retext (shape.set_text \"salt\"), delete (shape.delete), or add (shape.drop of the master
                        rectangle at the page's centre). --move DX,DY is the move edit with a delta of your own. Not both, and only with --via

  diacraft-cli compare FIRST SECOND
  diacraft-cli compare --content FIRST SECOND
      Compares two OpenDocument drawings (.odg, .otg) and prints one JSON line: each file's SHA-256 and what its package holds, and what
      differs between them, part by part (parts_identical) and element by element (element_diff), with the rules of roundtrip's line. A file
      that does not open is a report with the class of the refusal, and the exit status is 0. A flat drawing (.fodg) is read, and has no
      parts to compare: parts.comparable is false, with the class flat_not_compared.
      With --content the two files are checked for the same content (the design's section 4.3), by the lists of their format: two VSDX packages,
      or two OpenDocument drawings (a flat one is compared with a package once it is split the way the reader splits it). The line is {format,
      first: {sha256, bytes}, second: {sha256, bytes}, content: {compared, same, count, differences}}. Two files of different formats are refused
      in words (exit status 1), and so is a .diacraft file, which has no content check of its own. A file that does not open is a report with the
      class of the refusal and the exit status 0.

Fonts: `run`, `export`, `roundtrip` and `convert` to a picture, and `mcp` for a session of this process (not --connect, and in the default mode only once it has chosen
the headless session), read the machine's fonts once, at the start and before any command, and the commands that measure
text (a text block that grows with its text) and the pictures they draw use that one set, as the window does. --bundled-fonts reads nothing from
the machine: the bundled faces are the same everywhere, so the numbers and the pictures of a run can be compared between machines. `oracle` has
its own rule: it measures in the machine's faces, and leaves out the cells a stand-in face measured.

Options:
  -V, --version  Print the version and exit
  -h, --help     Print this help and exit

Environment:
  RUST_LOG       off, error, warn (the default), info, debug or trace: what is logged to standard error

Exit status: 0 on success, 1 when a step, a connection or a file fails, 2 for a mistake in the command line.
";

/// Reads the command line (without the program name). `--bundled-fonts` is an option of the commands that set text (`mcp`, `run`, `export`,
/// `roundtrip` and `convert` to a picture); `oracle` has the faces of its own machine, which its report names, and `commands` sets none.
pub fn parse(args: &[String]) -> Result<Parsed, String> {
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Parsed { action: Action::Version, fonts: FontChoice::System });
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Parsed { action: Action::Help, fonts: FontChoice::System });
    }
    let Some((command, rest)) = args.split_first() else {
        return Err(format!("no command given\n\n{USAGE}"));
    };
    let mut fonts = FontChoice::System;
    let action = match command.as_str() {
        "commands" => match rest.first() {
            None => Ok(Action::Commands),
            Some(extra) => Err(format!("`commands` takes no arguments; unexpected `{}`", shown(extra, MOST_SHOWN))),
        },
        "mcp" => mcp(rest, &mut fonts).map(Action::Mcp),
        "run" => run(rest, &mut fonts).map(Action::Run),
        "oracle" => oracle(rest).map(Action::Oracle),
        "export" => export(rest, &mut fonts).map(Action::Export),
        "convert" => convert(rest, &mut fonts).map(Action::Convert),
        "roundtrip" => roundtrip(rest, &mut fonts).map(Action::Roundtrip),
        "compare" => compare(rest).map(Action::Compare),
        other => Err(format!("unknown command `{}`; see --help", shown(other, MOST_SHOWN))),
    }?;
    Ok(Parsed { action, fonts })
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

fn export(args: &[String], fonts: &mut FontChoice) -> Result<ExportArgs, String> {
    let (mut file, mut page, mut out, mut dpi, mut text, mut transparent) =
        (None::<String>, None::<usize>, None::<String>, None::<f64>, None::<String>, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bundled-fonts" {
            bundled_fonts(fonts)?;
        } else if arg == "--transparent" {
            if std::mem::replace(&mut transparent, true) {
                return Err("--transparent was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--out", &mut rest)? {
            if out.replace(value.to_owned()).is_some() {
                return Err("--out was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--page", &mut rest)? {
            if page.replace(page_value(value)?).is_some() {
                return Err("--page was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--dpi", &mut rest)? {
            if dpi.replace(dpi_value(value)?).is_some() {
                return Err("--dpi was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--text", &mut rest)? {
            if text.replace(text_value(value)?.to_owned()).is_some() {
                return Err("--text was given twice".to_owned());
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option `{}` for `export`; see --help", shown(arg, MOST_SHOWN)));
        } else if file.replace(arg.clone()).is_some() {
            return Err("`export` takes one drawing: a second file was given".to_owned());
        }
    }
    let file = file.ok_or_else(|| "nothing to export: give the drawing's file".to_owned())?;
    let out = out.ok_or_else(|| "nowhere to write: give --out PATH".to_owned())?;
    let format = Format::of(&out).ok_or_else(|| "--out must end in .png, .svg or .pdf: that says what to write".to_owned())?;
    if dpi.is_some() && format != Format::Png {
        return Err("--dpi is for a PNG: an SVG is in CSS pixels and a PDF in points".to_owned());
    }
    if text.is_some() && format == Format::Png {
        return Err("--text is for an SVG or a PDF: a PNG has only pixels".to_owned());
    }
    Ok(ExportArgs { file, page: page.unwrap_or(1), out, format, dpi, outlines: text.as_deref() == Some("outlines"), transparent })
}

fn convert(args: &[String], fonts: &mut FontChoice) -> Result<ConvertArgs, String> {
    let (mut file, mut page, mut out, mut dpi, mut text, mut transparent) =
        (None::<String>, None::<usize>, None::<String>, None::<f64>, None::<String>, false);
    let (mut drop_macros, mut drop_objects) = (false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bundled-fonts" {
            bundled_fonts(fonts)?;
        } else if arg == "--drop-macros" {
            if std::mem::replace(&mut drop_macros, true) {
                return Err("--drop-macros was given twice".to_owned());
            }
        } else if arg == "--drop-objects" {
            if std::mem::replace(&mut drop_objects, true) {
                return Err("--drop-objects was given twice".to_owned());
            }
        } else if arg == "--transparent" {
            if std::mem::replace(&mut transparent, true) {
                return Err("--transparent was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--out", &mut rest)? {
            if out.replace(value.to_owned()).is_some() {
                return Err("--out was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--page", &mut rest)? {
            if page.replace(page_value(value)?).is_some() {
                return Err("--page was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--dpi", &mut rest)? {
            if dpi.replace(dpi_value(value)?).is_some() {
                return Err("--dpi was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--text", &mut rest)? {
            if text.replace(text_value(value)?.to_owned()).is_some() {
                return Err("--text was given twice".to_owned());
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option `{}` for `convert`; see --help", shown(arg, MOST_SHOWN)));
        } else if file.replace(arg.clone()).is_some() {
            return Err("`convert` takes one drawing: a second file was given".to_owned());
        }
    }
    let file = file.ok_or_else(|| "nothing to convert: give the drawing's file".to_owned())?;
    let out = out.ok_or_else(|| "nowhere to write: give --out PATH".to_owned())?;
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
            page: page.unwrap_or(1),
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

fn roundtrip(args: &[String], fonts: &mut FontChoice) -> Result<RoundtripArgs, String> {
    let (mut file, mut out, mut shift) = (None::<String>, None::<String>, None::<(f64, f64)>);
    let (mut via, mut edit) = (None::<Via>, None::<EditKind>);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bundled-fonts" {
            bundled_fonts(fonts)?;
        } else if let Some(value) = value_of(arg, "--out", &mut rest)? {
            if out.replace(value.to_owned()).is_some() {
                return Err("--out was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--via", &mut rest)? {
            if value != "diacraft" {
                return Err("--via must be `diacraft`: the only format a file can visit".to_owned());
            }
            if via.replace(Via::Diacraft).is_some() {
                return Err("--via was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--edit", &mut rest)? {
            let Some(kind) = EditKind::of(value) else {
                return Err("--edit must be `cell`, `move`, `retext`, `delete` or `add`".to_owned());
            };
            if edit.replace(kind).is_some() {
                return Err("--edit was given twice".to_owned());
            }
        } else if let Some(value) = value_of(arg, "--move", &mut rest)? {
            let parsed = value.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?)));
            let Some((dx, dy)) = parsed.filter(|(x, y)| x.is_finite() && y.is_finite() && x.abs() <= MAX_MOVE && y.abs() <= MAX_MOVE) else {
                return Err(format!("--move must be two numbers in inches, such as 1.5,-0.25 (each at most {MAX_MOVE} either way)"));
            };
            if shift.replace((dx, dy)).is_some() {
                return Err("--move was given twice".to_owned());
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option `{}` for `roundtrip`; see --help", shown(arg, MOST_SHOWN)));
        } else if file.replace(arg.clone()).is_some() {
            return Err("`roundtrip` takes one file: a second was given".to_owned());
        }
    }
    let file = file.ok_or_else(|| "nothing to open: give the file".to_owned())?;
    let out = out.ok_or_else(|| "nowhere to save: give --out PATH".to_owned())?;
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

fn compare(args: &[String]) -> Result<CompareArgs, String> {
    let mut content = false;
    let mut files: Vec<&String> = Vec::new();
    for arg in args {
        if arg == "--content" {
            if std::mem::replace(&mut content, true) {
                return Err("--content was given twice".to_owned());
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option `{}` for `compare`; see --help", shown(arg, MOST_SHOWN)));
        } else {
            files.push(arg);
        }
    }
    match files.as_slice() {
        [first, second] => Ok(CompareArgs { first: (*first).clone(), second: (*second).clone(), content }),
        _ => Err(format!("`compare` takes two files, and {} were given", files.len())),
    }
}

/// The value of an option written `--name VALUE` or `--name=VALUE`, taken from `rest`.
fn value_of<'a>(arg: &'a str, name: &str, rest: &mut std::slice::Iter<'a, String>) -> Result<Option<&'a str>, String> {
    if arg == name {
        return rest.next().map(|v| Some(v.as_str())).ok_or_else(|| format!("{name} needs a value"));
    }
    Ok(arg.strip_prefix(name).and_then(|tail| tail.strip_prefix('=')))
}

fn mcp(args: &[String], fonts: &mut FontChoice) -> Result<Mode, String> {
    let (mut headless, mut connect) = (false, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bundled-fonts" {
            bundled_fonts(fonts)?;
        } else if arg == "--headless" {
            if std::mem::replace(&mut headless, true) {
                return Err("--headless was given twice".to_owned());
            }
        } else if let Some(addr) = value_of(arg, "--connect", &mut rest)? {
            if connect.replace(addr.to_owned()).is_some() {
                return Err("--connect was given twice".to_owned());
            }
        } else {
            return Err(format!("unknown option `{}` for `mcp`; see --help", shown(arg, MOST_SHOWN)));
        }
    }
    match (headless, connect) {
        (true, Some(_)) => Err("give either --connect or --headless, not both".to_owned()),
        (true, None) => Ok(Mode::Headless),
        // The session is the running app's and has the app's faces: the option would do nothing, and nothing is ignored.
        (false, Some(_)) if *fonts == FontChoice::Bundled => {
            Err("--bundled-fonts is for a session of this process, and the app's session has the app's faces: give --headless or neither".to_owned())
        }
        (false, Some(addr)) => Ok(Mode::Connect(addr)),
        (false, None) => Ok(Mode::Auto),
    }
}

fn run(args: &[String], fonts: &mut FontChoice) -> Result<Vec<Source>, String> {
    let mut sources: Vec<Source> = Vec::new();
    // Whether the last source is a `--cmd` that has not been given its `--params` yet.
    let mut params_open = false;
    let mut stdin_used = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bundled-fonts" {
            bundled_fonts(fonts)?;
        } else if let Some(id) = value_of(arg, "--cmd", &mut rest)? {
            sources.push(Source::Cmd { id: id.to_owned(), params: Value::Object(serde_json::Map::new()) });
            params_open = true;
        } else if let Some(raw) = value_of(arg, "--params", &mut rest)? {
            let params = match serde_json::from_str::<Value>(raw) {
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
        } else if let Some(source) = value_of(arg, "--commands", &mut rest)? {
            if source == "-" && std::mem::replace(&mut stdin_used, true) {
                return Err("--commands - was given twice: standard input is read once".to_owned());
            }
            sources.push(Source::Commands(source.to_owned()));
            params_open = false;
        } else {
            return Err(format!("unknown option `{}` for `run`; see --help", shown(arg, MOST_SHOWN)));
        }
    }
    if sources.is_empty() {
        return Err("nothing to run: give --cmd or --commands".to_owned());
    }
    Ok(sources)
}

fn oracle(args: &[String]) -> Result<OracleArgs, String> {
    let mut out = OracleArgs { files: Vec::new(), json: false, source: None, aggregate: false, cap: DEFAULT_CAP };
    let mut cap_given = false;
    let mut only_files = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if only_files || !arg.starts_with("--") {
            out.files.push(arg.clone());
        } else if arg == "--" {
            only_files = true;
        } else if arg == "--json" {
            if std::mem::replace(&mut out.json, true) {
                return Err("--json was given twice".to_owned());
            }
        } else if arg == "--aggregate" {
            if std::mem::replace(&mut out.aggregate, true) {
                return Err("--aggregate was given twice".to_owned());
            }
        } else if let Some(source) = value_of(arg, "--source", &mut rest)? {
            if out.source.replace(source.to_owned()).is_some() {
                return Err("--source was given twice".to_owned());
            }
        } else if let Some(cap) = value_of(arg, "--cap", &mut rest)? {
            if std::mem::replace(&mut cap_given, true) {
                return Err("--cap was given twice".to_owned());
            }
            out.cap = match cap.parse::<usize>() {
                Ok(n) if n > 0 && cap.bytes().all(|b| b.is_ascii_digit()) => n,
                _ => return Err("--cap must be a whole number of files, 1 or more".to_owned()),
            };
        } else {
            return Err(format!("unknown option `{}` for `oracle`; see --help", shown(arg, MOST_SHOWN)));
        }
    }
    if out.files.is_empty() {
        return Err("nothing to report on: give one or more files".to_owned());
    }
    if out.aggregate && (out.source.is_some() || out.json) {
        return Err("--aggregate prints JSON and takes the sources from the reports: give neither --json nor --source".to_owned());
    }
    if cap_given && !out.aggregate {
        return Err("--cap is for --aggregate".to_owned());
    }
    Ok(out)
}
