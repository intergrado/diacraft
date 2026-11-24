//! `Note`: everything a layout did that the caller may want to say. Task 8 and the oracle rely on it being complete, so each way a
//! text can come out other than as written has a note and a test here.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, lay_out};

use crate::{cf, db, em_db, glyph_count, lay_text, left, run};

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn notes_of(text: &str) -> Vec<Note> {
    lay_text(&em_db(), text, &tenth(), &left(), &top_block(), Some(50.0)).notes
}

#[test]
fn letters_no_face_has_are_counted_and_a_text_every_face_draws_says_nothing() {
    assert_eq!(notes_of("ab"), Vec::<Note>::new());
    // The face has ASCII only, and nothing else in the database does: two letters draw its missing-glyph box.
    assert_eq!(notes_of("ab中文"), vec![Note::MissingGlyphs { clusters: 2 }]);
    // A letter and its marks are one cluster, counted once.
    assert_eq!(notes_of("a中\u{0301}\u{0302}"), vec![Note::MissingGlyphs { clusters: 1 }]);
}

#[test]
fn a_letter_with_one_glyph_missing_among_several_is_counted() {
    // The face has `a` and not the acute: the cluster draws the letter and a box, and is one cluster with a glyph missing.
    let d = db(&[SynthFont::new("Em").ascii(1000)]);
    let laid = lay_text(&d, "a\u{0301}b", &tenth(), &left(), &top_block(), Some(50.0));
    assert_eq!(laid.notes, vec![Note::MissingGlyphs { clusters: 1 }]);
}

#[test]
fn two_thousand_uncovered_letters_are_one_note_with_their_count() {
    let text: String = (0..2048u32).filter_map(|i| char::from_u32(0x4E00 + i)).collect();
    assert_eq!(notes_of(&text), vec![Note::MissingGlyphs { clusters: 2048 }]);
}

#[test]
fn a_face_with_no_glyphs_at_all_is_reported_as_a_face_that_failed() {
    let laid = lay_text(&FontDb::empty(), "ab", &tenth(), &left(), &top_block(), Some(5.0));
    assert!(laid.notes.contains(&Note::FaceFailed { family: "(no font)".to_owned() }), "{:?}", laid.notes);
    assert!(laid.notes.contains(&Note::MissingGlyphs { clusters: 2 }), "{:?}", laid.notes);
}

/// Run `f`, counting the panics it makes on this thread without printing them; other threads' panics still reach the hook that was
/// there, which is handed back before this returns.
fn count_panics<T>(f: impl FnOnce() -> T) -> (T, usize) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    // The panic hook is the process's, and tests run in threads: one window at a time, or one test's hook would be taken by another.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _window = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let counted = Arc::new(AtomicUsize::new(0));
    let (mine, count) = (std::thread::current().id(), counted.clone());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == mine {
            count.fetch_add(1, Ordering::SeqCst);
        } else {
            previous(info);
        }
    }));
    let out = f();
    let _ = std::panic::take_hook();
    (out, counted.load(Ordering::SeqCst))
}

/// A database with a synthetic face in it that a read of its tables has made panic, so that it is set aside. The panic is injected
/// where a damaged font's would be raised (`Face::guarded_read`): the reader in use, `read-fonts` 0.46, has none that a damaged font
/// provokes, where 0.41.0 had one, which these tests used to search for.
fn db_with_a_set_aside_face() -> FontDb {
    let mut d = FontDb::empty();
    assert_eq!(d.add_font_data(SynthFont::new("Panics").ascii(600).build()).added, 1);
    let face = d.pick("Panics", Default::default());
    assert!(!face.is_broken(), "a face is whole until it fails");
    let (read, panics) = count_panics(|| face.guarded_read::<()>(|| panic!("the reader met a table it could not take")));
    assert_eq!((read, panics), (None, 1));
    assert!(face.is_broken(), "a face that panicked is set aside");
    d
}

#[test]
fn a_face_the_reader_panicked_on_is_reported_once_however_often_it_is_used() {
    let d = db_with_a_set_aside_face();
    let c = CharFormat { size: 0.1, ..cf("Panics", 0.0) };
    let runs = [run("abc ", &c, &left()), run("def", &c, &left())];
    let laid = lay_out(&runs, &top_block(), Some(5.0), &d);
    let failed: Vec<&Note> = laid.notes.iter().filter(|n| matches!(n, Note::FaceFailed { .. })).collect();
    assert_eq!(failed, vec![&Note::FaceFailed { family: "Panics".to_owned() }]);
    assert!(glyph_count(&laid.lines[0]) > 0, "the text is still laid out");
}

#[test]
fn a_face_that_was_set_aside_is_reported_even_when_nothing_is_shaped_with_it() {
    // Text of no size is not shaped, so the face is never asked to shape: it is reported because the layout met it and it is broken.
    let d = db_with_a_set_aside_face();
    let c = CharFormat { size: 0.0, ..cf("Panics", 0.0) };
    let laid = lay_out(&[run("abc", &c, &left())], &top_block(), Some(5.0), &d);
    assert!(laid.notes.contains(&Note::FaceFailed { family: "Panics".to_owned() }), "{:?}", laid.notes);
}

#[test]
fn a_width_that_was_given_and_is_not_a_number_is_a_bad_number() {
    let at = |width: Option<f64>| lay_text(&em_db(), "ab", &tenth(), &left(), &top_block(), width).notes;
    assert!(at(Some(f64::NAN)).contains(&Note::BadNumber));
    assert!(at(Some(f64::INFINITY)).contains(&Note::BadNumber));
    assert!(at(Some(f64::NEG_INFINITY)).contains(&Note::BadNumber));
    assert!(!at(None).contains(&Note::BadNumber), "no width is not a bad one");
    assert!(!at(Some(2.0)).contains(&Note::BadNumber));
    assert!(!at(Some(-2.0)).contains(&Note::BadNumber), "a negative width is a number");
}

#[test]
fn stand_in_notes_stop_at_256_and_the_names_left_out_are_counted() {
    let d = em_db();
    let runs: Vec<_> = (0..300).map(|i| run("a", &CharFormat { size: 0.1, ..cf(&format!("Missing Face {i}"), 0.0) }, &left())).collect();
    let laid = lay_out(&runs, &top_block(), Some(50.0), &d);
    let stand_ins = laid.notes.iter().filter(|n| matches!(n, Note::StandIn { .. })).count();
    assert_eq!(stand_ins, 256);
    assert!(laid.notes.contains(&Note::StandInsOmitted { count: 44 }), "{:?}", laid.notes);
    // Exactly at the cap, nothing is left out.
    let runs: Vec<_> = (0..256).map(|i| run("a", &CharFormat { size: 0.1, ..cf(&format!("Missing Face {i}"), 0.0) }, &left())).collect();
    let laid = lay_out(&runs, &top_block(), Some(50.0), &d);
    assert!(!laid.notes.iter().any(|n| matches!(n, Note::StandInsOmitted { .. })));
}

fn glyphs_in(laid: &Laid) -> usize {
    laid.lines.iter().map(glyph_count).sum()
}

#[test]
fn a_mark_on_a_space_is_drawn_wherever_the_line_ends() {
    // `aaaa`, a space with an acute on it, `bbbb`: a line that broke at that space once dropped the pair as trailing space.
    let d = db(&[SynthFont::new("Em").ascii(1000).chars("\u{0301}", 0)]);
    let text = "aaaa \u{0301}bbbb";
    for width in [0.3, 0.45, 0.5, 0.6, 0.85, 1.2, 5.0] {
        let laid = lay_text(&d, text, &tenth(), &left(), &top_block(), Some(width));
        assert_eq!(glyphs_in(&laid), 10, "width {width}");
    }
}

#[test]
fn a_search_for_a_face_that_could_not_look_everywhere_says_so() {
    // The database may read one byte of `cmap` tables: the Han face in the folder can't be looked at, so the letter draws a box.
    let dir = std::env::temp_dir().join(format!("diacraft-text-cut-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("han.ttf"), SynthFont::new("Han").chars("中", 1000).build()).unwrap();
    let mut d = FontDb::with_font_dirs(std::slice::from_ref(&dir)).with_byte_limits(1 << 30, 1);
    d.add_font_data(SynthFont::new("Em").ascii(1000).build());
    let laid = lay_text(&d, "a中", &tenth(), &left(), &top_block(), Some(5.0));
    assert!(laid.notes.contains(&Note::FaceSearchCut), "{:?}", laid.notes);
    assert!(laid.notes.contains(&Note::MissingGlyphs { clusters: 1 }), "{:?}", laid.notes);
    let _ = std::fs::remove_dir_all(&dir);
    // With the bytes to read it, the letter finds its face and there is nothing to say.
    let none: Vec<Note> = Vec::new();
    let mut ok = FontDb::empty();
    ok.add_font_data(SynthFont::new("Em").ascii(1000).build());
    ok.add_font_data(SynthFont::new("Han").chars("中", 1000).build());
    assert_eq!(lay_text(&ok, "a中", &tenth(), &left(), &top_block(), Some(5.0)).notes, none);
}
