//! `diacraft-cli export`: one page of a drawing as a PNG, an SVG or a PDF.
//!
//! **The drawing is read as `file.open` reads it** (`diacraft_engine::file::read_file`, then the reader of the format), so a file that cannot
//! be opened is the same error in words, with the kind of trouble and never the path. **The page is drawn as the window draws it**, through
//! `diacraft_render`'s walk, so what the export leaves out or draws by a stand-in is the window's own list, printed.
//!
//! **Where it writes.** Exactly where `--out` says: this command line has no granted folders yet (the automation roots of gaps A2 come with
//! the engine's `file.export` and the MCP tools, which write for a client that was not the one who typed the path). The file is written whole
//! or not at all (`craft_atomic`: a temporary file beside it, then a rename), so a failure leaves any file that was there as it was, and
//! `--out` is never the drawing itself.
//!
//! **`convert` to a picture type** goes through this path: [`make`] is the read, the draw and the write, and `convert` adds the type of the drawing to the line.

use std::path::Path;

use craft_atomic::atomic_write;
use diacraft_color::Rgba;
use diacraft_engine::file::read_file;
use diacraft_engine::shown;
use diacraft_pdf::PdfOptions;
use diacraft_render::export::{ExportError, Exported, TextMode, export_png};
use diacraft_render::{Parallel, Renderer, default_threads};
use diacraft_svg::SvgOptions;
use diacraft_vsdx::Limits;
use serde_json::{Value, json};

use crate::args::{ExportArgs, Format};
use crate::{Failure, write_line};

/// Bytes a file may hold beyond what a package may inflate to, as the container's own overhead (as `file.open` has it).
const FILE_SLACK: u64 = 64 * 1024 * 1024;

/// The most characters of a message from the drawing that is repeated.
const MOST_SHOWN: usize = 300;

/// The pixels to the inch of a PNG when `--dpi` is not given.
const DEFAULT_DPI: f64 = 96.0;

pub fn run(args: ExportArgs) -> Result<(), Failure> {
    let (line, _input) = make(&args)?;
    write_line(&serde_json::to_string(&line).map_err(|e| Failure::Message(format!("cannot encode the result: {e}")))?)
}

/// Reads the drawing, draws the page and writes the picture (`export` and `convert` to a picture type): the line `export` prints, and the type of the drawing that was read, as `file.open` names it
/// (`vsdx`, `diacraft-stencil`, `odg`...; `None` for a package that is none of the six VSDX types).
pub fn make(args: &ExportArgs) -> Result<(Value, Option<&'static str>), Failure> {
    let limits = Limits::default();
    let max = limits.opc.max_package_bytes.saturating_add(FILE_SLACK);
    let bytes = read_file(Path::new(&args.file), max).map_err(|e| Failure::Message(format!("cannot read the drawing: {e}")))?;
    let cannot = |e: &dyn std::fmt::Display| Failure::Message(format!("cannot open the drawing: {}", shown(&e.to_string(), MOST_SHOWN)));
    // The file's own bytes decide, as for `file.open`, and in its order: a `.diacraft` file is read by its own reader (within what the process leaves of the memory budget), then an
    // OpenDocument drawing, then a VSDX package.
    let (doc, input) = if diacraft_engine::nativefile::sniff(&bytes).is_some() {
        let limits = diacraft_engine::nativefile::Limits::beside(diacraft_engine::memory::held());
        let read = diacraft_engine::nativefile::read_native(&bytes, &limits).map_err(|e| cannot(&e))?;
        (read.doc, Some(diacraft_engine::nativefile::variant_word(read.opened.kind)))
    } else if diacraft_odg::sniff(&bytes).is_some() {
        let (doc, _report) = diacraft_engine::odgfile::read_odg(&bytes, &diacraft_odg::Limits::default()).map_err(|e| cannot(&e))?;
        (doc, Some("odg"))
    } else {
        let (doc, report) = diacraft_vsdx::read(&bytes, &limits).map_err(|e| cannot(&e))?;
        (doc, report.variant.map(|variant| variant.extension()))
    };
    let count = doc.pages.len();
    let page = args.page.checked_sub(1).and_then(|at| doc.pages.get(at)).ok_or_else(|| {
        let pages = if count == 1 { "1 page".to_owned() } else { format!("{count} pages") };
        Failure::Message(format!("the drawing has {pages}: there is no page {}", args.page))
    })?;
    refuse_the_drawing_itself(&args.file, &args.out)?;

    let paper = (!args.transparent).then_some(Rgba::WHITE);
    let text = if args.outlines { TextMode::AsOutlines } else { TextMode::AsText };
    let title = Some(page.name.to_string());
    // The faces `main` read once for the run (the machine's first, as the window does, or the bundled ones with `--bundled-fonts`): the same
    // database the engine's commits measure in, so that text is laid out in the faces a block was sized in.
    let mut renderer = Renderer::with_shared_fonts(diacraft_engine::measure::renderer_fonts());
    renderer.set_paper(paper);
    // A PNG is drawn on the machine's cores less one (`DIACRAFT_THREADS` says otherwise); the picture is the same bytes at any count.
    renderer.set_parallel(Parallel::with_threads(default_threads()));
    let made: Result<Exported, ExportError> = match args.format {
        Format::Png => export_png(&mut renderer, &doc, page.id, args.dpi.unwrap_or(DEFAULT_DPI)),
        Format::Svg => diacraft_svg::export_page(&mut renderer, &doc, page.id, &SvgOptions { paper, text, title, ..SvgOptions::default() }),
        Format::Pdf => diacraft_pdf::export_page(&mut renderer, &doc, page.id, &PdfOptions { paper, text, title, ..PdfOptions::default() }),
    };
    let exported = made.map_err(|e| Failure::Message(e.to_string()))?;
    atomic_write(Path::new(&args.out), &exported.bytes).map_err(|e| Failure::Message(format!("cannot write the file: {}", describe(&e))))?;

    let skipped: Vec<Value> =
        exported.skipped.iter().map(|s| json!({"kind": format!("{:?}", s.kind), "count": s.count, "first": shown(&s.first, MOST_SHOWN)})).collect();
    let line = json!({
        "out": args.out,
        "format": args.format.name(),
        "page": {"id": page.id, "name": shown(&page.name, MOST_SHOWN)},
        "width": exported.width,
        "height": exported.height,
        "bytes": exported.bytes.len(),
        "skipped": skipped,
    });
    Ok((line, input))
}

/// `out` must not be the drawing `file`: writing over it would replace a drawing with a picture of itself, or with a copy of itself that nothing asked for. The two are the same when
/// they are one file by their real paths, so a link to the drawing, and another spelling of its path, are caught. (A second name for the one file that is no link, a hard link, is not the same
/// path: a write goes to a new file beside the target and is renamed over it, which replaces that name and not the file the drawing is.)
pub fn refuse_the_drawing_itself(file: &str, out: &str) -> Result<(), Failure> {
    let same = match (std::fs::canonicalize(file), std::fs::canonicalize(out)) {
        (Ok(a), Ok(b)) => a == b,
        // The output is not there yet, so it is not the drawing.
        _ => false,
    };
    if same { Err(Failure::Message("--out is the drawing itself: it would be written over".to_owned())) } else { Ok(()) }
}

/// What the system said, as words: its kind, and not its text, which carries the path.
fn describe(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "a folder in the path is not there",
        std::io::ErrorKind::PermissionDenied => "permission was refused",
        std::io::ErrorKind::IsADirectory => "the path is a folder",
        std::io::ErrorKind::StorageFull => "the disk is full",
        _ => "the system refused",
    }
}
