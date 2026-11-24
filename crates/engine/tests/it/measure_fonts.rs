//! The faces the engine's commits measure text in (of the task review): a window shares the renderer's font database with the commits it
//! makes, so a block that is sized by `TEXTWIDTH` is sized in the faces its text is drawn in. Typing into a shape works out the cells that read its
//! text, and the width they get is the width of the letters of the database that was set.
//!
//! The database is process-wide, so this is one test: it sets, edits, and takes the database back.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_doc::{Cell, CellAddr, Document, Lookup, RowKey, Value};
use diacraft_engine::draw::set_text;
use diacraft_engine::measure::{measure_fonts, scan_fonts_with, set_measure_fonts};
use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

/// The setting is the process's: the tests of this file take turns (nextest gives each a process of its own; a plain `cargo test` does not).
static SETTING: Mutex<()> = Mutex::new(());

/// A database of one face, `Em`, in which every printable ASCII character advances `advance` thousandths of an em.
fn em_db(advance: u16) -> FontDb {
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(SynthFont::new("Em").ascii(advance).build()).added, 1);
    db
}

fn em(advance: u16) -> Arc<FontDb> {
    Arc::new(em_db(advance))
}

/// The small term a measure of a text that holds a character carries, in paper inches (a project rule).
const TERM: f64 = 5e-5;

/// A shape whose text is `a` in face `Em` at 12 points, with no margins, and a cell `User.W` holding `TEXTWIDTH(TheText)` that is stale.
fn doc() -> Document {
    let shape = ShapeBuilder::new(1)
        .text("a")
        .row_cell("Character", None, RowKey::Ix(0), "Font", "Em")
        .row_cell("Character", None, RowKey::Ix(0), "Size", &(12.0f64 / 72.0).to_string())
        .set(CellAddr::in_row("User", None, RowKey::name("W"), "Value"), Cell::with_formula("TEXTWIDTH(TheText)", Some(Value::from_text("9"))));
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn width(doc: &Document) -> f64 {
    let at = doc.find_shape(diacraft_doc::Owner::Page(1), 1).unwrap();
    match doc.shape(&at).unwrap().sheet.lookup(&CellAddr::in_row("User", None, RowKey::name("W"), "Value")) {
        Lookup::Found(c) => c.value.as_ref().unwrap().text().parse().unwrap(),
        other => panic!("{other:?}"),
    }
}

/// The scan of the machine's faces is still going when the window opens (a cold disk): a commit that measures no text goes on at once, and a
/// commit that measures text waits for the scan the first time and then measures in the faces the scan found, not in the bundled ones.
#[test]
fn a_commit_that_measures_text_waits_for_the_scan_and_one_that_does_not_does_not() {
    let _only = SETTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (release, gate) = channel::<()>();
    let fonts = scan_fonts_with(move || {
        let _ = gate.recv();
        em_db(1000)
    });
    assert!(!fonts.is_ready());
    // A shape whose text nothing measures: typing into it works out nothing that reads the faces.
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut plain = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text("a"))).build();
        let _ = tx.send(set_text(&mut plain, 1, 1, "ab").is_ok());
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(true), "a commit that measures no text does not wait for the scan");
    assert!(!fonts.is_ready(), "and the scan is still held back");
    // A shape whose width is `TEXTWIDTH` of its text: the commit needs the faces, and waits.
    let (tx, rx) = channel();
    let commit = std::thread::spawn(move || {
        let mut wide = doc();
        set_text(&mut wide, 1, 1, "ab").unwrap();
        let _ = tx.send(());
        width(&wide)
    });
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "a commit that measures text waits for the scan");
    release.send(()).unwrap();
    let measured = commit.join().unwrap();
    assert!((measured - (2.0 / 6.0 + TERM)).abs() < 1e-9, "measured in the faces the scan found and not in the bundled ones: {measured}");
    assert!(fonts.is_ready());
    set_measure_fonts(None);
}

#[test]
fn typing_works_text_out_in_the_faces_that_were_shared() {
    let _only = SETTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(measure_fonts().is_none(), "nothing is shared until an application shares its renderer's faces");
    // Two letters in a face of one em an advance, and in one of half an em: 12 points an em, so a third of an inch and a sixth.
    set_measure_fonts(Some(em(1000)));
    let mut wide = doc();
    set_text(&mut wide, 1, 1, "ab").unwrap();
    assert!((width(&wide) - (2.0 / 6.0 + TERM)).abs() < 1e-9, "{}", width(&wide));
    set_measure_fonts(Some(em(500)));
    let mut narrow = doc();
    set_text(&mut narrow, 1, 1, "ab").unwrap();
    assert!((width(&narrow) - (1.0 / 6.0 + TERM)).abs() < 1e-9, "{}", width(&narrow));
    // Taken back, the commits measure in the bundled faces, in which there is no `Em`: another width again, and the same on every machine.
    set_measure_fonts(None);
    let mut bundled = doc();
    set_text(&mut bundled, 1, 1, "ab").unwrap();
    let alone = width(&bundled);
    assert!((alone - 2.0 / 6.0).abs() > 1e-3 && (alone - 1.0 / 6.0).abs() > 1e-3, "{alone}");
    assert!(measure_fonts().is_none());
}

/// `shape.formatting` names the face a run with no face is drawn in from the database the application shares with the engine, and a read in the
/// window's frame never waits for the scan that is finding it: while the scan goes the bundled faces name it, and when it has finished the machine's do.
#[test]
fn the_default_face_the_query_names_is_in_the_shared_database_and_the_query_never_waits_for_the_scan() {
    use diacraft_engine::formatting::look_of;
    let _only = SETTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let unnamed = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text("a"))).build();
    let said = |doc: &Document| look_of(doc, 1, &[1]).unwrap().remove(0).font;
    let bundled = FontDb::bundled().default_family();
    assert!(bundled.is_some() && bundled.as_deref() != Some("Em"));
    assert_eq!(said(&unnamed), bundled, "nothing is shared: the bundled faces");
    // The scan is held back: the query goes on at once, with the bundled faces.
    let (release, gate) = channel::<()>();
    let fonts = scan_fonts_with(move || {
        let _ = gate.recv();
        em_db(1000)
    });
    let (tx, rx) = channel();
    let doc = unnamed.clone();
    std::thread::spawn(move || {
        let _ = tx.send(look_of(&doc, 1, &[1]).unwrap().remove(0).font);
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(bundled.clone()), "the query does not wait for the scan");
    assert!(!fonts.is_ready(), "and the scan is still held back");
    // The scan ends: the faces it found name the default, as they are the faces the renderer draws in.
    release.send(()).unwrap();
    fonts.get();
    assert_eq!(said(&unnamed).as_deref(), Some("Em"));
    // A database set by hand does the same, and taken back the bundled faces name it again.
    set_measure_fonts(Some(Arc::new(FontDb::empty())));
    assert_eq!(said(&unnamed), None, "a database with no face has no name to give");
    set_measure_fonts(None);
    assert_eq!(said(&unnamed), bundled);
}
