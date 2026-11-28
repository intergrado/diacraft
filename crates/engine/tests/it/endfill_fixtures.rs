//! A measurement, not a check: which of the drawings DiaCraft makes itself change their look when the inside of a hollow line end takes the shape's fill.
//!
//! Two ignored tests, run by hand on two commits:
//!
//! * `DIACRAFT_ENDFILL_FIXTURES=/absolute/dir cargo test -p diacraft-engine --test it endfill_fixtures::write -- --ignored` renders the fixtures to PNG files under the folder:
//!   `masters/` (every master of every built-in stencil, dropped on a blank page), `templates/` (a connector between two boxes in each template, with each hollow end), `connectors/`
//!   (the same on a blank drawing, over a coloured box) and `site/` (the two scripts that make the website's pictures).
//! * `DIACRAFT_ENDFILL_DIFF=/absolute/before:/absolute/after cargo test -p diacraft-engine --test it endfill_fixtures::diff -- --ignored --nocapture` compares two such folders
//!   file by file and prints the files whose pixels differ, with the count of pixels.

use std::path::{Path, PathBuf};

use diacraft_engine::Session;
use diacraft_engine::stencil;
use diacraft_render::{Renderer, View};
use serde_json::{Value, json};

use crate::{blank_session, run};

/// Pixels to the inch of every picture.
const PPI: f64 = 100.0;

/// The hollow marks (`ends.rs`): each has a closed part drawn as an outline.
const HOLLOW: [i64; 17] = [14, 15, 16, 17, 18, 19, 20, 21, 22, 29, 30, 31, 32, 33, 34, 40, 41];

fn write_png(path: &Path, session: &Session) {
    let doc = session.document().expect("a document");
    let view = View::fit_page(doc, 1, PPI).expect("the page has a size");
    let r = Renderer::new().render(doc, 1, &view);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("make the folder");
    let file = std::fs::File::create(path).expect("create the picture");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), r.width, r.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the header");
    writer.write_image_data(&r.rgba_premul).expect("the pixels");
}

/// A connector between two boxes (the first one blue, so that an inside shows over a colour), with `end` as its end; it gives the connector's id.
fn connected(s: &mut Session, end: Option<i64>) -> u32 {
    // The ids are the commands' own: a template's page may hold shapes already, so 1, 2 and 3 are not always these.
    let drop = |s: &mut Session, x: f64| {
        let made = run(s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": x, "y": 4.0}));
        u32::try_from(made["id"].as_u64().expect("an id")).expect("a small id")
    };
    let (first, second) = (drop(s, 2.0), drop(s, 6.0));
    run(s, "shape.format", json!({"ids": [first], "fill": "#3366CC"}));
    let made = run(s, "connector.add", json!({"from": {"id": first}, "to": {"id": second}}));
    let connector = u32::try_from(made["id"].as_u64().expect("an id")).expect("a small id");
    if let Some(end) = end {
        run(s, "shape.format", json!({"ids": [connector], "end_arrow": end, "begin_arrow": end, "end_arrow_size": 4, "begin_arrow_size": 4}));
    }
    connector
}

#[test]
#[ignore = "a measurement: set DIACRAFT_ENDFILL_FIXTURES to a folder"]
fn write() {
    let root = PathBuf::from(std::env::var("DIACRAFT_ENDFILL_FIXTURES").expect("DIACRAFT_ENDFILL_FIXTURES"));
    // Every master of every built-in stencil, dropped on a blank page. A master that is one-dimensional with a hollow end is listed with the fill it resolves to.
    let (mut dropped, mut left_out) = (0, 0);
    for st in stencil::stencils() {
        for master in st.masters() {
            let mut s = blank_session();
            let Ok(made) = s.execute("shape.drop", &json!({"master": master.id, "stencil": st.id, "page": 1, "x": 5.5, "y": 4.25})) else {
                left_out += 1;
                continue;
            };
            dropped += 1;
            if let (Some(id), Some(doc)) = (made["id"].as_u64().and_then(|n| u32::try_from(n).ok()), s.document()) {
                let page = doc.pages.get(0).map_or(0, |p| p.id);
                let cell = |name: &str| {
                    let at = diacraft_engine::doc::CellAddr::direct(name);
                    let shape = doc.find_shape(diacraft_engine::doc::Owner::Page(page), id)?;
                    diacraft_engine::doc::Resolver::new(doc)
                        .resolve(&diacraft_engine::doc::SheetRef::Shape(shape), &at)
                        .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref().and_then(|v| v.number())))
                };
                let ends = [cell("BeginArrow"), cell("EndArrow")];
                if ends.iter().flatten().any(|n| HOLLOW.contains(&(*n as i64))) {
                    let look = diacraft_engine::formatting::look_of(doc, page, &[id]).ok().and_then(|mut l| l.pop());
                    println!("HOLLOWMASTER {}/{} ends {ends:?} fill {:?} pattern {:?}", st.id, master.id, look.map(|l| l.fill), cell("FillPattern"));
                }
            }
            write_png(&root.join("masters").join(format!("{}--{}.png", st.id, master.id.replace('/', "_"))), &s);
        }
    }
    println!("MASTERS dropped {dropped}, left out {left_out}");
    // A connector in each template, as it is made and with each hollow end.
    let listed = blank_session().execute("template.list", &json!({})).expect("the templates");
    let ids: Vec<String> = listed["templates"].as_array().expect("a list").iter().filter_map(|t| t["id"].as_str().map(str::to_owned)).collect();
    for id in &ids {
        for end in std::iter::once(None).chain(HOLLOW.iter().map(|n| Some(*n))) {
            let mut s = blank_session();
            run(&mut s, "file.new", json!({"template": id}));
            let connector = connected(&mut s, end);
            if end.is_none() {
                // Which style gives the connector its fill: the fill link's name, and what the connector resolves to.
                if let Some(doc) = s.document() {
                    let page = doc.pages.get(0).map_or(0, |p| p.id);
                    let fill = diacraft_engine::formatting::look_of(doc, page, &[connector]).ok().and_then(|mut l| l.pop()).map(|l| l.fill);
                    let link = doc
                        .find_shape(diacraft_engine::doc::Owner::Page(page), connector)
                        .and_then(|at| doc.shape(&at))
                        .and_then(|sh| sh.sheet.links.fill);
                    let name = link.and_then(|id| doc.styles.iter().find(|st| st.id == id)).and_then(|st| st.name.clone());
                    println!("TEMPLATE {id}: the connector's fill {fill:?}, its fill link {link:?} {name:?}");
                }
            }
            write_png(&root.join("templates").join(format!("{id}--{}.png", end.map_or("default".to_owned(), |n| n.to_string()))), &s);
        }
    }
    // The same on the blank drawing.
    for end in std::iter::once(None).chain(HOLLOW.iter().map(|n| Some(*n))) {
        let mut s = blank_session();
        let _ = connected(&mut s, end);
        write_png(&root.join("connectors").join(format!("blank--{}.png", end.map_or("default".to_owned(), |n| n.to_string()))), &s);
    }
    // The scripts of the website's pictures, command by command.
    for name in ["integration-architecture", "org-chart"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/website-pictures").join(format!("{name}.jsonl"));
        let mut s = Session::new();
        for line in std::fs::read_to_string(&path).expect("the script").lines().filter(|l| !l.trim().is_empty()) {
            let step: Value = serde_json::from_str(line).expect("a line of JSON");
            let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
            let command = step["command"].as_str().expect("a command");
            // The scripts end by saving: a measurement writes no file, and a step that does not apply here is left out of the picture.
            if !command.starts_with("file.save") && !command.starts_with("file.export") {
                let _ = s.execute(command, &params);
            }
        }
        write_png(&root.join("site").join(format!("{name}.png")), &s);
    }
}

fn pixels(path: &Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("open")));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut buf = vec![0; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut buf).expect("a frame");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "png") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
#[ignore = "a measurement: set DIACRAFT_ENDFILL_DIFF to before:after, two folders"]
fn diff() {
    let both = std::env::var("DIACRAFT_ENDFILL_DIFF").expect("DIACRAFT_ENDFILL_DIFF");
    let (before, after) = both.split_once(':').expect("before:after");
    let (before, after) = (Path::new(before), Path::new(after));
    let (mut total, mut changed) = (0usize, 0usize);
    let mut by_set: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    for file in files_under(before) {
        let rel = file.strip_prefix(before).expect("under before");
        let set = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        let other = after.join(rel);
        total += 1;
        let entry = by_set.entry(set).or_default();
        entry.0 += 1;
        if !other.exists() {
            println!("MISSING {}", rel.display());
            continue;
        }
        let (a, b) = (pixels(&file), pixels(&other));
        if a.0 != b.0 || a.1 != b.1 {
            println!("SIZE {}", rel.display());
            continue;
        }
        let differing = a.2.chunks(4).zip(b.2.chunks(4)).filter(|(p, q)| p != q).count();
        if differing > 0 {
            changed += 1;
            entry.1 += 1;
            println!("CHANGED {differing:>7} px  {}", rel.display());
        }
    }
    for (set, (n, c)) in &by_set {
        println!("SET {set}: {c} of {n} pictures differ");
    }
    println!("TOTAL: {changed} of {total} pictures differ");
}

#[test]
#[ignore = "a measurement: set DIACRAFT_ENDFILL_FIXTURES to a folder; writes .odg files for another program to draw"]
fn write_odgs() {
    // A connector between two boxes with each of four hollow ends and four fills, saved as `.odg`, for a look at what another program draws inside the marks (the
    // marker of an `.odg` is a path in the line's colour: ODF has no fill for its inside).
    let root = PathBuf::from(std::env::var("DIACRAFT_ENDFILL_FIXTURES").expect("DIACRAFT_ENDFILL_FIXTURES")).join("odg");
    std::fs::create_dir_all(&root).expect("make the folder");
    for (number, name) in [(21, "square"), (16, "triangle"), (20, "disc"), (22, "diamond")] {
        for (fill, label) in [("none", "none"), ("#E03030", "red"), ("#E03030/50", "half"), ("#E03030/grad", "gradient")] {
            let mut s = blank_session();
            run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 2.0, "y": 4.0}));
            run(&mut s, "shape.drop", json!({"master": "rectangle", "stencil": "basic", "page": 1, "x": 7.0, "y": 4.0}));
            run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
            let colour = fill.split('/').next().expect("a colour");
            run(
                &mut s,
                "shape.format",
                json!({"ids": [3], "fill": colour, "end_arrow": number, "end_arrow_size": 6, "line": "#202020", "weight": 2}),
            );
            if fill.ends_with("/50") {
                run(&mut s, "shape.format", json!({"ids": [3], "fill_transparency": 50}));
            }
            if fill.ends_with("/grad") {
                run(
                    &mut s,
                    "shape.gradient",
                    json!({"ids": [3], "kind": "linear", "angle": 0, "stops": [{"position": 0, "color": "#E03030"}, {"position": 1, "color": "#3050E0"}]}),
                );
            }
            run(&mut s, "file.save_as", json!({"path": root.join(format!("{name}-{label}.odg")).to_str().expect("a path")}));
        }
    }
}
