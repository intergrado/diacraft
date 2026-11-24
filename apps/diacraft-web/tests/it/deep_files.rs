//! The deepest legal files and the largest drawings, for the browser. A WebAssembly stack is 1 MiB unless the link says more (`.cargo/config.toml`), and a drawing that nests, a formula that chains and
//! an XML part that goes deep are what recurse on it. The engine's own limits say how deep a file may be (the XML depth 200, the shape depth, the formula's nesting and chain); these files are the deepest of
//! each, built in code, and each is opened by the engine here, on a test thread (2 MiB, the stack the limits were checked on), as the browser opens it. The browser run opens the same files in the page:
//! set `DIACRAFT_WEB_FILES_OUT` to a folder and this test writes them there (and does nothing else with the disk).

use std::path::PathBuf;

use diacraft_engine::Session;
use diacraft_engine::memfs::{self, Keep};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, ZipItem, cell, cell_f, nested_shapes, shape, shape_with, typical_drawing, zip};
use serde_json::json;

fn page_of(shapes: &str) -> Vec<u8> {
    // A page with a size, as a drawing has one (an `.odg` is written onto a page of the size).
    VsdxBuilder::new().page(PageSpec::new(1, "Deep").sheet(&[cell("PageWidth", "11"), cell("PageHeight", "8.5")].concat()).shapes(shapes)).build()
}

/// Groups inside groups as deep as the XML limit lets a shape be: a shape at level d is at XML depth 2d + 1, and the default depth is 200, so 99 levels.
pub fn deepest_groups() -> Vec<u8> {
    page_of(&nested_shapes(98, &shape(9999, &[cell("PinX", "3"), cell("PinY", "3")].concat())))
}

/// An element nested to the XML limit inside a shape: the part's root, `Shapes` and `Shape` are three levels, and the rest is the depth of the unknown element a file may hold (kept as it is).
pub fn deepest_xml() -> Vec<u8> {
    let depth = diacraft_container::limits::MAX_XML_DEPTH - 3;
    page_of(&shape_with(1, "", &format!("{}{}", "<Q>".repeat(depth), "</Q>".repeat(depth))))
}

/// A formula that is a chain as long as the parser allows (the chain counts as depth, 512) and one nested as deep as it allows (100 brackets).
pub fn longest_formulas() -> Vec<u8> {
    let chain = vec!["1"; diacraft_formula::MAX_DEPTH - 1].join("+");
    let brackets = diacraft_formula::MAX_NESTING - 1;
    let nested = format!("{}1{}", "(".repeat(brackets), ")".repeat(brackets));
    let cells = [
        cell_f("Width", &(diacraft_formula::MAX_DEPTH - 1).to_string(), &chain),
        cell_f("Height", "1", &nested),
        cell("PinX", "3"),
        cell("PinY", "3"),
    ]
    .concat();
    page_of(&shape(1, &cells))
}

/// A thread's memory files turned on for a test, and off (and emptied) at its end, even by a failure.
struct Memory;

impl Memory {
    fn on() -> Memory {
        memfs::use_on_this_thread(true);
        Memory
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        memfs::use_on_this_thread(false);
    }
}

/// Opens `bytes` in a session of its own, the way the browser does: the file is put in memory under a name, and `file.open` reads that.
fn opens(name: &str, bytes: Vec<u8>) -> Session {
    let path = PathBuf::from(format!("/web/1/{name}"));
    memfs::put(&path, bytes, Keep::Once).unwrap();
    let mut session = Session::new();
    session.execute("file.open", &json!({ "path": path.to_str().unwrap() })).unwrap_or_else(|e| panic!("{name} was refused: {e}"));
    session
}

#[test]
fn the_deepest_legal_files_open_and_save_as_each_type() {
    let _memory = Memory::on();
    let files: [(&str, Vec<u8>); 3] = [("deep-groups", deepest_groups()), ("deep-xml", deepest_xml()), ("long-formulas", longest_formulas())];
    let out = std::env::var_os("DIACRAFT_WEB_FILES_OUT").map(PathBuf::from);
    for (name, bytes) in files {
        let mut session = opens(&format!("{name}.vsdx"), bytes.clone());
        assert!(session.document().is_some(), "{name}");
        // Each is written as the three types, in memory, and read again: the same depth through every reader and writer.
        for extension in ["diacraft", "odg", "vsdx"] {
            let path = PathBuf::from(format!("/web/2/{name}.{extension}"));
            session.execute("file.save_as", &json!({ "path": path.to_str().unwrap() })).unwrap_or_else(|e| panic!("{name}.{extension}: {e}"));
            let written = memfs::take(&path).unwrap();
            if let Some(out) = &out {
                std::fs::create_dir_all(out).unwrap();
                std::fs::write(out.join(format!("{name}.{extension}")), &written).unwrap();
            }
            opens(&format!("again-{name}.{extension}"), written);
        }
        if let Some(out) = &out {
            std::fs::write(out.join(format!("{name}-source.vsdx")), &bytes).unwrap();
        }
    }
}

/// A ladder of drawings from small to the most the reader admits (twenty pages of 4,000 shapes), for the browser's memory: the page opens what fits the budget and refuses in words what does not.
/// Written only when `DIACRAFT_WEB_FILES_OUT` is set.
#[test]
fn the_ladder_of_big_drawings_is_written_for_the_browser_when_asked() {
    let Some(out) = std::env::var_os("DIACRAFT_WEB_FILES_OUT").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (pages, per_page) in [(1, 100), (2, 1000), (5, 2000), (10, 2000), (10, 4000), (20, 4000)] {
        let bytes = typical_drawing(pages, per_page, None);
        std::fs::write(out.join(format!("big-{pages}x{per_page}.vsdx")), bytes).unwrap();
    }
}

/// Files that are no drawing, or that claim more than a drawing may: what the page must refuse in words. Each is a name and its bytes.
pub fn hostile_files() -> Vec<(&'static str, Vec<u8>)> {
    let good = deepest_groups();
    let mut cut = good.clone();
    cut.truncate(cut.len() / 2);
    let mut flipped = good.clone();
    for (n, byte) in flipped.iter_mut().enumerate() {
        if n % 7 == 3 {
            *byte ^= 0x5a;
        }
    }
    // A package whose one part says it is 400 MiB (it is a run of zeros, which costs the file almost nothing): refused by the reader's limits and not read.
    let bomb = zip(&[ZipItem::stored("[Content_Types].xml", b"<Types/>"), ZipItem::zeros_under_the_ratio_cap("visio/document.xml", 400 << 20)]);
    let mut png_head = b"\x89PNG\r\n\x1a\n".to_vec();
    png_head.extend(std::iter::repeat_n(7u8, 4096));
    vec![
        ("junk.vsdx", b"this is not a drawing file at all, only some words".to_vec()),
        ("empty.odg", Vec::new()),
        ("zeros.diacraft", vec![0; 8192]),
        ("cut.vsdx", cut),
        ("flipped.vsdx", flipped),
        ("bomb.vsdx", bomb),
        ("png-as-vsdx.vsdx", png_head),
    ]
}

/// Each hostile file is refused by the engine in words, in memory as in the browser, and none is a crash or leaves anything in the store; the drawing that was open stays. Written for the browser when
/// `DIACRAFT_WEB_FILES_OUT` is set.
#[test]
fn hostile_files_are_refused_in_words_and_the_open_drawing_stays() {
    let _memory = Memory::on();
    let out = std::env::var_os("DIACRAFT_WEB_FILES_OUT").map(PathBuf::from);
    let mut session = opens("good.vsdx", deepest_groups());
    let before = session.document().cloned();
    for (name, bytes) in hostile_files() {
        if let Some(out) = &out {
            std::fs::create_dir_all(out).unwrap();
            std::fs::write(out.join(name), &bytes).unwrap();
        }
        let path = PathBuf::from(format!("/web/3/{name}"));
        memfs::put(&path, bytes, Keep::Once).unwrap();
        let refused = session.execute("file.open", &json!({ "path": path.to_str().unwrap() }));
        // A flipped package may still read, with warnings: it is no crash, and it is not asked to fail.
        if let Err(e) = &refused {
            let words = e.to_string();
            assert!(!words.is_empty() && !words.contains("/web/"), "{name}: {words}");
            assert_eq!(session.document().cloned(), before, "{name}: the drawing that was open is as it was");
        } else {
            assert_eq!(name, "flipped.vsdx", "{name} was read, and only a flipped package may be");
            session = opens("good.vsdx", deepest_groups());
        }
        memfs::remove(&path);
    }
    assert_eq!(memfs::entries(), 0);
}
