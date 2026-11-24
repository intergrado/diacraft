//! A new shape flipped on one axis and turned (30 degrees in the case found), saved to an `.odg` through the commands. DiaCraft read its own file as the session had
//! it, but **LibreOffice drew the shape at another angle** (an overlap of 0.945 with the session's picture, where a shape with no flip, or both, scores 0.990): a single mirror
//! (`draw:mirror-horizontal` or `draw:mirror-vertical`) beside `draw:transform="rotate (a)"` is drawn by LibreOffice 26.8.2.2 on the picture of the turned frame, which is the
//! model's turn the other way round, so the file holds minus the model's turn beside one mirror and the reader reads it back (the odg crate's
//! `write_flipturn.rs` holds the measurement). The same reading is why a shape LibreOffice itself wrote with one mirror and a turn was drawn wrongly by DiaCraft.
//!
//! The shapes are made by commands (`shape.drop`, `shape.resize`, `shape.format`, `shape.flip`, `shape.rotate`) and saved by `file.save`; the black-box check of the picture
//! against LibreOffice is an ignored test, run by hand.

use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_render::{Renderer, View};
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, open_drawing, same_frame};
use crate::odg_file::{assert_saves_again_as_no_edit, frame_at, number, save};
use crate::run;

/// The first page drawn at 50 pixels to the inch, on white paper.
fn drawn(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

#[track_caller]
fn assert_draws_alike(made: &Document, read: &Document, what: &str) {
    let (a, b) = (drawn(made), drawn(read));
    assert_eq!((a.0, a.1), (b.0, b.1), "{what}: the picture's size");
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= 8, "{what}: the file draws it differently from the session (the worst channel differs by {worst})");
}

/// A right triangle dropped with its centre at (`x`, `y`) inches, `w` by `h`, filled, flipped as `flips` says (`h` horizontal, `v` vertical, in that order) and then turned to
/// `angle` degrees: its `ID`.
fn triangle(s: &mut Session, (x, y): (f64, f64), (w, h): (f64, f64), flips: &str, angle: f64) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": "right_triangle", "page": 0, "x": x, "y": y }));
    let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    run(s, "shape.resize", json!({ "id": id, "x": x - w / 2.0, "y": y - h / 2.0, "width": w, "height": h }));
    run(s, "shape.format", json!({ "ids": [id], "fill": "#3465a4" }));
    for flip in flips.chars() {
        let axis = if flip == 'h' { "horizontal" } else { "vertical" };
        run(s, "shape.flip", json!({ "ids": [id], "axis": axis }));
    }
    if angle != 0.0 {
        run(s, "shape.rotate", json!({ "ids": [id], "angle": angle }));
    }
    id
}

/// The flips of the matrix, in the words of `triangle`: none, each axis alone, both.
const FLIPS: [&str; 4] = ["", "h", "v", "hv"];
/// The turns of the matrix, degrees: nothing, the case found, a quarter, past an eighth over a quarter, and one past a half turn.
const ANGLES: [f64; 5] = [0.0, 30.0, 90.0, 135.0, 210.0];

fn cell(doc: &Document, at: usize, name: &str) -> f64 {
    let shape = doc.pages.get(0).unwrap().shapes.get(at).unwrap();
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// The shape of the drawing the test makes, at the end of the page's shapes: the file's one element, as `content.xml` writes it.
fn last_custom_shape(content: &str) -> String {
    let start = content.rfind("<draw:custom-shape ").expect("a custom shape");
    let end = content[start..].find("</draw:custom-shape>").expect("its end");
    content[start..start + end].to_owned()
}

/// The turn a custom shape's `draw:transform` holds, radians, or 0 when it has none.
fn file_turn(element: &str) -> f64 {
    let Some(at) = element.find("draw:transform=\"rotate (") else { return 0.0 };
    let rest = &element[at + "draw:transform=\"rotate (".len()..];
    rest[..rest.find(')').expect("a closing parenthesis")].trim().parse().expect("a number")
}

/// What the session has of the new shape is what the file gives back, in the matrix of the brief: each of the flips, alone and both, at 0, 30, 90, 135 and 210 degrees.
/// The pin, the size, the flips and the turn come back (a turn is the same one a whole turn away), the picture is the session's, and a second save is no edit.
#[test]
fn a_new_flipped_and_turned_shape_saves_and_reopens_as_the_session_has_it() {
    for flips in FLIPS {
        for angle in ANGLES {
            let what = format!("flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            triangle(&mut s, (4.0, 6.0), (3.0, 1.5), flips, angle);
            let saved = save(&mut s);
            assert_eq!(saved["edited"], true, "{what}: {saved}");
            assert_eq!(saved["warning_count"], 0, "{what}: the save says nothing: {saved}");
            let mut again = Session::new();
            run(&mut again, "file.open", json!({ "path": dir.path("a.odg") }));
            let (made, read) = (s.document().unwrap(), again.document().unwrap());
            same_frame(frame_at(made, 0, 0), frame_at(read, 0, 0), &what);
            for name in ["FlipX", "FlipY"] {
                assert_eq!(cell(made, 0, name), cell(read, 0, name), "{what}: {name}");
            }
            // The reader takes the width from the length of the turned frame's side (`hypot`), which glibc rounds correctly and macOS's library
            // may not: there it came back a last bit short (2.9999999999999996 for 3), so a few units of the last place are the same width.
            let (width, width_read) = (number(made, 0, 0, "Width"), number(read, 0, 0, "Width"));
            assert!((width - width_read).abs() <= 4.0 * f64::EPSILON * width.abs(), "{what}: width {width} read back as {width_read}");
            assert_draws_alike(made, read, &what);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// **The turn a file holds beside one mirror is minus the session's** (the case found: a triangle flipped on one axis and turned 30 degrees, drawn by LibreOffice at another angle
/// than the session's). With both mirrors or none it is the session's own. Each of the matrix, by the file's `rotate`.
#[test]
fn the_turn_written_beside_a_single_mirror_is_minus_the_sessions_and_the_same_beside_none_or_both() {
    for flips in FLIPS {
        for angle in ANGLES {
            let what = format!("flipped {flips:?} and turned {angle} degrees");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &Drawing::new().page(""));
            triangle(&mut s, (4.0, 6.0), (3.0, 1.5), flips, angle);
            save(&mut s);
            let element = last_custom_shape(&content_of(&dir.read("a.odg")));
            let mirrors = (element.contains("draw:mirror-horizontal=\"true\""), element.contains("draw:mirror-vertical=\"true\""));
            assert_eq!(mirrors, (flips.contains('h'), flips.contains('v')), "{what}: {element}");
            // The session's turn, brought into (-pi, pi] as the writer writes it.
            let model = cell(s.document().unwrap(), 0, "Angle");
            let wrapped = model.sin().atan2(model.cos());
            let want = if mirrors.0 != mirrors.1 { -wrapped } else { wrapped };
            let got = file_turn(&element);
            assert!(
                (got - want).abs() < 1e-9 || (got - want).abs() > std::f64::consts::TAU - 1e-9,
                "{what}: the file's turn is {got}, expected {want}: {element}"
            );
        }
    }
}

// The black-box check of the picture against LibreOffice, by hand: `ODG_FLIPTURN_DIR=<an empty folder> cargo test -p diacraft-engine --test it -- --ignored flipturn_lo`.

/// The ink of a picture: a pixel that is not white, as a mask of its size.
fn ink_of(png_bytes: &[u8]) -> (usize, usize, Vec<bool>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().expect("a PNG");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut buf).expect("its pixels");
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("a colour type of the check's pictures: {other:?}"),
    };
    let (w, h) = (info.width as usize, info.height as usize);
    let ink = (0..w * h).map(|i| buf[i * channels..i * channels + 3].iter().any(|v| *v < 235)).collect();
    (w, h, ink)
}

/// The overlap of two ink masks of one size over a box of pixels (x0, y0, x1, y1): the share of the pixels either has that both have.
fn overlap(a: &(usize, usize, Vec<bool>), b: &(usize, usize, Vec<bool>), (x0, y0, x1, y1): (usize, usize, usize, usize)) -> f64 {
    assert_eq!((a.0, a.1), (b.0, b.1), "the two pictures have the same size");
    let (mut both, mut either) = (0usize, 0usize);
    for y in y0..y1.min(a.1) {
        for x in x0..x1.min(a.0) {
            let (p, q) = (a.2[y * a.0 + x], b.2[y * b.0 + x]);
            both += usize::from(p && q);
            either += usize::from(p || q);
        }
    }
    if either == 0 { 1.0 } else { both as f64 / either as f64 }
}

/// **LibreOffice 26.8.2.2 draws the matrix as the session does.** Twenty right triangles on one page (the rows the four flips, the columns the five turns), saved to an `.odg`,
/// converted by LibreOffice to a PDF (a black box: its own profile in the folder, only a missing output is a failure) and drawn at 100 pixels to the inch beside DiaCraft's own
/// picture of the session. Each triangle's cell (the box 0.77 inch each side of its pin) is scored by the overlap of the ink; the **baseline** is the same turn with no flip (the
/// strokes and the anti-aliasing of two programs make it about 0.98 or more), and a case must come within 0.02 of it. On the writer and the reader before a change a triangle flipped
/// on one axis and turned scored 0.05 to 0.29 against LibreOffice; and DiaCraft's picture of a LibreOffice file made with the same mirrors and turns scored as low.
#[test]
#[ignore = "a black-box check against LibreOffice 26.8.2.2, run by hand with ODG_FLIPTURN_DIR set to an empty folder"]
fn flipturn_lo_draws_the_matrix_as_the_session_does() {
    let folder = std::path::PathBuf::from(std::env::var("ODG_FLIPTURN_DIR").expect("set ODG_FLIPTURN_DIR to an empty folder"));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("flipturn.odg");
    let mut s = Session::new();
    std::fs::write(&file, Drawing::new().page("").bytes()).unwrap();
    run(&mut s, "file.open", json!({ "path": file.to_str().unwrap() }));
    // The page is 11.69 inches tall; the model's y is up from its foot. The triangles are made row by row, so the shape at `r * 5 + c` is the one of that row and column.
    for (r, flips) in FLIPS.iter().enumerate() {
        for (c, angle) in ANGLES.iter().enumerate() {
            triangle(&mut s, (1.0 + 1.55 * c as f64, 11.69 - (1.2 + 1.7 * r as f64)), (1.18, 0.59), flips, *angle);
        }
    }
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["warning_count"], 0, "{saved}");
    let dia = folder.join("dia.png");
    run(&mut s, "file.export", json!({ "path": dia.to_str().unwrap(), "dpi": 100 }));
    // LibreOffice, a black box: a profile of its own in the folder; only a missing output is a failure.
    let out = folder.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let status = std::process::Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", folder.display()))
        .args(["--headless", "--norestore", "--convert-to", "pdf", "--outdir"])
        .arg(&out)
        .arg(&file)
        .output()
        .expect("soffice is installed");
    let pdf = out.join("flipturn.pdf");
    assert!(pdf.exists(), "LibreOffice wrote no PDF ({:?}): {}", status.status, String::from_utf8_lossy(&status.stderr));
    let raster = std::process::Command::new("pdftoppm")
        .args(["-r", "100", "-png", "-singlefile"])
        .arg(&pdf)
        .arg(out.join("lo"))
        .output()
        .expect("pdftoppm is installed");
    assert!(raster.status.success(), "pdftoppm: {}", String::from_utf8_lossy(&raster.stderr));
    let lo = ink_of(&std::fs::read(out.join("lo.png")).unwrap());
    let ours = ink_of(&std::fs::read(&dia).unwrap());
    let mut scores = vec![vec![0.0f64; ANGLES.len()]; FLIPS.len()];
    for (r, row) in scores.iter_mut().enumerate() {
        for (c, score) in row.iter_mut().enumerate() {
            let pin = frame_at(s.document().unwrap(), 0, r * ANGLES.len() + c);
            let (px, py) = ((pin[0] * 100.0) as usize, ((11.69 - pin[1]) * 100.0) as usize);
            *score = overlap(&lo, &ours, (px.saturating_sub(77), py.saturating_sub(77), px + 77, py + 77));
        }
    }
    println!("overlap of the ink, LibreOffice against the session (columns: turns {ANGLES:?}; rows: flips {FLIPS:?})");
    for (flips, row) in FLIPS.iter().zip(&scores) {
        println!("  {:>3}: {}", if flips.is_empty() { "-" } else { flips }, row.iter().map(|v| format!("{v:.3}")).collect::<Vec<_>>().join(" "));
    }
    for (r, row) in scores.iter().enumerate() {
        for (c, score) in row.iter().enumerate() {
            let baseline = scores[0][c];
            assert!(
                *score >= 0.9 && *score >= baseline - 0.02,
                "flips {:?} turned {}: {score:.3} against a baseline of {baseline:.3}",
                FLIPS[r],
                ANGLES[c]
            );
        }
    }
}
