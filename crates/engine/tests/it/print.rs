//! `file.print_preview` and `file.print`: what a print of the drawing puts on paper, and the PDF of it. They change nothing in the drawing,
//! `file.print` is judged by the rules of an export's path before anything is written, and the plan is read from the cells of an opened file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_engine::print::{Pages, plan};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// A scratch folder under `TMPDIR`, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-print-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cells(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(n, v)| format!(r#"<Cell N="{n}" V="{v}"/>"#)).collect()
}

/// A drawing read from a file whose pages carry these cells: a page is `(name, cells)`.
fn drawing(dir: &Dir, name: &str, pages: &[(&str, &[(&str, &str)])]) -> Session {
    let mut builder = VsdxBuilder::new();
    for (i, (page_name, extra)) in pages.iter().enumerate() {
        let mut all = vec![("PageScale", "1"), ("DrawingScale", "1")];
        all.extend_from_slice(extra);
        builder = builder.page(PageSpec::new(i as u32 + 1, page_name).sheet(&cells(&all)));
    }
    std::fs::write(dir.path(name), builder.build()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.path(name)}));
    s
}

/// What a print must leave as it was: the document, its history, its revision, and whether it is unsaved. (A command that ran is in the journal.)
fn untouched(s: &Session) -> (Option<diacraft_doc::Document>, usize, usize, u64, bool, Option<PathBuf>) {
    let snap = snapshot(s);
    (snap.document, snap.undo_steps, snap.redo_steps, snap.revision, s.is_dirty(), s.path().map(PathBuf::from))
}

fn fails(s: &mut Session, id: &str, params: Value) -> EngineError {
    s.execute(id, &params).unwrap_err()
}

#[test]
fn the_plan_is_read_from_the_cells_of_an_opened_file() {
    let dir = Dir::new();
    // A 17 by 11 inch page on A4 sideways with a half inch left margin; a second page, small, on Legal.
    let mut s = drawing(
        &dir,
        "d.vsdx",
        &[
            ("Wide", &[("PageWidth", "17"), ("PageHeight", "11"), ("PaperKind", "9"), ("PrintPageOrientation", "2"), ("PageLeftMargin", "0.5")]),
            ("Small", &[("PageWidth", "4"), ("PageHeight", "3"), ("PaperKind", "5")]),
        ],
    );
    let before = untouched(&s);
    let answer = run(&mut s, "file.print_preview", json!({}));
    assert_eq!(untouched(&s), before, "a preview changes nothing, and a query is not journaled");
    let wide = &answer["pages"][0];
    assert_eq!(wide["page"], json!({"id": 1, "name": "Wide"}));
    assert_eq!(wide["paper"]["name"], "A4");
    assert_eq!(wide["orientation"], "landscape");
    // A4 sideways is 11.693 by 8.268 inches; a half inch on the left and a quarter on the other sides leave 10.943 by 7.768.
    let printable = &wide["printable"];
    assert!((printable["width"].as_f64().unwrap() - (297.0 / 25.4 - 0.75)).abs() < 1e-9, "{printable}");
    assert!((printable["height"].as_f64().unwrap() - (210.0 / 25.4 - 0.5)).abs() < 1e-9, "{printable}");
    assert_eq!((wide["across"].as_u64(), wide["down"].as_u64()), (Some(2), Some(2)), "17 / 10.943 across, 11 / 7.768 down");
    assert_eq!(wide["sheets"].as_array().unwrap().len(), 4);
    assert_eq!(wide["sheets"][1], json!({"number": 1, "column": 1, "row": 0}));
    let small = &answer["pages"][1];
    // The small page says nothing of the way its sheet is held, so it is held the way the page is: wider than tall, sideways.
    assert_eq!((small["paper"]["name"].as_str(), small["orientation"].as_str()), (Some("Legal"), Some("landscape")));
    assert_eq!(small["sheet"], json!({"width": 14.0, "height": 8.5}));
    assert_eq!(small["sheets"][0]["number"], 4, "the sheets are numbered through the print");
    assert_eq!(answer["sheet_count"], 5);
    // Two kinds of paper make two jobs.
    let jobs = answer["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!((jobs[0]["first"].as_u64(), jobs[0]["sheets"].as_u64()), (Some(0), Some(4)));
    assert_eq!((jobs[1]["first"].as_u64(), jobs[1]["sheets"].as_u64()), (Some(4), Some(1)));
    // One page asked for.
    let one = run(&mut s, "file.print_preview", json!({"page": 2}));
    assert_eq!(one["sheet_count"], 1);
    assert_eq!(one["pages"][0]["page"]["name"], "Small");
}

#[test]
fn a_blank_drawing_prints_on_one_sheet() {
    let mut s = blank_session();
    let answer = run(&mut s, "file.print_preview", json!({}));
    assert_eq!(answer["sheet_count"], 1, "{answer}");
    assert_eq!(answer["pages"].as_array().unwrap().len(), 1);
    // The new page is Letter sideways with no margins: the page is the sheet.
    let page = &answer["pages"][0];
    assert_eq!((page["paper"]["name"].as_str(), page["orientation"].as_str()), (Some("Letter"), Some("landscape")));
    assert_eq!(page["printable"], json!({"left": 0.0, "top": 0.0, "width": 11.0, "height": 8.5}));
}

#[test]
fn a_print_that_cannot_be_planned_is_an_error_in_words_and_changes_nothing() {
    let dir = Dir::new();
    let mut s = drawing(&dir, "d.vsdx", &[("Big", &[("PageWidth", "100000"), ("PageHeight", "100000")])]);
    let before = untouched(&s);
    let error = fails(&mut s, "file.print_preview", json!({}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("sheets of paper"), "{error}");
    let error = fails(&mut s, "file.print_preview", json!({"page": 9}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("no page 9"), "{error}");
    let error = fails(&mut s, "file.print_preview", json!({"page": "one"}));
    assert!(matches!(error, EngineError::BadParams { .. }), "{error:?}");
    assert_eq!(untouched(&s), before);
    assert!(matches!(Session::new().execute("file.print_preview", &json!({})), Err(EngineError::Disabled { .. })));
}

#[test]
fn print_writes_a_pdf_with_a_page_for_each_sheet_and_the_drawing_is_not_touched() {
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2, "y": 2}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    let before = untouched(&s);
    let answer = run(&mut s, "file.print", json!({"path": dir.path("p.pdf")}));
    let file = std::fs::read(dir.path("p.pdf")).unwrap();
    assert!(file.starts_with(b"%PDF-"));
    assert_eq!(answer["bytes"].as_u64(), Some(file.len() as u64));
    assert_eq!(answer["sheet_count"], 2, "two pages, a sheet each");
    assert!(answer["skipped"].is_array());
    assert_eq!(untouched(&s), before, "a print changes nothing in the drawing");
    assert_eq!(dir.files(), ["p.pdf"], "no temporary file is left");
    assert_eq!(s.journal().last().map(|(id, _)| id.as_str()), Some("file.print"));
    // One page of the two, text as outlines, and the extension in any case.
    let one = run(&mut s, "file.print", json!({"path": dir.path("one.PDF"), "page": 2, "text": "outlines"}));
    assert_eq!(one["sheet_count"], 1);
    assert!(std::fs::read(dir.path("one.PDF")).unwrap().starts_with(b"%PDF-"));
    check_session(&s).unwrap();
}

/// Another reader, where the machine has one, counts the pages of the file the way the plan says.
#[test]
fn another_reader_finds_a_page_for_each_sheet() {
    use std::process::Command;
    if Command::new("pdfinfo").arg("-v").output().is_err() {
        eprintln!("SKIPPED: pdfinfo (poppler) is not on this machine");
        return;
    }
    let dir = Dir::new();
    let mut s = drawing(
        &dir,
        "d.vsdx",
        &[
            ("Wide", &[("PageWidth", "17"), ("PageHeight", "11"), ("PrintPageOrientation", "1")]),
            ("Small", &[("PageWidth", "4"), ("PageHeight", "3")]),
        ],
    );
    let answer = run(&mut s, "file.print", json!({"path": dir.path("p.pdf")}));
    assert_eq!(answer["sheet_count"], 7, "3 across and 2 down, then 1");
    let out = Command::new("pdfinfo").arg(dir.path("p.pdf")).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(text.contains("Pages: 7"), "{text}");
}

#[test]
fn what_cannot_be_asked_is_refused_before_anything_is_written() {
    let dir = Dir::new();
    let mut s = blank_session();
    let before = untouched(&s);
    let p = |name: &str| dir.path(name);
    for params in [
        json!({}),
        json!({"path": 5}),
        json!({"path": p("x")}),
        json!({"path": p("x.png")}),
        json!({"path": p("x.vsdx")}),
        json!({"path": p("x.pdf"), "text": "paths"}),
        json!({"path": p("x.pdf"), "text": 1}),
        json!({"path": p("x.pdf"), "page": -1}),
        json!({"path": p("x.pdf"), "copies": 2}),
        json!({"path": p("x.pdf"), "color": "sepia"}),
        json!({"path": p("x.pdf"), "color": true}),
    ] {
        let error = fails(&mut s, "file.print", params.clone());
        assert!(matches!(error, EngineError::BadParams { .. }), "{params}: {error:?}");
        assert!(!error.to_string().chars().any(char::is_control), "{error}");
        assert_eq!(untouched(&s), before, "{params}");
    }
    assert!(dir.files().is_empty(), "nothing was written: {:?}", dir.files());
    let error = fails(&mut s, "file.print", json!({"path": p("x.pdf"), "page": 99}));
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    assert!(dir.files().is_empty());
    assert!(matches!(Session::new().execute("file.print", &json!({"path": p("x.pdf")})), Err(EngineError::Disabled { .. })));
}

#[test]
fn a_print_in_a_batch_is_refused_because_a_file_cannot_be_taken_back() {
    let dir = Dir::new();
    let mut s = blank_session();
    let before = untouched(&s);
    let params = json!({"commands": [{"command": "page.add"}, {"command": "file.print", "params": {"path": dir.path("x.pdf")}}]});
    let error = s.execute("command.batch", &params).unwrap_err();
    assert!(error.to_string().contains("batch") || error.to_string().contains("file.print"), "{error}");
    assert_eq!(untouched(&s), before);
    assert!(dir.files().is_empty());
}

#[test]
fn the_open_drawing_is_never_written_whatever_the_path_is_spelled_like() {
    let dir = Dir::new();
    // The drawing's own file is named like a PDF: only the judgement of what the path is can keep it safe.
    let mut s = drawing(&dir, "d.pdf", &[("First", &[("PageWidth", "4"), ("PageHeight", "3")])]);
    let bytes = std::fs::read(dir.path("d.pdf")).unwrap();
    std::fs::create_dir(dir.path("sub")).unwrap();
    let before = untouched(&s);
    for path in [dir.path("d.pdf"), dir.path("sub/../d.pdf"), dir.path("./d.pdf")] {
        let error = fails(&mut s, "file.print", json!({"path": path}));
        assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("drawing's own file"), "{path}: {error}");
        assert_eq!(std::fs::read(dir.path("d.pdf")).unwrap(), bytes, "{path}: the drawing is as it was");
        assert_eq!(untouched(&s), before);
    }
    run(&mut s, "file.print", json!({"path": dir.path("e.pdf")}));
    assert_eq!(std::fs::read(dir.path("d.pdf")).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn a_link_is_refused_unless_it_leads_to_the_open_drawing_and_then_the_drawing_itself_is_refused() {
    use std::os::unix::fs::symlink;
    let dir = Dir::new();
    let mut s = drawing(&dir, "d.pdf", &[("First", &[("PageWidth", "4"), ("PageHeight", "3")])]);
    let bytes = std::fs::read(dir.path("d.pdf")).unwrap();
    std::fs::write(dir.path("other.txt"), b"another file").unwrap();
    symlink(dir.path("other.txt"), dir.path("l.pdf")).unwrap();
    let error = fails(&mut s, "file.print", json!({"path": dir.path("l.pdf")}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("link"), "{error}");
    assert_eq!(std::fs::read(dir.path("other.txt")).unwrap(), b"another file");
    symlink(dir.path("d.pdf"), dir.path("own.pdf")).unwrap();
    let error = fails(&mut s, "file.print", json!({"path": dir.path("own.pdf")}));
    assert!(matches!(error, EngineError::Failed { .. }) && error.to_string().contains("drawing's own file"), "{error}");
    assert_eq!(std::fs::read(dir.path("d.pdf")).unwrap(), bytes);
}

#[test]
fn the_plan_the_window_asks_for_is_the_one_the_command_answers() {
    // The window plans through `print::plan` and the command describes the same plan.
    let dir = Dir::new();
    let mut s = drawing(&dir, "d.vsdx", &[("Wide", &[("PageWidth", "17"), ("PageHeight", "11"), ("PrintPageOrientation", "1")])]);
    let direct = plan(s.document().unwrap(), Pages::All).unwrap();
    let answer = run(&mut s, "file.print_preview", json!({}));
    assert_eq!(answer, diacraft_engine::print::describe(&direct));
    assert_eq!(direct.sheet_count(), 6);
    assert_eq!(plan(s.document().unwrap(), Pages::One(1)).unwrap(), direct);
    assert!(plan(s.document().unwrap(), Pages::One(7)).is_err());
}

/// `color: grayscale` writes the print in black and white. Another reader, where the machine has one, draws the file with no colour in it,
/// and the same drawing printed in colour has its colours.
#[test]
fn a_print_in_grayscale_has_no_colour_in_it() {
    use std::process::Command;
    let dir = Dir::new();
    let mut s = blank_session();
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 4}));
    run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 7, "y": 4}));
    run(&mut s, "shape.format", json!({"ids": [1], "fill": "#E02020", "line": "#1030C0", "weight": 4}));
    run(&mut s, "shape.format", json!({"ids": [2], "fill": "#20B040", "line": "#E0A000", "weight": 3}));
    run(&mut s, "shape.set_text", json!({"id": 2, "page": 1, "text": "Green"}));
    run(&mut s, "text.format", json!({"ids": [2], "color": "#FFFF00", "size": 24}));
    let before = untouched(&s);
    let grey = run(&mut s, "file.print", json!({"path": dir.path("grey.pdf"), "color": "grayscale"}));
    let colour = run(&mut s, "file.print", json!({"path": dir.path("colour.pdf"), "color": "color"}));
    assert_eq!((grey["sheet_count"].as_u64(), colour["sheet_count"].as_u64()), (Some(1), Some(1)));
    assert_eq!(untouched(&s), before, "a print changes nothing in the drawing");
    if Command::new("pdftoppm").arg("-v").output().is_err() {
        eprintln!("SKIPPED: pdftoppm (poppler) is not on this machine");
        return;
    }
    let coloured = |name: &str| -> (usize, usize) {
        let ppm = dir.path(&format!("{name}.ppm"));
        let out =
            Command::new("pdftoppm").args(["-r", "24", "-singlefile"]).arg(dir.path(&format!("{name}.pdf"))).arg(dir.path(name)).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let bytes = std::fs::read(&ppm).unwrap();
        // A binary PPM: "P6", the width, the height and the most a sample may be, each followed by one blank, then the samples.
        let mut fields = 0;
        let mut start = 0;
        let mut in_field = false;
        for (i, b) in bytes.iter().enumerate() {
            if b.is_ascii_whitespace() {
                if in_field {
                    fields += 1;
                    in_field = false;
                    if fields == 4 {
                        start = i + 1;
                        break;
                    }
                }
            } else {
                in_field = true;
            }
        }
        let pixels = bytes[start..].as_chunks::<3>().0;
        let spread = |&[r, g, b]: &[u8; 3]| r.max(g).max(b) - r.min(g).min(b);
        (pixels.iter().filter(|p| spread(p) > 2).count(), pixels.iter().filter(|p| **p != [255, 255, 255]).count())
    };
    let (colour_pixels, _) = coloured("colour");
    let (grey_pixels, grey_ink) = coloured("grey");
    assert!(colour_pixels > 100, "the colour print has colour: {colour_pixels}");
    assert_eq!(grey_pixels, 0, "the print in grayscale has none");
    assert!(grey_ink > 100, "and the shapes are on it, in greys: {grey_ink}");
}
