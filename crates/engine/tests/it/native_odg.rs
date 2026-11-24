//! The `.odg` half of the native format in the engine: a `.diacraft` or new drawing saved as an `.odg` or `.otg` goes through the model writer
//! (`diacraft_odg::write_with(doc, None, ..)`), the thumbnail and the generator follow the content stamp, an `.otg` is a format to open and save, and a drawing that visits
//! `.diacraft` comes back as an `.odg` of the same content (the design's 4.1, 4.4 and 7).

use diacraft_doc::Document;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::edit::drop_shape;
use crate::native_common::{Dir, PINNED, made, opened, pinned, status};
use crate::odg_build::{Drawing, TEMPLATE, part_bytes, part_of, rect};
use crate::run;

/// A PNG that no rendering of ours makes: a thumbnail a file held, so that a save that kept it can be told from one that drew its own.
fn held_png() -> Vec<u8> {
    diacraft_render::export::encode_png(3, 2, &[200, 30, 60, 255].repeat(6), None).unwrap()
}

/// The text of a refusal of `command`.
fn refusal(s: &mut Session, command: &str, params: Value) -> String {
    match s.execute(command, &params) {
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. } | EngineError::Disabled { reason: msg, .. }) => msg,
        other => panic!("{command} {params}: expected a refusal, got {other:?}"),
    }
}

/// The thumbnail a package holds.
fn thumbnail_of(bytes: &[u8]) -> Option<Vec<u8>> {
    part_bytes(bytes, "Thumbnails/thumbnail.png")
}

/// The generator `meta.xml` names.
fn generator_of(bytes: &[u8]) -> String {
    let meta = part_of(bytes, "meta.xml");
    meta.split_once("<meta:generator>")
        .and_then(|(_, rest)| rest.split_once("</meta:generator>"))
        .map(|(name, _)| name.to_owned())
        .unwrap_or_default()
}

/// What a package says it is.
fn kind_of(bytes: &[u8]) -> Option<diacraft_odg::Variant> {
    diacraft_odg::sniff(bytes)
}

/// An `.odg` of one page with a rectangle, holding a thumbnail of its own and the generator of another program.
fn source_with_thumbnail() -> Vec<u8> {
    Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello")).file("Thumbnails/thumbnail.png", &held_png()).bytes()
}

/// A rectangle dropped on the first page of a drawing read from an `.odg` (its pages are numbered from 0).
fn drop_on_the_odg_page(s: &mut Session) {
    run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 5.0, "y": 5.0 }));
}

/// Every page of `doc` drawn at 40 pixels to the inch on white paper: its size and its pixels.
fn pictures(doc: &Document) -> Vec<(u32, u32, Vec<u8>)> {
    let mut renderer = diacraft_render::Renderer::with_shared_fonts(diacraft_engine::measure::renderer_fonts());
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    doc.pages
        .iter()
        .map(|page| {
            let view = diacraft_render::View::fit_page(doc, page.id, 40.0).expect("a page that can be drawn");
            let out = renderer.render(doc, page.id, &view);
            (out.width, out.height, out.rgba_premul)
        })
        .collect()
}

/// How many pixels, over all the pages, two drawings differ in (by more than a few levels of a channel). The pages are as many and as large.
pub(crate) fn pixels_apart(a: &Document, b: &Document) -> usize {
    let (a, b) = (pictures(a), pictures(b));
    assert_eq!(a.len(), b.len(), "as many pages");
    let mut apart = 0;
    for ((wa, ha, pa), (wb, hb, pb)) in a.iter().zip(&b) {
        assert_eq!((wa, ha), (wb, hb), "each page is the size it was");
        apart += pa.chunks(4).zip(pb.chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 24)).count();
    }
    apart
}

/// A shape's text as an editor shows it, for each shape of the first page.
fn texts_of(doc: &Document) -> Vec<String> {
    doc.pages.iter().next().map_or_else(Vec::new, |page| {
        page.shapes.iter().map(|shape| diacraft_engine::draw::text_view(doc, page.id, shape.id).map(|v| v.chars).unwrap_or_default()).collect()
    })
}

// ---------------------------------------------------------------------------------------------------------------- the model path

/// The refusal of Call 18 is lifted: a `.diacraft` drawing, the same after a reopen, and a new drawing are saved as an `.odg` by the model writer. It writes a package of its own (a drawing
/// that has no file to splice into), and the file reads as the drawing that was saved, in the same picture; the file becomes the drawing's own, and a save after it is no edit.
#[test]
fn save_as_odg_from_diacraft_writes_with_the_model_writer() {
    let dir = Dir::new();
    let mut working = pinned();
    made(&mut working);
    run(&mut working, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    let mut reopened = opened(&dir.path("a.diacraft"));
    let mut brand_new = pinned();
    made(&mut brand_new);
    for (name, what, s) in
        [("w.odg", "saved as .diacraft", &mut working), ("r.odg", "opened from .diacraft", &mut reopened), ("n.odg", "new", &mut brand_new)]
    {
        let saved = run(s, "file.save_as", json!({ "path": dir.path(name) }));
        assert_eq!(
            (saved["variant"].as_str(), saved["kind"].as_str(), saved["form"].as_str(), saved["macros_dropped"].as_u64(), saved["dirty"].as_bool()),
            (Some("odg"), Some("drawing"), Some("package"), Some(0), Some(false)),
            "{what}: {saved}"
        );
        let written = dir.read(name);
        assert_eq!(kind_of(&written), Some(diacraft_odg::Variant::Drawing), "{what}");
        // The model writer's package: its own `office:version`, the generator and a thumbnail it rendered (no stamp to say the drawing is what a file held).
        assert!(part_of(&written, "content.xml").contains("office:version=\"1.4\""), "{what}");
        assert!(generator_of(&written).starts_with("DiaCraft"), "{what}: {}", generator_of(&written));
        let png = thumbnail_of(&written).unwrap_or_else(|| panic!("{what}: no thumbnail"));
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "{what}");
        // The file is the drawing's own now, and the drawing reads from it.
        assert_eq!(status(s), json!({ "dirty": false, "has_file": true, "format": "odg", "variant": "odg", "kind": "drawing" }), "{what}");
        assert!(s.source().is_some_and(|f| f.odg().is_some()), "{what}: the drawing stands on the `.odg` it wrote");
        let mut file = Session::new();
        run(&mut file, "file.open", json!({ "path": dir.path(name) }));
        assert_eq!(file.document().unwrap().pages.len(), 2, "{what}");
        assert_eq!(texts_of(file.document().unwrap()).len(), texts_of(s.document().unwrap()).len(), "{what}: the shapes of the first page");
        assert_eq!(pixels_apart(s.document().unwrap(), file.document().unwrap()), 0, "{what}: the same picture");
        // A save after it splices into the file, and an unchanged drawing is no edit.
        let again = run(s, "file.save", json!({}));
        assert_eq!(again["edited"], false, "{what}: {again}");
        assert_eq!(dir.read(name), written, "{what}: a second save is no edit");
    }
    // The model writer's file does not depend on what the session read: the same drawing, written after a reopen, is the same file.
    assert_eq!(dir.read("r.odg"), dir.read("w.odg"), "the save after a reopen is the save");
}

/// An edit made after a model save is an edit to the `.odg` it wrote: the splice writer takes the file the model writer made, saves the change, and the file read again holds it.
#[test]
fn an_edit_after_a_model_save_is_spliced_into_the_file_it_made() {
    let dir = Dir::new();
    let mut s = pinned();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.odg") }));
    let first = dir.read("a.odg");
    drop_shape(&mut s, "rectangle", 6.0, 6.0);
    assert!(s.is_dirty());
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["edited"], true, "{saved}");
    assert_ne!(dir.read("a.odg"), first);
    let mut file = Session::new();
    run(&mut file, "file.open", json!({ "path": dir.path("a.odg") }));
    let (before, after) = (texts_of(opened_len(&first).document().unwrap()).len(), texts_of(file.document().unwrap()).len());
    assert_eq!(after, before + 1, "the new rectangle is in the file");
    assert_eq!(pixels_apart(s.document().unwrap(), file.document().unwrap()), 0, "and the file draws what the model does");
}

/// A session that has opened the `.odg` `bytes`.
fn opened_len(bytes: &[u8]) -> Session {
    let dir = Dir::new();
    let path = dir.write("x.odg", bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// A drawing with every kind of start: the blank drawing and each built-in template, as new drawings, are saved as `.odg` and come back as the same picture.
#[test]
fn a_new_drawing_saved_as_odg_reopens_equal() {
    let dir = Dir::new();
    let listed = run(&mut pinned(), "template.list", json!({}));
    let mut starts: Vec<Option<String>> = vec![None];
    starts.extend(listed["templates"].as_array().unwrap().iter().map(|t| Some(t["id"].as_str().unwrap().to_owned())));
    for (n, start) in starts.into_iter().enumerate() {
        let mut s = Session::new();
        s.pin_clock(Some(PINNED));
        match &start {
            None => run(&mut s, "file.new", json!({})),
            Some(id) => run(&mut s, "file.new", json!({ "template": id })),
        };
        // A shape with a text and one with a fill, on whatever the first page is.
        let page = s.document().unwrap().pages.iter().next().unwrap().id;
        let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 3.0, "y": 3.0 }));
        run(&mut s, "shape.set_text", json!({ "id": a["id"], "text": "Hello" }));
        let b = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 3.0 }));
        run(&mut s, "shape.format", json!({ "ids": [b["id"]], "fill": "#2266aa", "weight": 2.0 }));
        let name = format!("n{n}.odg");
        run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
        let mut file = Session::new();
        run(&mut file, "file.open", json!({ "path": dir.path(&name) }));
        let what = format!("{start:?}");
        assert_eq!(file.document().unwrap().pages.len(), s.document().unwrap().pages.len(), "{what}");
        assert_eq!(texts_of(file.document().unwrap()), texts_of(s.document().unwrap()), "{what}: the text of each shape");
        assert_eq!(pixels_apart(s.document().unwrap(), file.document().unwrap()), 0, "{what}: the same picture");
    }
}

// -------------------------------------------------------------------------------------------------------------- the content stamp

/// **The thumbnail and the generator follow the content stamp, never the dirty flag** (the design's 4.4): an `.odg` that visits `.diacraft` and comes back unedited has the thumbnail and
/// the generator it was read with; an edit, made before the `.diacraft` save or in the session that opened it, gives the thumbnail of an edited save and DiaCraft's generator; an edit and
/// its undo is no edit; a drawing that was never read from a file has no stamp, and a thumbnail is rendered for it. (A hand mutant that takes `!s.is_dirty()` in place of the stamp
/// comparison, or that renders a thumbnail at every save, must fail this.)
#[test]
fn on_the_model_path_the_odg_thumbnail_follows_the_stamp() {
    let dir = Dir::new();
    let held = held_png();
    let source = dir.write("a.odg", &source_with_thumbnail());
    assert_eq!(thumbnail_of(&dir.read("a.odg")), Some(held.clone()));
    let mut v = pinned();
    run(&mut v, "file.open", json!({ "path": source }));
    assert!(v.content_stamp_pending(), "an `.odg` read from a file is a foreign file: its stamp is made when it is needed");
    run(&mut v, "file.save_as", json!({ "path": dir.path("x.diacraft") }));
    // Unedited: the file's own bookkeeping, in any number of sessions.
    let mut t = opened(&dir.path("x.diacraft"));
    assert_eq!(t.content_unchanged(), Some(true));
    run(&mut t, "file.save_as", json!({ "path": dir.path("b.odg") }));
    assert_eq!(thumbnail_of(&dir.read("b.odg")), Some(held.clone()), "the thumbnail is the one the file held");
    assert_eq!(generator_of(&dir.read("b.odg")), "TestWriter/1", "and the generator");
    // An edit and its undo is no edit.
    let mut u = opened(&dir.path("x.diacraft"));
    drop_on_the_odg_page(&mut u);
    assert_eq!(u.content_unchanged(), Some(false));
    run(&mut u, "edit.undo", json!({}));
    assert_eq!(u.content_unchanged(), Some(true));
    run(&mut u, "file.save_as", json!({ "path": dir.path("c.odg") }));
    assert_eq!(thumbnail_of(&dir.read("c.odg")), Some(held.clone()), "an edit and its undo are no edit");
    // An edit is an edited save: a thumbnail of the page as it is, and DiaCraft's generator.
    let mut w = opened(&dir.path("x.diacraft"));
    drop_on_the_odg_page(&mut w);
    run(&mut w, "file.save_as", json!({ "path": dir.path("d.odg") }));
    let edited = dir.read("d.odg");
    let png = thumbnail_of(&edited).expect("a thumbnail");
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']) && png != held, "the picture of the edited drawing, not the one the file held");
    assert!(generator_of(&edited).starts_with("DiaCraft"), "{}", generator_of(&edited));
    // The drawing edited before its `.diacraft` save is edited in the session that reads it, clean as that session is: the dirty flag does not survive a reopen, the stamp does.
    let mut e = pinned();
    run(&mut e, "file.open", json!({ "path": dir.path("a.odg") }));
    drop_on_the_odg_page(&mut e);
    run(&mut e, "file.save_as", json!({ "path": dir.path("y.diacraft") }));
    let mut f = opened(&dir.path("y.diacraft"));
    assert!(!f.is_dirty());
    run(&mut f, "file.save_as", json!({ "path": dir.path("e.odg") }));
    assert!(thumbnail_of(&dir.read("e.odg")).is_some_and(|png| png != held), "clean in the session, and not what the file held");
    assert!(generator_of(&dir.read("e.odg")).starts_with("DiaCraft"));
    // A drawing made new has no stamp: its first foreign save renders a thumbnail and writes the bookkeeping of a new file.
    let mut n = pinned();
    made(&mut n);
    assert_eq!(n.content_unchanged(), None);
    run(&mut n, "file.save_as", json!({ "path": dir.path("n.odg") }));
    assert!(thumbnail_of(&dir.read("n.odg")).is_some_and(|png| png.starts_with(&[0x89, b'P', b'N', b'G']) && png != held));
    assert!(generator_of(&dir.read("n.odg")).starts_with("DiaCraft"));
    // Rendered once and not again: the same drawing written after a reopen is the same file.
    let mut n2 = pinned();
    made(&mut n2);
    run(&mut n2, "file.save_as", json!({ "path": dir.path("n2.odg") }));
    assert_eq!(dir.read("n2.odg"), dir.read("n.odg"), "the same drawing, the same bytes, at the pinned clock");
}

// ------------------------------------------------------------------------------------------------------------------------ `.fodg`

/// A flat drawing is not a Save As target: the words say so, and point to the `.odg` package, which is what a drawing is saved as. Nothing is written and the drawing is as it was, whatever
/// the drawing is (a `.diacraft` drawing, a new one, an `.odg`).
#[test]
fn save_as_fodg_is_refused_in_words_pointing_to_odg() {
    let dir = Dir::new();
    let mut native = pinned();
    made(&mut native);
    run(&mut native, "file.save_as", json!({ "path": dir.path("n.diacraft") }));
    let mut new = pinned();
    made(&mut new);
    let mut odg = Session::new();
    run(&mut odg, "file.open", json!({ "path": dir.write("o.odg", &source_with_thumbnail()) }));
    for (what, s) in [("a .diacraft drawing", &mut native), ("a new drawing", &mut new), ("an .odg", &mut odg)] {
        let before = (s.is_dirty(), s.path().map(std::path::Path::to_path_buf), status(s));
        let words = refusal(s, "file.save_as", json!({ "path": dir.path("out.fodg") }));
        assert!(words.contains(".fodg") && words.contains("flat"), "{what}: {words}");
        assert!(words.contains(".odg"), "{what}: the words point to the .odg package: {words}");
        assert!(!dir.files().contains(&"out.fodg".to_owned()), "{what}: nothing is written");
        assert_eq!(before, (s.is_dirty(), s.path().map(std::path::Path::to_path_buf), status(s)), "{what}: the drawing is as it was");
    }
}

/// A drawing opened from a flat file is saved by Save, as the flat file it is, and reads again as the drawing it was; Save As writes it as a `.diacraft` file and as an `.odg` package
/// (`odg_flat_saveas.rs` holds them), and still refuses a `.fodg`, in words that point to the `.odg`.
#[test]
fn a_flat_drawing_saves_flat_and_reopens_equal_and_save_as_writes_it_by_the_name() {
    let dir = Dir::new();
    let flat = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello")).flat_bytes();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("f.fodg", &flat) }));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["form"], "flat");
    assert_eq!(dir.read("f.fodg"), flat, "an unedited save of a flat drawing is the file's own bytes");
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("f.fodg") }));
    assert!(s.document() == t.document(), "the model read back is the model of the flat drawing");
    let words = refusal(&mut s, "file.save_as", json!({ "path": dir.path("x.fodg") }));
    assert!(words.contains("flat") && words.contains(".odg"), "{words}");
    for (name, variant) in [("x.odg", "odg"), ("x.diacraft", "diacraft")] {
        let saved = run(&mut s, "file.save_as", json!({ "path": dir.path(name) }));
        assert_eq!(saved["variant"], variant, "{name}: {saved}");
        assert!(dir.files().contains(&name.to_owned()), "{name}");
    }
    assert_eq!(dir.read("f.fodg"), flat, "the flat file is as it was");
}

// ------------------------------------------------------------------------------------------------------------------------ `.otg`

/// An `.otg` is a format to open and save, and no built-in template (design section 6 and 7): `file.open` makes a new unnamed drawing from it, `edit_template` opens the template itself,
/// and Save As writes an `.odg` (a drawing) or an `.otg` (a template) as the name says.
#[test]
fn an_otg_opens_as_a_new_drawing_and_saves_as_otg_when_asked() {
    let dir = Dir::new();
    let template = Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello")).bytes_of_type(TEMPLATE);
    let path = dir.write("t.otg", &template);
    assert_eq!(kind_of(&template), Some(diacraft_odg::Variant::Template));
    // `file.open` of an `.otg` is a new drawing made from it: no file, no path, and nothing of the template's bytes is kept (NP10).
    let mut s = pinned();
    let opened = run(&mut s, "file.open", json!({ "path": path }));
    assert_eq!(opened["file"]["new_from_template"], true, "{opened}");
    assert_eq!((opened["file"]["variant"].as_str(), opened["file"]["kind"].as_str()), (Some("otg"), Some("template")), "{opened}");
    assert!(s.source().is_none() && s.path().is_none(), "a new unnamed drawing");
    assert_eq!(status(&mut s), json!({ "dirty": false, "has_file": false, "format": null, "variant": null, "kind": null }));
    assert_eq!(s.document().unwrap().pages.len(), 1);
    assert!(refusal(&mut s, "file.save", json!({})).contains("file.save_as"), "no file to save to yet");
    // It is saved as a drawing by the name `.odg` says, with the model writer, and as a template when `.otg` is asked for.
    run(&mut s, "file.save_as", json!({ "path": dir.path("d.odg") }));
    assert_eq!(kind_of(&dir.read("d.odg")), Some(diacraft_odg::Variant::Drawing), "a drawing made from a template is a drawing");
    assert_eq!(status(&mut s), json!({ "dirty": false, "has_file": true, "format": "odg", "variant": "odg", "kind": "drawing" }));
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("o.otg") }));
    assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("otg"), Some("template")), "{saved}");
    assert_eq!(kind_of(&dir.read("o.otg")), Some(diacraft_odg::Variant::Template));
    assert_eq!(status(&mut s), json!({ "dirty": false, "has_file": true, "format": "odg", "variant": "otg", "kind": "template" }));
    // A new drawing too: `.otg` is what it is saved as when asked, and it opens again as a new drawing made from it.
    let mut n = pinned();
    made(&mut n);
    run(&mut n, "file.save_as", json!({ "path": dir.path("m.otg") }));
    assert_eq!(kind_of(&dir.read("m.otg")), Some(diacraft_odg::Variant::Template));
    let mut again = pinned();
    let reopened = run(&mut again, "file.open", json!({ "path": dir.path("m.otg") }));
    assert_eq!(reopened["file"]["new_from_template"], true);
    assert_eq!(pixels_apart(n.document().unwrap(), again.document().unwrap()), 0, "the template starts the drawing it was made from");
    // `edit_template` opens the template itself: it is saved as the template it is, and Save As `.odg` writes a drawing from it.
    let mut e = pinned();
    let itself = run(&mut e, "file.open", json!({ "path": path, "edit_template": true }));
    assert_eq!(itself["file"]["new_from_template"], false, "{itself}");
    assert_eq!(status(&mut e), json!({ "dirty": false, "has_file": true, "format": "odg", "variant": "otg", "kind": "template" }));
    run(&mut e, "page.add", json!({ "name": "Second" }));
    let saved = run(&mut e, "file.save", json!({}));
    assert_eq!((saved["variant"].as_str(), saved["kind"].as_str()), (Some("otg"), Some("template")), "{saved}");
    assert_eq!(kind_of(&dir.read("t.otg")), Some(diacraft_odg::Variant::Template));
    assert_eq!(diacraft_odg::read(&dir.read("t.otg"), &diacraft_odg::Limits::default()).unwrap().0.pages.len(), 2);
    run(&mut e, "file.save_as", json!({ "path": dir.path("from_template.odg") }));
    assert_eq!(kind_of(&dir.read("from_template.odg")), Some(diacraft_odg::Variant::Drawing));
    // `file.new` starts a drawing from the file as well, and a drawing is no template to start from.
    let mut f = pinned();
    let started = run(&mut f, "file.new", json!({ "template": dir.path("o.otg") }));
    assert_eq!(started["template"]["builtin"], false, "{started}");
    assert!(f.source().is_none());
    let words = refusal(&mut f, "file.new", json!({ "template": dir.path("d.odg") }));
    assert!(words.contains("not a template"), "{words}");
}

/// A `.diacraft` drawing is saved as a template, and a stencil is saved as neither an `.odg` nor an `.otg`: OpenDocument has no stencil format.
#[test]
fn a_diacraft_drawing_saves_as_otg_and_a_stencil_does_not_save_as_odg_or_otg() {
    let dir = Dir::new();
    let mut s = pinned();
    made(&mut s);
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.otg") }));
    assert_eq!(kind_of(&dir.read("a.otg")), Some(diacraft_odg::Variant::Template));
    let mut st = pinned();
    drop_shape(&mut st, "rectangle", 2.0, 2.0);
    run(&mut st, "file.save_as", json!({ "path": dir.path("s.diacraft-stencil") }));
    for name in ["s.odg", "s.otg"] {
        let words = refusal(&mut st, "file.save_as", json!({ "path": dir.path(name) }));
        assert!(words.contains("stencil"), "{name}: {words}");
        assert!(!dir.files().contains(&name.to_owned()));
    }
}

// ------------------------------------------------------------------------------------------------------------------------ visits

/// Writes the files the black-box check of another program's reader opens into the folder `DIACRAFT_LO_DIR` names: an `#[ignore]`d helper of the report's LibreOffice run, and no test.
#[test]
#[ignore = "writes files for a black-box check by hand: DIACRAFT_LO_DIR names the folder"]
fn write_the_files_for_the_black_box_check() {
    let Ok(folder) = std::env::var("DIACRAFT_LO_DIR") else { return };
    let at = |name: &str| format!("{folder}/{name}");
    // A drawing a person made, as an `.odg` and as an `.otg`, from the `.diacraft` file and from the session.
    let mut made_one = pinned();
    made(&mut made_one);
    run(&mut made_one, "file.save_as", json!({ "path": at("a_made.diacraft") }));
    run(&mut made_one, "file.save_as", json!({ "path": at("a_made.odg") }));
    let mut again = opened(&at("a_made.diacraft"));
    run(&mut again, "file.save_as", json!({ "path": at("a_made.otg") }));
    run(&mut pinned(), "file.save_as", json!({ "path": at("b_blank.odg") }));
    // A drawing started from each built-in template, with a rectangle that holds a text.
    for (name, id) in [("c_diagram", "basic_diagram"), ("d_flowchart", "basic_flowchart")] {
        let mut s = Session::new();
        s.pin_clock(Some(PINNED));
        run(&mut s, "file.new", json!({ "template": id }));
        let page = s.document().unwrap().pages.iter().next().unwrap().id;
        let a = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 3.0, "y": 3.0 }));
        run(&mut s, "shape.set_text", json!({ "id": a["id"], "text": "Hello" }));
        run(&mut s, "file.save_as", json!({ "path": at(&format!("{name}.odg")) }));
    }
    // The visit of the first drawing through `.diacraft`, and a splice into the file the model writer made, and an `.otg` opened as a new drawing and saved as a drawing.
    let (visited, _) = visit(&Dir::new(), &std::fs::read(at("a_made.odg")).unwrap(), false);
    std::fs::write(at("f_visit.odg"), visited).unwrap();
    let mut spliced = pinned();
    made(&mut spliced);
    run(&mut spliced, "file.save_as", json!({ "path": at("g_spliced.odg") }));
    drop_shape(&mut spliced, "rectangle", 6.0, 6.0);
    run(&mut spliced, "file.save", json!({}));
    let mut from_template = pinned();
    run(&mut from_template, "file.open", json!({ "path": at("a_made.otg") }));
    run(&mut from_template, "file.save_as", json!({ "path": at("h_from_otg.odg") }));
}

/// What the content check says of an `.odg` that went through `.diacraft` and came back, and the file that came back.
fn visit(dir: &Dir, source: &[u8], edit: bool) -> (Vec<u8>, diacraft_container::canon::ContentDiff) {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("v.odg", source) }));
    if edit {
        drop_on_the_odg_page(&mut s);
    }
    run(&mut s, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let mut t = opened(&dir.path("v.diacraft"));
    run(&mut t, "file.save_as", json!({ "path": dir.path("w.odg") }));
    let written = dir.read("w.odg");
    let diff = diacraft_odg::compare_content(source, &written, &diacraft_odg::Limits::default()).unwrap();
    (written, diff)
}

/// A drawing a person made in DiaCraft and saved as an `.odg` by the model writer: the vectors a visit is made of (the writer's own output is the one that says `office:version` 1.4, has a
/// `settings.xml` and keeps no unused style, which a visit of any `.odg` gives back).
fn made_odg_vectors() -> Vec<(&'static str, bool, Vec<u8>)> {
    let page_of = |s: &Session| s.document().unwrap().pages.iter().next().unwrap().id;
    let mut vectors: Vec<(&'static str, bool, Vec<u8>)> = Vec::new();
    let mut save = |what: &'static str, shapes: bool, mut s: Session| {
        let dir = Dir::new();
        run(&mut s, "file.save_as", json!({ "path": dir.path("m.odg") }));
        vectors.push((what, shapes, dir.read("m.odg")));
    };
    save("an empty drawing", false, pinned());
    let mut shapes = pinned();
    let page = page_of(&shapes);
    let a = run(&mut shapes, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 3.0, "y": 3.0 }));
    run(&mut shapes, "shape.set_text", json!({ "id": a["id"], "text": "Hello" }));
    let b = run(&mut shapes, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 6.0, "y": 3.0 }));
    run(&mut shapes, "shape.format", json!({ "ids": [b["id"]], "fill": "#2266aa", "weight": 2.0 }));
    save("shapes with a text and a fill", true, shapes);
    let mut two = pinned();
    run(&mut two, "page.add", json!({ "name": "Second" }));
    save("two pages", false, two);
    let mut linked = pinned();
    let page = page_of(&linked);
    let a = run(&mut linked, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 3.0, "y": 3.0 }));
    let b = run(&mut linked, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 8.0, "y": 3.0 }));
    run(&mut linked, "connector.add", json!({ "from": { "id": a["id"] }, "to": { "id": b["id"] } }));
    save("two shapes and a connector", true, linked);
    let mut sized = pinned();
    run(&mut sized, "page.size", json!({ "width": 8.0, "height": 6.0 }));
    save("a page of its own size", false, sized);
    vectors
}

/// An `.odg` that visits `.diacraft` and comes back is the same content: the package, the document level rows, the meta and the settings, the thumbnail and the generator (the content check,
/// the lists: `compare --content`). The drawings are ones DiaCraft's model writer made, and the visit goes through a `.diacraft` file and a new session, as `roundtrip --via diacraft`
/// does. (A hand-built `.odg` of another program differs where the writer's own rules say it does, `office:version` 1.4 on every part among them: `measure_the_visits_of_hand_built_odgs`.)
///
/// **A drawing with shapes is the same content in all but the shapes' own markup until a later change**: the writer does not write a shape back as it was read yet, so the first save of
/// an engine-made shape carries its cells in DiaCraft's data and the save of the model read from it does not (a second model save is not byte-stable for them).
/// The package, the document level rows, `meta.xml`, `settings.xml`, the manifest, the thumbnail and the generator are the same for them already, and the shapes' elements are the only
/// difference: this holds a visit of such a drawing to that, and holds it to the whole content when the shapes are written back as read.
#[test]
fn an_odg_visit_through_diacraft_has_the_same_content() {
    for (what, shapes, source) in made_odg_vectors() {
        let dir = Dir::new();
        let (written, diff) = visit(&dir, &source, false);
        if shapes && !diff.same() {
            assert!(diff.shown.iter().all(|d| d.part == "content.xml"), "{what}: the shapes' own markup is the only difference: {:?}", diff.shown);
        } else {
            assert!(diff.same(), "{what}: the visit changed the content: {:?}", diff.shown);
        }
        assert_eq!(kind_of(&written), Some(diacraft_odg::Variant::Drawing), "{what}");
        // The thumbnail and the generator are the file's own.
        assert_eq!(thumbnail_of(&written), thumbnail_of(&source), "{what}: the thumbnail");
        assert_eq!(generator_of(&written), generator_of(&source), "{what}: the generator");
        if !shapes {
            // The second visit is the same file as the first, from the file that came back.
            let (again, diff) = visit(&Dir::new(), &written, false);
            assert!(diff.same(), "{what}: the second visit changed the content: {:?}", diff.shown);
            assert_eq!(again, written, "{what}: a visit of what came back is the same bytes (clock pinned)");
        }
    }
}

/// What a visit of hand-built `.odg` files changes, by what the content check says (an `#[ignore]` measure: `cargo test -p diacraft-engine --test it -- --ignored --nocapture
/// measure_the_visits_of_hand_built_odgs`). They are written like another program writes one, and the model writer says what it always says (ODF 1.4 on every part and
/// row, `settings.xml`, no unused automatic style), so no vector of them is the same content yet.
#[test]
#[ignore = "a measurement"]
fn measure_the_visits_of_hand_built_odgs() {
    let vectors: Vec<(&str, Vec<u8>)> = vec![
        ("an empty drawing", Drawing::new().page("").bytes()),
        ("an empty drawing with a thumbnail", Drawing::new().page("").file("Thumbnails/thumbnail.png", &held_png()).bytes()),
        ("two empty pages", Drawing::new().page("").page("").bytes()),
        ("a rectangle", Drawing::new().page(&rect("A", (2.0, 3.0, 4.0, 2.0), "hello")).bytes()),
    ];
    for (what, source) in vectors {
        let (_, diff) = visit(&Dir::new(), &source, false);
        let mut classes: Vec<String> = diff.shown.iter().map(|d| format!("{} {} {:?}", d.part, d.path, d.found)).collect();
        classes.sort();
        println!("{what}: {} differences", diff.count);
        for class in classes {
            println!("    {class}");
        }
    }
}

/// An edit that was made is a change of content, and the check says so: a visit with a salted edit differs, and only where the edit was (the page's shapes, and the bookkeeping an edited save
/// writes).
#[test]
fn a_salted_edit_is_a_difference_of_the_content_check() {
    let dir = Dir::new();
    let (written, diff) = visit(&dir, &Drawing::new().page("").bytes(), true);
    assert!(!diff.same(), "an edit is a difference");
    assert!(diff.shown.iter().any(|d| d.part == "content.xml"), "{:?}", diff.shown);
    assert!(generator_of(&written).starts_with("DiaCraft"));
}

/// A model with window settings: it holds a `windows` part, which the model writer does not write yet, so the save is refused in words, with the way out, and nothing is
/// written. (When the model writer writes the window's settings, this test says so and changes with it.)
#[test]
fn a_drawing_with_window_settings_is_saved_as_an_odg_or_refused_in_words() {
    let dir = Dir::new();
    let mut s = pinned();
    made(&mut s);
    run(&mut s, "window.settings.set", json!({ "grid": false }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.diacraft") }));
    match s.execute("file.save_as", &json!({ "path": dir.path("w.odg") })) {
        Ok(_) => {
            // Written: the file holds the settings, read back as they were.
            let mut t = Session::new();
            run(&mut t, "file.open", json!({ "path": dir.path("w.odg") }));
            assert!(t.document().unwrap().window_flags().is_some(), "the window's settings are in the file");
        }
        Err(EngineError::Failed { msg, .. } | EngineError::BadParams { msg, .. }) => {
            assert!(msg.contains("not written yet") && msg.contains(".diacraft"), "the refusal says what is missing and the way out: {msg}");
            assert!(msg.contains("Nothing was written"), "{msg}");
            assert!(!dir.files().contains(&"w.odg".to_owned()));
        }
        Err(other) => panic!("a refusal in words, or a save: {other:?}"),
    }
}

/// What a person's VSDX file gave a drawing is not DiaCraft's own skeleton, and is not left out: a `.diacraft` drawing that came from a VSDX file (its package data held, with a part no
/// field reads) is refused as an `.odg` in words, with the ways out, and nothing is written; a drawing started from a built-in template, whose VSDX data is exactly the skeleton's, is
/// written, and so is one whose window settings were turned, or refused in words until a later change writes them (never in a way that drops them).
#[test]
fn only_the_skeletons_own_vsdx_data_is_left_out_and_a_persons_is_refused_in_words() {
    let dir = Dir::new();
    let mut from_vsdx = pinned();
    run(&mut from_vsdx, "file.open", json!({ "path": dir.write("v.vsdx", &crate::native_common::vsdx_drawing()) }));
    run(&mut from_vsdx, "file.save_as", json!({ "path": dir.path("v.diacraft") }));
    let words = refusal(&mut from_vsdx, "file.save_as", json!({ "path": dir.path("v.odg") }));
    assert!(words.contains("Nothing was written") && words.contains(".diacraft") && words.contains(".vsdx"), "{words}");
    assert!(!dir.files().contains(&"v.odg".to_owned()));
    // A built-in template's drawing: the skeleton data goes, the Snap & Glue words stay, and the file reads back as the same picture.
    for id in ["basic_diagram", "basic_flowchart"] {
        let mut s = Session::new();
        s.pin_clock(Some(PINNED));
        run(&mut s, "file.new", json!({ "template": id }));
        run(&mut s, "snap.set", json!({ "grid": false }));
        let name = format!("{id}.odg");
        run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
        let mut file = Session::new();
        run(&mut file, "file.open", json!({ "path": dir.path(&name) }));
        assert_eq!(pixels_apart(s.document().unwrap(), file.document().unwrap()), 0, "{id}");
        assert!(file.document().unwrap().snap_words().snap.is_some(), "{id}: the Snap & Glue words are in the file");
        // The model of the session still holds the skeleton's data: the writer was given a copy.
        assert!(s.document().unwrap().vsdx.is_some() || s.source().is_some(), "{id}");
    }
}
