//! Finding a face that has a glyph the named faces lack.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;

use crate::{REGULAR, only};

#[test]
fn a_face_with_the_glyph_is_found_among_the_added_ones() {
    let db = only(&[SynthFont::new("Latin Only").ascii(500), SynthFont::new("Has Han").ascii(500).chars("中文", 1000)]);
    let face = db.covering(&['中']).expect("the face with the glyph");
    assert_eq!(face.family(), "Has Han");
    assert!(db.covering(&['\u{10FFFD}']).is_none(), "no face has it");
}

#[test]
fn a_cluster_needs_every_character_covered() {
    let db = only(&[SynthFont::new("Base Only").chars("e", 500), SynthFont::new("Both").chars("e\u{0301}", 500)]);
    // The first face covers the letter and not the mark: the cluster goes to the one that has both.
    let face = db.covering(&['e', '\u{0301}']).expect("a face for the whole cluster");
    assert_eq!(face.family(), "Both");
}

#[test]
fn spaces_and_joiners_never_decide_the_face() {
    let db = only(&[SynthFont::new("Has A").chars("a", 500)]);
    assert_eq!(db.covering(&['a', '\u{200D}', ' ']).map(|f| f.family().to_owned()), Some("Has A".to_owned()));
    assert!(db.covering(&[' ', '\u{200B}']).is_none(), "nothing to cover");
    assert!(db.covering(&[]).is_none());
}

#[test]
fn the_answer_is_remembered_and_stays_the_same() {
    let db = only(&[SynthFont::new("Has Han").ascii(500).chars("中", 1000)]);
    let a = db.covering(&['中']).expect("found");
    let b = db.covering(&['中']).expect("found again");
    assert_eq!(a.id(), b.id());
    assert!(db.covering(&['文']).is_none());
    assert!(db.covering(&['文']).is_none(), "a miss is remembered too");
}

#[test]
fn faces_added_later_are_searched() {
    let mut db = FontDb::empty();
    assert!(db.covering(&['中']).is_none());
    db.add_font_data(SynthFont::new("Late Han").chars("中", 1000).build());
    assert_eq!(db.covering(&['中']).map(|f| f.family().to_owned()), Some("Late Han".to_owned()), "adding a face forgets old misses");
}

#[test]
fn the_bundled_faces_cover_latin_greek_and_cyrillic_but_not_han() {
    let db = FontDb::bundled();
    for c in ['a', 'é', 'λ', 'ж'] {
        assert!(db.covering(&[c]).is_some(), "{c}");
    }
    if diacraft_fonts::craft_font_count() == 0 {
        assert!(db.covering(&['中']).is_none(), "no bundled face has Han, and the machine's aren't read here");
    }
}

#[test]
fn a_face_covers_what_its_cmap_maps_on_both_sides_of_the_end_of_the_basic_plane() {
    let db = only(&[SynthFont::new("Edges").chars("\u{0FFF}\u{1000}\u{FFFF}\u{10000}\u{10001}", 500)]);
    let face = db.pick("Edges", REGULAR);
    for c in ['\u{0FFF}', '\u{1000}', '\u{FFFF}', '\u{10000}', '\u{10001}'] {
        assert!(face.covers(c), "U+{:04X} is mapped", c as u32);
    }
    for c in ['\u{0FFE}', '\u{1001}', '\u{FFFE}', '\u{10002}'] {
        assert!(!face.covers(c), "U+{:04X} is not", c as u32);
    }
}
