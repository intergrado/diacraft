//! Part B: **how large a drawing the codec saves and opens** (close the gap to what the `.odg` and `.vsdx` readers admit).
//!
//! A drawing is built as a model, saved a part at a time into the bytes of its parts (what a package holds), let go, and opened a part at a time from those bytes, in
//! a child process held to `ulimit -v 3000000`. A child prints what it measured on a `CAPACITY` line: the shapes, the parts, the items and bytes of model the open was
//! charged, and the peak of the process in each phase. Two kinds of shape are measured, because the gap between them is what found:
//!
//! * **a shape of a diagram**: seven cells (two formulas), a text and a path of five rows of two cells; each shape has a position of its own;
//! * **a rectangle as the `.odg` reader makes it**: six cells and two sections (a path of five rows and four connection points), 167 items and 2.1 KB of XML. The reader
//!   shares what it can between rectangles, and so does the decode now.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_doc::{Cell, CellAddr, Document, Page, Shape, ShapeList, Value};
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, PartRole, PartSource, URN, decode_from, decode_shape, open_limits, save_parts};
#[cfg(target_os = "linux")]
use diacraft_sheetxml::model::{OPEN_ITEMS, OPEN_MODEL_BYTES};

use super::model_util::native_limits;

const SCENARIO: &str = "SHEETXML_CAPACITY_SCENARIO";
const D: &str = "diacraft";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// A shape of the kind a diagram has, as the codec writes it: seven cells (two with formulas), a text and a path of five rows of two cells.
fn typical_shape_xml() -> String {
    let cells = format!(
        "<{D}:Cell N=\"PinX\" V=\"1\"/><{D}:Cell N=\"PinY\" V=\"1\"/><{D}:Cell N=\"Width\" V=\"1\"/><{D}:Cell N=\"Height\" V=\"0.75\"/><{D}:Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><{D}:Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><{D}:Cell N=\"Angle\" V=\"0\"/>"
    );
    let points = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(ix, (t, x, y))| {
            format!("<{D}:Row IX=\"{}\" T=\"{t}\"><{D}:Cell N=\"X\" V=\"1\" F=\"{x}\"/><{D}:Cell N=\"Y\" V=\"1\" F=\"{y}\"/></{D}:Row>", ix + 1)
        })
        .collect();
    format!(
        "<{D}:shape id=\"1\"><{D}:sheet>{cells}<{D}:Section N=\"Geometry\" IX=\"0\">{rows}</{D}:Section></{D}:sheet><{D}:text><{D}:t>Label</{D}:t></{D}:text></{D}:shape>"
    )
}

/// A rectangle as the `.odg` reader makes it (the reader's own output for `<draw:rect>`, written by the codec): 36 cells, 9 rows, 2 sections.
fn rectangle_xml() -> String {
    let cell = |n: &str, v: &str, f: Option<&str>, u: Option<&str>| {
        format!(
            "<{D}:Cell N=\"{n}\" V=\"{v}\"{}{}/>",
            f.map_or(String::new(), |f| format!(" F=\"{f}\"")),
            u.map_or(String::new(), |u| format!(" U=\"{u}\""))
        )
    };
    let row = |ix: u32, t: Option<&str>, cells: String| {
        format!("<{D}:Row IX=\"{ix}\"{}>{cells}</{D}:Row>", t.map_or(String::new(), |t| format!(" T=\"{t}\"")))
    };
    let direct = [
        cell("Height", "0.984251968503937", None, Some("CM")),
        cell("LocPinX", "0.7874015748031495", Some("Width*0.5"), Some("CM")),
        cell("LocPinY", "0.4921259842519685", Some("Height*0.5"), Some("CM")),
        cell("PinX", "1.2598425196850394", None, Some("CM")),
        cell("PinY", "10.019685039370078", None, Some("CM")),
        cell("Width", "1.574803149606299", None, Some("CM")),
    ]
    .concat();
    let geometry = [
        row(1, Some("RelMoveTo"), cell("X", "0", None, None) + &cell("Y", "0", None, None)),
        row(2, Some("RelLineTo"), cell("X", "1", None, None) + &cell("Y", "0", None, None)),
        row(3, Some("RelLineTo"), cell("X", "1", None, None) + &cell("Y", "1", None, None)),
        row(4, Some("RelLineTo"), cell("X", "0", None, None) + &cell("Y", "1", None, None)),
        row(5, Some("RelLineTo"), cell("X", "0", None, None) + &cell("Y", "0", None, None)),
    ]
    .concat();
    let connection = |ix: u32, dx: &str, dy: &str, x: (&str, Option<&str>), y: (&str, Option<&str>)| {
        row(
            ix,
            None,
            cell("DirX", dx, None, None)
                + &cell("DirY", dy, None, None)
                + &cell("Type", "0", None, None)
                + &cell("X", x.0, x.1, Some("CM"))
                + &cell("Y", y.0, y.1, Some("CM")),
        )
    };
    let connections = [
        connection(0, "0", "1", ("0.7874015748031495", Some("Width*0.5")), ("0.984251968503937", Some("Height"))),
        connection(1, "-1", "0", ("0", None), ("0.4921259842519685", Some("Height*0.5"))),
        connection(2, "0", "-1", ("0.7874015748031495", Some("Width*0.5")), ("0", None)),
        connection(3, "1", "0", ("1.574803149606299", Some("Width")), ("0.4921259842519685", Some("Height*0.5"))),
    ]
    .concat();
    format!(
        "<{D}:shape id=\"1\" name=\"s1\" name-u=\"s1\" type=\"Shape\"><{D}:sheet LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">{direct}<{D}:Section N=\"Geometry\" IX=\"0\">{geometry}</{D}:Section><{D}:Section N=\"Connection\">{connections}</{D}:Section></{D}:sheet></{D}:shape>"
    )
}

pub(super) fn base_shape(kind: &str) -> Shape {
    let xml = match kind {
        "typical" => typical_shape_xml(),
        "rectangle" => rectangle_xml(),
        other => panic!("no kind {other}"),
    };
    let text = format!("<{D}:shape xmlns:{D}=\"{URN}\"{}", xml.strip_prefix(&format!("<{D}:shape")).unwrap());
    let el = parse_with(text.as_bytes(), &native_limits()).unwrap();
    decode_shape(&el, &MemBlobs::new(), &mut Budget::new(1 << 40, 1 << 40, 1 << 40), &mut Vec::new()).unwrap()
}

/// A drawing of `n` shapes of `kind` spread over `pages` pages, each with a position of its own (the way a drawing's shapes differ).
pub(super) fn drawing(kind: &str, n: usize, pages: usize) -> Document {
    let base = base_shape(kind);
    let mut doc = Document::empty();
    let per = n.div_ceil(pages.max(1));
    let mut made = 0usize;
    for p in 0..pages {
        let mut page = Page::new(u32::try_from(p + 1).unwrap(), &format!("Page {}", p + 1));
        let mut shapes = Vec::with_capacity(per);
        while shapes.len() < per && made < n {
            made += 1;
            let mut s = base.clone();
            s.id = u32::try_from(made).unwrap();
            let sheet = Arc::make_mut(&mut s.sheet);
            let at = |name: &str| CellAddr::direct(name);
            let (x, y) = (format!("{}.{}", made % 997, made % 89), format!("{}.{}", made / 997, made % 83));
            sheet.set_cell(&at("PinX"), Cell::constant(Value::from_text(&x))).unwrap();
            sheet.set_cell(&at("PinY"), Cell::constant(Value::from_text(&y))).unwrap();
            shapes.push(Arc::new(s));
        }
        page.shapes = ShapeList::from_vec(shapes);
        doc.pages.push(Arc::new(page));
    }
    doc
}

/// The parts of a package as bytes, one entry each.
struct Package {
    document: Vec<u8>,
    pages: Vec<Vec<u8>>,
    masters: Vec<Vec<u8>>,
}

/// A source that parses a part when it is asked for it and lets its bytes go with the tree it makes.
struct FromBytes<'a> {
    pages: &'a [Vec<u8>],
    masters: &'a [Vec<u8>],
    limits: &'a Limits,
}

impl PartSource for FromBytes<'_> {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        let bytes = self.pages.get(index).ok_or_else(|| CodecError::Malformed { part: "page".to_owned(), why: "no such part" })?;
        parse_with(bytes, &self.limits.xml).map_err(|_| CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" })
    }

    fn master(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        let bytes = self.masters.get(index).ok_or_else(|| CodecError::Malformed { part: "master".to_owned(), why: "no such part" })?;
        parse_with(bytes, &self.limits.xml).map_err(|_| CodecError::Malformed { part: "master".to_owned(), why: "a part the reader refuses" })
    }
}

/// What one run of a scenario measured.
struct Run {
    saved: Result<(), String>,
    opened: Option<Result<(), String>>,
    items: u64,
    model: u64,
    save_peak_mib: u64,
    open_peak_mib: u64,
    parts: usize,
    bytes: u64,
}

/// `kind:n:pages`: a drawing made, saved a part at a time, let go, and opened a part at a time from the bytes of what was saved.
fn run_scenario(spec: &str) -> Run {
    let mut it = spec.split(':');
    let (kind, n, pages) = (
        it.next().unwrap_or("typical"),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(1000usize),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(1usize),
    );
    let limits = open_limits();
    let doc = drawing(kind, n, pages);
    let before = status_kib("VmHWM");
    let mut blobs = MemBlobs::new();
    let mut package = Package { document: Vec::new(), pages: Vec::new(), masters: Vec::new() };
    let mut bytes = 0u64;
    let saved = save_parts(&doc, &mut blobs, &limits, &mut |part| {
        let b = to_bytes(part.tree, &[], &[], true).map_err(CodecError::from)?;
        bytes += b.len() as u64;
        match part.role {
            PartRole::Document => package.document = b,
            PartRole::Page { .. } => package.pages.push(b),
            PartRole::Master { .. } => package.masters.push(b),
        }
        Ok(())
    });
    let save_peak = status_kib("VmHWM").max(before);
    let parts = 1 + package.pages.len() + package.masters.len();
    let mut run = Run {
        saved: saved.map_err(|e| e.to_string()),
        opened: None,
        items: 0,
        model: 0,
        save_peak_mib: save_peak >> 10,
        open_peak_mib: 0,
        parts,
        bytes,
    };
    if run.saved.is_err() {
        return run;
    }
    drop(doc);
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    let document = parse_with(&package.document, &limits.xml).expect("the document part");
    let mut budget = Budget::for_open(&limits);
    let start = (budget.items, budget.bytes);
    let mut source = FromBytes { pages: &package.pages, masters: &package.masters, limits: &limits };
    let opened = decode_from(&document, &mut source, &blobs, &mut budget, &mut Vec::new());
    run.open_peak_mib = status_kib("VmHWM") >> 10;
    run.items = start.0 - budget.items;
    run.model = start.1 - budget.bytes;
    run.opened = Some(opened.map(drop).map_err(|e| e.to_string()));
    run
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn capacity_child() {
    let spec = std::env::var(SCENARIO).unwrap_or_default();
    let r = run_scenario(&spec);
    println!(
        "CAPACITY {spec} parts={} package_mib={} saved={:?} opened={:?} items={} model_mib={} save_peak_mib={} open_peak_mib={} vm_peak_mib={}",
        r.parts,
        r.bytes >> 20,
        r.saved.as_ref().err().map_or("yes", |e| e.as_str()),
        r.opened.as_ref().map(|o| o.as_ref().err().map_or("yes", |e| e.as_str())),
        r.items,
        r.model >> 20,
        r.save_peak_mib,
        r.open_peak_mib,
        status_kib("VmPeak") >> 10
    );
    println!("capacity child: no abort");
}

/// Runs `spec` in a child held to 3,000,000 KiB of address space and returns the `CAPACITY` line; an abort is the failure.
#[cfg(target_os = "linux")]
fn child(spec: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_capacity::capacity_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, spec).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("capacity child: no abort"), "{spec}: {text}");
    let line =
        text.lines().find_map(|l| l.find("CAPACITY ").map(|at| l[at..].to_owned())).unwrap_or_else(|| panic!("{spec}: no CAPACITY line in {text}"));
    eprintln!("{line}");
    line
}

#[cfg(target_os = "linux")]
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace().find_map(|w| w.strip_prefix(&format!("{key}="))).unwrap_or_else(|| panic!("no {key} in {line}"))
}

#[test]
#[cfg(target_os = "linux")]
fn ten_thousand_shapes_of_a_diagram_and_of_the_odg_reader_are_saved_and_opened_under_an_address_limit() {
    for kind in ["typical", "rectangle"] {
        let line = child(&format!("{kind}:10000:2"));
        assert_eq!(field(&line, "saved"), "\"yes\"", "{line}");
        assert_eq!(field(&line, "opened"), "Some(\"yes\")", "{line}");
    }
    let _ = (OPEN_ITEMS, OPEN_MODEL_BYTES);
}

/// The measurements the report states (`cargo test -- --ignored capacity_numbers --nocapture`): a drawing at each size, so that the sizes where the save and the open
/// each refuse are found by hand from the lines.
#[test]
#[ignore = "a measurement: it prints what each size of drawing is saved and opened as"]
#[cfg(target_os = "linux")]
fn capacity_numbers() {
    let sizes = std::env::var("CAPACITY_SIZES").unwrap_or_else(|_| "typical:40000:1,typical:40000:4".to_owned());
    for spec in sizes.split(',') {
        let _ = child(spec);
    }
}
