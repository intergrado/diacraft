//! Which face draws what,
//! and what happens when a name isn't on the machine.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, lay_out};

use crate::{cf, db, lay_text, left, run};

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn faces_of(laid: &Laid) -> Vec<String> {
    laid.lines.iter().flat_map(|l| l.runs.iter().map(|r| r.face.family().to_owned())).collect()
}

fn lay(db: &FontDb, text: &str, c: &CharFormat) -> Laid {
    lay_text(db, text, c, &left(), &block(), None)
}

#[test]
fn face_each_letter_reads_the_face_of_its_script() {
    let latin = SynthFont::new("Latin Face").ascii(500).chars("中", 500);
    let asian = SynthFont::new("Asian Face").chars("中文", 1000);
    let complex = SynthFont::new("Complex Face").chars("بج", 700);
    let db = db(&[latin, asian, complex]);
    let c = CharFormat { asian_font: "Asian Face".into(), complex_font: "Complex Face".into(), ..cf("Latin Face", 12.0) };
    // A and U+4E2D: two letters in two faces. One face for the run fails.
    assert_eq!(faces_of(&lay(&db, "A中", &c)), vec!["Latin Face", "Asian Face"]);
    // AsianFont 0: both in Latin Face (which, here, has the Han letter).
    let zero = CharFormat { asian_font: "0".into(), ..c.clone() };
    assert_eq!(faces_of(&lay(&db, "A中", &zero)), vec!["Latin Face"]);
    let empty = CharFormat { asian_font: String::new(), ..c.clone() };
    assert_eq!(faces_of(&lay(&db, "A中", &empty)), vec!["Latin Face"]);
    // U+0628 with ComplexScriptFont 0: Font.
    let latin_has_beh = SynthFont::new("Latin Face").ascii(500).chars("ب", 500);
    let db2 = crate::db(&[latin_has_beh]);
    let c2 = CharFormat { complex_font: "0".into(), ..cf("Latin Face", 12.0) };
    assert_eq!(faces_of(&lay(&db2, "ب", &c2)), vec!["Latin Face"]);
}

#[test]
fn digits_punctuation_and_spaces_stay_in_font_among_asian_letters() {
    // "中2、中": the digit and the Common punctuation read Font, not the letters' face.
    let latin = SynthFont::new("Latin Face").ascii(500).chars("、", 500);
    let asian = SynthFont::new("Asian Face").chars("中2、", 1000);
    let db = db(&[latin, asian]);
    let c = CharFormat { asian_font: "Asian Face".into(), ..cf("Latin Face", 12.0) };
    let laid = lay(&db, "中2、中", &c);
    assert_eq!(faces_of(&laid), vec!["Asian Face", "Latin Face", "Asian Face"]);
}

#[test]
fn asianfont_a_face_without_the_letter_hands_it_to_font_not_a_box() {
    let latin = SynthFont::new("Latin Face").ascii(500).chars("中", 500);
    let asian = SynthFont::new("Asian Face").ascii(500);
    let db = db(&[latin, asian]);
    let c = CharFormat { asian_font: "Asian Face".into(), ..cf("Latin Face", 12.0) };
    let laid = lay(&db, "中", &c);
    assert_eq!(faces_of(&laid), vec!["Latin Face"]);
    assert_ne!(laid.lines[0].runs[0].glyphs[0].gid, 0, "a real glyph and not .notdef");
}

#[test]
fn complexscriptfont_when_font_lacks_the_letter_a_face_that_has_it_is_found() {
    let font = SynthFont::new("Latin Face").ascii(500);
    let other = SynthFont::new("Somewhere Else").chars("ب", 600);
    let db = db(&[font, other]);
    let c = CharFormat { complex_font: "0".into(), ..cf("Latin Face", 12.0) };
    let laid = lay(&db, "ب", &c);
    assert_eq!(faces_of(&laid), vec!["Somewhere Else"]);
    assert_ne!(laid.lines[0].runs[0].glyphs[0].gid, 0);
}

#[test]
fn a_letter_no_face_has_is_drawn_as_notdef_and_never_dropped() {
    let db = db(&[SynthFont::new("Latin Face").ascii(500)]);
    let laid = lay(&db, "a\u{10FFFD}b", &cf("Latin Face", 12.0));
    let gids = crate::gids(&laid.lines[0]);
    assert_eq!(gids.len(), 3);
    assert_eq!(gids[1], 0);
}

#[test]
fn a_cluster_is_never_split_across_faces() {
    // e plus a combining acute: the base face has the letter and not the mark, another has both: the whole cluster goes there.
    let base = SynthFont::new("Base Face").ascii(500);
    let both = SynthFont::new("Both Face").chars("e\u{0301}", 500);
    let db = db(&[base, both]);
    let laid = lay(&db, "e\u{0301}x", &cf("Base Face", 12.0));
    let faces = faces_of(&laid);
    assert_eq!(faces, vec!["Both Face", "Base Face"]);
}

#[test]
fn face_a_missing_name_draws_in_its_stand_in_and_is_noted_once() {
    let db = db(&[SynthFont::new("Carlito").ascii(500), SynthFont::new("Liberation Sans").ascii(500)]);
    let calibri = cf("Calibri", 12.0);
    let arial = cf("Arial", 12.0);
    let laid = lay_out(&[run("ab ", &calibri, &left()), run("cd ", &calibri, &left()), run("ef", &arial, &left())], &block(), None, &db);
    assert_eq!(faces_of(&laid)[0], "Carlito");
    let notes: Vec<&Note> = laid.notes.iter().filter(|n| matches!(n, Note::StandIn { .. })).collect();
    assert_eq!(notes.len(), 2, "one per missing name: {notes:?}");
    assert!(notes.contains(&&Note::StandIn { requested: "Calibri".into(), used: "Carlito".into() }));
    assert!(notes.contains(&&Note::StandIn { requested: "Arial".into(), used: "Liberation Sans".into() }));
    // A name that is on the machine, in other capitals, is found and not noted.
    let laid = lay(&db, "ab", &cf("CARLITO", 12.0));
    assert!(laid.notes.is_empty(), "{:?}", laid.notes);
}

#[test]
fn text_still_draws_at_a_sensible_size_with_no_font_installed_and_no_craft_fonts() {
    // Review Focus 3: a drawing names a font that isn't installed, and there is nothing but the bundled faces.
    let db = FontDb::bundled();
    let c = cf("Segoe UI", 12.0);
    let laid = lay(&db, "Hello", &c);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(laid.lines[0].runs[0].glyphs.len(), 5);
    assert!((laid.lines[0].runs[0].size - 12.0 / 72.0).abs() < 1e-12, "the size asked for is the size drawn");
    // Wider than nothing and narrower than the whole page: a sensible width for five letters at 12 points.
    assert!(laid.size.width > 0.2 && laid.size.width < 0.7, "{}", laid.size.width);
    assert!(laid.lines[0].runs[0].glyphs.iter().all(|g| g.gid != 0), "no letter is a missing-glyph box");
    assert!(laid.notes.iter().any(|n| matches!(n, Note::StandIn { requested, .. } if requested == "Segoe UI")));
}

#[test]
fn an_empty_database_still_lays_text_out_with_the_face_that_has_no_glyphs() {
    let laid = lay(&FontDb::empty(), "Hello", &cf("Anything", 12.0));
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(laid.lines[0].runs.iter().map(|r| r.glyphs.len()).sum::<usize>(), 5);
    // Half an em a letter.
    assert!((laid.size.width - 5.0 * 6.0 / 72.0).abs() < 1e-9);
}

/// The names are cut and the text draws: that is the claim. The bound of a minute is a hang guard and no measure of cost (each layout takes
/// milliseconds), generous for a machine at load 50.
#[test]
fn a_font_name_that_is_hostile_is_cut_and_the_text_still_draws() {
    let db = FontDb::bundled();
    for name in ["A".repeat(5 << 20), "../../../etc/passwd".to_owned(), "\0\u{202E}".repeat(1000)] {
        let c = CharFormat { font: name, asian_font: "B".repeat(1 << 20), complex_font: "C".repeat(1 << 20), ..CharFormat::default() };
        let start = std::time::Instant::now();
        let laid = lay(&db, "Hello 中 ب", &c);
        assert_eq!(laid.lines.len(), 1);
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "took {:?}", start.elapsed());
    }
}
