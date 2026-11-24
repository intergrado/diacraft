//! Timed: what the picture of the first page costs a save of a hostile drawing, against a plain yardstick (a page of twenty thousand boxes, which the
//! preview's work draws whole), and that a save of it is the same file on two runs. The hostile drawings are those a clock once bounded: the same long text
//! in hundreds of shapes, one big picture that decodes to 64 million pixels shown by a page of shapes, twenty distinct big pictures, and a metafile of fifty
//! thousand items shown by a page of shapes. Each preview is cut by its work (or a metafile's cap of items), and never by a time.
//!
//! Timed, so it is `#[ignore]`d: run by hand, alone, on a quiet machine (`cargo test -p diacraft-engine --lib odgfile::preview_cost -- --ignored --nocapture
//! --test-threads=1`). It prints the machine's load beside its figures. **It holds each save to [`MOST_TIMES`] the yardstick's save, and each preview to
//! [`MOST_TIMES`] the yardstick's preview, in the CPU time of the process** (all its threads).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout)]

use std::sync::Arc;
use std::time::Instant;

use diacraft_odg::Variant;
use diacraft_render::limits::PREVIEW_MAX_WORK;
use diacraft_render::{Clock, Limits, SkipKind};
use serde_json::json;

use super::save::{render_thumbnail, save_model};
use crate::Session;
use crate::across::Options;

/// The shapes of the yardstick, and of the hostile pages of many shapes.
const SHAPES: usize = 20_000;

/// The most a hostile drawing's save (and its preview) may cost, in times the yardstick's.
const MOST_TIMES: f64 = 6.0;

/// The CPU time the process has used, all its threads, user and system, in seconds (`/proc/self/stat`, in hundredths).
fn cpu() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    let tick = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (tick(11) + tick(12)) as f64 / 100.0
}

/// The machine's load, the average of the last minute.
fn load() -> f64 {
    std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse().ok())).unwrap_or(0.0)
}

/// `f`, with its wall time and the process's CPU time, in seconds.
fn timed<T>(f: impl FnOnce() -> T) -> (T, f64, f64) {
    let (started, before) = (Instant::now(), cpu());
    let out = f();
    (out, started.elapsed().as_secs_f64(), cpu() - before)
}

/// A PNG of `side` by `side` pixels of one grey: a few hundred kilobytes that decode to `side` squared pixels.
fn grey_png(side: u32, grey: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&vec![grey; side as usize * side as usize]).unwrap();
    drop(writer);
    out
}

/// An EMF record of type `t` with these fields, padded to four bytes.
fn record(t: u32, fields: &[u8]) -> Vec<u8> {
    let mut body = fields.to_vec();
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    let mut out = [t, 8 + body.len() as u32].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    out.extend(body);
    out
}

fn le32(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// An EMF of `count` small filled rectangles (a red brush, no pen): bounds of 80 by 40, a frame of 20 by 10 millimetres.
fn emf_of_rects(count: i32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(record(39, &le32(&[1, 0, 0x0000_00C8, 0])));
    body.extend(record(37, &le32(&[1])));
    body.extend(record(37, &le32(&[0x8000_0008_u32 as i32])));
    for k in 0..count {
        body.extend(record(43, &le32(&[k % 200, k / 200, k % 200 + 3, k / 200 + 3])));
    }
    body.extend(record(14, &le32(&[0, 16, 20])));
    let total = 108 + body.len() as i32;
    let mut header = le32(&[0, 0, 79, 39, 0, 0, 2000, 1000, 0x464D_4520, 0x10000, total, 3]);
    header.extend([8u8, 0, 0, 0]);
    header.extend(le32(&[0, 0, 0, 320, 160, 80, 40, 0, 0, 0, 80_000, 40_000]));
    let mut out = record(1, &header);
    out.extend(body);
    out
}

/// A new drawing whose first page holds `count` copies of the shape `make` puts on it, as one edit: the copies share what the shape holds (its text, its
/// picture), as the shapes of one master do.
fn copies(count: usize, make: impl FnOnce(&mut Session, u32) -> u32) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let page = s.document().unwrap().pages.iter().next().unwrap().id;
    let id = make(&mut s, page);
    s.edit("Test edit", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let template = page.shapes.iter().find(|x| x.id == id).unwrap().clone();
        for k in 1..count {
            page.shapes.push(Arc::new(diacraft_doc::Shape { id: id + u32::try_from(k).unwrap(), ..(*template).clone() }));
        }
        Ok(())
    })
    .unwrap();
    s
}

fn id_of(made: &serde_json::Value) -> u32 {
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

/// A picture of `bytes` from a file, `width` inches wide.
fn picture(s: &mut Session, page: u32, name: &str, bytes: &[u8], width: f64) -> u32 {
    let dir = std::env::temp_dir().join(format!("diacraft-thumbdet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    id_of(&s.execute("insert.picture", &json!({ "path": path, "page": page, "x": 4.0, "y": 5.0, "width": width })).unwrap())
}

#[test]
#[ignore]
fn a_save_of_a_hostile_drawing_costs_a_small_constant_of_a_plain_one_and_is_the_same_file_on_two_runs() {
    let long = "The quick brown fox jumps over the lazy dog. ".repeat(360);
    let big = grey_png(8_000, 90);
    let emf = emf_of_rects(50_000);
    let drawings: Vec<(&str, Session)> = vec![
        (
            "yardstick: twenty thousand boxes",
            copies(SHAPES, |s, page| id_of(&s.execute("shape.drop", &json!({ "master": "rectangle", "page": page, "x": 4.0, "y": 5.0 })).unwrap())),
        ),
        (
            "the same long text in 300 boxes",
            copies(300, |s, page| {
                let id = id_of(&s.execute("shape.drop", &json!({ "master": "rectangle", "page": page, "x": 4.0, "y": 5.0 })).unwrap());
                s.execute("shape.set_text", &json!({ "id": id, "text": long })).unwrap();
                id
            }),
        ),
        ("one picture of 64 million pixels in every shape", copies(SHAPES, |s, page| picture(s, page, "big.png", &big, 3.0))),
        (
            "twenty distinct pictures of 36 million pixels",
            copies(1, |s, page| {
                let mut last = 0;
                for k in 0..20u8 {
                    last = picture(s, page, &format!("p{k}.png"), &grey_png(6_000, 40 + k), 3.0);
                }
                last
            }),
        ),
        ("a metafile of 50,000 items in every shape", copies(SHAPES, |s, page| picture(s, page, "m.emf", &emf, 3.0))),
    ];
    let mut yard: Option<(f64, f64)> = None;
    for (what, s) in &drawings {
        let doc = Arc::new(s.document().unwrap().clone());
        let ((rendered, work), preview_wall, preview_cpu) = timed(|| render_thumbnail(&doc, Limits::preview(), Clock::system()).unwrap());
        let (_, window_wall, window_cpu) = timed(|| render_thumbnail(&doc, Limits::default(), Clock::system()).unwrap());
        let (first, save_wall, save_cpu) = timed(|| save_model("file.save_as", s, &doc, Variant::Drawing, Options::default()).unwrap());
        let second = save_model("file.save_as", s, &doc, Variant::Drawing, Options::default()).unwrap();
        let cut: Vec<String> = rendered.skipped.iter().map(|k| format!("{:?} {}", k.kind, k.count)).collect();
        println!(
            "preview {what} (load {:.0}): preview {preview_wall:.3} s wall, {preview_cpu:.2} s CPU, {work} units, cut [{}]; \
             the window's limits {window_wall:.3} s wall, {window_cpu:.2} s CPU; save {save_wall:.3} s wall, {save_cpu:.2} s CPU, {} bytes",
            load(),
            cut.join(", "),
            first.bytes.len()
        );
        assert!(first.bytes == second.bytes, "{what}: two saves of the same drawing are the same file");
        assert!(work <= PREVIEW_MAX_WORK, "{what}: {work}");
        assert!(rendered.count(SkipKind::FrameBudget) + rendered.count(SkipKind::TextBudget) == 0, "{what}: {:?}", rendered.skipped);
        match yard {
            None => yard = Some((preview_cpu.max(0.01), save_cpu.max(0.01))),
            Some((yard_preview, yard_save)) => {
                assert!(
                    rendered.count(SkipKind::WorkLimit) + rendered.count(SkipKind::MetafileCapReached) >= 1,
                    "{what}: cut by a count: {:?}",
                    rendered.skipped
                );
                assert!(
                    preview_cpu <= MOST_TIMES * yard_preview.max(0.1),
                    "{what}: the preview took {preview_cpu:.2} s of CPU, the yardstick's {yard_preview:.2} s"
                );
                assert!(
                    save_cpu <= MOST_TIMES * yard_save.max(0.1),
                    "{what}: the save took {save_cpu:.2} s of CPU, the yardstick's {yard_save:.2} s"
                );
            }
        }
    }
}
