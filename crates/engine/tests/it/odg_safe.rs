//! A bitmap fill through `.diacraft` and back (the owner's "ship safe" for 0.2: a picture is never dropped with no word). An `.odg` is opened, saved as `.diacraft`, opened
//! again in a new session and saved as an `.odg`, as `roundtrip --via diacraft` does: the fill's picture comes back with the bytes it had, under the name it had, with its definition and its
//! manifest row, for a shape's own style, for a style sheet and for two shapes that share one, and the content check finds no difference in the drawing's styles or its pictures.

use diacraft_engine::Session;
use serde_json::{Value, json};

use crate::native_common::{Dir, opened, pinned};
use crate::odg_build::{Drawing, part_bytes, part_of};
use crate::run;

/// A PNG of 2 by 2 pixels that another program draws: red, green, blue and white.
fn png() -> Vec<u8> {
    diacraft_render::export::encode_png(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255], None).unwrap()
}

const FILL_IMAGE: &str =
    r#"<draw:fill-image draw:name="Img1" xlink:href="Pictures/img1.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;

fn rect(style: &str, x: f64) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="{x}cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

/// The three drawings of the brief: a shape with a bitmap fill, a style sheet with one, two shapes sharing one.
fn vectors() -> Vec<(&'static str, Vec<u8>)> {
    let bitmap = |name: &str, more: &str| {
        format!(
            r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"{more}/></style:style>"#
        )
    };
    let mut shape = Drawing::new().common(FILL_IMAGE).page(&rect("gr3", 2.0)).file("Pictures/img1.png", &png());
    shape.auto.push_str(&bitmap("gr3", ""));
    let mut sheet = Drawing::new()
        .common(FILL_IMAGE)
        .common(r#"<style:style style:name="Pictured" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/></style:style>"#)
        .page(&rect("gr4", 2.0))
        .file("Pictures/img1.png", &png());
    sheet.auto.push_str(r#"<style:style style:name="gr4" style:family="graphic" style:parent-style-name="Pictured"/>"#);
    let mut two = Drawing::new().common(FILL_IMAGE).page(&format!("{}{}", rect("gr3", 2.0), rect("gr5", 8.0))).file("Pictures/img1.png", &png());
    two.auto.push_str(&bitmap("gr3", ""));
    two.auto.push_str(&bitmap("gr5", r#" style:repeat="stretch""#));
    vec![
        ("a shape with a bitmap fill", shape.bytes()),
        ("a style sheet with a bitmap fill", sheet.bytes()),
        ("two shapes sharing one bitmap", two.bytes()),
    ]
}

/// The `.odg` that comes back from a visit of `source` through `.diacraft`, and what the last save said.
fn visit(source: &[u8]) -> (Vec<u8>, Value) {
    let dir = Dir::new();
    let mut s: Session = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    let said = run(&mut t, "file.save_as", json!({ "path": dir.path("w.odg") }));
    (dir.read("w.odg"), said)
}

/// The brief's test: each drawing visits `.diacraft` and comes back with its picture (the same bytes, the entry it had, the definition and the manifest row), the content check finds no
/// difference in `styles.xml` or in a picture, and the save says nothing of a definition left out.
#[test]
fn a_bitmap_fill_comes_back_from_a_visit_through_diacraft_with_its_picture() {
    for (what, source) in vectors() {
        let (written, said) = visit(&source);
        assert_eq!(part_bytes(&written, "Pictures/img1.png"), Some(png()), "{what}: the picture's bytes");
        let styles = part_of(&written, "styles.xml");
        assert!(styles.contains(r#"<draw:fill-image draw:name="Img1" xlink:href="Pictures/img1.png""#), "{what}: {styles}");
        let manifest = part_of(&written, "META-INF/manifest.xml");
        assert!(manifest.contains(r#"manifest:full-path="Pictures/img1.png" manifest:media-type="image/png""#), "{what}: {manifest}");
        let diff = diacraft_odg::compare_content(&source, &written, &diacraft_odg::Limits::default()).unwrap();
        let ours: Vec<String> = diff
            .shown
            .iter()
            .filter(|d| d.part == "styles.xml" || d.part.starts_with("Pictures/"))
            .map(|d| format!("{} {} {:?}", d.part, d.path, d.found))
            .collect();
        assert_eq!(ours, Vec::<String>::new(), "{what}: the styles and the pictures are the same content");
        assert!(!said.to_string().contains("definition"), "{what}: no definition is said to be left out: {said}");
    }
    // The style sheet says its fill natively, so that another program shows it on the shape that names the style.
    let (written, _) = visit(&vectors()[1].1);
    assert!(
        part_of(&written, "styles.xml")
            .contains(r#"style:name="Pictured" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/>"#)
    );
    // Two shapes of one picture: one entry, and the second keeps its repeat mode.
    let (written, _) = visit(&vectors()[2].1);
    assert!(part_of(&written, "content.xml").contains(r#"style:repeat="stretch""#));
    let package = diacraft_odg::Package::read(&written, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(package.parts().filter(|p| p.starts_with("Pictures/")).count(), 1);
}
