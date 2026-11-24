//! What `shape.gradient`, `shape.format` (a pattern) and `shape.shadow` make is written natively in an `.odg`, from a new drawing (the
//! model writer) and into a drawing read from a file (the splice), and comes back as what the controls show. A gradient and a hatch are definitions of DiaCraft's own in `office:styles`, a
//! shadow is the style's attributes, and what ODF has no form for rides in DiaCraft's data.
//!
//! The black-box check against LibreOffice (`lo_draws_what_diacraft_draws`) is ignored: it runs `soffice` and is started by hand with `ODG_FX_DIR` set to an empty folder.

use diacraft_engine::Session;
use diacraft_odg::Limits;
use diacraft_odg::package::Package;
use serde_json::{Value, json};

use crate::odg_build::Dir;
use crate::{blank_session, run};

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

fn shape_ids(s: &Session) -> Vec<u32> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().map(|shape| shape.id).collect()
}

/// What each shape looks like, as `shape.formatting` says (the fill, the pattern, the gradient, the shadow and the line).
fn looks(s: &mut Session) -> Vec<Value> {
    let page = page_id(s);
    let ids = shape_ids(s);
    let mut out = run(s, "shape.formatting", json!({"ids": ids, "page": page}))["shapes"].as_array().cloned().unwrap();
    for look in &mut out {
        if let Some(map) = look.as_object_mut() {
            for key in [
                "font",
                "size",
                "bold",
                "italic",
                "underline",
                "strike",
                "color",
                "align",
                "valign",
                "bullet",
                "ind_left",
                "case",
                "direction",
                "vertical",
            ] {
                map.remove(key);
            }
        }
    }
    out
}

fn same(what: &str, got: &[Value], want: &[Value]) {
    assert_eq!(got.len(), want.len(), "{what}: the shapes");
    for (g, w) in got.iter().zip(want) {
        for key in [
            "fill",
            "line",
            "weight",
            "fill_pattern",
            "fill_transparency",
            "fill_background",
            "line_transparency",
            "line_cap",
            "compound",
            "gradient",
            "line_gradient",
            "shadow",
        ] {
            assert_eq!(g[key], w[key], "{what}: shape {}: {key}", w["id"]);
        }
    }
}

/// `count` rectangles on the page, in rows of eight.
fn grid(s: &mut Session, count: usize) -> Vec<u32> {
    (0..count)
        .map(|k| {
            let (c, r) = ((k % 8) as f64, (k / 8) as f64);
            run(s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.2 + 1.2 * c, "y": 1.0 + 1.0 * r}))["id"].as_u64().unwrap() as u32
        })
        .collect()
}

/// The matrix the task measures: each layout of a gradient (0 to 13), each pattern (1 to 24, the solid fill first) and three shadows. Gradients have three stops, the middle one clear by a
/// quarter, as `shape.gradient` makes them.
fn matrix() -> Session {
    matrix_with(25)
}

/// [`matrix`] with the middle stop of each gradient `clear` percent clear (a transparency that ODF has no form for beside a gradient's stops).
fn matrix_with(clear: u32) -> Session {
    let mut s = blank_session();
    let ids = grid(&mut s, 14 + 24 + 3);
    for (dir, id) in (0..=13u32).zip(&ids) {
        run(
            &mut s,
            "shape.gradient",
            json!({"ids": [id], "direction": dir, "angle": 35, "stops": [
                {"position": 0, "color": "#C00000"}, {"position": 0.35, "color": "#00A040", "transparency": clear}, {"position": 1, "color": "#2030E0"}]}),
        );
    }
    for (pattern, id) in (1..=24u32).zip(&ids[14..]) {
        run(&mut s, "shape.format", json!({"ids": [id], "fill": "#A02010", "fill_pattern": pattern, "fill_background": "#FFF0C0"}));
    }
    let shadows = &ids[38..];
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [shadows[0]], "preset": "bottom_right", "color": "#202020", "transparency": 45, "blur": 6, "distance": 9, "angle": 70}),
    );
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [shadows[1]], "preset": "top_left", "color": "#0040C0", "transparency": 20, "blur": 0, "distance": 12}),
    );
    run(&mut s, "shape.shadow", json!({"ids": [shadows[2]], "preset": "center", "color": "#000000", "transparency": 70, "size": 120, "blur": 10}));
    s
}

fn parts(path: &str) -> (String, String) {
    let bytes = std::fs::read(path).unwrap();
    let pkg = Package::read(&bytes, &Limits::default()).unwrap();
    let text = |name: &str| String::from_utf8_lossy(pkg.part(name).unwrap()).into_owned();
    (text("content.xml"), text("styles.xml"))
}

#[test]
fn every_layout_of_a_gradient_every_pattern_and_three_shadows_come_back_from_a_new_odg_written_natively() {
    let mut s = matrix();
    let want = looks(&mut s);
    assert!(
        want[3]["gradient"].is_object() && want[16]["fill_pattern"] == json!(3) && want[39]["shadow"].is_object(),
        "the shapes have what they were given: {:?}",
        want[3]
    );
    let dir = Dir::new();
    let path = dir.path("matrix.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (content, styles) = parts(&path);
    // Fourteen layouts (the path one is written as the rectangular one from the centre), and fifteen of the patterns are lines.
    assert_eq!(styles.matches("<draw:gradient ").count(), 14, "{styles}");
    assert_eq!(styles.matches("<draw:hatch ").count(), 15, "{styles}");
    assert_eq!(content.matches("draw:shadow=\"visible\"").count(), 3, "three shadows");
    assert!(content.matches("draw:fill=\"gradient\"").count() == 14 && content.matches("draw:fill=\"hatch\"").count() == 15, "{content}");
    let mut back = reopened(&path);
    same("odg", &looks(&mut back), &want);
    // A second save of the drawing that was read (its own file, spliced) gives the same markup: nothing is added, and the definitions are not written twice.
    run(&mut back, "file.save", json!({}));
    let (content_again, styles_again) = parts(&path);
    assert_eq!(styles_again.matches("<draw:gradient ").count(), 14);
    assert_eq!(styles_again.matches("<draw:hatch ").count(), 15);
    assert_eq!(content_again.matches("draw:fill=\"gradient\"").count(), 14);
    same("odg saved again", &looks(&mut reopened(&path)), &want);
}

#[test]
fn a_gradient_a_pattern_and_a_shadow_set_on_the_shapes_of_a_file_are_written_into_it() {
    let mut plain = blank_session();
    grid(&mut plain, 41);
    let dir = Dir::new();
    let source = dir.path("source.odg");
    run(&mut plain, "file.save_as", json!({ "path": source }));
    let mut s = reopened(&source);
    let page = page_id(&s);
    let ids = shape_ids(&s);
    for (dir, id) in (0..=13u32).zip(&ids) {
        run(
            &mut s,
            "shape.gradient",
            json!({"ids": [id], "page": page, "direction": dir, "angle": 35, "stops": [
                {"position": 0, "color": "#C00000"}, {"position": 0.35, "color": "#00A040", "transparency": 25}, {"position": 1, "color": "#2030E0"}]}),
        );
    }
    for (pattern, id) in (1..=24u32).zip(&ids[14..]) {
        run(&mut s, "shape.format", json!({"ids": [id], "page": page, "fill": "#A02010", "fill_pattern": pattern, "fill_background": "#FFF0C0"}));
    }
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [ids[38]], "page": page, "preset": "bottom_right", "color": "#202020", "transparency": 45, "blur": 6, "distance": 9, "angle": 70}),
    );
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [ids[39]], "page": page, "preset": "top_left", "color": "#0040C0", "transparency": 20, "blur": 0, "distance": 12}),
    );
    run(
        &mut s,
        "shape.shadow",
        json!({"ids": [ids[40]], "page": page, "preset": "center", "color": "#000000", "transparency": 70, "size": 120, "blur": 10}),
    );
    let want = looks(&mut s);
    run(&mut s, "file.save", json!({}));
    let (content, styles) = parts(&source);
    // The gradients are written for the shapes the file held. The patterns of an existing shape stay as the file had them (the fill is the marks' colour) and are carried.
    assert_eq!(styles.matches("<draw:gradient ").count(), 14, "{styles}");
    assert_eq!(content.matches("draw:shadow=\"visible\"").count(), 3);
    let mut back = reopened(&source);
    same("odg spliced", &looks(&mut back), &want);
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_FX_DIR set to an empty folder"]
fn lo_draws_what_diacraft_draws() {
    let folder = std::path::PathBuf::from(std::env::var("ODG_FX_DIR").expect("set ODG_FX_DIR to an empty folder"));
    std::fs::create_dir_all(&folder).unwrap();
    // The matrix as `shape.gradient` makes it, and the same with opaque stops (the transparency of a stop has no form in ODF, so the layouts are compared without it).
    for (name, clear) in [("clear", 25u32), ("opaque", 0)] {
        let dir = folder.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = matrix_with(clear);
        let path = dir.join("matrix.odg").to_str().unwrap().to_owned();
        run(&mut s, "file.save_as", json!({ "path": path }));
        run(&mut s, "file.export", json!({"path": dir.join("diacraft.png").to_str().unwrap(), "dpi": 96}));
        println!("wrote {path} and diacraft.png: run LibreOffice on the first, then compare the pictures");
    }
}

/// What a save by LibreOffice (`soffice --convert-to odg` of `opaque/matrix.odg`, into `lo_saved/`) leaves of the looks, when DiaCraft opens it: the black-box check that LibreOffice's rewrite of
/// the definitions and styles leaves the cells DiaCraft carries as they were. Ignored: run by hand after `lo_draws_what_diacraft_draws` and LibreOffice, with `ODG_FX_DIR` set.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_FX_DIR set to the folder lo_draws_what_diacraft_draws wrote"]
fn what_libreoffice_saves_of_our_file_reads_back_as_it_was() {
    let folder = std::path::PathBuf::from(std::env::var("ODG_FX_DIR").expect("set ODG_FX_DIR"));
    let mut made = matrix_with(0);
    let want = looks(&mut made);
    let saved = folder.join("lo_saved").join("matrix.odg");
    let mut back = reopened(saved.to_str().unwrap());
    let got = looks(&mut back);
    assert_eq!(got.len(), want.len(), "the shapes");
    let mut differ = Vec::new();
    for (g, w) in got.iter().zip(&want) {
        for key in ["fill", "fill_pattern", "fill_transparency", "fill_background", "gradient", "shadow"] {
            if g[key] != w[key] {
                differ.push(format!("shape {} {key}: got {} / want {}", w["id"], g[key], w[key]));
            }
        }
    }
    println!("{} differences of {} shapes", differ.len(), want.len());
    for d in differ.iter().take(40) {
        println!("  {d}");
    }
}
