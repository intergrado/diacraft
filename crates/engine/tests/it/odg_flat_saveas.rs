//! A drawing opened from a flat `.fodg` may be saved by Save As as a `.diacraft` file and as an `.odg` package, and across to a VSDX type; Save, in place,
//! still writes the flat file, and a `.fodg` is never a Save As target. What a flat file holds that the package forms hold otherwise (the pictures
//! inline as base64, no manifest, one container of automatic styles) comes back: the `.diacraft` file reopens to an equal model, and the `.odg` is the same content by the `.odg` content
//! check (`diacraft_odg::compare_content`, which is what `compare --content` runs). The flat drawings are written by hand from the standard (`Drawing::flat_xml`).

use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::native_common::{opened as opened_pinned, pinned, status};
use crate::odg_build::{Dir, Drawing, rect};
use crate::odg_file::{edit_cases, four, shift};
use crate::{check_session, run};

/// The flat drawing as it states its version: `office:version="1.4"`, the one a package written from the model states on every part, so that the version, which
/// makes no content of, is no difference whether or not the check has that ruling yet.
fn flat_with_settings(flat: Vec<u8>) -> Vec<u8> {
    String::from_utf8(flat).expect("a flat drawing is text").replacen("office:version=\"1.3\"", "office:version=\"1.4\"", 1).into_bytes()
}

/// The settings of a flat drawing, as a program that keeps its view in the file writes them (the `config` vocabulary of ODF Part 3, section 3.10).
const SETTINGS: &str = concat!(
    "<office:settings><config:config-item-set config:name=\"ooo:view-settings\">",
    "<config:config-item config:name=\"VisibleAreaTop\" config:type=\"int\">0</config:config-item>",
    "<config:config-item config:name=\"ZoomFactor\" config:type=\"short\">100</config:config-item>",
    "</config:config-item-set></office:settings>"
);

/// The flat drawing with an `office:settings` after its `office:meta` (the order) and the `config` namespace on its root. A drawing that has none is written an empty `settings.xml` by the
/// model writer, which makes the same as none: the fixtures have settings, so that what they say is not left to that ruling.
fn with_settings(flat: Vec<u8>) -> Vec<u8> {
    String::from_utf8(flat)
        .expect("a flat drawing is text")
        .replacen("<office:document ", "<office:document xmlns:config=\"urn:oasis:names:tc:opendocument:xmlns:config:1.0\" ", 1)
        .replacen("</office:meta>", &format!("</office:meta>{SETTINGS}"), 1)
        .into_bytes()
}

/// A flat drawing as the tests that expect the same content have it: of version 1.4, with settings.
fn fixture(flat: Vec<u8>) -> Vec<u8> {
    flat_with_settings(with_settings(flat))
}

/// A 2 by 2 PNG whose four pixels are all different, so that a picture that was lost or replaced by a placeholder is told from it.
fn png() -> Vec<u8> {
    let pixels = [10u8, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255];
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("a header").write_image_data(&pixels).expect("the pixels");
    out
}

/// Base64 by RFC 4648 in lines of 72 characters (what a flat file holds a picture as).
fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let n = u32::from_be_bytes([0, chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)]);
        for k in 0..4usize {
            text.push(if k <= chunk.len() { char::from(alphabet[((n >> (18 - 6 * k)) & 63) as usize]) } else { '=' });
        }
    }
    let lines: Vec<&str> = text.as_bytes().chunks(72).map(|c| std::str::from_utf8(c).expect("base64 is ASCII")).collect();
    lines.join("\n")
}

/// A flat drawing of a rectangle and a picture, inline as `office:binary-data`: the bytes of the picture, and the file.
fn with_a_picture() -> (Vec<u8>, Vec<u8>) {
    let picture = png();
    let frame = format!(
        "<draw:frame draw:style-name=\"gr2\" draw:layer=\"layout\" draw:name=\"Pic\" svg:width=\"4cm\" svg:height=\"3cm\" svg:x=\"2cm\" svg:y=\"9cm\"><draw:image draw:mime-type=\"image/png\"><office:binary-data>{}</office:binary-data></draw:image></draw:frame>",
        base64(&picture)
    );
    let drawing = Drawing::new().page(&format!("{}{frame}", rect("A", (2.0, 3.0, 4.0, 2.0), "hello")));
    (picture, fixture(drawing.flat_bytes()))
}

/// The two-page drawing of the flat tests, with a connector glued to both shapes of the second page.
fn two_pages() -> Vec<u8> {
    fixture(crate::odg_flat::two_pages().flat_bytes())
}

/// The drawings the tests save: a name, the flat file.
fn flats() -> Vec<(&'static str, Vec<u8>)> {
    vec![("four rectangles", fixture(four().flat_bytes())), ("two pages with a glued connector", two_pages()), ("a picture", with_a_picture().1)]
}

/// The drawings of [`flats`] that are **the same content** as the package the model writer makes of them: all of them. A drawing of two pages that share an automatic style was not, before
/// (second round): the writer gives the shapes of the second page styles of their own, copies of the shared one that carry DiaCraft's own markup
/// (`diacraft:version` and `diacraft:id` on the style's graphic properties), and the markup counted. Copies of one style count as one now and DiaCraft's markup
/// counts as nothing when the written file adds it.
fn same_content_flats() -> Vec<(&'static str, Vec<u8>)> {
    flats()
}

/// A session that has opened `bytes` as `name` in `dir`, its clock pinned.
fn opened_flat(dir: &Dir, name: &str, bytes: &[u8]) -> Session {
    opened_pinned(&dir.write(name, bytes))
}

/// The words of a refusal.
fn refusal(s: &mut Session, command: &str, params: Value) -> String {
    match s.execute(command, &params) {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } | EngineError::Disabled { reason: msg, .. }) => msg,
        other => panic!("{command} {params}: expected a refusal, got {other:?}"),
    }
}

/// The result of the content check of two OpenDocument drawings: how many differences there are, and the first of them as text.
fn content_differences(a: &[u8], b: &[u8]) -> (u64, Vec<String>) {
    let diff = diacraft_odg::compare_content(a, b, &diacraft_odg::Limits::default()).expect("both drawings are read");
    (diff.count, diff.shown.iter().map(|d| format!("{}: {}", d.part, d.path)).collect())
}

// ------------------------------------------------------------------------------------------------------------------ Save As `.diacraft`

/// A drawing opened from a flat file is saved as a `.diacraft` file, which reopens as an equal model (pictures, glue, pages and all); the flat file is as it was, the `.diacraft` file is the
/// drawing's own from then on, and a Save writes it.
#[test]
fn a_flat_drawing_saved_as_diacraft_reopens_as_an_equal_model() {
    for (what, flat) in flats() {
        let dir = Dir::new();
        let mut s = opened_flat(&dir, "plan.fodg", &flat);
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("plan.diacraft") }));
        assert_eq!(
            (saved["variant"].as_str(), saved["kind"].as_str(), saved["dirty"].as_bool()),
            (Some("diacraft"), Some("drawing"), Some(false)),
            "{what}: {saved}"
        );
        assert_eq!(dir.read("plan.fodg"), flat, "{what}: the flat file is as it was");
        assert_eq!(
            status(&mut s),
            json!({ "dirty": false, "has_file": true, "format": "diacraft", "variant": "diacraft", "kind": "drawing" }),
            "{what}"
        );
        assert!(!s.source().is_some_and(diacraft_engine::file::OpenFile::is_flat), "{what}: the drawing stands on the `.diacraft` file now");
        let mut t = opened_pinned(&dir.path("plan.diacraft"));
        assert!(s.document() == t.document(), "{what}: the model read back is not the model of the flat drawing");
        assert!(!t.is_dirty(), "{what}");
        check_session(&t).unwrap();
        // The drawing goes on in its new file.
        let again = run(&mut t, "file.save", json!({}));
        assert_eq!(again["variant"], "diacraft", "{what}: {again}");
    }
}

/// An edit made to a flat drawing is in the `.diacraft` file, and so is what the flat writer refuses to save in place (a page added, a master brought by a stencil shape): that is the way
/// out the person has, and it loses nothing.
#[test]
fn what_the_flat_writer_refuses_is_saved_as_diacraft() {
    let dir = Dir::new();
    let flat = fixture(four().flat_bytes());
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    shift(&mut s, "A", 1.0, 0.5);
    run(&mut s, "page.add", json!({ "name": "Added" }));
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 5.0, "y": 5.0 }));
    let words = refusal(&mut s, "file.save", json!({}));
    assert!(words.contains("flat"), "Save in place is refused for what a flat file has no place for: {words}");
    assert_eq!(dir.read("plan.fodg"), flat, "and writes nothing");
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.diacraft") }));
    let t = opened_pinned(&dir.path("plan.diacraft"));
    assert!(s.document() == t.document(), "the edited drawing is read back as it was");
    assert_eq!(t.document().unwrap().pages.len(), 2, "the page is there");
    assert_eq!(dir.read("plan.fodg"), flat, "the flat file is still as it was");
}

// ------------------------------------------------------------------------------------------------------------------ Save As `.odg`

/// A drawing opened from a flat file is saved as an `.odg` package that has **the same content** by the content check: the flat file is compared with the package as the check reads a
/// flat file against a package, and finds nothing that differs. The package is the drawing's own file now, reads back as the drawing, and a save after it is no edit.
/// (A drawing of two pages that share an automatic style is the same content too: `same_content_flats`.)
#[test]
fn a_flat_drawing_saved_as_odg_is_the_same_content_by_the_content_check() {
    for (what, flat) in same_content_flats() {
        let dir = Dir::new();
        let mut s = opened_flat(&dir, "plan.fodg", &flat);
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
        assert_eq!(
            (saved["variant"].as_str(), saved["kind"].as_str(), saved["form"].as_str(), saved["dirty"].as_bool()),
            (Some("odg"), Some("drawing"), Some("package"), Some(false)),
            "{what}: {saved}"
        );
        let written = dir.read("plan.odg");
        assert_eq!(diacraft_odg::Form::of(&written), diacraft_odg::Form::Package, "{what}: a package, not a flat file");
        assert_eq!(dir.read("plan.fodg"), flat, "{what}: the flat file is as it was");
        if same_content_flats().iter().any(|(name, _)| *name == what) {
            assert_eq!(content_differences(&flat, &written), (0, Vec::new()), "{what}: the package has the content of the flat file");
        }
        assert_eq!(status(&mut s), json!({ "dirty": false, "has_file": true, "format": "odg", "variant": "odg", "kind": "drawing" }), "{what}");
        assert!(!s.source().is_some_and(diacraft_engine::file::OpenFile::is_flat), "{what}: the drawing stands on the package now");
        // It reads back as the drawing, in the same picture, and the next save is the file's own bytes.
        let mut t = opened_pinned(&dir.path("plan.odg"));
        crate::odg_flat::assert_same_model(&dir, &mut s, &mut t, what);
        let again = run(&mut s, "file.save", json!({}));
        assert_eq!((again["form"].as_str(), again["edited"].as_bool()), (Some("package"), Some(false)), "{what}: {again}");
        assert_eq!(dir.read("plan.odg"), written, "{what}: a save that changes nothing is the file");
    }
}

/// The version the flat file states is no difference of content: a flat drawing of 1.3 is the same content as the package that says 1.4 (brought the check to the
/// ruling; before it, **the only thing that differed was the version attribute**, and nothing else of the drawing was lost).
#[test]
fn a_flat_drawing_of_another_version_is_the_same_content() {
    let dir = Dir::new();
    let flat = with_settings(four().flat_bytes());
    assert!(String::from_utf8_lossy(&flat).contains("office:version=\"1.3\""), "the fixture says 1.3");
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    assert_eq!(content_differences(&flat, &dir.read("plan.odg")), (0, Vec::new()), "the version counts as nothing");
}

/// A flat drawing that has no `office:settings` is written an empty `settings.xml`, which counts as none: the drawing is the same content as the package that holds the empty
/// part (brought the check to the ruling; before it, **the only thing that differed was the settings part**, which held nothing, and the rest of the drawing, its generator
/// included, was the same).
#[test]
fn a_flat_drawing_with_no_settings_is_the_same_content_as_the_package_with_an_empty_settings_part() {
    let dir = Dir::new();
    let flat = flat_with_settings(four().flat_bytes());
    assert!(!String::from_utf8_lossy(&flat).contains("office:settings"));
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    let written = dir.read("plan.odg");
    assert_eq!(content_differences(&flat, &written), (0, Vec::new()), "an empty settings part is none");
    let settings = crate::odg_build::part_of(&written, "settings.xml");
    assert!(!settings.contains("config-item"), "and it holds no setting: {settings}");
}

/// What a flat drawing's `office:meta` and `office:settings` hold comes back in the package and in the `.diacraft` file: the generator of the program that made the file (the drawing is what
/// the file held, so the generator is its), the title and the properties, and the settings whole.
#[test]
fn the_meta_and_the_settings_of_a_flat_drawing_come_back() {
    let dir = Dir::new();
    let flat = String::from_utf8(fixture(four().flat_bytes()))
        .expect("text")
        .replacen(
            "<meta:generator>TestWriter/1</meta:generator>",
            concat!(
                "<meta:generator>TestWriter/1</meta:generator><dc:title>A plan</dc:title><meta:initial-creator>Someone</meta:initial-creator>",
                "<meta:creation-date>2030-01-02T03:04:05</meta:creation-date>",
                "<meta:user-defined meta:name=\"Project\" meta:value-type=\"string\">Apollo</meta:user-defined>",
                "<meta:document-statistic meta:object-count=\"4\"/>"
            ),
            1,
        )
        .into_bytes();
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    assert_eq!(&*s.document().unwrap().title, "A plan", "the flat file's title is the drawing's");
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    let written = dir.read("plan.odg");
    let meta = crate::odg_build::part_of(&written, "meta.xml");
    for part in [
        "<meta:generator>TestWriter/1</meta:generator>",
        "<dc:title>A plan</dc:title>",
        "Someone",
        "2030-01-02T03:04:05",
        "Project",
        "Apollo",
        "meta:object-count=\"4\"",
    ] {
        assert!(meta.contains(part), "meta.xml keeps {part}: {meta}");
    }
    assert!(crate::odg_build::part_of(&written, "settings.xml").contains("ZoomFactor"), "settings.xml keeps the settings");
    assert_eq!(content_differences(&flat, &written), (0, Vec::new()));
    // The `.diacraft` file, from a session of its own (the first stands on the package now, whose own data the next save takes).
    let mut other = opened_flat(&dir, "other.fodg", &flat);
    run(&mut other, "file.save_as", json!({ "path": dir.path("plan.diacraft") }));
    let t = opened_pinned(&dir.path("plan.diacraft"));
    assert!(other.document() == t.document(), "the `.diacraft` file reads back as the same model, title and properties included");
    assert_eq!(&*t.document().unwrap().title, "A plan");
    assert_eq!(t.document().unwrap().props.created.as_deref(), Some("2030-01-02T03:04:05"));
}

/// A picture a flat file holds inline (base64) is a picture of the package: an entry under `Pictures/` with the very bytes, named by the frame, with a row in the manifest, and no base64 left in
/// the content; the `.diacraft` file holds the same bytes, and both read back with the picture.
#[test]
fn the_picture_of_a_flat_drawing_comes_back_in_both_forms() {
    let (picture, flat) = with_a_picture();
    let dir = Dir::new();
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    let written = dir.read("plan.odg");
    let content = crate::odg_build::part_of(&written, "content.xml");
    assert!(!content.contains("office:binary-data"), "the picture is no longer inline: {content}");
    let href = content.split("xlink:href=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("the frame names an entry").to_owned();
    assert!(href.starts_with("Pictures/"), "{href}");
    assert_eq!(crate::odg_build::part_bytes(&written, &href), Some(picture.clone()), "the entry is the very bytes of the picture");
    let manifest = crate::odg_build::part_of(&written, "META-INF/manifest.xml");
    assert!(manifest.contains(&href) && manifest.contains("image/png"), "the manifest has a row for it: {manifest}");
    // The `.odg` reads back with the picture, and so does the `.diacraft` file.
    let (doc, report) = crate::odg_build::read_back(&written);
    assert!(report.warnings.iter().all(|w| !w.contains("picture")), "{:?}", report.warnings);
    let bytes_of = |doc: &diacraft_doc::Document| -> Vec<Vec<u8>> {
        doc.pages
            .iter()
            .flat_map(|p| p.shapes.iter())
            .filter_map(|shape| shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()))
            .collect()
    };
    assert_eq!(bytes_of(&doc), vec![picture.clone()], "the `.odg` holds the picture");
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.diacraft") }));
    let t = opened_pinned(&dir.path("plan.diacraft"));
    assert_eq!(bytes_of(t.document().unwrap()), vec![picture], "the `.diacraft` file holds the picture");
}

/// An edit made to a flat drawing is in the `.odg`, which is the content of the drawing as it is, and a change the flat writer refuses to save in place (a page added) is written.
#[test]
fn an_edited_flat_drawing_is_saved_as_odg_with_the_edit() {
    for (what, edit) in edit_cases() {
        let dir = Dir::new();
        let mut s = opened_flat(&dir, "plan.fodg", &fixture(four().flat_bytes()));
        let before = s.document().unwrap().clone();
        edit(&mut s, &before);
        let saved = match s.execute("file.save_as", &json!({ "path": dir.path("plan.odg") })) {
            Ok(saved) => saved,
            Err(e) => panic!("{what}: {e}"),
        };
        assert_eq!(saved["form"], "package", "{what}");
        crate::odg_file::assert_reopens_as_saved(&dir, &mut s, "plan.odg", 0);
    }
}

/// The unedited flat drawing visits `.diacraft` and comes back as an `.odg` that is the one the flat drawing is saved as directly: a visit changes nothing of what the drawing is (the design's
/// section 4.4: the stamp of the content as it was read goes with the `.diacraft` file, and the generator of the file it was read from with it).
#[test]
fn a_visit_through_diacraft_gives_the_odg_the_flat_drawing_is_saved_as() {
    for (what, flat) in flats() {
        let dir = Dir::new();
        let mut direct = opened_flat(&dir, "plan.fodg", &flat);
        run(&mut direct, "file.save_as", json!({ "path": dir.path("direct.odg") }));
        let mut visiting = opened_flat(&dir, "plan.fodg", &flat);
        run(&mut visiting, "file.save_as", json!({ "path": dir.path("visit.diacraft") }));
        let mut back = opened_pinned(&dir.path("visit.diacraft"));
        assert_eq!(back.content_unchanged(), Some(true), "{what}: the `.diacraft` file says the drawing is what the flat file held");
        run(&mut back, "file.save_as", json!({ "path": dir.path("visit.odg") }));
        assert_eq!(dir.read("visit.odg"), dir.read("direct.odg"), "{what}: the visit changed what the drawing is saved as");
        if same_content_flats().iter().any(|(name, _)| *name == what) {
            assert_eq!(content_differences(&flat, &dir.read("visit.odg")), (0, Vec::new()), "{what}: and it is the content of the flat file");
        }
    }
}

/// A flat drawing is saved as a template (`.otg`) by the name, as any drawing is; `.fodg` is no kind of Save As.
#[test]
fn a_flat_drawing_is_saved_as_a_template_by_its_name() {
    let dir = Dir::new();
    let mut s = opened_flat(&dir, "plan.fodg", &fixture(four().flat_bytes()));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("plan.otg") }));
    assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("otg"), Some("template")), "{saved}");
    assert_eq!(diacraft_odg::sniff(&dir.read("plan.otg")), Some(diacraft_odg::Variant::Template));
}

// ------------------------------------------------------------------------------------------------------------------ Save As a VSDX type

/// A flat drawing saved as a VSDX type goes across the formats as an `.odg` does: the file is written from the model, it says what it carries, and it reads back as a
/// drawing of the same pages and shapes.
#[test]
fn a_flat_drawing_saved_as_a_vsdx_type_is_written_across() {
    for (what, flat) in flats() {
        let dir = Dir::new();
        let mut s = opened_flat(&dir, "plan.fodg", &flat);
        let pages = s.document().unwrap().pages.len();
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("plan.vsdx") }));
        assert_eq!(saved["variant"], "vsdx", "{what}: {saved}");
        assert!(saved.get("carried").is_some(), "{what}: the result says what the file carries: {saved}");
        assert_eq!(dir.read("plan.fodg"), flat, "{what}: the flat file is as it was");
        assert_eq!(status(&mut s), json!({ "dirty": false, "has_file": true, "format": "vsdx", "variant": "vsdx", "kind": "drawing" }), "{what}");
        let (doc, _) = diacraft_vsdx::read(&dir.read("plan.vsdx"), &diacraft_vsdx::Limits::default()).expect("a VSDX file that reads");
        assert_eq!(doc.pages.len(), pages, "{what}: the pages");
        let names = |d: &diacraft_doc::Document| -> Vec<Vec<String>> {
            d.pages.iter().map(|p| p.shapes.iter().map(|x| x.name_u.as_deref().unwrap_or_default().to_owned()).collect()).collect()
        };
        assert_eq!(names(&doc), names(s.document().unwrap()), "{what}: the shapes");
        let pictures = |d: &diacraft_doc::Document| -> Vec<Vec<u8>> {
            d.pages
                .iter()
                .flat_map(|p| p.shapes.iter())
                .filter_map(|shape| shape.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec()))
                .collect()
        };
        assert_eq!(pictures(&doc), pictures(s.document().unwrap()), "{what}: the pictures");
        check_session(&s).unwrap();
    }
}

// ------------------------------------------------------------------------------------------------------------------ what stays

/// A `.fodg` is no kind of Save As, for a flat drawing as for any other: refused in words that point to the `.odg` package, with nothing written and the drawing as it was. Save, in place,
/// still writes the flat file.
#[test]
fn save_as_fodg_stays_refused_for_a_flat_drawing_and_save_in_place_stays_flat() {
    let dir = Dir::new();
    let flat = fixture(four().flat_bytes());
    let mut s = opened_flat(&dir, "plan.fodg", &flat);
    shift(&mut s, "A", 1.0, 0.0);
    let before = crate::snapshot(&s);
    for target in ["other.fodg", "OTHER.FODG", "plan.fodg"] {
        for force in [false, true] {
            let words = refusal(&mut s, "file.save_as", json!({ "path": dir.path(target), "force": force }));
            for part in ["flat", ".fodg", "not written by Save As", "Save the drawing as an .odg", "Nothing was written"] {
                assert!(words.contains(part), "{target}: says {part}: {words}");
            }
        }
    }
    assert_eq!(dir.files(), ["plan.fodg"], "nothing was written");
    assert_eq!(dir.read("plan.fodg"), flat);
    assert_eq!(crate::snapshot(&s), before, "the drawing and the session are as they were");
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!((saved["form"].as_str(), saved["edited"].as_bool()), (Some("flat"), Some(true)), "{saved}");
    assert_eq!(diacraft_odg::Form::of(&dir.read("plan.fodg")), diacraft_odg::Form::Flat, "Save in place writes the flat file");
}

/// The command is there for a flat drawing, and the types the window offers are the engine's answer: `.diacraft`, the VSDX types and the OpenDocument drawing and template, never `.fodg`.
#[test]
fn save_as_is_enabled_for_a_flat_drawing_and_offers_everything_but_fodg() {
    let dir = Dir::new();
    let s = opened_flat(&dir, "plan.fodg", &fixture(four().flat_bytes()));
    let listed = s.commands().into_iter().find(|c| c.id == "file.save_as").expect("the command");
    assert!(listed.enabled && listed.disabled_reason.is_none(), "{listed:?}");
    for extension in ["diacraft", "diacraft-template", "vsdx", "vsdm", "vstx", "odg", "otg", "ODG"] {
        assert!(s.save_as_allows(extension), "{extension}");
    }
    for extension in ["fodg", "FODG", "txt", ""] {
        assert!(!s.save_as_allows(extension), "{extension:?}");
    }
    let mut other = pinned();
    run(&mut other, "file.new", json!({}));
    for extension in ["diacraft", "odg", "otg", "vsdx"] {
        assert_eq!(s.save_as_allows(extension), other.save_as_allows(extension), "a flat drawing is offered what a new one is: {extension}");
    }
}

/// A flat file called by an `.odg` name is a flat file, and Save As over its own name is a Save As: it writes the package under the name, and the drawing is the package's from then on.
#[test]
fn a_flat_file_called_odg_is_made_a_package_by_a_save_as_over_its_name() {
    let dir = Dir::new();
    let flat = fixture(four().flat_bytes());
    let mut s = opened_flat(&dir, "plan.odg", &flat);
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    assert_eq!(diacraft_odg::Form::of(&dir.read("plan.odg")), diacraft_odg::Form::Package);
    assert!(!s.source().is_some_and(diacraft_engine::file::OpenFile::is_flat));
    assert_eq!(content_differences(&flat, &dir.read("plan.odg")), (0, Vec::new()));
}

/// A flat drawing saved in place keeps what Save As needs of the file it is now: the file written by an edited Save is the flat file, and the package saved from it afterwards holds the edit.
#[test]
fn a_flat_drawing_saved_in_place_and_then_as_odg_holds_both() {
    let dir = Dir::new();
    let mut s = opened_flat(&dir, "plan.fodg", &fixture(four().flat_bytes()));
    shift(&mut s, "A", 1.0, 0.5);
    assert_eq!(run(&mut s, "file.save", json!({}))["form"], "flat");
    let after_save = dir.read("plan.fodg");
    run(&mut s, "file.save_as", json!({ "path": dir.path("plan.odg") }));
    assert_eq!(dir.read("plan.fodg"), after_save, "the flat file is as the save left it");
    let mut t = opened_pinned(&dir.path("plan.odg"));
    crate::odg_flat::assert_same_model(&dir, &mut s, &mut t, "the edit saved in place and then as an .odg");
    assert_eq!(content_differences(&after_save, &dir.read("plan.odg")).0, 0, "the package is the content of the flat file as it was saved");
}
