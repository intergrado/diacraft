//! What a layout leaves in a saved file. The page's layout cells (`PlaceStyle`, `AvenueSizeX`, `AvenueSizeY`, `ResizePage`, ...) and the shapes' places are the
//! model and ride in every format; the Page Breaks box is DiaCraft's own data (`page_breaks`), as the AutoConnect box is, and the drawing window's
//! `ShowPageBreaks` in a VSDX file, read and written back as the grid and the rulers are. Each is checked in `.diacraft`, VSDX (the model writer and a save into the file) and
//! `.odg` (the model writer and a save into the file), by reading it back.

use std::collections::BTreeMap;

use diacraft_engine::Session;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, PINNED};
use crate::odg_build::{Drawing, rect};
use crate::relayout::page_cell;
use crate::{blank_session, run};

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    s.pin_clock(Some(PINNED));
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// A drawing of three shapes joined in a chain, laid out as a hierarchy down the page with a space of 0.5 and the Page Breaks box on.
fn laid_out() -> Session {
    let mut s = blank_session();
    s.pin_clock(Some(PINNED));
    let shapes: Vec<u32> = (0..3)
        .map(|i| crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0 + 3.0 * f64::from(i), "y": 4.0}))))
        .collect();
    for w in shapes.windows(2) {
        run(&mut s, "connector.add", json!({"from": {"id": w[0], "point": 3}, "to": {"id": w[1], "point": 1}}));
    }
    run(
        &mut s,
        "page.relayout",
        json!({"style": "hierarchy", "direction": "top_to_bottom", "alignment": "start", "spacing": 0.5, "enlarge_page": true}),
    );
    run(&mut s, "page_breaks.set", json!({"on": true}));
    s
}

/// What the drawing says of its layout and of the page breaks, and where its shapes are: what a save must keep.
fn facts(s: &mut Session) -> Value {
    let layout = run(s, "page.layout", json!({}));
    let breaks = run(s, "page_breaks.settings", json!({}));
    let d = s.document().unwrap();
    let cells: Vec<Option<f64>> = ["PlaceStyle", "AvenueSizeX", "AvenueSizeY", "ResizePage"].iter().map(|n| page_cell(d, n)).collect();
    json!({ "layout": layout, "page_breaks": breaks["on"], "cells": cells })
}

#[test]
fn the_choice_and_the_box_are_kept_by_a_diacraft_save() {
    let dir = Dir::new();
    let mut s = laid_out();
    let want = facts(&mut s);
    assert_eq!(want["layout"]["place_style"], 16, "{want}");
    assert_eq!(want["page_breaks"], true);
    let path = dir.path("laid.diacraft");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = opened(&path);
    assert_eq!(facts(&mut back), want, ".diacraft");
}

#[test]
fn and_by_the_vsdx_writer_and_a_save_into_a_vsdx_file() {
    let dir = Dir::new();
    let mut s = laid_out();
    let want = facts(&mut s);
    let path = dir.path("laid.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut back = opened(&path);
    assert_eq!(facts(&mut back), want, "VSDX from the model");
    // Turn the layout over in the file it came from, and save into it.
    run(&mut back, "page.relayout", json!({"style": "radial", "spacing": 0.75}));
    run(&mut back, "page_breaks.set", json!({"on": false}));
    let into = facts(&mut back);
    run(&mut back, "file.save", json!({}));
    assert_eq!(facts(&mut opened(&path)), into, "VSDX saved into itself");
}

#[test]
fn and_by_an_odg_written_from_the_model_and_a_save_into_an_odg_file() {
    let dir = Dir::new();
    let mut s = laid_out();
    let want = facts(&mut s);
    let path = dir.path("laid.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(facts(&mut opened(&path)), want, ".odg from the model");
    // An `.odg` of another program's: its page takes the layout, and the save splices into the file.
    let other = dir.write("other.odg", &Drawing::new().page(&rect("A", (1.0, 1.0, 2.0, 1.0), "")).bytes());
    let mut s = opened(&other);
    run(&mut s, "page.relayout", json!({"style": "circular", "spacing": 0.5}));
    run(&mut s, "page_breaks.set", json!({"on": true}));
    let into = facts(&mut s);
    run(&mut s, "file.save", json!({}));
    assert_eq!(facts(&mut opened(&other)), into, ".odg saved into itself");
}

#[test]
fn a_flat_odg_keeps_the_page_breaks_box_and_refuses_in_words_a_layout_it_cannot_write() {
    // DiaCraft's own data rides in a flat drawing too, so the box is kept. The layout's own cells are a change of the page's fields, which a save into a flat file refuses in words
    // (`edit.undo`, or Save As to `.diacraft`, gets round it): never silently.
    let dir = Dir::new();
    let path = dir.write("flat.fodg", &Drawing::new().page(&rect("R", (1.0, 1.0, 2.0, 1.0), "")).flat_bytes());
    let mut s = opened(&path);
    run(&mut s, "page_breaks.set", json!({"on": true}));
    run(&mut s, "file.save", json!({}));
    let mut back = opened(&path);
    assert_eq!(run(&mut back, "page_breaks.settings", json!({}))["on"], true, "the box is kept");
    run(&mut back, "page.relayout", json!({"style": "circular", "spacing": 0.5}));
    let refused = back.execute("file.save", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("saving does not yet write this change") && refused.contains("Save As"), "{refused}");
    let native = dir.path("flat-laid.diacraft");
    run(&mut back, "file.save_as", json!({ "path": native }));
    let mut again = opened(&native);
    assert_eq!(page_cell(again.document().unwrap(), "PlaceStyle"), Some(6.0));
    assert_eq!(run(&mut again, "page_breaks.settings", json!({}))["on"], true);
}

// ---- the VSDX drawing window's ShowPageBreaks ----

const WINDOWS_REL: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
const WINDOWS_TYPE: &str = "application/vnd.ms-visio.windows+xml";

/// A windows part as a file might hold one: a stencil window first, attributes the model has no field for, and a drawing window holding `flags` (children) before
/// `SnapSettings`.
fn windows_part(flags: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Windows ClientWidth=\"1680\" ClientHeight=\"947\" xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\">\r\n  <Window ID=\"1\" WindowType=\"Stencil\" ParentWindow=\"0\" Document=\"C:\\x\\BASIC_M.vssx\"/>\r\n  <Window ID=\"0\" WindowType=\"Drawing\" ContainerType=\"Page\" Page=\"1\" ViewScale=\"-1\">\r\n    {flags}\r\n    <SnapSettings>295</SnapSettings>\r\n  </Window>\r\n</Windows>"
    )
}

/// A VSDX drawing of one page, with `windows` as its windows part.
fn vsdx_with(windows: &str) -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "First").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#))
        .with(Pkg::new().part("/visio/windows.xml", WINDOWS_TYPE, windows).rel("/visio/document.xml", "rId90", WINDOWS_REL, "windows.xml"))
        .build()
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

/// The parts a save into `before` changed, added or took out.
fn changed(before: &[u8], after: &[u8]) -> Vec<String> {
    let (before, after) = (parts(before), parts(after));
    let names: std::collections::BTreeSet<String> =
        before.keys().chain(after.keys()).filter(|name| before.get(*name) != after.get(*name)).cloned().collect();
    names.into_iter().collect()
}

/// A part's text with its line ends as a part the writer made again has them (the reader takes CR LF as LF, `diacraft_container::xml`).
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn breaks(s: &mut Session) -> Value {
    run(s, "page_breaks.settings", json!({}))
}

/// A VSDX file whose drawing window says the page breaks show opens with the box ticked; an unedited save keeps the windows part byte for byte; the box turned off is
/// written where the file holds it, and nothing else of the part, and no DiaCraft data, is written. (neither read nor wrote the file's own child.)
#[test]
fn a_vsdx_files_show_page_breaks_is_read_and_written_back_where_it_is() {
    let dir = Dir::new();
    let part = windows_part(
        "<ShowRulers>1</ShowRulers>\r\n    <ShowGrid>1</ShowGrid>\r\n    <ShowPageBreaks>1</ShowPageBreaks>\r\n    <ShowGuides>1</ShowGuides>",
    );
    let original = vsdx_with(&part);
    let path = dir.write("breaks.vsdx", &original);
    let mut s = opened(&path);
    assert_eq!(breaks(&mut s), json!({"on": true, "held": true}), "the file's own value");
    run(&mut s, "file.save", json!({}));
    let unedited = std::fs::read(&path).unwrap();
    assert_eq!(parts(&unedited)["/visio/windows.xml"], part.as_bytes(), "an unedited save keeps the part byte for byte");
    assert!(changed(&original, &unedited).iter().all(|name| name == "/docProps/app.xml"), "{:?}", changed(&original, &unedited));
    run(&mut s, "page_breaks.set", json!({"on": false}));
    run(&mut s, "file.save", json!({}));
    let saved = std::fs::read(&path).unwrap();
    let want = part.replace("<ShowPageBreaks>1</ShowPageBreaks>", "<ShowPageBreaks>0</ShowPageBreaks>");
    assert_eq!(
        lf(&String::from_utf8(parts(&saved)["/visio/windows.xml"].clone()).unwrap()),
        lf(&want),
        "set where it is, and nothing else of the part changes"
    );
    assert_eq!(
        changed(&original, &saved),
        ["/docProps/app.xml", "/docProps/core.xml", "/visio/windows.xml"],
        "no DiaCraft data: the window holds the box"
    );
    assert_eq!(breaks(&mut opened(&path)), json!({"on": false, "held": false}));
}

/// A VSDX file whose drawing window holds no `ShowPageBreaks` opens with the box clear, and the box turned on puts the child in at its place in the order (after `ShowGrid`, before
/// `ShowGuides`); a reopen shows it ticked, and Save As `.diacraft` and `.odg` keep it (the windows part rides in both).
#[test]
fn a_vsdx_file_whose_window_lacks_it_gains_show_page_breaks_at_its_place() {
    let dir = Dir::new();
    let part = windows_part("<ShowRulers>1</ShowRulers>\r\n    <ShowGrid>0</ShowGrid>\r\n    <ShowGuides>1</ShowGuides>");
    let original = vsdx_with(&part);
    let path = dir.write("lacking.vsdx", &original);
    let mut s = opened(&path);
    assert_eq!(breaks(&mut s), json!({"on": false, "held": null}));
    run(&mut s, "page_breaks.set", json!({"on": true}));
    run(&mut s, "file.save", json!({}));
    let saved = std::fs::read(&path).unwrap();
    let windows = String::from_utf8(parts(&saved)["/visio/windows.xml"].clone()).unwrap();
    let at = |needle: &str| windows.find(needle).unwrap_or_else(|| panic!("{needle} in {windows}"));
    assert!(at("<ShowGrid>0</ShowGrid>") < at("<ShowPageBreaks>1</ShowPageBreaks>"), "after the grid: {windows}");
    assert!(at("<ShowPageBreaks>1</ShowPageBreaks>") < at("<ShowGuides>1</ShowGuides>"), "before the guides: {windows}");
    assert!(
        windows.contains("WindowType=\"Stencil\"") && windows.contains("BASIC_M.vssx") && windows.contains("<SnapSettings>295</SnapSettings>"),
        "{windows}"
    );
    assert_eq!(changed(&original, &saved), ["/docProps/app.xml", "/docProps/core.xml", "/visio/windows.xml"]);
    let mut back = opened(&path);
    assert_eq!(breaks(&mut back), json!({"on": true, "held": true}));
    assert_eq!(run(&mut back, "window.settings", json!({}))["grid"], false, "the grid as the file had it");
    for name in ["across.diacraft", "across.odg"] {
        let to = dir.path(name);
        run(&mut back, "file.save_as", json!({ "path": to }));
        assert_eq!(breaks(&mut opened(&to))["on"], true, "{name}");
    }
}
