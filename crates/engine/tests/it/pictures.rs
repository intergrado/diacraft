//! A picture from a file (`insert.picture`), through `Session::execute` as the control channel, the CLI and MCP call it. A picture is a shape of `Type` `Foreign` with a
//! `ForeignData` and the cells `ImgOffsetX`, `ImgOffsetY`, `ImgWidth` and `ImgHeight`; the formats are the ones the renderer and the
//! readers know, and the budgets are theirs. The decisions the specs leave open are in `docs/editing-commands.md`, "Pictures".

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use diacraft_doc::{Document, Media, Owner, Shape, ShapeType};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::doc;
use crate::{blank_session, check_session, run, snapshot};

/// A folder of the test's own, taken away at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-pictures-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn path(&self, name: &str) -> &Path {
        let _ = name;
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- pictures made by hand: a header that says its size, and a body of whatever ------------------------------------------------------------------------------------------------

/// A PNG the `png` crate writes, `w` by `h` pixels.
pub(crate) fn real_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![200u8; (w * h * 4) as usize]).unwrap();
    }
    out
}

/// The signature and the header chunk of a PNG that says `w` by `h`, and `body` bytes of whatever after (no valid data: only the header is read to size it).
fn png_header(w: u32, h: u32, body: usize) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.resize(b.len() + body, 0x5A);
    b
}

fn jpeg_header(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0];
    b.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8]);
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9]);
    b
}

fn gif_header(w: u16, h: u16) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0x3B]);
    b
}

fn bmp_header(w: i32, h: i32) -> Vec<u8> {
    let mut b = b"BM".to_vec();
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&[1, 0, 24, 0]);
    b.resize(54, 0);
    b
}

fn tiff_header(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 2, 0];
    for (tag, value) in [(256u16, w), (257, h)] {
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&[3, 0, 1, 0, 0, 0]);
        b.extend_from_slice(&value.to_le_bytes());
        b.extend_from_slice(&[0, 0]);
    }
    b.extend_from_slice(&[0, 0, 0, 0]);
    b
}

/// An EMF header whose frame is `w` by `h` hundredths of a millimetre.
fn emf_header(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0u8; 88];
    b[0..4].copy_from_slice(&1u32.to_le_bytes());
    b[4..8].copy_from_slice(&88u32.to_le_bytes());
    b[16..20].copy_from_slice(&100i32.to_le_bytes());
    b[20..24].copy_from_slice(&100i32.to_le_bytes());
    b[32..36].copy_from_slice(&(w as i32).to_le_bytes());
    b[36..40].copy_from_slice(&(h as i32).to_le_bytes());
    b[40..44].copy_from_slice(b" EMF");
    b[44..48].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    b
}

/// A placeable WMF: a bounding box of `w` by `h` units, `per_inch` units to the inch.
fn wmf_header(w: i16, h: i16, per_inch: u16) -> Vec<u8> {
    let mut b = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
    for v in [0i16, 0, w, h] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&per_inch.to_le_bytes());
    b.resize(22, 0);
    b.extend_from_slice(&[1, 0, 9, 0, 0, 3]);
    b
}

fn insert(s: &mut Session, path: &str) -> Json {
    run(s, "insert.picture", json!({"path": path}))
}

fn refused(s: &mut Session, params: Json) -> String {
    let e = s.execute("insert.picture", &params).expect_err("the call is refused");
    match &e {
        diacraft_engine::EngineError::BadParams { msg, .. } | diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

fn shape(d: &Document, id: u32) -> Arc<Shape> {
    let at = d.find_shape(Owner::Page(1), id).expect("the shape");
    Arc::clone(d.shape(&at).expect("the shape"))
}

fn cell_formula(shape: &Shape, name: &str) -> Option<String> {
    shape.sheet.cells.get(name).and_then(|c| c.formula.text().map(str::to_owned))
}

fn cell_number(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cells.get(name).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_png_becomes_a_foreign_shape_with_the_cells_a_picture_has_at_its_own_size_in_the_middle_of_the_page() {
    let dir = Scratch::new();
    let bytes = real_png(96, 48);
    let path = dir.file("logo.png", &bytes);
    let mut s = blank_session();
    let steps = s.history().unwrap().undo_len();
    let made = insert(&mut s, &path);
    let id = made["id"].as_u64().unwrap() as u32;
    assert_eq!((made["format"].as_str(), made["drawn"].as_bool()), (Some("png"), Some(true)));
    assert!(
        (made["width"].as_f64().unwrap() - 1.0).abs() < 1e-9 && (made["height"].as_f64().unwrap() - 0.5).abs() < 1e-9,
        "96 by 48 pixels at 96 to the inch: {made}"
    );
    let d = doc(&s);
    let pic = shape(d, id);
    assert_eq!(pic.kind, ShapeType::Foreign);
    let foreign = pic.foreign.as_ref().expect("foreign data");
    assert_eq!(foreign.attr("ForeignType"), Some("Bitmap"));
    let media = foreign.picture.as_ref().expect("the picture");
    assert_eq!(media.bytes.as_ref(), bytes.as_slice(), "the picture's bytes are the file's, unchanged");
    assert_eq!(&*media.content_type, "image/png");
    // The box: write `Width*1` and `Height*1`, the offsets 0.
    assert_eq!((cell_formula(&pic, "ImgWidth").as_deref(), cell_formula(&pic, "ImgHeight").as_deref()), (Some("Width*1"), Some("Height*1")));
    assert_eq!((cell_number(&pic, "ImgOffsetX"), cell_number(&pic, "ImgOffsetY")), (0.0, 0.0));
    assert!((cell_number(&pic, "Width") - 1.0).abs() < 1e-9 && (cell_number(&pic, "Height") - 0.5).abs() < 1e-9);
    // The middle of the page.
    let setup = diacraft_engine::draw::read_setup(d, 1).unwrap();
    assert!((cell_number(&pic, "PinX") - setup.width / 2.0).abs() < 1e-9 && (cell_number(&pic, "PinY") - setup.height / 2.0).abs() < 1e-9);
    // One undo step; undo takes it away.
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(doc(&s).find_shape(Owner::Page(1), id).is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!(doc(&s).find_shape(Owner::Page(1), id).is_some());
}

#[test]
fn where_and_how_big_the_caller_says_and_a_picture_too_big_for_the_page_is_made_to_fit() {
    let dir = Scratch::new();
    let path = dir.file("a.png", &real_png(96, 48));
    let mut s = blank_session();
    let at = run(&mut s, "insert.picture", json!({"path": path, "x": 2.0, "y": 3.0, "width": 4.0, "height": 1.0}));
    let pic = shape(doc(&s), at["id"].as_u64().unwrap() as u32);
    assert_eq!((cell_number(&pic, "PinX"), cell_number(&pic, "PinY"), cell_number(&pic, "Width"), cell_number(&pic, "Height")), (2.0, 3.0, 4.0, 1.0));
    // A width alone keeps the picture's proportions.
    let wide = run(&mut s, "insert.picture", json!({"path": path, "width": 3.0}));
    assert_eq!((wide["width"].as_f64(), wide["height"].as_f64()), (Some(3.0), Some(1.5)));
    // 4,800 by 2,400 pixels is 50 by 25 inches: it is made to fit within the page, keeping its proportions.
    let big = dir.file("big.png", &png_header(4_800, 2_400, 100));
    let fit = insert(&mut s, &big);
    let setup = diacraft_engine::draw::read_setup(doc(&s), 1).unwrap();
    let (w, h) = (fit["width"].as_f64().unwrap(), fit["height"].as_f64().unwrap());
    assert!(w <= setup.width * 0.8 + 1e-9 && h <= setup.height * 0.8 + 1e-9 && ((w / h) - 2.0).abs() < 1e-9, "{fit}");
    // A box that is not more than 0, or past the page, or a place that is not a number: refused.
    for params in [
        json!({"path": path, "width": 0}),
        json!({"path": path, "width": -1.0}),
        json!({"path": path, "height": 1e9}),
        json!({"path": path, "x": "a"}),
    ] {
        let words = refused(&mut s, params.clone());
        assert!(words.contains("width") || words.contains("height") || words.contains("`x`"), "{params}: {words}");
    }
}

/// (file name, bytes, format word, ForeignType, content type, drawn, width, height in inches)
type Case = (&'static str, Vec<u8>, &'static str, &'static str, &'static str, bool, f64, f64);

#[test]
fn each_format_is_told_by_its_bytes_sized_by_its_header_and_typed_for_the_files() {
    let dir = Scratch::new();
    let mut s = blank_session();
    let cases: Vec<Case> = vec![
        ("a.jpg", jpeg_header(192, 96), "jpeg", "Bitmap", "image/jpeg", true, 2.0, 1.0),
        ("a.gif", gif_header(96, 96), "gif", "Bitmap", "image/gif", false, 1.0, 1.0),
        ("a.bmp", bmp_header(144, -72), "bmp", "Bitmap", "image/bmp", true, 1.5, 0.75),
        ("a.tif", tiff_header(288, 96), "tiff", "Bitmap", "image/tiff", false, 3.0, 1.0),
        ("a.emf", emf_header(5_080, 2_540), "emf", "EnhMetaFile", "image/x-emf", true, 2.0, 1.0),
        ("a.wmf", wmf_header(1440, 720, 720), "wmf", "EnhMetaFile", "image/x-emf", true, 2.0, 1.0),
    ];
    for (name, bytes, word, foreign_type, content_type, drawn, w, h) in cases {
        // The name says nothing: the bytes do.
        let path = dir.file(name, &bytes);
        let made = insert(&mut s, &path);
        assert_eq!((made["format"].as_str(), made["drawn"].as_bool()), (Some(word), Some(drawn)), "{name}: {made}");
        assert!((made["width"].as_f64().unwrap() - w).abs() < 1e-6 && (made["height"].as_f64().unwrap() - h).abs() < 1e-6, "{name}: {made}");
        if !drawn {
            assert!(made["note"].as_str().unwrap().contains("placeholder"), "{name}: a picture that is not drawn says so: {made}");
        } else {
            assert!(made.get("note").is_none_or(Json::is_null), "{name}: {made}");
        }
        let pic = shape(doc(&s), made["id"].as_u64().unwrap() as u32);
        let foreign = pic.foreign.as_ref().unwrap();
        assert_eq!(foreign.attr("ForeignType"), Some(foreign_type), "{name}");
        assert_eq!(&*foreign.picture.as_ref().unwrap().content_type, content_type, "{name}");
    }
    // A picture is judged by its bytes: a PNG called `.jpg` is a PNG.
    let named_wrong = dir.file("really-a-png.jpg", &real_png(8, 8));
    assert_eq!(insert(&mut s, &named_wrong)["format"], "png");
}

#[test]
fn the_same_file_twice_is_one_picture_held_once() {
    let dir = Scratch::new();
    let path = dir.file("a.png", &real_png(16, 16));
    let other = dir.file("copy-of-a.png", &real_png(16, 16));
    let different = dir.file("b.png", &real_png(17, 16));
    let mut s = blank_session();
    let (a, b, c, d) = (insert(&mut s, &path), insert(&mut s, &other), insert(&mut s, &different), insert(&mut s, &path));
    let held = |made: &Json| Arc::clone(shape(doc(&s), made["id"].as_u64().unwrap() as u32).foreign.as_ref().unwrap().picture.as_ref().unwrap());
    let (ma, mb, mc, md): (Arc<Media>, Arc<Media>, Arc<Media>, Arc<Media>) = (held(&a), held(&b), held(&c), held(&d));
    assert!(Arc::ptr_eq(&ma, &mb) && Arc::ptr_eq(&ma, &md), "the same bytes are one picture, however many shapes show them");
    assert!(!Arc::ptr_eq(&ma, &mc), "other bytes are another");
}

#[test]
fn what_is_no_picture_or_cannot_be_held_is_refused_in_words_and_the_drawing_is_as_it_was() {
    let dir = Scratch::new();
    let mut s = blank_session();
    let before = snapshot(&s);
    let svg = dir.file("a.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>");
    let xml = dir.file("a.xml", b"<?xml version=\"1.0\"?><svg/>");
    let cases: Vec<(String, &str)> = vec![
        (dir.file("empty.png", b""), "empty"),
        (svg, "SVG"),
        (xml, "SVG"),
        (dir.file("text.png", b"just some words in a file called png"), "not a picture"),
        (dir.file("one.png", &[0x89, b'P']), "not a picture"),
        (dir.file("cut.png", &png_header(10, 10, 0)[..20]), "cut short"),
        (dir.file("zero.png", &png_header(0, 10, 4)), "no size"),
        (dir.file("zero2.png", &png_header(10, 0, 4)), "no size"),
        (dir.file("pixels.png", &png_header(8_193, 8_192, 4)), "megapixels"),
        (dir.file("many.png", &png_header(u32::MAX, u32::MAX, 4)), "megapixels"),
        (dir.file("nosof.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xD9]), "cut short"),
        (dir.file("loop.jpg", &[[0xFF, 0xD8].as_slice(), &[0xFF, 0xE1, 0, 2].repeat(50_000)].concat()), "cut short"),
        (dir.file("zero.gif", &gif_header(0, 5)), "no size"),
        (dir.file("zero.bmp", &bmp_header(0, 5)), "no size"),
        (dir.file("neg.bmp", &bmp_header(-5, 5)), "no size"),
        (dir.file("cut.tif", &tiff_header(5, 5)[..12]), "cut short"),
        (dir.file("far.tif", &[b'I', b'I', 42, 0, 0xFF, 0xFF, 0xFF, 0x7F, 0, 0]), "cut short"),
        (dir.file("ifd.tif", &[&[b'I', b'I', 42, 0, 8, 0, 0, 0, 0xFF, 0xFF][..], &[0u8; 64]].concat()), "cut short"),
        (dir.file("zero.emf", &emf_header(0, 0)), "no size"),
        (dir.file("short.emf", &emf_header(10, 10)[..44]), "cut short"),
        (dir.file("zero.wmf", &wmf_header(0, 0, 96)), "no size"),
        (dir.file("noinch.wmf", &wmf_header(100, 100, 0)), "no size"),
    ];
    for (path, words_have) in cases {
        let words = refused(&mut s, json!({"path": path}));
        assert!(words.contains(words_have), "{path}: {words}");
        assert!(!words.contains(dir.path("").to_str().unwrap()), "the words hold no path of the person's: {words}");
    }
    // A missing file, a folder, a path no system can name and text that is not a path.
    for (path, words_have) in [
        (dir.path("").join("missing.png").to_str().unwrap().to_owned(), "no such file"),
        (dir.path("").to_str().unwrap().to_owned(), "not a file"),
        ("with\0nul.png".to_owned(), "no such file"),
        (String::new(), "path"),
    ] {
        let words = refused(&mut s, json!({"path": path}));
        assert!(words.contains(words_have), "{path:?}: {words}");
    }
    assert_eq!(snapshot(&s), before, "every refusal left the drawing as it was");
    check_session(&s).unwrap();
}

#[test]
fn a_file_past_the_size_a_picture_may_have_is_refused_by_its_size_before_a_byte_is_read() {
    let dir = Scratch::new();
    let mut s = blank_session();
    // A sparse file of one byte over 64 MiB: no space is taken, and no byte of it may be read.
    let path = dir.path("").join("huge.png");
    {
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
    }
    let words = refused(&mut s, json!({"path": path.to_str().unwrap()}));
    assert!(words.contains("64 MiB") && words.contains("at most"), "{words}");
    // One of exactly the most is read (its bytes are a header and 64 MiB of filler).
    let at_most = dir.file("most.png", &png_header(10, 10, 64 * 1024 * 1024 - 33));
    assert_eq!(std::fs::metadata(&at_most).unwrap().len(), 64 * 1024 * 1024);
    assert_eq!(insert(&mut s, &at_most)["format"], "png");
}

#[test]
fn the_pictures_of_a_drawing_are_held_to_what_a_file_may_hold_and_the_words_say_how_much_is_left() {
    // The VSDX reader holds the pictures of a drawing to 512 MiB between them: a drawing that has more could be written and not read.
    let dir = Scratch::new();
    let mut s = blank_session();
    let bytes: Arc<[u8]> = Arc::from(png_header(10, 10, 60 * 1024 * 1024));
    // Eight pictures of 60 MiB, which share the bytes in memory (the model counts what a file would hold: each picture of its own).
    s.edit("test: eight big pictures", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        for n in 0..8u32 {
            let media = Arc::new(Media { part: Arc::from(format!("big{n}")), content_type: Arc::from("image/png"), bytes: Arc::clone(&bytes) });
            let foreign = diacraft_doc::ForeignData { picture: Some(media), ..Default::default() };
            let shape = Shape { kind: ShapeType::Foreign, foreign: Some(Arc::new(foreign)), ..Shape::new(100 + n) };
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    let path = dir.file("one-more.png", &real_png(40, 40));
    // 8 x 60 MiB is 480 MiB of the 512: a small picture fits and one of 60 MiB does not, whatever the per-picture cap says.
    insert(&mut s, &path);
    let more = dir.file("sixty.png", &png_header(11, 10, 60 * 1024 * 1024));
    let words = refused(&mut s, json!({"path": more}));
    assert!(words.contains("512 MiB") && words.contains("pictures"), "{words}");
    assert_ne!(snapshot(&s), before, "the small one was put in");
    check_session(&s).unwrap();
}

#[test]
fn junk_parameters_are_refused_in_words() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [
        json!({}),
        json!({"path": 5}),
        json!({"path": null}),
        json!({"path": ["a"]}),
        json!({"path": "x", "page": "one"}),
        json!({"path": "x", "page": 99}),
        json!({"path": "x", "nope": 1}),
    ] {
        let e = s.execute("insert.picture", &params).expect_err("refused");
        assert!(!matches!(e, diacraft_engine::EngineError::Internal { .. }), "{params}: {e:?}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_picture_goes_on_the_page_named_and_joins_the_active_layers() {
    let dir = Scratch::new();
    let path = dir.file("a.png", &real_png(8, 8));
    let mut s = blank_session();
    let second = run(&mut s, "page.add", json!({"name": "Second"}))["id"].as_u64().unwrap();
    run(&mut s, "layer.add", json!({"name": "Art", "active": true}));
    let made = run(&mut s, "insert.picture", json!({"path": path, "page": second}));
    assert_eq!(made["page"], second);
    let at = doc(&s).find_shape(Owner::Page(second as u32), made["id"].as_u64().unwrap() as u32);
    assert!(at.is_some(), "on the second page");
    let on_first = run(&mut s, "insert.picture", json!({"path": path, "page": 1}));
    let members = diacraft_engine::draw::layers::shape_layers(&shape(doc(&s), on_first["id"].as_u64().unwrap() as u32));
    assert_eq!(members, [0], "a new shape joins the active layer");
}

// ---- hostile pictures under the address limit (`ulimit -v 3000000`) --------------------------------------------------------------------------------------------------------------

const BUDGET_CHILD: &str = "ENGINE_PICTURE_BUDGET";

/// The child of the test below, run under `ulimit -v 3000000`: a file of a gibibyte (sparse), a picture of the most bytes, and pictures of 60 MiB each until the drawing's budget refuses one; each is
/// inserted or refused in words, and nothing aborts.
#[test]
#[ignore = "run by the test below under `ulimit -v`: alone it needs the scenario in the environment"]
fn picture_budget_child() {
    let scenario = std::env::var(BUDGET_CHILD).expect("the scenario");
    let dir = Scratch::new();
    let mut s = blank_session();
    match scenario.as_str() {
        "sparse" => {
            // A gibibyte that takes no space: refused by its length, before a byte is read.
            let path = dir.path("").join("huge.png");
            std::fs::File::create(&path).unwrap().set_len(1 << 30).unwrap();
            let words = refused(&mut s, json!({"path": path.to_str().unwrap()}));
            assert!(words.contains("64 MiB"), "{words}");
        }
        "budget" => {
            // The most a picture may be, then pictures of 60 MiB, each other bytes, until the drawing's 512 MiB are spent.
            let mut accepted = 0usize;
            let mut said = String::new();
            for n in 0..12u32 {
                let name = format!("p{n}.png");
                let path = dir.file(&name, &png_header(10 + n, 10, 60 * 1024 * 1024));
                match s.execute("insert.picture", &json!({"path": path})) {
                    Ok(_) => accepted += 1,
                    Err(e) => {
                        said = e.to_string();
                        break;
                    }
                }
                std::fs::remove_file(dir.path("").join(&name)).unwrap();
            }
            assert_eq!(accepted, 8, "eight pictures of 60 MiB are 480 MiB of the 512");
            assert!(said.contains("512 MiB"), "refused in words: {said}");
            check_session(&s).unwrap();
        }
        other => panic!("no scenario {other}"),
    }
    println!("picture budget: no abort");
}

#[cfg(target_os = "linux")]
fn run_budget_child(scenario: &str) {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact pictures::picture_budget_child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(BUDGET_CHILD, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("picture budget: no abort"), "{scenario}: {text}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_file_of_a_gibibyte_is_refused_by_its_length_under_the_address_limit() {
    run_budget_child("sparse");
}

#[test]
#[cfg(target_os = "linux")]
fn pictures_up_to_the_drawings_budget_are_inserted_and_the_next_is_refused_in_words_under_the_address_limit() {
    run_budget_child("budget");
}

// ---- a header of any bytes -----------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_header_of_any_bytes_is_read_or_refused_and_never_panics() {
    use diacraft_engine::draw::pictures::inspect;
    // Every signature, cut at every length, and followed by every byte value where a size is read: the answer is a size or a refusal.
    let heads: Vec<Vec<u8>> = vec![
        real_png(5, 5),
        png_header(5, 5, 8),
        jpeg_header(5, 5),
        gif_header(5, 5),
        bmp_header(5, 5),
        tiff_header(5, 5),
        emf_header(100, 100),
        wmf_header(10, 10, 96),
    ];
    for head in &heads {
        for cut in 0..head.len().min(200) {
            let _ = inspect(&head[..cut]);
        }
        for at in 0..head.len().min(120) {
            for value in [0u8, 1, 0x7F, 0x80, 0xFF] {
                let mut b = head.clone();
                b[at] = value;
                let _ = inspect(&b);
            }
        }
    }
    // Noise, with each signature in front.
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..2_000 {
        let len = (next() % 160) as usize;
        let mut b: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let which = (next() % 9) as usize;
        if let Some(head) = heads.get(which) {
            let n = head.len().min(8).min(b.len());
            b[..n].copy_from_slice(&head[..n]);
        }
        let _ = inspect(&b);
    }
}
